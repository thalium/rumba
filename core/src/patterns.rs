//! A minimal, extensible pattern matching engine.
//!
//! Patterns fire in the non-polynomial stage of simplification, on an
//! already-canonicalized (reduced) expression, before it is turned into a
//! polynomial. Each pattern is a hand-written matcher; the driver walks the
//! expression bottom-up and, at every node, tries only the patterns that
//! attach to that node's variant (see [`Tag`]).
//!
//! On a hit a pattern returns an already-canonical replacement (e.g.
//! [`Expr::zero`]); the driver does *not* re-reduce. Any resulting stray
//! constants are absorbed by the leading `reduce` of the next simplification
//! iteration.

use std::collections::BTreeMap;

use crate::expr::Expr;

/// A lightweight, `const`-constructible discriminant used to index patterns by
/// the expression variant they care about.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    And,
    Or,
    Xor,
    Add,
    Mul,
    Not,
    Scale,
    Var,
    Const,
}

impl Expr {
    /// The variant tag of this expression.
    fn tag(&self) -> Tag {
        match self {
            Expr::And(_) => Tag::And,
            Expr::Or(_) => Tag::Or,
            Expr::Xor(_) => Tag::Xor,
            Expr::Add(_) => Tag::Add,
            Expr::Mul(_) => Tag::Mul,
            Expr::Not(_) => Tag::Not,
            Expr::Scale(_, _) => Tag::Scale,
            Expr::Var(_) => Tag::Var,
            Expr::Const(_) => Tag::Const,
        }
    }
}

/// A single rewrite rule.
pub trait Pattern: Sync {
    /// The expression variants this pattern attaches to.
    fn tags(&self) -> &'static [Tag];

    /// Attempt to rewrite `e` (which is already reduced). Returns the canonical
    /// replacement on a hit, or `None` when the pattern does not apply.
    fn apply(&self, e: &Expr, mask: u64) -> Option<Expr>;
}

/// Splits a (reduced) term into `(factor, core)` such that `term == factor·core`
/// (mod 2ⁿ), seeing through `reduce`'s distribution of a scalar over a sum.
///
/// For `Scale(c, X)` this is `(c, X)`. For a uniformly-scaled sum such as
/// `2·x + 2·y` (the reduced form of `2·(x + y)`) it is `(2, x + y)`; the factor
/// comes from [`Expr::get_factor`] and the core is the term with that factor
/// divided out. A bare expression is `(1, itself)`. `reduce` guarantees no
/// nested `Scale(Scale(..))`, so a single strip is enough.
fn split(e: &Expr, mask: u64) -> (u64, Expr) {
    let f = e.get_factor(mask);
    if f == 1 {
        return (1, e.clone());
    }
    let strip = |t: &Expr| match t {
        Expr::Scale(_, inner) => (**inner).clone(),
        other => other.clone(),
    };
    let core = match e {
        Expr::Add(terms) => Expr::Add(terms.iter().map(strip).collect()),
        other => strip(other),
    };
    (f, core)
}

/// `coefficient * e`, reduced into the same canonical form seen by patterns.
fn scaled(e: &Expr, coefficient: u64, mask: u64) -> Expr {
    Expr::scale(coefficient, e.clone()).reduce_masked(mask)
}

/// The canonical arithmetic negation of `e`.
fn negated(e: &Expr, mask: u64) -> Expr {
    scaled(e, mask, mask)
}

/// Converts a reduced expression into its top-level additive coefficient map.
/// This lets us compare distributed forms such as `2*x - 2*y` and `x - y`.
fn additive_coefficients(e: &Expr, mask: u64) -> BTreeMap<Expr, u64> {
    let mut result = BTreeMap::new();
    let terms: &[Expr] = match e {
        Expr::Add(terms) => terms,
        _ => std::slice::from_ref(e),
    };
    for term in terms {
        let (coefficient, core) = match term {
            Expr::Scale(c, core) => (c & mask, core.as_ref().clone()),
            Expr::Const(c) => (c & mask, Expr::make_const(1)),
            _ => (1, term.clone()),
        };
        let entry = result.entry(core).or_insert(0u64);
        *entry = entry.wrapping_add(coefficient) & mask;
    }
    result.retain(|_, coefficient| *coefficient != 0);
    result
}

