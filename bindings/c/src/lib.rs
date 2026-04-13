use std::cell::RefCell;
use std::ffi::{CString, c_char, c_void};
use std::ptr::null;
use std::slice;

use rumba_core::expr::Expr;
use rumba_core::lang::{self, Insn, Program};
use rumba_core::simplify::simplify_mba;
use rumba_core::varint::make_mask;

#[cfg(feature = "parse")]
use rumba_core::parser::parse_expr;

#[cfg(feature = "parse")]
use std::ffi::CStr;

use crate::pool::Pool;

mod pool;

pub const RUMBA_EXPR_REPR_FLAG_LATEX: u8 = 1;
pub const RUMBA_EXPR_REPR_FLAG_HEX: u8 = 2;

thread_local! {
    static EXPR_POOL: RefCell<Pool<Expr>> = RefCell::new(Pool::new(1));
    static PROGRAM_POOL: RefCell<Pool<Program>> = RefCell::new(Pool::new(1));

    static LAST_ERROR: RefCell<Option<CString>> = const{ RefCell::new(None) };
}

#[allow(dead_code)]
fn set_last_error(err: &str) {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = Some(CString::new(err).unwrap());
    });
}

#[repr(C)]
/// The type of an expression
pub enum ExprTy {
    /// A variable
    Var,

    /// A constant
    Const,

    /// A bitwise not
    Not,

    /// A multiplication between a constant and an expression
    Scale,

    /// A bitwise and
    And,

    /// A bitwise or
    Or,

    /// A bitwise xor
    Xor,

    /// A sum
    Add,

    /// A product
    Mul,
}

unsafe fn to_ref<T>(ptr: *const c_void) -> &'static T {
    assert!(!ptr.is_null());
    unsafe { &*(ptr as *const T) }
}

unsafe fn to_mut_ref<T>(ptr: *mut c_void) -> &'static mut T {
    assert!(!ptr.is_null());
    unsafe { &mut *(ptr as *mut T) }
}

/// Returns a valid pointer in the POOL
fn make_expr_ptr(e: Expr) -> *mut c_void {
    EXPR_POOL.with_borrow_mut(|pool| pool.alloc(e))
}

fn make_program_ptr(p: Program) -> *mut c_void {
    PROGRAM_POOL.with_borrow_mut(|pool| pool.alloc(p))
}

fn to_ptr<T>(r: &T) -> *const c_void {
    r as *const T as *const c_void
}

fn to_mut_ptr<T>(r: &mut T) -> *mut c_void {
    r as *mut T as *mut c_void
}

unsafe fn vec_from_ptr<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        return &[];
    }

    assert!(!ptr.is_null());
    unsafe { slice::from_raw_parts(ptr, len) }
}

/// Takes ownership of a pointer from the POOL
/// / Safety: caller promises ptr is valid and from POOL
unsafe fn take_ownership(ptr: *mut c_void) -> Expr {
    EXPR_POOL.with_borrow_mut(|pool|
        // Safety: the user promises that ptr is a valid pointer from the pool
        unsafe { pool.take(ptr) })
}

/// Frees the expression `ptr`
///
/// # Safety
/// Caller promises ptr is valid and from POOL
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_free(ptr: *mut c_void) {
    EXPR_POOL.with_borrow_mut(|pool| unsafe {
        pool.free(ptr);
    })
}

/// Performs arithmentic reductions on `ptr` modulo 2^`n`
/// This function takes frees `ptr` and returns a new pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_reduce(ptr: *mut c_void, n: u8) -> *mut c_void {
    let expr = unsafe { take_ownership(ptr) };
    make_expr_ptr(expr.reduce(make_mask(n)))
}

/// Simplifies the MBA in `ptr` modulo 2^`n`
/// This function takes frees `ptr` and returns a new pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_simplify(ptr: *mut c_void, n: u8) -> *mut c_void {
    let expr = unsafe { take_ownership(ptr) };
    make_expr_ptr(simplify_mba(expr, n))
}

/// Evaluates the epxression `ptr` modulo 2^`n`
/// on the variables in the array `arr` of length `len`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_eval(
    ptr: *const c_void,
    n: u8,
    arr: *const u64,
    len: usize,
) -> u64 {
    let expr = unsafe { to_ref::<Expr>(ptr) };
    let vars = unsafe { vec_from_ptr(arr, len) };
    expr.eval(vars).get(make_mask(n))
}

/// Counts the number of nodes in the expression `ptr`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_size(ptr: *const c_void) -> usize {
    let expr = unsafe { to_ref::<Expr>(ptr) };
    expr.size()
}

