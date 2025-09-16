use mbalib::expr::{Binop, Expr, TruthTable};

fn standard_binop() -> Binop {
    Binop {
        left: Box::new(Expr::Var(0)),
        right: Box::new(Expr::Var(1)),
    }
}

// Precompute all standard operations
fn standard_ops() -> Vec<(String, TruthTable)> {
    let mut ops = Vec::new();

    let mut add_op = |op: Expr| ops.push((op.to_string(), op.truth_table()));

    // add_op(Expr::Var(0));
    // add_op(Expr::Var(1));
    // add_op(Expr::Const(0));
    // add_op(Expr::Const(1));
    add_op(Expr::And(standard_binop()));
    add_op(Expr::Or(standard_binop()));
    add_op(Expr::Xor(standard_binop()));
    // add_op(Expr::Not(Box::new(Expr::Var(0))));
    // add_op(Expr::Not(Box::new(Expr::Var(1))));
    add_op(Expr::Lshift(standard_binop()));
    add_op(Expr::Rshift(standard_binop()));
    add_op(Expr::RshiftS(standard_binop()));
    add_op(Expr::Le(standard_binop()));
    add_op(Expr::Lt(standard_binop()));
    add_op(Expr::Ge(standard_binop()));
    add_op(Expr::Gt(standard_binop()));
    add_op(Expr::LeS(standard_binop()));
    add_op(Expr::LtS(standard_binop()));
    add_op(Expr::GeS(standard_binop()));
    add_op(Expr::GtS(standard_binop()));
    add_op(Expr::Ne(standard_binop()));
    add_op(Expr::Eq(standard_binop()));
    add_op(Expr::Plus(standard_binop()));
    add_op(Expr::Minus(standard_binop()));
    add_op(Expr::Times(standard_binop()));

    ops
}

fn match_standard(
    expr: &Expr,
    standard: &[(String, TruthTable)],
    counts: &mut [usize],
) -> Option<String> {
    let tt = expr.truth_table();
    for (name, std_tt) in standard {
        // Statistics on appeared values
        for v in tt {
            counts[v as usize] += 1;
        }

        if &tt == std_tt && name != &expr.to_string() {
            return Some(name.clone());
        }
    }
    None
}

fn main() {
    let mut rng = rand::rng();

    let d = 42;
    let m = (1u64 << 63) / d as u64 + 1;

    let s = 0;

    let k = 63 + s;

    for _ in 0..10 {
        let n: u32 = rand::random();
        println!(
            "{} / {} = {} ({}, {})",
            n,
            d,
            ((n as u128) * (m as u128)) >> k,
            n / d,
            (((n as u128) * (m as u128)) >> k) as u32 == (n / d)
        );
    }
}
