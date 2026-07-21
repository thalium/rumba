use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use rumba_core::{
    expr::{Expr, VarId},
    simplify::{
        BinaryBitwiseDependencyCandidate, BitwiseDependencyClosureExperiment,
        CertifiedBitwiseDependency, HiddenAtomTrace, HiddenScopeTrace,
        discover_binary_bitwise_dependency_candidates, simplify_mba,
    },
    varint::{VarInt, make_mask},
};

const WIDTHS: [u8; 5] = [4, 8, 16, 32, 64];
const MAX_PASSES: usize = 2;
const MAX_ANCHORS: usize = 4;
const MAX_NODES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RelativeTz {
    Unknown,
    AtLeastLowBit,
    AboveLowBit,
}

#[derive(Clone, Debug)]
struct Anchor {
    expression: Expr,
    width: u8,
}

#[derive(Default)]
struct RelativeTzAnalyzer {
    memo: HashMap<(Expr, Expr), RelativeTz>,
    queries: usize,
    above_lowbit_proved: usize,
}

impl RelativeTzAnalyzer {
    fn analyze(&mut self, anchor: &Expr, node: &Expr, width: u8) -> RelativeTz {
        self.queries += 1;
        let key = (anchor.clone(), node.clone());
        if let Some(level) = self.memo.get(&key) {
            return *level;
        }
        let mask = make_mask(width);
        let anchor = anchor.clone().reduce(mask);
        let node = node.clone().reduce(mask);
        let level = if node == Expr::zero() {
            RelativeTz::AboveLowBit
        } else if node == anchor || is_arithmetic_negation(&node, &anchor, width) {
            RelativeTz::AtLeastLowBit
        } else {
            match &node {
                Expr::Var(_) | Expr::Const(_) | Expr::Not(_) => RelativeTz::Unknown,
                Expr::Scale(coefficient, inner) => {
                    let inner = self.analyze(&anchor, inner, width);
                    if coefficient.get(mask) & 1 == 1 {
                        inner
                    } else if inner >= RelativeTz::AtLeastLowBit {
                        RelativeTz::AboveLowBit
                    } else {
                        RelativeTz::Unknown
                    }
                }
                Expr::Mul(terms) | Expr::And(terms) => terms
                    .iter()
                    .map(|term| self.analyze(&anchor, term, width))
                    .max()
                    .unwrap_or(RelativeTz::Unknown),
                Expr::Add(terms) | Expr::Or(terms) | Expr::Xor(terms) => terms
                    .iter()
                    .map(|term| self.analyze(&anchor, term, width))
                    .min()
                    .unwrap_or(RelativeTz::AboveLowBit),
            }
        };
        if level == RelativeTz::AboveLowBit {
            self.above_lowbit_proved += 1;
        }
        self.memo.insert(key, level);
        level
    }
}

fn is_arithmetic_negation(node: &Expr, anchor: &Expr, width: u8) -> bool {
    let mask = make_mask(width);
    (node.clone() + anchor.clone()).reduce(mask) == Expr::zero()
}

/// Convert conjunction-basis NOTs to arithmetic collection without invoking
/// the MBA solver: `a & ~b == a - (a & b)`.
fn collect_conjunctive_arithmetic(expression: Expr, width: u8) -> Expr {
    let mask = make_mask(width);
    let expression = match expression {
        Expr::Var(_) | Expr::Const(_) => expression,
        Expr::Not(inner) => !collect_conjunctive_arithmetic(*inner, width),
        Expr::Scale(coefficient, inner) => {
            coefficient * collect_conjunctive_arithmetic(*inner, width)
        }
        Expr::And(terms) => {
            let terms: Vec<_> = terms
                .into_iter()
                .map(|term| collect_conjunctive_arithmetic(term, width))
                .collect();
            if terms.len() == 2 {
                if let Expr::Not(right) = &terms[1] {
                    terms[0].clone() - (terms[0].clone() & (**right).clone())
                } else if let Expr::Not(left) = &terms[0] {
                    terms[1].clone() - (terms[1].clone() & (**left).clone())
                } else {
                    Expr::And(terms)
                }
            } else {
                Expr::And(terms)
            }
        }
        Expr::Or(terms) => Expr::Or(
            terms
                .into_iter()
                .map(|term| collect_conjunctive_arithmetic(term, width))
                .collect(),
        ),
        Expr::Xor(terms) => Expr::Xor(
            terms
                .into_iter()
                .map(|term| collect_conjunctive_arithmetic(term, width))
                .collect(),
        ),
        Expr::Add(terms) => Expr::Add(
            terms
                .into_iter()
                .map(|term| collect_conjunctive_arithmetic(term, width))
                .collect(),
        ),
        Expr::Mul(terms) => Expr::Mul(
            terms
                .into_iter()
                .map(|term| collect_conjunctive_arithmetic(term, width))
                .collect(),
        ),
    };
    expression.reduce(mask)
}

