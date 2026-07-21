use std::collections::{BTreeMap, BTreeSet};

use rumba_core::{
    expr::{Expr, VarId},
    p8::{experiment_pipeline, select_lowbit_above},
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, simplify_mba},
    varint::make_mask,
};
use z3::{Params, SatResult, Solver, ast::BV};

const WIDTH: u8 = 64;
const SMT_TIMEOUT_MS: u32 = 5_000;
const DATASETS: [(&str, &str); 7] = [
    ("loki_tiny.csv", include_str!("../../third_party/dataset/loki_tiny.csv")),
    ("mba_flatten.csv", include_str!("../../third_party/dataset/mba_flatten.csv")),
    (
        "mba_obf_linear.csv",
        include_str!("../../third_party/dataset/mba_obf_linear.csv"),
    ),
    (
        "mba_obf_nonlinear.csv",
        include_str!("../../third_party/dataset/mba_obf_nonlinear.csv"),
    ),
    ("neureduce.csv", include_str!("../../third_party/dataset/neureduce.csv")),
    ("qsynth_ea.csv", include_str!("../../third_party/dataset/qsynth_ea.csv")),
    ("syntia.csv", include_str!("../../third_party/dataset/syntia.csv")),
];

fn fold_bv(
    terms: &[Expr],
    identity: BV,
    variables: &BTreeMap<VarId, BV>,
    operation: impl Fn(BV, BV) -> BV,
) -> BV {
    terms.iter().fold(identity, |left, term| {
        operation(left, expression_to_bv(term, variables))
    })
}

fn expression_to_bv(expression: &Expr, variables: &BTreeMap<VarId, BV>) -> BV {
    let bits = u32::from(WIDTH);
    let mask = make_mask(WIDTH);
    match expression {
        Expr::Var(variable) => variables[variable].clone(),
        Expr::Const(constant) => BV::from_u64(constant.get(mask), bits),
        Expr::Not(inner) => expression_to_bv(inner, variables).bvnot(),
        Expr::Scale(coefficient, inner) => BV::from_u64(coefficient.get(mask), bits)
            .bvmul(&expression_to_bv(inner, variables)),
        Expr::And(terms) => fold_bv(
            terms,
            BV::from_u64(mask, bits),
            variables,
            |left, right| left.bvand(&right),
        ),
        Expr::Or(terms) => fold_bv(
            terms,
            BV::from_u64(0, bits),
            variables,
            |left, right| left.bvor(&right),
        ),
        Expr::Xor(terms) => fold_bv(
            terms,
            BV::from_u64(0, bits),
            variables,
            |left, right| left.bvxor(&right),
        ),
        Expr::Add(terms) => fold_bv(
            terms,
            BV::from_u64(0, bits),
            variables,
            |left, right| left.bvadd(&right),
        ),
        Expr::Mul(terms) => fold_bv(
            terms,
            BV::from_u64(1, bits),
            variables,
            |left, right| left.bvmul(&right),
        ),
    }
}

fn exact_equivalence(left: &Expr, right: &Expr) -> SatResult {
    let variable_ids = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .collect::<BTreeSet<_>>();
    let variables = variable_ids
        .iter()
        .map(|variable| {
            (
                *variable,
                BV::new_const(format!("v{}", variable.0), u32::from(WIDTH)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let solver = Solver::new();
    let mut params = Params::new();
    params.set_u32("timeout", SMT_TIMEOUT_MS);
    solver.set_params(&params);
    solver.assert(expression_to_bv(left, &variables).ne(&expression_to_bv(right, &variables)));
    solver.check()
}

fn main() {
    let mut cases = 0;
    let mut baseline_ng = 0;
    let mut experimental_ng = 0;
    let mut baseline_ng_by_dataset = BTreeMap::<&str, usize>::new();
    let mut experimental_ng_by_dataset = BTreeMap::<&str, usize>::new();
    let mut no_oracle_changed = 0;
    let mut exact_unsat = 0;
    let mut exact_sat = 0;
    let mut exact_unknown = 0;
    let mut p8c_resolved_residuals = 0;

    for (dataset, csv) in DATASETS {
        for row in csv.lines().filter(|row| !row.trim().is_empty()) {
            cases += 1;
            let (mba, ground_truth) = row.split_once(',').unwrap();
            let (Ok(mba), Ok(ground_truth)) = (
                simplify_mba(parse_expr(mba.trim()).unwrap(), WIDTH),
                simplify_mba(parse_expr(ground_truth.trim()).unwrap(), WIDTH),
            ) else {
                baseline_ng += 1;
                experimental_ng += 1;
                *baseline_ng_by_dataset.entry(dataset).or_default() += 1;
                *experimental_ng_by_dataset.entry(dataset).or_default() += 1;
                continue;
            };

            // No-oracle validation: only the simplified MBA is visible here.
            let direct = select_lowbit_above(mba.clone(), WIDTH);
            if direct.changed {
                no_oracle_changed += 1;
                match exact_equivalence(&mba, &direct.result) {
                    SatResult::Unsat => exact_unsat += 1,
                    SatResult::Sat => exact_sat += 1,
                    SatResult::Unknown => exact_unknown += 1,
                }
            }

            let residual = ground_truth - mba;
            if simplify_mba(residual.clone(), WIDTH) == Ok(Expr::zero()) {
                continue;
            }
            baseline_ng += 1;
            *baseline_ng_by_dataset.entry(dataset).or_default() += 1;
            let Ok((diagnosed, trace)) = diagnose_hidden_atoms(residual, WIDTH) else {
                experimental_ng += 1;
                *experimental_ng_by_dataset.entry(dataset).or_default() += 1;
                continue;
            };
            let Some(scope) = trace.iter().find(|scope| scope.input == diagnosed) else {
                experimental_ng += 1;
                *experimental_ng_by_dataset.entry(dataset).or_default() += 1;
                continue;
            };
            let Some(pipeline) = experiment_pipeline(scope).unwrap() else {
                experimental_ng += 1;
                *experimental_ng_by_dataset.entry(dataset).or_default() += 1;
                continue;
            };
            if !pipeline.residual_zero {
                experimental_ng += 1;
                *experimental_ng_by_dataset.entry(dataset).or_default() += 1;
            } else if pipeline.after_p8c.result == Expr::zero()
                && !pipeline.p7e.residual_zero
                && pipeline.after_p8a.result != Expr::zero()
                && pipeline.after_p8b.result != Expr::zero()
            {
                p8c_resolved_residuals += 1;
            }
        }
    }

    println!("cases={cases}");
    println!("baseline_NG={baseline_ng}");
    println!("experimental_NG={experimental_ng}");
    for (dataset, baseline) in baseline_ng_by_dataset {
        let experimental = experimental_ng_by_dataset
            .get(dataset)
            .copied()
            .unwrap_or(0);
        println!(
            "dataset={dataset} baseline_NG={baseline} experimental_NG={experimental}"
        );
    }
    println!();
    println!("no_oracle_changed={no_oracle_changed}");
    println!("exact_UNSAT={exact_unsat}");
    println!("exact_SAT={exact_sat}");
    println!("exact_UNKNOWN={exact_unknown}");
    println!("p8c_resolved_residuals={p8c_resolved_residuals}");
    println!(
        "changed_vs_resolved={no_oracle_changed}_no_oracle_changed_vs_{p8c_resolved_residuals}_residuals_resolved"
    );
}
