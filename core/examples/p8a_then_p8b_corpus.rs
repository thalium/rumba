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
    reductions_over_50_percent: usize,
    resolved_by_p8a: usize,
    resolved_only_after_p8b: usize,
    p8a_changed_residuals: usize,
    p8b_changed_after_p8a: usize,
    regressions: usize,
    baseline_time: Duration,
    p8a_time: Duration,
    p8b_after_p8a_time: Duration,
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
    let mut state = 0xa54f_f53a_5f1d_36f1u64;
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
        let initial = p7e.simplified_after_substitution.clone();
        let initial_nodes = initial.size();

        let p8a_started = Instant::now();
        let after_p8a = p8a::analyze_expression(
            p7e.restored_after_substitution.clone(),
            BIT_COUNT,
        );
        report.p8a_time += p8a_started.elapsed();
        if after_p8a.changed {
            report.p8a_changed_residuals += 1;
        }

        let final_result = if after_p8a.result == Expr::zero() {
            report.resolved_by_p8a += 1;
            after_p8a.result
        } else {
            let p8b_started = Instant::now();
            let after_p8b =
                p8b::analyze_rewritten_residual(after_p8a.result.clone(), BIT_COUNT);
            report.p8b_after_p8a_time += p8b_started.elapsed();
            if after_p8b.changed
                && (after_p8b.result == Expr::zero()
                    || after_p8b.result.size() < after_p8a.result.size())
            {
                report.p8b_changed_after_p8a += 1;
                if after_p8b.result == Expr::zero() {
                    report.resolved_only_after_p8b += 1;
                }
                after_p8b.result
            } else {
                after_p8a.result
            }
        };

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
                "resolved dataset={} line={} initial_nodes={}",
                case.dataset, case.line, initial_nodes,
            );
        } else if final_result.size().saturating_mul(2) < initial_nodes {
            report.reductions_over_50_percent += 1;
            report
                .strongly_reduced_lines
                .push((case.dataset, case.line));
            println!(
                "reduced dataset={} line={} initial_nodes={} final_nodes={}",
                case.dataset,
                case.line,
                initial_nodes,
                final_result.size(),
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
    let combined = report.p8a_time + report.p8b_after_p8a_time;
    let overhead = if report.baseline_time.is_zero() {
        0.0
    } else {
        100.0 * combined.as_secs_f64() / report.baseline_time.as_secs_f64()
    };

    println!();
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
    println!("resolved_by_p8a={}", report.resolved_by_p8a);
    println!(
        "resolved_only_after_p8b={}",
        report.resolved_only_after_p8b
    );
    println!("p8a_changed_residuals={}", report.p8a_changed_residuals);
    println!("p8b_changed_after_p8a={}", report.p8b_changed_after_p8a);
    println!();
    println!("p8a_ms={:.3}", report.p8a_time.as_secs_f64() * 1_000.0);
    println!(
        "p8b_after_p8a_ms={:.3}",
        report.p8b_after_p8a_time.as_secs_f64() * 1_000.0
    );
    println!("combined_ms={:.3}", combined.as_secs_f64() * 1_000.0);
    println!("combined_overhead_percent={overhead:.3}");
    println!();
    print_lines("resolved_lines", &report.resolved_lines);
    print_lines(
        "strongly_reduced_lines",
        &report.strongly_reduced_lines,
    );
    println!("regressions={}", report.regressions);
    println!("smt_called=false");
    println!("pct_fallback_called=false");
}