#[derive(Clone, Copy, Debug, Default)]
struct NormalizationTimings {
    relative_tz_analysis: Duration,
    normalization: Duration,
    relative_tz_queries: usize,
    above_lowbit_proved: usize,
    masked_negations_normalized: usize,
}

fn normalize_masked_negation_once(
    expression: Expr,
    node_width: u8,
    anchors: &[Anchor],
    analyzer: &mut RelativeTzAnalyzer,
    timings: &mut NormalizationTimings,
) -> (Expr, bool) {
    let rewrite_children = |terms: Vec<Expr>,
                            analyzer: &mut RelativeTzAnalyzer,
                            timings: &mut NormalizationTimings| {
        let mut changed = false;
        let terms = terms
            .into_iter()
            .map(|term| {
                let (term, term_changed) = normalize_masked_negation_once(
                    term,
                    node_width,
                    anchors,
                    analyzer,
                    timings,
                );
                changed |= term_changed;
                term
            })
            .collect::<Vec<_>>();
        (terms, changed)
    };

    match expression {
        Expr::Var(_) | Expr::Const(_) => (expression, false),
        Expr::Not(inner) => {
            let (inner, changed) = normalize_masked_negation_once(
                *inner,
                node_width,
                anchors,
                analyzer,
                timings,
            );
            (!inner, changed)
        }
        Expr::Scale(coefficient, inner) => {
            let (inner, changed) = normalize_masked_negation_once(
                *inner,
                node_width,
                anchors,
                analyzer,
                timings,
            );
            (coefficient * inner, changed)
        }
        Expr::And(terms) => {
            let (terms, child_changed) = rewrite_children(terms, analyzer, timings);
            if terms.len() == 2 {
                for anchor in anchors.iter().take(MAX_ANCHORS) {
                    if anchor.width != node_width {
                        continue;
                    }
                    let pair = if is_arithmetic_negation(&terms[0], &anchor.expression, node_width)
                    {
                        Some((&terms[1], &terms[0]))
                    } else if is_arithmetic_negation(
                        &terms[1],
                        &anchor.expression,
                        node_width,
                    ) {
                        Some((&terms[0], &terms[1]))
                    } else {
                        None
                    };
                    let Some((word_mask, _negated_anchor)) = pair else {
                        continue;
                    };
                    let started = Instant::now();
                    let level = analyzer.analyze(&anchor.expression, word_mask, node_width);
                    timings.relative_tz_analysis += started.elapsed();
                    if level != RelativeTz::AboveLowBit {
                        continue;
                    }
                    let started = Instant::now();
                    let replacement = word_mask.clone()
                        - (word_mask.clone() & anchor.expression.clone());
                    timings.normalization += started.elapsed();
                    timings.masked_negations_normalized += 1;
                    return (replacement.reduce(make_mask(node_width)), true);
                }
            }
            (Expr::And(terms), child_changed)
        }
        Expr::Or(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, timings);
            (Expr::Or(terms), changed)
        }
        Expr::Xor(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, timings);
            (Expr::Xor(terms), changed)
        }
        Expr::Add(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, timings);
            (Expr::Add(terms), changed)
        }
        Expr::Mul(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, timings);
            (Expr::Mul(terms), changed)
        }
    }
}

fn normalize_masked_negations(
    original: Expr,
    node_width: u8,
    anchors: &[Anchor],
) -> (Expr, bool, NormalizationTimings) {
    if original.size() > MAX_NODES || anchors.len() > MAX_ANCHORS {
        return (original, false, NormalizationTimings::default());
    }
    let mut current = original.clone();
    let mut changed = false;
    let mut analyzer = RelativeTzAnalyzer::default();
    let mut timings = NormalizationTimings::default();
    for _ in 0..MAX_PASSES {
        let (next, pass_changed) = normalize_masked_negation_once(
            current,
            node_width,
            anchors,
            &mut analyzer,
            &mut timings,
        );
        current = next.reduce(make_mask(node_width));
        changed |= pass_changed;
        if !pass_changed {
            break;
        }
    }
    timings.relative_tz_queries = analyzer.queries;
    timings.above_lowbit_proved = analyzer.above_lowbit_proved;
    if changed {
        (current, true, timings)
    } else {
        (original, false, timings)
    }
}

