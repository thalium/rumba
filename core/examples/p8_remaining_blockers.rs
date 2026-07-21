use std::collections::BTreeMap;

use rumba_core::{
    expr::Expr,
    p8::{AboveLowResult, classify_above_lowbit, experiment_pipeline},
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, simplify_mba},
    varint::make_mask,
};

const WIDTH: u8 = 64;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum FirstBlocker {
    NoLowBitCandidate,
    AnchorCandidateUnproved,
    AnchorProvedNoUse,
    AboveLowUnknown,
    MultipleAnchors,
    RewriteAppliedNonZero,
    TerminalNormalizationFailure,
}

impl FirstBlocker {
    fn name(self) -> &'static str {
        match self {
            Self::NoLowBitCandidate => "NoLowBitCandidate",
            Self::AnchorCandidateUnproved => "AnchorCandidateUnproved",
            Self::AnchorProvedNoUse => "AnchorProvedNoUse",
            Self::AboveLowUnknown => "AboveLowUnknown",
            Self::MultipleAnchors => "MultipleAnchors",
            Self::RewriteAppliedNonZero => "RewriteAppliedNonZero",
            Self::TerminalNormalizationFailure => "TerminalNormalizationFailure",
        }
    }
}

#[derive(Clone)]
struct AnchorSite {
    node: Expr,
    has_use: bool,
    plausible: bool,
    proved: bool,
    above: AboveLowResult,
}

#[derive(Default)]
struct RawSymptoms {
    no_lowbit_candidate: bool,
    anchor_candidate_unproved: bool,
    anchor_proved_no_use: bool,
    above_low_unknown: bool,
    multiple_anchors: bool,
    rewrite_applied_nonzero: bool,
    terminal_normalization_failure: bool,
}

fn conjunction(mut terms: Vec<Expr>) -> Expr {
    match terms.len() {
        0 => Expr::make_const(u64::MAX),
        1 => terms.pop().unwrap(),
        _ => Expr::And(terms),
    }
}

fn sampled_lowbit_shape(left: &Expr, right: &Expr) -> bool {
    let left_vars = left.get_vars();
    let right_vars = right.get_vars();
    if left_vars.is_disjoint(&right_vars) {
        return false;
    }
    let max_var = left_vars
        .into_iter()
        .chain(right_vars)
        .map(|variable| variable.0)
        .max()
        .unwrap_or(0);
    let mask = make_mask(WIDTH);
    for sample in 0..12usize {
        let values = (0..=max_var)
            .map(|variable| {
                let mut value = (sample as u64)
                    .wrapping_mul(0x9e37_79b9_7f4a_7c15)
                    .wrapping_add((variable as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9));
                value ^= value >> 30;
                value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
                value ^= value >> 27;
                value.wrapping_mul(0x94d0_49bb_1331_11eb)
            })
            .collect::<Vec<_>>();
        let intersection = (left.eval(&values).get(mask) & right.eval(&values).get(mask)) & mask;
        if intersection != 0 && !intersection.is_power_of_two() {
            return false;
        }
    }
    true
}

fn collect_anchor_sites(expression: &Expr, output: &mut Vec<AnchorSite>) {
    match expression {
        Expr::And(terms) => {
            for left in 0..terms.len() {
                for right in left + 1..terms.len() {
                    let proved = (terms[left].clone() + terms[right].clone())
                        .reduce(make_mask(WIDTH))
                        == Expr::zero();
                    let plausible = proved || sampled_lowbit_shape(&terms[left], &terms[right]);
                    if !plausible {
                        continue;
                    }
                    let remaining_terms = terms
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| *index != left && *index != right)
                        .map(|(_, term)| term.clone())
                        .collect::<Vec<_>>();
                    let remaining = conjunction(remaining_terms.clone());
                    let above = if remaining_terms.is_empty() {
                        AboveLowResult::Unknown
                    } else {
                        classify_above_lowbit(&terms[left], &remaining, WIDTH)
                    };
                    output.push(AnchorSite {
                        node: expression.clone(),
                        has_use: !remaining_terms.is_empty(),
                        plausible,
                        proved,
                        above,
                    });
                }
            }
            for term in terms {
                collect_anchor_sites(term, output);
            }
        }
        Expr::Not(inner) | Expr::Scale(_, inner) => collect_anchor_sites(inner, output),
        Expr::Or(terms) | Expr::Xor(terms) | Expr::Add(terms) | Expr::Mul(terms) => {
            for term in terms {
                collect_anchor_sites(term, output);
            }
        }
        Expr::Var(_) | Expr::Const(_) => {}
    }
}

