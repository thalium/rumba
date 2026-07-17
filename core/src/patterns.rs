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

use crate::expr::Expr;
use crate::varint::VarInt;

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
            Expr::Const(c) if c.get(mask) == mask => neg_one = true,
            _ => rest.push(t.clone()),
        }
    }
    if !neg_one || rest.is_empty() {
        return None;
    }
    let m_x = match rest.len() {
        1 => rest.pop().unwrap(),
        _ => Expr::Add(rest),
    };
    Some(split(&m_x, mask))
}

/// `X & (-X) & (m·X) = 0` for any expression `X` and any even `m`.
///
/// `X & -X` isolates the single lowest set bit of `X`, at position `p`. Bit `p`
/// of `m·X` equals the parity of `m`, so any even multiple of `X` is zero at
/// that bit and the whole conjunction collapses to `0`.
///
/// Writing `X = a·C`, the three terms appear as children with the same core `C`
/// and coefficients `a`, `-a`, and `b`, where `b` is an even multiple of `a`
/// modulo `2ⁿ`. The latter is exactly `trailing_zeros(b) > trailing_zeros(a)`.
struct LowBitAnnihilator;

impl Pattern for LowBitAnnihilator {
    fn tags(&self) -> &'static [Tag] {
        &[Tag::And]
    }

    fn apply(&self, e: &Expr, mask: u64) -> Option<Expr> {
        let Expr::And(children) = e else {
            return None;
        };
        if children.len() < 3 {
            return None;
        }

        // Normalize each conjunct to `(factor, core)` so that `a·C`, `-a·C` and
        // `m·C` are recognized even when `reduce` has distributed the scalar
        // over a sum (e.g. `2·(x+y)` stored as `2·x + 2·y`).
        let norm: Vec<(u64, Expr)> = children.iter().map(|c| split(c, mask)).collect();

        for (a, core) in &norm {
            let a = *a;

            // Is a coefficient-1 term for this core available? Either directly,
            // or — when `X`'s core is an `And` — because `reduce` flattened the
            // bare `X` into the parent, making its conjuncts siblings.
            let bare_present = norm.iter().any(|(f, c)| *f == 1 && c == core)
                || match core {
                    Expr::And(elems) => elems.iter().all(|el| children.contains(el)),
                    _ => false,
                };

            let neg = a.wrapping_neg() & mask; // -a mod 2ⁿ
            let tz_a = a.trailing_zeros();

            let mut has_neg = false;
            let mut has_even_multiple = false;
            let mut visit = |b: u64| {
                if b == neg {
                    has_neg = true;
                }
                // `b` is an even multiple of `a` (mod 2ⁿ).
                if b.trailing_zeros() > tz_a {
                    has_even_multiple = true;
                }
            };

            for (b, b_core) in &norm {
                if b_core == core {
                    visit(*b);
                }
            }
            if bare_present {
                visit(1);
            }

            if has_neg && has_even_multiple {
                return Some(Expr::zero());
            }
        }

        None
    }
}

/// `(X & -X) & (m·X - 1) = X & -X` for any expression `X` and any even `m`.
///
/// `X & -X` isolates the single lowest set bit of `X`, at position `p`. As in
/// [`LowBitAnnihilator`], an even multiple `m·X` is zero across bits `0..=p`, so
/// `m·X - 1` borrows all the way up and is *one* at bit `p`. Conjoining bit `p`
/// with something that is set at bit `p` leaves it unchanged, so the `m·X - 1`
/// term is redundant and can be dropped.
///
/// Writing `X = a·C`, the isolating pair appears as two children with core `C`
/// and coefficients `a` and `-a`, while the redundant term is the reduced form
/// of `m·X - 1`, i.e. `Add([Const(-1), Scale(m, C)])`, with `m` an even multiple
/// of `a` (`trailing_zeros(m) > trailing_zeros(a)`).
struct LowBitRedundantMask;

