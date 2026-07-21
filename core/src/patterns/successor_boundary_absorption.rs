use crate::expr::Expr;

use super::{Pattern, Tag, rebuild_and, scale_relation};

/// If `A = X + 1`, every set bit of `A & (m*A)` for even `m` is above A's
/// lowest set bit and is therefore also present in `A - 1 = X`:
///
/// `X & (X + 1) & (m * (X + 1)) = (X + 1) & (m * (X + 1))`.
pub(super) struct SuccessorBoundaryAbsorption;

impl Pattern for SuccessorBoundaryAbsorption {
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

        for (predecessor_idx, predecessor) in children.iter().enumerate() {
            let successor = (predecessor.clone() + Expr::make_const(1)).reduce_masked(mask);
            let Some(successor_idx) = children
                .iter()
                .enumerate()
                .find(|(idx, child)| *idx != predecessor_idx && **child == successor)
                .map(|(idx, _)| idx)
            else {
                continue;
            };
            if children.iter().enumerate().any(|(idx, child)| {
                idx != predecessor_idx
                    && idx != successor_idx
                    && scale_relation(child, &successor, mask)
                        .is_some_and(|coefficient| coefficient.trailing_zeros() > 0)
            }) {
                let mut kept = children.clone();
                kept.remove(predecessor_idx);
                return Some(rebuild_and(kept));
            }
        }

        // If `X` is itself a conjunction, reduction flattens it into the
        // parent and there is no direct predecessor child. Restrict this more
        // expensive path to successors for which `A - 1` is exactly an And.
        for (successor_idx, successor) in children.iter().enumerate() {
            let predecessor = (successor.clone() - Expr::make_const(1)).reduce_masked(mask);
            let Expr::And(factors) = predecessor else {
                continue;
            };
            let mut used = vec![false; children.len()];
            used[successor_idx] = true;
            let mut redundant_indices = Vec::with_capacity(factors.len());
            for factor in &factors {
                let Some((idx, _)) = children
                    .iter()
                    .enumerate()
                    .find(|(idx, child)| !used[*idx] && *child == factor)
                else {
                    redundant_indices.clear();
                    break;
                };
                used[idx] = true;
                redundant_indices.push(idx);
            }
            if redundant_indices.len() != factors.len() {
                continue;
            }
            if !children.iter().enumerate().any(|(idx, child)| {
                !used[idx]
                    && scale_relation(child, successor, mask)
                        .is_some_and(|coefficient| coefficient.trailing_zeros() > 0)
            }) {
                continue;
            }
            let kept: Vec<_> = children
                .iter()
                .enumerate()
                .filter(|(idx, _)| !redundant_indices.contains(idx))
                .map(|(_, child)| child.clone())
                .collect();
            return Some(rebuild_and(kept));
        }
        None
    }
}
