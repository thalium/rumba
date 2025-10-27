use std::{
    collections::{HashMap, HashSet},
    fmt,
    ops::{Add, BitAnd, BitOr, BitXor, Mul, Neg, Not, Shl, Shr, Sub},
    u128, vec,
};

use crate::expr;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Binop {
    pub left: Box<Expr>,
    pub right: Box<Expr>,
}

impl Binop {
    pub fn new(left: Expr, right: Expr) -> Self {
        Self {
            left: Box::new(left),
            right: Box::new(right),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Expr {
    Var(usize),

    Const(u128),

    // Unary
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Scale(u128, Box<Expr>),

    // Bitwise
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Xor(Vec<Expr>),

    // Shifts
    Shl(Binop),
    Shr(Binop),
    RshiftS(Binop),

    // Comp
    Le(Binop),
    Lt(Binop),
    Ge(Binop),
    Gt(Binop),
    LeS(Binop),
    LtS(Binop),
    GeS(Binop),
    GtS(Binop),
    Ne(Binop),
    Eq(Binop),

    // Arithmetic
    Add(Vec<Expr>),
    Sub(Vec<Expr>),
    Mul(Vec<Expr>),
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

impl Shl for Expr {
    type Output = Expr;

    fn shl(self, rhs: Self) -> Self::Output {
        Expr::Shl(Binop::new(self, rhs))
    }
}

impl Shr for Expr {
    type Output = Expr;

    fn shr(self, rhs: Self) -> Self::Output {
        Expr::Shr(Binop::new(self, rhs))
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
        Expr::Sub(vec![self, rhs])
    }
}

impl Mul for Expr {
    type Output = Expr;

    fn mul(self, rhs: Self) -> Self::Output {
        Expr::Mul(vec![self, rhs])
    }
}

impl Mul<Expr> for u128 {
    type Output = Expr;

    fn mul(self, rhs: Expr) -> Self::Output {
        if self == 0 {
            Expr::Const(0)
        } else if self == 1 {
            rhs
        } else {
            Expr::Scale(self, Box::new(rhs))
        }
    }
}

impl Not for Expr {
    type Output = Expr;

    fn not(self) -> Self::Output {
        Expr::Not(Box::new(self))
    }
}

impl Neg for Expr {
    type Output = Expr;

    fn neg(self) -> Self::Output {
        Expr::Neg(Box::new(self))
    }
}

pub type TruthTable = [u128; 256 * 256];

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.repr(32, true, false))
    }
}

impl<T: Into<u128>> From<T> for Expr {
    fn from(value: T) -> Self {
        let value: u128 = value.into();

        Expr::Const(value)
    }
}

/// Displays a constant properly (with the best sign)
fn display_const(c: u128, n: u32, hex: bool, latex: bool) -> String {
    let mask = if n == 128 {
        u128::MAX
    } else {
        (1u128 << n) - 1
    };

    let val = c & mask;
    let sign_bit = 1u128 << (n - 1);

    if val & sign_bit != 0 {
        let signed = (val as i128) - (1i128 << n);
        match (hex, latex) {
            (true, true) => format!("(-\\mathrm{{{:#x}}})", -signed),
            (true, false) => format!("(-{:#x})", -signed),
            _ => format!("(-{})", -signed),
        }
    } else {
        match (hex, latex) {
            (true, true) => format!("\\mathrm{{{:#x}}}", val),
            (true, false) => format!("{:#x}", val),
            _ => format!("{}", val),
        }
    }
}

impl Expr {
    /// Counts the number of nodes in the expression
    pub fn size(&self) -> usize {
        match self {
            Expr::Var(_) | Expr::Const(_) => 1,

            Expr::Not(expr) | Expr::Neg(expr) | Expr::Scale(_, expr) => expr.size(),

            Expr::Shl(Binop { left, right })
            | Expr::Shr(Binop { left, right })
            | Expr::RshiftS(Binop { left, right })
            | Expr::Le(Binop { left, right })
            | Expr::Lt(Binop { left, right })
            | Expr::Ge(Binop { left, right })
            | Expr::Gt(Binop { left, right })
            | Expr::LeS(Binop { left, right })
            | Expr::LtS(Binop { left, right })
            | Expr::GeS(Binop { left, right })
            | Expr::GtS(Binop { left, right })
            | Expr::Ne(Binop { left, right })
            | Expr::Eq(Binop { left, right }) => left.size() + right.size(),

            Expr::And(exprs)
            | Expr::Or(exprs)
            | Expr::Xor(exprs)
            | Expr::Add(exprs)
            | Expr::Sub(exprs)
            | Expr::Mul(exprs) => exprs.iter().map(|e| e.size()).sum(),
        }
    }

