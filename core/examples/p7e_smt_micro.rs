use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use rumba_core::{
    expr::{Expr, VarId},
    simplify::{
        BinaryBitwiseDependencyCandidate,
        enumerate_binary_bitwise_dependency_candidates, simplify_mba,
    },
    varint::{VarInt, make_mask},
};
use z3::{Params, SatResult, Solver, ast::BV};

const SMT_TIMEOUT_MS: u32 = 5_000;

#[derive(Clone, Debug, PartialEq, Eq)]
enum SmtResult {
    Unsat,
    Sat { counterexample: String },
    Unknown { reason: String },
}

impl SmtResult {
    fn label(&self) -> &'static str {
        match self {
            Self::Unsat => "UNSAT",
            Self::Sat { .. } => "SAT",
            Self::Unknown { .. } => "UNKNOWN",
        }
    }
}

#[derive(Debug)]
struct DiagnosticReport {
    candidate: BinaryBitwiseDependencyCandidate,
    width_results: Vec<(u8, SmtResult)>,
    negative_control: SmtResult,
    ordinary_residual: Expr,
    ordinary_residual_zero: bool,
    final_residual_smt: SmtResult,
    semantic_result: Expr,
    semantic_result_zero: bool,
    proof_time: Duration,
}

fn fold_bitvectors(
    terms: &[Expr],
    identity: BV,
    variables: &BTreeMap<VarId, BV>,
    width: u8,
    operation: impl Fn(BV, BV) -> BV,
) -> BV {
    terms.iter().fold(identity, |left, term| {
        operation(left, expression_to_bv(term, variables, width))
    })
}

/// Exact, syntax-directed translation of RUMBA expressions to fixed-width Z3
/// bit-vectors. Arithmetic wraps naturally at `width` bits.
fn expression_to_bv(expression: &Expr, variables: &BTreeMap<VarId, BV>, width: u8) -> BV {
    let size = u32::from(width);
    let mask = make_mask(width);
    match expression {
        Expr::Var(variable) => variables[variable].clone(),
        Expr::Const(constant) => BV::from_u64(constant.get(mask), size),
        Expr::Not(inner) => expression_to_bv(inner, variables, width).bvnot(),
        Expr::Scale(coefficient, inner) => {
            BV::from_u64(coefficient.get(mask), size)
                .bvmul(&expression_to_bv(inner, variables, width))
        }
        Expr::And(terms) => fold_bitvectors(
            terms,
            BV::from_u64(mask, size),
            variables,
            width,
            |left, right| left.bvand(&right),
        ),
        Expr::Or(terms) => fold_bitvectors(
            terms,
            BV::from_u64(0, size),
            variables,
            width,
            |left, right| left.bvor(&right),
        ),
        Expr::Xor(terms) => fold_bitvectors(
            terms,
            BV::from_u64(0, size),
            variables,
            width,
            |left, right| left.bvxor(&right),
        ),
        Expr::Add(terms) => fold_bitvectors(
            terms,
            BV::from_u64(0, size),
            variables,
            width,
            |left, right| left.bvadd(&right),
        ),
        Expr::Mul(terms) => fold_bitvectors(
            terms,
            BV::from_u64(1, size),
            variables,
            width,
            |left, right| left.bvmul(&right),
        ),
    }
}

