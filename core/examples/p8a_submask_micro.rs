use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use rumba_core::{
    expr::{Expr, VarId},
    simplify::simplify_mba,
    varint::{VarInt, make_mask},
};

const WIDTHS: [u8; 5] = [4, 8, 16, 32, 64];
const MAX_SATURATION_ROUNDS: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum WordTerm {
    Root(VarId),
    Constant(u64),
    Pow2Scale { shift: u8, inner: Box<WordTerm> },
    Or(Box<WordTerm>, Box<WordTerm>),
    Neg(Box<WordTerm>),
}

impl WordTerm {
    fn root(id: usize) -> Self {
        Self::Root(id.into())
    }

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
            Self::Pow2Scale { shift, inner } => {
                VarInt::from(1u64 << shift) * inner.to_expr()
            }
            Self::Or(left, right) => left.to_expr() | right.to_expr(),
            Self::Neg(inner) => -inner.to_expr(),
        }
    }

    fn collect_subterms(&self, output: &mut HashSet<Self>) {
        if !output.insert(self.clone()) {
            return;
        }
        match self {
            Self::Pow2Scale { inner, .. } | Self::Neg(inner) => {
                inner.collect_subterms(output);
            }
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
            let masked = value & make_mask(width);
            let count = if masked == 0 {
                width
            } else {
                (masked.trailing_zeros() as u8).min(width)
            };
            Some(ValuationExpr::Constant(count))
        }
        WordTerm::Pow2Scale { shift, inner } => Some(ValuationExpr::Shift {
            inner: Box::new(valuation(inner, width)?),
            amount: *shift,
        }),
        WordTerm::Or(left, right) => Some(ValuationExpr::Min(
            Box::new(valuation(left, width)?),
            Box::new(valuation(right, width)?),
        )),
        // No valuation rule for negation is part of this micro-domain.
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

    fn visit_assignments(
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
            if !visit_assignments(index + 1, roots, width, left, right) {
                return false;
            }
        }
        true
    }

    visit_assignments(0, &mut roots, width, &left, &right)
}

