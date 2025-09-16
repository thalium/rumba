use crate::{
    anf,
    expr::{self, Expr},
};
use std::{
    array::from_fn,
    fmt::{Display, Write},
    ops::{Add, BitAnd, BitOr, BitXor, Index, IndexMut, Mul, Neg, Not, Shl, Shr, Sub},
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ANF {
    Xor(Vec<ANF>),
    And(Vec<ANF>),
    One,
    Zero,
    Var(usize, usize),
}

impl ANF {
    fn rules(&self) -> Self {
        match self {
            ANF::And(anfs) => {
                let anfs: Vec<ANF> = anfs
                    .iter()
                    .cloned()
                    .filter(|a| !matches!(a, ANF::One))
                    .collect();

                if anfs.contains(&ANF::Zero) {
                    return ANF::Zero;
                }

                match anfs.len() {
                    0 => ANF::One,
                    1 => anfs.into_iter().next().unwrap().rules(),
                    _ => ANF::And(anfs.iter().map(|v| v.rules()).collect()),
                }
            }

            ANF::Xor(anfs) => {
                let anfs: Vec<ANF> = anfs
                    .iter()
                    .cloned()
                    .filter(|a| !matches!(a, ANF::Zero))
                    .collect();

                match anfs.len() {
                    0 => ANF::Zero,
                    1 => anfs.into_iter().next().unwrap().rules(),
                    _ => ANF::Xor(anfs.iter().map(|v| v.rules()).collect()),
                }
            }

            e => e.clone(),
        }
    }

    fn flatten(&self) -> Self {
        match self {
            ANF::Xor(anfs) => {
                let flattened: Vec<_> = anfs
                    .iter()
                    .flat_map(|anf| match anf {
                        ANF::Xor(inner) => inner.iter().cloned().map(|e| e.flatten()).collect(),
                        _ => vec![anf.clone()],
                    })
                    .collect();
                ANF::Xor(flattened)
            }

            ANF::And(anfs) => {
                let flattened: Vec<_> = anfs
                    .iter()
                    .flat_map(|anf| match anf {
                        ANF::And(inner) => inner.iter().cloned().map(|e| e.flatten()).collect(),
                        _ => vec![anf.clone()],
                    })
                    .collect();
                ANF::And(flattened)
            }

            e => e.clone(),
        }
    }

    fn distribute(self) -> Self {
        match self {
            ANF::And(mut factors) => {
                // If any factor is an XOR, distribute it
                if let Some(pos) = factors.iter().position(|f| matches!(f, ANF::Xor(_))) {
                    if let ANF::Xor(xor_terms) = factors.remove(pos) {
                        let mut new_terms = vec![];
                        for term in xor_terms {
                            let mut copy = factors.clone();
                            copy.insert(pos, term);
                            new_terms.push(ANF::And(copy).distribute());
                        }
                        return ANF::Xor(new_terms);
                    }
                }

                ANF::And(factors)
            }

            e => e,
        }
    }

    fn sort(&mut self) -> &Self {
        match self {
            ANF::And(anfs) | ANF::Xor(anfs) => {
                for anf in anfs.iter_mut() {
                    anf.sort();
                }

                anfs.sort();
            }

            _ => (),
        };

        self
    }

    fn reduce(&self) -> Self {
        match self {
            ANF::And(anfs) => {
                let mut anfs = anfs.clone();
                anfs.dedup(); // remove duplicates
                match anfs.len() {
                    0 => ANF::One,
                    1 => anfs.into_iter().next().unwrap().reduce(),
                    _ => ANF::And(anfs.iter().map(|v| v.reduce()).collect()),
                }
            }

            ANF::Xor(anfs) => {
                // remove pairs of duplicates
                let mut reduced = vec![];
                let mut i = 0;
                while i < anfs.len() {
                    let mut count = 1;
                    while i + count < anfs.len() && anfs[i] == anfs[i + count] {
                        count += 1;
                    }
                    if count % 2 == 1 {
                        reduced.push(anfs[i].reduce());
                    }
                    i += count;
                }

                match reduced.len() {
                    0 => ANF::Zero,
                    1 => reduced.into_iter().next().unwrap().reduce(),
                    _ => ANF::Xor(reduced),
                }
            }

            e => e.clone(),
        }
    }

    // Returns a simplified ANF
    fn simplify(&self) -> Self {
        let mut flat = self.rules().flatten();
        flat.sort();

        flat.reduce().distribute()
    }

    fn iter_simplify(mut self) -> Self {
        loop {
            let simplified = self.simplify();
            if simplified == self {
                return self;
            }
            self = simplified;
        }
    }
}

impl Default for ANF {
    fn default() -> Self {
        ANF::Zero
    }
}

impl BitXor for &ANF {
    type Output = ANF;

    fn bitxor(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (ANF::One, ANF::One) => ANF::Zero,
            (ANF::Zero, x) | (x, ANF::Zero) => x.clone(),
            (x, y) => {
                let res = ANF::Xor(vec![x.clone(), y.clone()]);
                res.iter_simplify()
            }
        }
    }
}

