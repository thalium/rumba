use std::collections::HashMap;

use crate::{expr::Expr, poly::solve_polynomial};

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

    fn make_bitwise(&mut self, e: Expr) -> Expr {
        let mask = 1 << 32 - 1;

        if e.is_bitwise() {
            return e;
        }

        let s = solve_non_poly(&e);

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

fn np2p(e: Expr, var2expr: &mut VarExpr) -> Expr {
    let make_vec_polynomial = |exprs: Vec<Expr>, var2expr: &mut VarExpr| {
        exprs
            .into_iter()
            .map(|e| var2expr.make_bitwise(e))
            .collect()
    };

    e.map(|e| match e {
        Expr::Not(expr) => !var2expr.make_bitwise(*expr),

        Expr::And(exprs) => Expr::And(make_vec_polynomial(exprs, var2expr)),
        Expr::Or(exprs) => Expr::Or(make_vec_polynomial(exprs, var2expr)),
        Expr::Xor(exprs) => Expr::Xor(make_vec_polynomial(exprs, var2expr)),

        _ => np2p(e, var2expr),
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

pub fn solve_non_poly(e: &Expr) -> Expr {
    let mut var2expr = VarExpr::new(e.get_vars().iter().copied().max().unwrap_or(0) + 1);

    let e = np2p(e.clone(), &mut var2expr);

    println!("Created polynomial problem: {}", e);

    let e = solve_polynomial(&e);

    println!("Found polynomial solution: {}", e);

    let e = p2np(e, &var2expr);

    e.arith_reduce()
}
