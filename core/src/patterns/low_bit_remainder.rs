use crate::expr::Expr;

use super::{Pattern, Tag, is_low_bit_of, rebuild_and, split};

/// Drops `X` from `X & (X - (X & -X))`: subtracting the low bit only clears a
/// bit already present in `X`, so the remainder is a bitwise subset of `X`.
/// Also recognizes the dual reconstruction
/// `X & (-1 - X + (X & -X)) = X & -X`.
pub(super) struct LowBitRemainder;

impl Pattern for LowBitRemainder {
    fn tags(&self) -> &'static [Tag] {
        &[Tag::And]
    }

    fn apply(&self, e: &Expr, mask: u64) -> Option<Expr> {
        let Expr::And(children) = e else {
            return None;
        };

        for (x_idx, x) in children.iter().enumerate() {
            for (other_idx, other) in children.iter().enumerate() {
                if x_idx == other_idx {
                    continue;
                }
                let Expr::Add(terms) = other else {
                    continue;
                };
                for low_bit_term in terms {
                    let (_, low_bit) = split(low_bit_term, mask);
                    if !is_low_bit_of(&low_bit, x, mask) {
                        continue;
                    }

                    let remainder = (x.clone() - low_bit.clone()).reduce(mask);
                    if *other == remainder {
                        let mut kept = children.clone();
                        kept.remove(x_idx);
                        return Some(rebuild_and(kept));
                    }

                    let reconstruction =
                        (-Expr::make_const(1) - x.clone() + low_bit.clone()).reduce(mask);
                    if *other == reconstruction {
                        let mut kept: Vec<_> = children
                            .iter()
                            .enumerate()
                            .filter(|(idx, _)| *idx != x_idx && *idx != other_idx)
                            .map(|(_, child)| child.clone())
                            .collect();
                        kept.push(low_bit.clone());
                        return Some(rebuild_and(kept).reduce(mask));
                    }
                }
            }
        }
        None
    }
}
