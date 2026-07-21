// The solver's contract is best-effort: it must degrade to a `SolveError` (or
// leave the input alone) rather than abort the process. Forbid silent `unwrap()`
// and `panic!` in library code so every fallible spot is either handled or an
// explicit, message-carrying `expect()`. Tests may still unwrap freely.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::panic))]

pub mod expr;
pub mod simplify;

pub(crate) mod patterns;
pub(crate) mod reduce;
pub(crate) mod utils;
pub(crate) mod varint;

#[cfg(feature = "jit")]
pub(crate) mod jit;
pub mod lang;

#[cfg(feature = "parse")]
pub mod parser;
