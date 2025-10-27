use log::error;
use std::collections::HashMap;

use crate::{expr::Expr, poly::solve_polynomial, symba::is_bitwise};

struct VarExpr {
    forwards: HashMap<usize, Expr>,
    back: HashMap<Expr, usize>,
    id: usize, // The id of the next variable
}

impl VarExpr {
    fn new(t: usize) -> Self {
        Self {
            forwards: HashMap::new(),
            back: HashMap::new(),
            id: t,
        }
    }

    fn make_bitwise(&mut self, e: Expr, n: u32) -> Expr {
        let mask = 1 << 32 - 1;

        if is_bitwise(&e, n) {
            // e is bitwise
            return e;
        }

        let s = solve_non_poly(&e, n);

        // TODO: remove this ?
        if is_bitwise(&s, n) {
            // e is bitwise
            return s;
        }

        if let Some(v) = self.back.get(&s) {
            Expr::Var(*v)
        } else {
            if let Expr::Const(c) = s {
                if let Some(v) = self.back.get(&Expr::Const((!c) & mask)) {
                    return !Expr::Var(*v);
                }
            }
            let v = self.id;
            self.back.insert(s.clone(), v);
            self.forwards.insert(v, s);
            self.id += 1;
            Expr::Var(v)
        }
    }
}

fn np2p(e: Expr, var2expr: &mut VarExpr, n: u32) -> Expr {
    let make_vec_bitwise = |exprs: Vec<Expr>, var2expr: &mut VarExpr| {
        exprs
            .into_iter()
            .map(|e| var2expr.make_bitwise(e, n))
            .collect()
    };

    e.map(|e| match e {
        Expr::Not(expr) => !var2expr.make_bitwise(*expr, n),

        Expr::And(exprs) => Expr::And(make_vec_bitwise(exprs, var2expr)),
        Expr::Or(exprs) => Expr::Or(make_vec_bitwise(exprs, var2expr)),
        Expr::Xor(exprs) => Expr::Xor(make_vec_bitwise(exprs, var2expr)),

        _ => np2p(e, var2expr, n),
    })
}

fn p2np(e: Expr, var2expr: &VarExpr) -> Expr {
    e.map(|e| match e {
        Expr::Var(v) => {
            if let Some(e) = var2expr.forwards.get(&v) {
                e.clone()
            } else {
                Expr::Var(v)
            }
        }
        _ => p2np(e, var2expr),
    })
}

pub fn solve_non_poly(e: &Expr, n: u32) -> Expr {
    let e = e.clone().arith_reduce();

    let mut var2expr = VarExpr::new(e.get_vars().iter().copied().max().unwrap_or(0) + 1);

    // error!("Solving non polynomial problem: {}", e);

    let e = np2p(e, &mut var2expr, n);

    // error!("Created polynomial problem: {}", e);

    let e = solve_polynomial(&e, n);

    // error!("Found polynomial solution: {}", e);

    let e = p2np(e, &var2expr);

    e.arith_reduce().mod_simplify(n)
}
