//! Descriptor-native FSC-2 frontend.
//!
//! Recipes are untrusted producers certified exhaustively by the tests below.
//! Runtime acceptance is exact descriptor-key lookup; misses preserve the
//! existing v1 simplifier unchanged.

use crate::{
    expr::{Expr, VarId},
    factorized_section::{OneBaseDescriptor, compile_one_base},
};

#[derive(Clone, Copy)]
#[repr(u8)]
enum Recipe {
    Zero = 0,
    Nor = 1,
    XAndNotY = 2,
    NotY = 3,
    NotXAndY = 4,
    NotX = 5,
    Xor = 6,
    Nand = 7,
    And = 8,
    Xnor = 9,
    X = 10,
    XOrNotY = 11,
    Y = 12,
    NotXOrY = 13,
    Or = 14,
    Ones = 15,
}

impl Recipe {
    fn from_signature(signature: u8) -> Self {
        match signature {
            0 => Self::Zero,
            1 => Self::Nor,
            2 => Self::XAndNotY,
            3 => Self::NotY,
            4 => Self::NotXAndY,
            5 => Self::NotX,
            6 => Self::Xor,
            7 => Self::Nand,
            8 => Self::And,
            9 => Self::Xnor,
            10 => Self::X,
            11 => Self::XOrNotY,
            12 => Self::Y,
            13 => Self::NotXOrY,
            14 => Self::Or,
            15 => Self::Ones,
            _ => unreachable!("FSC-2 signatures are four bits"),
        }
    }

    fn instantiate(self, variables: [VarId; 2]) -> Expr {
        let x = || Expr::Var(variables[0]);
        let y = || Expr::Var(variables[1]);
        match self {
            Self::Zero => Expr::Const(0),
            Self::Nor => Expr::Not(Box::new(Expr::Or(vec![x(), y()]))),
            Self::XAndNotY => Expr::And(vec![x(), Expr::Not(Box::new(y()))]),
            Self::NotY => Expr::Not(Box::new(y())),
            Self::NotXAndY => Expr::And(vec![Expr::Not(Box::new(x())), y()]),
            Self::NotX => Expr::Not(Box::new(x())),
            Self::Xor => Expr::Xor(vec![x(), y()]),
            Self::Nand => Expr::Not(Box::new(Expr::And(vec![x(), y()]))),
            Self::And => Expr::And(vec![x(), y()]),
            Self::Xnor => Expr::Not(Box::new(Expr::Xor(vec![x(), y()]))),
            Self::X => x(),
            Self::XOrNotY => Expr::Or(vec![x(), Expr::Not(Box::new(y()))]),
            Self::Y => y(),
            Self::NotXOrY => Expr::Or(vec![Expr::Not(Box::new(x())), y()]),
            Self::Or => Expr::Or(vec![x(), y()]),
            Self::Ones => Expr::Const(u64::MAX),
        }
    }
}

fn signature(descriptor: OneBaseDescriptor) -> Option<u8> {
    let zero = descriptor.r0.checked_neg()?;
    if !matches!(zero, 0 | 1) {
        return None;
    }
    let mut signature = zero as u8;
    for (index, epsilon) in descriptor.epsilon_nonzero.into_iter().enumerate() {
        let value = zero.checked_add(epsilon)?;
        if !matches!(value, 0 | 1) {
            return None;
        }
        signature |= (value as u8) << (index + 1);
    }
    Some(signature)
}

fn render(descriptor: OneBaseDescriptor) -> Option<Expr> {
    Some(Recipe::from_signature(signature(descriptor)?).instantiate(descriptor.variables))
}

pub(crate) fn simplify(source: &Expr, width: u8) -> Option<Expr> {
    render(compile_one_base(source, width)?)
}

#[cfg(all(test, feature = "parse"))]
mod tests {
    use std::{hint::black_box, time::Instant};

    use super::*;
    use crate::{
        factorized_section::{
            compile_declared_one_base, compile_one_base_authoritative, compile_one_base_profiled,
        },
        parser::parse_expr,
        simplify::{simplify_mba, simplify_mba_v1},
    };

    const WIDTH: u8 = 64;
    const DATASETS: [(&str, &str); 7] = [
        (
            "loki_tiny.csv",
            include_str!("../../third_party/dataset/loki_tiny.csv"),
        ),
        (
            "mba_flatten.csv",
            include_str!("../../third_party/dataset/mba_flatten.csv"),
        ),
        (
            "mba_obf_linear.csv",
            include_str!("../../third_party/dataset/mba_obf_linear.csv"),
        ),
        (
            "mba_obf_nonlinear.csv",
            include_str!("../../third_party/dataset/mba_obf_nonlinear.csv"),
        ),
        (
            "neureduce.csv",
            include_str!("../../third_party/dataset/neureduce.csv"),
        ),
        (
            "qsynth_ea.csv",
            include_str!("../../third_party/dataset/qsynth_ea.csv"),
        ),
        (
            "syntia.csv",
            include_str!("../../third_party/dataset/syntia.csv"),
        ),
    ];

