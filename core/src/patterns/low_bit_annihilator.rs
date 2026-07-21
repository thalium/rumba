use crate::expr::Expr;

use super::{Pattern, Tag, are_negations, scale_relation, split};

/// `X & (-X) & (m·X) = 0` for any expression `X` and any even `m`.
///
/// `X & -X` isolates the single lowest set bit of `X`, at position `p`. Bit `p`
/// of `m·X` equals the parity of `m`, so any even multiple of `X` is zero at
/// that bit and the whole conjunction collapses to `0`.
///
/// Writing `X = a·C`, the three terms appear as children with the same core `C`
/// and coefficients `a`, `-a`, and `b`, where `b` is an even multiple of `a`
/// modulo `2ⁿ`. The latter is exactly `trailing_zeros(b) > trailing_zeros(a)`.
pub(super) struct LowBitAnnihilator;

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

        // The factor/core path above cannot factor an affine expression whose
        // reduced terms have mixed coefficients, e.g. X = x - y. Compare the
        // complete canonical additive forms as a fallback.
        for (i, x) in children.iter().enumerate() {
            if !children
                .iter()
                .enumerate()
                .any(|(j, other)| i != j && are_negations(x, other, mask))
            {
                continue;
            }
            if children.iter().enumerate().any(|(j, other)| {
                i != j
                    && scale_relation(other, x, mask)
                        .is_some_and(|coefficient| coefficient.trailing_zeros() > 0)
            }) {
                return Some(Expr::zero());
            }
        }

        None
    }
}
