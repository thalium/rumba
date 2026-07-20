use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use rumba_core::{
    expr::Expr,
    parser::parse_expr,
    simplify::{
        diagnose_hidden_atoms, experiment_bitwise_dependency_closure, simplify_mba,
    },
};

const BIT_COUNT: u8 = 64;
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

struct Case {
    dataset: &'static str,
    line: usize,
    mba: Expr,
    ground_truth: Expr,
}

#[derive(Default)]
struct Trial {
    baseline: Duration,
    trace: Duration,
    closure: Duration,
    ng: usize,
    resolved: usize,
    loki_resolved: usize,
    qsynth_resolved: usize,
    datasets: BTreeMap<&'static str, (usize, usize)>,
}

fn load_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (dataset, csv) in DATASETS {
        for (index, row) in csv.lines().enumerate() {
            if row.trim().is_empty() {
                continue;
            }
            let (mba, ground_truth) = row
                .split_once(',')
                .unwrap_or_else(|| panic!("{dataset}:{}: expected two columns", index + 1));
            cases.push(Case {
                dataset,
                line: index + 1,
                mba: parse_expr(mba.trim()).unwrap(),
                ground_truth: parse_expr(ground_truth.trim()).unwrap(),
            });
        }
    }
    cases
}

fn run_trial(cases: &[Case]) -> Trial {
    let mut trial = Trial::default();
    let mut misses = Vec::new();

    for case in cases {
        let started = Instant::now();
        let simplified_mba = simplify_mba(case.mba.clone(), BIT_COUNT);
        let simplified_gt = simplify_mba(case.ground_truth.clone(), BIT_COUNT);
        let raw_residual = match (simplified_mba, simplified_gt) {
            (Ok(mba), Ok(gt)) => Some(gt - mba),
            _ => None,
        };
        let is_ng = raw_residual
            .as_ref()
            .is_none_or(|residual| simplify_mba(residual.clone(), BIT_COUNT) != Ok(Expr::zero()));
        trial.baseline += started.elapsed();

        if is_ng {
            trial.ng += 1;
            trial.datasets.entry(case.dataset).or_default().0 += 1;
            if let Some(residual) = raw_residual {
                misses.push((case, residual));
            }
        }
    }

    for (case, residual) in misses {
        let started = Instant::now();
        let Ok((diagnosed_residual, trace)) = diagnose_hidden_atoms(residual, BIT_COUNT) else {
            trial.trace += started.elapsed();
            continue;
        };
        trial.trace += started.elapsed();
        let Some(scope) = trace.iter().find(|scope| scope.input == diagnosed_residual) else {
            continue;
        };

        let started = Instant::now();
        let resolved = experiment_bitwise_dependency_closure(scope)
            .ok()
            .flatten()
            .is_some_and(|experiment| experiment.residual_zero);
        trial.closure += started.elapsed();
        if resolved {
            trial.resolved += 1;
            trial.datasets.entry(case.dataset).or_default().1 += 1;
            if case.dataset == "loki_tiny.csv" {
                trial.loki_resolved += 1;
            }
            if case.dataset == "qsynth_ea.csv" {
                trial.qsynth_resolved += 1;
            }
        }

        if case.dataset == "qsynth_ea.csv"
            && [53, 249, 260, 369, 481].contains(&case.line)
            && !resolved
        {
            panic!("target QSynth line {} was not resolved", case.line);
        }
    }

    trial
}

fn main() {
    let cases = load_cases();
    let trials = std::env::args()
        .nth(1)
        .map(|value| value.parse::<usize>().expect("trial count must be an integer"))
        .unwrap_or(3);
    println!("cases={} trials={}", cases.len(), trials);

    for index in 0..trials {
        let trial = run_trial(&cases);
        let closure_overhead = 100.0 * trial.closure.as_secs_f64() / trial.baseline.as_secs_f64();
        let diagnostic_overhead = 100.0
            * (trial.trace + trial.closure).as_secs_f64()
            / trial.baseline.as_secs_f64();
        println!(
            "trial={} baseline_ms={:.3} trace_ms={:.3} closure_ms={:.3} closure_overhead_percent={:.3} diagnostic_overhead_percent={:.3} ng_before={} resolved={} ng_after={} loki_resolved={} qsynth_resolved={}",
            index + 1,
            trial.baseline.as_secs_f64() * 1_000.0,
            trial.trace.as_secs_f64() * 1_000.0,
            trial.closure.as_secs_f64() * 1_000.0,
            closure_overhead,
            diagnostic_overhead,
            trial.ng,
            trial.resolved,
            trial.ng - trial.resolved,
            trial.loki_resolved,
            trial.qsynth_resolved,
        );
        for (dataset, (ng_before, resolved)) in &trial.datasets {
            println!(
                "  dataset={} ng_before={} resolved={} ng_after={}",
                dataset,
                ng_before,
                resolved,
                ng_before - resolved,
            );
        }
    }
}