    /// Evaluates the expression with the given variable values
    pub fn eval(&self, vars: &[u128]) -> u128 {
        match self {
            Expr::Var(i) => vars[*i],
            Expr::Const(c) => *c,

            Expr::And(exprs) => exprs
                .iter()
                .map(|e| e.eval(vars))
                .fold(u128::MAX, |x, y| x & y),

            Expr::Or(exprs) => exprs.iter().map(|e| e.eval(vars)).fold(0, |x, y| x | y),

            Expr::Xor(exprs) => exprs.iter().map(|e| e.eval(vars)).fold(0, |x, y| x ^ y),

            Expr::Add(exprs) => exprs
                .iter()
                .map(|e| e.eval(vars))
                .fold(0, |x, y| x.wrapping_add(y)),

            Expr::Sub(exprs) => {
                let mut iter = exprs.iter().map(|e| e.eval(vars));
                if let Some(first) = iter.next() {
                    iter.fold(first, |x, y| x.wrapping_sub(y))
                } else {
                    0
                }
            }

            Expr::Mul(exprs) => exprs
                .iter()
                .map(|e| e.eval(vars))
                .fold(1, |x, y| x.wrapping_mul(y)),

            Expr::Scale(v, e) => v.wrapping_mul(e.eval(vars)),

            Expr::Not(e) => !e.eval(vars),

            Expr::Shl(b) => b.left.eval(vars).wrapping_shl(b.right.eval(vars) as u32),
            Expr::Shr(b) => b.left.eval(vars).wrapping_shr(b.right.eval(vars) as u32),

            Expr::RshiftS(b) => {
                let l = b.left.eval(vars) as i128;
                let r = b.right.eval(vars) as u32;
                (l >> r) as u128
            }

            Expr::Le(b) => (b.left.eval(vars) <= b.right.eval(vars)) as u128,
            Expr::Lt(b) => (b.left.eval(vars) < b.right.eval(vars)) as u128,
            Expr::Ge(b) => (b.left.eval(vars) >= b.right.eval(vars)) as u128,
            Expr::Gt(b) => (b.left.eval(vars) > b.right.eval(vars)) as u128,
            Expr::Ne(b) => (b.left.eval(vars) != b.right.eval(vars)) as u128,
            Expr::Eq(b) => (b.left.eval(vars) == b.right.eval(vars)) as u128,

            Expr::LeS(b) => ((b.left.eval(vars) as i128) <= (b.right.eval(vars) as i128)) as u128,
            Expr::LtS(b) => ((b.left.eval(vars) as i128) < (b.right.eval(vars) as i128)) as u128,
            Expr::GeS(b) => ((b.left.eval(vars) as i128) >= (b.right.eval(vars) as i128)) as u128,
            Expr::GtS(b) => ((b.left.eval(vars) as i128) > (b.right.eval(vars) as i128)) as u128,

            Expr::Neg(expr) => -(expr.eval(vars) as i128) as u128,
        }
    }