    fn corpus() -> Vec<Expr> {
        DATASETS
            .into_iter()
            .flat_map(|(dataset, contents)| {
                contents
                    .lines()
                    .enumerate()
                    .filter_map(move |(index, row)| {
                        if row.trim().is_empty() {
                            return None;
                        }
                        let (source, _) = row.split_once(',').unwrap_or_else(|| {
                            panic!("{dataset}:{} is not a two-column row", index + 1)
                        });
                        Some(parse_expr(source.trim()).expect("corpus expression"))
                    })
            })
            .collect()
    }

    fn same_key(left: OneBaseDescriptor, right: OneBaseDescriptor) -> bool {
        left.variables == right.variables
            && left.r0 == right.r0
            && left.epsilon_nonzero == right.epsilon_nonzero
    }

    fn semantic_mismatch(source: &Expr, candidate: &Expr, variables: [VarId; 2]) -> bool {
        let input_len = variables[1].0 + 1;
        let mut inputs = vec![0; input_len];
        (0..16u64).any(|seed| {
            inputs[variables[0].0] = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            inputs[variables[1].0] = seed.rotate_left(17).wrapping_mul(0xd6e8_feb8_6659_fd93);
            source.eval(&inputs, WIDTH) != candidate.eval(&inputs, WIDTH)
        })
    }

    fn median(values: &mut [u128]) -> u128 {
        values.sort_unstable();
        values[values.len() / 2]
    }

    #[test]
    fn all_static_recipes_replay_and_match_their_descriptors() {
        let variables = [VarId(0), VarId(1)];
        for expected in 0..16u8 {
            let candidate = Recipe::from_signature(expected).instantiate(variables);
            let mut replay = 0u8;
            for assignment in 0..4 {
                let inputs = [
                    u64::from(assignment & 1 != 0),
                    u64::from(assignment & 2 != 0),
                ];
                replay |= ((candidate.eval(&inputs, 1) & 1) as u8) << assignment;
            }
            assert_eq!(replay, expected);
            let zero = i128::from(expected & 1);
            let expected_key = (
                -zero,
                std::array::from_fn(|index| i128::from((expected >> (index + 1)) & 1) - zero),
            );
            assert_eq!(
                compile_declared_one_base(&candidate, variables),
                Some(expected_key)
            );
        }
    }