fn replace_subexpressions(
    expression: Expr,
    first: (&Expr, &Expr),
    second: (&Expr, &Expr),
) -> Expr {
    if expression == *first.0 {
        return first.1.clone();
    }
    if expression == *second.0 {
        return second.1.clone();
    }
    match expression {
        Expr::Var(_) | Expr::Const(_) => expression,
        Expr::Not(inner) => !replace_subexpressions(*inner, first, second),
        Expr::Scale(coefficient, inner) => {
            coefficient * replace_subexpressions(*inner, first, second)
        }
        Expr::And(terms) => Expr::And(
            terms
                .into_iter()
                .map(|term| replace_subexpressions(term, first, second))
                .collect(),
        ),
        Expr::Or(terms) => Expr::Or(
            terms
                .into_iter()
                .map(|term| replace_subexpressions(term, first, second))
                .collect(),
        ),
        Expr::Xor(terms) => Expr::Xor(
            terms
                .into_iter()
                .map(|term| replace_subexpressions(term, first, second))
                .collect(),
        ),
        Expr::Add(terms) => Expr::Add(
            terms
                .into_iter()
                .map(|term| replace_subexpressions(term, first, second))
                .collect(),
        ),
        Expr::Mul(terms) => Expr::Mul(
            terms
                .into_iter()
                .map(|term| replace_subexpressions(term, first, second))
                .collect(),
        ),
    }
}

fn absorb_bitwise_frontier(expression: Expr, width: u8) -> Expr {
    let expression = match expression {
        Expr::Var(_) | Expr::Const(_) => expression,
        Expr::Not(inner) => !absorb_bitwise_frontier(*inner, width),
        Expr::Scale(coefficient, inner) => coefficient * absorb_bitwise_frontier(*inner, width),
        Expr::And(terms) => {
            let mut flattened = Vec::new();
            for term in terms {
                match absorb_bitwise_frontier(term, width) {
                    Expr::And(nested) => flattened.extend(nested),
                    term => flattened.push(term),
                }
            }
            flattened.sort();
            flattened.dedup();
            Expr::And(flattened)
        }
        Expr::Or(terms) => Expr::Or(
            terms
                .into_iter()
                .map(|term| absorb_bitwise_frontier(term, width))
                .collect(),
        ),
        Expr::Xor(terms) => Expr::Xor(
            terms
                .into_iter()
                .map(|term| absorb_bitwise_frontier(term, width))
                .collect(),
        ),
        Expr::Add(terms) => Expr::Add(
            terms
                .into_iter()
                .map(|term| absorb_bitwise_frontier(term, width))
                .collect(),
        ),
        Expr::Mul(terms) => Expr::Mul(
            terms
                .into_iter()
                .map(|term| absorb_bitwise_frontier(term, width))
                .collect(),
        ),
    };
    expression.reduce(make_mask(width))
}

#[derive(Clone, Copy, Debug, Default)]
struct ProofTimings {
    p7e_discovery: Duration,
    relative_tz_analysis: Duration,
    normalization: Duration,
    final_simplification: Duration,
    total: Duration,
}

impl ProofTimings {
    fn add_assign(&mut self, other: Self) {
        self.p7e_discovery += other.p7e_discovery;
        self.relative_tz_analysis += other.relative_tz_analysis;
        self.normalization += other.normalization;
        self.final_simplification += other.final_simplification;
        self.total += other.total;
    }
}

#[derive(Debug)]
struct CaseProof {
    zero: bool,
    candidate: Option<BinaryBitwiseDependencyCandidate>,
    timings: ProofTimings,
}

#[derive(Clone, Debug, Default)]
pub struct P8bScopeMetrics {
    pub p7e_candidates_observed: usize,
    pub p8b_attempts: usize,
    pub p8b_proved: usize,
    pub p8b_unknown: usize,
    pub p8b_budget_exceeded: usize,
    pub relative_tz_queries: usize,
    pub above_lowbit_proved: usize,
    pub masked_negations_normalized: usize,
    pub candidate_generation: Duration,
    pub relative_tz: Duration,
    pub normalization: Duration,
    pub final_simplification: Duration,
}