fn replace_first(expression: Expr, target: &Expr, replacement: &Expr, done: &mut bool) -> Expr {
    if !*done && expression == *target {
        *done = true;
        return replacement.clone();
    }
    match expression {
        Expr::Var(_) | Expr::Const(_) => expression,
        Expr::Not(inner) => !replace_first(*inner, target, replacement, done),
        Expr::Scale(coefficient, inner) => {
            coefficient * replace_first(*inner, target, replacement, done)
        }
        Expr::And(terms) => Expr::And(
            terms
                .into_iter()
                .map(|term| replace_first(term, target, replacement, done))
                .collect(),
        ),
        Expr::Or(terms) => Expr::Or(
            terms
                .into_iter()
                .map(|term| replace_first(term, target, replacement, done))
                .collect(),
        ),
        Expr::Xor(terms) => Expr::Xor(
            terms
                .into_iter()
                .map(|term| replace_first(term, target, replacement, done))
                .collect(),
        ),
        Expr::Add(terms) => Expr::Add(
            terms
                .into_iter()
                .map(|term| replace_first(term, target, replacement, done))
                .collect(),
        ),
        Expr::Mul(terms) => Expr::Mul(
            terms
                .into_iter()
                .map(|term| replace_first(term, target, replacement, done))
                .collect(),
        ),
    }
}

fn counterfactual(expression: &Expr, site: &AnchorSite) -> Expr {
    let mut done = false;
    let assumed = replace_first(expression.clone(), &site.node, &Expr::zero(), &mut done);
    simplify_mba(assumed, WIDTH).unwrap_or_else(|_| expression.clone())
}

fn reduction_percent(before: &Expr, after: &Expr) -> usize {
    if after == &Expr::zero() {
        return 100;
    }
    100usize.saturating_sub(after.size().saturating_mul(100) / before.size().max(1))
}

fn classify(
    sites: &[AnchorSite],
    p8_changed: bool,
    residual_nonzero: bool,
) -> (FirstBlocker, RawSymptoms) {
    let plausible = sites.iter().filter(|site| site.plausible).count();
    let proved = sites.iter().filter(|site| site.proved).count();
    let proved_with_use = sites
        .iter()
        .filter(|site| site.proved && site.has_use)
        .count();
    let mut symptoms = RawSymptoms {
        no_lowbit_candidate: plausible == 0,
        anchor_candidate_unproved: sites.iter().any(|site| site.plausible && !site.proved),
        anchor_proved_no_use: proved != 0 && proved_with_use == 0,
        above_low_unknown: sites.iter().any(|site| {
            site.proved
                && site.has_use
                && site.above != AboveLowResult::AboveLowBit
        }),
        multiple_anchors: proved > 1,
        rewrite_applied_nonzero: p8_changed && residual_nonzero,
        terminal_normalization_failure: p8_changed && residual_nonzero,
    };
    let first = if symptoms.no_lowbit_candidate {
        FirstBlocker::NoLowBitCandidate
    } else if symptoms.anchor_candidate_unproved {
        FirstBlocker::AnchorCandidateUnproved
    } else if symptoms.anchor_proved_no_use {
        FirstBlocker::AnchorProvedNoUse
    } else if symptoms.above_low_unknown {
        FirstBlocker::AboveLowUnknown
    } else if symptoms.multiple_anchors {
        FirstBlocker::MultipleAnchors
    } else if symptoms.rewrite_applied_nonzero {
        FirstBlocker::RewriteAppliedNonZero
    } else {
        symptoms.terminal_normalization_failure = true;
        FirstBlocker::TerminalNormalizationFailure
    };
    (first, symptoms)
}

