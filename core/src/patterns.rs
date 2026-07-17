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

/// Splits a (reduced) expression into its scalar coefficient and core.
///
/// `reduce` guarantees no nested `Scale(Scale(..))`, so a single strip is
/// enough. A bare (non-`Scale`) expression has an implicit coefficient of 1.
fn split(e: &Expr, mask: u64) -> (u64, &Expr) {
    match e {
        Expr::Scale(c, inner) => (c.get(mask), inner.as_ref()),
        _ => (1, e),
    }
}

/// Recognizes the reduced form of a low-bit mask `m·X - 1`.
///
/// After `reduce`, `m·X - 1` is `Add([Const(-1), Scale(m, C)])`: a `-1` constant
/// (`mask`, i.e. all ones) plus a single scaled core. Returns `(m, C)` — the
/// even-multiple coefficient and the core `C` (`X`'s core) — on a match.
fn as_low_bit_mask(e: &Expr, mask: u64) -> Option<(u64, &Expr)> {
    let Expr::Add(terms) = e else {
        return None;
    };
    if terms.len() != 2 {
        return None;
    }
    let mut neg_one = false;
    let mut mult: Option<(u64, &Expr)> = None;
    for t in terms {
        match t {
            Expr::Const(c) if c.get(mask) == mask => neg_one = true,
            _ => mult = Some(split(t, mask)),
        }
    }
    match (mult, neg_one) {
        (Some(m), true) => Some(m),
        _ => None,
    }
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

        // Gate: a match needs the bare `X`, `-X` and `m·X`, and the latter two
        // are always `Scale`s. Bail cheaply otherwise (no allocation).
        if children.len() < 3 {
            return None;
        }
        if children
            .iter()
            .filter(|c| matches!(c, Expr::Scale(_, _)))
            .count()
            < 2
        {
            return None;
        }

        // The candidate cores are the inners of the `Scale` children: `-X` and
        // `m·X` always appear scaled, so every real match's core is found here.
        for cand in children {
            let Expr::Scale(_, core) = cand else {
                continue;
            };
            let core = core.as_ref();

            // Collect the coefficients of every term sharing this core.
            //
            // A bare `X` whose core is itself an `And` is flattened into the
            // parent by `reduce` (its conjuncts become siblings), so it has no
            // single child. When all of its conjuncts are present as siblings,
            // the coefficient-1 term is nonetheless available.
            let bare_present = match core {
                Expr::And(elems) => elems.iter().all(|el| children.contains(el)),
                _ => children.contains(core),
            };

            for a_child in children {
                let (a, a_core) = split(a_child, mask);
                if a_core != core {
                    continue;
                }
                // `a` is the coefficient of the candidate `X`. Skip the implicit
                // coefficient-1 term unless a bare `X` is actually present.
                if a == 1 && !bare_present && !matches!(a_child, Expr::Scale(_, _)) {
                    continue;
                }

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

                for other in children {
                    let (b, b_core) = split(other, mask);
                    if b_core == core {
                        visit(b);
                    }
                }
                if bare_present {
                    visit(1);
                }

                if has_neg && has_even_multiple {
                    return Some(Expr::zero());
                }
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

        for (idx, cand) in children.iter().enumerate() {
            // The redundant conjunct is the reduced `m·X - 1`.
            let Some((m, core)) = as_low_bit_mask(cand, mask) else {
                continue;
            };

            // `X & -X` must be isolated by a sibling pair `a·C` and `-a·C`, with
            // `m` an even multiple of `a`.
            for a_child in children {
                let (a, a_core) = split(a_child, mask);
                if a_core != core || m.trailing_zeros() <= a.trailing_zeros() {
                    continue;
                }
                let neg = a.wrapping_neg() & mask; // -a mod 2ⁿ
                let has_neg = children.iter().any(|o| {
                    let (b, b_core) = split(o, mask);
                    b_core == core && b == neg
                });
                if !has_neg {
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
    fn split_masked<'a>(term: &'a Expr, mask: u64) -> Option<(u64, u64, u64, &'a Expr)> {
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
                let coeff = crate::varint::VarInt::from(a1) * crate::varint::VarInt::from(b1);
                let collapsed = Expr::scale(coeff.mask(mask), core1.clone());

                let mut kept: Vec<Expr> = terms
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| *k != i && *k != j)
                    .map(|(_, t)| t.clone())
                    .collect();
                if kept.is_empty() {
                    return Some(collapsed);
                }
                kept.push(collapsed);
                return Some(Expr::Add(kept).reduce(mask));
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