    #[test]
    fn unsupported_paths_fail_closed_into_v1() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let z = Expr::Var(VarId(2));
        for source in [
            Expr::Mul(vec![x.clone(), y.clone()]),
            Expr::Add(vec![x.clone(), y.clone(), z]),
            Expr::And(vec![x + Expr::Const(1), y]),
        ] {
            assert!(simplify(&source, WIDTH).is_none());
            assert_eq!(
                simplify_mba(source.clone(), WIDTH),
                simplify_mba_v1(source, WIDTH)
            );
        }
    }

    #[test]
    #[ignore = "full FSC-2 41k differential"]
    fn fsc_baseline_41k_differential() {
        let corpus = corpus();
        assert_eq!(corpus.len(), 41_000);
        let mut one_base = 0usize;
        let mut direct = 0usize;
        let mut terminal = 0usize;
        let mut false_positive = 0usize;
        let mut descriptor_mismatch = 0usize;
        let mut semantic_mismatch_count = 0usize;
        let mut cost_regression = 0usize;
        let mut terminal_lost = 0usize;
        let mut production_mismatch = 0usize;
        for source in &corpus {
            let authoritative = compile_one_base_authoritative(source, WIDTH);
            let compiled = compile_one_base(source, WIDTH);
            one_base += usize::from(authoritative.is_some());
            direct += usize::from(compiled.is_some_and(|value| value.direct));
            false_positive += usize::from(compiled.is_some() && authoritative.is_none());
            descriptor_mismatch += usize::from(
                compiled
                    .zip(authoritative)
                    .is_some_and(|(left, right)| !same_key(left, right)),
            );
            let expected_candidate = authoritative.and_then(render);
            let candidate = compiled.and_then(render);
            terminal_lost += usize::from(expected_candidate.is_some() && candidate.is_none());
            if let Some(candidate) = candidate {
                terminal += 1;
                production_mismatch += usize::from(
                    simplify_mba(source.clone(), WIDTH).expect("production frontend") != candidate,
                );
                descriptor_mismatch += usize::from(
                    compile_declared_one_base(&candidate, compiled.unwrap().variables)
                        != compiled.map(|value| (value.r0, value.epsilon_nonzero)),
                );
                semantic_mismatch_count += usize::from(semantic_mismatch(
                    source,
                    &candidate,
                    compiled.unwrap().variables,
                ));
                let v1 = simplify_mba_v1(source.clone(), WIDTH).expect("v1");
                cost_regression += usize::from(candidate.size() > v1.size());
            }
        }
        println!(
            "FSC_BASELINE_41K passed={} one_base={} direct={} terminal={} false_positive={} descriptor_mismatch={} semantic_mismatch={} cost_regression={} terminal_lost={} production_mismatch={}",
            corpus.len(),
            one_base,
            direct,
            terminal,
            false_positive,
            descriptor_mismatch,
            semantic_mismatch_count,
            cost_regression,
            terminal_lost,
            production_mismatch
        );
        assert_eq!(one_base, 25_407);
        assert_eq!(direct, 6_892);
        assert_eq!(terminal, 15_164);
        assert_eq!(false_positive, 0);
        assert_eq!(descriptor_mismatch, 0);
        assert_eq!(semantic_mismatch_count, 0);
        assert_eq!(cost_regression, 0);
        assert_eq!(terminal_lost, 0);
        assert_eq!(production_mismatch, 0);
    }

    #[derive(Default)]
    struct Breakdown {
        source_analysis_ns: u128,
        direct_compile_ns: u128,
        general_fallback_ns: u128,
        descriptor_lookup_ns: u128,
        v1_fallback_ns: u128,
    }

    fn frontend(source: &Expr) -> (Expr, Breakdown) {
        let compile = compile_one_base_profiled(source, WIDTH);
        let lookup_started = Instant::now();
        let candidate = compile.descriptor.and_then(render);
        let descriptor_lookup_ns = lookup_started.elapsed().as_nanos();
        if let Some(candidate) = candidate {
            return (
                candidate,
                Breakdown {
                    source_analysis_ns: compile.source_analysis_ns,
                    direct_compile_ns: compile.direct_compile_ns,
                    general_fallback_ns: compile.general_fallback_ns,
                    descriptor_lookup_ns,
                    v1_fallback_ns: 0,
                },
            );
        }
        let fallback_started = Instant::now();
        let result = simplify_mba_v1(source.clone(), WIDTH).expect("v1 fallback");
        (
            result,
            Breakdown {
                source_analysis_ns: compile.source_analysis_ns,
                direct_compile_ns: compile.direct_compile_ns,
                general_fallback_ns: compile.general_fallback_ns,
                descriptor_lookup_ns,
                v1_fallback_ns: fallback_started.elapsed().as_nanos(),
            },
        )
    }

    #[test]
    #[ignore = "seven-sweep release FSC baseline benchmark"]
    fn fsc_baseline_release_benchmark() {
        const RUNS: usize = 7;
        let corpus = corpus();
        for source in corpus.iter().take(100) {
            black_box(simplify_mba_v1(source.clone(), WIDTH).expect("warm v1"));
            black_box(frontend(source).0);
        }
        let mut v1_totals = [0u128; RUNS];
        let mut baseline_totals = [0u128; RUNS];
        let mut breakdown: [Breakdown; RUNS] = std::array::from_fn(|_| Breakdown::default());
        for run in 0..RUNS {
            for (index, source) in corpus.iter().enumerate() {
                for offset in 0..2 {
                    let variant = (index + run + offset) % 2;
                    let started = Instant::now();
                    if variant == 0 {
                        black_box(simplify_mba_v1(black_box(source.clone()), WIDTH).expect("v1"));
                        v1_totals[run] += started.elapsed().as_nanos();
                    } else {
                        let (output, phases) = frontend(black_box(source));
                        black_box(output);
                        baseline_totals[run] += started.elapsed().as_nanos();
                        breakdown[run].source_analysis_ns += phases.source_analysis_ns;
                        breakdown[run].direct_compile_ns += phases.direct_compile_ns;
                        breakdown[run].general_fallback_ns += phases.general_fallback_ns;
                        breakdown[run].descriptor_lookup_ns += phases.descriptor_lookup_ns;
                        breakdown[run].v1_fallback_ns += phases.v1_fallback_ns;
                    }
                }
            }
        }
        let paired: [f64; RUNS] = std::array::from_fn(|run| {
            100.0 * (baseline_totals[run] as f64 - v1_totals[run] as f64) / v1_totals[run] as f64
        });
        let mut paired_ordered = paired;
        paired_ordered.sort_by(f64::total_cmp);
        let paired_median = paired_ordered[RUNS / 2];
        let mut deviations = paired.map(|value| (value - paired_median).abs());
        deviations.sort_by(f64::total_cmp);
        let mad = deviations[RUNS / 2];
        let v1_median = median(&mut v1_totals);
        let baseline_median = median(&mut baseline_totals);
        let phase_median = |select: fn(&Breakdown) -> u128| {
            let mut values = breakdown.iter().map(select).collect::<Vec<_>>();
            median(&mut values)
        };
        println!(
            "FSC_BASELINE_BENCH v1_median_ns={} baseline_median_ns={} paired_delta_percent={:.4} mad_pp={:.4} source_analysis_ns={} direct_compile_ns={} general_fallback_ns={} descriptor_lookup_ns={} v1_fallback_ns={}",
            v1_median,
            baseline_median,
            paired_median,
            mad,
            phase_median(|value| value.source_analysis_ns),
            phase_median(|value| value.direct_compile_ns),
            phase_median(|value| value.general_fallback_ns),
            phase_median(|value| value.descriptor_lookup_ns),
            phase_median(|value| value.v1_fallback_ns),
        );
    }
}
