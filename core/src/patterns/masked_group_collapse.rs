use std::collections::BTreeMap;

use crate::expr::{Expr, VarId};

use super::{Pattern, Tag, negated, scale_relation};

/// Widest variable set we build bit-column signatures over.
const MAX_VARS: usize = 4;

/// Most distinct non-shared conjuncts ("atoms") a group may have. The vector
/// enumeration below is `2^MAX_ATOMS` wide, so this stays small.
const MAX_ATOMS: usize = 5;

/// Cancels a group of `Add` terms that all sit under a common conjunction.
///
/// Obfuscators leave residues shaped like `Σ cᵢ·(Tᵢ & M)`, where `M` is a
/// conjunction that annihilates the low bits of everything the `Tᵢ` differ by.
/// The sum is zero, but no single node exposes that: the cancellation is
/// between *different* terms of the `Add`, and `M` never appears as a bare
/// annihilator for [`super::LowBitAnnihilator`] to match.
///
/// This pattern reasons one bit-column at a time. Terms sharing a conjunct set
/// `S` are grouped; each term's remaining conjuncts (its *atom*) are drawn from
/// a small pool of primitives `p₁..p_r`. At a given bit position inside `S`,
/// every primitive is either set or clear, so a column is described by a vector
/// `v ∈ {0,1}^r` and the group contributes `Σ_{atomᵢ ⊆ v} cᵢ` there. The group
/// is therefore zero as soon as, for every `v`, either that coefficient sum
/// vanishes or the column is impossible — i.e. `S ∧ ⋀ᵢ (pᵢ or ¬pᵢ)` is
/// identically zero. [`proves_zero`] discharges the impossible ones.
pub(super) struct MaskedGroupCollapse;

/// A term of an `Add`, split into its coefficient and its conjuncts.
struct Term {
    coefficient: u64,
    conjuncts: Vec<Expr>,
}

fn as_term(e: &Expr) -> Option<Term> {
    let (coefficient, core) = match e {
        Expr::Scale(c, inner) => (*c, inner.as_ref()),
        Expr::Const(_) => return None,
        other => (1, other),
    };
    let conjuncts = match core {
        Expr::And(children) => children.clone(),
        other => vec![other.clone()],
    };
    Some(Term {
        coefficient,
        conjuncts,
    })
}

/// The conjuncts common to `a` and `b`, with multiplicity.
fn intersect(a: &[Expr], b: &[Expr]) -> Vec<Expr> {
    let mut remaining = b.to_vec();
    let mut shared = Vec::new();
    for factor in a {
        if let Some(position) = remaining.iter().position(|other| other == factor) {
            remaining.swap_remove(position);
            shared.push(factor.clone());
        }
    }
    shared
}

/// `all` minus one occurrence of each conjunct in `shared`.
fn difference(all: &[Expr], shared: &[Expr]) -> Vec<Expr> {
    let mut remaining = all.to_vec();
    for factor in shared {
        if let Some(position) = remaining.iter().position(|other| other == factor) {
            remaining.swap_remove(position);
        }
    }
    remaining
}

/// Whether `e` is a linear combination of purely bitwise terms.
///
/// For such an expression the bit-column signature computed by [`signature`] is
/// exact: evaluating with every variable set to all-ones or all-zeros yields
/// `-s` where `s` is the column's integer coefficient sum, so a signature drawn
/// from `{0, mask}` proves `s ∈ {0, 1}`, i.e. that no column carries and `e` is
/// itself a bitwise function. Without this guard the signature would be
/// meaningless — `v0 & (v0 + 1)` reads as the constant zero on uniform inputs.
fn is_linear_mba(e: &Expr) -> bool {
    let is_leaf = |t: &Expr| matches!(t, Expr::Const(_)) || t.is_bitwise();
    let is_term = |t: &Expr| match t {
        Expr::Scale(_, inner) => is_leaf(inner),
        other => is_leaf(other),
    };
    match e {
        Expr::Add(terms) => terms.iter().all(is_term),
        other => is_term(other),
    }
}

