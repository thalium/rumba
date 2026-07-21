// The remaining `unwrap()`s here read the children of a `Pair` that pest only
// produces after matching `expr.pest`; the grammar guarantees the shape, so a
// missing child means the grammar and this code have drifted apart — a bug to
// surface loudly, not an input error. Numeric conversions (which *can* fail on
// grammar-valid input) are handled with explicit errors above.
#![allow(clippy::unwrap_used)]

use pest::Parser;
use pest_derive::Parser;

use crate::{
    expr::Expr,
    lang::{Insn, InsnKind, Program},
};

#[derive(Parser)]
#[grammar = "expr.pest"]
pub struct RumbaParser;

macro_rules! build_nary {
    ($pair:expr, $name:ident) => {{
        let mut inner = $pair.into_inner();
        let first = build_expr(inner.next().unwrap())?;
        let rest = inner.map(build_expr).collect::<Result<Vec<_>, _>>()?;
        if rest.is_empty() {
            first
        } else {
            Expr::$name(std::iter::once(first).chain(rest).collect())
        }
    }};
}

fn build_expr(pair: pest::iterators::Pair<Rule>) -> Result<Expr, String> {
    let expr = match pair.as_rule() {
        Rule::dec_number => {
            let val = pair
                .as_str()
                .parse::<u64>()
                .map_err(|_| format!("integer literal out of range: {}", pair.as_str()))?;
            Expr::Const(val.into())
        }

        Rule::hex_number => {
            let val = u64::from_str_radix(&pair.as_str()[2..], 16)
                .map_err(|_| format!("hex literal out of range: {}", pair.as_str()))?;
            Expr::Const(val.into())
        }

        Rule::var => {
            let idx = pair.as_str()[1..]
                .parse::<usize>()
                .map_err(|_| format!("variable index out of range: {}", pair.as_str()))?;
            Expr::Var(idx.into())
        }

        Rule::unary => {
            let mut inner = pair.into_inner();

            let first = inner.next().unwrap();

            match first.as_rule() {
                Rule::unary_op => {
                    let op = first.as_str();
                    let rhs = build_expr(inner.next().unwrap())?;
                    match op {
                        "~" | "!" => !rhs,
                        "-" => -rhs,
                        _ => unreachable!(),
                    }
                }
                Rule::atom => build_expr(first)?,
                _ => unreachable!(),
            }
        }
        Rule::or => build_nary!(pair, Or),
        Rule::xor => build_nary!(pair, Xor),
        Rule::and => build_nary!(pair, And),
        Rule::mul => build_nary!(pair, Mul),

        Rule::add => {
            let mut inner = pair.into_inner();

            // Start with the first term
            let first = build_expr(inner.next().unwrap())?;

            let mut exprs = vec![first];

            // Handle remaining (op, mul) pairs
            while let Some(pair) = inner.next() {
                let op_str = pair.as_str();
                let rhs = build_expr(inner.next().unwrap())?;

                match op_str {
                    "+" => exprs.push(rhs),
                    "-" => exprs.push(-rhs),
                    _ => unreachable!(),
                }
            }

            if exprs.len() == 1 {
                exprs.into_iter().next().unwrap()
            } else {
                Expr::Add(exprs)
            }
        }

        Rule::number | Rule::expr | Rule::atom => build_expr(pair.into_inner().next().unwrap())?,
        _ => unreachable!("unexpected rule: {:?}", pair.as_rule()),
    };

    Ok(expr)
}

pub fn parse_expr(input: &str) -> Result<Expr, String> {
    let mut pairs = RumbaParser::parse(Rule::expr, input).map_err(|e| e.to_string())?;

    build_expr(pairs.next().unwrap())
}

