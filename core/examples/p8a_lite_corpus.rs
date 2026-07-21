use std::time::{Duration, Instant};

use rumba_core::{
    expr::Expr,
    parser::parse_expr,
    simplify::{
        diagnose_hidden_atoms, experiment_bitwise_dependency_closure, simplify_mba,
    },
    varint::make_mask,
};

#[allow(dead_code)]
#[path = "p8a_submask_micro.rs"]
mod p8a;

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
struct CorpusReport {
    cases: usize,
    baseline_ng: usize,
    p7e_resolved: usize,
    ng_before: usize,
    ng_resolved: usize,
    ng_reduced_over_50_percent: usize,
    semantic_regressions: usize,
    scopes_attempted: usize,
    submask_queries: usize,
    submask_proved: usize,
    valuation_queries: usize,
    valuation_proved: usize,
    unknown_results: usize,
    baseline_time: Duration,
    p7e_time: Duration,
    analysis_time: Duration,
    scope_times: Vec<Duration>,
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

fn semantically_equal(left: &Expr, right: &Expr, width: u8) -> bool {
    let mask = make_mask(width);
    let max_var = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .map(|variable| variable.0)
        .max()
        .unwrap_or(0);
    let mut state = 0x6a09_e667_f3bc_c909u64;
    for sample in 0..200 {
        let mut values = Vec::with_capacity(max_var + 1);
        for variable in 0..=max_var {
            state ^= state << 7;
            state ^= state >> 9;
            state ^= state << 8;
            values.push(
                state
                    .wrapping_add((sample as u64) << 32)
                    .wrapping_add(variable as u64)
                    & mask,
            );
        }
        if left.eval(&values).get(mask) != right.eval(&values).get(mask) {
            return false;
        }
    }
    true
}

fn percentile(values: &[Duration], percentile: f64) -> Duration {
    if values.is_empty() {
        return Duration::ZERO;
    }
    let mut values = values.to_vec();
    values.sort();
    let index = ((values.len() - 1) as f64 * percentile).round() as usize;
    values[index]
}

fn run_corpus(cases: &[Case]) -> CorpusReport {
    let mut report = CorpusReport {
        cases: cases.len(),
        ..CorpusReport::default()
    };
    let mut misses = Vec::new();

    for case in cases {
        let started = Instant::now();
        let simplified_mba = simplify_mba(case.mba.clone(), BIT_COUNT);
        let simplified_gt = simplify_mba(case.ground_truth.clone(), BIT_COUNT);
        let residual = match (simplified_mba, simplified_gt) {
            (Ok(mba), Ok(gt)) => Some(gt - mba),
            _ => None,
        };
        let is_ng = residual
            .as_ref()
            .is_none_or(|residual| simplify_mba(residual.clone(), BIT_COUNT) != Ok(Expr::zero()));
        report.baseline_time += started.elapsed();
        if is_ng {
            report.baseline_ng += 1;
            if let Some(residual) = residual {
                misses.push((case, residual));
            }
        }
    }

    for (case, residual) in misses {
        let p7e_started = Instant::now();
        let diagnosed = diagnose_hidden_atoms(residual.clone(), BIT_COUNT);
        let (source, before_p8a, p7e_zero) = match diagnosed {
            Ok((diagnosed_residual, trace)) => {
                let experiment = trace
                    .iter()
                    .find(|scope| scope.input == diagnosed_residual)
                    .and_then(|scope| experiment_bitwise_dependency_closure(scope).ok().flatten());
                if let Some(experiment) = experiment {
                    (
                        experiment.restored_after_substitution,
                        experiment.simplified_after_substitution,
                        experiment.residual_zero,
                    )
                } else {
                    let simplified = simplify_mba(residual.clone(), BIT_COUNT)
                        .unwrap_or_else(|_| residual.clone());
                    (residual, simplified, false)
                }
            }
            Err(_) => {
                let simplified =
                    simplify_mba(residual.clone(), BIT_COUNT).unwrap_or_else(|_| residual.clone());
                (residual, simplified, false)
            }
        };
        report.p7e_time += p7e_started.elapsed();
        if p7e_zero {
            report.p7e_resolved += 1;
            continue;
        }

        report.ng_before += 1;
        report.scopes_attempted += 1;
        let started = Instant::now();
        let analysis = p8a::analyze_expression(source.clone(), BIT_COUNT);
        let elapsed = started.elapsed();
        report.analysis_time += elapsed;
        report.scope_times.push(elapsed);
        report.submask_queries += analysis.metrics.submask_queries;
        report.submask_proved += analysis.metrics.submask_proved;
        report.valuation_queries += analysis.metrics.valuation_queries;
        report.valuation_proved += analysis.metrics.valuation_proved;
        if analysis.unknown {
            report.unknown_results += 1;
        }
        if analysis.changed && !semantically_equal(&source, &analysis.result, BIT_COUNT) {
            report.semantic_regressions += 1;
            println!(
                "semantic_regression dataset={} line={} before={} after={}",
                case.dataset, case.line, source, analysis.result,
            );
            continue;
        }
        if analysis.result == Expr::zero() {
            report.ng_resolved += 1;
            println!(
                "resolved dataset={} line={} before_nodes={} source_nodes={}",
                case.dataset,
                case.line,
                before_p8a.size(),
                source.size(),
            );
        } else if analysis.result.size().saturating_mul(2) < before_p8a.size() {
            report.ng_reduced_over_50_percent += 1;
            println!(
                "reduced dataset={} line={} before_nodes={} after_nodes={}",
                case.dataset,
                case.line,
                before_p8a.size(),
                analysis.result.size(),
            );
        }
    }
    report
}

fn main() {
    let cases = load_cases();
    let report = run_corpus(&cases);
    let median = percentile(&report.scope_times, 0.50);
    let p95 = percentile(&report.scope_times, 0.95);
    let total_overhead = if report.baseline_time.is_zero() {
        0.0
    } else {
        100.0 * report.analysis_time.as_secs_f64() / report.baseline_time.as_secs_f64()
    };

    println!();
    println!("cases={}", report.cases);
    println!("baseline_ng={}", report.baseline_ng);
    println!("p7e_resolved={}", report.p7e_resolved);
    println!("NG_before={}", report.ng_before);
    println!("NG_resolved={}", report.ng_resolved);
    println!(
        "NG_reduced_over_50_percent={}",
        report.ng_reduced_over_50_percent
    );
    println!("semantic_regressions={}", report.semantic_regressions);
    println!();
    println!("scopes_attempted={}", report.scopes_attempted);
    println!("submask_queries={}", report.submask_queries);
    println!("submask_proved={}", report.submask_proved);
    println!("valuation_queries={}", report.valuation_queries);
    println!("valuation_proved={}", report.valuation_proved);
    println!("unknown_results={}", report.unknown_results);
    println!();
    println!("baseline_time_ms={:.3}", report.baseline_time.as_secs_f64() * 1_000.0);
    println!("p7e_time_ms={:.3}", report.p7e_time.as_secs_f64() * 1_000.0);
    println!("analysis_time_ms={:.3}", report.analysis_time.as_secs_f64() * 1_000.0);
    println!("total_overhead_percent={total_overhead:.3}");
    println!("median_scope_time_us={:.3}", median.as_secs_f64() * 1_000_000.0);
    println!("p95_scope_time_us={:.3}", p95.as_secs_f64() * 1_000_000.0);
    println!("smt_called=false");
}
