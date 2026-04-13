use std::collections::HashMap;

use crate::{expr::Expr, varint::VarInt};

/// Distributes an expression
fn distribute<F>(
    v: &mut [Expr],
    self_cst: fn(Vec<Expr>) -> Expr,
    child_cst: fn(Vec<Expr>) -> Expr,
    extractor: F,
) -> Option<Expr>
where
    F: Fn(&mut [Expr]) -> Option<(usize, Vec<Expr>)>,
{
    if let Some((idx, terms)) = extractor(v) {
        let mut distributed_terms: Vec<Expr> = Vec::with_capacity(terms.len());

        for term in terms {
            let mut copy = v.to_vec();
            copy[idx] = term;
            distributed_terms.push(self_cst(copy));
        }

        return Some(child_cst(distributed_terms));
    }

    None
}

macro_rules! distribute {
    ($self:ident, $child:ident, $v:expr) => {
        distribute($v, Expr::$self, Expr::$child, distribute!(@extractor $child))
    };

    (@extractor $variant:ident) => {
        |v: &mut [Expr]| {
            v.iter_mut().enumerate().find_map(|(i, e)| match e {
                Expr::$variant(v) => Some((i, std::mem::take(v))),
                _ => None,
            })
        }
    };
}

/// Remove pairs of elements in a vector
fn remove_pairs(v: Vec<Expr>) -> Vec<Expr> {
    let mut reduced = Vec::with_capacity(v.len());
    let mut iter = v.into_iter().peekable();

    while let Some(current) = iter.next() {
        let mut count = 1;

        while let Some(next) = iter.peek() {
            if *next == current {
                count += 1;
                iter.next();
                continue;
            }

            break;
        }

        if (count & 1) == 1 {
            reduced.push(current);
        }
    }

    reduced
}

/// Deduplicates a list
fn dedupe(mut v: Vec<Expr>) -> Vec<Expr> {
    // TODO: Somehow using hashset here makes the program crash
    // We need a better comparison function
    v.sort();
    v.dedup();
    v
}

struct Reducer {
    mask: u64,
}

/// The result enum for a flattening handler
#[must_use]
enum FlattenResult {
    Expr(Expr),
    Vec(Vec<Expr>),
    None,
}

impl Reducer {
    /// Flattens an expression by concatenating any same typed child expression into it
    fn flatten<F>(&self, v: Vec<Expr>, mut handler: F) -> Vec<Expr>
    where
        F: FnMut(Expr) -> FlattenResult,
    {
        let mut flat = Vec::with_capacity(v.len());

        let mut stack: Vec<_> = v.into_iter().map(|e| self.reduce(e)).collect();

        while let Some(e) = stack.pop() {
            match handler(e) {
                FlattenResult::Vec(mut v) => {
                    stack.append(&mut v);
                }

                FlattenResult::Expr(e) => flat.push(e),

                FlattenResult::None => {}
            }
        }

        flat
    }

    pub fn group_terms(&self, exprs: Vec<Expr>) -> Expr {
        let initial_len = exprs.len();

        let mut map = HashMap::<Expr, VarInt>::with_capacity(initial_len);

        for e in exprs.into_iter() {
            if let Expr::Scale(c, e) = e {
                let count = map.entry(*e).or_insert(VarInt::ZERO);
                *count = *count + c;
            } else {
                let count = map.entry(e).or_insert(VarInt::ZERO);
                *count = *count + VarInt::ONE;
            }
        }

        let mut out = Vec::with_capacity(map.len());

        for (e, mut count) in map {
            count = count.mask(self.mask);

            if *count == 0 {
                continue;
            }

            out.push(Expr::scale(count, e));
        }

        out.sort();

        if initial_len > out.len() {
            self.reduce(Expr::Add(out))
        } else {
            Expr::Add(out)
        }
    }

    /// Reduces a not node
    fn reduce_not(&self, expr: Expr) -> Expr {
        match expr {
            // !!x = x
            Expr::Not(x) => self.reduce(*x),

            Expr::Const(v) => Expr::Const(!v),

            // De Morgan's laws
            Expr::And(exprs) => Expr::Or(exprs.into_iter().map(|e| self.reduce(!e)).collect()),

            // De Morgan's laws
            Expr::Or(exprs) => Expr::And(exprs.into_iter().map(|e| self.reduce(!e)).collect()),

            _ => !self.reduce(expr),
        }
    }

    /// Reduces a scale node
    fn reduce_scale(&self, scale: VarInt, expr: Expr) -> Expr {
        let scale = scale.mask(self.mask);

        match *scale {
            0 => Expr::zero(),

            1 => self.reduce(expr),

            _ => match self.reduce(expr) {
                Expr::Const(c2) => Expr::Const((scale * c2).mask(self.mask)),

                Expr::Scale(c2, e) => {
                    let c = (scale * c2).mask(self.mask);
                    match *c {
                        0 => Expr::zero(),
                        1 => *e,
                        c => c * *e,
                    }
                }

                Expr::Add(sum) => {
                    // TODO: the arith reduce is unnecessary here
                    self.reduce(Expr::Add(sum.into_iter().map(|e| scale * e).collect()))
                }

                other => scale * other,
            },
        }
    }