pub fn parse_program(input: &str) -> Result<Program, String> {
    let mut program = Program::default();

    let mut pairs = RumbaParser::parse(Rule::program, input).map_err(|e| e.to_string())?;
    let program_pair = pairs.next().unwrap();

    for pair in program_pair.into_inner() {
        if pair.as_rule() != Rule::statement {
            continue;
        }

        for stmt in pair.into_inner() {
            let (ty, id, kind) = match stmt.as_rule() {
                Rule::unknown => {
                    let mut inner = stmt.into_inner();
                    let type_str = inner.next().unwrap().as_str();
                    let var_str = inner.next().unwrap().as_str();

                    let t: u8 = type_str[1..]
                        .parse()
                        .map_err(|_| format!("type width out of range: {type_str}"))?;
                    let var_id: usize = var_str[1..]
                        .parse()
                        .map_err(|_| format!("variable index out of range: {var_str}"))?;

                    let mut unknown_vars = Vec::new();
                    if let Some(vars_pair) = inner.next() {
                        for v in vars_pair.into_inner() {
                            let v_id: usize = v.as_str()[1..]
                                .parse()
                                .map_err(|_| format!("variable index out of range: {}", v.as_str()))?;
                            unknown_vars.push(v_id.into());
                        }
                    }

                    (t, var_id.into(), InsnKind::Unknown(unknown_vars))
                }

                Rule::assign => {
                    let mut inner = stmt.into_inner();
                    let type_str = inner.next().unwrap().as_str();
                    let var_str = inner.next().unwrap().as_str();

                    let t: u8 = type_str[1..]
                        .parse()
                        .map_err(|_| format!("type width out of range: {type_str}"))?;
                    let var_id: usize = var_str[1..]
                        .parse()
                        .map_err(|_| format!("variable index out of range: {var_str}"))?;

                    let expr_pair = inner.next().unwrap();
                    let expr = build_expr(expr_pair)?;

                    (t, var_id.into(), InsnKind::Assign(expr))
                }
                _ => unreachable!(),
            };

            program.push(Insn { ty, id, kind })?;
        }
    }

    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_program() {
        let input = r#"
# Define our variables
u8 v0 = unknown()
u8 v1 = unknown()

# A classic MBA
u8 v2 = v0 ^ v1
u8 v3 = v0 & v1
u8 v4 = v2 + 2 * v3

# This line is our output
u8 v5 = unknown(v4)
"#;

        let mut program = parse_program(input).expect("Failed to parse");

        println!("Program:\n{}", program);

        // Optional: basic assertions
        assert_eq!(program.len(), 6);
        assert_eq!(
            *program.get_index(0).unwrap(),
            Insn {
                ty: 8,
                id: 0.into(),
                kind: InsnKind::Unknown(vec![])
            }
        );
        assert_eq!(
            *program.get_index(1).unwrap(),
            Insn {
                ty: 8,
                id: 1.into(),
                kind: InsnKind::Unknown(vec![])
            }
        );
        assert_eq!(
            *program.get_index(2).unwrap(),
            Insn {
                ty: 8,
                id: 2.into(),
                kind: InsnKind::Assign(Expr::Var(0.into()) ^ Expr::Var(1.into()))
            }
        );
        assert_eq!(
            *program.get_index(3).unwrap(),
            Insn {
                ty: 8,
                id: 3.into(),
                kind: InsnKind::Assign(Expr::Var(0.into()) & Expr::Var(1.into()))
            }
        );
        assert_eq!(
            *program.get_index(4).unwrap(),
            Insn {
                ty: 8,
                id: 4.into(),
                kind: InsnKind::Assign(
                    Expr::Var(2.into()) + Expr::Const(2.into()) * Expr::Var(3.into())
                )
            }
        );
        assert_eq!(
            *program.get_index(5).unwrap(),
            Insn {
                ty: 8,
                id: 5.into(),
                kind: InsnKind::Unknown(vec![4.into()])
            }
        );

        program.simplify().expect("Error during simplification");
        println!("{}", program);
    }

    #[test]
    fn oversized_literal_errors_instead_of_panicking() {
        // u64::MAX + 1 matches the grammar but does not fit in u64.
        let err = parse_expr("18446744073709551616").unwrap_err();
        assert!(err.contains("out of range"), "unexpected error: {err}");
    }
}