    /// Calculates the truth table of an expression on n values with t variables
    pub fn truth_table(&self, n: usize, t: usize) -> Vec<u128> {
        let size = n.pow(t as u32);
        let mut tt = vec![0u128; size];

        for i in 0..size {
            // Decode i into base-N digits (one value per variable)
            let mut vars = vec![0u128; t];
            let mut idx = i;
            for v in 0..t {
                vars[v] = (idx % n) as u128;
                idx /= n;
            }

            tt[i] = self.eval(&vars) as u128;
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

            Expr::Not(expr) | Expr::Neg(expr) | Expr::Scale(_, expr) => {
                let v = vec![expr.visit::<T, F>(f.clone())];
                f(self, v)
            }

            Expr::And(exprs)
            | Expr::Or(exprs)
            | Expr::Xor(exprs)
            | Expr::Add(exprs)
            | Expr::Sub(exprs)
            | Expr::Mul(exprs) => {
                let v = exprs.iter().map(|e| e.visit(f.clone())).collect();
                f(self, v)
            }

            Expr::Shl(binop)
            | Expr::Shr(binop)
            | Expr::RshiftS(binop)
            | Expr::Le(binop)
            | Expr::Lt(binop)
            | Expr::Ge(binop)
            | Expr::Gt(binop)
            | Expr::LeS(binop)
            | Expr::LtS(binop)
            | Expr::GeS(binop)
            | Expr::GtS(binop)
            | Expr::Ne(binop)
            | Expr::Eq(binop) => {
                let v = vec![binop.left.visit(f.clone()), binop.right.visit(f.clone())];
                f(self, v)
            }
        }
    }

