/// Hand crafted CPP wrapper for rumba
#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <variant>
#include <vector>

namespace rumba {

struct Expression;
struct Const;
struct Var;
struct Not;
struct Scale;
struct And;
struct Or;
struct Xor;
struct Add;
struct Mul;

using ExpressionData =
    std::variant<Var, Const, Not, Scale, And, Or, Xor, Add, Mul>;

class ExpressionRef {
public:
  /// @brief For chaining
  [[nodiscard]] auto operator->() const -> ExpressionRef const *;

  /// @brief Evaluates the expression at a given set of variables
  [[nodiscard]] auto eval(std::vector<uint64_t> &vars, uint8_t n) const
      -> uint64_t;

  /// @brief Returns the number of nodes in the expression
  [[nodiscard]] auto size() const -> size_t;

  /// @brief Converts the expression to a string
  [[nodiscard]] auto to_string(uint8_t n = 64, bool hex = false,
                               bool latex = false) const -> std::string;

  [[nodiscard]] auto data() const -> ExpressionData;

  [[nodiscard]] auto clone() const -> Expression;

  friend struct Expression;
  friend class Instruction;

private:
  /// @brief Creates a new expression from the provided pointer
  explicit ExpressionRef(void const *ptr) : ptr_(ptr) {}

  /// @brief Helper function
  [[nodiscard]] auto make_vector() const -> std::vector<rumba::ExpressionRef>;

  void const *ptr_;
};

struct Var {
  size_t id;
};

struct Const {
  uint64_t value;
};

struct Not {
  ExpressionRef expr;
};

struct Scale {
  uint64_t scale;
  ExpressionRef expr;
};

struct And {
  std::vector<ExpressionRef> children;
};

struct Or {
  std::vector<ExpressionRef> children;
};

struct Xor {
  std::vector<ExpressionRef> children;
};

struct Add {
  std::vector<ExpressionRef> children;
};

struct Mul {
  std::vector<ExpressionRef> children;
};

/// An owning expression
class Expression {
public:
  Expression(const Expression &) = delete;
  auto operator=(const Expression &) = delete;

  ~Expression();

  Expression(Expression &&other) noexcept;

  auto operator=(Expression &&other) noexcept -> Expression &;

  /// @brief Create a new const expression
  [[nodiscard]] static auto make_const(uint64_t c) -> Expression;

  /// @brief Create a new var expression
  [[nodiscard]] static auto make_var(size_t id) -> Expression;

  /// @brief Performs arithmentic reductions on this expression
  auto reduce(uint8_t n) -> Expression &;

  /// @brief Simplifies the expression
  auto simplify(uint8_t n) -> Expression &;

  auto operator*() const -> ExpressionRef;
  auto operator->() const -> ExpressionRef;

  auto friend operator-(Expression lhs, Expression rhs) -> Expression;

  auto friend operator+(Expression lhs, Expression rhs) -> Expression;

  auto friend operator*(Expression lhs, Expression rhs) -> Expression;

  auto friend operator|(Expression lhs, Expression rhs) -> Expression;

  auto friend operator^(Expression lhs, Expression rhs) -> Expression;

  auto friend operator&(Expression lhs, Expression rhs) -> Expression;

  auto friend operator~(Expression expr) -> Expression;

  auto friend operator-(Expression expr) -> Expression;

  friend struct ExpressionRef;
  friend struct Program;

private:
  /// @brief Creates a new expression from the provided pointer
  explicit Expression(void *ptr) : ptr_(ptr) {}

  void *ptr_;
};

/// A rumba instruction
class Instruction {
public:
  enum class Kind : uint8_t { Assign, Unknown };

  /// @brief Gets the kind of this instruction
  [[nodiscard]] auto kind() -> Kind;

  /// @brief Gets the type of this instruction
  [[nodiscard]] auto ty() -> uint8_t;

  /// @brief Gets the id of this instruction
  [[nodiscard]] auto id() -> size_t;

  /// @brief Gets the expression of this instruction
  /// Should only be called if this is an assign instruction
  [[nodiscard]] auto expr() -> ExpressionRef;

  /// @brief Gets the number of arguments of this expression
  /// Should only be called if this is an unknown instruction
  [[nodiscard]] auto size() -> size_t;

  /// @brief Gets the i-th argument of this expression
  /// Should only be called if this is an unknown instruction
  [[nodiscard]] auto get(size_t i) -> size_t;

  friend class Program;

private:
  explicit Instruction(void const *ptr) : ptr_(ptr) {}
  void const *ptr_;
};

/// @brief A rumba program
class Program {
public:
  /// @brief Creates a new program
  Program();

  ~Program();

  /// @brief Returns the number of instructions in the program
  [[nodiscard]] auto size() -> size_t;

  /// @brief Returns the i-th instruction in the program
  [[nodiscard]] auto get(size_t i) -> Instruction;

  /// @brief Simplifies the program
  void simplify();

  /// @brief Adds an unknown instruction at the end of the program
  void push_unknown(size_t id, uint8_t ty, std::vector<size_t> const &args);

  /// @brief Adds an assign instruction at the end of the program
  void push_assign(size_t id, uint8_t ty, Expression &&e);

private:
  void *ptr_;
};

} // namespace rumba

[[nodiscard]] auto format_as(rumba::Expression const &) -> std::string;
[[nodiscard]] auto format_as(rumba::ExpressionRef const &) -> std::string;