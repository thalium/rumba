use std::collections::HashMap;

use rumba_core::{
    expr::Expr,
    simplify::simplify_mba,
    varint::make_mask,
};

const MAX_NODES: usize = 512;
const MAX_PASSES: usize = 2;

/// `Known(bit)` means both that the expression is divisible by the anchor
/// LowBit and that its quotient by that LowBit has the given parity.
///
/// When the anchor is zero the quotient is degenerate, but either selection
/// result is still zero, so every rewrite below remains valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RelativeLeadBit {
    Known(bool),
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
enum RelativeDivisibility {
    #[default]
    Unknown,
    AtLeastLowBit,
    AboveLowBit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum P8cVariant {
    #[default]
    Full,
    ZeroOnly,
    AnchorsOnly,
}

#[derive(Clone, Debug)]
struct LowBitAnchor {
    lowbit: Expr,
    root: Expr,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct P8cMetrics {
    pub anchor_candidates: usize,
    pub anchors_recognized: usize,
    pub relative_lead_queries: usize,
    pub known_zero: usize,
    pub known_one: usize,
    pub selections_zero: usize,
    pub selections_one: usize,
    pub lowbit_selections: usize,
    pub unknown_results: usize,
    pub unknown_due_to_product: usize,
    pub unknown_due_to_not: usize,
    pub unknown_due_to_variable: usize,
    pub unknown_due_to_constant: usize,
    pub derived_zero_from_one: usize,
    pub zero_by_even_scale: usize,
    pub zero_by_existing_above: usize,
    pub zero_by_one_xor_one: usize,
    pub zero_by_one_plus_one: usize,
    pub zero_by_one_minus_one: usize,
    pub zero_by_and: usize,
    pub zero_by_other: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P8cAnalysis {
    pub result: Expr,
    pub changed: bool,
    pub unknown: bool,
    pub metrics: P8cMetrics,
}

#[derive(Default)]
struct RelativeLeadAnalyzer {
    memo: HashMap<(Expr, Expr), RelativeLeadBit>,
    divisibility_memo: HashMap<(Expr, Expr), RelativeDivisibility>,
    variant: P8cVariant,
    metrics: P8cMetrics,
}

fn is_zero(expression: Expr, width: u8) -> bool {
    expression.reduce(make_mask(width)) == Expr::zero()
}

fn is_arithmetic_negation(left: &Expr, right: &Expr, width: u8) -> bool {
    is_zero(left.clone() + right.clone(), width)
}

fn conjunction(mut terms: Vec<Expr>) -> Expr {
    match terms.len() {
        0 => Expr::make_const(u64::MAX),
        1 => terms.pop().unwrap(),
        _ => Expr::And(terms),
    }
}

fn canonical_anchor(left: &Expr, right: &Expr, width: u8) -> LowBitAnchor {
    let mask = make_mask(width);
    let left = left.clone().reduce(mask);
    let right = right.clone().reduce(mask);
    let root = if left <= right { left } else { right };
    LowBitAnchor {
        lowbit: Expr::And(vec![root.clone(), (-root.clone()).reduce(mask)]).reduce(mask),
        root,
    }
}

fn find_anchor_pairs(
    terms: &[Expr],
    width: u8,
    metrics: &mut P8cMetrics,
) -> Vec<(usize, usize, LowBitAnchor)> {
    let mut anchors = Vec::new();
    for left in 0..terms.len() {
        for right in left + 1..terms.len() {
            metrics.anchor_candidates += 1;
            if !is_arithmetic_negation(&terms[left], &terms[right], width) {
                continue;
            }
            metrics.anchors_recognized += 1;
            anchors.push((
                left,
                right,
                canonical_anchor(&terms[left], &terms[right], width),
            ));
        }
    }
    anchors
}

/// Cheap production-style trigger. It performs no cloning or rebuilding and
/// stops at the first structural `a & -a` pair. The full diagnostic is only
/// useful when such an anchor already occurs in the expression.
pub fn has_lowbit_trigger(expression: &Expr, width: u8) -> bool {
    match expression {
        Expr::And(terms) => {
            for left in 0..terms.len() {
                for right in left + 1..terms.len() {
                    if is_arithmetic_negation(&terms[left], &terms[right], width) {
                        return true;
                    }
                }
            }
            terms
                .iter()
                .any(|term| has_lowbit_trigger(term, width))
        }
        Expr::Not(inner) | Expr::Scale(_, inner) => has_lowbit_trigger(inner, width),
        Expr::Or(terms) | Expr::Xor(terms) | Expr::Add(terms) | Expr::Mul(terms) => terms
            .iter()
            .any(|term| has_lowbit_trigger(term, width)),
        Expr::Var(_) | Expr::Const(_) => false,
    }
}

impl RelativeLeadAnalyzer {
    fn analyze(&mut self, anchor: &LowBitAnchor, expression: &Expr, width: u8) -> RelativeLeadBit {
        self.metrics.relative_lead_queries += 1;
        let mask = make_mask(width);
        let expression = expression.clone().reduce(mask);
        let key = (anchor.root.clone(), expression.clone());
        if let Some(result) = self.memo.get(&key) {
            return *result;
        }

        if self.variant == P8cVariant::AnchorsOnly {
            let result = match self.analyze_divisibility(anchor, &expression, width) {
                RelativeDivisibility::AboveLowBit => {
                    self.metrics.zero_by_existing_above += 1;
                    RelativeLeadBit::Known(false)
                }
                RelativeDivisibility::Unknown | RelativeDivisibility::AtLeastLowBit => {
                    RelativeLeadBit::Unknown
                }
            };
            self.record_result(&key, result);
            return result;
        }

        // Insert Unknown before recursion. This is also a cheap cycle guard for
        // any future graph-backed representation of expressions.
        self.memo.insert(key.clone(), RelativeLeadBit::Unknown);
        let result = if expression == Expr::zero() {
            self.metrics.zero_by_existing_above += 1;
            RelativeLeadBit::Known(false)
        } else if expression == anchor.root
            || is_arithmetic_negation(&expression, &anchor.root, width)
        {
            RelativeLeadBit::Known(true)
        } else {
            match &expression {
                Expr::Scale(coefficient, inner) => {
                    match self.analyze(anchor, inner, width) {
                        RelativeLeadBit::Known(bit) if coefficient.get(mask) & 1 == 0 => {
                            self.metrics.zero_by_even_scale += 1;
                            if bit {
                                self.metrics.derived_zero_from_one += 1;
                            }
                            RelativeLeadBit::Known(false)
                        }
                        RelativeLeadBit::Known(bit) => RelativeLeadBit::Known(bit),
                        RelativeLeadBit::Unknown => RelativeLeadBit::Unknown,
                    }
                }
                Expr::Add(terms) => {
                    match self.known_bits(anchor, terms, width) {
                        Some(bits) => {
                            let result = bits
                                .iter()
                                .copied()
                                .fold(false, |left, right| left ^ right);
                            if !result && bits.iter().any(|bit| *bit) {
                                self.metrics.derived_zero_from_one += 1;
                                if terms.iter().any(|term| {
                                    matches!(term, Expr::Scale(coefficient, _) if coefficient.get(mask) == mask)
                                }) {
                                    self.metrics.zero_by_one_minus_one += 1;
                                } else {
                                    self.metrics.zero_by_one_plus_one += 1;
                                }
                            }
                            RelativeLeadBit::Known(result)
                        }
                        None => RelativeLeadBit::Unknown,
                    }
                }
                Expr::Xor(terms) => {
                    match self.known_bits(anchor, terms, width) {
                        Some(bits) => {
                            let result = bits
                                .iter()
                                .copied()
                                .fold(false, |left, right| left ^ right);
                            if !result && bits.iter().any(|bit| *bit) {
                                self.metrics.derived_zero_from_one += 1;
                                self.metrics.zero_by_one_xor_one += 1;
                            }
                            RelativeLeadBit::Known(result)
                        }
                        None => RelativeLeadBit::Unknown,
                    }
                }
                Expr::And(terms) => {
                    match self.known_bits(anchor, terms, width) {
                        Some(bits) => {
                            let result = bits
                                .iter()
                                .copied()
                                .fold(true, |left, right| left & right);
                            if !result {
                                self.metrics.zero_by_and += 1;
                                if bits.iter().any(|bit| *bit) {
                                    self.metrics.derived_zero_from_one += 1;
                                }
                            }
                            RelativeLeadBit::Known(result)
                        }
                        None => RelativeLeadBit::Unknown,
                    }
                }
                Expr::Or(terms) => {
                    match self.known_bits(anchor, terms, width) {
                        Some(bits) => {
                            let result = bits
                                .iter()
                                .copied()
                                .fold(false, |left, right| left | right);
                            if !result {
                                self.metrics.zero_by_other += 1;
                            }
                            RelativeLeadBit::Known(result)
                        }
                        None => RelativeLeadBit::Unknown,
                    }
                }
                Expr::Mul(_) => {
                    self.metrics.unknown_due_to_product += 1;
                    RelativeLeadBit::Unknown
                }
                Expr::Not(_) => {
                    self.metrics.unknown_due_to_not += 1;
                    RelativeLeadBit::Unknown
                }
                Expr::Var(_) => {
                    self.metrics.unknown_due_to_variable += 1;
                    RelativeLeadBit::Unknown
                }
                Expr::Const(_) => {
                    self.metrics.unknown_due_to_constant += 1;
                    RelativeLeadBit::Unknown
                }
            }
        };

        let mut result = if self.variant == P8cVariant::ZeroOnly
            && result == RelativeLeadBit::Known(true)
        {
            RelativeLeadBit::Unknown
        } else {
            result
        };
        // Preserve the older divisibility fact when the quotient bit is not
        // known. This is the `AtLeast/Above` component of the product domain;
        // it lets an AboveLow operand zero an AND even if another operand has
        // an unknown quotient class.
        if result == RelativeLeadBit::Unknown
            && self.analyze_divisibility(anchor, &expression, width)
                == RelativeDivisibility::AboveLowBit
        {
            self.metrics.zero_by_existing_above += 1;
            result = RelativeLeadBit::Known(false);
        }
        self.record_result(&key, result);
        result
    }

    fn record_result(&mut self, key: &(Expr, Expr), result: RelativeLeadBit) {
        match result {
            RelativeLeadBit::Known(false) => self.metrics.known_zero += 1,
            RelativeLeadBit::Known(true) => self.metrics.known_one += 1,
            RelativeLeadBit::Unknown => self.metrics.unknown_results += 1,
        }
        self.memo.insert(key.clone(), result);
    }

    fn known_bits(
        &mut self,
        anchor: &LowBitAnchor,
        terms: &[Expr],
        width: u8,
    ) -> Option<Vec<bool>> {
        let mut bits = Vec::with_capacity(terms.len());
        for term in terms {
            let RelativeLeadBit::Known(bit) = self.analyze(anchor, term, width) else {
                return None;
            };
            bits.push(bit);
        }
        Some(bits)
    }

    fn analyze_divisibility(
        &mut self,
        anchor: &LowBitAnchor,
        expression: &Expr,
        width: u8,
    ) -> RelativeDivisibility {
        let mask = make_mask(width);
        let expression = expression.clone().reduce(mask);
        let key = (anchor.root.clone(), expression.clone());
        if let Some(result) = self.divisibility_memo.get(&key) {
            return *result;
        }
        self.divisibility_memo
            .insert(key.clone(), RelativeDivisibility::Unknown);
        let result = if expression == Expr::zero() {
            RelativeDivisibility::AboveLowBit
        } else if expression == anchor.root
            || is_arithmetic_negation(&expression, &anchor.root, width)
        {
            RelativeDivisibility::AtLeastLowBit
        } else {
            match &expression {
                Expr::Scale(coefficient, inner) => {
                    let inner = self.analyze_divisibility(anchor, inner, width);
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
                    .map(|term| self.analyze_divisibility(anchor, term, width))
                    .min()
                    .unwrap_or(RelativeDivisibility::AboveLowBit),
                Expr::And(terms) | Expr::Mul(terms) => terms
                    .iter()
                    .map(|term| self.analyze_divisibility(anchor, term, width))
                    .max()
                    .unwrap_or(RelativeDivisibility::Unknown),
                Expr::Var(_) | Expr::Const(_) | Expr::Not(_) => RelativeDivisibility::Unknown,
            }
        };
        self.divisibility_memo.insert(key, result);
        result
    }
}

fn replace_indices(terms: &[Expr], indices: &[usize], replacement: Expr, width: u8) -> Expr {
    let mut result = terms
        .iter()
        .enumerate()
        .filter(|(index, _)| !indices.contains(index))
        .map(|(_, term)| term.clone())
        .collect::<Vec<_>>();
    result.push(replacement);
    conjunction(result).reduce(make_mask(width))
}

fn select_direct_operand(
    terms: &[Expr],
    anchor_indices: (usize, usize),
    anchor: &LowBitAnchor,
    width: u8,
    analyzer: &mut RelativeLeadAnalyzer,
) -> Option<(Expr, bool)> {
    let remaining = terms
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != anchor_indices.0 && *index != anchor_indices.1)
        .map(|(_, term)| term.clone())
        .collect::<Vec<_>>();
    if remaining.is_empty() {
        return None;
    }
    let operand = conjunction(remaining);
    let RelativeLeadBit::Known(bit) = analyzer.analyze(anchor, &operand, width) else {
        return None;
    };
    let replacement = if bit {
        analyzer.metrics.selections_one += 1;
        anchor.lowbit.clone()
    } else {
        analyzer.metrics.selections_zero += 1;
        Expr::zero()
    };
    Some((replacement, bit))
}

fn select_lowbit_operand(
    terms: &[Expr],
    anchor_indices: (usize, usize),
    anchor: &LowBitAnchor,
    width: u8,
    analyzer: &mut RelativeLeadAnalyzer,
) -> Option<Expr> {
    for left in 0..terms.len() {
        if left == anchor_indices.0 || left == anchor_indices.1 {
            continue;
        }
        for right in left + 1..terms.len() {
            if right == anchor_indices.0 || right == anchor_indices.1 {
                continue;
            }
            if !is_arithmetic_negation(&terms[left], &terms[right], width) {
                continue;
            }
            let other = canonical_anchor(&terms[left], &terms[right], width);
            let RelativeLeadBit::Known(bit) = analyzer.analyze(anchor, &other.root, width) else {
                continue;
            };
            analyzer.metrics.lowbit_selections += 1;
            if bit {
                analyzer.metrics.selections_one += 1;
                return Some(replace_indices(
                    terms,
                    &[anchor_indices.0, anchor_indices.1, left, right],
                    anchor.lowbit.clone(),
                    width,
                ));
            }
            analyzer.metrics.selections_zero += 1;
            return Some(Expr::zero());
        }
    }
    None
}

fn rewrite_once(
    expression: Expr,
    width: u8,
    analyzer: &mut RelativeLeadAnalyzer,
) -> (Expr, bool) {
    let rewrite_children = |terms: Vec<Expr>, analyzer: &mut RelativeLeadAnalyzer| {
        let mut changed = false;
        let terms = terms
            .into_iter()
            .map(|term| {
                let (term, term_changed) = rewrite_once(term, width, analyzer);
                changed |= term_changed;
                term
            })
            .collect::<Vec<_>>();
        (terms, changed)
    };

    match expression {
        Expr::Var(_) | Expr::Const(_) => (expression, false),
        Expr::Not(inner) => {
            let (inner, changed) = rewrite_once(*inner, width, analyzer);
            (!inner, changed)
        }
        Expr::Scale(coefficient, inner) => {
            let (inner, changed) = rewrite_once(*inner, width, analyzer);
            (coefficient * inner, changed)
        }
        Expr::And(terms) => {
            let (terms, child_changed) = rewrite_children(terms, analyzer);
            let anchors = find_anchor_pairs(&terms, width, &mut analyzer.metrics);
            for (left, right, anchor) in anchors {
                // Handle `p & LowBit(e)` before the generic conjunction. The
                // latter intentionally cannot infer divisibility of `e & -e`
                // when neither operand is separately divisible by p.
                if let Some(rewritten) = select_lowbit_operand(
                    &terms,
                    (left, right),
                    &anchor,
                    width,
                    analyzer,
                ) {
                    return (rewritten, true);
                }
                if let Some((replacement, _bit)) = select_direct_operand(
                    &terms,
                    (left, right),
                    &anchor,
                    width,
                    analyzer,
                ) {
                    return (replacement, true);
                }
            }
            (Expr::And(terms), child_changed)
        }
        Expr::Or(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer);
            (Expr::Or(terms), changed)
        }
        Expr::Xor(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer);
            (Expr::Xor(terms), changed)
        }
        Expr::Add(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer);
            (Expr::Add(terms), changed)
        }
        Expr::Mul(terms) => {
            let (terms, changed) = rewrite_children(terms, analyzer);
            (Expr::Mul(terms), changed)
        }
    }
}

