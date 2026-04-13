use std::panic;

use rumba_core::{expr::Expr, simplify, varint::make_mask};

#[cfg(feature = "parse")]
use rumba_core::parser::parse_expr;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main() {
    console_log::init().unwrap();
    panic::set_hook(Box::new(console_error_panic_hook::hook));
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct ExprWasm {
    inner: Expr,
}

#[wasm_bindgen]
impl ExprWasm {
    #[wasm_bindgen]
    pub fn new_var(i: usize) -> ExprWasm {
        ExprWasm {
            inner: Expr::Var(i.into()),
        }
    }

    #[wasm_bindgen]
    pub fn new_const(n: f64) -> ExprWasm {
        if n < 0.0 {
            ExprWasm {
                inner: -Expr::Const(((-n) as u64).into()),
            }
        } else {
            ExprWasm {
                inner: Expr::Const((n as u64).into()),
            }
        }
    }

    #[cfg(feature = "parse")]
    #[wasm_bindgen]
    pub fn parse(s: &str) -> Result<ExprWasm, JsValue> {
        match parse_expr(s) {
            Ok(inner) => Ok(ExprWasm { inner }),
            Err(e) => Err(JsValue::from_str(&e)),
        }
    }

    #[wasm_bindgen]
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        self.inner.to_string()
    }

    #[wasm_bindgen]
    pub fn solve(&self, n: u8) -> Self {
        Self {
            inner: simplify::simplify_mba(self.inner.clone(), n),
        }
    }

    #[wasm_bindgen]
    pub fn reduce(&self, n: u8) -> Self {
        Self {
            inner: self.inner.clone().reduce(make_mask(n)),
        }
    }

    // Unary
    #[wasm_bindgen]
    pub fn not(&self) -> ExprWasm {
        ExprWasm {
            inner: !self.inner.clone(),
        }
    }

    #[wasm_bindgen]
    pub fn neg(&self) -> ExprWasm {
        ExprWasm {
            inner: -self.inner.clone(),
        }
    }

    #[wasm_bindgen]
    #[allow(clippy::boxed_local)]
    pub fn and(&self, others: Box<[ExprWasm]>) -> ExprWasm {
        let mut v: Vec<Expr> = others.iter().map(|e| e.inner.clone()).collect();
        v.push(self.inner.clone());
        ExprWasm {
            inner: Expr::And(v),
        }
    }

    #[wasm_bindgen]
    #[allow(clippy::boxed_local)]
    pub fn or(&self, others: Box<[ExprWasm]>) -> ExprWasm {
        let mut v: Vec<Expr> = others.iter().map(|e| e.inner.clone()).collect();
        v.push(self.inner.clone());
        ExprWasm { inner: Expr::Or(v) }
    }

    #[wasm_bindgen]
    #[allow(clippy::boxed_local)]
    pub fn xor(&self, others: Box<[ExprWasm]>) -> ExprWasm {
        let mut v: Vec<Expr> = others.iter().map(|e| e.inner.clone()).collect();
        v.push(self.inner.clone());
        ExprWasm {
            inner: Expr::Xor(v),
        }
    }

    // Arithmetic
    #[wasm_bindgen]
    #[allow(clippy::boxed_local)]
    pub fn add(&self, others: Box<[ExprWasm]>) -> ExprWasm {
        let mut v: Vec<Expr> = others.iter().map(|e| e.inner.clone()).collect();
        v.push(self.inner.clone());
        ExprWasm {
            inner: Expr::Add(v),
        }
    }

    #[wasm_bindgen]
    #[allow(clippy::boxed_local)]
    pub fn sub(&self, others: Box<[ExprWasm]>) -> ExprWasm {
        let mut v: Vec<Expr> = others.iter().map(|e| -e.inner.clone()).collect();
        v.push(self.inner.clone());
        ExprWasm {
            inner: Expr::Add(v),
        }
    }

    #[wasm_bindgen]
    #[allow(clippy::boxed_local)]
    pub fn mul(&self, others: Box<[ExprWasm]>) -> ExprWasm {
        let mut v: Vec<Expr> = others.iter().map(|e| e.inner.clone()).collect();
        v.push(self.inner.clone());
        ExprWasm {
            inner: Expr::Mul(v),
        }
    }

    // Other utility methods
    #[wasm_bindgen]
    pub fn size(&self) -> usize {
        self.inner.size()
    }

    #[wasm_bindgen]
    #[allow(clippy::boxed_local)]
    pub fn eval(&self, vars: Box<[u64]>, bits: u8) -> u64 {
        let vars = vars.to_vec();
        self.inner.eval(&vars).get(make_mask(bits))
    }

    #[wasm_bindgen]
    pub fn truth_table(&self, n: u8, t: usize) -> Box<[u64]> {
        let mask = make_mask(n);
        self.inner.truth_table(t, mask).into_boxed_slice()
    }

    #[wasm_bindgen]
    pub fn repr(&self, n: u8, hex: bool, latex: bool) -> String {
        self.inner.repr(n, make_mask(n), hex, latex)
    }

    #[wasm_bindgen]
    pub fn is_bitwise(&self) -> bool {
        self.inner.is_bitwise()
    }

    #[wasm_bindgen]
    #[allow(clippy::boxed_local)]
    pub fn variables_in(&self, vars: Box<[f64]>) -> bool {
        let vars: Vec<usize> = vars.iter().map(|v| *v as usize).collect();
        self.inner.variables_in(&vars)
    }

    #[wasm_bindgen]
    pub fn simplify(&self, n: u8) -> ExprWasm {
        ExprWasm {
            inner: self.inner.clone().reduce(make_mask(n)),
        }
    }
}