/// Ask Z3 for a counterexample to `left == right` at one fixed width.
/// P7e is not called anywhere in this certification path.
fn certify_with_smt(left: &Expr, right: &Expr, width: u8) -> SmtResult {
    let variable_ids = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .collect::<std::collections::BTreeSet<_>>();
    let variables = variable_ids
        .iter()
        .map(|variable| {
            (
                *variable,
                BV::new_const(format!("v{}", variable.0), u32::from(width)),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let left_bv = expression_to_bv(left, &variables, width);
    let right_bv = expression_to_bv(right, &variables, width);
    let solver = Solver::new();
    let mut params = Params::new();
    params.set_u32("timeout", SMT_TIMEOUT_MS);
    solver.set_params(&params);
    solver.assert(left_bv.ne(&right_bv));

    match solver.check() {
        SatResult::Unsat => SmtResult::Unsat,
        SatResult::Unknown => SmtResult::Unknown {
            reason: solver
                .get_reason_unknown()
                .unwrap_or_else(|| "no reason provided".to_owned()),
        },
        SatResult::Sat => {
            let model = solver.get_model().expect("SAT result must have a model");
            let counterexample = variable_ids
                .iter()
                .map(|variable| {
                    let value = model
                        .eval(&variables[variable], true)
                        .and_then(|value| value.as_u64())
                        .expect("bit-vector model value must fit in u64");
                    format!("v{}={value:#x}", variable.0)
                })
                .collect::<Vec<_>>()
                .join(",");
            SmtResult::Sat { counterexample }
        }
    }
}

fn certify_candidates(
    target: &Expr,
    candidates: Vec<BinaryBitwiseDependencyCandidate>,
) -> Vec<BinaryBitwiseDependencyCandidate> {
    let mut certified = Vec::new();
    for candidate in candidates {
        if candidate.pct_proved {
            certified.push(candidate);
            continue;
        }
        match certify_with_smt(target, &candidate.expression, 64) {
            SmtResult::Unsat => certified.push(candidate),
            SmtResult::Sat { counterexample } => println!(
                "rejected_candidate_truth_table={:#06b} counterexample={counterexample}",
                candidate.truth_table,
            ),
            SmtResult::Unknown { reason } => println!(
                "unknown_candidate_truth_table={:#06b} reason={reason}",
                candidate.truth_table,
            ),
        }
    }
    certified
}

fn run_diagnostic() -> Result<DiagnosticReport, String> {
    let started = Instant::now();
    let x = Expr::Var(0.into());
    let twice_x = VarInt::from(2u64) * x.clone();
    let x_and_twice_x = x.clone() & twice_x.clone();
    let target = VarInt::from((-3i64) as u64) * x.clone() + x_and_twice_x.clone();
    let parent_a = -x.clone();
    let parent_b = twice_x;

    let inferred_candidates = enumerate_binary_bitwise_dependency_candidates(
        &target,
        &parent_a,
        &parent_b,
        64,
    );
    let candidates = certify_candidates(&target, inferred_candidates);
    if candidates.is_empty() {
        return Err("no binary candidate was certified".to_owned());
    }

    let candidate = candidates
        .into_iter()
        .next()
        .expect("non-empty candidate list checked above");

    let width_results = [4, 8, 16, 32, 64]
        .into_iter()
        .map(|width| {
            let result = certify_with_smt(&target, &candidate.expression, width);
            (width, result)
        })
        .collect::<Vec<_>>();
    if let Some((width, result)) = width_results
        .iter()
        .find(|(_, result)| *result != SmtResult::Unsat)
    {
        return Err(format!(
            "candidate was not certified at width {width}: {result:?}"
        ));
    }

    let negative_control = certify_with_smt(
        &(target.clone() + Expr::make_const(1)),
        &candidate.expression,
        64,
    );
    if !matches!(negative_control, SmtResult::Sat { .. }) {
        return Err(format!(
            "negative control was expected SAT, got {negative_control:?}"
        ));
    }

    // Substitute only after exact certification, then run one ordinary pass.
    let substituted = VarInt::from((-3i64) as u64) * x
        - (parent_a & candidate.expression.clone())
        + x_and_twice_x;
    let ordinary_residual = simplify_mba(substituted, 64)
        .map_err(|error| format!("ordinary simplification failed: {error}"))?;
    let ordinary_residual_zero = ordinary_residual == Expr::zero();
    let final_residual_smt = if ordinary_residual_zero {
        SmtResult::Unsat
    } else {
        certify_with_smt(&ordinary_residual, &Expr::zero(), 64)
    };
    // This replacement is local to the diagnostic. It is authorized only by
    // an exact UNSAT certificate for `ordinary_residual != 0`.
    let semantic_result = if ordinary_residual_zero || final_residual_smt == SmtResult::Unsat {
        Expr::zero()
    } else {
        ordinary_residual.clone()
    };
    let semantic_result_zero = semantic_result == Expr::zero();

    Ok(DiagnosticReport {
        candidate,
        width_results,
        negative_control,
        ordinary_residual,
        ordinary_residual_zero,
        final_residual_smt,
        semantic_result,
        semantic_result_zero,
        proof_time: started.elapsed(),
    })
}

fn main() -> Result<(), String> {
    let report = run_diagnostic()?;
    println!(
        "candidate_truth_table={:#06b}",
        report.candidate.truth_table
    );
    println!("candidate_expression={}", report.candidate.expression);
    println!("pct_proved={}", report.candidate.pct_proved);
    for (width, result) in &report.width_results {
        println!("smt_result_width_{width}={}", result.label());
    }
    println!("negative_control={}", report.negative_control.label());
    if let SmtResult::Sat { counterexample } = &report.negative_control {
        println!("negative_control_counterexample={counterexample}");
    }
    println!("candidate_found=true");
    println!("dependency_smt={}", report.width_results[4].1.label());
    println!("ordinary_residual={}", report.ordinary_residual);
    println!(
        "ordinary_residual_zero={}",
        report.ordinary_residual_zero
    );
    println!("final_residual_smt={}", report.final_residual_smt.label());
    println!("semantic_result={}", report.semantic_result);
    println!("semantic_result_zero={}", report.semantic_result_zero);
    println!(
        "proof_time_ms={:.3}",
        report.proof_time.as_secs_f64() * 1_000.0
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certifies_the_inferred_dependency_and_exposes_the_remaining_residual() {
        let report = run_diagnostic().unwrap();
        assert!(
            report
                .width_results
                .iter()
                .all(|(_, result)| *result == SmtResult::Unsat)
        );
        assert!(matches!(report.negative_control, SmtResult::Sat { .. }));
        assert!(!report.ordinary_residual_zero);
        assert_eq!(report.final_residual_smt, SmtResult::Unsat);
        assert_eq!(report.semantic_result, Expr::zero());
        assert!(report.semantic_result_zero);
    }
}
