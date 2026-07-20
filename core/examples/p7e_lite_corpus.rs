use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::{Duration, Instant},
};

use rumba_core::{
    expr::Expr,
    parser::parse_expr,
    simplify::{
        HiddenAtomDependencyKind, HiddenScopeTrace, diagnose_hidden_atoms,
        experiment_bitwise_dependency_closure, simplify_mba,
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

#[derive(Clone, Copy, Default)]
struct OperatorCounts {
    not: usize,
    scale: usize,
    and: usize,
    or: usize,
    xor: usize,
    add: usize,
    mul: usize,
}

struct UnresolvedSample<'a> {
    case: &'a Case,
    residual: Expr,
    after_p7e: Expr,
    direct_atoms: usize,
    hidden_atoms: usize,
    dependency_depth: usize,
    arithmetic_dependencies: usize,
    bitwise_dependencies: usize,
    lowbit_candidates: usize,
    proved_dependencies: usize,
    closure_iterations: usize,
    features: Vec<f64>,
    complexity: f64,
}

fn count_operators(expression: &Expr, counts: &mut OperatorCounts) {
    match expression {
        Expr::Var(_) | Expr::Const(_) => {}
        Expr::Not(inner) => {
            counts.not += 1;
            count_operators(inner, counts);
        }
        Expr::Scale(_, inner) => {
            counts.scale += 1;
            count_operators(inner, counts);
        }
        Expr::And(terms) => {
            counts.and += 1;
            for term in terms {
                count_operators(term, counts);
            }
        }
        Expr::Or(terms) => {
            counts.or += 1;
            for term in terms {
                count_operators(term, counts);
            }
        }
        Expr::Xor(terms) => {
            counts.xor += 1;
            for term in terms {
                count_operators(term, counts);
            }
        }
        Expr::Add(terms) => {
            counts.add += 1;
            for term in terms {
                count_operators(term, counts);
            }
        }
        Expr::Mul(terms) => {
            counts.mul += 1;
            for term in terms {
                count_operators(term, counts);
            }
        }
    }
}

fn hidden_dependency_depth(scope: &HiddenScopeTrace) -> usize {
    let graph: HashMap<_, _> = scope
        .atoms
        .iter()
        .map(|atom| (atom.atom, atom.dependent_atoms.as_slice()))
        .collect();
    fn visit(
        atom: rumba_core::expr::VarId,
        graph: &HashMap<rumba_core::expr::VarId, &[rumba_core::expr::VarId]>,
        active: &mut HashSet<rumba_core::expr::VarId>,
        memo: &mut HashMap<rumba_core::expr::VarId, usize>,
    ) -> usize {
        if let Some(depth) = memo.get(&atom) {
            return *depth;
        }
        if !active.insert(atom) {
            return 0;
        }
        let depth = 1 + graph
            .get(&atom)
            .into_iter()
            .flat_map(|parents| parents.iter())
            .map(|parent| visit(*parent, graph, active, memo))
            .max()
            .unwrap_or(0);
        active.remove(&atom);
        memo.insert(atom, depth);
        depth
    }

    let mut active = HashSet::new();
    let mut memo = HashMap::new();
    graph
        .keys()
        .map(|atom| visit(*atom, &graph, &mut active, &mut memo))
        .max()
        .unwrap_or(0)
}

fn normalized_features(samples: &mut [UnresolvedSample<'_>]) {
    let dimensions = samples.first().map_or(0, |sample| sample.features.len());
    for dimension in 0..dimensions {
        let minimum = samples
            .iter()
            .map(|sample| sample.features[dimension])
            .fold(f64::INFINITY, f64::min);
        let maximum = samples
            .iter()
            .map(|sample| sample.features[dimension])
            .fold(f64::NEG_INFINITY, f64::max);
        for sample in &mut *samples {
            sample.features[dimension] = if maximum > minimum {
                (sample.features[dimension] - minimum) / (maximum - minimum)
            } else {
                0.0
            };
        }
    }

    for sample in samples {
        sample.complexity = 0.20 * sample.features[0]
            + 0.25 * sample.features[1]
            + 0.10 * sample.features[2]
            + 0.10 * sample.features[3]
            + 0.15 * sample.features[4]
            + 0.10 * sample.features[5]
            + 0.10 * sample.features[6];
    }
}

fn feature_distance(left: &UnresolvedSample<'_>, right: &UnresolvedSample<'_>) -> f64 {
    left.features
        .iter()
        .zip(&right.features)
        .map(|(left, right)| (left - right).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn print_diverse_unresolved_samples(cases: &[Case], sample_count: usize) {
    let mut samples = Vec::new();
    for case in cases {
        let (Ok(simplified_mba), Ok(simplified_gt)) = (
            simplify_mba(case.mba.clone(), BIT_COUNT),
            simplify_mba(case.ground_truth.clone(), BIT_COUNT),
        ) else {
            continue;
        };
        let raw_residual = simplified_gt - simplified_mba;
        if simplify_mba(raw_residual.clone(), BIT_COUNT) == Ok(Expr::zero()) {
            continue;
        }
        let Ok((residual, trace)) = diagnose_hidden_atoms(raw_residual, BIT_COUNT) else {
            continue;
        };
        let Some(scope) = trace.iter().find(|scope| scope.input == residual) else {
            continue;
        };
        let Some(experiment) = experiment_bitwise_dependency_closure(scope)
            .ok()
            .flatten()
        else {
            continue;
        };
        if experiment.residual_zero {
            continue;
        }

        let mut operators = OperatorCounts::default();
        count_operators(&residual, &mut operators);
        let arithmetic_dependencies = scope
            .atoms
            .iter()
            .filter(|atom| atom.dependency_kind == HiddenAtomDependencyKind::ArithmeticDependent)
            .count();
        let bitwise_dependencies = scope
            .atoms
            .iter()
            .filter(|atom| atom.dependency_kind == HiddenAtomDependencyKind::BitwiseDependent)
            .count();
        let lowbit_candidates = scope
            .atoms
            .iter()
            .filter(|atom| atom.dependency_kind == HiddenAtomDependencyKind::LowBitCandidate)
            .count();
        let dependency_depth = hidden_dependency_depth(scope);
        let features = vec![
            (case.mba.size() as f64).ln_1p(),
            (residual.size() as f64).ln_1p(),
            (experiment.simplified_after_substitution.size() as f64).ln_1p(),
            experiment.direct_atoms.len() as f64,
            scope.atoms.len() as f64,
            dependency_depth as f64,
            arithmetic_dependencies as f64,
            bitwise_dependencies as f64,
            lowbit_candidates as f64,
            experiment.dependencies.len() as f64,
            experiment.closure_iterations as f64,
            operators.not as f64,
            operators.scale as f64,
            operators.and as f64,
            operators.or as f64,
            operators.xor as f64,
            operators.add as f64,
            operators.mul as f64,
            residual.get_vars().len() as f64,
        ];
        samples.push(UnresolvedSample {
            case,
            residual,
            after_p7e: experiment.simplified_after_substitution,
            direct_atoms: experiment.direct_atoms.len(),
            hidden_atoms: scope.atoms.len(),
            dependency_depth,
            arithmetic_dependencies,
            bitwise_dependencies,
            lowbit_candidates,
            proved_dependencies: experiment.dependencies.len(),
            closure_iterations: experiment.closure_iterations,
            features,
            complexity: 0.0,
        });
    }

    normalized_features(&mut samples);
    let mut hard_pool: Vec<_> = (0..samples.len()).collect();
    hard_pool.sort_by(|left, right| {
        samples[*right]
            .complexity
            .total_cmp(&samples[*left].complexity)
    });
    hard_pool.truncate((sample_count * 6).max(sample_count).min(samples.len()));
    let mut selected = Vec::new();
    if let Some(hardest) = hard_pool.first().copied() {
        selected.push(hardest);
    }
    while selected.len() < sample_count.min(samples.len()) {
        let Some(next) = hard_pool
            .iter()
            .copied()
            .filter(|index| !selected.contains(index))
            .max_by(|left, right| {
                let score = |index: usize| {
                    let candidate = &samples[index];
                    let diversity = selected
                        .iter()
                        .map(|index| feature_distance(candidate, &samples[*index]))
                        .fold(f64::INFINITY, f64::min);
                    diversity + 0.35 * candidate.complexity
                };
                score(*left).total_cmp(&score(*right))
            })
        else {
            break;
        };
        selected.push(next);
    }

    println!("unresolved={} selected={}", samples.len(), selected.len());
    for (rank, index) in selected.into_iter().enumerate() {
        let sample = &samples[index];
        println!(
            "sample={} dataset={} line={} complexity={:.3} input_nodes={} residual_nodes={} after_nodes={} vars={} direct_atoms={} hidden_atoms={} dependency_depth={} arithmetic_dependencies={} bitwise_dependencies={} lowbit_candidates={} proved_dependencies={} closure_iterations={}",
            rank + 1,
            sample.case.dataset,
            sample.case.line,
            sample.complexity,
            sample.case.mba.size(),
            sample.residual.size(),
            sample.after_p7e.size(),
            sample.residual.get_vars().len(),
            sample.direct_atoms,
            sample.hidden_atoms,
            sample.dependency_depth,
            sample.arithmetic_dependencies,
            sample.bitwise_dependencies,
            sample.lowbit_candidates,
            sample.proved_dependencies,
            sample.closure_iterations,
        );
        println!("  mba={}", sample.case.mba);
        println!("  ground_truth={}", sample.case.ground_truth);
        println!("  residual={}", sample.residual);
        println!("  after_p7e={}", sample.after_p7e);
    }
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
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.first().is_some_and(|argument| argument == "--samples") {
        let sample_count = arguments
            .get(1)
            .map(|value| value.parse::<usize>().expect("sample count must be an integer"))
            .unwrap_or(5);
        print_diverse_unresolved_samples(&cases, sample_count);
        return;
    }
    let trials = arguments
        .first()
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
