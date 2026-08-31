use std::{
    cmp::max,
    fmt::{self, Display},
    ops::{Add, BitAnd, BitOr, BitXor, Mul, Neg, Not, Sub},
    vec,
};

use rand::random_range;
use rustc_hash::FxHashSet as HashSet;

use crate::varint::{VarInt, make_mask};

#[cfg(feature = "jit")]
use crate::jit;

/// Identifies a variable within an [`Expr`] by its index.
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

/// A Mixed Boolean-Arithmetic expression tree.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Expr {
    /// A variable, referenced by its [`VarId`].
    Var(VarId),

    /// A constant value.
    Const(u64),

    // Unary
    /// Bitwise complement (`~e`).
    Not(Box<Expr>),
    /// Multiplication of an expression by a constant coefficient.
    Scale(u64, Box<Expr>),

    // Bitwise
    /// Bitwise AND of all operands.
    And(Vec<Expr>),
    /// Bitwise OR of all operands.
    Or(Vec<Expr>),
    /// Bitwise XOR of all operands.
    Xor(Vec<Expr>),

    // Arithmetic
    /// Arithmetic sum of all operands.
    Add(Vec<Expr>),
    /// Arithmetic product of all operands.
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
        Expr::product(vec![self, rhs])
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
    /// Builds a constant expression from `c`.
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

    /// Builds the canonical arithmetic product: constants and existing scales
    /// are represented by one outer [`Expr::Scale`], never as `Mul` children.
    pub(crate) fn product(terms: Vec<Expr>) -> Expr {
        let mut coefficient = 1u64;
        let mut factors = Vec::with_capacity(terms.len());
        let mut pending = terms;
        while let Some(term) = pending.pop() {
            match term {
                Expr::Const(value) => coefficient = coefficient.wrapping_mul(value),
                Expr::Scale(value, child) => {
                    coefficient = coefficient.wrapping_mul(value);
                    pending.push(*child);
                }
                Expr::Mul(mut children) => pending.append(&mut children),
                factor => factors.push(factor),
            }
        }
        if coefficient == 0 {
            return Expr::zero();
        }
        factors.reverse();
        match factors.len() {
            0 => Expr::Const(coefficient),
            1 => Expr::scale(coefficient, factors.pop().expect("one product factor")),
            _ => Expr::scale(coefficient, Expr::Mul(factors)),
        }
    }

    fn mapped_product(terms: Vec<Expr>) -> Expr {
        if terms.len() == 1 && !matches!(terms.first(), Some(Expr::Const(_) | Expr::Scale(_, _))) {
            Expr::Mul(terms)
        } else {
            Expr::product(terms)
        }
    }

    /// Whether every arithmetic product keeps constants outside its `Mul`
    /// factor list. Other historical polynomial shapes are outside this
    /// invariant.
    pub(crate) fn products_are_canonical(&self) -> bool {
        match self {
            Expr::Var(_) | Expr::Const(_) => true,
            Expr::Not(child) | Expr::Scale(_, child) => child.products_are_canonical(),
            Expr::Mul(children) => children.iter().all(|child| {
                !matches!(child, Expr::Const(_) | Expr::Scale(_, _))
                    && child.products_are_canonical()
            }),
            Expr::And(children)
            | Expr::Or(children)
            | Expr::Xor(children)
            | Expr::Add(children) => children.iter().all(Expr::products_are_canonical),
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
            // A single-element collection is semantically its sole child, so it
            // binds exactly as tightly (see also `repr_masked`, which renders
            // straight through it).
            Expr::And(e) | Expr::Or(e) | Expr::Xor(e) | Expr::Add(e) | Expr::Mul(e)
                if e.len() == 1 =>
            {
                e[0].precedence()
            }

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
        // Strict `<`: every operator here is flat and associative/commutative,
        // so a same-precedence child never needs grouping. This also drops the
        // cosmetic parens around a degenerate single-child node (e.g. `(v1)`).
        if parent.precedence() < self.precedence() {
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

        // A single-element collection node is just its child; render through it
        // so no spurious `(v1)` wrapper appears.
        if let Expr::And(e) | Expr::Or(e) | Expr::Xor(e) | Expr::Add(e) | Expr::Mul(e) = self
            && e.len() == 1
        {
            return e[0].repr_masked(n, mask, hex, latex);
        }

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

            Expr::Add(exprs) => {
                let star = if latex { "\\cdot" } else { "*" };
                let sign_bit = 1u64 << (n - 1);

                // Splits a term into (is_negative, magnitude). A negative
                // constant or a `Scale` with a negative coefficient is folded
                // into a subtraction so the sum reads `a - b` rather than
                // `a + (-b)`. Everything else keeps its normal, unsigned repr.
                let term = |e: &Expr| -> (bool, String) {
                    match e {
                        Expr::Const(c) if c & mask & sign_bit != 0 => {
                            let m = VarInt::from(c.wrapping_neg());
                            (true, m.repr(n, mask, hex, latex))
                        }
                        Expr::Scale(c, inner) if c & mask & sign_bit != 0 => {
                            let m = c.wrapping_neg() & mask;
                            // The magnitude sits in a product context, so the
                            // inner expression is parenthesized against `Mul`.
                            let is = inner.parenthesize(
                                &Expr::Mul(vec![]),
                                inner.repr_masked(n, mask, hex, latex),
                            );
                            if m == 1 {
                                (true, is)
                            } else {
                                let cs = VarInt::from(m).repr(n, mask, hex, latex);
                                (true, format!("{cs} {star} {is}"))
                            }
                        }
                        _ => (false, recurs(e)),
                    }
                };

                let mut out = String::new();
                for (i, e) in exprs.iter().enumerate() {
                    let (neg, s) = term(e);
                    if i == 0 {
                        out.push_str(&if neg { format!("-{s}") } else { s });
                    } else {
                        out.push_str(if neg { " - " } else { " + " });
                        out.push_str(&s);
                    }
                }
                out
            }

            Expr::And(exprs) | Expr::Or(exprs) | Expr::Xor(exprs) | Expr::Mul(exprs) => {
                join(exprs, &format!(" {} ", self.symbol(latex)))
            }
        }
    }

    /// A string representation of this expression on `n` bits.
    pub fn repr(&self, n: u8, hex: bool, latex: bool) -> String {
        self.repr_masked(n, make_mask(n), hex, latex)
    }

    /// Whether this expression is purely bitwise (no arithmetic operators).
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

    /// Whether every variable in this expression is contained in `allowed_vars`.
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
            Expr::Mul(exprs) => Expr::mapped_product(vec_map(exprs, f)),
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
            Expr::Mul(exprs) => Expr::mapped_product(vec_try_map(exprs, f)?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_product_extracts_constants_and_scales() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        assert_eq!(Expr::Const(2) * x.clone(), Expr::scale(2, x.clone()));
        assert_eq!(x.clone() * Expr::Const(2), Expr::scale(2, x.clone()));
        assert_eq!(
            Expr::scale(2, x.clone()) * Expr::scale(3, y.clone()),
            Expr::scale(6, Expr::Mul(vec![x.clone(), y.clone()]))
        );
        assert_eq!(Expr::Const(0) * x.clone() * y, Expr::zero());
        assert_eq!(
            Expr::Mul(vec![Expr::Const(7), x.clone()]).reduce(64),
            Expr::scale(7, x)
        );
    }
}