/// If `candidate == coefficient * base` modulo the current bit width, returns
/// that coefficient. At least one coefficient of `base` must be odd; the old
/// factor/core matcher remains as a fallback for uniformly even expressions.
fn scale_relation(candidate: &Expr, base: &Expr, mask: u64) -> Option<u64> {
    let arithmetic = |e: &Expr| match e {
        Expr::Not(inner) => (-inner.as_ref().clone() - Expr::make_const(1)).reduce_masked(mask),
        _ => e.clone(),
    };
    let candidate = additive_coefficients(&arithmetic(candidate), mask);
    let base = additive_coefficients(&arithmetic(base), mask);
    if candidate.keys().ne(base.keys()) {
        return None;
    }
    let (first, &pivot) = base
        .iter()
        .find(|(_, coefficient)| **coefficient & 1 == 1)?;

    // Newton iteration computes the inverse of an odd integer modulo 2^64;
    // masking below adapts it to the active bit width.
    let mut inverse = pivot;
    for _ in 0..6 {
        inverse = inverse.wrapping_mul(2u64.wrapping_sub(pivot.wrapping_mul(inverse)));
    }
    let coefficient = candidate[first].wrapping_mul(inverse) & mask;
    base.iter()
        .all(|(core, value)| candidate[core] == value.wrapping_mul(coefficient) & mask)
        .then_some(coefficient)
}

fn are_negations(a: &Expr, b: &Expr, mask: u64) -> bool {
    let arithmetic = |e: &Expr| match e {
        Expr::Not(inner) => (-inner.as_ref().clone() - Expr::make_const(1)).reduce_masked(mask),
        _ => e.clone(),
    };
    let a = arithmetic(a);
    let b = arithmetic(b);
    negated(&a, mask) == b
        || negated(&b, mask) == a
        || scale_relation(&a, &b, mask) == Some(mask)
        || scale_relation(&b, &a, mask) == Some(mask)
}

fn is_low_bit_of(candidate: &Expr, base: &Expr, mask: u64) -> bool {
    let Expr::And(children) = candidate else {
        return false;
    };
    children.len() == 2
        && ((scale_relation(&children[0], base, mask) == Some(1)
            && are_negations(&children[0], &children[1], mask))
            || (scale_relation(&children[1], base, mask) == Some(1)
                && are_negations(&children[0], &children[1], mask)))
}

fn rebuild_and(mut children: Vec<Expr>) -> Expr {
    match children.len() {
        0 => Expr::make_const(u64::MAX),
        1 => children.remove(0),
        _ => Expr::And(children),
    }
}

/// Recognizes the reduced form of a low-bit mask `m·X - 1`.
///
/// After `reduce`, `m·X - 1` is an `Add` of a `-1` constant (`mask`, i.e. all
/// ones) and the terms of `m·X`. The latter may be a single `Scale(m, C)` or,
/// when `X` is a sum, a distributed group like `m·x + m·y`. Returns `(m, C)` —
/// the even-multiple coefficient and `X`'s core — via [`split`].
fn as_low_bit_mask(e: &Expr, mask: u64) -> Option<(u64, Expr)> {
    let Expr::Add(terms) = e else {
        return None;
    };
    let mut neg_one = false;
    let mut rest: Vec<Expr> = Vec::with_capacity(terms.len());
    for t in terms {
        match t {
            Expr::Const(c) if (c & mask) == mask => neg_one = true,
            _ => rest.push(t.clone()),
        }
    }
    if !neg_one || rest.is_empty() {
        return None;
    }
    let m_x = match rest.len() {
        1 => rest.remove(0),
        _ => Expr::Add(rest),
    };
    Some(split(&m_x, mask))
}