/// Evaluates `e` once per bit-column pattern over `vars`, each variable held at
/// all-ones or all-zeros.
fn signature(e: &Expr, vars: &[VarId], mask: u64) -> Vec<u64> {
    let width = vars.iter().map(|v| v.0 + 1).max().unwrap_or(0);
    let mut values = vec![0u64; width];
    let mut table = Vec::with_capacity(1 << vars.len());
    for column in 0..(1usize << vars.len()) {
        for (bit, var) in vars.iter().enumerate() {
            values[var.0] = if (column >> bit) & 1 == 1 { mask } else { 0 };
        }
        table.push(e.eval_bits(&values).get(mask));
    }
    table
}

/// The bitwise function `e` denotes, or `None` when it is not one (or when we
/// cannot soundly tell — see [`is_linear_mba`]).
fn bitwise_signature(e: &Expr, vars: &[VarId], mask: u64) -> Option<Vec<u64>> {
    if !is_linear_mba(e) {
        return None;
    }
    let table = signature(e, vars, mask);
    table
        .iter()
        .all(|value| *value == 0 || *value == mask)
        .then_some(table)
}

/// Whether `⋀ conjuncts` is identically zero.
///
/// The one fact used is the low-bit annihilator `Y & (-Y) & (m·Y) = 0` for even
/// `m`: `Y & -Y` isolates `Y`'s lowest set bit `2^k`, which an even multiple of
/// `Y` cannot carry. So the conjunction vanishes whenever it contains `-Y`, an
/// even multiple of `Y`, and a third conjunct whose bits are contained in `Y`'s
/// — containment being either an exact match or a proven bitwise submask.
///
/// Purely bitwise conjuncts are also offered as a single merged witness: the
/// containment usually only holds for their conjunction, not for any one of
/// them (`v1 & ~v0 ⊆ Y` where neither `v1` nor `~v0` is).
fn proves_zero(conjuncts: &[Expr], vars: &[VarId], mask: u64) -> bool {
    let mut candidates = conjuncts.to_vec();
    let bitwise: Vec<Expr> = conjuncts
        .iter()
        .filter(|c| c.is_bitwise())
        .cloned()
        .collect();
    if bitwise.len() >= 2 {
        candidates.push(Expr::And(bitwise));
    }
    let signatures: Vec<Option<Vec<u64>>> = candidates
        .iter()
        .map(|c| bitwise_signature(c, vars, mask))
        .collect();

    for (i, negative) in candidates.iter().enumerate() {
        let base = negated(negative, mask);
        let base_signature = bitwise_signature(&base, vars, mask);

        for (j, multiple) in candidates.iter().enumerate() {
            if j == i {
                continue;
            }
            // An even, non-zero multiple of `base`; odd ones keep the low bit.
            match scale_relation(multiple, &base, mask) {
                Some(m) if m != 0 && m & 1 == 0 => {}
                _ => continue,
            }

            for (k, witness) in candidates.iter().enumerate() {
                if k == i || k == j {
                    continue;
                }
                if scale_relation(witness, &base, mask) == Some(1) {
                    return true;
                }
                let (Some(witness_bits), Some(base_bits)) = (&signatures[k], &base_signature)
                else {
                    continue;
                };
                if witness_bits
                    .iter()
                    .zip(base_bits)
                    .all(|(w, b)| w & !b & mask == 0)
                {
                    return true;
                }
            }
        }
    }
    false
}

