use crate::expr::{Binop, Expr};
use std::{
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
    pub fn size(&self) -> usize {
        match self {
            ANF::Xor(anfs) | ANF::And(anfs) => anfs.iter().map(|a| a.size()).sum(),
            ANF::One | ANF::Zero | ANF::Var(_, _) => 1,
        }
    }

    pub fn eval(&self, vars: &Vec<u128>) -> u128 {
        match self {
            ANF::Xor(anfs) => anfs.iter().map(|a| a.eval(vars)).fold(0, |x, y| x ^ y),
            ANF::And(anfs) => anfs.iter().map(|a| a.eval(vars)).fold(1, |x, y| x & y),
            ANF::One => 1,
            ANF::Zero => 0,
            ANF::Var(i, j) => (vars[*i] >> j) & 1,
        }
    }

    pub fn xor(mut terms: Vec<ANF>) -> ANF {
        let mut flat = Vec::with_capacity(terms.len());
        for t in terms.drain(..) {
            match t {
                ANF::Xor(inner) => flat.extend(inner),
                ANF::Zero => {}
                other => flat.push(other),
            }
        }

        // Reduce duplicates (pairs cancel)
        flat.sort();
        let mut reduced = Vec::new();
        let mut i = 0;
        while i < flat.len() {
            let mut count = 1;
            while i + count < flat.len() && flat[i] == flat[i + count] {
                count += 1;
            }
            if count % 2 == 1 {
                reduced.push(flat[i].clone());
            }
            i += count;
        }

        match reduced.len() {
            0 => ANF::Zero,
            1 => reduced[0].clone(),
            _ => ANF::Xor(reduced),
        }
    }

    pub fn and(factors: Vec<ANF>) -> ANF {
        // First, flatten nested ANDs and filter trivial elements
        let mut flat = Vec::new();
        for f in factors {
            match f {
                ANF::And(inner) => flat.extend(inner),
                ANF::One => {}
                ANF::Zero => return ANF::Zero,
                other => flat.push(other),
            }
        }

        // If any child is XOR, distribute AND over XOR
        if let Some(pos) = flat.iter().position(|f| matches!(f, ANF::Xor(_))) {
            if let ANF::Xor(xor_terms) = flat.remove(pos) {
                let mut distributed_terms = Vec::new();
                for term in xor_terms {
                    let mut copy = flat.clone();
                    copy.insert(pos, term);
                    distributed_terms.push(ANF::and(copy)); // recursive call, simplified incrementally
                }
                return ANF::xor(distributed_terms);
            }
        }

        // Deduplicate
        flat.sort();
        flat.dedup();

        match flat.len() {
            0 => ANF::One,
            1 => flat[0].clone(),
            _ => ANF::And(flat),
        }
    }
}

impl Default for ANF {
    fn default() -> Self {
        ANF::Zero
    }
}

impl BitXor for ANF {
    type Output = Self;

    fn bitxor(self, rhs: Self) -> Self::Output {
        ANF::xor(vec![self, rhs])
    }
}

impl BitAnd for ANF {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        ANF::and(vec![self, rhs])
    }
}

impl BitOr for ANF {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        (self.clone() & rhs.clone()) ^ (self ^ rhs)
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
pub struct ANFExpr {
    pub bits: Vec<ANF>,
}

impl Index<usize> for ANFExpr {
    type Output = ANF;

    fn index(&self, idx: usize) -> &Self::Output {
        &self.bits[idx]
    }
}

impl IndexMut<usize> for ANFExpr {
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        &mut self.bits[idx]
    }
}

impl BitXor for ANFExpr {
    type Output = Self;

    fn bitxor(self, rhs: Self) -> Self::Output {
        Self::Output {
            bits: self
                .bits
                .into_iter()
                .zip(rhs.bits)
                .map(|(l, r)| l ^ r)
                .collect(),
        }
    }
}

impl BitAnd for ANFExpr {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self::Output {
            bits: self
                .bits
                .into_iter()
                .zip(rhs.bits)
                .map(|(l, r)| l & r)
                .collect(),
        }
    }
}

impl BitOr for ANFExpr {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        (self.clone() & rhs.clone()) ^ (self ^ rhs)
    }
}

impl Add for ANFExpr {
    type Output = ANFExpr;

    fn add(self, rhs: Self) -> Self::Output {
        let mut c = ANF::Zero;

        Self::Output {
            bits: self
                .bits
                .into_iter()
                .zip(rhs.bits)
                .map(|(xi, yi)| {
                    // sum bit = xi ^ yi ^ c
                    let r = ANF::xor(vec![xi.clone(), yi.clone(), c.clone()]);

                    // new carry = (xi & yi) ^ (xi & c) ^ (yi & c)
                    c = ANF::xor(vec![
                        xi.clone() & yi.clone(),
                        xi.clone() & c.clone(),
                        yi & c.clone(),
                    ]);

                    r
                })
                .collect(),
        }
    }
}

impl Shl<u128> for ANFExpr {
    type Output = Self;

