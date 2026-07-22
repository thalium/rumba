//! Cosmetic rewriting of a solved expression back into bitwise form.
//!
//! The solver works in the linear-MBA basis, where `a | b` is carried as
//! `a + b - (a & b)`. That form is what the linear algebra needs, but it is not
//! what a reader wants to see, so the three encodings that have an exact
//! bitwise counterpart are folded back on the way out:
//!
//! | solved form            | prettified |
//! |------------------------|------------|
//! | `a + b - (a & b)`      | `a \| b`   |
//! | `a + b - 2 * (a & b)`  | `a ^ b`    |
//! | `-1 - a`               | `~a`       |
//!
//! Nothing else is recognised. In particular a sum that merely *contains* one
//! of these patterns among other terms is left alone: matching a subset of an
//! `Add` means choosing between overlapping candidates, and that choice is a
//! simplification decision rather than a cosmetic one.
//!
//! This runs once, on the expression handed back to the caller. It must never
//! run inside the solver's own recursion: the bitwise nodes it produces are
//! larger in [`Expr::size`] terms than the linear forms they replace, so a
//! prettified intermediate would perturb the fixed-point loop's size-based
//! stopping rule.

use crate::{expr::Expr, varint::make_mask};

/// Rewrites the exact linear encodings of `|`, `^` and `~` back into bitwise
/// nodes, bottom-up, on `n` bits.
pub(crate) fn prettify(e: Expr, n: u8) -> Expr {
    prettify_masked(e, make_mask(n))
}

fn prettify_masked(e: Expr, mask: u64) -> Expr {
    // Children first: an outer pattern is recognised in terms of the operands
    // its children have already settled on, so `a` and `b` stay comparable
    // whether or not they were themselves rewritten.
    let e = e.map(|child| prettify_masked(child, mask));

    as_binary_bitwise(&e, mask)
        .or_else(|| as_not(&e, mask))
        .unwrap_or(e)
}

/// Peels the single-operand n-ary wrappers the solver leaves behind, so that
/// the `a` of a bare term and the `a` inside `a & b` compare equal: the solver
/// emits `a + b - (a & b)` as `And([a]) + And([b]) - (a & b)`.
fn peel(e: &Expr) -> &Expr {
    match e {
        Expr::And(inner)
        | Expr::Or(inner)
        | Expr::Xor(inner)
        | Expr::Add(inner)
        | Expr::Mul(inner)
            if inner.len() == 1 =>
        {
            peel(&inner[0])
        }
        _ => e,
    }
}

/// `a + b - (a & b)` -> `a | b`, and `a + b - 2 * (a & b)` -> `a ^ b`.
fn as_binary_bitwise(e: &Expr, mask: u64) -> Option<Expr> {
    let Expr::Add(terms) = e else { return None };
    if terms.len() != 3 {
        return None;
    }

    // The `&` term is the only one that can be a scaled conjunction, so it
    // identifies itself; the other two must then be its operands.
    for (i, term) in terms.iter().enumerate() {
        let Expr::Scale(coeff, inner) = term else {
            continue;
        };
        let Expr::And(operands) = inner.as_ref() else {
            continue;
        };
        let [a, b] = &operands[..] else { continue };
        let (a, b) = (peel(a), peel(b));

        let rest: Vec<&Expr> = terms
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, t)| peel(t))
            .collect();
        let matches_operands = (rest[0] == a && rest[1] == b) || (rest[0] == b && rest[1] == a);
        if !matches_operands {
            continue;
        }

        // `-1` and `-2` on this width.
        let minus_one = mask;
        let minus_two = mask & mask.wrapping_sub(1);
        let operands = vec![a.clone(), b.clone()];
        return match coeff & mask {
            c if c == minus_one => Some(Expr::Or(operands)),
            c if c == minus_two => Some(Expr::Xor(operands)),
            _ => None,
        };
    }

    None
}

