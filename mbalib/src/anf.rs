use std::{
    array::from_fn,
    fmt::{Display, Write},
    ops::{Add, BitAnd, BitXor, Index, IndexMut, Mul, Not, Shl, Shr, Sub},
};

use crate::expr::{self, Expr};

#[derive(Clone, Debug)]
pub struct Binop {
    pub left: Box<ANF>,
    pub right: Box<ANF>,
}

impl Binop {
    pub fn new(left: ANF, right: ANF) -> Self {
        Self {
            left: Box::new(left),
            right: Box::new(right),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ANF {
    Xor(Binop),
    And(Binop),
    One,
    Zero,
    Var(usize, usize),
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
            (x, y) => ANF::Xor(Binop::new(x.clone(), y.clone())),
        }
    }
}

impl BitAnd for &ANF {
    type Output = ANF;

    fn bitand(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (ANF::Zero, _) | (_, ANF::Zero) => ANF::Zero,
            (ANF::One, x) | (x, ANF::One) => x.clone(),
            (x, y) => ANF::And(Binop::new(x.clone(), y.clone())),
        }
    }
}

impl Display for ANF {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ANF::Xor(Binop { left, right }) => {
                f.write_fmt(format_args!("({} ^ {})", *left, *right))
            }
            ANF::And(Binop { left, right }) => {
                f.write_fmt(format_args!("({} & {})", *left, *right))
            }
            ANF::One => f.write_char('1'),
            ANF::Zero => f.write_char('1'),
            ANF::Var(i, j) => f.write_fmt(format_args!("v{}_{}", i, j)),
        }
    }
}

#[derive(Clone, Debug)]
struct ANFExpr<const N: usize> {
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

                &(&l & &r) ^ &(&l ^ &r)
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
}