impl Pattern for LowBitRedundantMask {
    fn tags(&self) -> &'static [Tag] {
        &[Tag::And]
    }

    fn apply(&self, e: &Expr, mask: u64) -> Option<Expr> {
        let Expr::And(children) = e else {
            return None;
        };

        // A match needs `X`, `-X` and the `m·X - 1` conjunct.
        if children.len() < 3 {
            return None;
        }

        let norm: Vec<(u64, Expr)> = children.iter().map(|c| split(c, mask)).collect();

        for (idx, cand) in children.iter().enumerate() {
            // The redundant conjunct is the reduced `m·X - 1`.
            let Some((m, core)) = as_low_bit_mask(cand, mask) else {
                continue;
            };

            // `X & -X` must be isolated by a sibling pair `a·C` and `-a·C`, with
            // `m` an even multiple of `a`.
            let isolated = norm.iter().any(|(a, a_core)| {
                *a_core == core
                    && m.trailing_zeros() > a.trailing_zeros()
                    && norm
                        .iter()
                        .any(|(b, b_core)| *b_core == core && *b == a.wrapping_neg() & mask)
            });
            if !isolated {
                continue;
            }

            // Drop the redundant conjunct; the rest is already canonical.
            let mut kept = children.clone();
            kept.remove(idx);
            return Some(if kept.len() == 1 {
                kept.pop().unwrap()
            } else {
                Expr::And(kept)
            });
        }

        None
    }
}

/// `a·(X & M) + a·((-X) & M) = a·m·X`, where `M = m·X - 1` and `m` is even.
///
/// Masking with `M = m·X - 1` splits `X` at its low bits: for even `m`, `m·X` is
/// zero across bits `0..=p` (`p` = position of `X`'s lowest set bit), so `M` is
/// all-ones there and `-1` (all-ones) above. `X & M` keeps `X`'s value below the
/// split and drops it above; `(-X) & M` picks up the complementary part of the
/// two's-complement negation. Their sum reconstitutes exactly `m·X`.
///
/// Writing `X = a_t·C`, the two summands appear (post-`reduce`) as terms scaled
/// by a common `a`, each an `And` of `M = Add([Const(-1), Scale(b, C)])` and a
/// core term — `a_t·C` in one, `-a_t·C` in the other — where `b = m·a_t` is an
/// even multiple of `a_t`. The pair collapses to a single `Scale(a·b, C)` since
/// `a·m·X = a·m·a_t·C = a·b·C`.
struct LowBitMaskSplit;

impl LowBitMaskSplit {
    /// Splits `X & (m·X - 1)`: returns `(a, a_t, b, C)` for a term `a·(t & M)`
    /// where `t = a_t·C`, `M = m·X - 1` carries even multiple `b` over the same
    /// core `C`, and `b` is an even multiple of `a_t`.
    fn split_masked(term: &Expr, mask: u64) -> Option<(u64, u64, u64, Expr)> {
        let (a, core) = split(term, mask);
        let Expr::And(conj) = core else {
            return None;
        };
        if conj.len() != 2 {
            return None;
        }
        // One conjunct is the mask `M`, the other is the core term `t`.
        for (i, j) in [(0, 1), (1, 0)] {
            let Some((b, m_core)) = as_low_bit_mask(&conj[i], mask) else {
                continue;
            };
            let (a_t, t_core) = split(&conj[j], mask);
            if t_core != m_core || b.trailing_zeros() <= a_t.trailing_zeros() {
                continue;
            }
            return Some((a, a_t, b, m_core));
        }
        None
    }
}

impl Pattern for LowBitMaskSplit {
    fn tags(&self) -> &'static [Tag] {
        &[Tag::Add]
    }

    fn apply(&self, e: &Expr, mask: u64) -> Option<Expr> {
        let Expr::Add(terms) = e else {
            return None;
        };
        if terms.len() < 2 {
            return None;
        }

        for i in 0..terms.len() {
            let Some((a1, a_t1, b1, core1)) = Self::split_masked(&terms[i], mask) else {
                continue;
            };
            for j in (i + 1)..terms.len() {
                let Some((a2, a_t2, b2, core2)) = Self::split_masked(&terms[j], mask) else {
                    continue;
                };
                // Same outer scale `a`, same mask `M` (core `C` and multiple `b`),
                // and the two core terms negate each other (`a_t` and `-a_t`).
                if a1 != a2
                    || b1 != b2
                    || core1 != core2
                    || a_t1 != a_t2.wrapping_neg() & mask
                {
                    continue;
                }

                // Replace the pair with the single term `a·m·X = a·b·C`.
                let coeff = (VarInt::from(a1) * VarInt::from(b1)).mask(mask);
                let collapsed = Expr::scale(coeff, core1);

                let kept: Vec<Expr> = terms
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| *k != i && *k != j)
                    .map(|(_, t)| t.clone())
                    .chain(std::iter::once(collapsed))
                    .collect();
                // `collapsed` may be a scaled sum, and may combine with a kept
                // term, so re-canonicalize.
                return Some(Expr::Add(kept).reduce(mask));
            }
        }

        None
    }
}