#[derive(Clone, Debug)]
pub struct P8bScopeAnalysis {
    pub result: Expr,
    pub compact_residual: Expr,
    pub changed: bool,
    pub dependencies_proved: usize,
    pub dependencies: Vec<P8bDependency>,
    pub all_certified_dependencies_exact: bool,
    pub unknown_causes_no_rewrite: bool,
    pub metrics: P8bScopeMetrics,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P8bDirectAnalysis {
    pub result: Expr,
    pub changed: bool,
    pub unknown_causes_no_rewrite: bool,
}

#[derive(Clone, Debug)]
pub struct P8bDependency {
    pub target: VarId,
    pub candidate: Expr,
}

fn replacement_map(
    p7e_dependencies: &[CertifiedBitwiseDependency],
    p8b_dependencies: &[P8bDependency],
) -> HashMap<VarId, Expr> {
    p7e_dependencies
        .iter()
        .map(|dependency| (dependency.target(), dependency.candidate().clone()))
        .chain(
            p8b_dependencies
                .iter()
                .map(|dependency| (dependency.target, dependency.candidate.clone())),
        )
        .collect()
}

fn apply_replacements(mut expression: Expr, replacements: &HashMap<VarId, Expr>) -> Expr {
    let mut replacements: Vec<_> = replacements.iter().collect();
    replacements.sort_by_key(|(target, _)| std::cmp::Reverse(**target));
    for (target, replacement) in replacements {
        expression = expression.replace_var(*target, replacement);
    }
    expression
}

fn expand_hidden_atoms(
    expression: Expr,
    atoms: &HashMap<VarId, &HiddenAtomTrace>,
    replacements: &HashMap<VarId, Expr>,
    active: &mut Vec<VarId>,
) -> Expr {
    let expression = apply_replacements(expression, replacements);
    match expression {
        Expr::Var(variable) if atoms.contains_key(&variable) && !active.contains(&variable) => {
            active.push(variable);
            let expanded = expand_hidden_atoms(
                atoms[&variable].simplified.clone(),
                atoms,
                replacements,
                active,
            );
            active.pop();
            expanded
        }
        Expr::Var(_) | Expr::Const(_) => expression,
        Expr::Not(inner) => !expand_hidden_atoms(*inner, atoms, replacements, active),
        Expr::Scale(coefficient, inner) => {
            coefficient * expand_hidden_atoms(*inner, atoms, replacements, active)
        }
        Expr::And(terms) => Expr::And(
            terms
                .into_iter()
                .map(|term| expand_hidden_atoms(term, atoms, replacements, active))
                .collect(),
        ),
        Expr::Or(terms) => Expr::Or(
            terms
                .into_iter()
                .map(|term| expand_hidden_atoms(term, atoms, replacements, active))
                .collect(),
        ),
        Expr::Xor(terms) => Expr::Xor(
            terms
                .into_iter()
                .map(|term| expand_hidden_atoms(term, atoms, replacements, active))
                .collect(),
        ),
        Expr::Add(terms) => Expr::Add(
            terms
                .into_iter()
                .map(|term| expand_hidden_atoms(term, atoms, replacements, active))
                .collect(),
        ),
        Expr::Mul(terms) => Expr::Mul(
            terms
                .into_iter()
                .map(|term| expand_hidden_atoms(term, atoms, replacements, active))
                .collect(),
        ),
    }
}

fn contains_masked_arithmetic_negation(
    expression: &Expr,
    anchors: &[Anchor],
    width: u8,
) -> bool {
    match expression {
        Expr::And(terms) if terms.len() == 2 => {
            anchors.iter().any(|anchor| {
                anchor.width == width
                    && (is_arithmetic_negation(&terms[0], &anchor.expression, width)
                        || is_arithmetic_negation(&terms[1], &anchor.expression, width))
            }) || terms
                .iter()
                .any(|term| contains_masked_arithmetic_negation(term, anchors, width))
        }
        Expr::Not(inner) | Expr::Scale(_, inner) => {
            contains_masked_arithmetic_negation(inner, anchors, width)
        }
        Expr::And(terms)
        | Expr::Or(terms)
        | Expr::Xor(terms)
        | Expr::Add(terms)
        | Expr::Mul(terms) => terms
            .iter()
            .any(|term| contains_masked_arithmetic_negation(term, anchors, width)),
        Expr::Var(_) | Expr::Const(_) => false,
    }
}

fn accumulate_normalization_metrics(
    metrics: &mut P8bScopeMetrics,
    timings: NormalizationTimings,
) {
    metrics.relative_tz += timings.relative_tz_analysis;
    metrics.normalization += timings.normalization;
    metrics.relative_tz_queries += timings.relative_tz_queries;
    metrics.above_lowbit_proved += timings.above_lowbit_proved;
    metrics.masked_negations_normalized += timings.masked_negations_normalized;
}

/// Run P8b only on one residual that remained non-zero after diagnostic P7e.
/// Failed candidates never enter `discovered`, so Unknown is strictly
/// non-rewriting and no dependency can participate in its own proof round.
pub fn analyze_scope_after_p7e(
    scope: &HiddenScopeTrace,
    p7e: &BitwiseDependencyClosureExperiment,
) -> P8bScopeAnalysis {
    const MAX_PARENT_WORDS: usize = 8;
    let Some(pre_restore) = &scope.pre_restore_result else {
        return P8bScopeAnalysis {
            result: p7e.simplified_after_substitution.clone(),
            compact_residual: p7e.substituted_pre_restore.clone(),
            changed: false,
            dependencies_proved: 0,
            dependencies: Vec::new(),
            all_certified_dependencies_exact: true,
            unknown_causes_no_rewrite: true,
            metrics: P8bScopeMetrics::default(),
        };
    };
    let atoms: HashMap<_, _> = scope.atoms.iter().map(|atom| (atom.atom, atom)).collect();
    let hidden: std::collections::HashSet<_> = atoms.keys().copied().collect();
    let mut discovered = Vec::<P8bDependency>::new();
    let mut metrics = P8bScopeMetrics::default();

    for _round in 0..MAX_PASSES {
        let replacements = replacement_map(&p7e.dependencies, &discovered);
        let current_pre_restore = apply_replacements(pre_restore.clone(), &replacements);
        let mut targets: Vec<_> = current_pre_restore
            .get_vars()
            .into_iter()
            .filter(|variable| hidden.contains(variable))
            .filter(|variable| !replacements.contains_key(variable))
            .collect();
        targets.sort();
        if targets.is_empty() {
            break;
        }

        let mut round_discovered = Vec::new();
        for target in targets.iter().copied() {
            let definition = atoms[&target]
                .dependency_definition
                .clone()
                .unwrap_or_else(|| atoms[&target].simplified.clone());
            let expanded_target = expand_hidden_atoms(
                definition,
                &atoms,
                &replacements,
                &mut Vec::new(),
            )
            .reduce(make_mask(scope.bit_width));

            let mut parents = Vec::<(VarId, Expr)>::new();
            for parent in targets.iter().copied().filter(|parent| parent.0 < target.0) {
                let expanded = expand_hidden_atoms(
                    Expr::Var(parent),
                    &atoms,
                    &replacements,
                    &mut Vec::new(),
                )
                .reduce(make_mask(scope.bit_width));
                if !parents.iter().any(|(_, existing)| *existing == expanded) {
                    parents.push((parent, expanded));
                }
            }
            let mut original_variables: Vec<_> = current_pre_restore
                .get_vars()
                .into_iter()
                .filter(|variable| !hidden.contains(variable))
                .collect();
            original_variables.sort();
            for variable in original_variables {
                parents.push((variable, Expr::Var(variable)));
            }
            if parents.len() > MAX_PARENT_WORDS {
                metrics.p8b_budget_exceeded += 1;
                parents.truncate(MAX_PARENT_WORDS);
            }

            'pairs: for left in 0..parents.len() {
                for right in left + 1..parents.len() {
                    let generation_started = Instant::now();
                    let candidates = discover_binary_bitwise_dependency_candidates(
                        &expanded_target,
                        &parents[left].1,
                        &parents[right].1,
                        scope.bit_width,
                    );
                    metrics.candidate_generation += generation_started.elapsed();
                    metrics.p7e_candidates_observed += candidates.len();

                    for candidate in candidates {
                        let differential = collect_conjunctive_arithmetic(
                            expanded_target.clone() - candidate.expression.clone(),
                            scope.bit_width,
                        );
                        let mut anchor_variables: Vec<_> = differential
                            .get_vars()
                            .into_iter()
                            .collect();
                        anchor_variables.sort();
                        if differential.size() > MAX_NODES
                            || anchor_variables.len() > MAX_ANCHORS
                        {
                            metrics.p8b_budget_exceeded += 1;
                            continue;
                        }
                        let anchors: Vec<_> = anchor_variables
                            .into_iter()
                            .map(|variable| Anchor {
                                expression: Expr::Var(variable),
                                width: scope.bit_width,
                            })
                            .collect();
                        if !contains_masked_arithmetic_negation(
                            &differential,
                            &anchors,
                            scope.bit_width,
                        ) {
                            continue;
                        }
                        metrics.p8b_attempts += 1;
                        let (normalized, changed, timings) = normalize_masked_negations(
                            differential,
                            scope.bit_width,
                            &anchors,
                        );
                        accumulate_normalization_metrics(&mut metrics, timings);
                        if !changed || normalized != Expr::zero() {
                            metrics.p8b_unknown += 1;
                            continue;
                        }

                        let compact = replace_subexpressions(
                            candidate.expression,
                            (&parents[left].1, &Expr::Var(parents[left].0)),
                            (&parents[right].1, &Expr::Var(parents[right].0)),
                        )
                        .reduce(make_mask(scope.bit_width));
                        round_discovered.push(P8bDependency {
                            target,
                            candidate: compact,
                        });
                        metrics.p8b_proved += 1;
                        break 'pairs;
                    }
                }
            }
        }
        if round_discovered.is_empty() {
            break;
        }
        round_discovered.sort_by_key(|dependency| dependency.target);
        discovered.extend(round_discovered);
    }

