//! The error type returned by the MBA solver.

use crate::expr::{Expr, VarId};

/// Why the solver could not simplify an expression.
///
/// Simplifying an MBA is best-effort: a caller that hands over an expression the
/// solver cannot handle should be able to keep its original expression and carry
/// on, not die. Every variant is a "leave this one alone" signal rather than a
/// reason to abort the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveError {
    /// The linear MBA still held more variables (`found`) than the solver's
    /// internal limit (`max`) after reduction / PCT expansion, so its truth
    /// table is too large to build.
    TooManyVariables {
        /// Number of variables the expression carried after reduction.
        found: usize,
        /// The solver's internal variable limit.
        max: usize,
    },

    /// A variable produced during reduction had no entry in the restore map.
    /// Indicates an inconsistent variable map rather than a hard input.
    UnknownVariable(VarId),

    /// A solved linear MBA came back in a shape the polynomial reconstruction
    /// does not model.
    UnrecognizedForm(Expr),
}

impl std::fmt::Display for SolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveError::TooManyVariables { found, max } => {
                write!(f, "too many variables: {found} (max {max})")
            }
            SolveError::UnknownVariable(v) => write!(f, "unknown variable v{v}"),
            SolveError::UnrecognizedForm(e) => {
                write!(f, "solved linear MBA is in an unrecognized form: {e}")
            }
        }
    }
}

impl std::error::Error for SolveError {}
