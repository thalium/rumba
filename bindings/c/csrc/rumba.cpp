#include "rumba/rumba.hpp"
#include <cstddef>
#include <cstdint>
#include <vector>

extern "C" {
#include "rumba/rumba.h"
}

auto rumba::ExpressionRef::eval(std::vector<uint64_t> &vars, uint8_t n) const
    -> uint64_t {
  return rumba_expr_eval(ptr_, n, vars.data(), vars.size());
}

auto rumba::ExpressionRef::size() const -> size_t {
  return rumba_expr_size(ptr_);
}

auto rumba::ExpressionRef::to_string(uint8_t n, bool hex, bool latex) const
    -> std::string {
  uint8_t flags = 0;
  flags |= hex ? RUMBA_EXPR_REPR_FLAG_HEX : 0;
  flags |= latex ? RUMBA_EXPR_REPR_FLAG_LATEX : 0;

  size_t len;
  auto *c_str = rumba_expr_repr(ptr_, n, &len, flags);

  return std::string(c_str, len);
}

auto rumba::ExpressionRef::operator->() const -> ExpressionRef const * {
  return this;
}

auto rumba::ExpressionRef::clone() const -> Expression {
  return Expression(rumba_expr_clone(ptr_));
}

auto rumba::ExpressionRef::make_vector() const
    -> std::vector<rumba::ExpressionRef> {
  const auto sz = rumba_expr_children_size(ptr_);
  auto v = std::vector<rumba::ExpressionRef>();
  v.reserve(sz);
  for (size_t i = 0; i < sz; ++i) {
    const auto *ptr = rumba_expr_get_child(ptr_, i);
    v.push_back(rumba::ExpressionRef(ptr));
  }

  return v;
}

auto rumba::ExpressionRef::data() const -> ExpressionData {
  const auto ty = rumba_expr_ty(ptr_);
  switch (ty) {
  case RUMBA_EXPR_TYPE_VAR:
    return {Var{.id = rumba_expr_get_var(ptr_)}};

  case RUMBA_EXPR_TYPE_CONST:
    return {Const{.value = rumba_expr_get_const(ptr_)}};

  case RUMBA_EXPR_TYPE_NOT:
    return {Not{.expr = ExpressionRef(rumba_expr_get_child(ptr_, 0))}};

  case RUMBA_EXPR_TYPE_SCALE:
    return {Scale{.scale = rumba_expr_get_const(ptr_),
                  .expr = ExpressionRef(rumba_expr_get_child(ptr_, 0))}};

  case RUMBA_EXPR_TYPE_AND: {
    return {And{.children = make_vector()}};
  }

  case RUMBA_EXPR_TYPE_OR: {
    return {Or{.children = make_vector()}};
  }

  case RUMBA_EXPR_TYPE_XOR: {
    return {Xor{.children = make_vector()}};
  }

  case RUMBA_EXPR_TYPE_ADD: {
    return {Add{.children = make_vector()}};
  }

  case RUMBA_EXPR_TYPE_MUL: {
    return {Mul{.children = make_vector()}};
  }
  }
}

rumba::Expression::Expression(Expression &&other) noexcept : ptr_(other.ptr_) {
  other.ptr_ = nullptr;
}

auto rumba::Expression::operator=(Expression &&other) noexcept -> Expression & {
  if (ptr_ != other.ptr_) {
    rumba_expr_free(ptr_);
    ptr_ = other.ptr_;
    other.ptr_ = nullptr;
  }
  return *this;
}

auto rumba::Expression::make_const(uint64_t c) -> Expression {
  return Expression(rumba_make_const(c));
}

auto rumba::Expression::make_var(size_t id) -> Expression {
  return Expression(rumba_make_var(id));
}

auto rumba::Expression::reduce(uint8_t n) -> Expression & {
  ptr_ = rumba_expr_reduce(ptr_, n);
  return *this;
}

auto rumba::Expression::simplify(uint8_t n) -> Expression & {
  ptr_ = rumba_expr_simplify(ptr_, n);
  return *this;
}

auto rumba::operator+(Expression lhs, Expression rhs) -> Expression {
  auto res = Expression(rumba_expr_add(lhs.ptr_, rhs.ptr_));
  lhs.ptr_ = nullptr;
  rhs.ptr_ = nullptr;
  return res;
}