fn main() {
    let mut ng_input = 0;
    let mut first_counts = BTreeMap::<&str, usize>::new();
    let mut raw_counts = BTreeMap::<&str, usize>::new();
    let mut counterfactual_anchor_zero = 0;
    let mut counterfactual_above_zero = 0;
    let mut counterfactual_reduced_over_50 = 0;
    let mut causal_lines = Vec::new();
    let mut case_lines = Vec::new();

    for (dataset, csv) in DATASETS {
        for (index, row) in csv.lines().filter(|row| !row.trim().is_empty()).enumerate() {
            let (mba, ground_truth) = row.split_once(',').unwrap();
            let (Ok(mba), Ok(ground_truth)) = (
                simplify_mba(parse_expr(mba.trim()).unwrap(), WIDTH),
                simplify_mba(parse_expr(ground_truth.trim()).unwrap(), WIDTH),
            ) else {
                continue;
            };
            let residual = ground_truth - mba;
            if simplify_mba(residual.clone(), WIDTH) == Ok(Expr::zero()) {
                continue;
            }
            let Ok((diagnosed, trace)) = diagnose_hidden_atoms(residual, WIDTH) else {
                continue;
            };
            let Some(scope) = trace.iter().find(|scope| scope.input == diagnosed) else {
                continue;
            };
            let Some(pipeline) = experiment_pipeline(scope).unwrap() else {
                continue;
            };
            if pipeline.residual_zero {
                continue;
            }
            ng_input += 1;
            let expression = pipeline.result;
            let mut sites = Vec::new();
            collect_anchor_sites(&expression, &mut sites);
            let p8_changed = pipeline.after_p8a.changed
                || pipeline.after_p8b.changed
                || pipeline.after_p8c.changed;
            let (first, symptoms) = classify(&sites, p8_changed, true);
            *first_counts.entry(first.name()).or_default() += 1;
            for (name, present) in [
                ("NoLowBitCandidate", symptoms.no_lowbit_candidate),
                ("AnchorCandidateUnproved", symptoms.anchor_candidate_unproved),
                ("AnchorProvedNoUse", symptoms.anchor_proved_no_use),
                ("AboveLowUnknown", symptoms.above_low_unknown),
                ("MultipleAnchors", symptoms.multiple_anchors),
                ("RewriteAppliedNonZero", symptoms.rewrite_applied_nonzero),
                (
                    "TerminalNormalizationFailure",
                    symptoms.terminal_normalization_failure,
                ),
            ] {
                if present {
                    *raw_counts.entry(name).or_default() += 1;
                }
            }

            let mut anchor_zero = false;
            let mut above_zero = false;
            let mut best_reduction = 0;
            for site in &sites {
                let eligible_anchor = site.plausible
                    && !site.proved
                    && site.above == AboveLowResult::AboveLowBit;
                let eligible_above = site.proved
                    && site.has_use
                    && site.above != AboveLowResult::AboveLowBit;
                if !eligible_anchor && !eligible_above {
                    continue;
                }
                let result = counterfactual(&expression, site);
                best_reduction = best_reduction.max(reduction_percent(&expression, &result));
                if eligible_anchor && result == Expr::zero() {
                    anchor_zero = true;
                }
                if eligible_above && result == Expr::zero() {
                    above_zero = true;
                }
            }
            counterfactual_anchor_zero += usize::from(anchor_zero);
            counterfactual_above_zero += usize::from(above_zero);
            counterfactual_reduced_over_50 += usize::from(best_reduction > 50);
            if anchor_zero || above_zero || best_reduction > 50 {
                causal_lines.push(format!(
                    "{dataset}:{}:{}:anchor_zero={anchor_zero}:above_zero={above_zero}:reduction={best_reduction}",
                    index + 1,
                    first.name(),
                ));
            }
            case_lines.push(format!(
                "{dataset}:{}:first_blocker={}:counterfactual_anchor_zero={anchor_zero}:counterfactual_above_zero={above_zero}:counterfactual_reduction_percent={best_reduction}",
                index + 1,
                first.name(),
            ));
        }
    }

    println!("NG_input={ng_input}");
    for blocker in [
        FirstBlocker::NoLowBitCandidate,
        FirstBlocker::AnchorCandidateUnproved,
        FirstBlocker::AnchorProvedNoUse,
        FirstBlocker::AboveLowUnknown,
        FirstBlocker::MultipleAnchors,
        FirstBlocker::RewriteAppliedNonZero,
        FirstBlocker::TerminalNormalizationFailure,
    ] {
        println!(
            "first_blocker_{}={}",
            blocker.name(),
            first_counts.get(blocker.name()).copied().unwrap_or(0)
        );
    }
    println!();
    for name in [
        "NoLowBitCandidate",
        "AnchorCandidateUnproved",
        "AnchorProvedNoUse",
        "AboveLowUnknown",
        "MultipleAnchors",
        "RewriteAppliedNonZero",
        "TerminalNormalizationFailure",
    ] {
        println!(
            "raw_{name}={}",
            raw_counts.get(name).copied().unwrap_or(0)
        );
    }
    println!();
    println!("counterfactual_anchor_zero={counterfactual_anchor_zero}");
    println!("counterfactual_above_zero={counterfactual_above_zero}");
    println!(
        "counterfactual_reduced_over_50_percent={counterfactual_reduced_over_50}"
    );
    println!("causal_lines=[{}]", causal_lines.join(","));
    println!("case_reports=[{}]", case_lines.join(","));
    println!("normal_path_modified=false");
}
