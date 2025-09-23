use std::{
    fmt,
    ops::{Add, BitAnd, BitOr, BitXor, Mul, Neg, Not, Shl, Shr, Sub},
    u128, vec,
};

use crate::expr;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Expr {
    Var(usize),

    Const(u128),

    // Unary
    Not(Box<Expr>),
    Neg(Box<Expr>),

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
        let to_string = |e: &Expr| e.parenthesize(self, e.to_string());

        let mut join = |exprs: &Vec<Expr>, c: &str| {
            write!(
                f,
                "{}",
                exprs.iter().map(to_string).collect::<Vec<String>>().join(c)
            )
        };

        match self {
            Expr::Var(i) => write!(f, "v{}", i),
            Expr::Const(c) => {
                if *c as i128 >= 0 {
                    write!(f, "{}", c)
                } else {
                    write!(f, "({})", *c as i128)
                }
            }
            Expr::Not(expr) => write!(f, "!{}", to_string(expr)),
            Expr::Neg(expr) => write!(f, "-{}", to_string(expr)),

            Expr::And(exprs) => join(exprs, " & "),
            Expr::Or(exprs) => join(exprs, " | "),
            Expr::Xor(exprs) => join(exprs, " ^ "),
            Expr::Add(exprs) => join(exprs, " + "),
            Expr::Sub(exprs) => join(exprs, " - "),
            Expr::Mul(exprs) => join(exprs, " * "),

            _ => todo!(), // Expr::Shl(exprs) => join(exprs, " << "),
                          // Expr::Shr(exprs) => join(exprs, " >> "),
                          // Expr::RshiftS(exprs) => join(exprs, " >>s "),
                          // Expr::Le(exprs) => join(exprs, " <= "),
                          // Expr::Lt(exprs) => join(exprs, " < "),
                          // Expr::Ge(exprs) => join(exprs, " >= "),
                          // Expr::Gt(exprs) => join(exprs, " > "),
                          // Expr::LeS(exprs) => join(exprs, " <=s "),
                          // Expr::LtS(exprs) => join(exprs, " <s "),
                          // Expr::GeS(exprs) => join(exprs, " >=s "),
                          // Expr::GtS(exprs) => join(exprs, " >s "),
                          // Expr::Ne(exprs) => join(exprs, " != "),
                          // Expr::Eq(exprs) => join(exprs, " == "),
        }
    }
}

impl<T: Into<u128>> From<T> for Expr {
    fn from(value: T) -> Self {
        let value: u128 = value.into();

        Expr::Const(value)
    }
}

impl Expr {
    pub fn size(&self) -> usize {
        match self {
            Expr::Var(_) | Expr::Const(_) => 1,

            Expr::Not(expr) | Expr::Neg(expr) => expr.size(),

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

            Expr::Neg(expr) => !expr.eval(vars),
        }
    }

    // Calculates the truth table of an expression on n values with t variables
    pub fn truth_table(&self, n: usize, t: usize) -> Vec<u128> {
        let size = n.pow(t as u32);
        let mut tt = vec![0u128; size];

        for i in 0..size {
            // Decode i into base-N digits (one value per variable)
            let mut vars = vec![0u128; t];
            let mut idx = i;
            for v in (0..t).rev() {
                vars[v] = (idx % n) as u128;
                idx /= n;
            }

            tt[i] = self.eval(&vars) % n as u128;
        }

        tt
    }

