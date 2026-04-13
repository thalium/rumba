use std::{
    cmp::max,
    collections::HashSet,
    fmt::{self, Display},
    ops::{Add, BitAnd, BitOr, BitXor, Mul, Neg, Not, Sub},
    vec,
};

use rand::random_range;

use crate::varint::VarInt;

#[cfg(feature = "jit")]
use crate::jit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarId(pub usize);

impl Display for VarId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("{}", self.0))
    }
}

impl From<usize> for VarId {
    fn from(value: usize) -> Self {
        VarId(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Expr {
    Var(VarId),

    Const(VarInt),

    // Unary
    Not(Box<Expr>),
    Scale(VarInt, Box<Expr>),

    // Bitwise
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Xor(Vec<Expr>),

    // Arithmetic
    Add(Vec<Expr>),
    Mul(Vec<Expr>),
}

impl Not for Expr {
    type Output = Expr;

    fn not(self) -> Self::Output {
        Expr::Not(Box::new(self))
    }
}

impl BitXor for Expr {
    type Output = Expr;

    fn bitxor(self, rhs: Self) -> Self::Output {
        Expr::Xor(vec![self, rhs])
    }
}

impl BitAnd for Expr {
    type Output = Expr;

    fn bitand(self, rhs: Self) -> Self::Output {
        Expr::And(vec![self, rhs])
    }
}

impl BitOr for Expr {
    type Output = Expr;

    fn bitor(self, rhs: Self) -> Self::Output {
        Expr::Or(vec![self, rhs])
    }
}

impl Add for Expr {
    type Output = Expr;

    fn add(self, rhs: Self) -> Self::Output {
        Expr::Add(vec![self, rhs])
    }
}

impl Sub for Expr {
    type Output = Expr;

    fn sub(self, rhs: Self) -> Self::Output {
        Expr::Add(vec![self, -rhs])
    }
}

impl Mul for Expr {
    type Output = Expr;

    fn mul(self, rhs: Self) -> Self::Output {
        Expr::Mul(vec![self, rhs])
    }
}

impl Mul<Expr> for u64 {
    type Output = Expr;

    fn mul(self, rhs: Expr) -> Self::Output {
        if self == 0 {
            Expr::Const(VarInt::ZERO)
        } else if self == 1 {
            rhs
        } else {
            Expr::Scale(self.into(), Box::new(rhs))
        }
    }
}

impl Mul<Expr> for VarInt {
    type Output = Expr;

    fn mul(self, rhs: Expr) -> Self::Output {
        if self == VarInt::ZERO {
            Expr::Const(self)
        } else if self == VarInt::ONE {
            rhs
        } else {
            Expr::Scale(self, Box::new(rhs))
        }
    }
}

impl Neg for Expr {
    type Output = Expr;

    fn neg(self) -> Self::Output {
        Expr::Scale(VarInt::MAX, Box::new(self))
    }
}

pub type TruthTable = [u64; 256 * 256];

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.repr(64, u64::MAX, true, false))
    }
}

impl From<VarInt> for Expr {
    fn from(value: VarInt) -> Self {
        Expr::Const(value)
    }
}

impl Expr {
    pub fn make_const(c: u64) -> Self {
        Expr::Const(c.into())
    }

    /// An null expression
    pub const fn zero() -> Self {
        Expr::Const(VarInt::ZERO)
    }

    pub fn scale(c: VarInt, e: Expr) -> Expr {
        match c {
            VarInt::ZERO => Expr::zero(),
            VarInt::ONE => e,
            _ => Expr::Scale(c, Box::new(e)),
        }
    }

    /// Counts the number of nodes in the expression
    pub fn size(&self) -> usize {
        let children_size = match self {
            Expr::Var(_) | Expr::Const(_) => 0,

            Expr::Not(expr) | Expr::Scale(_, expr) => expr.size(),

            Expr::And(exprs)
            | Expr::Or(exprs)
            | Expr::Xor(exprs)
            | Expr::Add(exprs)
            | Expr::Mul(exprs) => exprs.iter().map(|e| e.size()).sum(),
        };

        children_size + 1
    }