/// Diagnostic-only P8c-lite. It is demand-driven by a LowBit anchor used as
/// an AND mask and returns the input unchanged on Unknown or a non-beneficial
/// result. It is not called by RUMBA's normal simplification path.
pub fn analyze_expression(expression: Expr, width: u8) -> P8cAnalysis {
    analyze_expression_with_variant(expression, width, P8cVariant::Full)
}

pub fn analyze_expression_with_variant(
    expression: Expr,
    width: u8,
    variant: P8cVariant,
) -> P8cAnalysis {
    let original = expression;
    if original.size() > MAX_NODES {
        return P8cAnalysis {
            result: original,
            changed: false,
            unknown: true,
            metrics: P8cMetrics::default(),
        };
    }

    let mut analyzer = RelativeLeadAnalyzer {
        variant,
        ..RelativeLeadAnalyzer::default()
    };
    // Expose lowbit pairs inside nested conjunctions. Expr's `&` constructor
    // preserves nesting, while the existing reducer provides the canonical
    // n-ary conjunction expected by the demand-driven anchor scan.
    let mut current = original.clone().reduce(make_mask(width));
    let mut changed = false;
    for _ in 0..MAX_PASSES {
        let (next, pass_changed) = rewrite_once(current, width, &mut analyzer);
        current = next.reduce(make_mask(width));
        changed |= pass_changed;
        if !pass_changed {
            break;
        }
    }
    if !changed {
        return P8cAnalysis {
            result: original,
            changed: false,
            unknown: true,
            metrics: analyzer.metrics,
        };
    }
    let Ok(simplified) = simplify_mba(current, width) else {
        return P8cAnalysis {
            result: original,
            changed: false,
            unknown: true,
            metrics: analyzer.metrics,
        };
    };
    if simplified != Expr::zero() && simplified.size() >= original.size() {
        return P8cAnalysis {
            result: original,
            changed: false,
            unknown: true,
            metrics: analyzer.metrics,
        };
    }
    P8cAnalysis {
        result: simplified,
        changed: true,
        unknown: false,
        metrics: analyzer.metrics,
    }
}