/// `X & -((X | Y) & -X) = X` for any expressions `X` and `Y` (with `Y`
/// optionally absent, i.e. the inner disjunction collapsing to just `X`).
///
/// This is the conjunction form of Gamba's *nested bitwise identity* rule. The
/// term `-((X | Y) & -X)` is redundant inside a conjunction that already
/// contains `X`: `(X | Y) & -X` keeps only bits of `-X` that are also set in
/// `X | Y`; negating and re-conjoining with `X` leaves `X` unchanged. The
/// identity was checked exhaustively for all `X, Y` on 4/6/8-bit widths.
///
/// After `reduce`, the redundant conjunct appears as `Scale(-1, inner)` where
/// `inner = And([c0, c1])` and one of `{c0, c1}` is `-X` while the other is
/// either `X` itself (the `Y`-absent case) or an `Or` containing `X`. In the
/// `Y`-absent case the required sibling may be either `X` or `-X` (they are the
/// two conjuncts of `inner`).
struct NestedBitwiseIdentity;

impl NestedBitwiseIdentity {
    /// `-e`, canonicalized, so it compares equal to a reduced sibling.
    fn negate(e: &Expr, mask: u64) -> Expr {
        Expr::scale(VarInt::from(mask), e.clone()).reduce(mask)
    }

    /// For a redundant conjunct `-(c0 & c1)`, returns the sibling expressions
    /// whose presence in the outer conjunction makes it collapse.
    fn required_siblings(c0: &Expr, c1: &Expr, mask: u64) -> Vec<Expr> {
        // `Y`-absent case: the inner conjuncts are `X` and `-X`. Either one
        // being present suffices.
        if Self::negate(c1, mask) == *c0 {
            return vec![c0.clone(), c1.clone()];
        }
        // `Y`-present case: one conjunct is a disjunction `(X | Y)` and the
        // other is `-X`; the required sibling is `X = -(-X)`.
        if let Expr::Or(elems) = c0 {
            let x = Self::negate(c1, mask);
            if elems.contains(&x) {
                return vec![x];
            }
        }
        if let Expr::Or(elems) = c1 {
            let x = Self::negate(c0, mask);
            if elems.contains(&x) {
                return vec![x];
            }
        }
        Vec::new()
    }
}

impl Pattern for NestedBitwiseIdentity {
    fn tags(&self) -> &'static [Tag] {
        &[Tag::And]
    }

    fn apply(&self, e: &Expr, mask: u64) -> Option<Expr> {
        let Expr::And(children) = e else {
            return None;
        };
        if children.len() < 2 {
            return None;
        }

        for (idx, child) in children.iter().enumerate() {
            // The redundant conjunct is exactly `-1 · (c0 & c1)`.
            let Expr::Scale(c, inner) = child else {
                continue;
            };
            if c.get(mask) != mask {
                continue;
            }
            let Expr::And(conj) = inner.as_ref() else {
                continue;
            };
            if conj.len() != 2 {
                continue;
            }

            for sibling in Self::required_siblings(&conj[0], &conj[1], mask) {
                if children
                    .iter()
                    .enumerate()
                    .any(|(k, c)| k != idx && *c == sibling)
                {
                    // Drop the redundant conjunct; the rest is already canonical.
                    let mut kept: Vec<Expr> = children
                        .iter()
                        .enumerate()
                        .filter(|(k, _)| *k != idx)
                        .map(|(_, c)| c.clone())
                        .collect();
                    return Some(if kept.len() == 1 {
                        kept.pop().unwrap()
                    } else {
                        Expr::And(kept)
                    });
                }
            }
        }

        None
    }
}

