use crate::expr::Expr;

type Signature = [u128; 4];

/// Is an expression a sclaed bitwise expression ?
/// 2 * (v0 ^ v1) is a scaled bitwise expression
fn is_scaled_bitwise(e: &Expr) -> bool {
    if let Expr::Mul(m) = e {
        match m.as_slice() {
            [Expr::Const(_), e] => e.is_bitwise(),
            [e, Expr::Const(_)] => e.is_bitwise(),
            _ => false,
        }
    } else {
        e.is_bitwise()
    }
}

/// Is an expression a linear MBA
fn is_linear(e: &Expr) -> bool {
    if let Expr::Add(sum) = e {
        sum.iter().all(|e| is_scaled_bitwise(e))
    } else {
        is_scaled_bitwise(e)
    }
}

/// Counts the amount of unique values in a signature
fn count_values(s: &Signature) -> usize {
    let mut count = 0;

    for i in 0..s.len() {
        if !s[..i].contains(&s[i]) {
            count += 1;
        }
    }

    count
}

/// Divides all values in a signature by a given value
fn div(s: &Signature, v: u128) -> Signature {
    s.map(|x| x / v)
}

/// Replaces all values in a signature with another one
fn repl(s: &Signature, from: u128, to: u128) -> Signature {
    s.map(|x| if x == from { to } else { x })
}

/// Finds coefficients to express an expression with the given signature as a linear combination of -1, x, y, x&y
fn find_coeffs(v: &Signature) -> Signature {
    let c0 = v[0];
    let c1 = v[2].wrapping_sub(v[0]);
    let c2 = v[1].wrapping_sub(v[0]);
    let c3 = v[3]
        .wrapping_add(v[0])
        .wrapping_sub(v[2])
        .wrapping_sub(v[1]);
    [c0, c1, c2, c3]
}

/// Converts a truth table to the equivalent bitwise expression
fn tt_to_expr(s: &Signature) -> Option<Expr> {
    let v0 = Expr::Var(0);
    let v1 = Expr::Var(1);

    match s {
        [0, 0, 0, 0] => Some(Expr::Const(0)),
        [0, 0, 0, 1] => Some(v0 & v1),
        [0, 0, 1, 0] => Some(v0 & (!v1)),
        [0, 0, 1, 1] => Some(v0),
        [0, 1, 0, 0] => Some((!v0) & v1),
        [0, 1, 0, 1] => Some(v1),
        [0, 1, 1, 0] => Some(v0 ^ v1),
        [0, 1, 1, 1] => Some(v0 | v1),
        [1, 0, 0, 0] => Some(!(v0 | v1)),
        [1, 0, 0, 1] => Some(!(v0 ^ v1)),
        [1, 0, 1, 0] => Some(!v1),
        [1, 0, 1, 1] => Some(v0 | (!v1)),
        [1, 1, 0, 0] => Some(!v0),
        [1, 1, 0, 1] => Some((!v0) | v1),
        [1, 1, 1, 0] => Some(!(v0 & v1)),
        [1, 1, 1, 1] => Some(!Expr::Const(1)),

        _ => None,
    }
}

/// Finds a simpler solution for certain edge cases
fn simple_solution(s: Signature) -> Option<Expr> {
    let unique_values = count_values(&s);

    // Case 1
    if unique_values == 1 {
        return Some(Expr::Const(s[0]));
    }

    if unique_values == 2 {
        // Case 2
        if s[0] == 0 {
            let v = if s[1] == 0 { s[2] } else { s[1] };
            let s = div(&s, v);
            return Some(Expr::Const(v) * tt_to_expr(&s).unwrap());
        }

        // Case 3
        if s[1] == 2 * s[0] || s[1] == 2 * s[0] {
            let a = s[0];
            let b = if s[1] == a { s[2] } else { s[1] };
            let s = repl(&s, a, 0);
            let s = repl(&s, b, 1);
            let e = tt_to_expr(&s).unwrap();

            return Some(-Expr::Const(a) * (!e));
        }

        // Case 4
        let a = s[0];
        let b = if s[1] == a { s[2] } else { s[1] };

        let s = repl(&s, a, 0);
        let s = repl(&s, b, 1);
        let e = tt_to_expr(&s).unwrap();

        return Some(Expr::Const(a) + Expr::Const(b.wrapping_sub(a)) * e);
    }

    None
}

/// Gets the signature vector of a linear MBA
fn get_signature(e: &Expr) -> Signature {
    [
        e.eval(&[0, 0]),
        e.eval(&[0, 1]),
        e.eval(&[1, 0]),
        e.eval(&[1, 1]),
    ]
}

/// Attempts to turn an MBA Expression into a boolean expresion
fn make_bool_expr(e: &Expr) -> Option<Expr> {
    let s = get_signature(e);
    tt_to_expr(&s)
}

/// Simplifies an MBA
fn make_mba_expr(e: &Expr) -> Expr {
    let s = get_signature(e);
    let coeffs = find_coeffs(&s);

    return -Expr::Const(coeffs[0])
        + Expr::Const(coeffs[1]) * Expr::Var(0)
        + Expr::Const(coeffs[2]) * Expr::Var(1)
        + Expr::Const(coeffs[3]) * (Expr::Var(0) & Expr::Var(1));
}

/// Simplifies a linear MBA
pub fn solve_linear(e: &Expr) -> Expr {
    let signature = get_signature(e);

    // Finds a simpler solution for certain edge cases
    if let Some(e) = simple_solution(signature) {
        e
    } else {
        make_mba_expr(e)
    }
}

#[derive(Clone, Copy)]
enum Shape {
    Arithmetic,
    Boolean,
}

fn solve_(e: Expr, shape: Shape) -> Expr {
    match e {
        // Recursion end
        // TODO: When on a constant, we might need to introduce a variable
        Expr::Var(_) | Expr::Const(_) => return e,
        _ => (),
    }

    let e = e.arith_reduce();

    if is_linear(&e) {
        match shape {
            Shape::Arithmetic => make_mba_expr(&e),
            Shape::Boolean => {
                if let Some(e) = make_bool_expr(&e) {
                    e
                } else {
                    // TODO:
                    // Expr::Var(3)
                    panic!("AAA");
                }
            }
        }
    } else {
        // We want to match the parent's type
        let shape = if e.is_arithmetic() {
            Shape::Arithmetic
        } else {
            Shape::Boolean
        };

        let e = e.map(|e| solve_(e, shape));
        e.arith_reduce()
    }
}

pub fn solve(e: Expr) -> Expr {
    solve_(e, Shape::Arithmetic)
}