    /// Counts the number of variables in the expression
    pub fn get_vars(&self) -> HashSet<usize> {
        self.visit(|e, children: Vec<HashSet<usize>>| {
            let mut acc: HashSet<usize> = children.into_iter().flatten().collect();
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

            Expr::Not(_) | Expr::Neg(_) => 2,

            Expr::Mul(_) | Expr::Scale(_, _) => 3,

            Expr::Add(_) | Expr::Sub(_) => 4,

            Expr::Shl(_) | Expr::Shr(_) | Expr::RshiftS(_) => 5,

            Expr::Le(_)
            | Expr::Lt(_)
            | Expr::Ge(_)
            | Expr::Gt(_)
            | Expr::LeS(_)
            | Expr::LtS(_)
            | Expr::GeS(_)
            | Expr::GtS(_) => 6,

            Expr::Ne(_) | Expr::Eq(_) => 7,

            Expr::And(_) => 8,

            Expr::Xor(_) => 9,

            Expr::Or(_) => 10,
        }
    }

    /// Parenthesizes an expression if needed
    fn parenthesize(&self, parent: &Expr, s: String) -> String {
        if parent.precedence() < self.precedence() {
            format!("({})", s)
        } else {
            s
        }
    }

    fn symbol(&self, latex: bool) -> &str {
        match (self, latex) {
            (Expr::Var(_), _) | (Expr::Const(_), _) => "",

            (Expr::Not(_), true) => "\\neg",
            (Expr::Not(_), false) => "~",

            (Expr::Neg(_), _) => "-",

            (Expr::Scale(_, _), true) => "\\cdot",
            (Expr::Scale(_, _), false) => "*",

            (Expr::And(_), true) => "\\land",
            (Expr::And(_), false) => "&",

            (Expr::Or(_), true) => "\\lor",
            (Expr::Or(_), false) => "|",

            (Expr::Xor(_), true) => "\\oplus",
            (Expr::Xor(_), false) => "^",

            (Expr::Shl(_), true) => "\\ll",
            (Expr::Shl(_), false) => "<<",

            (Expr::Shr(_), true) => "\\gg",
            (Expr::Shr(_), false) => ">>",

            (Expr::RshiftS(_), true) => "\\gg_s",
            (Expr::RshiftS(_), false) => ">>s",

            (Expr::Le(_), true) => "\\le",
            (Expr::Le(_), false) => "<=",

            (Expr::Lt(_), _) => "<",

            (Expr::Ge(_), true) => "\\ge",
            (Expr::Ge(_), false) => ">=",

            (Expr::Gt(_), _) => ">",

            (Expr::LeS(_), true) => "\\le_s",
            (Expr::LeS(_), false) => "<=s",

            (Expr::LtS(_), true) => "<_s",
            (Expr::LtS(_), false) => "<",

            (Expr::GeS(_), true) => "\\ge_s",
            (Expr::GeS(_), false) => ">=s",

            (Expr::GtS(_), true) => ">_s",
            (Expr::GtS(_), false) => ">s",

            (Expr::Ne(_), true) => "\\ne",
            (Expr::Ne(_), false) => "",

            (Expr::Eq(_), _) => "=",

            (Expr::Add(_), _) => "+",

            (Expr::Sub(_), _) => "-",

            (Expr::Mul(_), true) => "\\cdot",
            (Expr::Mul(_), false) => "*",
        }
    }

    /// A string representation of this expression
    pub fn repr(&self, n: u32, hex: bool, latex: bool) -> String {
        let recurs = |e: &Expr| e.parenthesize(self, e.repr(n, hex, latex));

        let join = |exprs: &Vec<Expr>, c: &str| {
            format!(
                "{}",
                exprs.iter().map(recurs).collect::<Vec<String>>().join(c)
            )
        };

        match self {
            Expr::Var(v) => {
                if latex {
                    format!("v_{{{}}}", v)
                } else {
                    format!("v{}", v)
                }
            }

            Expr::Const(c) => display_const(*c, n, hex, latex),

            Expr::Scale(c, expr) => format!(
                "{} {} {}",
                display_const(*c, n, hex, latex),
                self.symbol(latex),
                recurs(&*expr)
            ),

            Expr::Not(expr) | Expr::Neg(expr) => {
                format!("{} {}", self.symbol(latex), recurs(&*expr))
            }

            Expr::And(exprs)
            | Expr::Or(exprs)
            | Expr::Xor(exprs)
            | Expr::Add(exprs)
            | Expr::Sub(exprs)
            | Expr::Mul(exprs) => join(exprs, &format!(" {} ", self.symbol(latex))),

            Expr::Shl(binop)
            | Expr::Shr(binop)
            | Expr::RshiftS(binop)
            | Expr::Le(binop)
            | Expr::Lt(binop)
            | Expr::Ge(binop)
            | Expr::Gt(binop)
            | Expr::LeS(binop)
            | Expr::LtS(binop)
            | Expr::GeS(binop)
            | Expr::GtS(binop)
            | Expr::Ne(binop)
            | Expr::Eq(binop) => format!(
                "{} {} {}",
                recurs(&*binop.left),
                self.symbol(latex),
                recurs(&*binop.right)
            ),
        }
    }

    // pub fn random<R: Rng>(rng: &mut R, depth: u32, n_vars: usize) -> Self {
    //     if depth == 0 || rng.random_bool(0.2) {
    //         // leaf
    //         if rng.random_bool(0.5) {
    //             Expr::Var(rng.random_range(0..n_vars))
    //         } else {
    //             Expr::Const(rng.random::<u128>())
    //         }
    //     } else {
    //         // choose an operator
    //         match rng.random_range(0..7) {
    //             0 => Expr::And(Binop::random(rng, depth, n_vars)),
    //             1 => Expr::Or(Binop::random(rng, depth, n_vars)),
    //             2 => Expr::Xor(Binop::random(rng, depth, n_vars)),
    //             3 => Expr::Not(Box::new(Expr::random(rng, depth, n_vars))),
    //             4 => Expr::Add(Binop::random(rng, depth, n_vars)),
    //             5 => Expr::Sub(Binop::random(rng, depth, n_vars)),
    //             6 => Expr::Mul(Binop::random(rng, depth, n_vars)),

    //             7 => Expr::Shl(Binop::random(rng, depth, n_vars)),
    //             8 => Expr::Shr(Binop::random(rng, depth, n_vars)),
    //             9 => Expr::RshiftS(Binop::random(rng, depth, n_vars)),
    //             10 => Expr::Le(Binop::random(rng, depth, n_vars)),
    //             11 => Expr::Lt(Binop::random(rng, depth, n_vars)),
    //             12 => Expr::Ge(Binop::random(rng, depth, n_vars)),
    //             13 => Expr::Gt(Binop::random(rng, depth, n_vars)),
    //             14 => Expr::Ne(Binop::random(rng, depth, n_vars)),
    //             15 => Expr::Eq(Binop::random(rng, depth, n_vars)),
    //             16 => Expr::LeS(Binop::random(rng, depth, n_vars)),
    //             17 => Expr::LtS(Binop::random(rng, depth, n_vars)),
    //             18 => Expr::GeS(Binop::random(rng, depth, n_vars)),
    //             19 => Expr::GtS(Binop::random(rng, depth, n_vars)),
    //             _ => unreachable!(),
    //         }
    //         .simplify()
    //     }
    // }

    // Is this a constant
    pub fn is_constant(&self) -> bool {
        match self {
            Expr::Const(_) => true,
            _ => false,
        }
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

            Expr::Var(i) => allowed_vars.contains(i),

            Expr::Not(expr) | Expr::Neg(expr) | Expr::Scale(_, expr) => {
                expr.variables_in(allowed_vars)
            }

            Expr::Shl(Binop { left, right })
            | Expr::Shr(Binop { left, right })
            | Expr::RshiftS(Binop { left, right })
            | Expr::Le(Binop { left, right })
            | Expr::Lt(Binop { left, right })
            | Expr::Ge(Binop { left, right })
            | Expr::Gt(Binop { left, right })
            | Expr::LeS(Binop { left, right })
            | Expr::LtS(Binop { left, right })
            | Expr::GeS(Binop { left, right })
            | Expr::GtS(Binop { left, right })
            | Expr::Ne(Binop { left, right })
            | Expr::Eq(Binop { left, right }) => {
                left.variables_in(allowed_vars) && right.variables_in(allowed_vars)
            }

            Expr::And(exprs)
            | Expr::Or(exprs)
            | Expr::Xor(exprs)
            | Expr::Add(exprs)
            | Expr::Sub(exprs)
            | Expr::Mul(exprs) => exprs.iter().all(|e| e.variables_in(allowed_vars)),
        }
    }