/// Returns a string representation of `ptr` modulo 2^`n`
/// `len` is the length of the returned string
/// `flags` RUMBA_EXPR_REPR_FLAG
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_repr(
    ptr: *const c_void,
    n: u8,
    len: *mut usize,
    flags: u8,
) -> *mut c_char {
    let expr = unsafe { to_ref::<Expr>(ptr) };
    let s = expr.repr(
        n,
        make_mask(n),
        (flags & RUMBA_EXPR_REPR_FLAG_HEX) != 0,
        (flags & RUMBA_EXPR_REPR_FLAG_LATEX) != 0,
    );

    if !len.is_null() {
        unsafe {
            *len = s.len();
        }
    }

    let c_string = CString::new(s).unwrap();
    c_string.into_raw()
}

/// Gets the type of the expression `ptr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_ty(ptr: *const c_void) -> ExprTy {
    let expr = unsafe { to_ref::<Expr>(ptr) };
    match expr {
        Expr::Var(_) => ExprTy::Var,
        Expr::Const(_) => ExprTy::Const,
        Expr::Not(_) => ExprTy::Not,
        Expr::Scale(_, _) => ExprTy::Scale,
        Expr::And(_) => ExprTy::And,
        Expr::Or(_) => ExprTy::Or,
        Expr::Xor(_) => ExprTy::Xor,
        Expr::Add(_) => ExprTy::Add,
        Expr::Mul(_) => ExprTy::Mul,
    }
}

/// Gets the number of children of this node.
/// This should only be called on expressions of types
/// `And, Or, Xor, Mul, Add, Scale, Not`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_children_size(ptr: *const c_void) -> usize {
    let expr = unsafe { to_ref::<Expr>(ptr) };

    match expr {
        Expr::And(exprs)
        | Expr::Or(exprs)
        | Expr::Xor(exprs)
        | Expr::Add(exprs)
        | Expr::Mul(exprs) => exprs.len(),

        Expr::Scale(_, _) | Expr::Not(_) => 1,

        _ => panic!("Expression does not have children"),
    }
}

/// Gets the `idx`-th child of this node.
/// This should only be called on expressions of types
/// `And, Or, Xor, Mul, Add, Scale, Not`.
/// `idx` should be less than `rumba_expr_children_size`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_get_child(ptr: *const c_void, idx: usize) -> *const c_void {
    let expr = unsafe { to_ref::<Expr>(ptr) };

    match expr {
        Expr::And(exprs)
        | Expr::Or(exprs)
        | Expr::Xor(exprs)
        | Expr::Add(exprs)
        | Expr::Mul(exprs) => {
            if idx >= exprs.len() {
                panic!("Index out of bounds");
            }
            to_ptr(&exprs[idx])
        }

        Expr::Scale(_, e) | Expr::Not(e) => {
            if idx != 0 {
                panic!("Index out of bounds");
            }
            to_ptr(e.as_ref())
        }

        _ => panic!("Expression does not have children"),
    }
}

/// Mutably gets the `idx`-th child of this node.
/// This should only be called on expressions of types
/// `And, Or, Xor, Mul, Add, Scale, Not`.
/// `idx` should be less than `rumba_expr_children_size`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_get_child_mut(ptr: *mut c_void, idx: usize) -> *mut c_void {
    let expr = unsafe { to_mut_ref::<Expr>(ptr) };

    match expr {
        Expr::And(exprs)
        | Expr::Or(exprs)
        | Expr::Xor(exprs)
        | Expr::Add(exprs)
        | Expr::Mul(exprs) => {
            if idx >= exprs.len() {
                panic!("Index out of bounds");
            }
            to_mut_ptr(&mut exprs[idx])
        }

        Expr::Scale(_, e) | Expr::Not(e) => {
            if idx != 0 {
                panic!("Index out of bounds");
            }
            to_mut_ptr(e.as_mut())
        }

        _ => panic!("Expression does not have children"),
    }
}

/// Gets the constant of this node.
/// This should only be called on expressions of type `Const`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_get_const(ptr: *const c_void) -> u64 {
    let expr = unsafe { to_ref::<Expr>(ptr) };

    match expr {
        Expr::Scale(c, _) | Expr::Const(c) => **c,
        _ => panic!("Expression is not a CONST"),
    }
}

/// Gets the variable id of this node.
/// This should only be called on expressions of type `Var`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_get_var(ptr: *const c_void) -> usize {
    let expr = unsafe { to_ref::<Expr>(ptr) };

    match expr {
        Expr::Var(id) => id.0,
        _ => panic!("Expression is not a VAR"),
    }
}

/// Create a new CONSTANT expression
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_make_const(c: u64) -> *mut c_void {
    make_expr_ptr(Expr::Const(c.into()))
}

/// Create a new VARIABLE expression
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_make_var(id: usize) -> *mut c_void {
    make_expr_ptr(Expr::Var(id.into()))
}