/// Returns the non-constant part of a reduced `m*X - 1` mask.
fn low_bit_mask_multiple(e: &Expr, mask: u64) -> Option<Expr> {
    let Expr::Add(terms) = e else {
        return None;
    };
    let mut neg_one = false;
    let mut rest = Vec::new();
    for term in terms {
        match term {
            Expr::Const(c) if (c & mask) == mask => neg_one = true,
            _ => rest.push(term.clone()),
        }
    }
    if !neg_one || rest.is_empty() {
        return None;
    }
    Some(if rest.len() == 1 {
        rest.remove(0)
    } else {
        Expr::Add(rest)
    })
}

mod low_bit_annihilator;
mod low_bit_mask_split;
mod low_bit_redundant_mask;
mod low_bit_remainder;
mod nested_bitwise_identity;
mod successor_boundary_absorption;

use low_bit_annihilator::LowBitAnnihilator;
use low_bit_mask_split::LowBitMaskSplit;
use low_bit_redundant_mask::LowBitRedundantMask;
use low_bit_remainder::LowBitRemainder;
use nested_bitwise_identity::NestedBitwiseIdentity;
use successor_boundary_absorption::SuccessorBoundaryAbsorption;

/// The registered patterns, tried in order at each node.
static PATTERNS: &[&dyn Pattern] = &[
    &LowBitAnnihilator,
    &LowBitRedundantMask,
    &LowBitRemainder,
    &SuccessorBoundaryAbsorption,
    &LowBitMaskSplit,
    &NestedBitwiseIdentity,
];

