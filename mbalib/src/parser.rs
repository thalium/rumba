use pest::Parser;
use pest_derive::Parser;

use crate::expr::{Binop, Expr};

#[derive(Parser)]
#[grammar = "expr.pest"]
pub struct ExprParser;

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
        Rule::number => {
            let val = pair.as_str().parse::<u128>().unwrap();
            Expr::Const(val)
        }
        Rule::var => {
            let idx = pair.as_str()[1..].parse::<usize>().unwrap();
            Expr::Var(idx)
        }
        Rule::unary => {
            let mut inner = pair.into_inner();

            let first = inner.next().unwrap();

            match first.as_rule() {
                Rule::unary_op => {
                    let op = first.as_str();
                    let rhs = build_expr(inner.next().unwrap());
                    match op {
                        "!" => Expr::Not(Box::new(rhs)),
                        "-" => Expr::Neg(Box::new(rhs)),
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
        Rule::add => build_nary!(pair, Add),
        Rule::mul => build_nary!(pair, Mul),

        Rule::shift => {
            // handle comparisons here and wrap into Eq, Lt, etc.
            // requires peeking at operator string
            let mut pairs = pair.into_inner();
            let lhs = build_expr(pairs.next().unwrap());
            if let Some(op_pair) = pairs.next() {
                let op = op_pair.as_str();
                let rhs = build_expr(pairs.next().unwrap());
                match op {
                    "<<" => Expr::Shl(Binop::new(lhs, rhs)),
                    ">>" => Expr::Shr(Binop::new(lhs, rhs)),
                    _ => unreachable!(),
                }
            } else {
                lhs
            }
        }

        Rule::comp => {
            // handle comparisons here and wrap into Eq, Lt, etc.
            // requires peeking at operator string
            let mut pairs = pair.into_inner();
            let lhs = build_expr(pairs.next().unwrap());
            if let Some(op_pair) = pairs.next() {
                let op = op_pair.as_str();
                let rhs = build_expr(pairs.next().unwrap());
                match op {
                    "<" => Expr::Lt(Binop::new(lhs, rhs)),
                    "<=" => Expr::Le(Binop::new(lhs, rhs)),
                    ">" => Expr::Gt(Binop::new(lhs, rhs)),
                    ">=" => Expr::Ge(Binop::new(lhs, rhs)),
                    "==" => Expr::Eq(Binop::new(lhs, rhs)),
                    "!=" => Expr::Ne(Binop::new(lhs, rhs)),
                    _ => unreachable!(),
                }
            } else {
                lhs
            }
        }
        Rule::expr => build_expr(pair.into_inner().next().unwrap()),
        Rule::atom => build_expr(pair.into_inner().next().unwrap()),
        _ => panic!("{:?}", pair),
    }
}

pub fn parse_expr(input: &str) -> Result<Expr, String> {
    let mut pairs = ExprParser::parse(Rule::expr, input).map_err(|e| e.to_string())?;

    Ok(build_expr(pairs.next().unwrap()))
}