/// Create a new NOT expression.
/// `ptr` is freed and a new expression in allocated
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_bnot(ptr: *mut c_void) -> *mut c_void {
    let v = unsafe { take_ownership(ptr) };
    make_expr_ptr(!v)
}

/// Create a new AND expression.
/// `lhs` and `rhs` are freed and a new expression in allocated
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_band(lhs: *mut c_void, rhs: *mut c_void) -> *mut c_void {
    let lhs = unsafe { take_ownership(lhs) };
    let rhs = unsafe { take_ownership(rhs) };

    make_expr_ptr(lhs & rhs)
}

/// Create a new XOR expression.
/// `lhs` and `rhs` are freed and a new expression in allocated
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_bxor(lhs: *mut c_void, rhs: *mut c_void) -> *mut c_void {
    let lhs = unsafe { take_ownership(lhs) };
    let rhs = unsafe { take_ownership(rhs) };

    make_expr_ptr(lhs ^ rhs)
}

/// Create a new OR expression.
/// `lhs` and `rhs` are freed and a new expression in allocated
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_bor(lhs: *mut c_void, rhs: *mut c_void) -> *mut c_void {
    let lhs = unsafe { take_ownership(lhs) };
    let rhs = unsafe { take_ownership(rhs) };

    make_expr_ptr(lhs | rhs)
}

/// Create a new ADD expression.
/// `ptr` is freed and a new expression in allocated
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_add(lhs: *mut c_void, rhs: *mut c_void) -> *mut c_void {
    let lhs = unsafe { take_ownership(lhs) };
    let rhs = unsafe { take_ownership(rhs) };

    make_expr_ptr(lhs + rhs)
}

/// Create a new MUL expression.
/// `ptr` is freed and a new expression in allocated
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_mul(lhs: *mut c_void, rhs: *mut c_void) -> *mut c_void {
    let lhs = unsafe { take_ownership(lhs) };
    let rhs = unsafe { take_ownership(rhs) };

    make_expr_ptr(lhs * rhs)
}

/// Create a new substraction expression.
/// `ptr` is freed and a new expression in allocated
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_sub(lhs: *mut c_void, rhs: *mut c_void) -> *mut c_void {
    let lhs = unsafe { take_ownership(lhs) };
    let rhs = unsafe { take_ownership(rhs) };

    make_expr_ptr(lhs - rhs)
}

/// Create a new neg expression.
/// `ptr` is freed and a new expression in allocated
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_neg(ptr: *mut c_void) -> *mut c_void {
    let v = unsafe { take_ownership(ptr) };
    make_expr_ptr(-(v))
}

/// Clones an expression.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_clone(ptr: *const c_void) -> *mut c_void {
    let ptr = unsafe { to_ref::<Expr>(ptr) };
    make_expr_ptr(ptr.clone())
}

#[cfg(feature = "parse")]
/// Parses an expression from a string into res
/// If an error occurs returns 1
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_expr_parse(ptr: *const c_char, res: *mut *mut c_void) -> u8 {
    assert!(!res.is_null());

    let str = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();

    let parse_res = parse_expr(str);
    match parse_res {
        Ok(e) => {
            unsafe {
                *res = make_expr_ptr(e);
            }
            0
        }

        Err(err) => {
            set_last_error(&err);
            1
        }
    }
}

// MARK: Lang

/// Creates a new program
#[unsafe(no_mangle)]
pub extern "C" fn rumba_make_program() -> *mut c_void {
    make_program_ptr(Program::default())
}

/// Free a rumba program
/// # Safety
/// The caller promises that p is a valid pointer to a program
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_program_free(p: *mut c_void) {
    PROGRAM_POOL.with_borrow_mut(|pool| unsafe {
        pool.free(p);
    })
}

/// Simplifies a program
/// If an error occurs returns 1
///
/// # Safety
/// The caller promises that p is a valid pointer to a program
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_program_simplify(p: *mut c_void) -> u8 {
    let p: &mut Program = unsafe { to_mut_ref(p) };
    println!("{}\n-----", p);

    match p.simplify() {
        Err(e) => {
            set_last_error(&e);
            1
        }

        Ok(_) => {
            println!("{}", p);
            0
        }
    }
}

/// Returns the number of instructions in a program
///
/// # Safety
/// The caller promises that p is a valid pointer to a program
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_program_size(p: *mut c_void) -> usize {
    let p: &mut Program = unsafe { to_mut_ref(p) };
    p.insns.len()
}

