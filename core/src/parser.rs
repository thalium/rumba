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
        let mut inner = $pair.into_inner().map(build_expr);
        let first = inner.next().unwrap();
        let rest: Vec<_> = inner.collect();
        if rest.is_empty() {
            first
        } else {
            Expr::$name(std::iter::once(first).chain(rest).collect())
        }
    }};
}

fn build_expr(pair: pest::iterators::Pair<Rule>) -> Expr {
    match pair.as_rule() {
        Rule::dec_number => {
            let val = pair.as_str().parse::<u64>().unwrap();
            Expr::Const(val.into())
        }

        Rule::hex_number => {
            let val = u64::from_str_radix(&pair.as_str()[2..], 16).unwrap();
            Expr::Const(val.into())
        }

        Rule::var => {
            let idx = pair.as_str()[1..].parse::<usize>().unwrap();
            Expr::Var(idx.into())
        }

        Rule::unary => {
            let mut inner = pair.into_inner();

            let first = inner.next().unwrap();

            match first.as_rule() {
                Rule::unary_op => {
                    let op = first.as_str();
                    let rhs = build_expr(inner.next().unwrap());
                    match op {
                        "~" | "!" => !rhs,
                        "-" => -rhs,
                        _ => unreachable!(),
                    }
                }
                Rule::atom => build_expr(first),
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
            let first = build_expr(inner.next().unwrap());

            let mut exprs = vec![first];

            // Handle remaining (op, mul) pairs
            while let Some(pair) = inner.next() {
                let op_str = pair.as_str();
                let rhs = build_expr(inner.next().unwrap());

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

        Rule::number | Rule::expr | Rule::atom => build_expr(pair.into_inner().next().unwrap()),
        _ => panic!("{:?}", pair),
    }
}

pub fn parse_expr(input: &str) -> Result<Expr, String> {
    let mut pairs = RumbaParser::parse(Rule::expr, input).map_err(|e| e.to_string())?;

    Ok(build_expr(pairs.next().unwrap()))
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

                    let t: u8 = type_str[1..].parse().unwrap();
                    let var_id: usize = var_str[1..].parse().unwrap();

                    let mut unknown_vars = Vec::new();
                    if let Some(vars_pair) = inner.next() {
                        for v in vars_pair.into_inner() {
                            let v_id: usize = v.as_str()[1..].parse().unwrap();
                            unknown_vars.push(v_id.into());
                        }
                    }

                    (t, var_id.into(), InsnKind::Unknown(unknown_vars))
                }

                Rule::assign => {
                    let mut inner = stmt.into_inner();
                    let type_str = inner.next().unwrap().as_str();
                    let var_str = inner.next().unwrap().as_str();

                    let t: u8 = type_str[1..].parse().unwrap();
                    let var_id: usize = var_str[1..].parse().unwrap();

                    let expr_pair = inner.next().unwrap();
                    let expr = build_expr(expr_pair);

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
        assert_eq!(program.insns.len(), 6);
        assert_eq!(
            program.insns[0],
            Insn {
                ty: 8,
                id: 0.into(),
                kind: InsnKind::Unknown(vec![])
            }
        );
        assert_eq!(
            program.insns[1],
            Insn {
                ty: 8,
                id: 1.into(),
                kind: InsnKind::Unknown(vec![])
            }
        );
        assert_eq!(
            program.insns[2],
            Insn {
                ty: 8,
                id: 2.into(),
                kind: InsnKind::Assign(Expr::Var(0.into()) ^ Expr::Var(1.into()))
            }
        );
        assert_eq!(
            program.insns[3],
            Insn {
                ty: 8,
                id: 3.into(),
                kind: InsnKind::Assign(Expr::Var(0.into()) & Expr::Var(1.into()))
            }
        );
        assert_eq!(
            program.insns[4],
            Insn {
                ty: 8,
                id: 4.into(),
                kind: InsnKind::Assign(
                    Expr::Var(2.into()) + Expr::Const(2.into()) * Expr::Var(3.into())
                )
            }
        );
        assert_eq!(
            program.insns[5],
            Insn {
                ty: 8,
                id: 5.into(),
                kind: InsnKind::Unknown(vec![4.into()])
            }
        );

        program.simplify().expect("Error during simplification");
        println!("{}", program);
    }
}
