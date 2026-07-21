use crate::expr::Expr;

use super::{
    Pattern, Tag, are_negations, as_low_bit_mask, low_bit_mask_multiple, scale_relation, split,
};

/// `(X & -X) & (m·X - 1) = X & -X` for any expression `X` and any even `m`.
///
/// `X & -X` isolates the single lowest set bit of `X`, at position `p`. As in
/// [`LowBitAnnihilator`](super::low_bit_annihilator::LowBitAnnihilator), an even
/// multiple `m·X` is zero across bits `0..=p`, so `m·X - 1` borrows all the way up
/// and is *one* at bit `p`. Conjoining bit `p` with something that is set at bit
/// `p` leaves it unchanged, so the `m·X - 1` term is redundant and can be dropped.
///
/// Writing `X = a·C`, the isolating pair appears as two children with core `C`
/// and coefficients `a` and `-a`, while the redundant term is the reduced form
/// of `m·X - 1`, i.e. `Add([Const(-1), Scale(m, C)])`, with `m` an even multiple
/// of `a` (`trailing_zeros(m) > trailing_zeros(a)`).
pub(super) struct LowBitRedundantMask;

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
                // As above, compare full canonical affine forms when a common
                // scalar cannot be factored syntactically.
                let Some(multiple) = low_bit_mask_multiple(cand, mask) else {
                    continue;
                };
                let affine_isolated = children.iter().enumerate().any(|(i, x)| {
                    i != idx
                        && scale_relation(&multiple, x, mask)
                            .is_some_and(|coefficient| coefficient.trailing_zeros() > 0)
                        && children
                            .iter()
                            .enumerate()
                            .any(|(j, other)| j != idx && j != i && are_negations(x, other, mask))
                });
                if !affine_isolated {
                    continue;
                }
            }

            // Drop the redundant conjunct; the rest is already canonical.
            let mut kept = children.clone();
            kept.remove(idx);
            return Some(if kept.len() == 1 {
                kept.remove(0)
            } else {
                Expr::And(kept)
            });
        }

        None
    }
}