    fn shl(self, rhs: u128) -> Self::Output {
        let n = self.bits.len();

        Self::Output {
            bits: (0..n)
                .map(|i| {
                    if i as u128 >= rhs {
                        self.bits[i - rhs as usize].clone()
                    } else {
                        ANF::Zero
                    }
                })
                .collect(),
        }
    }
}

impl Shr<u128> for ANFExpr {
    type Output = Self;

    fn shr(self, rhs: u128) -> Self::Output {
        let rhs = rhs as usize;
        let n = self.bits.len();
        Self::Output {
            bits: (0..n)
                .map(|i| {
                    if i + rhs < n {
                        self.bits[i + rhs].clone() // take the bit rhs positions to the right
                    } else {
                        ANF::Zero // fill top bits with zero
                    }
                })
                .collect(),
        }
    }
}

impl Mul<ANF> for ANFExpr {
    type Output = ANFExpr;

    fn mul(self, rhs: ANF) -> Self::Output {
        Self::Output {
            bits: self.bits.into_iter().map(|a| a & rhs.clone()).collect(),
        }
    }
}

impl Mul for ANFExpr {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        let n = self.bits.len();

        let mut sum = ANFExpr::zero(n);
        let mut carry = ANFExpr::zero(n);

        for (i, rhs_bit) in rhs.bits.into_iter().enumerate() {
            if rhs_bit == ANF::Zero {
                continue;
            }

            let shifted = (self.clone() << (i as u128)) * rhs_bit; // shift self by i

            // CSA combine sum, carry, shifted
            let mut new_sum = ANFExpr::zero(n);
            let mut new_carry = ANFExpr::zero(n);

            for (j, ((a, b), c)) in sum
                .bits
                .into_iter()
                .zip(carry.bits)
                .zip(shifted.bits)
                .enumerate()
            {
                new_sum.bits[j] = ANF::xor(vec![a.clone(), b.clone(), c.clone()]);
                new_carry.bits[j] = ANF::xor(vec![
                    ANF::and(vec![a.clone(), b.clone()]),
                    ANF::and(vec![a, c.clone()]),
                    ANF::and(vec![b, c]),
                ]);
            }

            sum = new_sum;
            carry = new_carry << 1;
        }

        sum + carry
    }
}

impl Not for ANFExpr {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self::Output {
            bits: self.bits.into_iter().map(|anf| anf ^ ANF::One).collect(),
        }
    }
}

impl Neg for ANFExpr {
    type Output = ANFExpr;

    fn neg(self) -> Self::Output {
        let n = self.bits.len();
        !self + ANFExpr::one(n)
    }
}

impl Sub for ANFExpr {
    type Output = ANFExpr;

    fn sub(self, rhs: Self) -> Self::Output {
        let mut b = ANF::Zero;

        Self::Output {
            bits: self
                .bits
                .into_iter()
                .zip(rhs.bits)
                .map(|(xi, yi)| {
                    // difference bit = xi ^ yi ^ b
                    let d = ANF::xor(vec![xi.clone(), yi.clone(), b.clone()]);

                    // borrow = (!xi & yi) | (b & !(xi ^ yi))
                    let not_xi = ANF::xor(vec![xi.clone(), ANF::One]);
                    let not_xi_xor_yi = ANF::xor(vec![xi.clone(), yi.clone()]);
                    let not_xor = ANF::xor(vec![not_xi_xor_yi, ANF::One]);

                    let term1 = ANF::and(vec![not_xi, yi.clone()]);
                    let term2 = ANF::and(vec![b.clone(), not_xor]);

                    b = ANF::xor(vec![
                        term1.clone(),
                        term2.clone(),
                        ANF::and(vec![term1, term2]),
                    ]);

                    d
                })
                .collect(),
        }
    }
}

impl<T: Into<u128>> From<(T, usize)> for ANFExpr {
    fn from((value, n): (T, usize)) -> Self {
        let value: u128 = value.into();

        ANFExpr::from_value(value, n)
    }
}

impl From<(Expr, usize)> for ANFExpr {
    fn from((value, n): (Expr, usize)) -> Self {
        match value {
            Expr::Var(v) => Self {
                bits: (0..n).map(|i| ANF::Var(v, i)).collect(),
            },

            Expr::Const(c) => (c, n).into(),

            Expr::Mul(terms) => {
                let mut terms = terms.into_iter().map(|t| (t, n).into());
                let first = terms.next().unwrap();
                terms.fold(first, |a, b| a * b)
            }

            Expr::Add(terms) => {
                let mut terms = terms.into_iter().map(|t| (t, n).into());
                let first = terms.next().unwrap();
                terms.fold(first, |a, b| a + b)
            }

            Expr::Sub(terms) => {
                let mut terms = terms.into_iter().map(|t| (t, n).into());
                let first = terms.next().unwrap();
                terms.fold(first, |a, b| a - b)
            }

            Expr::Not(expr) => {
                let expr: Self = (*expr, n).into();
                !expr
            }

            Expr::Neg(expr) => {
                let expr: Self = (*expr, n).into();
                -expr
            }

            Expr::And(terms) => {
                let mut terms = terms.into_iter().map(|t| (t, n).into());
                let first = terms.next().unwrap();
                terms.fold(first, |a, b| a & b)
            }

            Expr::Xor(terms) => {
                let mut terms = terms.into_iter().map(|t| (t, n).into());
                let first = terms.next().unwrap();
                terms.fold(first, |a, b| a ^ b)
            }

            Expr::Or(terms) => {
                let mut terms = terms.into_iter().map(|t| (t, n).into());
                let first = terms.next().unwrap();
                terms.fold(first, |a, b| a | b)
            }

            Expr::Shl(Binop { left, right }) => {
                let left: Self = (*left, n).into();

                if let Expr::Const(c) = *right {
                    left << c as u128
                } else {
                    todo!()
                }
            }

            Expr::Shr(Binop { left, right }) => {
                let left: Self = (*left, n).into();

                if let Expr::Const(c) = *right {
                    left >> c as u128
                } else {
                    todo!()
                }
            }

            _ => todo!(),
        }
    }
}

