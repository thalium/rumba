use crate::expr::Expr;

use super::{Pattern, Tag, are_negations, low_bit_mask_multiple, scale_relation, scaled, split};

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
pub(super) struct LowBitMaskSplit;

impl LowBitMaskSplit {
    /// Splits `X & (m·X - 1)`: returns `(a, a_t, b, C)` for a term `a·(t & M)`
    /// where `t = a_t·C`, `M = m·X - 1` carries even multiple `b` over the same
    /// core `C`, and `b` is an even multiple of `a_t`.
    fn split_masked(term: &Expr, mask: u64) -> Option<(u64, Expr, Expr)> {
        let (a, core) = split(term, mask);
        let Expr::And(conj) = core else {
            return None;
        };
        if conj.len() != 2 {
            return None;
        }
        // One conjunct is the mask `M`, the other is the core term `t`.
        // Besides `m*X-1`, accept the adjacent split at `m*X` itself; for
        // even m both `(X & mX) + (-X & mX) = mX` and the `mX-1` form hold.
        for (i, j) in [(0, 1), (1, 0)] {
            let multiple = low_bit_mask_multiple(&conj[i], mask).unwrap_or_else(|| conj[i].clone());
            let t = conj[j].clone();
            let is_even_multiple = scale_relation(&multiple, &t, mask)
                .is_some_and(|coefficient| coefficient.trailing_zeros() > 0)
                || {
                    let (multiple_factor, multiple_core) = split(&multiple, mask);
                    let (t_factor, t_core) = split(&t, mask);
                    multiple_core == t_core
                        && multiple_factor.trailing_zeros() > t_factor.trailing_zeros()
                };
            if !is_even_multiple {
                continue;
            }
            return Some((a, t, multiple));
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
            let Some((a1, t1, multiple1)) = Self::split_masked(&terms[i], mask) else {
                continue;
            };
            for j in (i + 1)..terms.len() {
                let Some((a2, t2, multiple2)) = Self::split_masked(&terms[j], mask) else {
                    continue;
                };
                // Same outer scale `a`, same mask `M` (core `C` and multiple `b`),
                // and the two core terms negate each other (`a_t` and `-a_t`).
                if a1 != a2 || multiple1 != multiple2 || !are_negations(&t1, &t2, mask) {
                    continue;
                }

                // Replace the pair with the single term `a·m·X = a·b·C`.
                let collapsed = scaled(&multiple1, a1, mask);

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
