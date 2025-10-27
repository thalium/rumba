use std::collections::HashMap;

use crate::{
    expr::{Binop, Expr},
    symba::solve_linear,
};

struct DegVar {
    forwards: HashMap<(usize, usize), usize>,
    back: HashMap<usize, (usize, usize)>,
    id: usize, // The id of the next variable
    t: usize,
    d: usize,
}

impl DegVar {
    fn new(id: usize) -> Self {
        Self {
            forwards: Default::default(),
            back: Default::default(),
            id,
            t: id,
            d: 0,
        }
    }

    fn encode(&mut self, var: usize, deg: usize) -> usize {
        if let Some(v) = self.forwards.get(&(var, deg)) {
            *v
        } else {
            let v = self.id;
            self.id += 1;
            self.back.insert(v, (var, deg));
            self.forwards.insert((var, deg), v);
            v
        }
    }

    fn decode(&mut self, var: usize) -> (usize, usize) {
        if let Some(&v) = self.back.get(&var) {
            v
        } else {
            panic!("Unknown var");
        }
    }
}

fn make_linear(e: &Expr, vars: &mut DegVar, deg: usize) -> Expr {
    vars.d = vars.d.max(deg);

    let vec_map = |exprs: &Vec<Expr>, vars: &mut DegVar| {
        exprs
            .into_iter()
            .map(|e| make_linear(&e, vars, deg))
            .collect()
    };

    let binop_map = |b: &Binop, vars: &mut DegVar| {
        Binop::new(
            make_linear(&*b.left, vars, deg),
            make_linear(&*b.right, vars, deg),
        )
    };

    match e {
        Expr::Var(v) => {
            // We start at 0
            vars.t = vars.t.max(*v + 1);
            Expr::Var(vars.encode(*v, deg))
        }
        Expr::Const(_) => e.clone(),

        Expr::Not(expr) => !make_linear(&*expr, vars, deg),
        Expr::Neg(expr) => -make_linear(&*expr, vars, deg),
        Expr::Scale(v, expr) => *v * make_linear(&*expr, vars, deg),

        Expr::And(exprs) => Expr::And(vec_map(exprs, vars)),
        Expr::Or(exprs) => Expr::Or(vec_map(exprs, vars)),
        Expr::Xor(exprs) => Expr::Xor(vec_map(exprs, vars)),
        Expr::Add(exprs) => Expr::Add(vec_map(exprs, vars)),
        Expr::Sub(exprs) => Expr::Sub(vec_map(exprs, vars)),

        Expr::Shl(binop) => Expr::Shl(binop_map(binop, vars)),
        Expr::Shr(binop) => Expr::Shr(binop_map(binop, vars)),
        Expr::RshiftS(binop) => Expr::RshiftS(binop_map(binop, vars)),
        Expr::Le(binop) => Expr::Le(binop_map(binop, vars)),
        Expr::Lt(binop) => Expr::Lt(binop_map(binop, vars)),
        Expr::Ge(binop) => Expr::Ge(binop_map(binop, vars)),
        Expr::Gt(binop) => Expr::Gt(binop_map(binop, vars)),
        Expr::LeS(binop) => Expr::LeS(binop_map(binop, vars)),
        Expr::LtS(binop) => Expr::LtS(binop_map(binop, vars)),
        Expr::GeS(binop) => Expr::GeS(binop_map(binop, vars)),
        Expr::GtS(binop) => Expr::GtS(binop_map(binop, vars)),
        Expr::Ne(binop) => Expr::Ne(binop_map(binop, vars)),
        Expr::Eq(binop) => Expr::Eq(binop_map(binop, vars)),

        Expr::Mul(exprs) => Expr::Mul(
            exprs
                .into_iter()
                .enumerate()
                .map(|(i, e)| make_linear(e, vars, deg + i))
                .collect(),
        ),
    }
}

fn make_polynomial(e: Expr, vars: &mut DegVar) -> Expr {
    let vec_map = |exprs: Vec<Expr>, vars: &mut DegVar| {
        exprs
            .into_iter()
            .map(|e| make_polynomial(e, vars))
            .collect()
    };

    let binop_map = |b: Binop, vars: &mut DegVar| {
        Binop::new(
            make_polynomial(*b.left, vars),
            make_polynomial(*b.right, vars),
        )
    };

    match e {
        Expr::Var(v) => Expr::Var(vars.decode(v).0),
        Expr::Const(_) => e,

        Expr::Not(expr) => !make_polynomial(*expr, vars),
        Expr::Neg(expr) => -make_polynomial(*expr, vars),
        Expr::Scale(v, expr) => v * make_polynomial(*expr, vars),

        Expr::Or(exprs) => Expr::Or(vec_map(exprs, vars)),
        Expr::Xor(exprs) => Expr::Xor(vec_map(exprs, vars)),
        Expr::Add(exprs) => Expr::Add(vec_map(exprs, vars)),
        Expr::Sub(exprs) => Expr::Sub(vec_map(exprs, vars)),
        Expr::Mul(exprs) => Expr::Mul(vec_map(exprs, vars)),

        Expr::Shl(binop) => Expr::Shl(binop_map(binop, vars)),
        Expr::Shr(binop) => Expr::Shr(binop_map(binop, vars)),
        Expr::RshiftS(binop) => Expr::RshiftS(binop_map(binop, vars)),
        Expr::Le(binop) => Expr::Le(binop_map(binop, vars)),
        Expr::Lt(binop) => Expr::Lt(binop_map(binop, vars)),
        Expr::Ge(binop) => Expr::Ge(binop_map(binop, vars)),
        Expr::Gt(binop) => Expr::Gt(binop_map(binop, vars)),
        Expr::LeS(binop) => Expr::LeS(binop_map(binop, vars)),
        Expr::LtS(binop) => Expr::LtS(binop_map(binop, vars)),
        Expr::GeS(binop) => Expr::GeS(binop_map(binop, vars)),
        Expr::GtS(binop) => Expr::GtS(binop_map(binop, vars)),
        Expr::Ne(binop) => Expr::Ne(binop_map(binop, vars)),
        Expr::Eq(binop) => Expr::Eq(binop_map(binop, vars)),

        Expr::And(exprs) => {
            let mut grouped: HashMap<usize, Vec<usize>> = HashMap::new();

            for e in &exprs {
                if let Expr::Var(v) = e {
                    let (v, d) = vars.decode(*v);
                    grouped.entry(d).or_default().push(v);
                }
            }

            let mut terms = Vec::new();

            for (_, vs) in grouped {
                terms.push(Expr::And(vs.iter().map(|&v| Expr::Var(v)).collect()));
            }

            match terms.len() {
                0 => Expr::Const(1),
                1 => terms.into_iter().next().unwrap(),
                _ => Expr::Mul(terms),
            }
        }
    }
}

pub fn solve_polynomial(e: &Expr, n: u32) -> Expr {
    let mut vars = DegVar::new(0);
    let mut e = make_linear(e, &mut vars, 0);

    // error!("LINEAR: {}", e);

    // TODO!
    e = solve_linear(&e, n);

    // error!("LINEAR SOLVED: {}", e);

    // Removes the "and"s
    e = make_polynomial(e, &mut vars);

    e.arith_reduce().mod_simplify(n)
}
