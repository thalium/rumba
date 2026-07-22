//! Simplification of Mixed Boolean-Arithmetic (MBA) expressions.
//!
//! MBA expressions interleave bitwise operators (`&`, `|`, `^`, `~`) with
//! arithmetic ones (`+`, `-`, `*`) to obscure an otherwise simple value. They
//! are a common code-obfuscation primitive; `rumba-core` recovers the
//! underlying expression.
//!
//! # Overview
//!
//! - [`expr::Expr`] is the expression tree. Build one directly with the
//!   standard operators (`+`, `-`, `*`, `&`, `|`, `^`, `!`) over
//!   [`expr::Expr::Var`] and constants, or enable the `parse` feature to read
//!   one from a string with [`parser::parse_expr`].
//! - [`simplify::simplify_mba`] reduces an expression on a given bit width.
//!   Pass a [`simplify::SimplifyCache`] via [`simplify::simplify_mba_cached`]
//!   to reuse linear solves across many calls.
//!
//! Simplification is best-effort: it returns a [`simplify::SolveError`] or
//! leaves the input untouched rather than aborting the process.
//!
//! # Example
//!
//! ```
//! use rumba_core::expr::{Expr, VarId};
//! use rumba_core::simplify::simplify_mba;
//!
//! let x = Expr::Var(VarId(0));
//! let y = Expr::Var(VarId(1));
//!
//! // (x ^ y) + 2*(x & y) is an MBA encoding of x + y.
//! let mba = (x.clone() ^ y.clone()) + 2u64 * (x.clone() & y.clone());
//!
//! let simplified = simplify_mba(mba, 64).expect("simplification failed");
//!
//! // The result is semantically equal to x + y.
//! assert!(simplified.sem_equal(&(x + y), 64, 1000).is_ok());
//! ```
//!
//! # Features
//!
//! - `parse` — string parsing of expressions via [`parser`].
//! - `jit` — JIT-compiled evaluation for faster semantic checks.
//!
//! The solver's contract is best-effort: it must degrade to a `SolveError` (or
//! leave the input alone) rather than abort the process. Forbid silent `unwrap()`
//! and `panic!` in library code so every fallible spot is either handled or an
//! explicit, message-carrying `expect()`. Tests may still unwrap freely.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::panic))]
#![warn(missing_docs)]

/// The [`expr::Expr`] expression tree and its building blocks.
pub mod expr;
/// MBA simplification entry points.
pub mod simplify;

pub(crate) mod patterns;
pub(crate) mod prettify;
pub(crate) mod reduce;
pub(crate) mod utils;
pub(crate) mod varint;

#[cfg(all(feature = "jit", not(feature = "internal-bench")))]
pub(crate) mod jit;

/// The JIT compiler, exposed only under the `internal-bench` feature so the
/// in-tree benchmarks can reach it. Hidden from docs and not part of the public
/// API; it is `pub(crate)` in every other configuration.
#[doc(hidden)]
#[cfg(feature = "internal-bench")]
pub mod jit;

/// A lower-level typed representation of MBA expressions used during solving.
pub mod lang;

/// Parsing of MBA expressions from their textual form.
#[cfg(feature = "parse")]
pub mod parser;
