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
#[path = "p8b_masked_negation_micro.rs"]
mod p8b;

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
    ng_input: usize,
    ng_resolved: usize,
    residuals_reduced_over_50_percent: usize,
    p7e_candidates_observed: usize,
    p8b_attempts: usize,
    p8b_proved: usize,
    p8b_unknown: usize,
    p8b_budget_exceeded: usize,
    relative_tz_queries: usize,
    above_lowbit_proved: usize,
    masked_negations_normalized: usize,
    regressions: usize,
    all_certified_dependencies_exact: bool,
    unknown_causes_no_rewrite: bool,
    loki_14545_resolved: bool,
    loki_23816_resolved: bool,
    baseline_time: Duration,
    trace_time: Duration,
    p7e_time: Duration,
    candidate_generation: Duration,
    relative_tz: Duration,
    normalization: Duration,
    final_simplification: Duration,
    p8b_total: Duration,
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
    let mut state = 0xbb67_ae85_84ca_a73bu64;
    for sample in 0..200 {
        let values = (0..=max_var)
            .map(|variable| {
                state ^= state << 7;
                state ^= state >> 9;
                state ^= state << 8;
                state
                    .wrapping_add((sample as u64) << 32)
                    .wrapping_add(variable as u64)
                    & mask
            })
            .collect::<Vec<_>>();
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
        all_certified_dependencies_exact: true,
        unknown_causes_no_rewrite: true,
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
        let trace_started = Instant::now();
        let Ok((diagnosed_residual, trace)) = diagnose_hidden_atoms(residual, BIT_COUNT) else {
            report.trace_time += trace_started.elapsed();
            report.ng_input += 1;
            continue;
        };
        report.trace_time += trace_started.elapsed();
        let Some(scope) = trace.iter().find(|scope| scope.input == diagnosed_residual) else {
            report.ng_input += 1;
            continue;
        };

        let p7e_started = Instant::now();
        let p7e = experiment_bitwise_dependency_closure(scope)
            .ok()
            .flatten();
        report.p7e_time += p7e_started.elapsed();
        let Some(p7e) = p7e else {
            report.ng_input += 1;
            continue;
        };
        if p7e.residual_zero {
            report.p7e_resolved += 1;
            continue;
        }

        report.ng_input += 1;
        let scope_started = Instant::now();
        let analysis = p8b::analyze_scope_after_p7e(scope, &p7e);
        let scope_elapsed = scope_started.elapsed();
        report.p8b_total += scope_elapsed;
        report.scope_times.push(scope_elapsed);
        let metrics = analysis.metrics;
        report.p7e_candidates_observed += metrics.p7e_candidates_observed;
        report.p8b_attempts += metrics.p8b_attempts;
        report.p8b_proved += metrics.p8b_proved;
        report.p8b_unknown += metrics.p8b_unknown;
        report.p8b_budget_exceeded += metrics.p8b_budget_exceeded;
        report.relative_tz_queries += metrics.relative_tz_queries;
        report.above_lowbit_proved += metrics.above_lowbit_proved;
        report.masked_negations_normalized += metrics.masked_negations_normalized;
        report.candidate_generation += metrics.candidate_generation;
        report.relative_tz += metrics.relative_tz;
        report.normalization += metrics.normalization;
        report.final_simplification += metrics.final_simplification;
        report.all_certified_dependencies_exact &= analysis.all_certified_dependencies_exact;
        report.unknown_causes_no_rewrite &= analysis.unknown_causes_no_rewrite;
        if !analysis.changed && analysis.result != p7e.simplified_after_substitution {
            report.unknown_causes_no_rewrite = false;
        }

        if analysis.changed
            && !semantically_equal(&p7e.restored_after_substitution, &analysis.result, BIT_COUNT)
        {
            report.regressions += 1;
            println!(
                "regression dataset={} line={} before={} after={}",
                case.dataset, case.line, p7e.restored_after_substitution, analysis.result,
            );
            continue;
        }
        if analysis.result == Expr::zero() {
            report.ng_resolved += 1;
            if case.dataset == "loki_tiny.csv" && case.line == 14545 {
                report.loki_14545_resolved = true;
            }
            if case.dataset == "loki_tiny.csv" && case.line == 23816 {
                report.loki_23816_resolved = true;
            }
            println!(
                "resolved dataset={} line={} dependencies={} before_nodes={}",
                case.dataset,
                case.line,
                analysis.dependencies_proved,
                p7e.simplified_after_substitution.size(),
            );
        } else if analysis.result.size().saturating_mul(2)
            < p7e.simplified_after_substitution.size()
        {
            report.residuals_reduced_over_50_percent += 1;
            println!(
                "reduced dataset={} line={} dependencies={} before_nodes={} after_nodes={}",
                case.dataset,
                case.line,
                analysis.dependencies_proved,
                p7e.simplified_after_substitution.size(),
                analysis.result.size(),
            );
        } else if analysis.changed {
            println!(
                "proved dataset={} line={} dependencies={} before_nodes={} after_nodes={}",
                case.dataset,
                case.line,
                analysis.dependencies_proved,
                p7e.simplified_after_substitution.size(),
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
    let maximum = report.scope_times.iter().copied().max().unwrap_or_default();
    let overhead = if report.baseline_time.is_zero() {
        0.0
    } else {
        100.0 * report.p8b_total.as_secs_f64() / report.baseline_time.as_secs_f64()
    };

    println!();
    println!("cases={}", report.cases);
    println!("baseline_NG={}", report.baseline_ng);
    println!("p7e_resolved={}", report.p7e_resolved);
    println!("NG_input={}", report.ng_input);
    println!("NG_resolved={}", report.ng_resolved);
    println!("NG_remaining={}", report.ng_input - report.ng_resolved);
    println!(
        "residuals_reduced_over_50_percent={}",
        report.residuals_reduced_over_50_percent
    );
    println!();
    println!(
        "p7e_candidates_observed={}",
        report.p7e_candidates_observed
    );
    println!("p8b_attempts={}", report.p8b_attempts);
    println!("p8b_proved={}", report.p8b_proved);
    println!("p8b_unknown={}", report.p8b_unknown);
    println!("p8b_budget_exceeded={}", report.p8b_budget_exceeded);
    println!();
    println!("relative_tz_queries={}", report.relative_tz_queries);
    println!("above_lowbit_proved={}", report.above_lowbit_proved);
    println!(
        "masked_negations_normalized={}",
        report.masked_negations_normalized
    );
    println!();
    println!("regressions={}", report.regressions);
    println!(
        "all_certified_dependencies_exact={}",
        report.all_certified_dependencies_exact
    );
    println!(
        "unknown_causes_no_rewrite={}",
        report.unknown_causes_no_rewrite
    );
    println!("loki_14545_resolved={}", report.loki_14545_resolved);
    println!("loki_23816_resolved={}", report.loki_23816_resolved);
    println!();
    println!("trace_ms={:.3}", report.trace_time.as_secs_f64() * 1_000.0);
    println!("p7e_ms={:.3}", report.p7e_time.as_secs_f64() * 1_000.0);
    println!(
        "candidate_generation_ms={:.3}",
        report.candidate_generation.as_secs_f64() * 1_000.0
    );
    println!("relative_tz_ms={:.3}", report.relative_tz.as_secs_f64() * 1_000.0);
    println!("normalization_ms={:.3}", report.normalization.as_secs_f64() * 1_000.0);
    println!(
        "final_simplification_ms={:.3}",
        report.final_simplification.as_secs_f64() * 1_000.0
    );
    println!("p8b_total_ms={:.3}", report.p8b_total.as_secs_f64() * 1_000.0);
    println!("incremental_overhead_percent={overhead:.3}");
    println!("median_scope_us={:.3}", median.as_secs_f64() * 1_000_000.0);
    println!("p95_scope_us={:.3}", p95.as_secs_f64() * 1_000_000.0);
    println!("max_scope_us={:.3}", maximum.as_secs_f64() * 1_000_000.0);
    println!("smt_called=false");
    println!("p8a_called=false");
    println!("pct_fallback_called=false");
}
