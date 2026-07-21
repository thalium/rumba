//! Experimental P8 pipeline.
//!
//! This module deliberately stays outside [`crate::simplify::simplify_mba`].
//! It packages the measured `P7e -> P8a -> P8b -> P8c` order so corpus
//! diagnostics and future opt-in callers exercise one implementation instead
//! of importing the historical examples.

use std::collections::{HashMap, HashSet};

use crate::{
    expr::{Expr, VarId},
    simplify::{
        BitwiseDependencyClosureExperiment, HiddenScopeTrace, SolveError,
        experiment_bitwise_dependency_closure, simplify_mba,
    },
    varint::make_mask,
};

const MAX_PASSES: usize = 2;
const MAX_ANCHORS: usize = 4;
const MAX_NODES: usize = 512;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct P8PassMetrics {
    pub anchor_candidates: usize,
    pub anchors_recognized: usize,
    pub above_lowbit_queries: usize,
    pub above_lowbit_proved: usize,
    pub rewrites: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P8PassResult {
    pub result: Expr,
    pub changed: bool,
    pub unknown_causes_no_rewrite: bool,
    pub metrics: P8PassMetrics,
}

impl P8PassResult {
    fn unchanged(expression: Expr, metrics: P8PassMetrics) -> Self {
        Self {
            result: expression,
            changed: false,
            unknown_causes_no_rewrite: true,
            metrics,
        }
    }
}

#[derive(Clone, Debug)]
pub struct P8PipelineExperiment {
    pub p7e: BitwiseDependencyClosureExperiment,
    pub input_after_p7e: Expr,
    pub after_p8a: P8PassResult,
    pub after_p8b: P8PassResult,
    pub after_p8c: P8PassResult,
    pub result: Expr,
    pub residual_zero: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
enum RelativeDivisibility {
    #[default]
    Unknown,
    AtLeastLowBit,
    AboveLowBit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AboveLowResult {
    Unknown,
    AtLeastLowBit,
    AboveLowBit,
}

#[derive(Default)]
struct DivisibilityAnalyzer {
    memo: HashMap<(Expr, Expr), RelativeDivisibility>,
    queries: usize,
    proved: usize,
}

fn is_arithmetic_negation(left: &Expr, right: &Expr, width: u8) -> bool {
    (left.clone() + right.clone()).reduce(make_mask(width)) == Expr::zero()
}

impl DivisibilityAnalyzer {
    fn analyze(&mut self, anchor: &Expr, expression: &Expr, width: u8) -> RelativeDivisibility {
        self.queries += 1;
        let mask = make_mask(width);
        let anchor = anchor.clone().reduce(mask);
        let expression = expression.clone().reduce(mask);
        let key = (anchor.clone(), expression.clone());
        if let Some(result) = self.memo.get(&key) {
            return *result;
        }
        self.memo.insert(key.clone(), RelativeDivisibility::Unknown);
        let result = if expression == Expr::zero() {
            RelativeDivisibility::AboveLowBit
        } else if expression == anchor || is_arithmetic_negation(&expression, &anchor, width) {
            RelativeDivisibility::AtLeastLowBit
        } else {
            match &expression {
                Expr::Scale(coefficient, inner) => {
                    let inner = self.analyze(&anchor, inner, width);
                    if coefficient.get(mask) & 1 == 0
                        && inner >= RelativeDivisibility::AtLeastLowBit
                    {
                        RelativeDivisibility::AboveLowBit
                    } else {
                        inner
                    }
                }
                Expr::Add(terms) | Expr::Or(terms) | Expr::Xor(terms) => terms
                    .iter()
                    .map(|term| self.analyze(&anchor, term, width))
                    .min()
                    .unwrap_or(RelativeDivisibility::AboveLowBit),
                Expr::And(terms) | Expr::Mul(terms) => terms
                    .iter()
                    .map(|term| self.analyze(&anchor, term, width))
                    .max()
                    .unwrap_or(RelativeDivisibility::Unknown),
                Expr::Var(_) | Expr::Const(_) | Expr::Not(_) => RelativeDivisibility::Unknown,
            }
        };
        if result == RelativeDivisibility::AboveLowBit {
            self.proved += 1;
        }
        self.memo.insert(key, result);
        result
    }
}

/// Query the exact AnchorsOnly abstraction without applying a rewrite.
/// This is exposed for causal diagnostics such as `p8_remaining_blockers`.
pub fn classify_above_lowbit(anchor: &Expr, expression: &Expr, width: u8) -> AboveLowResult {
    match DivisibilityAnalyzer::default().analyze(anchor, expression, width) {
        RelativeDivisibility::Unknown => AboveLowResult::Unknown,
        RelativeDivisibility::AtLeastLowBit => AboveLowResult::AtLeastLowBit,
        RelativeDivisibility::AboveLowBit => AboveLowResult::AboveLowBit,
    }
}

// --- P8a: submask absorption -------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum WordTerm {
    Root(VarId),
    Constant(u64),
    Pow2Scale { shift: u8, inner: Box<WordTerm> },
    Or(Box<WordTerm>, Box<WordTerm>),
    Neg(Box<WordTerm>),
}

impl WordTerm {
    fn pow2_scale(shift: u8, inner: Self) -> Self {
        Self::Pow2Scale {
            shift,
            inner: Box::new(inner),
        }
    }

    fn or(left: Self, right: Self) -> Self {
        if left <= right {
            Self::Or(Box::new(left), Box::new(right))
        } else {
            Self::Or(Box::new(right), Box::new(left))
        }
    }

    fn neg(inner: Self) -> Self {
        Self::Neg(Box::new(inner))
    }

    fn to_expr(&self) -> Expr {
        match self {
            Self::Root(variable) => Expr::Var(*variable),
            Self::Constant(value) => Expr::make_const(*value),
            Self::Pow2Scale { shift, inner } => (1u64 << shift) * inner.to_expr(),
            Self::Or(left, right) => left.to_expr() | right.to_expr(),
            Self::Neg(inner) => -inner.to_expr(),
        }
    }

    fn collect_subterms(&self, output: &mut HashSet<Self>) {
        if !output.insert(self.clone()) {
            return;
        }
        match self {
            Self::Pow2Scale { inner, .. } | Self::Neg(inner) => inner.collect_subterms(output),
            Self::Or(left, right) => {
                left.collect_subterms(output);
                right.collect_subterms(output);
            }
            Self::Root(_) | Self::Constant(_) => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ValuationExpr {
    Root(VarId),
    Constant(u8),
    Shift { inner: Box<Self>, amount: u8 },
    Min(Box<Self>, Box<Self>),
}

impl ValuationExpr {
    fn evaluate(&self, roots: &[u8], width: u8) -> u8 {
        match self {
            Self::Root(variable) => roots[variable.0],
            Self::Constant(value) => *value,
            Self::Shift { inner, amount } => {
                width.min(inner.evaluate(roots, width).saturating_add(*amount))
            }
            Self::Min(left, right) => left
                .evaluate(roots, width)
                .min(right.evaluate(roots, width)),
        }
    }

    fn max_root(&self) -> Option<usize> {
        match self {
            Self::Root(variable) => Some(variable.0),
            Self::Constant(_) => None,
            Self::Shift { inner, .. } => inner.max_root(),
            Self::Min(left, right) => left.max_root().max(right.max_root()),
        }
    }
}

fn valuation(term: &WordTerm, width: u8) -> Option<ValuationExpr> {
    match term {
        WordTerm::Root(variable) => Some(ValuationExpr::Root(*variable)),
        WordTerm::Constant(value) => {
            let value = value & make_mask(width);
            Some(ValuationExpr::Constant(if value == 0 {
                width
            } else {
                (value.trailing_zeros() as u8).min(width)
            }))
        }
        WordTerm::Pow2Scale { shift, inner } => Some(ValuationExpr::Shift {
            inner: Box::new(valuation(inner, width)?),
            amount: *shift,
        }),
        WordTerm::Or(left, right) => Some(ValuationExpr::Min(
            Box::new(valuation(left, width)?),
            Box::new(valuation(right, width)?),
        )),
        WordTerm::Neg(_) => None,
    }
}

fn valuations_equal(left: &WordTerm, right: &WordTerm, width: u8) -> bool {
    let (Some(left), Some(right)) = (valuation(left, width), valuation(right, width)) else {
        return false;
    };
    let root_count = left
        .max_root()
        .max(right.max_root())
        .map_or(0, |root| root + 1);
    let mut roots = vec![0; root_count];
    fn visit(
        index: usize,
        roots: &mut [u8],
        width: u8,
        left: &ValuationExpr,
        right: &ValuationExpr,
    ) -> bool {
        if index == roots.len() {
            return left.evaluate(roots, width) == right.evaluate(roots, width);
        }
        for value in 0..=width {
            roots[index] = value;
            if !visit(index + 1, roots, width, left, right) {
                return false;
            }
        }
        true
    }
    visit(0, &mut roots, width, &left, &right)
}

struct SubmaskDomain {
    width: u8,
    terms: HashSet<WordTerm>,
    facts: HashSet<(WordTerm, WordTerm)>,
}

impl SubmaskDomain {
    fn new(width: u8, roots: impl IntoIterator<Item = WordTerm>) -> Self {
        let mut terms = HashSet::new();
        for root in roots {
            root.collect_subterms(&mut terms);
        }
        Self {
            width,
            terms,
            facts: HashSet::new(),
        }
    }

    fn saturate(&mut self) {
        for _ in 0..MAX_PASSES {
            let mut inferred = HashSet::new();
            for term in &self.terms {
                if let WordTerm::Or(left, right) = term {
                    inferred.insert(((**left).clone(), term.clone()));
                    inferred.insert(((**right).clone(), term.clone()));
                }
            }
            let facts = self
                .facts
                .iter()
                .chain(inferred.iter())
                .cloned()
                .collect::<Vec<_>>();
            for (left, right) in facts {
                if valuations_equal(&left, &right, self.width) {
                    inferred.insert((WordTerm::neg(right), WordTerm::neg(left)));
                }
            }
            let before = self.facts.len();
            self.facts.extend(inferred);
            if self.facts.len() == before {
                break;
            }
        }
    }

    fn simplify_and(&self, left: &WordTerm, right: &WordTerm) -> Option<WordTerm> {
        if self.facts.contains(&(left.clone(), right.clone())) {
            Some(left.clone())
        } else if self.facts.contains(&(right.clone(), left.clone())) {
            Some(right.clone())
        } else {
            None
        }
    }
}

fn parse_word_term(expression: &Expr, width: u8) -> Option<WordTerm> {
    let mask = make_mask(width);
    match expression {
        Expr::Var(variable) => Some(WordTerm::Root(*variable)),
        Expr::Const(value) => Some(WordTerm::Constant(value.get(mask))),
        Expr::Scale(coefficient, inner) => {
            let coefficient = coefficient.get(mask);
            let inner = parse_word_term(inner, width)?;
            if coefficient == 1 {
                Some(inner)
            } else if coefficient == mask {
                Some(WordTerm::neg(inner))
            } else if coefficient.is_power_of_two() {
                Some(WordTerm::pow2_scale(
                    coefficient.trailing_zeros() as u8,
                    inner,
                ))
            } else {
                None
            }
        }
        Expr::Or(terms) => {
            let mut terms = terms.iter().map(|term| parse_word_term(term, width));
            let mut result = terms.next()??;
            for term in terms {
                result = WordTerm::or(result, term?);
            }
            Some(result)
        }
        Expr::Not(_) | Expr::And(_) | Expr::Xor(_) | Expr::Add(_) | Expr::Mul(_) => None,
    }
}

fn collect_direct_word_terms(expression: &Expr, width: u8, output: &mut HashSet<WordTerm>) {
    if let Some(term) = parse_word_term(expression, width) {
        output.insert(term);
    }
    match expression {
        Expr::Not(inner) | Expr::Scale(_, inner) => {
            collect_direct_word_terms(inner, width, output)
        }
        Expr::And(terms)
        | Expr::Or(terms)
        | Expr::Xor(terms)
        | Expr::Add(terms)
        | Expr::Mul(terms) => {
            for term in terms {
                collect_direct_word_terms(term, width, output);
            }
        }
        Expr::Var(_) | Expr::Const(_) => {}
    }
}

fn observations_match(left: &Expr, right: &Expr, width: u8) -> bool {
    let mask = make_mask(width);
    let max_var = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .map(|variable| variable.0)
        .max()
        .unwrap_or(0);
    for sample in 0..12usize {
        let values = (0..=max_var)
            .map(|variable| match sample {
                0 => 0,
                1 => mask,
                2 => 1,
                3 => mask.wrapping_sub(1),
                _ => {
                    let mut value = (sample as u64)
                        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
                        .wrapping_add((variable as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9));
                    value ^= value >> 30;
                    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
                    value ^= value >> 27;
                    value.wrapping_mul(0x94d0_49bb_1331_11eb) & mask
                }
            })
            .collect::<Vec<_>>();
        if left.eval(&values).get(mask) != right.eval(&values).get(mask) {
            return false;
        }
    }
    true
}

fn normalize_word_operand(expression: &Expr, width: u8) -> Option<WordTerm> {
    if let Some(term) = parse_word_term(expression, width) {
        return Some(term);
    }
    let mut bases = HashSet::new();
    collect_direct_word_terms(expression, width, &mut bases);
    let mut bases = bases.into_iter().collect::<Vec<_>>();
    bases.sort_by_key(|term| (term.to_expr().size(), term.clone()));
    bases.truncate(8);
    for left in 0..bases.len() {
        for right in left + 1..bases.len() {
            let union = WordTerm::or(bases[left].clone(), bases[right].clone());
            for candidate in [union.clone(), WordTerm::neg(union)] {
                let candidate_expression = candidate.to_expr();
                if observations_match(expression, &candidate_expression, width)
                    && simplify_mba(expression.clone() - candidate_expression, width)
                        == Ok(Expr::zero())
                {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn rewrite_submask_ands(expression: Expr, width: u8, metrics: &mut P8PassMetrics) -> (Expr, bool) {
    let rewrite_children = |terms: Vec<Expr>, metrics: &mut P8PassMetrics| {
        let mut changed = false;
        let terms: Vec<_> = terms
            .into_iter()
            .map(|term| {
                let (term, term_changed) = rewrite_submask_ands(term, width, metrics);
                changed |= term_changed;
                term
            })
            .collect();
        (terms, changed)
    };
    match expression {
        Expr::Var(_) | Expr::Const(_) => (expression, false),
        Expr::Not(inner) => {
            let (inner, changed) = rewrite_submask_ands(*inner, width, metrics);
            (!inner, changed)
        }
        Expr::Scale(coefficient, inner) => {
            let (inner, changed) = rewrite_submask_ands(*inner, width, metrics);
            (coefficient * inner, changed)
        }
        Expr::And(terms) => {
            let (terms, child_changed) = rewrite_children(terms, metrics);
            if terms.len() == 2 {
                metrics.anchor_candidates += 1;
                if let (Some(left), Some(right)) = (
                    normalize_word_operand(&terms[0], width),
                    normalize_word_operand(&terms[1], width),
                ) {
                    let mut domain = SubmaskDomain::new(width, [left.clone(), right.clone()]);
                    domain.saturate();
                    if let Some(simplified) = domain.simplify_and(&left, &right) {
                        metrics.anchors_recognized += 1;
                        metrics.rewrites += 1;
                        return (simplified.to_expr(), true);
                    }
                }
            }
            (Expr::And(terms), child_changed)
        }
        Expr::Or(terms) => {
            let (terms, changed) = rewrite_children(terms, metrics);
            (Expr::Or(terms), changed)
        }
        Expr::Xor(terms) => {
            let (terms, changed) = rewrite_children(terms, metrics);
            (Expr::Xor(terms), changed)
        }
        Expr::Add(terms) => {
            let (terms, changed) = rewrite_children(terms, metrics);
            (Expr::Add(terms), changed)
        }
        Expr::Mul(terms) => {
            let (terms, changed) = rewrite_children(terms, metrics);
            (Expr::Mul(terms), changed)
        }
    }
}

pub fn analyze_submask(expression: Expr, width: u8) -> P8PassResult {
    let original = expression;
    let mut metrics = P8PassMetrics::default();
    let (rewritten, changed) = rewrite_submask_ands(original.clone(), width, &mut metrics);
    if !changed {
        return P8PassResult::unchanged(original, metrics);
    }
    let Ok(simplified) = simplify_mba(rewritten, width) else {
        return P8PassResult::unchanged(original, metrics);
    };
    if simplified != Expr::zero() && simplified.size() >= original.size() {
        return P8PassResult::unchanged(original, metrics);
    }
    P8PassResult {
        result: simplified,
        changed: true,
        unknown_causes_no_rewrite: true,
        metrics,
    }
}

// --- P8b: terminal masked-negation normalization ----------------------------

fn collect_conjunctive_arithmetic(expression: Expr, width: u8) -> Expr {
    let expression = match expression {
        Expr::Var(_) | Expr::Const(_) => expression,
        Expr::Not(inner) => !collect_conjunctive_arithmetic(*inner, width),
        Expr::Scale(coefficient, inner) => {
            coefficient * collect_conjunctive_arithmetic(*inner, width)
        }
        Expr::And(terms) => {
            let terms = terms
                .into_iter()
                .map(|term| collect_conjunctive_arithmetic(term, width))
                .collect::<Vec<_>>();
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
    expression.reduce(make_mask(width))
}

fn contains_masked_negation(expression: &Expr, anchors: &[Expr], width: u8) -> bool {
    match expression {
        Expr::And(terms) if terms.len() == 2 => {
            anchors.iter().any(|anchor| {
                is_arithmetic_negation(&terms[0], anchor, width)
                    || is_arithmetic_negation(&terms[1], anchor, width)
            }) || terms
                .iter()
                .any(|term| contains_masked_negation(term, anchors, width))
        }
        Expr::Not(inner) | Expr::Scale(_, inner) => {
            contains_masked_negation(inner, anchors, width)
        }
        Expr::And(terms)
        | Expr::Or(terms)
        | Expr::Xor(terms)
        | Expr::Add(terms)
        | Expr::Mul(terms) => terms
            .iter()
            .any(|term| contains_masked_negation(term, anchors, width)),
        Expr::Var(_) | Expr::Const(_) => false,
    }
}

fn normalize_masked_negation_once(
    expression: Expr,
    width: u8,
    anchors: &[Expr],
    analyzer: &mut DivisibilityAnalyzer,
    metrics: &mut P8PassMetrics,
) -> (Expr, bool) {
    let rewrite_children = |terms: Vec<Expr>,
                            analyzer: &mut DivisibilityAnalyzer,
                            metrics: &mut P8PassMetrics| {
        let mut changed = false;
        let terms: Vec<_> = terms
            .into_iter()
            .map(|term| {
                let (term, term_changed) =
                    normalize_masked_negation_once(term, width, anchors, analyzer, metrics);
                changed |= term_changed;
                term
            })
            .collect();
        (terms, changed)
    };
    match expression {
        Expr::Var(_) | Expr::Const(_) => (expression, false),
        Expr::Not(inner) => {
            let (inner, changed) =
                normalize_masked_negation_once(*inner, width, anchors, analyzer, metrics);
            (!inner, changed)
        }
        Expr::Scale(coefficient, inner) => {
            let (inner, changed) =
                normalize_masked_negation_once(*inner, width, anchors, analyzer, metrics);
            (coefficient * inner, changed)
        }
        Expr::And(terms) => {
            let (terms, child_changed) = rewrite_children(terms, analyzer, metrics);
            if terms.len() == 2 {
                for anchor in anchors {
                    let word_mask = if is_arithmetic_negation(&terms[0], anchor, width) {
                        Some(&terms[1])
                    } else if is_arithmetic_negation(&terms[1], anchor, width) {
                        Some(&terms[0])
                    } else {
                        None
                    };
                    let Some(word_mask) = word_mask else {
                        continue;
                    };
                    metrics.above_lowbit_queries += 1;
                    if analyzer.analyze(anchor, word_mask, width)
                        != RelativeDivisibility::AboveLowBit
                    {
                        continue;
                    }
                    metrics.above_lowbit_proved += 1;
                    metrics.rewrites += 1;
                    return (
                        (word_mask.clone() - (word_mask.clone() & anchor.clone()))
                            .reduce(make_mask(width)),
                        true,
                    );
                }
            }
            (Expr::And(terms), child_changed)
        }
        Expr::Or(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, metrics);
            (Expr::Or(terms), changed)
        }
        Expr::Xor(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, metrics);
            (Expr::Xor(terms), changed)
        }
        Expr::Add(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, metrics);
            (Expr::Add(terms), changed)
        }
        Expr::Mul(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, metrics);
            (Expr::Mul(terms), changed)
        }
    }
}

pub fn normalize_masked_negations(expression: Expr, width: u8) -> P8PassResult {
    let original = expression;
    if original.size() > MAX_NODES {
        return P8PassResult::unchanged(original, P8PassMetrics::default());
    }
    let mut variables = original.get_vars().into_iter().collect::<Vec<_>>();
    variables.sort();
    if variables.len() > MAX_ANCHORS {
        return P8PassResult::unchanged(original, P8PassMetrics::default());
    }
    let anchors = variables.into_iter().map(Expr::Var).collect::<Vec<_>>();
    let collected = collect_conjunctive_arithmetic(original.clone(), width);
    if !contains_masked_negation(&collected, &anchors, width) {
        return P8PassResult::unchanged(original, P8PassMetrics::default());
    }
    let mut analyzer = DivisibilityAnalyzer::default();
    let mut metrics = P8PassMetrics::default();
    let mut current = collected;
    let mut changed = false;
    for _ in 0..MAX_PASSES {
        let (next, pass_changed) = normalize_masked_negation_once(
            current,
            width,
            &anchors,
            &mut analyzer,
            &mut metrics,
        );
        current = next.reduce(make_mask(width));
        changed |= pass_changed;
        if !pass_changed {
            break;
        }
    }
    metrics.above_lowbit_queries += analyzer.queries;
    metrics.above_lowbit_proved += analyzer.proved;
    if !changed {
        return P8PassResult::unchanged(original, metrics);
    }
    let Ok(result) = simplify_mba(current, width) else {
        return P8PassResult::unchanged(original, metrics);
    };
    if result != Expr::zero() && result.size() >= original.size() {
        return P8PassResult::unchanged(original, metrics);
    }
    P8PassResult {
        result,
        changed: true,
        unknown_causes_no_rewrite: true,
        metrics,
    }
}

// --- P8c: LowBit anchor + AboveLow selection --------------------------------

fn conjunction(mut terms: Vec<Expr>) -> Expr {
    match terms.len() {
        0 => Expr::make_const(u64::MAX),
        1 => terms.pop().unwrap(),
        _ => Expr::And(terms),
    }
}

fn rewrite_lowbit_above(
    expression: Expr,
    width: u8,
    analyzer: &mut DivisibilityAnalyzer,
    metrics: &mut P8PassMetrics,
) -> (Expr, bool) {
    let rewrite_children = |terms: Vec<Expr>,
                            analyzer: &mut DivisibilityAnalyzer,
                            metrics: &mut P8PassMetrics| {
        let mut changed = false;
        let terms: Vec<_> = terms
            .into_iter()
            .map(|term| {
                let (term, term_changed) = rewrite_lowbit_above(term, width, analyzer, metrics);
                changed |= term_changed;
                term
            })
            .collect();
        (terms, changed)
    };
    match expression {
        Expr::Var(_) | Expr::Const(_) => (expression, false),
        Expr::Not(inner) => {
            let (inner, changed) = rewrite_lowbit_above(*inner, width, analyzer, metrics);
            (!inner, changed)
        }
        Expr::Scale(coefficient, inner) => {
            let (inner, changed) = rewrite_lowbit_above(*inner, width, analyzer, metrics);
            (coefficient * inner, changed)
        }
        Expr::And(terms) => {
            let (terms, child_changed) = rewrite_children(terms, analyzer, metrics);
            for left in 0..terms.len() {
                for right in left + 1..terms.len() {
                    metrics.anchor_candidates += 1;
                    if !is_arithmetic_negation(&terms[left], &terms[right], width) {
                        continue;
                    }
                    metrics.anchors_recognized += 1;
                    let remaining = terms
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| *index != left && *index != right)
                        .map(|(_, term)| term.clone())
                        .collect::<Vec<_>>();
                    if remaining.is_empty() {
                        continue;
                    }
                    metrics.above_lowbit_queries += 1;
                    if analyzer.analyze(
                        &terms[left],
                        &conjunction(remaining),
                        width,
                    ) == RelativeDivisibility::AboveLowBit
                    {
                        metrics.above_lowbit_proved += 1;
                        metrics.rewrites += 1;
                        return (Expr::zero(), true);
                    }
                }
            }
            (Expr::And(terms), child_changed)
        }
        Expr::Or(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, metrics);
            (Expr::Or(terms), changed)
        }
        Expr::Xor(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, metrics);
            (Expr::Xor(terms), changed)
        }
        Expr::Add(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, metrics);
            (Expr::Add(terms), changed)
        }
        Expr::Mul(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer, metrics);
            (Expr::Mul(terms), changed)
        }
    }
}

pub fn select_lowbit_above(expression: Expr, width: u8) -> P8PassResult {
    let original = expression;
    if original.size() > MAX_NODES {
        return P8PassResult::unchanged(original, P8PassMetrics::default());
    }
    let mut analyzer = DivisibilityAnalyzer::default();
    let mut metrics = P8PassMetrics::default();
    let mut current = original.clone().reduce(make_mask(width));
    let mut changed = false;
    for _ in 0..MAX_PASSES {
        let (next, pass_changed) =
            rewrite_lowbit_above(current, width, &mut analyzer, &mut metrics);
        current = next.reduce(make_mask(width));
        changed |= pass_changed;
        if !pass_changed {
            break;
        }
    }
    metrics.above_lowbit_queries += analyzer.queries;
    metrics.above_lowbit_proved += analyzer.proved;
    if !changed {
        return P8PassResult::unchanged(original, metrics);
    }
    let Ok(result) = simplify_mba(current, width) else {
        return P8PassResult::unchanged(original, metrics);
    };
    if result != Expr::zero() && result.size() >= original.size() {
        return P8PassResult::unchanged(original, metrics);
    }
    P8PassResult {
        result,
        changed: true,
        unknown_causes_no_rewrite: true,
        metrics,
    }
}

/// Run the integrated diagnostic pipeline in its empirically selected order.
pub fn experiment_pipeline(
    scope: &HiddenScopeTrace,
) -> Result<Option<P8PipelineExperiment>, SolveError> {
    let Some(p7e) = experiment_bitwise_dependency_closure(scope)? else {
        return Ok(None);
    };
    let input_after_p7e = if p7e.residual_zero {
        p7e.simplified_after_substitution.clone()
    } else {
        // P8a needs the rich restored form, not only P7e's final normal form.
        p7e.restored_after_substitution.clone()
    };
    let after_p8a = if p7e.residual_zero {
        P8PassResult::unchanged(input_after_p7e.clone(), P8PassMetrics::default())
    } else {
        analyze_submask(input_after_p7e.clone(), scope.bit_width)
    };
    let after_p8b = normalize_masked_negations(after_p8a.result.clone(), scope.bit_width);
    let after_p8c = select_lowbit_above(after_p8b.result.clone(), scope.bit_width);
    let result = after_p8c.result.clone();
    Ok(Some(P8PipelineExperiment {
        p7e,
        input_after_p7e,
        after_p8a,
        after_p8b,
        after_p8c,
        residual_zero: result == Expr::zero(),
        result,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_only_selects_even_multiples_and_preserves_unknowns() {
        for width in [4, 8, 16, 32, 64] {
            let x = Expr::Var(0.into());
            let lowbit = x.clone() & -x.clone();
            assert_eq!(
                select_lowbit_above(lowbit.clone() & (2 * x.clone()), width).result,
                Expr::zero()
            );
            let unknown = lowbit & Expr::Var(1.into());
            let result = select_lowbit_above(unknown.clone(), width);
            assert_eq!(result.result, unknown);
            assert!(!result.changed);
        }
    }

    #[test]
    fn anchors_only_rewrites_are_exhaustively_exact_at_small_widths() {
        for width in 1..=8 {
            let mask = make_mask(width);
            let x = Expr::Var(0.into());
            let y = Expr::Var(1.into());
            let lowbit = x.clone() & -x.clone();
            let operands = [
                2 * x.clone(),
                4 * x.clone(),
                (2 * x.clone()) & y.clone(),
                (2 * x.clone()) | (4 * x.clone()),
                (2 * x.clone()) + (4 * x.clone()),
            ];
            for operand in operands {
                let original = lowbit.clone() & operand;
                let rewritten = select_lowbit_above(original.clone(), width).result;
                for x_value in 0..=mask {
                    for y_value in 0..=mask {
                        let values = [x_value, y_value];
                        assert_eq!(
                            original.eval(&values).get(mask),
                            rewritten.eval(&values).get(mask),
                            "width={width} x={x_value:#x} y={y_value:#x}"
                        );
                    }
                }
            }
        }
    }
}