    if discovered.is_empty() {
        return P8bScopeAnalysis {
            result: p7e.simplified_after_substitution.clone(),
            compact_residual: p7e.substituted_pre_restore.clone(),
            changed: false,
            dependencies_proved: 0,
            dependencies: Vec::new(),
            all_certified_dependencies_exact: true,
            unknown_causes_no_rewrite: true,
            metrics,
        };
    }

    let replacements = replacement_map(&p7e.dependencies, &discovered);
    let substituted = apply_replacements(pre_restore.clone(), &replacements);
    let frontier = absorb_bitwise_frontier(substituted, scope.bit_width);
    let restored = expand_hidden_atoms(
        frontier.clone(),
        &atoms,
        &replacements,
        &mut Vec::new(),
    );
    let final_started = Instant::now();
    let result = simplify_mba(restored, scope.bit_width)
        .unwrap_or_else(|_| p7e.simplified_after_substitution.clone());
    metrics.final_simplification += final_started.elapsed();
    P8bScopeAnalysis {
        result,
        compact_residual: frontier,
        changed: true,
        dependencies_proved: discovered.len(),
        dependencies: discovered,
        all_certified_dependencies_exact: true,
        unknown_causes_no_rewrite: true,
        metrics,
    }
}

/// Restore a compact residual only after all P8b/P8a compact rewrites have
/// completed. This prevents P8a from losing the certified P8b dependencies to
/// an early definition expansion.
pub fn restore_compact_after_p8b(
    scope: &HiddenScopeTrace,
    p7e: &BitwiseDependencyClosureExperiment,
    p8b_dependencies: &[P8bDependency],
    compact_residual: Expr,
) -> Expr {
    let atoms: HashMap<_, _> = scope.atoms.iter().map(|atom| (atom.atom, atom)).collect();
    let replacements = replacement_map(&p7e.dependencies, p8b_dependencies);
    let restored = expand_hidden_atoms(
        compact_residual,
        &atoms,
        &replacements,
        &mut Vec::new(),
    );
    simplify_mba(restored, scope.bit_width)
        .unwrap_or_else(|_| p7e.simplified_after_substitution.clone())
}

