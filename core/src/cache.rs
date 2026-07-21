//! Memoization of solved linear MBAs.
//!
//! The solver reaches its cache at exactly one point: a [`get`](LinearCache::get),
//! and on a miss an [`insert`](LinearCache::insert), with the expensive solve
//! running *between* them. Two implementations are provided: [`LocalCache`] for a
//! single-threaded caller, and [`MbaCache`] for one shared across threads.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::expr::Expr;

/// How a cache has performed. Read with [`MbaCache::stats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    /// Distinct linear MBAs currently memoized.
    pub entries: usize,
}

impl CacheStats {
    /// Fraction of lookups served from the cache, or `None` before any lookup.
    pub fn hit_rate(&self) -> Option<f64> {
        let total = self.hits + self.misses;
        (total > 0).then(|| self.hits as f64 / total as f64)
    }
}

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

    /// Hit/miss tallies and current size.
    fn stats(&self) -> CacheStats;

    /// Drop every entry and reset the tallies.
    fn clear(&self);
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
    hits: Cell<u64>,
    misses: Cell<u64>,
}

impl LocalCache {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LinearCache for LocalCache {
    fn get(&self, e: &Expr) -> Option<Expr> {
        let hit = self.entries.borrow().get(e).cloned();

        let counter = match &hit {
            Some(_) => &self.hits,
            None => &self.misses,
        };
        counter.set(counter.get() + 1);

        hit
    }

    fn insert(&self, e: Expr, solved: Expr) {
        self.entries.borrow_mut().insert(e, solved);
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.get(),
            misses: self.misses.get(),
            entries: self.entries.borrow().len(),
        }
    }

    fn clear(&self) {
        self.entries.borrow_mut().clear();
        self.hits.set(0);
        self.misses.set(0);
    }
}

/// A [`LinearCache`] shareable across threads, and across calls to
/// [`simplify_mba_with_cache`](crate::simplify::simplify_mba_with_cache).
///
/// Critical sections are one hash-map operation each — the solve itself runs
/// outside the lock — so contention stays low even with many workers.
///
/// A caller that never shares should still prefer [`LocalCache`]. The lock and
/// the atomics add roughly 35ns per lookup (~99ns to ~134ns, `benches/cache.rs`).
/// Against a miss, which pays for a full solve, that is nothing; against a hit,
/// which is only an `Expr` hash, it is about a third — and a cache exists to be
/// hit. Measured end to end through the solver the difference came out near 8%
/// on an all-hits workload.
#[derive(Debug, Default)]
pub struct MbaCache {
    entries: Mutex<HashMap<Expr, Expr>>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl MbaCache {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LinearCache for MbaCache {
    fn get(&self, e: &Expr) -> Option<Expr> {
        let hit = self
            .entries
            .lock()
            .expect("MBA cache mutex poisoned")
            .get(e)
            .cloned();

        match &hit {
            Some(_) => &self.hits,
            None => &self.misses,
        }
        .fetch_add(1, Ordering::Relaxed);

        hit
    }

    fn insert(&self, e: Expr, solved: Expr) {
        self.entries
            .lock()
            .expect("MBA cache mutex poisoned")
            .insert(e, solved);
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            entries: self.entries.lock().expect("MBA cache mutex poisoned").len(),
        }
    }

    fn clear(&self) {
        self.entries
            .lock()
            .expect("MBA cache mutex poisoned")
            .clear();
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
    }
}
