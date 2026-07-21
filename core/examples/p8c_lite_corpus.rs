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
#[allow(dead_code)]
#[path = "p8c_relative_lead_micro.rs"]
mod p8c;

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
    current_pipeline_resolved: usize,
    ng_input: usize,
    ng_resolved: usize,
    reductions_over_50_percent: usize,
    p8c_scopes_triggered: usize,
    p8c_changed_residuals: usize,
    p8b_changed_after_p8c: usize,
    regressions: usize,
    baseline_time: Duration,
    current_p8b_time: Duration,
    p8c_time: Duration,
    p8b_after_p8c_time: Duration,
    metrics: p8c::P8cMetrics,
    resolved_lines: Vec<(&'static str, usize)>,
    strongly_reduced_lines: Vec<(&'static str, usize)>,
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
    let mut state = 0xc6bc_2796_92b5_cc83u64;
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

fn accumulate_metrics(total: &mut p8c::P8cMetrics, current: p8c::P8cMetrics) {
    total.anchor_candidates += current.anchor_candidates;
    total.anchors_recognized += current.anchors_recognized;
    total.relative_lead_queries += current.relative_lead_queries;
    total.known_zero += current.known_zero;
    total.known_one += current.known_one;
    total.selections_zero += current.selections_zero;
    total.selections_one += current.selections_one;
    total.lowbit_selections += current.lowbit_selections;
    total.unknown_results += current.unknown_results;
    total.unknown_due_to_product += current.unknown_due_to_product;
    total.unknown_due_to_not += current.unknown_due_to_not;
    total.unknown_due_to_variable += current.unknown_due_to_variable;
    total.unknown_due_to_constant += current.unknown_due_to_constant;
}

fn accept_p8b(input: Expr, width: u8) -> (Expr, bool) {
    let analysis = p8b::analyze_rewritten_residual(input.clone(), width);
    if analysis.changed
        && (analysis.result == Expr::zero() || analysis.result.size() < input.size())
    {
        (analysis.result, true)
    } else {
        (input, false)
    }
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
            continue;
        };
        let Some(scope) = trace.iter().find(|scope| scope.input == diagnosed_residual) else {
            continue;
        };
        let Some(p7e) = experiment_bitwise_dependency_closure(scope)
            .ok()
            .flatten()
        else {
            continue;
        };
        if p7e.residual_zero {
            report.p7e_resolved += 1;
            continue;
        }

        let after_p8a = p8a::analyze_expression(
            p7e.restored_after_substitution.clone(),
            BIT_COUNT,
        );

        // Establish the current P7e -> P8a -> P8b result first. P8c coverage
        // is measured only on the cases that remain NG after this pipeline.
        let current_p8b_started = Instant::now();
        let (current_result, _) = accept_p8b(after_p8a.result.clone(), BIT_COUNT);
        report.current_p8b_time += current_p8b_started.elapsed();
        if current_result == Expr::zero() {
            report.current_pipeline_resolved += 1;
            continue;
        }
        report.ng_input += 1;

        let p8c_started = Instant::now();
        let after_p8c = p8c::analyze_expression(after_p8a.result.clone(), BIT_COUNT);
        report.p8c_time += p8c_started.elapsed();
        if after_p8c.metrics.anchors_recognized != 0 {
            report.p8c_scopes_triggered += 1;
        }
        accumulate_metrics(&mut report.metrics, after_p8c.metrics);
        if after_p8c.changed {
            report.p8c_changed_residuals += 1;
        }

        let p8b_started = Instant::now();
        let (final_result, p8b_changed) = accept_p8b(after_p8c.result, BIT_COUNT);
        report.p8b_after_p8c_time += p8b_started.elapsed();
        if p8b_changed {
            report.p8b_changed_after_p8c += 1;
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
            println!(
                "resolved dataset={} line={} nodes_before={} nodes_after_p8c={} before={}",
                case.dataset,
                case.line,
                current_result.size(),
                final_result.size(),
                current_result,
            );
        } else if final_result.size().saturating_mul(2) < current_result.size() {
            report.reductions_over_50_percent += 1;
            report
                .strongly_reduced_lines
                .push((case.dataset, case.line));
            println!(
                "reduced dataset={} line={} nodes_before={} nodes_after={} before={} after={}",
                case.dataset,
                case.line,
                current_result.size(),
                final_result.size(),
                current_result,
                final_result,
            );
        }
    }
    report
}