#[derive(Debug)]
struct SubmaskDomain {
    width: u8,
    terms: HashSet<WordTerm>,
    facts: HashSet<(WordTerm, WordTerm)>,
    rounds: usize,
    valuation_queries: usize,
    valuation_proved: usize,
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
            rounds: 0,
            valuation_queries: 0,
            valuation_proved: 0,
        }
    }

    fn seed_submask(&mut self, left: WordTerm, right: WordTerm) {
        left.collect_subterms(&mut self.terms);
        right.collect_subterms(&mut self.terms);
        self.facts.insert((left, right));
    }

    fn seed_concrete_submask(&mut self, left: WordTerm, right: WordTerm) {
        let (WordTerm::Constant(a), WordTerm::Constant(b)) = (&left, &right) else {
            return;
        };
        let mask = make_mask(self.width);
        if ((a & mask) & (b & mask)) == (a & mask) {
            self.seed_submask(left, right);
        }
    }

    fn saturate(&mut self) {
        for round in 0..MAX_SATURATION_ROUNDS {
            let mut inferred = HashSet::new();

            // Rules 1 and 2: both operands are submasks of their OR.
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
            // Rule 5: reverse the submask order under negation only when the
            // truncated valuations are equal.
            for (left, right) in facts {
                self.valuation_queries += 1;
                if valuations_equal(&left, &right, self.width) {
                    self.valuation_proved += 1;
                    inferred.insert((WordTerm::neg(right), WordTerm::neg(left)));
                }
            }

            let before = self.facts.len();
            self.facts.extend(inferred);
            self.rounds = round + 1;
            if self.facts.len() == before {
                break;
            }
        }
    }

    fn proves_submask(&self, left: &WordTerm, right: &WordTerm) -> bool {
        self.facts.contains(&(left.clone(), right.clone()))
    }

    // Rule 6. Failure returns None and therefore performs no rewrite.
    fn simplify_and(&self, left: &WordTerm, right: &WordTerm) -> Option<WordTerm> {
        if self.proves_submask(left, right) {
            Some(left.clone())
        } else if self.proves_submask(right, left) {
            Some(right.clone())
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct P8aMetrics {
    pub submask_queries: usize,
    pub submask_proved: usize,
    pub valuation_queries: usize,
    pub valuation_proved: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P8aAnalysis {
    pub result: Expr,
    pub changed: bool,
    pub unknown: bool,
    pub metrics: P8aMetrics,
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
            collect_direct_word_terms(inner, width, output);
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
    const SAMPLE_COUNT: usize = 12;
    let mask = make_mask(width);
    let max_var = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .map(|variable| variable.0)
        .max()
        .unwrap_or(0);
    for sample in 0..SAMPLE_COUNT {
        let values = (0..=max_var)
            .map(|variable| match sample {
                0 => 0,
                1 => mask,
                2 => 1,
                3 => mask.wrapping_sub(1),
                _ => {
                    let seed = (sample as u64)
                        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
                        .wrapping_add((variable as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9));
                    let mut value = seed;
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

/// Use the existing exact simplifier only as a normal-form certificate. The
/// candidate space is generic: ORs (and their arithmetic negations) of direct
/// word terms already present below the operand.
fn normalize_word_operand(expression: &Expr, width: u8) -> Option<WordTerm> {
    if let Some(term) = parse_word_term(expression, width) {
        return Some(term);
    }

    const MAX_BASE_TERMS: usize = 8;
    let mut bases = HashSet::new();
    collect_direct_word_terms(expression, width, &mut bases);
    let mut bases: Vec<_> = bases.into_iter().collect();
    bases.sort_by_key(|term| (term.to_expr().size(), term.clone()));
    bases.truncate(MAX_BASE_TERMS);

    for left_index in 0..bases.len() {
        for right_index in left_index + 1..bases.len() {
            let union = WordTerm::or(
                bases[left_index].clone(),
                bases[right_index].clone(),
            );
            for candidate in [union.clone(), WordTerm::neg(union)] {
                let candidate_expression = candidate.to_expr();
                if !observations_match(expression, &candidate_expression, width) {
                    continue;
                }
                let relation = expression.clone() - candidate_expression;
                if simplify_mba(relation, width) == Ok(Expr::zero()) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn rewrite_submask_ands(
    expression: Expr,
    width: u8,
    metrics: &mut P8aMetrics,
) -> (Expr, bool) {
    let rewrite_children = |terms: Vec<Expr>, metrics: &mut P8aMetrics| {
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
                metrics.submask_queries += 1;
                if let (Some(left), Some(right)) = (
                    normalize_word_operand(&terms[0], width),
                    normalize_word_operand(&terms[1], width),
                ) {
                    let mut domain = SubmaskDomain::new(width, [left.clone(), right.clone()]);
                    domain.saturate();
                    metrics.valuation_queries += domain.valuation_queries;
                    metrics.valuation_proved += domain.valuation_proved;
                    if let Some(simplified) = domain.simplify_and(&left, &right) {
                        metrics.submask_proved += 1;
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

/// Diagnostic-only P8a-lite entry point. On any failed or non-beneficial
/// analysis it returns the input unchanged and marks the result Unknown.
pub fn analyze_expression(expression: Expr, width: u8) -> P8aAnalysis {
    let original = expression;
    let mut metrics = P8aMetrics::default();
    let (rewritten, changed) = rewrite_submask_ands(original.clone(), width, &mut metrics);
    if !changed {
        return P8aAnalysis {
            result: original,
            changed: false,
            unknown: true,
            metrics,
        };
    }
    let Ok(simplified) = simplify_mba(rewritten, width) else {
        return P8aAnalysis {
            result: original,
            changed: false,
            unknown: true,
            metrics,
        };
    };
    if simplified != Expr::zero() && simplified.size() >= original.size() {
        return P8aAnalysis {
            result: original,
            changed: false,
            unknown: true,
            metrics,
        };
    }
    P8aAnalysis {
        result: simplified,
        changed: true,
        unknown: false,
        metrics,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ProofOutcome {
    Zero,
    Unknown(Expr),
}

fn prove_family_member(width: u8, shift: u8) -> ProofOutcome {
    let x = WordTerm::root(0);
    let scaled = WordTerm::pow2_scale(shift, x.clone());
    let union = WordTerm::or(x.clone(), scaled.clone());
    let neg_x = WordTerm::neg(x.clone());
    let neg_union = WordTerm::neg(union.clone());

    let coefficient = 1u64 + (1u64 << shift);
    let intersection = x.to_expr() & scaled.to_expr();
    let h = VarInt::from(0u64.wrapping_sub(coefficient)) * x.to_expr() + intersection;
    let original = h.clone() - (neg_x.to_expr() & h.clone());

    // Existing RUMBA normalization proves the generic OR identity. If it
    // cannot, this diagnostic must leave the original expression untouched.
    if simplify_mba(h - neg_union.to_expr(), width) != Ok(Expr::zero()) {
        return ProofOutcome::Unknown(original);
    }

    let mut domain = SubmaskDomain::new(width, [union.clone()]);
    domain.saturate();
    let required_facts_hold = domain.proves_submask(&x, &union)
        && valuations_equal(&x, &union, width)
        && domain.proves_submask(&neg_union, &neg_x)
        && domain.rounds <= MAX_SATURATION_ROUNDS;
    if !required_facts_hold {
        return ProofOutcome::Unknown(original);
    }

    let Some(simplified_and) = domain.simplify_and(&neg_x, &neg_union) else {
        return ProofOutcome::Unknown(original);
    };
    let rewritten = neg_union.to_expr() - simplified_and.to_expr();
    if simplify_mba(rewritten, width) == Ok(Expr::zero()) {
        ProofOutcome::Zero
    } else {
        ProofOutcome::Unknown(original)
    }
}

fn unequal_valuation_is_rejected() -> bool {
    let width = 4;
    let a = WordTerm::Constant(0b0010);
    let b = WordTerm::Constant(0b0011);
    let mut domain = SubmaskDomain::new(width, [a.clone(), b.clone()]);
    domain.seed_concrete_submask(a.clone(), b.clone());
    domain.saturate();
    domain.proves_submask(&a, &b)
        && !valuations_equal(&a, &b, width)
        && !domain.proves_submask(&WordTerm::neg(b), &WordTerm::neg(a))
}

fn missing_submask_is_rejected() -> bool {
    let width = 4;
    let a = WordTerm::Constant(0b0101);
    let b = WordTerm::Constant(0b0011);
    let mut domain = SubmaskDomain::new(width, [a.clone(), b.clone()]);
    domain.seed_concrete_submask(a.clone(), b.clone());
    domain.saturate();
    valuations_equal(&a, &b, width)
        && !domain.proves_submask(&a, &b)
        && !domain.proves_submask(&WordTerm::neg(b), &WordTerm::neg(a))
}

fn overflow_test_passes() -> bool {
    WIDTHS.into_iter().all(|width| {
        let mask = make_mask(width);
        let x = 1u64 << (width - 1);
        x.wrapping_mul(2) & mask == 0
    })
}

#[derive(Debug)]
struct DiagnosticReport {
    width_results: Vec<(u8, bool)>,
    generalized_results: Vec<(u8, bool)>,
    unequal_valuation_rejected: bool,
    missing_submask_rejected: bool,
    overflow_test_passed: bool,
    proof_time: Duration,
}

fn run_diagnostic() -> DiagnosticReport {
    let started = Instant::now();
    let width_results = WIDTHS
        .into_iter()
        .map(|width| (width, prove_family_member(width, 1) == ProofOutcome::Zero))
        .collect();
    let generalized_results = (1..=3)
        .map(|shift| {
            (
                shift,
                WIDTHS
                    .into_iter()
                    .all(|width| prove_family_member(width, shift) == ProofOutcome::Zero),
            )
        })
        .collect();
    DiagnosticReport {
        width_results,
        generalized_results,
        unequal_valuation_rejected: unequal_valuation_is_rejected(),
        missing_submask_rejected: missing_submask_is_rejected(),
        overflow_test_passed: overflow_test_passes(),
        proof_time: started.elapsed(),
    }
}

fn main() {
    let report = run_diagnostic();
    for (width, zero) in &report.width_results {
        println!("width_{width}_zero={zero}");
    }
    println!();
    for (shift, zero) in &report.generalized_results {
        println!("generalized_r{shift}_zero={zero}");
    }
    println!();
    println!(
        "unequal_valuation_rejected={}",
        report.unequal_valuation_rejected
    );
    println!(
        "missing_submask_rejected={}",
        report.missing_submask_rejected
    );
    println!("overflow_test_passed={}", report.overflow_test_passed);
    println!();
    println!("smt_called=false");
    println!("proof_time_us={}", report.proof_time.as_micros());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concrete_valuation(value: u64, width: u8) -> u8 {
        let value = value & make_mask(width);
        if value == 0 {
            width
        } else {
            (value.trailing_zeros() as u8).min(width)
        }
    }

    fn is_submask(left: u64, right: u64, width: u8) -> bool {
        let mask = make_mask(width);
        ((left & mask) & (right & mask)) == (left & mask)
    }

    fn family_expression(shift: u8) -> Expr {
        let x = Expr::Var(0.into());
        let scaled = VarInt::from(1u64 << shift) * x.clone();
        let intersection = x.clone() & scaled;
        let coefficient = 1u64 + (1u64 << shift);
        let h = VarInt::from(0u64.wrapping_sub(coefficient)) * x.clone() + intersection;
        h.clone() - ((-x) & h)
    }

    #[test]
    fn proves_requested_widths_and_generalized_family_without_smt() {
        let report = run_diagnostic();
        assert!(report.width_results.iter().all(|(_, zero)| *zero));
        assert!(report.generalized_results.iter().all(|(_, zero)| *zero));
        assert!(report.unequal_valuation_rejected);
        assert!(report.missing_submask_rejected);
        assert!(report.overflow_test_passed);
    }

    #[test]
    fn p8a_lite_analyzer_proves_the_micro_family() {
        for width in WIDTHS {
            for shift in 1..=3 {
                let analysis = analyze_expression(family_expression(shift), width);
                assert_eq!(analysis.result, Expr::zero(), "width={width} r={shift}");
                assert!(analysis.changed, "width={width} r={shift}");
                assert!(!analysis.unknown, "width={width} r={shift}");
            }
        }
    }

    #[test]
    fn transfer_functions_are_exhaustive_at_widths_one_through_eight() {
        for width in 1..=8 {
            let limit = 1u64 << width;
            let mask = make_mask(width);
            for a in 0..limit {
                for shift in 0..=width + 1 {
                    let scaled = a.wrapping_shl(u32::from(shift)) & mask;
                    let transfer = valuation(
                        &WordTerm::pow2_scale(shift, WordTerm::Constant(a)),
                        width,
                    )
                    .unwrap()
                    .evaluate(&[], width);
                    assert_eq!(
                        transfer,
                        width.min(concrete_valuation(a, width).saturating_add(shift)),
                        "scale width={width} a={a:#x} shift={shift}",
                    );
                    assert_eq!(transfer, concrete_valuation(scaled, width));
                }
                for b in 0..limit {
                    let union = a | b;
                    let a_term = WordTerm::Constant(a);
                    let b_term = WordTerm::Constant(b);
                    let union_term = WordTerm::or(a_term.clone(), b_term.clone());
                    let transfer = valuation(&union_term, width)
                        .unwrap()
                        .evaluate(&[], width);
                    assert_eq!(
                        transfer,
                        concrete_valuation(a, width).min(concrete_valuation(b, width)),
                        "or width={width} a={a:#x} b={b:#x}",
                    );
                    assert_eq!(transfer, concrete_valuation(union, width));

                    let mut or_domain = SubmaskDomain::new(width, [union_term.clone()]);
                    or_domain.saturate();
                    assert!(or_domain.proves_submask(&a_term, &union_term));
                    assert!(or_domain.proves_submask(&b_term, &union_term));
                    assert_eq!(
                        or_domain.simplify_and(&a_term, &union_term),
                        Some(a_term.clone()),
                    );
                    assert_eq!(
                        or_domain.simplify_and(&b_term, &union_term),
                        Some(b_term.clone()),
                    );

                    if is_submask(a, b, width)
                        && concrete_valuation(a, width) == concrete_valuation(b, width)
                    {
                        let neg_b = 0u64.wrapping_sub(b) & mask;
                        let neg_a = 0u64.wrapping_sub(a) & mask;
                        let mut negation_domain =
                            SubmaskDomain::new(width, [a_term.clone(), b_term.clone()]);
                        negation_domain.seed_submask(a_term.clone(), b_term.clone());
                        negation_domain.saturate();
                        assert!(negation_domain.proves_submask(
                            &WordTerm::neg(b_term.clone()),
                            &WordTerm::neg(a_term.clone()),
                        ));
                        assert!(
                            is_submask(neg_b, neg_a, width),
                            "negation width={width} a={a:#x} b={b:#x}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn transfer_functions_hold_for_pseudorandom_32_and_64_bit_words() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            state ^= state << 7;
            state ^= state >> 9;
            state ^= state << 8;
            state
        };
        for width in [32, 64] {
            let mask = make_mask(width);
            for _ in 0..20_000 {
                let a = next() & mask;
                let b = next() & mask;
                let shift = (next() % (u64::from(width) + 2)) as u8;
                let scaled = if shift >= 64 {
                    0
                } else {
                    a.wrapping_shl(u32::from(shift)) & mask
                };
                assert_eq!(
                    concrete_valuation(a | b, width),
                    concrete_valuation(a, width).min(concrete_valuation(b, width)),
                );
                assert_eq!(
                    concrete_valuation(scaled, width),
                    width.min(concrete_valuation(a, width).saturating_add(shift)),
                );
                if is_submask(a, b, width)
                    && concrete_valuation(a, width) == concrete_valuation(b, width)
                {
                    assert!(is_submask(
                        0u64.wrapping_sub(b) & mask,
                        0u64.wrapping_sub(a) & mask,
                        width,
                    ));
                }
            }
        }
    }

    #[test]
    fn returns_unknown_without_rewriting_when_a_required_fact_is_missing() {
        let x = WordTerm::root(0);
        let union = WordTerm::or(x.clone(), WordTerm::pow2_scale(1, x));
        let neg_x = WordTerm::neg(WordTerm::root(0));
        let neg_union = WordTerm::neg(union.clone());
        let domain = SubmaskDomain::new(8, [union]);
        assert_eq!(domain.simplify_and(&neg_x, &neg_union), None);
    }
}