/// `-1 - a` -> `~a`.
fn as_not(e: &Expr, mask: u64) -> Option<Expr> {
    let Expr::Add(terms) = e else { return None };
    if terms.len() != 2 {
        return None;
    }

    for (i, term) in terms.iter().enumerate() {
        let Expr::Const(c) = peel(term) else { continue };
        if c & mask != mask {
            continue;
        }

        let Expr::Scale(coeff, inner) = peel(&terms[1 - i]) else {
            continue;
        };
        if coeff & mask == mask {
            return Some(!peel(inner).clone());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simplify::simplify_mba;

    fn v(i: usize) -> Expr {
        Expr::Var(i.into())
    }

    /// The solver wraps bare terms in single-operand `And`s; the patterns have
    /// to see through that, so build the inputs the way it emits them.
    fn term(e: Expr) -> Expr {
        Expr::And(vec![e])
    }

    #[test]
    fn folds_the_three_exact_forms() {
        let or = Expr::Add(vec![term(v(0)), term(v(1)), u64::MAX * (v(0) & v(1))]);
        assert_eq!(prettify(or, 32), v(0) | v(1));

        let xor = Expr::Add(vec![term(v(0)), term(v(1)), (u64::MAX - 1) * (v(0) & v(1))]);
        assert_eq!(prettify(xor, 32), v(0) ^ v(1));

        let not = Expr::Add(vec![Expr::Const(u64::MAX), u64::MAX * v(0)]);
        assert_eq!(prettify(not, 32), !v(0));
    }

    /// Coefficients are compared on the working width, not on 64 bits.
    #[test]
    fn folds_on_narrow_widths() {
        let or = Expr::Add(vec![term(v(0)), term(v(1)), 0xff * (v(0) & v(1))]);
        assert_eq!(prettify(or, 8), v(0) | v(1));

        let xor = Expr::Add(vec![term(v(0)), term(v(1)), 0xfe * (v(0) & v(1))]);
        assert_eq!(prettify(xor, 8), v(0) ^ v(1));

        // The same 8-bit coefficients mean nothing on 32 bits.
        let unchanged = Expr::Add(vec![term(v(0)), term(v(1)), 0xff * (v(0) & v(1))]);
        assert_eq!(prettify(unchanged.clone(), 32), unchanged);
    }

    /// Only whole nodes match: a sum that merely contains the pattern is left
    /// alone, because picking a subset of an `Add` is a simplification choice.
    #[test]
    fn leaves_a_sum_that_only_contains_the_pattern() {
        let e = Expr::Add(vec![
            term(v(0)),
            term(v(1)),
            u64::MAX * (v(0) & v(1)),
            term(v(2)),
        ]);
        assert_eq!(prettify(e.clone(), 32), e);
    }

    /// Nested nodes are folded: the walk is bottom-up over the whole tree.
    #[test]
    fn folds_a_nested_node() {
        let or = Expr::Add(vec![term(v(0)), term(v(1)), u64::MAX * (v(0) & v(1))]);
        let e = Expr::Mul(vec![or, v(2)]);

        assert_eq!(prettify(e, 32), Expr::Mul(vec![v(0) | v(1), v(2)]));
    }

    #[test]
    fn leaves_other_coefficients_alone() {
        // `-3` is not an encoding of any of the three operators.
        let e = Expr::Add(vec![term(v(0)), term(v(1)), (u64::MAX - 2) * (v(0) & v(1))]);
        assert_eq!(prettify(e.clone(), 32), e);
    }

    /// Folding is cosmetic: the prettified result must still mean the same
    /// thing as what the solver produced.
    #[cfg(feature = "parse")]
    #[test]
    fn preserves_semantics_through_the_solver() {
        for src in ["v0 | v1", "v0 ^ v1", "~v0", "~(v0 & v1)"] {
            let e = crate::parser::parse_expr(src).unwrap();
            let solved = simplify_mba(e.clone(), 32).unwrap();
            assert!(
                e.sem_equal(&solved, 32, 1000).is_ok(),
                "{src} became {solved}"
            );
        }
    }
}