/// Order-control entry point: apply only P8b's certified masked-negation
/// normalization to an already rewritten residual. It performs no P7e
/// discovery and returns the input exactly when the trigger or proof is absent.
pub fn analyze_rewritten_residual(expression: Expr, width: u8) -> P8bDirectAnalysis {
    if expression.size() > MAX_NODES {
        return P8bDirectAnalysis {
            result: expression,
            changed: false,
            unknown_causes_no_rewrite: true,
        };
    }
    let mut variables: Vec<_> = expression.get_vars().into_iter().collect();
    variables.sort();
    if variables.len() > MAX_ANCHORS {
        return P8bDirectAnalysis {
            result: expression,
            changed: false,
            unknown_causes_no_rewrite: true,
        };
    }
    let anchors: Vec<_> = variables
        .into_iter()
        .map(|variable| Anchor {
            expression: Expr::Var(variable),
            width,
        })
        .collect();
    let collected = collect_conjunctive_arithmetic(expression.clone(), width);
    if !contains_masked_arithmetic_negation(&collected, &anchors, width) {
        return P8bDirectAnalysis {
            result: expression,
            changed: false,
            unknown_causes_no_rewrite: true,
        };
    }
    let (normalized, changed, _) = normalize_masked_negations(collected, width, &anchors);
    if !changed {
        return P8bDirectAnalysis {
            result: expression,
            changed: false,
            unknown_causes_no_rewrite: true,
        };
    }
    let result = simplify_mba(normalized, width).unwrap_or_else(|_| expression.clone());
    P8bDirectAnalysis {
        result,
        changed: true,
        unknown_causes_no_rewrite: true,
    }
}

fn certify_dependency_without_pct(
    target: &Expr,
    candidate: &Expr,
    anchor: &Expr,
    width: u8,
) -> (bool, NormalizationTimings) {
    let differential = collect_conjunctive_arithmetic(target.clone() - candidate.clone(), width);
    let (normalized, changed, timings) = normalize_masked_negations(
        differential.clone(),
        width,
        &[Anchor {
            expression: anchor.clone(),
            width,
        }],
    );
    (changed && normalized == Expr::zero(), timings)
}

fn prove_case(width: u8, shift: u8) -> CaseProof {
    let total_started = Instant::now();
    let x = Expr::Var(0.into());
    let parent_a = -x.clone();
    let parent_b = VarInt::from(1u64 << shift) * x.clone();
    let coefficient = 1u64 + (1u64 << shift);
    let target = VarInt::from(0u64.wrapping_sub(coefficient)) * x.clone()
        + (x.clone() & parent_b.clone());

    let discovery_started = Instant::now();
    let candidates = discover_binary_bitwise_dependency_candidates(
        &target,
        &parent_a,
        &parent_b,
        width,
    );
    let discovery_time = discovery_started.elapsed();

    let mut selected = None;
    let mut normalization_timings = NormalizationTimings::default();
    for candidate in candidates {
        let (certified, timings) =
            certify_dependency_without_pct(&target, &candidate.expression, &x, width);
        normalization_timings.relative_tz_analysis += timings.relative_tz_analysis;
        normalization_timings.normalization += timings.normalization;
        if certified {
            selected = Some(candidate);
            break;
        }
    }

    let final_started = Instant::now();
    let zero = selected.as_ref().is_some_and(|candidate| {
        let symbolic_a = Expr::Var(1.into());
        let symbolic_b = Expr::Var(2.into());
        let compact = replace_subexpressions(
            candidate.expression.clone(),
            (&parent_a, &symbolic_a),
            (&parent_b, &symbolic_b),
        );
        let pre_restore = compact.clone() - (symbolic_a & compact);
        absorb_bitwise_frontier(pre_restore, width) == Expr::zero()
    });
    let final_time = final_started.elapsed();

    CaseProof {
        zero,
        candidate: selected,
        timings: ProofTimings {
            p7e_discovery: discovery_time,
            relative_tz_analysis: normalization_timings.relative_tz_analysis,
            normalization: normalization_timings.normalization,
            final_simplification: final_time,
            total: total_started.elapsed(),
        },
    }
}

