use std::{collections::HashMap, usize};

use crate::expr::Expr;

type Signature = [u128; 4];

/// Is an expression a sclaed bitwise expression ?
/// 2 * (v0 ^ v1) is a scaled bitwise expression
fn is_scaled_bitwise(e: &Expr) -> bool {
    if let Expr::Scale(_, e) = e {
        e.is_bitwise()
    } else {
        e.is_bitwise()
    }
}

/// Is an expression a linear MBA
fn is_linear(e: &Expr) -> bool {
    if let Expr::Add(sum) = e {
        sum.iter().all(|e| is_scaled_bitwise(e) || e.is_constant())
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

/// 2 variable version
/// Finds coefficients to express an expression with the given signature as a linear combination of -1, x, y, x&y
fn find_coeffs2(v: &Signature) -> Signature {
    let c0 = v[0];
    let c1 = v[2].wrapping_sub(v[0]);
    let c2 = v[1].wrapping_sub(v[0]);
    let c3 = v[3]
        .wrapping_add(v[0])
        .wrapping_sub(v[2])
        .wrapping_sub(v[1]);
    [c0, c1, c2, c3]
}

/// 2 variable version
/// Converts a truth table to the equivalent bitwise expression
fn tt_to_expr2(s: &Signature) -> Option<Expr> {
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
fn simple_solution2(s: Signature) -> Option<Expr> {
    let unique_values = count_values(&s);

    // Case 1
    if unique_values == 1 {
        return Some(Expr::Const(s[0]));
    }

    if unique_values == 2 {
        // Case 2
        if s[0] == 0 {
            let v = if s[1] == 0 { s[2] } else { s[1] };
            let v = if v == 0 { s[3] } else { v };
            let s = div(&s, v);
            return Some(v * tt_to_expr2(&s).unwrap());
        }

        // Case 3
        if s[1] == 2 * s[0] || s[1] == 2 * s[0] {
            let a = s[0];
            let b = if s[1] == a { s[2] } else { s[1] };
            let s = repl(&s, a, 0);
            let s = repl(&s, b, 1);
            let e = tt_to_expr2(&s).unwrap();

            return Some((-(a as i128) as u128) * (!e));
        }

        // Case 4
        let a = s[0];
        let b = if s[1] == a { s[2] } else { s[1] };

        let s = repl(&s, a, 0);
        let s = repl(&s, b, 1);
        let e = tt_to_expr2(&s).unwrap();

        return Some(Expr::Const(a) + b.wrapping_sub(a) * e);
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
    tt_to_expr2(&s)
}

/// Simplifies an MBA
fn make_mba_expr(e: &Expr) -> Expr {
    let s = get_signature(e);
    let coeffs = find_coeffs2(&s);

    return -Expr::Const(coeffs[0])
        + coeffs[1] * Expr::Var(0)
        + coeffs[2] * Expr::Var(1)
        + coeffs[3] * (Expr::Var(0) & Expr::Var(1));
}

/// Simplifies a linear MBA with 2 variables
pub fn solve_linear2(e: &Expr) -> Expr {
    let signature = get_signature(e);

    // Finds a simpler solution for certain edge cases
    if let Some(e) = simple_solution2(signature) {
        e
    } else {
        make_mba_expr(e)
    }
}

// Returns all sorted sublists of [0, t[ ordered by size
fn sorted_sublists(t: usize) -> Vec<Vec<usize>> {
    fn combine(
        nums: &Vec<usize>,
        sz: usize,
        start: usize,
        current: &mut Vec<usize>,
        result: &mut Vec<Vec<usize>>,
    ) {
        if current.len() == sz {
            result.push(current.clone());
            return;
        }
        for i in start..nums.len() {
            current.push(nums[i]);
            combine(nums, sz, i + 1, current, result);
            current.pop();
        }
    }

    let nums: Vec<usize> = (0..t).collect();
    let mut result = Vec::new();

    for sz in 1..=nums.len() {
        combine(&nums, sz, 0, &mut Vec::new(), &mut result);
    }

    result
}

// The amount of leading zeros in x's truth table
fn leading_zeros(x: usize) -> u128 {
    2u128.pow(x as u32)
}

fn sub_coeff(tt: &mut Vec<u128>, coeff: u128, index: usize, sublist: Vec<usize>) {
    let are_vars_true = |i: usize| sublist[1..].iter().copied().all(|v| ((i >> v) & 1) == 1);

    let gp_size = leading_zeros(sublist[0]) as usize;
    let period = 2 * gp_size;

    let mut start = index;
    while start < tt.len() {
        for i in start..(start + gp_size) {
            if sublist.len() == 1 || are_vars_true(i) {
                tt[i] = tt[i].wrapping_sub(coeff);
            }
        }
        start += period;
    }
}

/// Simplifies a linear MBA
fn solve_linear_inner(e: &Expr, t: usize) -> Expr {
    let mut tt = e.truth_table(2, t);

    let mut terms: Vec<Expr> = vec![];

    // The constant term
    let constant = tt[0];

    if constant != 0 {
        terms.push(Expr::Const(constant));

        for v in &mut tt {
            *v = v.wrapping_sub(constant);
        }
    }

    for sublist in sorted_sublists(t) {
        // The index of the first non zero value of this conjuction in the truth table
        let index: u128 = sublist.iter().copied().map(leading_zeros).sum();
        let coeff = tt[index as usize];

        if coeff == 0 {
            continue;
        }

        terms.push(coeff * Expr::And(sublist.iter().copied().map(|v| Expr::Var(v)).collect()));

        sub_coeff(&mut tt, coeff, index as usize, sublist);
    }

    match terms.len() {
        0 => Expr::Const(0),
        1 => terms.into_iter().next().unwrap(),
        _ => Expr::Add(terms),
    }
}

fn reduce_vars(
    e: &Expr,
    new_vars: &mut HashMap<usize, usize>,
    old_vars: &mut HashMap<usize, usize>,
    t: &mut usize,
) -> Expr {
    match e {
        Expr::Var(v) => {
            if let Some(v) = new_vars.get(&v) {
                Expr::Var(*v)
            } else {
                old_vars.insert(*t, *v);
                new_vars.insert(*v, *t);
                let v = *t;
                *t += 1;
                Expr::Var(v)
            }
        }

        _ => e.clone().map(|e| reduce_vars(&e, new_vars, old_vars, t)),
    }
}

// Resets the original variables
fn reset_vars(e: &Expr, old_vars: &HashMap<usize, usize>) -> Expr {
    match e {
        Expr::Var(v) => {
            let v = old_vars.get(&v).unwrap();
            Expr::Var(*v)
        }

        _ => e.clone().map(|e| reset_vars(&e, old_vars)),
    }
}

/// Simplifies a linear MBA
pub fn solve_linear(e: &Expr) -> Expr {
    let mut new_vars = HashMap::new();
    let mut old_vars = HashMap::new();
    let mut t = 0;

    // Reduce the number of variables in the expression
    let e = reduce_vars(&e, &mut new_vars, &mut old_vars, &mut t);

    let e = solve_linear_inner(&e, t);

    reset_vars(&e, &old_vars)
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

#[cfg(test)]
mod tests {
    use super::*; // import the outer module

    #[test]
    fn test_sorted_sublists() {
        assert_eq!(
            sorted_sublists(4),
            vec![
                vec![0],
                vec![1],
                vec![2],
                vec![3],
                vec![0, 1],
                vec![0, 2],
                vec![0, 3],
                vec![1, 2],
                vec![1, 3],
                vec![2, 3],
                vec![0, 1, 2],
                vec![0, 1, 3],
                vec![0, 2, 3],
                vec![1, 2, 3],
                vec![0, 1, 2, 3],
            ]
        )
    }
}