    /// Reduces a and node
    fn reduce_and(&self, exprs: Vec<Expr>) -> Expr {
        let mut c: VarInt = self.mask.into();

        let mut flat = self.flatten(exprs, |e| match e {
            Expr::And(v) => FlattenResult::Vec(v),
            Expr::Const(v) => {
                c = c & v;
                FlattenResult::None
            }
            _ => FlattenResult::Expr(e),
        });

        c = c.mask(self.mask);

        if *c == 0 {
            return Expr::zero();
        }

        if *c != self.mask {
            flat.push(Expr::Const(c));
        }

        if let Some(distributed) = distribute!(And, Xor, &mut flat) {
            return self.reduce(distributed);
        }

        flat = dedupe(flat);

        match flat.len() {
            0 => Expr::Const(VarInt::MAX),
            1 => flat.pop().unwrap(),
            _ => Expr::And(flat),
        }
    }

    fn reduce_or(&self, exprs: Vec<Expr>) -> Expr {
        let mut c = VarInt::ZERO;

        let mut flat = self.flatten(exprs, |e| match e {
            Expr::Or(v) => FlattenResult::Vec(v),
            Expr::Const(v) => {
                c = c | v;
                FlattenResult::None
            }
            _ => FlattenResult::Expr(e),
        });

        c = c.mask(self.mask);

        if *c != 0 {
            flat.push(Expr::Const(c));
        }

        if let Some(distributed) = distribute!(Or, And, &mut flat) {
            return self.reduce(distributed);
        }

        flat = dedupe(flat);

        match flat.len() {
            0 => Expr::zero(),
            1 => flat.pop().unwrap(),
            _ => Expr::Or(flat),
        }
    }

    fn reduce_xor(&self, exprs: Vec<Expr>) -> Expr {
        let mut c = VarInt::ZERO;

        let mut flat = self.flatten(exprs, |e| match e {
            Expr::Xor(v) => FlattenResult::Vec(v),
            Expr::Const(v) => {
                c = c ^ v;
                FlattenResult::None
            }
            _ => FlattenResult::Expr(e),
        });

        c = c.mask(self.mask);

        if *c != 0 {
            flat.push(Expr::Const(c));
        }

        flat = remove_pairs(flat);

        match flat.len() {
            0 => Expr::zero(),
            1 => flat.pop().unwrap(),
            _ => Expr::Xor(flat),
        }
    }

    fn reduce_add(&self, exprs: Vec<Expr>) -> Expr {
        // Used with dynamic masking
        if self.mask == 1 {
            return self.reduce_xor(exprs);
        }

        let mut c = VarInt::ZERO;

        let mut flat = self.flatten(exprs, |e| match e {
            Expr::Add(v) => FlattenResult::Vec(v),
            Expr::Const(v) => {
                c = c + v;
                FlattenResult::None
            }
            _ => FlattenResult::Expr(e),
        });

        c = c.mask(self.mask);

        if *c != 0 {
            flat.push(Expr::Const(c));
        }

        match flat.len() {
            0 => Expr::zero(),
            1 => flat.pop().unwrap(),
            _ => self.group_terms(flat),
        }
    }

    fn reduce_mul(&self, exprs: Vec<Expr>) -> Expr {
        // Used with dynamic masking
        if self.mask == 1 {
            return self.reduce_and(exprs);
        }

        let mut c = VarInt::ONE;

        let mut flat = self.flatten(exprs, |e| match e {
            Expr::Mul(v) => FlattenResult::Vec(v),
            Expr::Const(v) => {
                c = c * v;
                FlattenResult::None
            }
            Expr::Scale(s, v) => {
                c = c * s;
                FlattenResult::Expr(*v)
            }
            _ => FlattenResult::Expr(e),
        });

        c = c.mask(self.mask);

        if *c == 0 {
            return Expr::zero();
        }

        if let Some(distributed) = distribute!(Mul, Add, &mut flat) {
            return self.reduce(Expr::scale(c, distributed));
        }

        flat.sort();

        match flat.len() {
            0 => Expr::Const(c),
            1 => Expr::scale(c, flat.pop().unwrap()),
            _ => Expr::scale(c, Expr::Mul(flat)),
        }
    }

    fn reduce(&self, expr: Expr) -> Expr {
        match expr {
            Expr::Var(_) => expr,

            Expr::Const(c) => Expr::Const(c.mask(self.mask)),

            Expr::Not(expr) => self.reduce_not(*expr),

            Expr::Scale(c, e) => self.reduce_scale(c, *e),

            Expr::And(exprs) => self.reduce_and(exprs),

            Expr::Or(exprs) => self.reduce_or(exprs),

            Expr::Xor(exprs) => self.reduce_xor(exprs),

            Expr::Add(exprs) => self.reduce_add(exprs),

            Expr::Mul(exprs) => self.reduce_mul(exprs),
        }
    }

    // fn reduce_(&self, expr: Expr) -> Expr {
    //     let old = expr.clone();
    //     let res = self.reduce_(expr);

    //     if let Err((v, v1, v2)) = old.sem_equal(&res, self.mask, 500) {
    //         println!("Semantic error {} vs {}", old, res);
    //         println!("Semantic error {} vs {}", v1, v2);
    //         println!("{}\n\n", old.symbol(false));
    //     }
    //     res
    // }
}

impl Expr {
    pub fn reduce(self, mask: u64) -> Self {
        Reducer { mask }.reduce(self)
    }
}