/// Returns the i-th instructions in a program
///
/// # Safety
/// The caller promises that p is a valid pointer to a program
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_program_get(p: *mut c_void, i: usize) -> *const c_void {
    let p: &mut Program = unsafe { to_mut_ref(p) };
    match p.insns.get_index(i) {
        Some((_, insn)) => to_ptr(insn),
        None => null(),
    }
}

/// Returns the id of an instruction
///
/// # Safety
/// The caller promises that `insn` is a valid pointer to an instruction
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_insn_id(insn: *const c_void) -> usize {
    let insn: &Insn = unsafe { to_ref(insn) };
    insn.id.0
}

/// Returns the type of an instruction
///
/// # Safety
/// The caller promises that `insn` is a valid pointer to an instruction
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_insn_ty(insn: *const c_void) -> u8 {
    let insn: &Insn = unsafe { to_ref(insn) };
    insn.ty
}

#[repr(C)]
/// The kind of an instruction
pub enum InsnKind {
    Assign,
    Unknown,
}

/// Returns the kind of an instruction
///
/// # Safety
/// The caller promises that `insn` is a valid pointer to an instruction
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_insn_kind(insn: *const c_void) -> InsnKind {
    let insn: &Insn = unsafe { to_ref(insn) };
    match insn.kind {
        lang::InsnKind::Assign(_) => InsnKind::Assign,
        lang::InsnKind::Unknown(_) => InsnKind::Unknown,
    }
}

/// Returns the expression of an assign instruction
///
/// # Safety
/// The caller promises that `insn` is a valid pointer to an instruction
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_insn_assign_get(insn: *const c_void) -> *const c_void {
    let insn: &Insn = unsafe { to_ref(insn) };
    match &insn.kind {
        lang::InsnKind::Assign(e) => to_ptr(e),

        lang::InsnKind::Unknown(_) => {
            set_last_error("Attempted to get an expression out of an unknown instruction");
            null()
        }
    }
}

/// Returns the number of args of an unkonwn instruction
///
/// # Safety
/// The caller promises that `insn` is a valid pointer to an instruction
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_insn_unknown_size(insn: *const c_void) -> usize {
    let insn: &Insn = unsafe { to_ref(insn) };
    match &insn.kind {
        lang::InsnKind::Assign(_) => {
            set_last_error("Attempted to get an args out of an assign instruction");
            usize::MAX
        }

        lang::InsnKind::Unknown(args) => args.len(),
    }
}

/// Returns the i-th arg of an unkonwn instruction
///
/// # Safety
/// The caller promises that `insn` is a valid pointer to an instruction
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_insn_unknown_get(insn: *const c_void, i: usize) -> usize {
    let insn: &Insn = unsafe { to_ref(insn) };
    match &insn.kind {
        lang::InsnKind::Assign(_) => {
            set_last_error("Attempted to get an args out of an assign instruction");
            usize::MAX
        }

        lang::InsnKind::Unknown(args) => args[i].0,
    }
}

/// Adds an unknown instruction to the program
/// If an error occurs returns 1
///
/// # Safety
/// The caller promises that `p` is a valid pointer to a program
/// The caller promises that `args` is a valid pointer to an array of usize of size `args_len`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_program_push_unknown(
    p: *mut c_void,
    ty: u8,
    id: usize,
    args: *const usize,
    args_len: usize,
) -> u8 {
    let p: &mut Program = unsafe { to_mut_ref(p) };
    let insn = Insn {
        ty,
        id: id.into(),
        kind: lang::InsnKind::Unknown(unsafe {
            vec_from_ptr(args, args_len)
                .iter()
                .map(|v| (*v).into())
                .collect()
        }),
    };

    match p.push(insn) {
        Ok(()) => 0,

        Err(e) => {
            set_last_error(e.as_str());
            1
        }
    }
}

/// Adds an assign instruction
/// If an error occurs returns 1
///
/// This takes ownership of the expression e
///
/// # Safety
/// The caller promises that `p` is a valid pointer to a program
/// The caller promises that `e` is a valid pointer to an expression
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rumba_program_push_assign(
    p: *mut c_void,
    ty: u8,
    id: usize,
    e: *mut c_void,
) -> u8 {
    let p: &mut Program = unsafe { to_mut_ref(p) };
    let insn = Insn {
        ty,
        id: id.into(),
        kind: lang::InsnKind::Assign(unsafe { take_ownership(e) }),
    };

    match p.push(insn) {
        Ok(()) => 0,

        Err(e) => {
            set_last_error(e.as_str());
            1
        }
    }
}

/// Retrieves a detailed explanation of the last error
#[unsafe(no_mangle)]
pub extern "C" fn get_err_str() -> *const c_char {
    LAST_ERROR.with(|cell| {
        if let Some(ref err) = *cell.borrow() {
            err.as_ptr()
        } else {
            std::ptr::null()
        }
    })
}
