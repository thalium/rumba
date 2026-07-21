use std::{
    cmp::max,
    collections::HashSet,
    fmt::{self, Display},
    ops::{Add, BitAnd, BitOr, BitXor, Mul, Neg, Not, Sub},
    vec,
};

use rand::random_range;

use crate::varint::{VarInt, make_mask};

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

    Const(u64),

    // Unary
    Not(Box<Expr>),
    Scale(u64, Box<Expr>),

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
        Expr::scale(self, rhs)
    }
}

impl Neg for Expr {
    type Output = Expr;

    fn neg(self) -> Self::Output {
        Expr::Scale(u64::MAX, Box::new(self))
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.repr_masked(64, u64::MAX, true, false))
    }
}

impl From<u64> for Expr {
    fn from(value: u64) -> Self {
        Expr::Const(value)
    }
}

impl Expr {
    pub fn make_const(c: u64) -> Self {
        Expr::Const(c)
    }

    /// An null expression
    pub const fn zero() -> Self {
        Expr::Const(0)
    }

    pub(crate) fn scale(c: u64, e: Expr) -> Expr {
        match c {
            0 => Expr::zero(),
            1 => e,
            _ => Expr::Scale(c, Box::new(e)),
        }
    }

    /// The common scalar factor of a linear term: `c` for `Scale(c, _)`, and the
    /// shared `c` for an `Add` whose terms are all `Scale(c, _)` (a bare term
    /// counts as `c = 1`); `1` otherwise.
    ///
    /// `reduce` distributes a scalar over a sum, so `2·(x + y)` is stored as
    /// `2·x + 2·y`; this recovers the `2` that is no longer syntactically
    /// present, letting callers see `x + y`, `2·x + 2·y` and `-x - y` as scalar
    /// multiples of the same core.
    pub(crate) fn get_factor(&self, mask: u64) -> u64 {
        let term_factor = |t: &Expr| match t {
            Expr::Scale(c, _) => c & mask,
            _ => 1,
        };
        match self {
            Expr::Scale(c, _) => c & mask,
            Expr::Add(terms) => {
                let f = term_factor(&terms[0]);
                if terms.iter().all(|t| term_factor(t) == f) {
                    f
                } else {
                    1
                }
            }
            _ => 1,
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
    pub(crate) fn eval_bits(&self, vars: &[u64]) -> VarInt {
        match self {
            Expr::Var(i) => vars[i.0].into(),

            Expr::Const(c) => (*c).into(),

            Expr::And(exprs) => exprs
                .iter()
                .map(|e| e.eval_bits(vars))
                .fold(VarInt::MAX, |x, y| x & y),

            Expr::Or(exprs) => exprs
                .iter()
                .map(|e| e.eval_bits(vars))
                .fold(VarInt::ZERO, |x, y| x | y),

            Expr::Xor(exprs) => exprs
                .iter()
                .map(|e| e.eval_bits(vars))
                .fold(VarInt::ZERO, |x, y| x ^ y),

            Expr::Add(exprs) => exprs
                .iter()
                .map(|e| e.eval_bits(vars))
                .fold(VarInt::ZERO, |x, y| x + y),

            Expr::Mul(exprs) => exprs
                .iter()
                .map(|e| e.eval_bits(vars))
                .fold(VarInt::ONE, |x, y| x * y),

            Expr::Scale(v, e) => VarInt::from(*v) * e.eval_bits(vars),

            Expr::Not(e) => !e.eval_bits(vars),
        }
    }

    /// Evaluates the expression for the given variable values on `n` bits.
    pub fn eval(&self, vars: &[u64], n: u8) -> u64 {
        self.eval_bits(vars).get(make_mask(n))
    }

    /// Checks, by random sampling on `n` bits, whether two expressions are
    /// semantically equal. On a counterexample returns the sampled variable
    /// values and the two differing evaluations.
    pub fn sem_equal(
        &self,
        other: &Expr,
        n: u8,
        samples: usize,
    ) -> Result<(), (Vec<u64>, u64, u64)> {
        self.sem_equal_masked(other, make_mask(n), samples)
    }

    /// The truth table of this expression over its first `t` variables on `n`
    /// bits (one entry per assignment of the `t` variables to 0/1).
    pub fn truth_table(&self, t: usize, n: u8) -> Vec<u64> {
        self.truth_table_masked(t, make_mask(n))
    }

    /// Checks if two expressions are semantically equal
    /// In case of error returns the variables that caused the error
    /// as well as the evaluations of self and other
    pub(crate) fn sem_equal_masked(
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

            let v1 = self.eval_bits(&vars).get(mask);
            let v2 = other.eval_bits(&vars).get(mask);

            if v1 != v2 {
                return Err((vars, v1, v2));
            }
        }

        Ok(())
    }

    /// Calculates the truth table of an expression on n values with t variables
    pub(crate) fn truth_table_masked(&self, t: usize, mask: u64) -> Vec<u64> {
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
            // Compiling costs ~169us; a compiled evaluation is ~4.7ns against
            // ~114ns interpreted (see `benches/bench.rs`). So the JIT pays for
            // itself past 169us / (114ns - 4.7ns) ~= 1546 evaluations, i.e.
            // from t = 11 (2048) up. At t = 10 (1024) it is still a net loss.
            if t > 10 {
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
            tt.push(self.eval_bits(&vars).get(mask));
        }

        tt
    }

    /// Calls a function recursively on each node of an expression
    pub(crate) fn visit<T, F>(&self, mut f: F) -> T
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

    pub(crate) fn symbol(&self, latex: bool) -> &str {
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
    pub(crate) fn repr_masked(&self, n: u8, mask: u64, hex: bool, latex: bool) -> String {
        let recurs = |e: &Expr| e.parenthesize(self, e.repr_masked(n, mask, hex, latex));

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

            Expr::Const(c) => VarInt::from(*c).repr(n, mask, hex, latex),

            Expr::Scale(c, expr) => format!(
                "{} {} {}",
                VarInt::from(*c).repr(n, mask, hex, latex),
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

    /// A string representation of this expression on `n` bits.
    pub fn repr(&self, n: u8, hex: bool, latex: bool) -> String {
        self.repr_masked(n, make_mask(n), hex, latex)
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
    pub(crate) fn replace_var(self, target_var: VarId, replacement: &Expr) -> Self {
        match self {
            Expr::Var(v) if v == target_var => replacement.clone(),
            _ => self.map(|e| e.replace_var(target_var, replacement)),
        }
    }

    // Helper recusive function that needs a reference
    pub(crate) fn map<F>(self, mut f: F) -> Self
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

    /// [`Expr::map`] for a fallible transform: rebuilds the node from mapped
    /// children, short-circuiting on the first error. Lets the solver's
    /// recursive rewrites return `Result` without hand-rolling the match at
    /// each site.
    pub(crate) fn try_map<F, E>(self, mut f: F) -> Result<Self, E>
    where
        F: FnMut(Self) -> Result<Self, E>,
    {
        fn vec_try_map<F, E>(exprs: Vec<Expr>, f: F) -> Result<Vec<Expr>, E>
        where
            F: FnMut(Expr) -> Result<Expr, E>,
        {
            exprs.into_iter().map(f).collect()
        }

        Ok(match self {
            Expr::Var(_) | Expr::Const(_) => self,

            Expr::Not(expr) => !f(*expr)?,
            Expr::Scale(v, expr) => v * f(*expr)?,

            Expr::And(exprs) => Expr::And(vec_try_map(exprs, f)?),
            Expr::Or(exprs) => Expr::Or(vec_try_map(exprs, f)?),
            Expr::Xor(exprs) => Expr::Xor(vec_try_map(exprs, f)?),
            Expr::Add(exprs) => Expr::Add(vec_try_map(exprs, f)?),
            Expr::Mul(exprs) => Expr::Mul(vec_try_map(exprs, f)?),
        })
    }
}