fn print_lines(label: &str, lines: &[(&str, usize)]) {
    let value = lines
        .iter()
        .map(|(dataset, line)| format!("{dataset}:{line}"))
        .collect::<Vec<_>>()
        .join(",");
    println!("{label}=[{value}]");
}

fn main() {
    let cases = load_cases();
    let report = run_corpus(&cases);
    let p8c_ms = report.p8c_time.as_secs_f64() * 1_000.0;
    let current_p8b_ms = report.current_p8b_time.as_secs_f64() * 1_000.0;
    let p8b_after_p8c_ms = report.p8b_after_p8c_time.as_secs_f64() * 1_000.0;
    let incremental_ms = p8c_ms + p8b_after_p8c_ms - current_p8b_ms;
    let overhead = if report.baseline_time.is_zero() {
        0.0
    } else {
        100.0 * (incremental_ms / 1_000.0) / report.baseline_time.as_secs_f64()
    };

    println!();
    println!("cases={}", report.cases);
    println!("baseline_NG={}", report.baseline_ng);
    println!("p7e_resolved={}", report.p7e_resolved);
    println!(
        "current_p8a_p8b_resolved={}",
        report.current_pipeline_resolved
    );
    println!("NG_input={}", report.ng_input);
    println!("NG_resolved={}", report.ng_resolved);
    println!("NG_remaining={}", report.ng_input - report.ng_resolved);
    println!(
        "reductions_over_50_percent={}",
        report.reductions_over_50_percent
    );
    println!();
    println!("scopes_attempted={}", report.ng_input);
    println!("scopes_with_anchors={}", report.p8c_scopes_triggered);
    println!("p8c_changed_residuals={}", report.p8c_changed_residuals);
    println!("p8b_changed_after_p8c={}", report.p8b_changed_after_p8c);
    println!("anchor_candidates={}", report.metrics.anchor_candidates);
    println!("anchors_recognized={}", report.metrics.anchors_recognized);
    println!(
        "relative_lead_queries={}",
        report.metrics.relative_lead_queries
    );
    println!("known_zero={}", report.metrics.known_zero);
    println!("known_one={}", report.metrics.known_one);
    println!("selections_zero={}", report.metrics.selections_zero);
    println!("selections_one={}", report.metrics.selections_one);
    println!("lowbit_selections={}", report.metrics.lowbit_selections);
    println!("unknown_results={}", report.metrics.unknown_results);
    println!(
        "unknown_due_to_product={}",
        report.metrics.unknown_due_to_product
    );
    println!("unknown_due_to_not={}", report.metrics.unknown_due_to_not);
    println!(
        "unknown_due_to_variable={}",
        report.metrics.unknown_due_to_variable
    );
    println!(
        "unknown_due_to_constant={}",
        report.metrics.unknown_due_to_constant
    );
    println!();
    println!("current_p8b_ms={current_p8b_ms:.3}");
    println!("p8c_ms={p8c_ms:.3}");
    println!("p8b_after_p8c_ms={p8b_after_p8c_ms:.3}");
    println!("incremental_ms={incremental_ms:.3}");
    println!("incremental_overhead_percent={overhead:.3}");
    println!();
    print_lines("resolved_lines", &report.resolved_lines);
    print_lines(
        "strongly_reduced_lines",
        &report.strongly_reduced_lines,
    );
    println!("regressions={}", report.regressions);
    println!("smt_called=false");
    println!("pct_fallback_called=false");
    println!("normal_path_modified=false");
}