/// The registered patterns, tried in order at each node.
static PATTERNS: &[&dyn Pattern] = &[
    &LowBitAnnihilator,
    &LowBitRedundantMask,
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
        let e = (x.clone() & (-x.clone()) & (m * x)).reduce(mask);
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
    fn fires_on_scaled_core() {
        // X = 2·y (X itself carries a coefficient).
        let x = 2u64 * var(0);
        assert_eq!(annihilate(x, 2), Expr::zero());
    }

    #[test]
    fn fires_within_larger_and() {
        let mask = make_mask(N);
        let x = var(0);
        let e = (var(1) & x.clone() & (-x.clone()) & (2u64 * x) & var(2)).reduce(mask);
        assert_eq!(apply_patterns(e, mask), Expr::zero());
    }

    #[test]
    fn ignores_odd_multiple() {
        // 3·X is an odd multiple: the identity does NOT hold.
        let mask = make_mask(N);
        let x = var(0);
        let e = (x.clone() & (-x.clone()) & (3u64 * x)).reduce(mask);
        assert_ne!(apply_patterns(e.clone(), mask), Expr::zero());
    }

    #[test]
    fn ignores_missing_negation() {
        let mask = make_mask(N);
        let x = var(0);
        let e = (x.clone() & (2u64 * x)).reduce(mask);
        assert_ne!(apply_patterns(e.clone(), mask), Expr::zero());
    }

    #[test]
    fn ignores_unrelated_and() {
        let mask = make_mask(N);
        let e = (var(0) & var(1) & (2u64 * var(2))).reduce(mask);
        let out = apply_patterns(e.clone(), mask);
        assert_eq!(out, e);
    }

    #[test]
    fn original_is_semantically_zero() {
        // Sanity: the pre-rewrite expression really is 0.
        let mask = make_mask(N);
        for m in [2u64, 4, 6, 8, 10, 1 << 20] {
            let x = var(0);
            let e = (x.clone() & (-x.clone()) & (m * x)).reduce(mask);
            assert!(
                e.sem_equal(&Expr::zero(), mask, 500).is_ok(),
                "m={m} not semantically zero"
            );
        }
    }

    /// `(X & -X) & (m·X - 1)` reduced, then run through the pattern pass.
    fn redundant_mask(x: Expr, m: u64) -> Expr {
        let mask = make_mask(N);
        let e = (x.clone() & (-x.clone()) & (m * x - Expr::make_const(1))).reduce(mask);
        apply_patterns(e, mask)
    }

    #[test]
    fn mask_reduces_to_low_bit() {
        let mask = make_mask(N);
        let expected = (var(0) & (-var(0))).reduce(mask);
        for m in [2u64, 4, 6, 8, 10, 1 << 20] {
            assert_eq!(redundant_mask(var(0), m), expected, "m={m}");
        }
    }

    #[test]
    fn mask_fires_within_larger_and() {
        let mask = make_mask(N);
        let x = var(0);
        let e = (var(1) & x.clone() & (-x.clone()) & (6u64 * x.clone() - Expr::make_const(1)) & var(2))
            .reduce(mask);
        let expected = (var(1) & x.clone() & (-x.clone()) & var(2)).reduce(mask);
        assert_eq!(apply_patterns(e, mask), expected);
    }

    #[test]
    fn mask_ignores_odd_multiple() {
        // `3·X - 1` is not zero across the low bits: identity does not hold.
        let mask = make_mask(N);
        let x = var(0);
        let e = (x.clone() & (-x.clone()) & (3u64 * x - Expr::make_const(1))).reduce(mask);
        let expected = (var(0) & (-var(0))).reduce(mask);
        assert_ne!(apply_patterns(e, mask), expected);
    }

    #[test]
    fn mask_ignores_missing_negation() {
        let mask = make_mask(N);
        let x = var(0);
        let e = (x.clone() & (2u64 * x - Expr::make_const(1))).reduce(mask);
        assert_eq!(apply_patterns(e.clone(), mask), e);
    }

    #[test]
    fn mask_is_semantically_equivalent() {
        let mask = make_mask(N);
        for m in [2u64, 4, 6, 8, 10, 1 << 20] {
            let x = var(0);
            let before = (x.clone() & (-x.clone()) & (m * x - Expr::make_const(1))).reduce(mask);
            let after = apply_patterns(before.clone(), mask);
            assert!(
                before.sem_equal(&after, mask, 500).is_ok(),
                "m={m} rewrite changed semantics"
            );
        }
    }

    /// `a·(X & M) + a·((-X) & M)` with `M = m·X - 1`, reduced and rewritten.
    fn mask_split(x: Expr, a: u64, m: u64) -> Expr {
        let mask = make_mask(N);
        let mm = m * x.clone() - Expr::make_const(1);
        let e = (a * (x.clone() & mm.clone()) + a * ((-x) & mm)).reduce(mask);
        apply_patterns(e, mask)
    }

    #[test]
    fn split_collapses_to_multiple() {
        let mask = make_mask(N);
        for a in [1u64, 3, 5] {
            for m in [2u64, 4, 6, 8, 10] {
                let expected = (a * (m * var(0))).reduce(mask);
                assert_eq!(mask_split(var(0), a, m), expected, "a={a} m={m}");
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
                let before = (a * (x.clone() & mm.clone()) + a * ((-x) & mm)).reduce(mask);
                let after = apply_patterns(before.clone(), mask);
                assert!(
                    before.sem_equal(&after, mask, 500).is_ok(),
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
        let expected = (3u64 * (6u64 * x.clone())).reduce(mask);
        assert_eq!(mask_split(x, 3, 6), expected);
    }

    #[test]
    fn split_fires_within_larger_add() {
        let mask = make_mask(N);
        let x = var(0);
        let mm = 6u64 * x.clone() - Expr::make_const(1);
        let e = (var(1) + 3u64 * (x.clone() & mm.clone()) + 3u64 * ((-x.clone()) & mm)).reduce(mask);
        let expected = (var(1) + 3u64 * (6u64 * x)).reduce(mask);
        assert_eq!(apply_patterns(e, mask), expected);
    }

    #[test]
    fn split_ignores_odd_multiple() {
        // `3·X - 1` is an odd multiple: the identity does not hold.
        let mask = make_mask(N);
        let x = var(0);
        let mm = 3u64 * x.clone() - Expr::make_const(1);
        let e = (3u64 * (x.clone() & mm.clone()) + 3u64 * ((-x) & mm)).reduce(mask);
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
        let e = (x.clone() & (-(disj & (-x.clone())))).reduce(mask);
        apply_patterns(e, mask)
    }

    #[test]
    fn nested_collapses_y_absent() {
        let mask = make_mask(N);
        assert_eq!(nested(var(0), None), var(0).reduce(mask));
        // Works with a compound core `X = v0 + v1`.
        let x = var(0) + var(1);
        assert_eq!(nested(x.clone(), None), x.reduce(mask));
    }

    #[test]
    fn nested_collapses_y_present() {
        let mask = make_mask(N);
        assert_eq!(nested(var(0), Some(var(1))), var(0).reduce(mask));
    }

    #[test]
    fn nested_fires_within_larger_and() {
        let mask = make_mask(N);
        let x = var(0);
        let redundant = -((x.clone()) & (-x.clone()));
        let e = (var(1) & x.clone() & redundant & var(2)).reduce(mask);
        let expected = (var(1) & x.clone() & var(2)).reduce(mask);
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
                let before = (x.clone() & (-(disj & (-x.clone())))).reduce(mask);
                let after = apply_patterns(before.clone(), mask);
                assert!(
                    before.sem_equal(&after, mask, 500).is_ok(),
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
        let e = (var(1) & (-((x.clone()) & (-x.clone())))).reduce(mask);
        assert_eq!(apply_patterns(e.clone(), mask), e);
    }

    #[test]
    fn split_ignores_mismatched_scale() {
        // Different outer scales `a` on the two summands: no collapse.
        let mask = make_mask(N);
        let x = var(0);
        let mm = 6u64 * x.clone() - Expr::make_const(1);
        let e = (3u64 * (x.clone() & mm.clone()) + 5u64 * ((-x) & mm)).reduce(mask);
        let out = apply_patterns(e.clone(), mask);
        assert_eq!(out, e);
    }
}
