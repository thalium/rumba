//! Cost of the two [`LinearCache`] implementations, measured through the real
//! solver rather than against a bare map.
//!
//! `LocalCache` is a plain `RefCell<HashMap>`; `MbaCache` adds a `Mutex` and
//! atomic counters so one cache can back solves on several threads. Both are
//! driven here in the same process, so the comparison is immune to the drift
//! between separate benchmark runs.
//!
//! The hit path is what matters: a cache exists to be hit, and a hit costs only
//! an `Expr` hash — so lock overhead lands against a small denominator there,
//! not against the ~100us solve a miss pays for.

use criterion::{Criterion, criterion_group, criterion_main};
use rumba_core::expr::{Expr, VarId};
use rumba_core::simplify::{LinearCache, LocalCache, MbaCache, simplify_mba_with_cache};

const BITS: u8 = 32;

/// `(v0 ^ v1) + 2*(v0 & v1)` — a small linear MBA equal to `v0 + v1`.
fn mba(a: u32, b: u32) -> Expr {
    let (va, vb) = (Expr::Var(VarId(a as usize)), Expr::Var(VarId(b as usize)));
    Expr::Add(vec![
        Expr::Xor(vec![va.clone(), vb.clone()]),
        Expr::Scale(2u64.into(), Box::new(Expr::And(vec![va, vb]))),
    ])
}

/// Solve the same small set repeatedly: the first pass fills the cache, every
/// later pass is served from it.
fn hammer<C: LinearCache>(cache: &C, exprs: &[Expr]) {
    for e in exprs {
        std::hint::black_box(simplify_mba_with_cache(cache, e.clone(), BITS).unwrap());
    }
}

fn bench_caches(c: &mut Criterion) {
    let exprs: Vec<Expr> = (0..8).map(|i| mba(i, i + 1)).collect();

    let local = LocalCache::new();
    hammer(&local, &exprs); // warm, so the timed runs are all hits
    c.bench_function("solve_hits_local", |b| b.iter(|| hammer(&local, &exprs)));

    let shared = MbaCache::new();
    hammer(&shared, &exprs);
    c.bench_function("solve_hits_mutex", |b| b.iter(|| hammer(&shared, &exprs)));

    // Cold path: a fresh cache each iteration, so every lookup misses and pays
    // for a real solve. Lock overhead should vanish against the solve cost.
    c.bench_function("solve_misses_local", |b| {
        b.iter(|| hammer(&LocalCache::new(), &exprs))
    });
    c.bench_function("solve_misses_mutex", |b| {
        b.iter(|| hammer(&MbaCache::new(), &exprs))
    });
}

criterion_group!(benches, bench_caches);
criterion_main!(benches);
