#![cfg(all(feature = "parse", feature = "jit"))]

use criterion::{Criterion, criterion_group, criterion_main};
use rand::random_range;
use rumba_core::{
    expr::Expr,
    jit::{self, JitFunction},
    parser::parse_expr,
};

fn jit_compile(e: &Expr) {
    let jit_fn = jit::compile(e);
    std::hint::black_box(jit_fn);
}

fn bench_jit_compilation(c: &mut Criterion) {
    let e = parse_expr("1*~(v0&v1)+3*(v0|~v1)+2*v0-1*~(v0|~v1)-6*(v0&~v1)-5*(v0&v1)").unwrap();

    c.bench_function("jit_compile", |b| b.iter(|| jit_compile(&e)));
}

fn eval(e: &Expr, data: &Vec<Vec<u64>>) {
    for v in data {
        let res = e.eval(v);
        std::hint::black_box(res);
    }
}

fn bench_eval(c: &mut Criterion) {
    let data: Vec<Vec<u64>> = (0..512)
        .map(|_| vec![random_range(0..u64::MAX), random_range(0..u64::MAX)])
        .collect();

    let e = parse_expr("1*~(v0&v1)+3*(v0|~v1)+2*v0-1*~(v0|~v1)-6*(v0&~v1)-5*(v0&v1)").unwrap();

    c.bench_function("eval", |b| b.iter(|| eval(&e, &data)));
}

fn jit_eval(jit_fn: &JitFunction, data: &Vec<Vec<u64>>) {
    for v in data {
        let res = jit_fn.eval(v);
        std::hint::black_box(res);
    }
}

fn bench_jit_eval(c: &mut Criterion) {
    let data: Vec<Vec<u64>> = (0..512)
        .map(|_| vec![random_range(0..u64::MAX), random_range(0..u64::MAX)])
        .collect();

    let e = parse_expr("1*~(v0&v1)+3*(v0|~v1)+2*v0-1*~(v0|~v1)-6*(v0&~v1)-5*(v0&v1)").unwrap();
    let jit_fn = jit::compile(&e);

    c.bench_function("jit_eval", |b| b.iter(|| jit_eval(&jit_fn, &data)));
}

criterion_group!(benches, bench_jit_compilation, bench_eval, bench_jit_eval);
criterion_main!(benches);