    // Replaces a given var with another expression
    pub fn replace_var(self, target_var: usize, replacement: &Expr) -> Self {
        self.map(|e| match e {
            Expr::Var(v) if v == target_var => replacement.clone(),
            _ => e,
        })
    }

    pub fn group_terms(self) -> Self {
        match self {
            Expr::Add(exprs) => {
                let initial_len = exprs.len();
                let mut map = HashMap::<Expr, usize>::new();

                let mut add_expr = |e, c| {
                    if let Some(count) = map.get_mut(&e) {
                        *count = count.wrapping_add(c);
                    } else {
                        map.insert(e, c);
                    }
                };

                for e in exprs.into_iter() {
                    if let Expr::Scale(c, e) = e {
                        add_expr(*e, c as usize);
                    } else {
                        add_expr(e, 1);
                    }
                }

                let mut out = Vec::with_capacity(map.len());

                for (e, count) in map {
                    if count == 0 {
                        continue;
                    }

                    out.push((count as u128) * e);
                }

                out.sort();

                if initial_len > out.len() {
                    Expr::Add(out).arith_reduce()
                } else {
                    Expr::Add(out)
                }
            }
            _ => self,
        }
    }

    pub fn arith_reduce(self) -> Self {
        match self {
            Expr::Not(expr) => match *expr {
                Expr::Not(x) => x.arith_reduce(),

                Expr::Const(v) => Expr::Const(!v),

                Expr::And(exprs) => Expr::Or(
                    exprs
                        .into_iter()
                        .map(|e| Expr::Not(Box::new(e)).arith_reduce())
                        .collect(),
                ),

                Expr::Or(exprs) => Expr::And(
                    exprs
                        .into_iter()
                        .map(|e| Expr::Not(Box::new(e)).arith_reduce())
                        .collect(),
                ),

                _ => Expr::Not(Box::new(expr.arith_reduce())),
            },

            Expr::Neg(expr) => match *expr {
                Expr::Neg(x) => x.arith_reduce(),
                Expr::Const(v) => Expr::Const(-(v as i128) as u128),
                _ => (-1i128 as u128) * expr.arith_reduce(),
            },

            Expr::Scale(0, _) => Expr::Const(0),
            Expr::Scale(1, e) => e.arith_reduce(),
            Expr::Scale(c1, e) => match e.arith_reduce() {
                Expr::Const(c2) => Expr::Const(c1.wrapping_mul(c2)),
                Expr::Scale(c2, e) => c1.wrapping_mul(c2) * *e,
                Expr::Add(sum) => {
                    Expr::Add(sum.into_iter().map(|e| c1 * e).collect()).arith_reduce()
                }
                other => c1 * other,
            },

            Expr::And(exprs) => {
                fn collect(e: Expr, c: &mut u128, out: &mut Vec<Expr>) {
                    match e.arith_reduce() {
                        Expr::And(inner) => {
                            for e in inner {
                                collect(e, c, out);
                            }
                        }

                        Expr::Const(v) => {
                            *c &= v;
                        }

                        other => out.push(other),
                    }
                }

                let mut c = u128::MAX;
                let mut flat = Vec::with_capacity(exprs.len());

                for e in exprs.into_iter() {
                    collect(e, &mut c, &mut flat);
                }

                if c == 0 {
                    return Expr::Const(0);
                }

                if c != u128::MAX {
                    flat.push(Expr::Const(c));
                }

                // If any child is XOR, distribute AND over XOR
                if let Some(pos) = flat.iter().position(|f| matches!(f, Expr::Xor(_))) {
                    if let Expr::Xor(xor_terms) = flat.remove(pos) {
                        let mut distributed_terms = Vec::new();
                        for term in xor_terms {
                            let mut copy = flat.clone();
                            copy.insert(pos, term);
                            distributed_terms.push(Expr::And(copy).arith_reduce()); // recursive call, simplified incrementally
                        }
                        return Expr::Xor(distributed_terms).arith_reduce();
                    }
                }

                // Deduplicate
                flat.sort();
                flat.dedup();

                match flat.len() {
                    0 => Expr::Const(u128::MAX),
                    1 => flat[0].clone(),
                    _ => Expr::And(flat),
                }
            }

            Expr::Or(exprs) => {
                fn collect(e: Expr, c: &mut u128, out: &mut Vec<Expr>) {
                    match e.arith_reduce() {
                        Expr::Or(inner) => {
                            for e in inner {
                                collect(e, c, out);
                            }
                        }

                        Expr::Const(v) => {
                            *c |= v;
                        }

                        other => out.push(other),
                    }
                }

                let mut c = 0;
                let mut flat = Vec::with_capacity(exprs.len());

                for e in exprs.into_iter() {
                    collect(e, &mut c, &mut flat);
                }

                if c != 0u128 {
                    flat.push(Expr::Const(c));
                }

                // If any child is AND, distribute OR over AND
                if let Some(pos) = flat.iter().position(|f| matches!(f, Expr::And(_))) {
                    if let Expr::And(and_terms) = flat.remove(pos) {
                        let mut distributed_terms = Vec::new();
                        for term in and_terms {
                            let mut copy = flat.clone();
                            copy.insert(pos, term);
                            distributed_terms.push(Expr::Or(copy).arith_reduce()); // recursive call, simplified incrementally
                        }
                        return Expr::And(distributed_terms).arith_reduce();
                    }
                }

                // Deduplicate
                flat.sort();
                flat.dedup();

                match flat.len() {
                    0 => Expr::Const(0),
                    1 => flat[0].clone(),
                    _ => Expr::Or(flat),
                }
            }

            Expr::Xor(exprs) => {
                // Flatten
                fn collect(e: Expr, c: &mut u128, out: &mut Vec<Expr>) {
                    match e.arith_reduce() {
                        Expr::Xor(inner) => {
                            for e in inner {
                                collect(e, c, out);
                            }
                        }

                        Expr::Const(v) => {
                            *c ^= v;
                        }

                        other => out.push(other),
                    }
                }

                let mut c = 0;
                let mut flat = Vec::with_capacity(exprs.len());

                for e in exprs.into_iter() {
                    collect(e, &mut c, &mut flat);
                }

                if c != 0 {
                    flat.push(Expr::Const(c));
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
                    0 => Expr::Const(0),
                    1 => reduced[0].clone(),
                    _ => Expr::Xor(reduced),
                }
            }

            Expr::Add(exprs) => {
                // Flatten
                fn collect(e: Expr, c: &mut u128, out: &mut Vec<Expr>) {
                    match e.arith_reduce() {
                        Expr::Add(inner) => {
                            for e in inner {
                                collect(e, c, out);
                            }
                        }

                        Expr::Const(v) => {
                            *c = c.wrapping_add(v);
                        }

                        other => out.push(other),
                    }
                }

                let mut c: u128 = 0;
                let mut flat = Vec::with_capacity(exprs.len());

                for e in exprs.into_iter() {
                    collect(e, &mut c, &mut flat);
                }

                if c != 0 {
                    flat.push(Expr::Const(c));
                }

                flat.sort();

                match flat.len() {
                    0 => Expr::Const(0),
                    1 => flat.into_iter().next().unwrap(),
                    _ => Expr::Add(flat).group_terms(),
                }
            }

            Expr::Sub(exprs) => match exprs.len() {
                0 => Expr::Const(0),
                1 => exprs.into_iter().next().unwrap().arith_reduce(),
                _ => {
                    let mut terms = Vec::with_capacity(exprs.len());
                    terms.push(exprs[0].clone());
                    for i in 1..exprs.len() {
                        terms.push(-exprs[i].clone());
                    }
                    Expr::Add(terms).arith_reduce()
                }
            },

            Expr::Mul(exprs) => {
                // Flatten
                fn collect(e: Expr, c: &mut u128, out: &mut Vec<Expr>) {
                    match e.arith_reduce() {
                        Expr::Mul(inner) => {
                            for e in inner {
                                collect(e, c, out);
                            }
                        }

                        Expr::Const(v) => {
                            *c = c.wrapping_mul(v);
                        }

                        Expr::Scale(v, e) => {
                            *c = c.wrapping_mul(v);
                            collect(*e, c, out);
                        }

                        other => out.push(other),
                    }
                }

                let mut c: u128 = 1;
                let mut flat = Vec::with_capacity(exprs.len());

                for e in exprs.into_iter() {
                    collect(e, &mut c, &mut flat);
                }

                if c == 0 {
                    return Expr::Const(0);
                }

                // If any child is Add, distribute Mul over Add
                if let Some(pos) = flat.iter().position(|f| matches!(f, Expr::Add(_))) {
                    if let Expr::Add(terms) = flat.remove(pos) {
                        let mut distributed_terms = Vec::new();
                        for term in terms {
                            let mut copy = flat.clone();
                            copy.insert(pos, c * term);
                            distributed_terms.push(Expr::Mul(copy).arith_reduce()); // recursive call, simplified incrementally
                        }
                        return Expr::Add(distributed_terms).arith_reduce();
                    }
                }

                flat.sort();

                match flat.len() {
                    0 => Expr::Const(c),
                    1 => (c * flat.into_iter().next().unwrap()).arith_reduce(),
                    _ => {
                        if c != 1 {
                            (c * Expr::Mul(flat)).arith_reduce()
                        } else {
                            Expr::Mul(flat)
                        }
                    }
                }
            }

            Expr::Shl(b) => match (*b.left, *b.right) {
                (Expr::Const(c1), Expr::Const(c2)) => Expr::Const(c1 << c2),
                (l, Expr::Const(n)) => (2u128.pow(n as u32) * l).arith_reduce(),
                (l, r) => Expr::Shl(Binop::new(l.arith_reduce(), r.arith_reduce())),
            },

            Expr::Shr(b) => match (*b.left, *b.right) {
                (Expr::Const(c1), Expr::Const(c2)) => Expr::Const(c1 >> c2),
                (l, r) => Expr::Shr(Binop::new(l.arith_reduce(), r.arith_reduce())),
            },

            _ => self,
        }
    }