auto rumba::operator-(Expression lhs, Expression rhs) -> Expression {
  auto res = Expression(rumba_expr_sub(lhs.ptr_, rhs.ptr_));
  lhs.ptr_ = nullptr;
  rhs.ptr_ = nullptr;
  return res;
}

auto rumba::operator*(Expression lhs, Expression rhs) -> Expression {
  auto res = Expression(rumba_expr_mul(lhs.ptr_, rhs.ptr_));
  lhs.ptr_ = nullptr;
  rhs.ptr_ = nullptr;
  return res;
}

auto rumba::operator|(Expression lhs, Expression rhs) -> Expression {
  auto res = Expression(rumba_expr_bor(lhs.ptr_, rhs.ptr_));
  lhs.ptr_ = nullptr;
  rhs.ptr_ = nullptr;
  return res;
}

auto rumba::operator^(Expression lhs, Expression rhs) -> Expression {
  auto res = Expression(rumba_expr_bxor(lhs.ptr_, rhs.ptr_));
  lhs.ptr_ = nullptr;
  rhs.ptr_ = nullptr;
  return res;
}

auto rumba::operator&(Expression lhs, Expression rhs) -> Expression {
  auto res = Expression(rumba_expr_band(lhs.ptr_, rhs.ptr_));
  lhs.ptr_ = nullptr;
  rhs.ptr_ = nullptr;
  return res;
}

auto rumba::operator~(Expression expr) -> Expression {
  auto res = Expression(rumba_expr_bnot(expr.ptr_));
  expr.ptr_ = nullptr;
  return res;
}

auto rumba::operator-(Expression expr) -> Expression {
  auto res = Expression(rumba_expr_neg(expr.ptr_));
  expr.ptr_ = nullptr;
  return res;
}

auto rumba::Expression::operator*() const -> ExpressionRef {
  return ExpressionRef(ptr_);
}

auto rumba::Expression::operator->() const -> ExpressionRef {
  return ExpressionRef(ptr_);
}

rumba::Expression ::~Expression() {
  if (ptr_ != nullptr) {
    rumba_expr_free(ptr_);
    ptr_ = nullptr;
  }
}

rumba::Program::Program() : ptr_(rumba_make_program()) {}

rumba::Program::~Program() {
  if (ptr_ != nullptr) {
    rumba_program_free(ptr_);
    ptr_ = nullptr;
  }
}

auto rumba::Program::size() -> size_t { return rumba_program_size(ptr_); }

auto rumba::Program::get(size_t i) -> Instruction {
  return Instruction(rumba_program_get(ptr_, i));
}

void rumba::Program::push_unknown(size_t id, uint8_t ty,
                                  std::vector<size_t> const &args) {
  auto res = rumba_program_push_unknown(ptr_, ty, id, args.data(), args.size());

  if (res != 0) {
    throw std::string(get_err_str());
  }
}

void rumba::Program::push_assign(size_t id, uint8_t ty, Expression &&e) {
  auto res = rumba_program_push_assign(ptr_, ty, id, e.ptr_);
  e.ptr_ = nullptr;

  if (res != 0) {
    throw std::string(get_err_str());
  }
}

void rumba::Program::simplify() {
  const auto res = rumba_program_simplify(ptr_);

  if (res != 0) {
    throw std::string(get_err_str());
  }
}

auto rumba::Instruction::kind() -> Kind {
  switch (rumba_insn_kind(ptr_)) {
  case INSN_KIND_ASSIGN:
    return Kind::Assign;

  case INSN_KIND_UNKNOWN:
    return Kind::Unknown;
  };
}

auto rumba::Instruction::ty() -> uint8_t { return rumba_insn_ty(ptr_); }

auto rumba::Instruction::id() -> size_t { return rumba_insn_id(ptr_); }

auto rumba::Instruction::expr() -> ExpressionRef {
  return ExpressionRef(rumba_insn_assign_get(ptr_));
}

auto rumba::Instruction::size() -> size_t {
  return rumba_insn_unknown_size(ptr_);
}

auto rumba::Instruction::get(size_t i) -> size_t {
  return rumba_insn_unknown_get(ptr_, i);
}

auto format_as(rumba::Expression const &e) -> std::string {
  return e->to_string();
}

auto format_as(rumba::ExpressionRef const &e) -> std::string {
  return e.to_string();
}