impl MaskedGroupCollapse {
    /// Tries to prove that the terms of `parsed` selected by `group` sum to
    /// zero, given they all share the conjunct set `shared`.
    ///
    /// Splitting the bit positions of `shared` by which primitives are set
    /// gives `Σ cᵢ·(Tᵢ & S) = Σ_v s_v·(S & ⋀ ±p)`, so the group vanishes once
    /// every column either cancels (`s_v = 0`) or is impossible. Emitting the
    /// surviving columns instead of bailing out was tried and made the corpus
    /// worse: the rewritten conjunctions block later stages from recognizing
    /// the terms they came from.
    fn group_vanishes(
        group: &[usize],
        parsed: &[Option<Term>],
        shared: &[Expr],
        vars: &[VarId],
        mask: u64,
    ) -> bool {
        // Collect the atoms and index them by the primitives they use.
        let mut primitives: Vec<Expr> = Vec::new();
        let mut atoms: Vec<(u64, usize)> = Vec::with_capacity(group.len());
        for &index in group {
            let term = parsed[index].as_ref().expect("group members are terms");
            let mut selector = 0usize;
            for factor in difference(&term.conjuncts, shared) {
                let position = match primitives.iter().position(|other| *other == factor) {
                    Some(position) => position,
                    None => {
                        primitives.push(factor);
                        primitives.len() - 1
                    }
                };
                if primitives.len() > MAX_ATOMS {
                    return false;
                }
                selector |= 1 << position;
            }
            atoms.push((term.coefficient, selector));
        }
        // Every term has the same conjuncts: `reduce` would have merged them.
        if primitives.is_empty() {
            return false;
        }

        for column in 0..(1usize << primitives.len()) {
            let sum = atoms
                .iter()
                .filter(|(_, selector)| selector & !column == 0)
                .fold(0u64, |acc, (coefficient, _)| acc.wrapping_add(*coefficient));
            if sum & mask == 0 {
                continue;
            }
            let mut conjuncts = shared.to_vec();
            for (bit, primitive) in primitives.iter().enumerate() {
                conjuncts.push(if (column >> bit) & 1 == 1 {
                    primitive.clone()
                } else {
                    !primitive.clone()
                });
            }
            // A non-cancelling column must be impossible.
            if !proves_zero(&conjuncts, vars, mask) {
                return false;
            }
        }
        true
    }
}

impl Pattern for MaskedGroupCollapse {
    fn tags(&self) -> &'static [Tag] {
        &[Tag::Add]
    }

    fn apply(&self, e: &Expr, mask: u64) -> Option<Expr> {
        let Expr::Add(terms) = e else {
            return None;
        };
        if terms.len() < 2 {
            return None;
        }

        let mut vars: Vec<VarId> = e.get_vars().into_iter().collect();
        if vars.is_empty() || vars.len() > MAX_VARS {
            return None;
        }
        vars.sort_unstable();

        let parsed: Vec<Option<Term>> = terms.iter().map(as_term).collect();

        // Conjuncts held by at least two terms are the candidate group pivots.
        let mut occurrences: BTreeMap<&Expr, usize> = BTreeMap::new();
        for term in parsed.iter().flatten() {
            for factor in &term.conjuncts {
                *occurrences.entry(factor).or_default() += 1;
            }
        }
        let pivots: Vec<Expr> = occurrences
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .map(|(factor, _)| factor.clone())
            .collect();

        for pivot in pivots {
            let group: Vec<usize> = parsed
                .iter()
                .enumerate()
                .filter(|(_, term)| term.as_ref().is_some_and(|t| t.conjuncts.contains(&pivot)))
                .map(|(index, _)| index)
                .collect();
            if group.len() < 2 {
                continue;
            }

            let mut shared = parsed[group[0]]
                .as_ref()
                .expect("group members are terms")
                .conjuncts
                .clone();
            for &index in &group[1..] {
                let term = parsed[index].as_ref().expect("group members are terms");
                shared = intersect(&shared, &term.conjuncts);
            }
            if shared.is_empty() {
                continue;
            }

            if !Self::group_vanishes(&group, &parsed, &shared, &vars, mask) {
                continue;
            }

            let mut kept: Vec<Expr> = terms
                .iter()
                .enumerate()
                .filter(|(index, _)| !group.contains(index))
                .map(|(_, term)| term.clone())
                .collect();
            return Some(match kept.len() {
                0 => Expr::zero(),
                1 => kept.remove(0),
                _ => Expr::Add(kept),
            });
        }

        None
    }
}