    // Operator precedence
    // https://en.cppreference.com/w/c/language/operator_precedence.html
    fn precedence(&self) -> usize {
        match self {
            Expr::Var(_) | Expr::Const(_) => 0,

            Expr::Not(_) | Expr::Neg(_) => 2,

            Expr::Mul(_) => 3,

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

    // Parenthesizes an expression if needed
    fn parenthesize(&self, parent: &Expr, s: String) -> String {
        if parent.precedence() < self.precedence() {
            format!("({})", s)
        } else {
            s
        }
    }

    pub fn latex(&self) -> String {
        let to_latex = |e: &Expr| e.parenthesize(self, e.latex());

        let join = |exprs: &Vec<Expr>, c: &str| {
            format!(
                "{}",
                exprs.iter().map(to_latex).collect::<Vec<String>>().join(c)
            )
        };

        match self {
            Expr::Var(i) => format!("v_{{{}}}", i),
            Expr::Const(c) => {
                if (*c as i128) < 0 {
                    format!("({})", *c as i128)
                } else {
                    format!("{}", c)
                }
            }
            Expr::Not(e) => format!("\\neg {}", to_latex(e)),
            Expr::Neg(e) => format!("-{}", to_latex(e)),

            Expr::And(exprs) => join(exprs, " \\wedge "),
            Expr::Or(exprs) => join(exprs, " \\vee "),
            Expr::Xor(exprs) => join(exprs, " \\oplus "),
            Expr::Add(exprs) => join(exprs, " + ").replace("+ -", "-"),
            Expr::Sub(exprs) => join(exprs, " - "),
            Expr::Mul(exprs) => join(exprs, " \\cdot "),

            _ => panic!("MISSING LATEX"),
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

    // Is this a bitwise expression
    pub fn is_bitwise(&self) -> bool {
        match self {
            Expr::Var(_) | Expr::Const(_) => true,

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

            Expr::Not(expr) | Expr::Neg(expr) => expr.variables_in(allowed_vars),

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

    pub fn simplify(self) -> Self {
        match self {
            Expr::Not(expr) => match *expr {
                Expr::Not(x) => x.simplify(),

                Expr::Const(v) => Expr::Const(!v),

                Expr::And(exprs) => Expr::Or(
                    exprs
                        .into_iter()
                        .map(|e| Expr::Not(Box::new(e)).simplify())
                        .collect(),
                ),

                Expr::Or(exprs) => Expr::And(
                    exprs
                        .into_iter()
                        .map(|e| Expr::Not(Box::new(e)).simplify())
                        .collect(),
                ),

                _ => Expr::Not(Box::new(expr.simplify())),
            },

            Expr::Neg(expr) => match *expr {
                Expr::Neg(x) => x.simplify(),
                Expr::Const(v) => Expr::Const(-(v as i128) as u128),
                _ => Expr::Mul(vec![Expr::Const(-1i128 as u128), expr.simplify()]),
            },

            Expr::And(exprs) => {
                // The constant term
                let mut c = u128::MAX;

                // First, flatten nested ANDs and filter trivial elements
                let mut flat = Vec::with_capacity(exprs.capacity());
                for e in exprs.into_iter().map(|e| e.simplify()) {
                    match e {
                        Expr::And(inner) => flat.extend(inner),
                        Expr::Const(0) => return Expr::Const(0),
                        Expr::Const(v) => c &= v,
                        other => flat.push(other),
                    }
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
                            distributed_terms.push(Expr::And(copy).simplify()); // recursive call, simplified incrementally
                        }
                        return Expr::Xor(distributed_terms).simplify();
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
                // The constant term
                let mut c = 0u128;

                // First, flatten nested ORs and filter trivial elements
                let mut flat = Vec::with_capacity(exprs.capacity());
                for e in exprs.into_iter().map(|e| e.simplify()) {
                    match e {
                        Expr::Or(inner) => flat.extend(inner),
                        Expr::Const(v) => c |= v,
                        other => flat.push(other),
                    }
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
                            distributed_terms.push(Expr::Or(copy).simplify()); // recursive call, simplified incrementally
                        }
                        return Expr::And(distributed_terms).simplify();
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
                // The constant term
                let mut c = 0u128;

                let mut flat = Vec::with_capacity(exprs.len());
                for e in exprs.into_iter().map(|e| e.simplify()) {
                    match e {
                        Expr::Xor(inner) => flat.extend(inner),
                        Expr::Const(v) => c ^= v,
                        other => flat.push(other),
                    }
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
                // The constant term
                let mut c = 0u128;

                let mut flat = Vec::with_capacity(exprs.len());
                for e in exprs.into_iter().map(|e| e.simplify()) {
                    match e {
                        Expr::Add(inner) => flat.extend(inner),
                        Expr::Const(v) => c = c.wrapping_add(v),
                        other => flat.push(other),
                    }
                }

                if c != 0 {
                    flat.push(Expr::Const(c));
                }

                match flat.len() {
                    0 => Expr::Const(0),
                    1 => flat.into_iter().next().unwrap(),
                    _ => Expr::Add(flat),
                }
            }

            Expr::Sub(exprs) => match exprs.len() {
                0 => Expr::Const(0),
                1 => exprs.into_iter().next().unwrap().simplify(),
                _ => {
                    let mut terms = Vec::with_capacity(exprs.len());
                    terms.push(exprs[0].clone());
                    for i in 1..exprs.len() {
                        terms.push(Expr::Neg(Box::new(exprs[i].clone())));
                    }
                    Expr::Add(terms)
                }
            },

            Expr::Mul(exprs) => {
                // The constant term
                let mut c: u128 = 1;

                let mut flat = Vec::with_capacity(exprs.len());
                for e in exprs.into_iter().map(|e| e.simplify()) {
                    match e {
                        Expr::Mul(inner) => flat.extend(inner),
                        Expr::Const(v) => c = c.wrapping_mul(v),
                        other => flat.push(other),
                    }
                }

                if c != 1 {
                    flat.push(Expr::Const(c));
                }

                // If any child is Add, distribute Mul over Add
                if let Some(pos) = flat.iter().position(|f| matches!(f, Expr::Add(_))) {
                    if let Expr::Add(terms) = flat.remove(pos) {
                        let mut distributed_terms = Vec::new();
                        for term in terms {
                            let mut copy = flat.clone();
                            copy.insert(pos, term);
                            distributed_terms.push(Expr::Mul(copy).simplify()); // recursive call, simplified incrementally
                        }
                        return Expr::Add(distributed_terms).simplify();
                    }
                }

                match flat.len() {
                    0 => Expr::Const(1),
                    1 => flat.into_iter().next().unwrap(),
                    _ => Expr::Mul(flat),
                }
            }

            _ => self,
        }
    }
}
