#![cfg(feature = "parse")]

use rumba_core::{
    expr::Expr,
    p8::{P8PipelineExperiment, experiment_pipeline},
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, simplify_mba},
};

const WIDTH: u8 = 64;
const LOKI: &str = include_str!("../../third_party/dataset/loki_tiny.csv");
const QSYNTH: &str = include_str!("../../third_party/dataset/qsynth_ea.csv");

fn pipeline_for_row(row: &str) -> Option<P8PipelineExperiment> {
    let (mba, ground_truth) = row.split_once(',').unwrap();
    let mba = simplify_mba(parse_expr(mba.trim()).unwrap(), WIDTH).unwrap();
    let ground_truth =
        simplify_mba(parse_expr(ground_truth.trim()).unwrap(), WIDTH).unwrap();
    let residual = ground_truth - mba;
    if simplify_mba(residual.clone(), WIDTH) == Ok(Expr::zero()) {
        return None;
    }
    let (diagnosed, trace) = diagnose_hidden_atoms(residual, WIDTH).unwrap();
    let scope = trace.iter().find(|scope| scope.input == diagnosed).unwrap();
    experiment_pipeline(scope).unwrap()
}

fn loki_line(line: usize) -> P8PipelineExperiment {
    pipeline_for_row(LOKI.lines().nth(line - 1).unwrap()).unwrap()
}

#[test]
fn integrated_order_reaches_the_expected_stage_specific_cases() {
    let p8a = loki_line(14545);
    assert!(!p8a.p7e.residual_zero);
    assert_eq!(p8a.after_p8a.result, Expr::zero());

    let p8b = loki_line(23810);
    assert_ne!(p8b.after_p8a.result, Expr::zero());
    assert_eq!(p8b.after_p8b.result, Expr::zero());

    let p8c = loki_line(19728);
    assert_ne!(p8c.after_p8b.result, Expr::zero());
    assert_eq!(p8c.after_p8c.result, Expr::zero());
    assert!(p8c.residual_zero);
}

#[test]
fn p7e_zero_is_the_pipeline_result_and_all_qsynth_ng_are_resolved() {
    let mut baseline_ng = 0;
    let mut resolved = 0;
    for row in QSYNTH.lines().filter(|row| !row.trim().is_empty()) {
        let Some(pipeline) = pipeline_for_row(row) else {
            continue;
        };
        baseline_ng += 1;
        assert_eq!(pipeline.result, Expr::zero());
        assert!(pipeline.residual_zero);
        resolved += 1;
    }
    assert_eq!(baseline_ng, 19);
    assert_eq!(resolved, 19);
}