fn main() {
    let x = Expr::Var(0.into());
    let p = x.clone() & -x.clone();
    for coefficient in 1..=4u64 {
        let expression = simplify_mba(p.clone() & (coefficient * x.clone()), 64).unwrap();
        let result = analyze_expression(expression, 64);
        println!(
            "coefficient={coefficient} result={} changed={}",
            result.result, result.changed
        );
    }
    println!("smt_called=false");
    println!("normal_path_modified=false");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(coefficient: u64, width: u8) -> Expr {
        let x = Expr::Var(0.into());
        let p = x.clone() & -x.clone();
        let input = simplify_mba(p & (coefficient * x), width).unwrap();
        analyze_expression(input, width).result
    }

    #[test]
    fn selects_lowbit_for_odd_multiples_and_zero_for_even_multiples() {
        for width in [4, 8, 16, 32, 64] {
            let x = Expr::Var(0.into());
            let p = (x.clone() & -x).reduce(make_mask(width));
            assert_eq!(selected(1, width), p);
            assert_eq!(selected(2, width), Expr::zero());
            assert_eq!(selected(3, width), p);
            assert_eq!(selected(4, width), Expr::zero());
        }
    }

    #[test]
    fn transfers_add_xor_and_or_and_negation_exactly() {
        for width in [4, 8, 16, 32, 64] {
            let x = Expr::Var(0.into());
            let p = x.clone() & -x.clone();
            let known_zero = 2 * x.clone();
            let known_one = x.clone() | known_zero.clone();
            let cases = [
                (known_one.clone() + known_zero.clone(), p.clone()),
                (x.clone() ^ known_one.clone(), Expr::zero()),
                (known_zero.clone() | known_one.clone(), p.clone()),
                (known_zero.clone() & x.clone(), Expr::zero()),
                (-known_one, p.clone()),
            ];
            for (operand, expected) in cases {
                let result = analyze_expression(p.clone() & operand.clone(), width);
                assert_eq!(
                    result.result,
                    expected.reduce(make_mask(width)),
                    "width={width} operand={operand} metrics={:?}",
                    result.metrics,
                );
            }
        }
    }

    #[test]
    fn selects_between_two_lowbits() {
        for width in [4, 8, 16, 32, 64] {
            let x = Expr::Var(0.into());
            let p = x.clone() & -x.clone();
            let lowbit_2x = (2 * x.clone()) & -(2 * x.clone());
            let lowbit_3x = (3 * x.clone()) & -(3 * x.clone());
            assert_eq!(
                analyze_expression(p.clone() & lowbit_2x, width).result,
                Expr::zero()
            );
            assert_eq!(
                analyze_expression(p.clone() & lowbit_3x, width).result,
                p.reduce(make_mask(width))
            );
        }
    }

    #[test]
    fn general_products_and_unrelated_variables_stay_unknown() {
        let x = Expr::Var(0.into());
        let y = Expr::Var(1.into());
        let p = x.clone() & -x.clone();
        for operand in [x.clone() * y.clone(), y] {
            let original = p.clone() & operand;
            let result = analyze_expression(original.clone(), 64);
            assert_eq!(result.result, original);
            assert!(!result.changed);
            assert!(result.unknown);
        }
    }

    #[test]
    fn every_applied_rewrite_is_exhaustively_exact_at_small_widths() {
        for width in 1..=8 {
            let mask = make_mask(width);
            let x = Expr::Var(0.into());
            let p = x.clone() & -x.clone();
            let two_x = 2 * x.clone();
            let three_x = 3 * x.clone();
            let operands = [
                two_x.clone(),
                three_x.clone(),
                x.clone() + two_x.clone(),
                x.clone() ^ two_x.clone(),
                x.clone() & two_x.clone(),
                x.clone() | two_x.clone(),
                -three_x.clone(),
                (two_x.clone() & -two_x.clone()),
                (three_x.clone() & -three_x),
            ];
            for operand in operands {
                let original = p.clone() & operand;
                let analysis = analyze_expression(original.clone(), width);
                for value in 0..=mask {
                    let values = [value];
                    assert_eq!(
                        original.eval(&values).get(mask),
                        analysis.result.eval(&values).get(mask),
                        "width={width} value={value:#x} original={original} result={}",
                        analysis.result,
                    );
                }
            }
        }
    }
}