    /// Evaluates the expression with the given variable values
    pub fn eval(&self, vars: &[u64]) -> VarInt {
        match self {
            Expr::Var(i) => vars[i.0].into(),

            Expr::Const(c) => *c,

            Expr::And(exprs) => exprs
                .iter()
                .map(|e| e.eval(vars))
                .fold(VarInt::MAX, |x, y| x & y),

            Expr::Or(exprs) => exprs
                .iter()
                .map(|e| e.eval(vars))
                .fold(VarInt::ZERO, |x, y| x | y),

            Expr::Xor(exprs) => exprs
                .iter()
                .map(|e| e.eval(vars))
                .fold(VarInt::ZERO, |x, y| x ^ y),

            Expr::Add(exprs) => exprs
                .iter()
                .map(|e| e.eval(vars))
                .fold(VarInt::ZERO, |x, y| x + y),

            Expr::Mul(exprs) => exprs
                .iter()
                .map(|e| e.eval(vars))
                .fold(VarInt::ONE, |x, y| x * y),

            Expr::Scale(v, e) => *v * e.eval(vars),

            Expr::Not(e) => !e.eval(vars),
        }
    }

    /// Checks if two expressions are semantically equal
    /// In case of error returns the variables that caused the error
    /// as well as the evaluations of self and other
    pub fn sem_equal(
        &self,
        other: &Expr,
        mask: u64,
        count: usize,
    ) -> Result<(), (Vec<u64>, u64, u64)> {
        let t = max(
            self.get_vars()
                .iter()
                .copied()
                .map(|v| v.0)
                .max()
                .unwrap_or(0),
            other
                .get_vars()
                .iter()
                .copied()
                .map(|v| v.0)
                .max()
                .unwrap_or(0),
        );

        for _ in 0..count {
            let vars: Vec<_> = (0..=t).map(|_| random_range(0..=mask)).collect();

            let v1 = self.eval(&vars).get(mask);
            let v2 = other.eval(&vars).get(mask);

            if v1 != v2 {
                return Err((vars, v1, v2));
            }
        }

        Ok(())
    }

    /// Calculates the truth table of an expression on n values with t variables
    pub fn truth_table(&self, t: usize, mask: u64) -> Vec<u64> {
        // if t > 20 {
        //     panic!("CRAZYY");
        // }

        let size = 1usize << t;
        let mut tt = Vec::with_capacity(size);

        let mut vars = vec![0u64; t];

        // Decode i into 2^t binary values (one value per variable)
        let vars_from_i = |i: usize, vars: &mut [u64]| {
            let mut idx = i;
            for var in vars {
                *var = (idx & 1) as u64;
                idx >>= 1;
            }
        };

        #[cfg(feature = "jit")]
        {
            // TODO: empirical
            if t > 9 {
                let jit_fn = jit::compile(self);

                for i in 0..size {
                    vars_from_i(i, &mut vars);
                    tt.push(jit_fn.eval(&vars) & mask);
                }

                return tt;
            }
        }

        for i in 0..size {
            vars_from_i(i, &mut vars);
            tt.push(self.eval(&vars).get(mask));
        }

        tt
    }

    /// Calls a function recursively on each node of an expression
    pub fn visit<T, F>(&self, mut f: F) -> T
    where
        F: FnMut(&Expr, Vec<T>) -> T + Clone,
    {
        match self {
            Expr::Var(_) | Expr::Const(_) => f(self, vec![]),

            Expr::Not(expr) | Expr::Scale(_, expr) => {
                let v = vec![expr.visit::<T, F>(f.clone())];
                f(self, v)
            }

            Expr::And(exprs)
            | Expr::Or(exprs)
            | Expr::Xor(exprs)
            | Expr::Add(exprs)
            | Expr::Mul(exprs) => {
                let v = exprs.iter().map(|e| e.visit(f.clone())).collect();
                f(self, v)
            }
        }
    }

    /// Counts the number of variables in the expression
    pub fn get_vars(&self) -> HashSet<VarId> {
        self.visit(|e, children: Vec<HashSet<VarId>>| {
            let mut acc: HashSet<VarId> = children.into_iter().flatten().collect();
            if let Expr::Var(v) = e {
                acc.insert(*v);
            }
            acc
        })
    }

    // Operator precedence
    // https://en.cppreference.com/w/c/language/operator_precedence.html
    fn precedence(&self) -> usize {
        match self {
            Expr::Var(_) | Expr::Const(_) => 0,

            Expr::Not(_) => 2,

            Expr::Mul(_) | Expr::Scale(_, _) => 3,

            Expr::Add(_) => 4,

            Expr::And(_) => 8,

            Expr::Xor(_) => 9,

            Expr::Or(_) => 10,
        }
    }

    /// Parenthesizes an expression if needed
    fn parenthesize(&self, parent: &Expr, s: String) -> String {
        if parent.precedence() <= self.precedence() {
            format!("({})", s)
        } else {
            s
        }
    }