    /// Is the outer most expression a boolean expression
    pub fn is_bool(&self) -> bool {
        match self {
            Expr::Not(_)
            | Expr::And(_)
            | Expr::Or(_)
            | Expr::Xor(_)
            | Expr::Shl(_)
            | Expr::Shr(_) => true,

            _ => false,
        }
    }

    /// Is the outer most expression an arithmetic expression
    pub fn is_arithmetic(&self) -> bool {
        match self {
            Expr::Neg(_) | Expr::Add(_) | Expr::Sub(_) | Expr::Mul(_) | Expr::Scale(_, _) => true,
            _ => false,
        }
    }

    // Helper recusive function that needs a reference
    pub fn map<F>(self, mut f: F) -> Self
    where
        F: FnMut(Self) -> Self,
    {
        let vec_map = |exprs: Vec<Expr>, f: F| exprs.into_iter().map(f).collect();
        let binop_map = |b: Binop, mut f: F| Binop::new(f(*b.left), f(*b.right));

        match self {
            Expr::Var(_) | Expr::Const(_) => self,

            Expr::Not(expr) => !f(*expr),
            Expr::Neg(expr) => -f(*expr),
            Expr::Scale(v, expr) => v * f(*expr),

            Expr::And(exprs) => Expr::And(vec_map(exprs, f)),
            Expr::Or(exprs) => Expr::Or(vec_map(exprs, f)),
            Expr::Xor(exprs) => Expr::Xor(vec_map(exprs, f)),
            Expr::Add(exprs) => Expr::Add(vec_map(exprs, f)),
            Expr::Sub(exprs) => Expr::Sub(vec_map(exprs, f)),
            Expr::Mul(exprs) => Expr::Mul(vec_map(exprs, f)),

            Expr::Shl(binop) => Expr::Shl(binop_map(binop, f)),
            Expr::Shr(binop) => Expr::Shr(binop_map(binop, f)),
            Expr::RshiftS(binop) => Expr::RshiftS(binop_map(binop, f)),
            Expr::Le(binop) => Expr::Le(binop_map(binop, f)),
            Expr::Lt(binop) => Expr::Lt(binop_map(binop, f)),
            Expr::Ge(binop) => Expr::Ge(binop_map(binop, f)),
            Expr::Gt(binop) => Expr::Gt(binop_map(binop, f)),
            Expr::LeS(binop) => Expr::LeS(binop_map(binop, f)),
            Expr::LtS(binop) => Expr::LtS(binop_map(binop, f)),
            Expr::GeS(binop) => Expr::GeS(binop_map(binop, f)),
            Expr::GtS(binop) => Expr::GtS(binop_map(binop, f)),
            Expr::Ne(binop) => Expr::Ne(binop_map(binop, f)),
            Expr::Eq(binop) => Expr::Eq(binop_map(binop, f)),
        }
    }

