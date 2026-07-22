//! Browser/Node integration tests for the wasm bindings.
//!
//! Run with `wasm-pack test --node bindings/wasm -- --features parse`.

use wasm::ExprWasm;
use wasm_bindgen_test::*;

/// Builds `x + 3` from the constructors and evaluates it.
#[wasm_bindgen_test]
fn builds_and_evaluates_expression() {
    let x = ExprWasm::new_var(0);
    let three = ExprWasm::new_const(3.0);
    let sum = x.add(vec![three].into_boxed_slice());

    assert_eq!(sum.eval(vec![10].into_boxed_slice(), 64), 13);
}

/// Parses an MBA, simplifies it, and checks it evaluates like `v0 + v1`.
#[cfg(feature = "parse")]
#[wasm_bindgen_test]
fn parses_simplifies_and_evaluates_mba() {
    let mba = ExprWasm::parse("(v0^v1)+2*(v0&v1)").expect("parse failed");
    let simplified = mba.solve(64).expect("solve failed");

    assert_eq!(
        simplified.eval(vec![123, 456].into_boxed_slice(), 64),
        123u64.wrapping_add(456)
    );
}
