//! Memoization of solved linear MBAs.
//!
//! The solver reaches its cache at exactly one point: a [`get`](LinearCache::get),
//! and on a miss an [`insert`](LinearCache::insert), with the expensive solve
//! running *between* them. Two implementations are provided: [`LocalCache`] for a
//! single-threaded caller, and [`MbaCache`] for one shared across threads.

use std::{cell::RefCell, collections::HashMap, sync::Mutex};

use crate::expr::Expr;

/// A memo of solved linear MBAs.
///
/// The solver reaches its cache at exactly one point (`MBASolver::solve_linear`):
/// a [`get`](LinearCache::get), and on a miss an [`insert`](LinearCache::insert),
/// with the expensive solve running *between* them. Two implementations are
/// provided: [`LocalCache`] for a single-threaded caller, and [`MbaCache`] for one
/// shared across threads.
///
/// [`get`](LinearCache::get) hands back an owned `Expr` rather than a guard or a
/// borrow. That is deliberate: the solver recurses into itself (`hide_in_var`
/// re-enters `simplify_mba_inner` with this same cache), and neither [`RefCell`]
/// nor [`Mutex`] tolerates a live guard across such a call — one panics, the
/// other deadlocks. Returning owned values makes that unrepresentable.
pub trait LinearCache {
    /// The memoized solution for `e`, tallying the lookup as a hit or a miss.
    fn get(&self, e: &Expr) -> Option<Expr>;

    /// Memoize `solved` as the solution for `e`.
    fn insert(&self, e: Expr, solved: Expr);
}

/// A [`LinearCache`] for one thread: no locking, no atomics.
///
/// This is what [`simplify_mba`](crate::simplify::simplify_mba) uses. Accessing it
/// costs a borrow-flag check, so a single-threaded caller pays nothing for a
/// sharing capability it does not use. Not [`Sync`] — use [`MbaCache`] to share one
/// across threads.
#[derive(Debug, Default)]
pub struct LocalCache {
    entries: RefCell<HashMap<Expr, Expr>>,
}

impl LocalCache {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LinearCache for LocalCache {
    fn get(&self, e: &Expr) -> Option<Expr> {
        self.entries.borrow().get(e).cloned()
    }

    fn insert(&self, e: Expr, solved: Expr) {
        self.entries.borrow_mut().insert(e, solved);
    }
}

/// A [`LinearCache`] shareable across threads, and across calls to
/// [`simplify_mba_cached`](crate::simplify::simplify_mba_cached). This is what
/// the public [`SimplifyCache`](crate::simplify::SimplifyCache) wraps.
///
/// Critical sections are one hash-map operation each — the solve itself runs
/// outside the lock — so contention stays low even with many workers.
///
/// A caller that never shares should still prefer [`LocalCache`]: the lock adds
/// roughly 35ns per lookup. Against a miss, which pays for a full solve, that is
/// nothing; against a hit, which is only an `Expr` hash, it is about a third —
/// and a cache exists to be hit.
#[derive(Debug, Default)]
pub struct MbaCache {
    entries: Mutex<HashMap<Expr, Expr>>,
}

impl LinearCache for MbaCache {
    fn get(&self, e: &Expr) -> Option<Expr> {
        self.entries
            .lock()
            .expect("MBA cache mutex poisoned")
            .get(e)
            .cloned()
    }

    fn insert(&self, e: Expr, solved: Expr) {
        self.entries
            .lock()
            .expect("MBA cache mutex poisoned")
            .insert(e, solved);
    }
}
