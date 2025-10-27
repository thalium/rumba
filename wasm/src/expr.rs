use std::panic;

use mbalib::{
    anf::ANFExpr,
    blast::blast,
    expr::{Binop, Expr},
    mcts::{IO, MCTS, Node},
    nonpoly::solve_non_poly,
    parser::parse_expr,
    poly::solve_polynomial,
    symba::{self},
};
use wasm_bindgen::prelude::*;

use crate::anf::ANFWrapper;

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
    #[wasm_bindgen(constructor)]
    pub fn new_var(i: usize) -> ExprWasm {
        ExprWasm {
            inner: Expr::Var(i),
        }
    }

    #[wasm_bindgen]
    pub fn new_const(n: f64) -> ExprWasm {
        if n < 0.0 {
            ExprWasm {
                inner: Expr::Neg(Box::new(Expr::Const((-n) as u128))),
            }
        } else {
            ExprWasm {
                inner: Expr::Const(n as u128),
            }
        }
    }

    #[wasm_bindgen]
    pub fn parse(s: &str) -> Result<ExprWasm, JsValue> {
        match parse_expr(s) {
            Ok(inner) => Ok(ExprWasm { inner }),
            Err(e) => Err(JsValue::from_str(&e)),
        }
    }

    #[wasm_bindgen]
    pub fn to_string(&self) -> String {
        self.inner.to_string()
    }

    #[wasm_bindgen]
    pub fn blast(&self) -> Self {
        Self {
            inner: blast(self.inner.clone()),
        }
    }

    #[wasm_bindgen]
    pub fn solve(&self, n: u32) -> Self {
        Self {
            inner: symba::solve_linear(&self.inner, n),
        }
    }

    #[wasm_bindgen]
    pub fn solvep(&self, n: u32) -> Self {
        Self {
            inner: solve_polynomial(&self.inner, n),
        }
    }

    #[wasm_bindgen]
    pub fn solvenp(&self, n: u32) -> Self {
        Self {
            inner: solve_non_poly(&self.inner, n),
        }
    }

    #[wasm_bindgen]
    pub fn mod_simplify(&self, n: u32) -> Self {
        Self {
            inner: self.inner.clone().mod_simplify(n).arith_reduce(),
        }
    }

    #[wasm_bindgen]
    pub fn anf(&self, n: usize) -> Vec<ANFWrapper> {
        let a: ANFExpr = (self.inner.clone(), n).into();
        a.bits.into_iter().map(|a| a.clone().into()).collect()
    }

    // Unary
    #[wasm_bindgen]
    pub fn not(&self) -> ExprWasm {
        ExprWasm {
            inner: Expr::Not(Box::new(self.inner.clone())),
        }
    }

    #[wasm_bindgen]
    pub fn neg(&self) -> ExprWasm {
        ExprWasm {
            inner: Expr::Neg(Box::new(self.inner.clone())),
        }
    }

    #[wasm_bindgen]
    pub fn and(&self, others: Box<[ExprWasm]>) -> ExprWasm {
        let mut v: Vec<Expr> = others.iter().map(|e| e.inner.clone()).collect();
        v.push(self.inner.clone());
        ExprWasm {
            inner: Expr::And(v),
        }
    }

    #[wasm_bindgen]
    pub fn or(&self, others: Box<[ExprWasm]>) -> ExprWasm {
        let mut v: Vec<Expr> = others.iter().map(|e| e.inner.clone()).collect();
        v.push(self.inner.clone());
        ExprWasm { inner: Expr::Or(v) }
    }

    #[wasm_bindgen]
    pub fn xor(&self, others: Box<[ExprWasm]>) -> ExprWasm {
        let mut v: Vec<Expr> = others.iter().map(|e| e.inner.clone()).collect();
        v.push(self.inner.clone());
        ExprWasm {
            inner: Expr::Xor(v),
        }
    }

    #[wasm_bindgen]
    pub fn shl(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::Shl(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }

    #[wasm_bindgen]
    pub fn shr(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::Shr(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }

    #[wasm_bindgen]
    pub fn rshift_s(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::RshiftS(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }

    // Comparison
    #[wasm_bindgen]
    pub fn le(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::Le(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }
    #[wasm_bindgen]
    pub fn lt(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::Lt(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }
    #[wasm_bindgen]
    pub fn ge(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::Ge(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }
    #[wasm_bindgen]
    pub fn gt(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::Gt(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }
    #[wasm_bindgen]
    pub fn le_s(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::LeS(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }
    #[wasm_bindgen]
    pub fn lt_s(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::LtS(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }
    #[wasm_bindgen]
    pub fn ge_s(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::GeS(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }
    #[wasm_bindgen]
    pub fn gt_s(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::GtS(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }
    #[wasm_bindgen]
    pub fn ne(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::Ne(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }
    #[wasm_bindgen]
    pub fn eq(&self, left: &ExprWasm, right: &ExprWasm) -> ExprWasm {
        ExprWasm {
            inner: Expr::Eq(Binop::new(left.inner.clone(), right.inner.clone())),
        }
    }

    // Arithmetic
    #[wasm_bindgen]
    pub fn add(&self, others: Box<[ExprWasm]>) -> ExprWasm {
        let mut v: Vec<Expr> = others.iter().map(|e| e.inner.clone()).collect();
        v.push(self.inner.clone());
        ExprWasm {
            inner: Expr::Add(v),
        }
    }

    #[wasm_bindgen]
    pub fn sub(&self, others: Box<[ExprWasm]>) -> ExprWasm {
        let mut v: Vec<Expr> = others.iter().map(|e| e.inner.clone()).collect();
        v.push(self.inner.clone());
        ExprWasm {
            inner: Expr::Sub(v),
        }
    }

    #[wasm_bindgen]
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
    pub fn eval(&self, vars: Box<[f64]>, bits: f64) -> f64 {
        let vars: Vec<u128> = vars.iter().map(|v| *v as u128).collect();
        (self.inner.eval(&vars) % 2u128.pow(bits as u32)) as f64
    }

    #[wasm_bindgen]
    pub fn truth_table(&self, n: usize, t: usize) -> Box<[f64]> {
        self.inner
            .truth_table(n, t)
            .into_iter()
            .map(|v| v as f64)
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    #[wasm_bindgen]
    pub fn repr(&self, n: u32, hex: bool, latex: bool) -> String {
        self.inner.repr(n, hex, latex)
    }

    #[wasm_bindgen]
    pub fn is_bitwise(&self) -> bool {
        self.inner.is_bitwise()
    }

    #[wasm_bindgen]
    pub fn variables_in(&self, vars: Box<[f64]>) -> bool {
        let vars: Vec<usize> = vars.iter().map(|v| *v as usize).collect();
        self.inner.variables_in(&vars)
    }

    #[wasm_bindgen]
    pub fn simplify(&self) -> ExprWasm {
        ExprWasm {
            inner: self.inner.clone().arith_reduce(),
        }
    }
}

#[wasm_bindgen(getter_with_clone)]
pub struct MCSTNodeWasm {
    pub id: usize,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub expression: ExprWasm,
    pub score: f64,
    pub visits: usize,
    pub active: bool,
}

impl From<&Node> for MCSTNodeWasm {
    fn from(node: &Node) -> Self {
        MCSTNodeWasm {
            id: node.id,
            parent: node.parent,
            children: node.children.clone(),
            expression: ExprWasm {
                inner: node.expression.clone(),
            },
            score: node.score,
            visits: node.visits,
            active: node.active,
        }
    }
}

#[wasm_bindgen]
pub struct IOWasm {
    pub v0: u128,
    pub v1: u128,
    pub out: u128,
}

impl From<&IO> for IOWasm {
    fn from(io: &IO) -> Self {
        Self {
            v0: io.v0,
            v1: io.v1,
            out: io.out,
        }
    }
}
#[wasm_bindgen]
pub struct MCTSWasm {
    inner: MCTS,
}

#[wasm_bindgen]
impl MCTSWasm {
    #[wasm_bindgen(constructor)]
    pub fn new(n: usize, expr: &ExprWasm) -> Self {
        let inner = MCTS::new(n, expr.inner.clone());
        Self { inner }
    }

    pub fn run(&mut self) -> ExprWasm {
        let result = self.inner.run();
        ExprWasm { inner: result }
    }

    pub fn get_nodes(&self) -> Vec<MCSTNodeWasm> {
        self.inner.nodes.iter().map(MCSTNodeWasm::from).collect()
    }

    pub fn get_io(&self) -> Vec<IOWasm> {
        self.inner.ios.iter().map(IOWasm::from).collect()
    }
}
