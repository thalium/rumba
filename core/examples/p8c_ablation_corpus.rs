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

use p8c::{P8cMetrics, P8cVariant};

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
struct VariantReport {
    resolved: usize,
    changed: usize,
    regressions: usize,
    analysis_time: Duration,
    metrics: P8cMetrics,
    resolved_lines: Vec<(&'static str, usize)>,
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
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
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

fn accept_p8b(input: Expr) -> Expr {
    let analysis = p8b::analyze_rewritten_residual(input.clone(), BIT_COUNT);
    if analysis.changed
        && (analysis.result == Expr::zero() || analysis.result.size() < input.size())
    {
        analysis.result
    } else {
        input
    }
}

fn accumulate(total: &mut P8cMetrics, current: P8cMetrics) {
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
    total.derived_zero_from_one += current.derived_zero_from_one;
    total.zero_by_even_scale += current.zero_by_even_scale;
    total.zero_by_existing_above += current.zero_by_existing_above;
    total.zero_by_one_xor_one += current.zero_by_one_xor_one;
    total.zero_by_one_plus_one += current.zero_by_one_plus_one;
    total.zero_by_one_minus_one += current.zero_by_one_minus_one;
    total.zero_by_and += current.zero_by_and;
    total.zero_by_other += current.zero_by_other;
}

fn run_variant(
    case: &Case,
    rich_form: Expr,
    reference: &Expr,
    variant: P8cVariant,
    report: &mut VariantReport,
) {
    let started = Instant::now();
    let analysis = p8c::analyze_expression_with_variant(rich_form, BIT_COUNT, variant);
    report.analysis_time += started.elapsed();
    accumulate(&mut report.metrics, analysis.metrics);
    if analysis.changed {
        report.changed += 1;
    }
    let result = accept_p8b(analysis.result);
    if !semantically_equal(reference, &result, BIT_COUNT) {
        report.regressions += 1;
    } else if result == Expr::zero() {
        report.resolved += 1;
        report.resolved_lines.push((case.dataset, case.line));
    }
}

fn print_lines(lines: &[(&str, usize)]) -> String {
    lines
        .iter()
        .map(|(dataset, line)| format!("{dataset}:{line}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn print_variant(name: &str, report: &VariantReport) {
    println!("variant={name}");
    println!("resolved_count={}", report.resolved);
    println!("resolved_lines=[{}]", print_lines(&report.resolved_lines));
    println!("changed_residuals={}", report.changed);
    println!("known_zero={}", report.metrics.known_zero);
    println!("known_one={}", report.metrics.known_one);
    println!(
        "derived_zero_from_one={}",
        report.metrics.derived_zero_from_one
    );
    println!("selections_zero={}", report.metrics.selections_zero);
    println!("zero_by_even_scale={}", report.metrics.zero_by_even_scale);
    println!(
        "zero_by_existing_above={}",
        report.metrics.zero_by_existing_above
    );
    println!(
        "zero_by_one_xor_one={}",
        report.metrics.zero_by_one_xor_one
    );
    println!(
        "zero_by_one_plus_one={}",
        report.metrics.zero_by_one_plus_one
    );
    println!(
        "zero_by_one_minus_one={}",
        report.metrics.zero_by_one_minus_one
    );
    println!("zero_by_and={}", report.metrics.zero_by_and);
    println!("zero_by_other={}", report.metrics.zero_by_other);
    println!(
        "analysis_ms={:.3}",
        report.analysis_time.as_secs_f64() * 1_000.0
    );
    println!("regressions={}", report.regressions);
    println!();
}

fn main() {
    let cases = load_cases();
    let mut baseline_ng = 0;
    let mut p7e_resolved = 0;
    let mut current_pipeline_resolved = 0;
    let mut ng_input = 0;
    let mut full = VariantReport::default();
    let mut zero_only = VariantReport::default();
    let mut anchors_only = VariantReport::default();
    let mut p8b_then_p8c = VariantReport::default();

    for case in &cases {
        let (Ok(mba), Ok(gt)) = (
            simplify_mba(case.mba.clone(), BIT_COUNT),
            simplify_mba(case.ground_truth.clone(), BIT_COUNT),
        ) else {
            continue;
        };
        let residual = gt - mba;
        if simplify_mba(residual.clone(), BIT_COUNT) == Ok(Expr::zero()) {
            continue;
        }
        baseline_ng += 1;
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
            p7e_resolved += 1;
            continue;
        }
        let rich_form = p8a::analyze_expression(
            p7e.restored_after_substitution.clone(),
            BIT_COUNT,
        )
        .result;
        let after_current_pipeline = accept_p8b(rich_form.clone());
        if after_current_pipeline == Expr::zero() {
            current_pipeline_resolved += 1;
            continue;
        }
        ng_input += 1;

        run_variant(
            case,
            rich_form.clone(),
            &p7e.restored_after_substitution,
            P8cVariant::Full,
            &mut full,
        );
        run_variant(
            case,
            rich_form.clone(),
            &p7e.restored_after_substitution,
            P8cVariant::ZeroOnly,
            &mut zero_only,
        );
        run_variant(
            case,
            rich_form,
            &p7e.restored_after_substitution,
            P8cVariant::AnchorsOnly,
            &mut anchors_only,
        );

        // Order control: consume the representation already normalized by
        // P8b, and do not run P8b a second time.
        let started = Instant::now();
        let reverse = p8c::analyze_expression_with_variant(
            after_current_pipeline,
            BIT_COUNT,
            P8cVariant::Full,
        );
        p8b_then_p8c.analysis_time += started.elapsed();
        accumulate(&mut p8b_then_p8c.metrics, reverse.metrics);
        if reverse.changed {
            p8b_then_p8c.changed += 1;
        }
        if !semantically_equal(
            &p7e.restored_after_substitution,
            &reverse.result,
            BIT_COUNT,
        ) {
            p8b_then_p8c.regressions += 1;
        } else if reverse.result == Expr::zero() {
            p8b_then_p8c.resolved += 1;
            p8b_then_p8c.resolved_lines.push((case.dataset, case.line));
        }
    }

    println!("cases={}", cases.len());
    println!("baseline_NG={baseline_ng}");
    println!("p7e_resolved={p7e_resolved}");
    println!("current_p8a_p8b_resolved={current_pipeline_resolved}");
    println!("NG_input={ng_input}");
    println!();
    print_variant("full", &full);
    print_variant("zero_only", &zero_only);
    print_variant("anchors_only", &anchors_only);
    print_variant("p8b_then_p8c_full", &p8b_then_p8c);
    println!("smt_called=false");
    println!("pct_fallback_called=false");
    println!("normal_path_modified=false");
}