/// Applies the registered patterns to `e`, walking bottom-up.
pub fn apply_patterns(e: Expr, mask: u64) -> Expr {
    // Rewrite children first so parents see canonicalized subterms.
    let e = e.map(|child| apply_patterns(child, mask));

    let tag = e.tag();
    for pattern in PATTERNS {
        if pattern.tags().contains(&tag)
            && let Some(replacement) = pattern.apply(&e, mask)
        {
            return replacement;
        }
    }

    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{Expr, VarId};
    use crate::varint::make_mask;

    const N: u8 = 64;

    fn var(i: usize) -> Expr {
        Expr::Var(VarId(i))
    }

    /// `X & -X & (m·X)` reduced, then run through the pattern pass.
    fn annihilate(x: Expr, m: u64) -> Expr {
        let mask = make_mask(N);
        let e = (x.clone() & (-x.clone()) & (m * x)).reduce_masked(mask);
        apply_patterns(e, mask)
    }

    #[test]
    fn fires_on_power_of_two() {
        for k in 1..8 {
            let e = annihilate(var(0), 1 << k);
            assert_eq!(e, Expr::zero(), "k={k}");
        }
    }

    #[test]
    fn fires_on_even_non_power_of_two() {
        // 6 is even but not a power of two -> still valid.
        assert_eq!(annihilate(var(0), 6), Expr::zero());
        assert_eq!(annihilate(var(0), 10), Expr::zero());
    }

    #[test]
    fn fires_on_complex_core() {
        let x = var(0) | var(1);
        assert_eq!(annihilate(x, 2), Expr::zero());

        let x = var(0) & var(1);
        assert_eq!(annihilate(x, 4), Expr::zero());
    }

    #[test]
    fn annihilator_fires_on_affine_core() {
        let x = var(0) - var(1);
        for m in [2u64, 4, 6, 10] {
            assert_eq!(annihilate(x.clone(), m), Expr::zero(), "m={m}");
        }
    }

    #[test]
    fn fires_on_scaled_core() {
        // X = 2·y (X itself carries a coefficient).
        let x = 2u64 * var(0);
        assert_eq!(annihilate(x, 2), Expr::zero());
    }

    #[test]
    fn fires_within_larger_and() {
        let mask = make_mask(N);
        let x = var(0);
        let e = (var(1) & x.clone() & (-x.clone()) & (2u64 * x) & var(2)).reduce_masked(mask);
        assert_eq!(apply_patterns(e, mask), Expr::zero());
    }

    #[test]
    fn ignores_odd_multiple() {
        // 3·X is an odd multiple: the identity does NOT hold.
        let mask = make_mask(N);
        let x = var(0);
        let e = (x.clone() & (-x.clone()) & (3u64 * x)).reduce_masked(mask);
        assert_ne!(apply_patterns(e.clone(), mask), Expr::zero());
    }

    #[test]
    fn ignores_missing_negation() {
        let mask = make_mask(N);
        let x = var(0);
        let e = (x.clone() & (2u64 * x)).reduce_masked(mask);
        assert_ne!(apply_patterns(e.clone(), mask), Expr::zero());
    }

    #[test]
    fn ignores_unrelated_and() {
        let mask = make_mask(N);
        let e = (var(0) & var(1) & (2u64 * var(2))).reduce_masked(mask);
        let out = apply_patterns(e.clone(), mask);
        assert_eq!(out, e);
    }

    #[test]
    fn original_is_semantically_zero() {
        // Sanity: the pre-rewrite expression really is 0.
        let mask = make_mask(N);
        for m in [2u64, 4, 6, 8, 10, 1 << 20] {
            let x = var(0);
            let e = (x.clone() & (-x.clone()) & (m * x)).reduce_masked(mask);
            assert!(
                e.sem_equal_masked(&Expr::zero(), mask, 500).is_ok(),
                "m={m} not semantically zero"
            );
        }
    }

    /// `(X & -X) & (m·X - 1)` reduced, then run through the pattern pass.
    fn redundant_mask(x: Expr, m: u64) -> Expr {
        let mask = make_mask(N);
        let e = (x.clone() & (-x.clone()) & (m * x - Expr::make_const(1))).reduce_masked(mask);
        apply_patterns(e, mask)
    }

    #[test]
    fn mask_reduces_to_low_bit() {
        let mask = make_mask(N);
        let expected = (var(0) & (-var(0))).reduce_masked(mask);
        for m in [2u64, 4, 6, 8, 10, 1 << 20] {
            assert_eq!(redundant_mask(var(0), m), expected, "m={m}");
        }
    }

    #[test]
    fn redundant_mask_fires_on_affine_core() {
        let mask = make_mask(N);
        let x = var(0) - var(1);
        let expected = (x.clone() & (-x.clone())).reduce_masked(mask);
        for m in [2u64, 4, 6, 10] {
            assert_eq!(redundant_mask(x.clone(), m), expected, "m={m}");
        }
    }

    #[test]
    fn mask_fires_within_larger_and() {
        let mask = make_mask(N);
        let x = var(0);
        let e =
            (var(1) & x.clone() & (-x.clone()) & (6u64 * x.clone() - Expr::make_const(1)) & var(2))
                .reduce_masked(mask);
        let expected = (var(1) & x.clone() & (-x.clone()) & var(2)).reduce_masked(mask);
        assert_eq!(apply_patterns(e, mask), expected);
    }

    #[test]
    fn mask_ignores_odd_multiple() {
        // `3·X - 1` is not zero across the low bits: identity does not hold.
        let mask = make_mask(N);
        let x = var(0);
        let e = (x.clone() & (-x.clone()) & (3u64 * x - Expr::make_const(1))).reduce_masked(mask);
        let expected = (var(0) & (-var(0))).reduce_masked(mask);
        assert_ne!(apply_patterns(e, mask), expected);
    }

    #[test]
    fn mask_ignores_missing_negation() {
        let mask = make_mask(N);
        let x = var(0);
        let e = (x.clone() & (2u64 * x - Expr::make_const(1))).reduce_masked(mask);
        assert_eq!(apply_patterns(e.clone(), mask), e);
    }

    #[test]
    fn mask_is_semantically_equivalent() {
        let mask = make_mask(N);
        for m in [2u64, 4, 6, 8, 10, 1 << 20] {
            let x = var(0);
            let before = (x.clone() & (-x.clone()) & (m * x - Expr::make_const(1))).reduce_masked(mask);
            let after = apply_patterns(before.clone(), mask);
            assert!(
                before.sem_equal_masked(&after, mask, 500).is_ok(),
                "m={m} rewrite changed semantics"
            );
        }
    }

    #[test]
    fn low_bit_remainder_is_absorbed_by_base() {
        let mask = make_mask(N);
        for x in [var(0), var(0) - var(1), var(0) + 2u64 * var(1)] {
            let low_bit = (x.clone() & (-x.clone())).reduce_masked(mask);
            let remainder = (x.clone() - low_bit).reduce_masked(mask);
            let before = (x & remainder.clone()).reduce_masked(mask);
            assert_eq!(apply_patterns(before, mask), remainder);
        }
    }

    #[test]
    fn low_bit_is_reconstructed_from_complementary_mask() {
        let mask = make_mask(N);
        for x in [var(0), var(0) - var(1), var(0) + 2u64 * var(1)] {
            let low_bit = (x.clone() & (-x.clone())).reduce_masked(mask);
            let complementary = (-Expr::make_const(1) - x.clone() + low_bit.clone()).reduce_masked(mask);
            let before = (x & complementary).reduce_masked(mask);
            assert_eq!(apply_patterns(before, mask), low_bit);
        }
    }

    #[test]
    fn successor_boundary_is_absorbed_by_predecessor() {
        let mask = make_mask(N);
        for x in [
            var(0),
            var(0) - var(1),
            var(0) & var(1),
            var(0) + (var(0) & var(1)),
        ] {
            let successor = (x.clone() + Expr::make_const(1)).reduce_masked(mask);
            for multiple in [2u64, 4, 6, mask - 1] {
                let boundary = (successor.clone() & multiple * successor.clone()).reduce_masked(mask);
                let before = (x.clone() & boundary.clone()).reduce_masked(mask);
                assert_eq!(
                    apply_patterns(before, mask),
                    boundary,
                    "multiple={multiple}"
                );
            }
        }
    }

    #[test]
    fn successor_boundary_keeps_odd_multiple() {
        let mask = make_mask(N);
        let x = var(0) - var(1);
        let successor = (x.clone() + Expr::make_const(1)).reduce_masked(mask);
        let before = (x & successor.clone() & 3u64 * successor).reduce_masked(mask);
        assert_eq!(apply_patterns(before.clone(), mask), before);
    }

    /// `a·(X & M) + a·((-X) & M)` with `M = m·X - 1`, reduced and rewritten.
    fn mask_split(x: Expr, a: u64, m: u64) -> Expr {
        let mask = make_mask(N);
        let mm = m * x.clone() - Expr::make_const(1);
        let e = (a * (x.clone() & mm.clone()) + a * ((-x) & mm)).reduce_masked(mask);
        apply_patterns(e, mask)
    }

    #[test]
    fn split_collapses_to_multiple() {
        let mask = make_mask(N);
        for a in [1u64, 3, 5] {
            for m in [2u64, 4, 6, 8, 10] {
                let expected = (a * (m * var(0))).reduce_masked(mask);
                assert_eq!(mask_split(var(0), a, m), expected, "a={a} m={m}");
            }
        }
    }

    #[test]
    fn split_at_even_multiple_collapses_to_multiple() {
        let mask = make_mask(N);
        for x in [var(0), var(0) - var(1), var(0) + Expr::make_const(1)] {
            for multiple in [2u64, 4, 6, 10] {
                let boundary = (multiple * x.clone()).reduce_masked(mask);
                let before = ((x.clone() & boundary.clone()) + ((-x.clone()) & boundary.clone()))
                    .reduce_masked(mask);
                assert_eq!(apply_patterns(before, mask), boundary);
            }
        }
    }

    #[test]
    fn split_is_semantically_equivalent() {
        let mask = make_mask(N);
        for a in [1u64, 3, 7] {
            for m in [2u64, 4, 6, 12, 1 << 10] {
                let x = var(0);
                let mm = m * x.clone() - Expr::make_const(1);
                let before = (a * (x.clone() & mm.clone()) + a * ((-x) & mm)).reduce_masked(mask);
                let after = apply_patterns(before.clone(), mask);
                assert!(
                    before.sem_equal_masked(&after, mask, 500).is_ok(),
                    "a={a} m={m} rewrite changed semantics"
                );
            }
        }
    }

    #[test]
    fn split_fires_on_scaled_core() {
        // X = 2·y carries its own coefficient.
        let mask = make_mask(N);
        let x = 2u64 * var(0);
        let expected = (3u64 * (6u64 * x.clone())).reduce_masked(mask);
        assert_eq!(mask_split(x, 3, 6), expected);
    }

    #[test]
    fn split_fires_on_affine_core() {
        let mask = make_mask(N);
        let x = var(0) - var(1);
        let expected = (3u64 * (6u64 * x.clone())).reduce_masked(mask);
        assert_eq!(mask_split(x, 3, 6), expected);
    }

    #[test]
    fn split_fires_within_larger_add() {
        let mask = make_mask(N);
        let x = var(0);
        let mm = 6u64 * x.clone() - Expr::make_const(1);
        let e =
            (var(1) + 3u64 * (x.clone() & mm.clone()) + 3u64 * ((-x.clone()) & mm)).reduce_masked(mask);
        let expected = (var(1) + 3u64 * (6u64 * x)).reduce_masked(mask);
        assert_eq!(apply_patterns(e, mask), expected);
    }

    #[test]
    fn split_ignores_odd_multiple() {
        // `3·X - 1` is an odd multiple: the identity does not hold.
        let mask = make_mask(N);
        let x = var(0);
        let mm = 3u64 * x.clone() - Expr::make_const(1);
        let e = (3u64 * (x.clone() & mm.clone()) + 3u64 * ((-x) & mm)).reduce_masked(mask);
        let out = apply_patterns(e.clone(), mask);
        assert_eq!(out, e);
    }

    /// `X & -((X | Y) & -X)` reduced, then run through the pattern pass.
    fn nested(x: Expr, y: Option<Expr>) -> Expr {
        let mask = make_mask(N);
        let disj = match y {
            Some(y) => x.clone() | y,
            None => x.clone(),
        };
        let e = (x.clone() & (-(disj & (-x.clone())))).reduce_masked(mask);
        apply_patterns(e, mask)
    }

    #[test]
    fn nested_collapses_y_absent() {
        let mask = make_mask(N);
        assert_eq!(nested(var(0), None), var(0).reduce_masked(mask));
        // Works with a compound core `X = v0 + v1`.
        let x = var(0) + var(1);
        assert_eq!(nested(x.clone(), None), x.reduce_masked(mask));
    }

    #[test]
    fn nested_collapses_y_present() {
        let mask = make_mask(N);
        assert_eq!(nested(var(0), Some(var(1))), var(0).reduce_masked(mask));
    }

    #[test]
    fn nested_fires_within_larger_and() {
        let mask = make_mask(N);
        let x = var(0);
        let redundant = -((x.clone()) & (-x.clone()));
        let e = (var(1) & x.clone() & redundant & var(2)).reduce_masked(mask);
        let expected = (var(1) & x.clone() & var(2)).reduce_masked(mask);
        assert_eq!(apply_patterns(e, mask), expected);
    }

    #[test]
    fn nested_is_semantically_equivalent() {
        let mask = make_mask(N);
        for x in [var(0), var(0) + var(1), !var(0)] {
            for y in [None, Some(var(1)), Some(var(0) & var(1))] {
                let disj = match &y {
                    Some(y) => x.clone() | y.clone(),
                    None => x.clone(),
                };
                let before = (x.clone() & (-(disj & (-x.clone())))).reduce_masked(mask);
                let after = apply_patterns(before.clone(), mask);
                assert!(
                    before.sem_equal_masked(&after, mask, 500).is_ok(),
                    "rewrite changed semantics"
                );
            }
        }
    }

    #[test]
    fn nested_ignores_unrelated() {
        // Redundant conjunct present but the required sibling `X` is not.
        let mask = make_mask(N);
        let x = var(0);
        let e = (var(1) & (-((x.clone()) & (-x.clone())))).reduce_masked(mask);
        assert_eq!(apply_patterns(e.clone(), mask), e);
    }

    #[test]
    fn split_ignores_mismatched_scale() {
        // Different outer scales `a` on the two summands: no collapse.
        let mask = make_mask(N);
        let x = var(0);
        let mm = 6u64 * x.clone() - Expr::make_const(1);
        let e = (3u64 * (x.clone() & mm.clone()) + 5u64 * ((-x) & mm)).reduce_masked(mask);
        let out = apply_patterns(e.clone(), mask);
        assert_eq!(out, e);
    }
}
