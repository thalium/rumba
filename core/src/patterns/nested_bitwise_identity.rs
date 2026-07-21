use crate::expr::Expr;
use crate::varint::VarInt;

use super::{Pattern, Tag};

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
pub(super) struct NestedBitwiseIdentity;

impl NestedBitwiseIdentity {
    /// `-e`, canonicalized, so it compares equal to a reduced sibling.
    fn negate(e: &Expr, mask: u64) -> Expr {
        Expr::scale(VarInt::from(mask), e.clone()).reduce_masked(mask)
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
                        kept.remove(0)
                    } else {
                        Expr::And(kept)
                    });
                }
            }
        }

        None
    }
}
