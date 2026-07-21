use std::time::{Duration, Instant};

use rumba_core::{
    expr::Expr,
    parser::parse_expr,
    simplify::simplify_mba,
    varint::make_mask,
};

#[allow(dead_code)]
#[path = "p8c_relative_lead_micro.rs"]
mod p8c;

use p8c::{P8cMetrics, P8cVariant};

const BIT_COUNT: u8 = 64;
const DATASETS: [&str; 7] = [
    include_str!("../../third_party/dataset/loki_tiny.csv"),
    include_str!("../../third_party/dataset/mba_flatten.csv"),
    include_str!("../../third_party/dataset/mba_obf_linear.csv"),
    include_str!("../../third_party/dataset/mba_obf_nonlinear.csv"),
    include_str!("../../third_party/dataset/neureduce.csv"),
    include_str!("../../third_party/dataset/qsynth_ea.csv"),
    include_str!("../../third_party/dataset/syntia.csv"),
];

#[derive(Default)]
struct VariantReport {
    attempted: usize,
    scopes_with_anchors: usize,
    changed: usize,
    unknown_unchanged_violations: usize,
    determinism_violations: usize,
    sampled_regressions: usize,
    trigger_time: Duration,
    analysis_time: Duration,
    metrics: P8cMetrics,
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

fn semantically_equal(left: &Expr, right: &Expr) -> bool {
    let mask = make_mask(BIT_COUNT);
    let max_var = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .map(|variable| variable.0)
        .max()
        .unwrap_or(0);
    let mut state = 0xd1b5_4a32_d192_ed03u64;
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

fn run_variant(input: &Expr, variant: P8cVariant, report: &mut VariantReport) {
    report.attempted += 1;
    let trigger_started = Instant::now();
    let triggered = p8c::has_lowbit_trigger(input, BIT_COUNT);
    report.trigger_time += trigger_started.elapsed();
    if !triggered {
        return;
    }
    report.scopes_with_anchors += 1;
    let started = Instant::now();
    let analysis = p8c::analyze_expression_with_variant(input.clone(), BIT_COUNT, variant);
    report.analysis_time += started.elapsed();
    if analysis.changed {
        report.changed += 1;
        if !semantically_equal(input, &analysis.result) {
            report.sampled_regressions += 1;
        }
    } else if analysis.unknown && analysis.result != *input {
        report.unknown_unchanged_violations += 1;
    }

    let repeated = p8c::analyze_expression_with_variant(input.clone(), BIT_COUNT, variant);
    if repeated.result != analysis.result
        || repeated.changed != analysis.changed
        || repeated.unknown != analysis.unknown
    {
        report.determinism_violations += 1;
    }
    accumulate(&mut report.metrics, analysis.metrics);
}

fn print_report(name: &str, report: &VariantReport, baseline_time: Duration) {
    let total_time = report.trigger_time + report.analysis_time;
    let overhead = if baseline_time.is_zero() {
        0.0
    } else {
        100.0 * total_time.as_secs_f64() / baseline_time.as_secs_f64()
    };
    println!("variant={name}");
    println!("attempted={}", report.attempted);
    println!("scopes_with_anchors={}", report.scopes_with_anchors);
    println!("changed={}", report.changed);
    println!("anchor_candidates={}", report.metrics.anchor_candidates);
    println!("anchors_recognized={}", report.metrics.anchors_recognized);
    println!("selections_zero={}", report.metrics.selections_zero);
    println!("selections_one={}", report.metrics.selections_one);
    println!(
        "unknown_unchanged_violations={}",
        report.unknown_unchanged_violations
    );
    println!("determinism_violations={}", report.determinism_violations);
    println!("sampled_regressions={}", report.sampled_regressions);
    println!(
        "trigger_ms={:.3}",
        report.trigger_time.as_secs_f64() * 1_000.0
    );
    println!(
        "analysis_ms={:.3}",
        report.analysis_time.as_secs_f64() * 1_000.0
    );
    println!("total_ms={:.3}", total_time.as_secs_f64() * 1_000.0);
    println!("overhead_percent={overhead:.3}");
    println!();
}

fn main() {
    let mut cases = 0;
    let mut simplified_nonzero = 0;
    let mut simplify_errors = 0;
    let mut baseline_time = Duration::ZERO;
    let mut full = VariantReport::default();
    let mut anchors_only = VariantReport::default();

    for csv in DATASETS {
        for row in csv.lines().filter(|row| !row.trim().is_empty()) {
            cases += 1;
            // Deliberately ignore the second CSV column: this benchmark has no
            // ground-truth oracle and sees only the MBA given to RUMBA.
            let mba = row.split_once(',').map_or(row, |(mba, _)| mba);
            let expression = parse_expr(mba.trim()).unwrap();
            let started = Instant::now();
            let simplified = simplify_mba(expression, BIT_COUNT);
            baseline_time += started.elapsed();
            let Ok(simplified) = simplified else {
                simplify_errors += 1;
                continue;
            };
            if simplified == Expr::zero() {
                continue;
            }
            simplified_nonzero += 1;
            run_variant(&simplified, P8cVariant::Full, &mut full);
            run_variant(
                &simplified,
                P8cVariant::AnchorsOnly,
                &mut anchors_only,
            );
        }
    }

    println!("cases={cases}");
    println!("simplified_nonzero={simplified_nonzero}");
    println!("simplify_errors={simplify_errors}");
    println!(
        "baseline_simplification_ms={:.3}",
        baseline_time.as_secs_f64() * 1_000.0
    );
    println!();
    print_report("full", &full, baseline_time);
    print_report("anchors_only", &anchors_only, baseline_time);
    println!("ground_truth_used=false");
    println!("smt_called=false");
    println!("pct_fallback_called=false");
    println!("normal_path_modified=false");
}