/// Diagnostics for the residues this pattern does *not* discharge.
///
/// Run with `cargo test --all-features --release ng_probe -- --ignored
/// --nocapture` after a dataset run has refreshed `ng/loki_tiny.csv.txt`.
#[cfg(all(test, feature = "parse"))]
mod ng_probe {
    use super::*;
    use crate::parser::parse_expr;
    use crate::varint::make_mask;

    fn diagnose(e: &Expr, mask: u64) -> String {
        let Expr::Add(terms) = e else {
            return "no-group: residue is not a sum".into();
        };
        let mut vars: Vec<VarId> = e.get_vars().into_iter().collect();
        vars.sort_unstable();
        let parsed: Vec<Option<Term>> = terms.iter().map(as_term).collect();

        let mut occurrences: BTreeMap<&Expr, usize> = BTreeMap::new();
        for term in parsed.iter().flatten() {
            for factor in &term.conjuncts {
                *occurrences.entry(factor).or_default() += 1;
            }
        }
        let mut best = String::from("no-group: no conjunct is shared by two terms");
        for (pivot, count) in occurrences {
            if count < 2 {
                continue;
            }
            let group: Vec<usize> = parsed
                .iter()
                .enumerate()
                .filter(|(_, t)| t.as_ref().is_some_and(|t| t.conjuncts.contains(pivot)))
                .map(|(index, _)| index)
                .collect();
            let mut shared = parsed[group[0]].as_ref().unwrap().conjuncts.clone();
            for &index in &group[1..] {
                shared = intersect(&shared, &parsed[index].as_ref().unwrap().conjuncts);
            }
            if shared.is_empty() {
                continue;
            }

            let mut primitives: Vec<Expr> = Vec::new();
            let mut atoms: Vec<(u64, usize)> = Vec::new();
            for &index in &group {
                let term = parsed[index].as_ref().unwrap();
                let mut selector = 0usize;
                for factor in difference(&term.conjuncts, &shared) {
                    let position =
                        primitives
                            .iter()
                            .position(|o| *o == factor)
                            .unwrap_or_else(|| {
                                primitives.push(factor.clone());
                                primitives.len() - 1
                            });
                    selector |= 1 << position;
                }
                atoms.push((term.coefficient, selector));
            }
            if primitives.len() > MAX_ATOMS {
                best = format!("too-many-atoms: {} > {MAX_ATOMS}", primitives.len());
                continue;
            }

            let mut failures = 0;
            for column in 0..(1usize << primitives.len()) {
                let sum = atoms
                    .iter()
                    .filter(|(_, s)| s & !column == 0)
                    .fold(0u64, |acc, (c, _)| acc.wrapping_add(*c));
                if sum & mask == 0 {
                    continue;
                }
                let mut conjuncts = shared.clone();
                for (bit, primitive) in primitives.iter().enumerate() {
                    conjuncts.push(if (column >> bit) & 1 == 1 {
                        primitive.clone()
                    } else {
                        !primitive.clone()
                    });
                }
                if !proves_zero(&conjuncts, &vars, mask) {
                    failures += 1;
                }
            }
            if failures > 0 {
                let loose = terms.len() - group.len();
                best = if loose > 0 {
                    format!("loose-terms: {loose} term(s) outside the group")
                } else {
                    format!(
                        "undischarged-column: {failures} of {} column(s), {} primitive(s)",
                        1usize << primitives.len(),
                        primitives.len()
                    )
                };
            }
        }
        best
    }

    #[test]
    #[ignore = "diagnostic; needs a refreshed ng report"]
    fn classify_surviving_residues() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ng/loki_tiny.csv.txt");
        let report = std::fs::read_to_string(&path).expect("run the dataset tests first");
        let mask = make_mask(64);

        let mut cases = 0;
        for line in report.lines() {
            let Some(text) = line.trim().strip_prefix("diff-produced: ") else {
                continue;
            };
            let residue = parse_expr(text).expect("ng report is parseable");
            let reduced = residue.reduce_masked(mask);
            cases += 1;
            println!("{cases:3}  {}", diagnose(&reduced, mask));
        }
        println!("total: {cases}");
    }
}