fn lowbit(value: u64, width: u8) -> u64 {
    let value = value & make_mask(width);
    value & value.wrapping_neg() & make_mask(width)
}

fn central_precondition(x: u64, word_mask: u64, width: u8) -> bool {
    let mask = make_mask(width);
    let doubled = lowbit(x, width).wrapping_mul(2) & mask;
    if doubled == 0 {
        word_mask & mask == 0
    } else {
        (word_mask & mask) % doubled == 0
    }
}

fn central_theorem_holds(x: u64, word_mask: u64, width: u8) -> bool {
    let mask = make_mask(width);
    let x = x & mask;
    let word_mask = word_mask & mask;
    (word_mask & x.wrapping_neg() & mask) == (word_mask & !x & mask)
}

fn validate_central_theorem_exhaustively() -> bool {
    for width in 1..=8 {
        let limit = 1u64 << width;
        for x in 0..limit {
            for word_mask in 0..limit {
                if central_precondition(x, word_mask, width)
                    && !central_theorem_holds(x, word_mask, width)
                {
                    return false;
                }
            }
        }
    }
    true
}

fn validate_central_theorem_randomly() -> bool {
    let mut state = 0x243f_6a88_85a3_08d3u64;
    let mut next = || {
        state ^= state << 7;
        state ^= state >> 9;
        state ^= state << 8;
        state
    };
    for width in [32, 64] {
        let mask = make_mask(width);
        for _ in 0..20_000 {
            let x = next() & mask;
            let doubled = lowbit(x, width).wrapping_mul(2) & mask;
            let word_mask = if doubled == 0 {
                0
            } else {
                next() & !(doubled - 1) & mask
            };
            if !central_precondition(x, word_mask, width)
                || !central_theorem_holds(x, word_mask, width)
            {
                return false;
            }
        }
    }
    true
}

fn rewrite_was_rejected(expression: Expr, node_width: u8, anchor: Anchor) -> bool {
    let (result, changed, _) = normalize_masked_negations(
        expression.clone(),
        node_width,
        &[anchor],
    );
    !changed && result == expression
}

fn negative_controls() -> (bool, bool, bool, bool) {
    let x = Expr::Var(0.into());
    let concrete_one = Expr::make_const(1);
    let concrete_at_lowbit = rewrite_was_rejected(
        concrete_one.clone() & (-concrete_one.clone()),
        4,
        Anchor {
            expression: concrete_one,
            width: 4,
        },
    );
    let symbolic_mask_equals_anchor = rewrite_was_rejected(
        x.clone() & (-x.clone()),
        4,
        Anchor {
            expression: x.clone(),
            width: 4,
        },
    );
    let at_lowbit = concrete_at_lowbit && symbolic_mask_equals_anchor;
    let unknown_mask = rewrite_was_rejected(
        (!x.clone()) & (-x.clone()),
        4,
        Anchor {
            expression: x.clone(),
            width: 4,
        },
    );
    let width_mismatch = rewrite_was_rejected(
        (VarInt::from(2u64) * x.clone()) & (-x.clone()),
        8,
        Anchor {
            expression: x,
            width: 4,
        },
    );
    let overflow_msb = WIDTHS.into_iter().all(|width| {
        let msb = Expr::make_const(1u64 << (width - 1));
        let zero_mask = Expr::zero() & (-msb.clone());
        let (zero_result, zero_changed, _) = normalize_masked_negations(
            zero_mask,
            width,
            &[Anchor {
                expression: msb.clone(),
                width,
            }],
        );
        let nonzero_rejected = rewrite_was_rejected(
            msb.clone() & (-msb.clone()),
            width,
            Anchor {
                expression: msb,
                width,
            },
        );
        zero_changed && zero_result == Expr::zero() && nonzero_rejected
    });
    (at_lowbit, unknown_mask, width_mismatch, overflow_msb)
}

#[derive(Debug)]
struct DiagnosticReport {
    candidate: BinaryBitwiseDependencyCandidate,
    width_results: Vec<(u8, bool)>,
    generalized_results: Vec<(u8, bool)>,
    central_theorem_exhaustive: bool,
    random_32_64_passed: bool,
    at_lowbit_rejected: bool,
    unknown_mask_rejected: bool,
    width_mismatch_rejected: bool,
    overflow_msb_passed: bool,
    timings: ProofTimings,
}