    pub fn symbol(&self, latex: bool) -> &str {
        match (self, latex) {
            (Expr::Var(_), _) | (Expr::Const(_), _) => "",

            (Expr::Not(_), true) => "\\neg",
            (Expr::Not(_), false) => "~",

            (Expr::Scale(_, _), true) => "\\cdot",
            (Expr::Scale(_, _), false) => "*",

            (Expr::And(_), true) => "\\land",
            (Expr::And(_), false) => "&",

            (Expr::Or(_), true) => "\\lor",
            (Expr::Or(_), false) => "|",

            (Expr::Xor(_), true) => "\\oplus",
            (Expr::Xor(_), false) => "^",

            (Expr::Add(_), _) => "+",

            (Expr::Mul(_), true) => "\\cdot",
            (Expr::Mul(_), false) => "*",
        }
    }

    /// A string representation of this expression
    pub fn repr(&self, n: u8, mask: u64, hex: bool, latex: bool) -> String {
        let recurs = |e: &Expr| e.parenthesize(self, e.repr(n, mask, hex, latex));

        let join = |exprs: &Vec<Expr>, c: &str| {
            exprs
                .iter()
                .map(recurs)
                .collect::<Vec<String>>()
                .join(c)
                .to_string()
        };

        match self {
            Expr::Var(v) => {
                if latex {
                    format!("v_{{{}}}", v.0)
                } else {
                    format!("v{}", v.0)
                }
            }

            Expr::Const(c) => c.repr(n, mask, hex, latex),

            Expr::Scale(c, expr) => format!(
                "{} {} {}",
                c.repr(n, mask, hex, latex),
                self.symbol(latex),
                recurs(expr)
            ),

            Expr::Not(expr) => {
                format!("{} {}", self.symbol(latex), recurs(expr))
            }

            Expr::And(exprs)
            | Expr::Or(exprs)
            | Expr::Xor(exprs)
            | Expr::Add(exprs)
            | Expr::Mul(exprs) => join(exprs, &format!(" {} ", self.symbol(latex))),
        }
    }

    // Is this a constant
    pub fn is_constant(&self) -> bool {
        matches!(self, Expr::Const(_))
    }

    // Is this a bitwise expression
    pub fn is_bitwise(&self) -> bool {
        match self {
            Expr::Var(_) => true,

            Expr::Not(expr) => expr.is_bitwise(),

            Expr::And(exprs) | Expr::Or(exprs) | Expr::Xor(exprs) => {
                exprs.iter().all(|e| e.is_bitwise())
            }

            _ => false,
        }
    }

    // Are all variables in the given set
    pub fn variables_in(&self, allowed_vars: &Vec<usize>) -> bool {
        match self {
            Expr::Const(_) => true,

            Expr::Var(i) => allowed_vars.contains(&i.0),

            Expr::Not(expr) | Expr::Scale(_, expr) => expr.variables_in(allowed_vars),

            Expr::And(exprs)
            | Expr::Or(exprs)
            | Expr::Xor(exprs)
            | Expr::Add(exprs)
            | Expr::Mul(exprs) => exprs.iter().all(|e| e.variables_in(allowed_vars)),
        }
    }

    // Replaces a given var with another expression
    pub fn replace_var(self, target_var: VarId, replacement: &Expr) -> Self {
        match self {
            Expr::Var(v) if v == target_var => replacement.clone(),
            _ => self.map(|e| e.replace_var(target_var, replacement)),
        }
    }

    /// Is the outer most expression a boolean expression
    pub fn is_bool(&self) -> bool {
        matches!(
            self,
            Expr::Not(_) | Expr::And(_) | Expr::Or(_) | Expr::Xor(_)
        )
    }

    /// Is the outer most expression an arithmetic expression
    pub fn is_arithmetic(&self) -> bool {
        matches!(self, Expr::Add(_) | Expr::Mul(_) | Expr::Scale(_, _))
    }

    // Helper recusive function that needs a reference
    pub fn map<F>(self, mut f: F) -> Self
    where
        F: FnMut(Self) -> Self,
    {
        let vec_map = |exprs: Vec<Expr>, f: F| exprs.into_iter().map(f).collect();

        match self {
            Expr::Var(_) | Expr::Const(_) => self,

            Expr::Not(expr) => !f(*expr),
            Expr::Scale(v, expr) => v * f(*expr),

            Expr::And(exprs) => Expr::And(vec_map(exprs, f)),
            Expr::Or(exprs) => Expr::Or(vec_map(exprs, f)),
            Expr::Xor(exprs) => Expr::Xor(vec_map(exprs, f)),
            Expr::Add(exprs) => Expr::Add(vec_map(exprs, f)),
            Expr::Mul(exprs) => Expr::Mul(vec_map(exprs, f)),
        }
    }
}