impl BitAnd for &ANF {
    type Output = ANF;

    fn bitand(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (ANF::Zero, _) | (_, ANF::Zero) => ANF::Zero,
            (ANF::One, x) | (x, ANF::One) => x.clone(),
            (x, y) => {
                let res = ANF::And(vec![x.clone(), y.clone()]);
                res.iter_simplify()
            }
        }
    }
}

impl Display for ANF {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ANF::Xor(v) => f.write_fmt(format_args!(
                "({})",
                v.iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<String>>()
                    .join(" ^ ")
            )),
            ANF::And(v) => f.write_fmt(format_args!(
                "({})",
                v.iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<String>>()
                    .join(" & ")
            )),
            ANF::One => f.write_char('1'),
            ANF::Zero => f.write_char('1'),
            ANF::Var(i, j) => f.write_fmt(format_args!("v{}_{}", i, j)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ANFExpr<const N: usize> {
    pub bits: [ANF; N],
}

impl<const N: usize> Index<usize> for ANFExpr<N> {
    type Output = ANF;

    fn index(&self, idx: usize) -> &Self::Output {
        &self.bits[idx]
    }
}

impl<const N: usize> IndexMut<usize> for ANFExpr<N> {
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        &mut self.bits[idx]
    }
}

impl<const N: usize> BitXor for &ANFExpr<N> {
    type Output = ANFExpr<N>;

    fn bitxor(self, rhs: Self) -> Self::Output {
        Self::Output {
            bits: from_fn(|i| &self[i] ^ &rhs[i]),
        }
    }
}

impl<const N: usize> BitAnd for &ANFExpr<N> {
    type Output = ANFExpr<N>;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self::Output {
            bits: from_fn(|i| &self[i] & &rhs[i]),
        }
    }
}

impl<const N: usize> BitOr for &ANFExpr<N> {
    type Output = ANFExpr<N>;

    fn bitor(self, rhs: Self) -> Self::Output {
        &(self & rhs) ^ &(self ^ rhs)
    }
}

impl<const N: usize> Add for &ANFExpr<N> {
    type Output = ANFExpr<N>;

    fn add(self, rhs: Self) -> Self::Output {
        let mut c = ANF::Zero;

        Self::Output {
            bits: from_fn(|i| {
                let xi = &self[i];
                let yi = &rhs[i];

                let r = &(xi ^ yi) ^ &c;

                c = &(xi & yi) ^ &(&c & &(xi ^ yi));

                r
            }),
        }
    }
}

impl<const N: usize> Shl<u8> for &ANFExpr<N> {
    type Output = ANFExpr<N>;

    fn shl(self, rhs: u8) -> Self::Output {
        Self::Output {
            bits: from_fn(|i| {
                let i = i + rhs as usize;
                if i < N { self[i].clone() } else { ANF::Zero }
            }),
        }
    }
}

impl<const N: usize> Shr<u8> for &ANFExpr<N> {
    type Output = ANFExpr<N>;

    fn shr(self, rhs: u8) -> Self::Output {
        let c = rhs as usize;
        Self::Output {
            bits: from_fn(|i| {
                if i >= c {
                    self[i - c].clone()
                } else {
                    ANF::Zero
                }
            }),
        }
    }
}

impl<const N: usize> Mul<&ANF> for &ANFExpr<N> {
    type Output = ANFExpr<N>;

    fn mul(self, rhs: &ANF) -> Self::Output {
        Self::Output {
            bits: from_fn(|i| &self[i] & &rhs),
        }
    }
}

impl<const N: usize> Mul for &ANFExpr<N> {
    type Output = ANFExpr<N>;

