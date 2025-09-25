use std::collections::HashMap;

use log::error;

use crate::expr::Expr;

fn preprocess(e: Expr) -> Expr {
    match e {
        Expr::Not(e) => !preprocess(*e),

        Expr::Add(xs) => Expr::Add(xs.into_iter().map(preprocess).collect()),
        Expr::And(xs) => normalize_nary(xs, Expr::And),
        Expr::Or(xs) => normalize_nary(xs, Expr::Or),
        Expr::Xor(xs) => normalize_nary(xs, Expr::Xor),

        Expr::Mul(exprs) => {
            let mut coeff = Expr::Const(1);
            let mut others = Vec::with_capacity(exprs.len());
            for e in exprs.into_iter() {
                match e {
                    Expr::Const(_) => coeff = e,
                    _ => others.push(preprocess(e)),
                }
            }
            others.insert(0, coeff);
            normalize_nary(others, Expr::Mul)
        }

        leaf => leaf,
    }
}

fn normalize_nary<F>(mut xs: Vec<Expr>, ctor: F) -> Expr
where
    F: Fn(Vec<Expr>) -> Expr + Copy,
{
    match xs.len() {
        0 => panic!("empty n-ary op"),
        1 => preprocess(xs.pop().unwrap()),
        2 => ctor(xs.into_iter().map(preprocess).collect()),
        _ => {
            let first = xs.remove(0);
            ctor(vec![preprocess(first), normalize_nary(xs, ctor)])
        }
    }
}

pub fn blast(mut e: Expr) -> Expr {
    let get_binop = |xs: Vec<Expr>| match xs.as_slice() {
        [x, y] => [x.clone(), y.clone()],
        _ => panic!("Expected binop"),
    };

    let mut var_exprs = HashMap::<String, Expr>::new();
    let mut vars = HashMap::<String, usize>::new();
    let mut var_idx = 3;

    let mut get_var = |e: &Expr| {
        let k = e.to_string();
        if let Some(v) = vars.get(&k) {
            Expr::Var(*v)
        } else {
            var_exprs.insert(k.clone(), e.clone());
            vars.insert(k, var_idx);
            var_idx += 1;
            Expr::Var(var_idx - 1)
        }
    };

    e = e.arith_reduce();
    e = preprocess(e);

    if let Expr::Add(sum) = &mut e {
        for term in sum {
            let mut t = term.clone();
            let mut coeff = 1;

            if let Expr::Mul(prod) = &t {
                match (&prod[0], &prod[1]) {
                    (Expr::Const(c), exp) => {
                        coeff = *c;
                        t = exp.clone();
                    }
                    _ => {
                        error!("Unexpected multiplication {}\n{}", t, e);
                        panic!("Unexpected multiplication")
                    }
                }
            }

            t = match t {
                Expr::And(xs) => match xs.as_slice() {
                    [x, Expr::Not(y)] => {
                        let x = get_var(&x);
                        let y = get_var(y);

                        x.clone() - (x & y)
                    }

                    [Expr::Not(x), y] => {
                        let x = get_var(&x);
                        let y = get_var(y);

                        y.clone() - (x & y)
                    }

                    [x, y] => {
                        let x = get_var(&x);
                        let y = get_var(&y);

                        x & y
                    }
                    _ => panic!("Expexted binop"),
                },

                Expr::Or(xs) => match xs.as_slice() {
                    [x, Expr::Not(y)] => {
                        let x = get_var(&x);
                        let y = get_var(y);

                        -y.clone() + (x & y) - Expr::Const(1)
                    }

                    [Expr::Not(x), y] => {
                        let x = get_var(&x);
                        let y = get_var(y);

                        -x.clone() + (x & y) - Expr::Const(1)
                    }

                    [x, y] => {
                        let x = get_var(&x);
                        let y = get_var(&y);

                        x.clone() + y.clone() - (x & y)
                    }
                    _ => panic!("Expexted binop"),
                },

                Expr::Xor(xs) => {
                    let [x, y] = get_binop(xs);
                    x.clone() + y.clone() - Expr::Const(2) * (x & y)
                }

                Expr::Not(e) => match *e {
                    Expr::And(xs) => {
                        let [x, y] = get_binop(xs);
                        -(x & y) - Expr::Const(1)
                    }

                    Expr::Or(xs) => {
                        let [x, y] = get_binop(xs);
                        -x.clone() - y.clone() + (x & y) - Expr::Const(1)
                    }

                    Expr::Xor(xs) => {
                        let [x, y] = get_binop(xs);
                        -x.clone() - y.clone() + Expr::Const(2) * (x & y) - Expr::Const(1)
                    }

                    x => {
                        let x = get_var(&x);
                        -x - Expr::Const(1)
                    }
                },

                Expr::Const(_) | Expr::Var(_) => t,

                _ => {
                    error!("BITWISE: {}, EXPRESSION: {}", t, e);
                    panic!("Unexpected bitwise expression")
                }
            };

            if coeff != 1 {
                t = Expr::Mul(vec![Expr::Const(coeff), t]);
            }

            *term = t;
        }
    } else {
        panic!("Expression does not have the correct form")
    }

    e = e.arith_reduce();

    for (k, exp) in var_exprs {
        e = e.replace_var(*vars.get(&k).unwrap(), &exp);
    }

    e.arith_reduce()
}