impl Display for ANFExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:?}", self.bits))
    }
}

impl ANFExpr {
    pub fn zero(n: usize) -> Self {
        Self {
            bits: vec![ANF::Zero; n],
        }
    }

    pub fn one(n: usize) -> Self {
        let mut res = Self::zero(n);
        res[0] = ANF::One;
        res
    }

    pub fn from_value(value: u128, n: usize) -> Self {
        let value: u128 = value.into();

        Self {
            bits: (0..n)
                .map(|i| {
                    if ((value >> i) & 1) == 1 {
                        ANF::One
                    } else {
                        ANF::Zero
                    }
                })
                .collect(),
        }
    }

    pub fn var(id: usize, n: usize) -> Self {
        Self {
            bits: (0..n).map(|i| ANF::Var(id, i)).collect(),
        }
    }

    pub fn eval(&self, vars: &Vec<u128>) -> u128 {
        let mut res = 0;

        for (i, anf) in self.bits.iter().enumerate() {
            res |= anf.eval(vars) << i
        }

        res
    }

    pub fn is_int(&self) -> bool {
        for anf in &self.bits {
            match anf {
                ANF::One | ANF::Zero => (),
                _ => return false,
            }
        }
        return true;
    }

    pub fn to_int(&self) -> Option<u128> {
        let mut res = 0;

        for (i, anf) in self.bits.iter().enumerate() {
            match anf {
                ANF::One => res |= 1 << i,
                ANF::Zero => (),
                _ => return None,
            }
        }

        Some(res)
    }

    pub fn size(&self) -> usize {
        self.bits.iter().map(|a| a.size()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*; // import the outer module

    const TV: [(u128, u128); 6] = [(1, 2), (3, 4), (0, 5), (42, 77), (88, 102), (155, 268)];

    #[test]
    fn test_add() {
        for (a, b) in TV {
            assert_eq!(
                (ANFExpr::from_value(a, 8) + ANFExpr::from_value(b, 8))
                    .to_int()
                    .unwrap(),
                (a as u8 + b as u8) as u128
            );
        }
    }

    #[test]
    fn test_sub() {
        for (a, b) in TV {
            assert_eq!(
                (ANFExpr::from_value(a, 8) - ANFExpr::from_value(b, 8))
                    .to_int()
                    .unwrap(),
                (a as u8).wrapping_sub(b as u8) as u128
            );
        }
    }

    #[test]
    fn test_mul() {
        for (a, b) in TV {
            assert_eq!(
                (ANFExpr::from_value(a, 8) * ANFExpr::from_value(b, 8))
                    .to_int()
                    .unwrap(),
                (a as u8).wrapping_mul(b as u8) as u128
            );
        }
    }

    #[test]
    fn test_xor() {
        for (a, b) in TV {
            assert_eq!(
                (ANFExpr::from_value(a, 8) ^ ANFExpr::from_value(b, 8))
                    .to_int()
                    .unwrap(),
                (a as u8 ^ b as u8) as u128
            );
        }
    }

    #[test]
    fn test_and() {
        for (a, b) in TV {
            assert_eq!(
                (ANFExpr::from_value(a, 8) & ANFExpr::from_value(b, 8))
                    .to_int()
                    .unwrap(),
                (a as u8 & b as u8) as u128
            );
        }
    }

    #[test]
    fn test_or() {
        for (a, b) in TV {
            assert_eq!(
                (ANFExpr::from_value(a, 8) | ANFExpr::from_value(b, 8))
                    .to_int()
                    .unwrap(),
                (a as u8 | b as u8) as u128
            );
        }
    }

    #[test]
    fn test_invert() {
        for (a, _) in TV {
            assert_eq!(
                (-ANFExpr::from_value(a, 8)).to_int().unwrap(),
                (-(a as i8) as u8) as u128
            );
        }
    }

    #[test]
    fn test_neg() {
        for (a, _) in TV {
            assert_eq!(
                (!ANFExpr::from_value(a, 8)).to_int().unwrap(),
                (!a as u8) as u128
            );
        }
    }
}