    pub fn mod_simplify(self, n: u32) -> Self {
        self.map(|e| match e {
            Expr::Const(c) => Expr::Const(c % 2u128.pow(n)),
            Expr::Scale(v, e) => (v % 2u128.pow(n)) * *e,
            _ => e.mod_simplify(n),
        })
    }

    pub fn canonic(self) -> Self {
        let vec_map = |exprs: Vec<Expr>| {
            let mut r: Vec<Expr> = exprs.into_iter().map(|e| e.canonic()).collect();
            r.sort();
            r
        };

        let binop_map = |b: Binop| Binop::new(b.left.canonic(), b.right.canonic());

        match self {
            Expr::Var(_) | Expr::Const(_) => self,

            Expr::Not(expr) => !expr.canonic(),
            Expr::Neg(expr) => (-1i128 as u128) * expr.canonic(),
            Expr::Scale(v, expr) => v * expr.canonic(),

            Expr::And(exprs) => Expr::And(vec_map(exprs)),
            Expr::Or(exprs) => Expr::Or(vec_map(exprs)),
            Expr::Xor(exprs) => Expr::Xor(vec_map(exprs)),
            Expr::Add(exprs) => Expr::Add(vec_map(exprs)),
            Expr::Sub(_) => todo!(),
            Expr::Mul(exprs) => Expr::Mul(vec_map(exprs)),

            Expr::Shl(binop) => Expr::Shl(binop_map(binop)),
            Expr::Shr(binop) => Expr::Shr(binop_map(binop)),
            Expr::RshiftS(binop) => Expr::RshiftS(binop_map(binop)),
            Expr::Le(binop) => Expr::Le(binop_map(binop)),
            Expr::Lt(binop) => Expr::Lt(binop_map(binop)),
            Expr::Ge(binop) => Expr::Ge(binop_map(binop)),
            Expr::Gt(binop) => Expr::Gt(binop_map(binop)),
            Expr::LeS(binop) => Expr::LeS(binop_map(binop)),
            Expr::LtS(binop) => Expr::LtS(binop_map(binop)),
            Expr::GeS(binop) => Expr::GeS(binop_map(binop)),
            Expr::GtS(binop) => Expr::GtS(binop_map(binop)),
            Expr::Ne(binop) => Expr::Ne(binop_map(binop)),
            Expr::Eq(binop) => Expr::Eq(binop_map(binop)),
        }
    }
}
