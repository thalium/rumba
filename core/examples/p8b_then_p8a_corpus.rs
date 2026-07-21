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
#[allow(dead_code)]
#[path = "p8b_masked_negation_micro.rs"]
mod p8b;

const BIT_COUNT: u8 = 64;
const ORDER_CONTROL_LINES: [usize; 3] = [14545, 23810, 23816];
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

#[derive(Clone, Debug)]
struct LineDetail {
    line: usize,
    nodes_initial: usize,
    nodes_after_p8b: usize,
    nodes_after_p8a: usize,
    zero: bool,
    reverse_nodes_after_p8a: usize,
    reverse_nodes_after_p8b: usize,
    reverse_semantically_equal: bool,
    reverse_zero: bool,
}

#[derive(Default)]
struct CorpusReport {
    cases: usize,
    baseline_ng: usize,
    p7e_resolved: usize,
    ng_input: usize,
    ng_resolved: usize,
    reductions_over_50_percent: usize,
    p8b_changed_residuals: usize,
    p8a_attempts_after_p8b: usize,
    p8a_proved_after_p8b: usize,
    p8a_facts_before: usize,
    p8a_facts_after_p8b: usize,
    regressions: usize,
    baseline_time: Duration,
    p8b_time: Duration,
    p8a_time_alone: Duration,
    p8a_time_after_p8b: Duration,
    resolved_lines: Vec<(&'static str, usize)>,
    strongly_reduced_lines: Vec<(&'static str, usize)>,
    details: Vec<LineDetail>,
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

fn p8a_fact_count(analysis: &p8a::P8aAnalysis) -> usize {
    analysis.metrics.submask_proved + analysis.metrics.valuation_proved
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
    let mut state = 0x3c6e_f372_fe94_f82bu64;
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
        let Ok((diagnosed_residual, trace)) = diagnose_hidden_atoms(residual, BIT_COUNT) else {
            report.ng_input += 1;
            continue;
        };
        let Some(scope) = trace.iter().find(|scope| scope.input == diagnosed_residual) else {
            report.ng_input += 1;
            continue;
        };
        let Some(p7e) = experiment_bitwise_dependency_closure(scope)
            .ok()
            .flatten()
        else {
            report.ng_input += 1;
            continue;
        };
        if p7e.residual_zero {
            report.p7e_resolved += 1;
            continue;
        }
        report.ng_input += 1;
        let initial_nodes = p7e.simplified_after_substitution.size();

        let p8a_alone_started = Instant::now();
        let p8a_alone = p8a::analyze_expression(
            p7e.restored_after_substitution.clone(),
            BIT_COUNT,
        );
        report.p8a_time_alone += p8a_alone_started.elapsed();
        report.p8a_facts_before += p8a_fact_count(&p8a_alone);

        let p8b_started = Instant::now();
        let p8b_result = p8b::analyze_scope_after_p7e(scope, &p7e);
        report.p8b_time += p8b_started.elapsed();
        if p8b_result.changed {
            report.p8b_changed_residuals += 1;
        }

        report.p8a_attempts_after_p8b += 1;
        let p8a_after_started = Instant::now();
        let p8a_after =
            p8a::analyze_expression(p8b_result.compact_residual.clone(), BIT_COUNT);
        let final_result = p8b::restore_compact_after_p8b(
            scope,
            &p7e,
            &p8b_result.dependencies,
            p8a_after.result.clone(),
        );
        report.p8a_time_after_p8b += p8a_after_started.elapsed();
        report.p8a_facts_after_p8b += p8a_fact_count(&p8a_after);
        if p8a_after.changed {
            report.p8a_proved_after_p8b += 1;
        }

        if !semantically_equal(
            &p7e.restored_after_substitution,
            &final_result,
            BIT_COUNT,
        ) {
            report.regressions += 1;
            println!(
                "regression dataset={} line={} before={} after={}",
                case.dataset, case.line, p7e.restored_after_substitution, final_result,
            );
            continue;
        }
        if final_result == Expr::zero() {
            report.ng_resolved += 1;
            report.resolved_lines.push((case.dataset, case.line));
        } else if final_result.size().saturating_mul(2) < initial_nodes {
            report.reductions_over_50_percent += 1;
            report
                .strongly_reduced_lines
                .push((case.dataset, case.line));
        }

        if case.dataset == "loki_tiny.csv" && ORDER_CONTROL_LINES.contains(&case.line) {
            let reverse = p8b::analyze_rewritten_residual(p8a_alone.result.clone(), BIT_COUNT);
            let reverse_semantically_equal = semantically_equal(
                &p7e.restored_after_substitution,
                &reverse.result,
                BIT_COUNT,
            );
            if !reverse_semantically_equal {
                report.regressions += 1;
            }
            report.details.push(LineDetail {
                line: case.line,
                nodes_initial: initial_nodes,
                nodes_after_p8b: p8b_result.result.size(),
                nodes_after_p8a: final_result.size(),
                zero: final_result == Expr::zero(),
                reverse_nodes_after_p8a: p8a_alone.result.size(),
                reverse_nodes_after_p8b: reverse.result.size(),
                reverse_semantically_equal,
                reverse_zero: reverse.result == Expr::zero() && reverse_semantically_equal,
            });
        }
    }
    report
}

fn print_lines(label: &str, lines: &[(&str, usize)]) {
    let lines = lines
        .iter()
        .map(|(dataset, line)| format!("{dataset}:{line}"))
        .collect::<Vec<_>>()
        .join(",");
    println!("{label}={lines}");
}

fn main() {
    let cases = load_cases();
    let report = run_corpus(&cases);
    let combined = report.p8b_time + report.p8a_time_after_p8b;
    let overhead = if report.baseline_time.is_zero() {
        0.0
    } else {
        100.0 * combined.as_secs_f64() / report.baseline_time.as_secs_f64()
    };

    println!("cases={}", report.cases);
    println!("baseline_NG={}", report.baseline_ng);
    println!("p7e_resolved={}", report.p7e_resolved);
    println!("NG_input={}", report.ng_input);
    println!("NG_resolved={}", report.ng_resolved);
    println!("NG_remaining={}", report.ng_input - report.ng_resolved);
    println!(
        "reductions_over_50_percent={}",
        report.reductions_over_50_percent
    );
    println!();
    println!("p8b_changed_residuals={}", report.p8b_changed_residuals);
    println!(
        "p8a_attempts_after_p8b={}",
        report.p8a_attempts_after_p8b
    );
    println!("p8a_proved_after_p8b={}", report.p8a_proved_after_p8b);
    println!("p8a_facts_before={}", report.p8a_facts_before);
    println!("p8a_facts_after_p8b={}", report.p8a_facts_after_p8b);
    println!("regressions={}", report.regressions);
    println!();
    println!(
        "p8a_time_alone_ms={:.3}",
        report.p8a_time_alone.as_secs_f64() * 1_000.0
    );
    println!("p8b_ms={:.3}", report.p8b_time.as_secs_f64() * 1_000.0);
    println!(
        "p8a_time_after_p8b_ms={:.3}",
        report.p8a_time_after_p8b.as_secs_f64() * 1_000.0
    );
    println!("combined_ms={:.3}", combined.as_secs_f64() * 1_000.0);
    println!("combined_overhead_percent={overhead:.3}");
    println!();
    print_lines("resolved_lines", &report.resolved_lines);
    print_lines(
        "strongly_reduced_lines",
        &report.strongly_reduced_lines,
    );
    println!();
    report.details.iter().for_each(|detail| {
        println!(
            "line={} nodes_initial={} nodes_after_p8b={} nodes_after_p8a={} zero={} reverse_nodes_after_p8a={} reverse_nodes_after_p8b={} reverse_semantically_equal={} reverse_zero={}",
            detail.line,
            detail.nodes_initial,
            detail.nodes_after_p8b,
            detail.nodes_after_p8a,
            detail.zero,
            detail.reverse_nodes_after_p8a,
            detail.reverse_nodes_after_p8b,
            detail.reverse_semantically_equal,
            detail.reverse_zero,
        );
    });
    println!();
    println!("smt_called=false");
    println!("pct_fallback_called=false");
}
