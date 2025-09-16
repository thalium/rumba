use rand::Rng;
use std::fmt;

#[derive(Clone)]
pub struct Binop {
    pub left: Box<Expr>,
    pub right: Box<Expr>,
}

impl Binop {
    fn random<R: Rng>(rng: &mut R, depth: u32, n_vars: usize) -> Self {
        Self {
            left: Box::new(Expr::random(rng, depth - 1, n_vars)),
            right: Box::new(Expr::random(rng, depth - 1, n_vars)),
        }
    }
}

#[derive(Clone)]
pub enum Expr {
    Var(usize),

    Const(u8),

    // Bitwise
    And(Binop),
    Or(Binop),
    Xor(Binop),
    Not(Box<Expr>),
    Lshift(Binop),
    Rshift(Binop),
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
    Plus(Binop),
    Minus(Binop),
    Times(Binop),
}

pub type TruthTable = [u8; 256 * 256];

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Var(i) => write!(f, "v{}", i),
            Expr::Const(c) => write!(f, "{}", c),

            // Bitwise
            Expr::And(b) => write!(f, "({} & {})", b.left, b.right),
            Expr::Or(b) => write!(f, "({} | {})", b.left, b.right),
            Expr::Xor(b) => write!(f, "({} ^ {})", b.left, b.right),
            Expr::Not(e) => write!(f, "(~{})", e),
            Expr::Lshift(b) => write!(f, "({} << {})", b.left, b.right),
            Expr::Rshift(b) => write!(f, "({} >> {})", b.left, b.right),
            Expr::RshiftS(b) => write!(f, "({} >>s {})", b.left, b.right),

            // Unsigned comparisons
            Expr::Le(b) => write!(f, "({} <= {})", b.left, b.right),
            Expr::Lt(b) => write!(f, "({} < {})", b.left, b.right),
            Expr::Ge(b) => write!(f, "({} >= {})", b.left, b.right),
            Expr::Gt(b) => write!(f, "({} > {})", b.left, b.right),
            Expr::Ne(b) => write!(f, "({} != {})", b.left, b.right),
            Expr::Eq(b) => write!(f, "({} == {})", b.left, b.right),

            // Signed comparisons
            Expr::LeS(b) => write!(f, "({} <=s {})", b.left, b.right),
            Expr::LtS(b) => write!(f, "({} <s {})", b.left, b.right),
            Expr::GeS(b) => write!(f, "({} >=s {})", b.left, b.right),
            Expr::GtS(b) => write!(f, "({} >s {})", b.left, b.right),

            // Arithmetic
            Expr::Plus(b) => write!(f, "({} + {})", b.left, b.right),
            Expr::Minus(b) => write!(f, "({} - {})", b.left, b.right),
            Expr::Times(b) => write!(f, "({} * {})", b.left, b.right),
        }
    }
}

impl Expr {
    pub fn eval(&self, vars: &[u8]) -> u8 {
        match self {
            Expr::Var(i) => vars[*i],
            Expr::Const(c) => *c,

            // Bitwise (wrapping not needed for &, |, ^, ~)
            Expr::And(b) => b.left.eval(vars) & b.right.eval(vars),
            Expr::Or(b) => b.left.eval(vars) | b.right.eval(vars),
            Expr::Xor(b) => b.left.eval(vars) ^ b.right.eval(vars),
            Expr::Not(e) => !e.eval(vars),
            Expr::Lshift(b) => b.left.eval(vars).wrapping_shl(b.right.eval(vars) as u32),
            Expr::Rshift(b) => b.left.eval(vars).wrapping_shr(b.right.eval(vars) as u32),
            Expr::RshiftS(b) => {
                let l = b.left.eval(vars) as i8;
                let r = b.right.eval(vars) as u32;
                l.wrapping_shr(r) as u8
            }

            // Unsigned comparisons (still 0/1)
            Expr::Le(b) => (b.left.eval(vars) <= b.right.eval(vars)) as u8,
            Expr::Lt(b) => (b.left.eval(vars) < b.right.eval(vars)) as u8,
            Expr::Ge(b) => (b.left.eval(vars) >= b.right.eval(vars)) as u8,
            Expr::Gt(b) => (b.left.eval(vars) > b.right.eval(vars)) as u8,
            Expr::Ne(b) => (b.left.eval(vars) != b.right.eval(vars)) as u8,
            Expr::Eq(b) => (b.left.eval(vars) == b.right.eval(vars)) as u8,

            // Signed comparisons
            Expr::LeS(b) => ((b.left.eval(vars) as i8) <= (b.right.eval(vars) as i8)) as u8,
            Expr::LtS(b) => ((b.left.eval(vars) as i8) < (b.right.eval(vars) as i8)) as u8,
            Expr::GeS(b) => ((b.left.eval(vars) as i8) >= (b.right.eval(vars) as i8)) as u8,
            Expr::GtS(b) => ((b.left.eval(vars) as i8) > (b.right.eval(vars) as i8)) as u8,

            // Arithmetic
            Expr::Plus(b) => b.left.eval(vars).wrapping_add(b.right.eval(vars)),
            Expr::Minus(b) => b.left.eval(vars).wrapping_sub(b.right.eval(vars)),
            Expr::Times(b) => b.left.eval(vars).wrapping_mul(b.right.eval(vars)),
        }
    }

    pub fn truth_table(&self) -> TruthTable {
        let mut tt = [0u8; 256 * 256];

        for x in 0..=255u8 {
            for y in 0..=255u8 {
                tt[(x as usize) * 256 + (y as usize)] = self.eval(&[x, y]);
            }
        }

        tt
    }

    pub fn random<R: Rng>(rng: &mut R, depth: u32, n_vars: usize) -> Self {
        if depth == 0 || rng.random_bool(0.2) {
            // leaf
            if rng.random_bool(0.5) {
                Expr::Var(rng.random_range(0..n_vars))
            } else {
                Expr::Const(rng.random::<u8>())
            }
        } else {
            // choose an operator
            match rng.random_range(0..7) {
                0 => Expr::And(Binop::random(rng, depth, n_vars)),
                1 => Expr::Or(Binop::random(rng, depth, n_vars)),
                2 => Expr::Xor(Binop::random(rng, depth, n_vars)),
                3 => Expr::Not(Box::new(Expr::random(rng, depth, n_vars))),
                4 => Expr::Plus(Binop::random(rng, depth, n_vars)),
                5 => Expr::Minus(Binop::random(rng, depth, n_vars)),
                6 => Expr::Times(Binop::random(rng, depth, n_vars)),

                7 => Expr::Lshift(Binop::random(rng, depth, n_vars)),
                8 => Expr::Rshift(Binop::random(rng, depth, n_vars)),
                9 => Expr::RshiftS(Binop::random(rng, depth, n_vars)),
                10 => Expr::Le(Binop::random(rng, depth, n_vars)),
                11 => Expr::Lt(Binop::random(rng, depth, n_vars)),
                12 => Expr::Ge(Binop::random(rng, depth, n_vars)),
                13 => Expr::Gt(Binop::random(rng, depth, n_vars)),
                14 => Expr::Ne(Binop::random(rng, depth, n_vars)),
                15 => Expr::Eq(Binop::random(rng, depth, n_vars)),
                16 => Expr::LeS(Binop::random(rng, depth, n_vars)),
                17 => Expr::LtS(Binop::random(rng, depth, n_vars)),
                18 => Expr::GeS(Binop::random(rng, depth, n_vars)),
                19 => Expr::GtS(Binop::random(rng, depth, n_vars)),
                _ => unreachable!(),
            }
        }
    }
}