fn run_diagnostic() -> Result<DiagnosticReport, String> {
    let _warmup = prove_case(64, 1);
    let mut timings = ProofTimings::default();
    let mut candidate = None;
    let width_results = WIDTHS
        .into_iter()
        .map(|width| {
            let proof = prove_case(width, 1);
            if width == 64 {
                candidate = proof.candidate.clone();
            }
            timings.add_assign(proof.timings);
            (width, proof.zero)
        })
        .collect();
    let generalized_results = (1..=3)
        .map(|shift| {
            let zero = WIDTHS.into_iter().all(|width| {
                let proof = prove_case(width, shift);
                timings.add_assign(proof.timings);
                proof.zero
            });
            (shift, zero)
        })
        .collect();
    let (at_lowbit_rejected, unknown_mask_rejected, width_mismatch_rejected, overflow_msb_passed) =
        negative_controls();
    Ok(DiagnosticReport {
        candidate: candidate.ok_or_else(|| "no width-64 candidate was certified".to_owned())?,
        width_results,
        generalized_results,
        central_theorem_exhaustive: validate_central_theorem_exhaustively(),
        random_32_64_passed: validate_central_theorem_randomly(),
        at_lowbit_rejected,
        unknown_mask_rejected,
        width_mismatch_rejected,
        overflow_msb_passed,
        timings,
    })
}

fn main() -> Result<(), String> {
    let report = run_diagnostic()?;
    println!("candidate_truth_table={:#06b}", report.candidate.truth_table);
    println!("candidate_expression={}", report.candidate.expression);
    println!("dependency_certified_without_pct=true");
    println!("pct_called=false");
    println!("smt_called=false");
    println!();
    for (width, zero) in &report.width_results {
        println!("width_{width}_zero={zero}");
    }
    println!();
    for (shift, zero) in &report.generalized_results {
        println!("generalized_r{shift}_zero={zero}");
    }
    println!();
    println!(
        "central_theorem_exhaustive={}",
        report.central_theorem_exhaustive
    );
    println!("random_32_64_passed={}", report.random_32_64_passed);
    println!();
    println!("at_lowbit_rejected={}", report.at_lowbit_rejected);
    println!("unknown_mask_rejected={}", report.unknown_mask_rejected);
    println!(
        "width_mismatch_rejected={}",
        report.width_mismatch_rejected
    );
    println!("overflow_msb_passed={}", report.overflow_msb_passed);
    println!();
    println!(
        "p7e_discovery_us={}",
        report.timings.p7e_discovery.as_micros()
    );
    println!(
        "relative_tz_analysis_us={}",
        report.timings.relative_tz_analysis.as_micros()
    );
    println!("normalization_us={}", report.timings.normalization.as_micros());
    println!(
        "final_simplification_us={}",
        report.timings.final_simplification.as_micros()
    );
    println!("total_proof_us={}", report.timings.total.as_micros());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certifies_target_and_generalized_family_without_pct_or_smt() {
        let report = run_diagnostic().unwrap();
        assert!(report.width_results.iter().all(|(_, zero)| *zero));
        assert!(report.generalized_results.iter().all(|(_, zero)| *zero));
        assert!(report.central_theorem_exhaustive);
        assert!(report.random_32_64_passed);
        assert!(report.at_lowbit_rejected);
        assert!(report.unknown_mask_rejected);
        assert!(report.width_mismatch_rejected);
        assert!(report.overflow_msb_passed);
    }

    #[test]
    fn relative_tz_transfers_match_the_requested_lattice() {
        let x = Expr::Var(0.into());
        let mut analyzer = RelativeTzAnalyzer::default();
        assert_eq!(
            analyzer.analyze(&x, &Expr::zero(), 64),
            RelativeTz::AboveLowBit
        );
        assert_eq!(
            analyzer.analyze(&x, &x, 64),
            RelativeTz::AtLeastLowBit
        );
        assert_eq!(
            analyzer.analyze(&x, &(-x.clone()), 64),
            RelativeTz::AtLeastLowBit
        );
        assert_eq!(
            analyzer.analyze(&x, &(VarInt::from(2u64) * x.clone()), 64),
            RelativeTz::AboveLowBit
        );
        assert_eq!(
            analyzer.analyze(&x, &(!x.clone()), 64),
            RelativeTz::Unknown
        );
    }

    #[test]
    fn unknown_keeps_the_original_expression() {
        let x = Expr::Var(0.into());
        let original = (!x.clone()) & (-x.clone());
        assert!(rewrite_was_rejected(
            original,
            64,
            Anchor {
                expression: x,
                width: 64,
            },
        ));
    }
}