    fn mul(self, rhs: Self) -> Self::Output {
        let mut sum = ANFExpr::<N> {
            bits: from_fn(|_| ANF::Zero),
        };

        for i in 0..N {
            sum = &sum + &(&(self << i as u8) * &rhs[i])
        }

        sum
    }
}

impl<const N: usize> Not for &ANFExpr<N> {
    type Output = ANFExpr<N>;

    fn not(self) -> Self::Output {
        Self::Output {
            bits: from_fn(|i| &self[i] ^ &ANF::One),
        }
    }
}

impl<const N: usize> Neg for &ANFExpr<N> {
    type Output = ANFExpr<N>;

    fn neg(self) -> Self::Output {
        &!self + &ANFExpr::<N>::one()
    }
}

impl<const N: usize> Sub for &ANFExpr<N> {
    type Output = ANFExpr<N>;

    fn sub(self, rhs: Self) -> Self::Output {
        &(self + &!rhs) + &Self::Output::one()
    }
}

impl<const N: usize> Default for ANFExpr<N> {
    fn default() -> Self {
        Self {
            bits: from_fn(|_| ANF::default().clone()),
        }
    }
}

impl<const N: usize, T: Into<u128>> From<T> for ANFExpr<N> {
    fn from(value: T) -> Self {
        let mut expr = Self::default();
        let value: u128 = value.into();

        for i in 0..N {
            expr.bits[i] = if ((value >> i) & 1) == 1 {
                ANF::One
            } else {
                ANF::Zero
            };
        }

        expr
    }
}

impl<const N: usize> From<Expr> for ANFExpr<N> {
    fn from(value: Expr) -> Self {
        match value {
            Expr::Var(v) => Self {
                bits: from_fn(|i| ANF::Var(v, i)),
            },

            Expr::Const(c) => c.into(),

            Expr::Times(expr::Binop { left, right }) => {
                let left: Self = (*left).into();
                let right: Self = (*right).into();

                &left * &right
            }

            Expr::Plus(expr::Binop { left, right }) => {
                let left: Self = (*left).into();
                let right: Self = (*right).into();

                &left + &right
            }

            Expr::Minus(expr::Binop { left, right }) => {
                let left: Self = (*left).into();
                let right: Self = (*right).into();

                &left - &right
            }

            Expr::Not(expr) => {
                let expr: Self = (*expr).into();
                !&expr
            }

            Expr::And(binop) => {
                let l: Self = (*binop.left).into();
                let r: Self = (*binop.right).into();
                &l & &r
            }

            Expr::Xor(binop) => {
                let l: Self = (*binop.left).into();
                let r: Self = (*binop.right).into();
                &l ^ &r
            }

            Expr::Or(binop) => {
                let l: Self = (*binop.left).into();
                let r: Self = (*binop.right).into();

                &l | &r
            }

            Expr::Lshift(expr::Binop { left, right }) => {
                let left: Self = (*left).into();

                if let Expr::Const(c) = *right {
                    &left << c
                } else {
                    todo!()
                }
            }

            Expr::Rshift(expr::Binop { left, right }) => {
                let left: Self = (*left).into();

                if let Expr::Const(c) = *right {
                    &left >> c
                } else {
                    todo!()
                }
            }

            Expr::RshiftS(expr::Binop { left, right }) => {
                let left: Self = (*left).into();

                let sign_bit = left[N - 1].clone();

                if let Expr::Const(c) = *right {
                    Self {
                        bits: from_fn(|i| {
                            let i = i + c as usize;
                            if i < N {
                                left[i].clone()
                            } else {
                                sign_bit.clone()
                            }
                        }),
                    }
                } else {
                    todo!()
                }
            }

            _ => todo!(),
        }
    }
}

impl<const N: usize> Display for ANFExpr<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:?}", self.bits))
    }
}

impl<const N: usize> ANFExpr<N> {
    pub fn one() -> Self {
        Self {
            bits: from_fn(|i| if i == 0 { ANF::One } else { ANF::Zero }),
        }
    }

    pub fn var(id: usize) -> Self {
        Self {
            bits: from_fn(|i| ANF::Var(id, i)),
        }
    }

    pub fn to_int(&self) -> Option<u128> {
        let mut res = 0;

        for i in 0..N {
            match self[i] {
                ANF::One => res |= 1 << i,
                ANF::Zero => (),
                _ => return None,
            }
        }

        Some(res)
    }
}
