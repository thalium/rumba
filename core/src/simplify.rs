use std::{
    cell::{Cell, RefCell},
    cmp::max,
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        Mutex,
        atomic::{AtomicI64, AtomicU64, Ordering},
    },
};

use crate::{
    bimap::BiMap,
    expr::{Expr, VarId},
    varint::{VarInt, make_mask},
};

use log::debug;

/// The largest number of variables a linear MBA may carry into the truth-table
/// solve. The signature is `2^t` wide, so this bounds one solve at 1M entries.
pub const MAX_VARS: usize = 20;
const MAX_SIMPLIFICATION_PASSES: usize = 8;
const MAX_FRONTIER_ATOMS: usize = 4;
const MAX_FRONTIER_SIGNATURE_SIZE: usize = 16;
const MAX_TRANSFORMED_AST_FACTOR: usize = 4;

/// Classification used by the opt-in hidden-atom diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HiddenAtomDependencyKind {
    Root,
    BitwiseDependent,
    ArithmeticDependent,
    LowBitCandidate,
}

/// One atom introduced by `hide_in_var` while diagnostics are enabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HiddenAtomTrace {
    pub atom: VarId,
    pub original: Expr,
    pub simplified: Expr,
    pub free_atoms: Vec<VarId>,
    pub dependent_atoms: Vec<VarId>,
    pub dependency_definition: Option<Expr>,
    pub dependency_kind: HiddenAtomDependencyKind,
}

/// Hidden atoms created by one recursive solver invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HiddenScopeTrace {
    pub scope: usize,
    pub bit_width: u8,
    pub input: Expr,
    pub pre_restore_result: Option<Expr>,
    pub atoms: Vec<HiddenAtomTrace>,
}

#[derive(Default)]
struct HiddenTraceState {
    next_scope: usize,
    scopes: Vec<HiddenScopeTrace>,
}

thread_local! {
    static HIDDEN_TRACE_STATE: RefCell<Option<HiddenTraceState>> = const { RefCell::new(None) };
}

fn begin_hidden_trace_scope(input: &Expr, bit_width: u8) -> Option<usize> {
    HIDDEN_TRACE_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.as_mut()?;
        let scope = state.next_scope;
        state.next_scope += 1;
        state.scopes.push(HiddenScopeTrace {
            scope,
            bit_width,
            input: input.clone(),
            pre_restore_result: None,
            atoms: Vec::new(),
        });
        Some(scope)
    })
}

fn record_pre_restore_result(scope: Option<usize>, result: &Expr) {
    let Some(scope) = scope else {
        return;
    };

    HIDDEN_TRACE_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.as_mut().expect("pre-restore result without trace session");
        let scope = state
            .scopes
            .iter_mut()
            .find(|candidate| candidate.scope == scope)
            .expect("unknown pre-restore trace scope");
        scope.pre_restore_result = Some(result.clone());
    });
}

fn record_hidden_atom(scope: Option<usize>, atom: VarId, original: Expr, simplified: Expr) {
    let Some(scope) = scope else {
        return;
    };

    let mut free_atoms: Vec<_> = simplified.get_vars().into_iter().collect();
    free_atoms.sort();
    HIDDEN_TRACE_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.as_mut().expect("hidden-atom trace scope without session");
        let scope = state
            .scopes
            .iter_mut()
            .find(|candidate| candidate.scope == scope)
            .expect("unknown hidden-atom trace scope");
        scope.atoms.push(HiddenAtomTrace {
            atom,
            original,
            simplified,
            free_atoms,
            dependent_atoms: Vec::new(),
            dependency_definition: None,
            dependency_kind: HiddenAtomDependencyKind::Root,
        });
    });
}

fn is_strict_bitwise_definition(e: &Expr, mask: u64) -> bool {
    match e {
        Expr::Const(c) => is_bitwise_constant(*c, mask),
        Expr::Var(_) => true,
        Expr::Not(e) => is_strict_bitwise_definition(e, mask),
        Expr::And(es) | Expr::Or(es) | Expr::Xor(es) => {
            es.iter().all(|e| is_strict_bitwise_definition(e, mask))
        }
        Expr::Scale(_, _) | Expr::Add(_) | Expr::Mul(_) => false,
    }
}

fn contains_lowbit_candidate(e: &Expr, mask: u64) -> bool {
    let is_lowbit_and = |terms: &[Expr]| {
        terms.iter().enumerate().any(|(i, left)| {
            let negated = (-left.clone()).reduce(mask);
            terms
                .iter()
                .enumerate()
                .any(|(j, right)| i != j && right.clone().reduce(mask) == negated)
        })
    };

    match e {
        Expr::And(es) if is_lowbit_and(es) => true,
        Expr::Var(_) | Expr::Const(_) => false,
        Expr::Not(e) | Expr::Scale(_, e) => contains_lowbit_candidate(e, mask),
        Expr::And(es)
        | Expr::Or(es)
        | Expr::Xor(es)
        | Expr::Add(es)
        | Expr::Mul(es) => es.iter().any(|e| contains_lowbit_candidate(e, mask)),
    }
}

fn abstract_known_hidden_atoms(
    e: &Expr,
    current: VarId,
    definitions: &[(VarId, Expr)],
) -> Expr {
    if let Some((atom, _)) = definitions
        .iter()
        .find(|(atom, definition)| *atom != current && definition == e)
    {
        return Expr::Var(*atom);
    }

    e.clone()
        .map(|e| abstract_known_hidden_atoms(&e, current, definitions))
}

fn finalize_hidden_trace(mut state: HiddenTraceState) -> Vec<HiddenScopeTrace> {
    for scope in &mut state.scopes {
        let hidden_atoms: HashSet<_> = scope.atoms.iter().map(|atom| atom.atom).collect();
        let definitions: Vec<_> = scope
            .atoms
            .iter()
            .map(|atom| (atom.atom, atom.simplified.clone()))
            .collect();
        let mask = make_mask(scope.bit_width);

        for atom in &mut scope.atoms {
            let dependency_definition = abstract_known_hidden_atoms(
                &atom.simplified,
                atom.atom,
                &definitions,
            );
            atom.dependent_atoms = dependency_definition
                .get_vars()
                .iter()
                .copied()
                .filter(|var| hidden_atoms.contains(var))
                .collect();
            atom.dependent_atoms.sort();
            atom.dependency_definition =
                (!atom.dependent_atoms.is_empty()).then_some(dependency_definition.clone());
            atom.dependency_kind = if contains_lowbit_candidate(&atom.simplified, mask)
                || contains_lowbit_candidate(&dependency_definition, mask)
            {
                HiddenAtomDependencyKind::LowBitCandidate
            } else if atom.dependent_atoms.is_empty() {
                HiddenAtomDependencyKind::Root
            } else if is_strict_bitwise_definition(&dependency_definition, mask) {
                HiddenAtomDependencyKind::BitwiseDependent
            } else {
                HiddenAtomDependencyKind::ArithmeticDependent
            };
        }
    }

    state.scopes
}

enum CandidateCertification {
    Proved(Expr),
    NonBooleanCube,
    ExactProofNotFound { candidate: Expr, residual: Option<Expr> },
}

fn try_certify_bitwise_candidate(
    definition: &Expr,
    bit_width: u8,
) -> CandidateCertification {
    const MAX_DIRECT_PARENTS: usize = 3;

    let mask = make_mask(bit_width);
    let mut parents: Vec<_> = definition.get_vars().into_iter().collect();
    parents.sort();
    if parents.len() > MAX_DIRECT_PARENTS {
        return CandidateCertification::NonBooleanCube;
    }

    let value_count = parents
        .iter()
        .map(|parent| parent.0)
        .max()
        .map_or(0, |max_id| max_id + 1);
    let mut values = vec![0; value_count];
    let mut minterms = Vec::new();

    for assignment in 0..(1usize << parents.len()) {
        for (index, parent) in parents.iter().enumerate() {
            values[parent.0] = if (assignment >> index) & 1 == 0 {
                0
            } else {
                mask
            };
        }

        let output = definition.eval(&values).get(mask);
        if output != 0 && output != mask {
            return CandidateCertification::NonBooleanCube;
        }
        if output == 0 {
            continue;
        }

        let terms: Vec<_> = parents
            .iter()
            .enumerate()
            .map(|(index, parent)| {
                let variable = Expr::Var(*parent);
                if (assignment >> index) & 1 == 0 {
                    !variable
                } else {
                    variable
                }
            })
            .collect();
        minterms.push(match terms.len() {
            0 => Expr::Const(VarInt::MAX),
            1 => terms.into_iter().next().unwrap(),
            _ => Expr::And(terms),
        });
    }

    let candidate = match minterms.len() {
        0 => Expr::zero(),
        1 => minterms.into_iter().next().unwrap(),
        _ => Expr::Or(minterms),
    }
    .reduce(mask);
    let proof = simplify_mba(
        (definition.clone() - candidate.clone()).reduce(mask),
        bit_width,
    );

    match proof {
        Ok(proof) if proof == Expr::zero() => CandidateCertification::Proved(candidate),
        Ok(residual) => CandidateCertification::ExactProofNotFound {
            candidate,
            residual: Some(residual),
        },
        Err(_) => CandidateCertification::ExactProofNotFound {
            candidate,
            residual: None,
        },
    }
}

#[cfg(test)]
fn certify_bitwise_candidate(definition: &Expr, bit_width: u8) -> Option<Expr> {
    match try_certify_bitwise_candidate(definition, bit_width) {
        CandidateCertification::Proved(candidate) => Some(candidate),
        CandidateCertification::NonBooleanCube
        | CandidateCertification::ExactProofNotFound { .. } => None,
    }
}

/// One exact word-level certificate produced by the P7b-lite diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectDependencyProof {
    pub atom: VarId,
    pub definition: Expr,
    pub candidate: Expr,
}

/// Why one direct dependency was not certified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectDependencyRejectReason {
    NonBooleanCube,
    ExactProofNotFound {
        candidate: Expr,
        residual: Option<Expr>,
    },
}

/// One direct dependency rejected by the P7b-lite diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectDependencyRejection {
    pub atom: VarId,
    pub reason: DirectDependencyRejectReason,
}

/// Result of one substitution pass over atoms referenced directly by a
/// pre-restoration solver result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectDependencyExperiment {
    pub scope: usize,
    pub direct_atoms: Vec<VarId>,
    pub attempted_atoms: Vec<VarId>,
    pub proofs: Vec<DirectDependencyProof>,
    pub rejections: Vec<DirectDependencyRejection>,
    pub substituted_pre_restore: Expr,
    pub simplified_after_substitution: Expr,
}

/// Certify and substitute direct arithmetic dependencies in one traced scope.
/// Returning a non-zero expression means only that the experiment did not
/// prove the scope zero.
pub fn experiment_direct_bitwise_dependencies(
    scope: &HiddenScopeTrace,
) -> Result<Option<DirectDependencyExperiment>, SolveError> {
    let Some(pre_restore_result) = &scope.pre_restore_result else {
        return Ok(None);
    };

    let atoms: HashMap<_, _> = scope.atoms.iter().map(|atom| (atom.atom, atom)).collect();
    let mut direct_atoms: Vec<_> = pre_restore_result
        .get_vars()
        .into_iter()
        .filter(|atom| atoms.contains_key(atom))
        .collect();
    direct_atoms.sort();

    let mut attempted_atoms = Vec::new();
    let mut proofs = Vec::new();
    let mut rejections = Vec::new();
    let mut substituted = pre_restore_result.clone();
    for atom_id in &direct_atoms {
        let atom = atoms[atom_id];
        if atom.dependency_kind != HiddenAtomDependencyKind::ArithmeticDependent {
            continue;
        }
        let Some(definition) = &atom.dependency_definition else {
            continue;
        };
        if definition.get_vars().len() > 3 {
            continue;
        }

        attempted_atoms.push(*atom_id);
        match try_certify_bitwise_candidate(definition, scope.bit_width) {
            CandidateCertification::Proved(candidate) => {
                substituted = substituted.replace_var(*atom_id, &candidate);
                proofs.push(DirectDependencyProof {
                    atom: *atom_id,
                    definition: definition.clone(),
                    candidate,
                });
            }
            CandidateCertification::NonBooleanCube => {
                rejections.push(DirectDependencyRejection {
                    atom: *atom_id,
                    reason: DirectDependencyRejectReason::NonBooleanCube,
                });
            }
            CandidateCertification::ExactProofNotFound {
                candidate,
                residual,
            } => {
                rejections.push(DirectDependencyRejection {
                    atom: *atom_id,
                    reason: DirectDependencyRejectReason::ExactProofNotFound {
                        candidate,
                        residual,
                    },
                });
            }
        }
    }

    let simplified_after_substitution = simplify_mba(substituted.clone(), scope.bit_width)?;
    Ok(Some(DirectDependencyExperiment {
        scope: scope.scope,
        direct_atoms,
        attempted_atoms,
        proofs,
        rejections,
        substituted_pre_restore: substituted,
        simplified_after_substitution,
    }))
}

/// Outcome of the exact complement proof attempted by P7c-lite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComplementRelationProof {
    Proved,
    NotProved { residual: Expr },
    ProofError(SolveError),
}

/// Result of the targeted complement experiment over the two direct hidden
/// atoms of one pre-restoration result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectComplementExperiment {
    pub scope: usize,
    pub left: VarId,
    pub right: VarId,
    pub left_definition: Expr,
    pub right_definition: Expr,
    pub relation: Expr,
    pub proof: ComplementRelationProof,
    pub substituted_pre_restore: Option<Expr>,
    pub simplified_after_substitution: Option<Expr>,
}

/// Test the exact relation `left + right + 1 == 0` for the two hidden atoms
/// referenced directly by a pre-restoration result.
pub fn experiment_direct_complement_relation(
    scope: &HiddenScopeTrace,
) -> Result<Option<DirectComplementExperiment>, SolveError> {
    let Some(pre_restore_result) = &scope.pre_restore_result else {
        return Ok(None);
    };

    let atoms: HashMap<_, _> = scope.atoms.iter().map(|atom| (atom.atom, atom)).collect();
    let mut direct_atoms: Vec<_> = pre_restore_result
        .get_vars()
        .into_iter()
        .filter(|atom| atoms.contains_key(atom))
        .collect();
    direct_atoms.sort();
    let [left, right] = direct_atoms.as_slice() else {
        return Ok(None);
    };
    let left_definition = atoms[left].simplified.clone();
    let right_definition = atoms[right].simplified.clone();
    let relation = (left_definition.clone()
        + right_definition.clone()
        + Expr::make_const(1))
    .reduce(make_mask(scope.bit_width));

    let (proof, substituted_pre_restore, simplified_after_substitution) =
        match simplify_mba(relation.clone(), scope.bit_width) {
            Ok(residual) if residual == Expr::zero() => {
                let substituted = pre_restore_result
                    .clone()
                    .replace_var(*right, &!Expr::Var(*left));
                let simplified = simplify_mba(substituted.clone(), scope.bit_width)?;
                (
                    ComplementRelationProof::Proved,
                    Some(substituted),
                    Some(simplified),
                )
            }
            Ok(residual) => (
                ComplementRelationProof::NotProved { residual },
                None,
                None,
            ),
            Err(error) => (
                ComplementRelationProof::ProofError(error),
                None,
                None,
            ),
        };

    Ok(Some(DirectComplementExperiment {
        scope: scope.scope,
        left: *left,
        right: *right,
        left_definition,
        right_definition,
        relation,
        proof,
        substituted_pre_restore,
        simplified_after_substitution,
    }))
}

const MAX_GUIDED_ALIAS_PAIRS: usize = 12;

fn insert_guided_pair(
    pairs: &mut Vec<(VarId, VarId)>,
    seen: &mut HashSet<(VarId, VarId)>,
    left: VarId,
    right: VarId,
) {
    if left == right || pairs.len() == MAX_GUIDED_ALIAS_PAIRS {
        return;
    }
    let pair = if left < right {
        (left, right)
    } else {
        (right, left)
    };
    if seen.insert(pair) {
        pairs.push(pair);
    }
}

fn atom_contexts_in_add_term(
    term: &Expr,
    direct_atoms: &HashSet<VarId>,
    mask: u64,
) -> Vec<(VarId, (u8, Expr))> {
    let mut body = term;
    while let Expr::Scale(_, inner) = body {
        body = inner;
    }

    let (tag, terms) = match body {
        Expr::Var(atom) if direct_atoms.contains(atom) => {
            return vec![(*atom, (0, Expr::zero()))];
        }
        Expr::And(terms) => (1, terms),
        Expr::Or(terms) => (2, terms),
        Expr::Xor(terms) => (3, terms),
        Expr::Mul(terms) => (4, terms),
        _ => return Vec::new(),
    };

    terms
        .iter()
        .enumerate()
        .filter_map(|(candidate_index, candidate)| {
            let Expr::Var(atom) = candidate else {
                return None;
            };
            if !direct_atoms.contains(atom) {
                return None;
            }
            let remaining: Vec<_> = terms
                .iter()
                .enumerate()
                .filter(|(term_index, _)| *term_index != candidate_index)
                .map(|(_, term)| term)
                .cloned()
                .collect();
            let context = match tag {
                1 => Expr::And(remaining),
                2 => Expr::Or(remaining),
                3 => Expr::Xor(remaining),
                4 => Expr::Mul(remaining),
                _ => unreachable!(),
            }
            .reduce(mask);
            Some((*atom, (tag, context)))
        })
        .collect()
}

fn collect_locally_copresent_pairs(
    expression: &Expr,
    direct_atoms: &HashSet<VarId>,
    pairs: &mut Vec<(VarId, VarId)>,
    seen: &mut HashSet<(VarId, VarId)>,
) {
    match expression {
        Expr::And(terms) | Expr::Or(terms) | Expr::Xor(terms) | Expr::Mul(terms) => {
            let mut local_atoms: Vec<_> = expression
                .get_vars()
                .into_iter()
                .filter(|atom| direct_atoms.contains(atom))
                .collect();
            local_atoms.sort();
            for (index, left) in local_atoms.iter().enumerate() {
                for right in &local_atoms[index + 1..] {
                    insert_guided_pair(pairs, seen, *left, *right);
                }
            }
            for term in terms {
                collect_locally_copresent_pairs(term, direct_atoms, pairs, seen);
            }
        }
        Expr::Add(terms) => {
            for term in terms {
                collect_locally_copresent_pairs(term, direct_atoms, pairs, seen);
            }
        }
        Expr::Not(inner) | Expr::Scale(_, inner) => {
            collect_locally_copresent_pairs(inner, direct_atoms, pairs, seen);
        }
        Expr::Var(_) | Expr::Const(_) => {}
    }
}

fn select_guided_direct_pairs(
    pre_restore_result: &Expr,
    direct_atoms: &HashSet<VarId>,
    mask: u64,
) -> Vec<(VarId, VarId)> {
    let terms = match pre_restore_result {
        Expr::Add(terms) => terms.as_slice(),
        expression => std::slice::from_ref(expression),
    };
    let mut contexts: BTreeMap<(u8, Expr), Vec<VarId>> = BTreeMap::new();
    for term in terms {
        for (atom, context) in atom_contexts_in_add_term(term, direct_atoms, mask) {
            contexts.entry(context).or_default().push(atom);
        }
    }

    let mut pairs = Vec::new();
    let mut seen = HashSet::new();
    for atoms in contexts.values_mut() {
        atoms.sort();
        atoms.dedup();
        for (index, left) in atoms.iter().enumerate() {
            for right in &atoms[index + 1..] {
                insert_guided_pair(&mut pairs, &mut seen, *left, *right);
            }
        }
    }
    collect_locally_copresent_pairs(
        pre_restore_result,
        direct_atoms,
        &mut pairs,
        &mut seen,
    );
    pairs
}

/// The three bounded semantic aliases considered by P7d-lite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticAlias {
    Equal,
    Complement,
    ArithmeticOpposite,
}

/// Outcome of one exact semantic-alias proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticAliasProof {
    Proved,
    NotProved { residual: Expr },
    ProofError(SolveError),
}

/// One relation attempted between a structurally selected pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticAliasAttempt {
    pub left: VarId,
    pub right: VarId,
    pub alias: SemanticAlias,
    pub relation: Expr,
    pub proof: SemanticAliasProof,
}

/// One alias selected for diagnostic substitution after exact certification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CertifiedSemanticAlias {
    pub left: VarId,
    pub right: VarId,
    pub alias: SemanticAlias,
}

/// Bounded P7d-lite result for one final pre-restoration scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuidedSemanticAliasExperiment {
    pub scope: usize,
    pub direct_atoms: Vec<VarId>,
    pub candidate_pairs: Vec<(VarId, VarId)>,
    pub attempts: Vec<SemanticAliasAttempt>,
    pub certified_aliases: Vec<CertifiedSemanticAlias>,
    pub substituted_pre_restore: Expr,
    pub simplified_after_substitution: Expr,
}

/// Search only equality, complement and arithmetic-opposite relations between
/// direct hidden atoms selected from the structure of one pre-restoration
/// result. This diagnostic does not alter the ordinary simplification path.
pub fn experiment_guided_semantic_aliases(
    scope: &HiddenScopeTrace,
) -> Result<Option<GuidedSemanticAliasExperiment>, SolveError> {
    let Some(pre_restore_result) = &scope.pre_restore_result else {
        return Ok(None);
    };

    let atoms: HashMap<_, _> = scope.atoms.iter().map(|atom| (atom.atom, atom)).collect();
    let mut direct_atoms: Vec<_> = pre_restore_result
        .get_vars()
        .into_iter()
        .filter(|atom| atoms.contains_key(atom))
        .collect();
    direct_atoms.sort();
    let direct_atom_set: HashSet<_> = direct_atoms.iter().copied().collect();
    let mask = make_mask(scope.bit_width);
    let candidate_pairs =
        select_guided_direct_pairs(pre_restore_result, &direct_atom_set, mask);
    let cache = LocalCache::new();
    let mut definitions = HashMap::new();
    for atom in &direct_atoms {
        let definition = atoms[atom].simplified.clone();
        let simplified = simplify_mba_with_cache(&cache, definition.clone(), scope.bit_width)
            .unwrap_or(definition);
        definitions.insert(*atom, simplified);
    }

    let mut attempts = Vec::new();
    let mut certified_aliases = Vec::new();
    for (left, right) in &candidate_pairs {
        let left_definition = &definitions[left];
        let right_definition = &definitions[right];
        let mut selected_alias = None;
        for alias in [
            SemanticAlias::Equal,
            SemanticAlias::Complement,
            SemanticAlias::ArithmeticOpposite,
        ] {
            let relation = match alias {
                SemanticAlias::Equal => left_definition.clone() - right_definition.clone(),
                SemanticAlias::Complement => {
                    left_definition.clone()
                        + right_definition.clone()
                        + Expr::make_const(1)
                }
                SemanticAlias::ArithmeticOpposite => {
                    left_definition.clone() + right_definition.clone()
                }
            }
            .reduce(mask);
            let proof = match simplify_mba_with_cache(
                &cache,
                relation.clone(),
                scope.bit_width,
            ) {
                Ok(residual) if residual == Expr::zero() => {
                    if selected_alias.is_none() {
                        selected_alias = Some(CertifiedSemanticAlias {
                            left: *left,
                            right: *right,
                            alias,
                        });
                    }
                    SemanticAliasProof::Proved
                }
                Ok(residual) => SemanticAliasProof::NotProved { residual },
                Err(error) => SemanticAliasProof::ProofError(error),
            };
            attempts.push(SemanticAliasAttempt {
                left: *left,
                right: *right,
                alias,
                relation,
                proof,
            });
        }
        if let Some(alias) = selected_alias {
            certified_aliases.push(alias);
        }
    }

    certified_aliases.sort_by_key(|alias| std::cmp::Reverse(alias.right));
    let mut substituted = pre_restore_result.clone();
    for alias in &certified_aliases {
        let representative = Expr::Var(alias.left);
        let replacement = match alias.alias {
            SemanticAlias::Equal => representative,
            SemanticAlias::Complement => !representative,
            SemanticAlias::ArithmeticOpposite => -representative,
        };
        substituted = substituted.replace_var(alias.right, &replacement);
    }
    let simplified_after_substitution =
        simplify_mba_with_cache(&cache, substituted.clone(), scope.bit_width)?;

    Ok(Some(GuidedSemanticAliasExperiment {
        scope: scope.scope,
        direct_atoms,
        candidate_pairs,
        attempts,
        certified_aliases,
        substituted_pre_restore: substituted,
        simplified_after_substitution,
    }))
}

/// The three structure-guided ternary relations allowed by P7f-micro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuidedTernaryRelation {
    OneMinusTwo,
    TwoMinusOne,
    ComplementSum,
}

fn make_guided_ternary_relation(
    kind: GuidedTernaryRelation,
    a: &Expr,
    b: &Expr,
    c: &Expr,
    mask: u64,
) -> Expr {
    match kind {
        GuidedTernaryRelation::OneMinusTwo => {
            a.clone() - b.clone() - c.clone()
        }
        GuidedTernaryRelation::TwoMinusOne => {
            a.clone() + b.clone() - c.clone()
        }
        GuidedTernaryRelation::ComplementSum => {
            a.clone() + b.clone() + c.clone() + Expr::make_const(1)
        }
    }
    .reduce(mask)
}

fn count_selected_atom_occurrences(
    expression: &Expr,
    selected_atoms: &HashSet<VarId>,
    counts: &mut HashMap<VarId, usize>,
) {
    match expression {
        Expr::Var(atom) => {
            if selected_atoms.contains(atom) {
                *counts.entry(*atom).or_default() += 1;
            }
        }
        Expr::Not(inner) | Expr::Scale(_, inner) => {
            count_selected_atom_occurrences(inner, selected_atoms, counts);
        }
        Expr::And(terms)
        | Expr::Or(terms)
        | Expr::Xor(terms)
        | Expr::Add(terms)
        | Expr::Mul(terms) => {
            for term in terms {
                count_selected_atom_occurrences(term, selected_atoms, counts);
            }
        }
        Expr::Const(_) => {}
    }
}

fn replace_vars_simultaneously(
    expression: Expr,
    replacements: &HashMap<VarId, Expr>,
) -> Expr {
    match expression {
        Expr::Var(atom) => replacements
            .get(&atom)
            .cloned()
            .unwrap_or(Expr::Var(atom)),
        expression => expression.map(|child| {
            replace_vars_simultaneously(child, replacements)
        }),
    }
}

/// One exact P7f-micro ternary proof attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuidedTernaryAttempt {
    pub atoms: [VarId; 3],
    pub kind: GuidedTernaryRelation,
    pub relation: Expr,
    pub proof: SemanticAliasProof,
}

/// One certified ternary relation selected for simultaneous substitution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertifiedGuidedTernaryRelation {
    pub atoms: [VarId; 3],
    pub kind: GuidedTernaryRelation,
    pub target: VarId,
    pub replacement: Expr,
}

/// Result of P7f-micro after the binary P7d aliases and one ternary pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuidedTernaryExperiment {
    pub scope: usize,
    pub binary_aliases: Vec<CertifiedSemanticAlias>,
    pub residual_after_binary_aliases: Expr,
    pub remaining_atoms: Vec<VarId>,
    pub occurrence_counts: Vec<(VarId, usize)>,
    pub attempts: Vec<GuidedTernaryAttempt>,
    pub certified_relations: Vec<CertifiedGuidedTernaryRelation>,
    pub substituted_residual: Expr,
    pub simplified_after_substitution: Expr,
}

/// Try only the three coefficient-bounded ternary relations allowed by
/// P7f-micro on the three or four hidden atoms left after P7d substitutions.
pub fn experiment_guided_ternary_relations(
    scope: &HiddenScopeTrace,
) -> Result<Option<GuidedTernaryExperiment>, SolveError> {
    let Some(binary_experiment) = experiment_guided_semantic_aliases(scope)? else {
        return Ok(None);
    };
    let residual_after_binary_aliases =
        binary_experiment.simplified_after_substitution.clone();
    let atoms: HashMap<_, _> = scope.atoms.iter().map(|atom| (atom.atom, atom)).collect();
    let mut remaining_atoms: Vec<_> = residual_after_binary_aliases
        .get_vars()
        .into_iter()
        .filter(|atom| atoms.contains_key(atom))
        .collect();
    let remaining_set: HashSet<_> = remaining_atoms.iter().copied().collect();
    let mut counts = HashMap::new();
    count_selected_atom_occurrences(
        &residual_after_binary_aliases,
        &remaining_set,
        &mut counts,
    );
    remaining_atoms.sort_by_key(|atom| {
        (std::cmp::Reverse(counts.get(atom).copied().unwrap_or(0)), *atom)
    });
    let occurrence_counts = remaining_atoms
        .iter()
        .map(|atom| (*atom, counts.get(atom).copied().unwrap_or(0)))
        .collect();

    let cache = LocalCache::new();
    let mut definitions = HashMap::new();
    for atom in &remaining_atoms {
        let definition = atoms[atom].simplified.clone();
        let simplified = simplify_mba_with_cache(
            &cache,
            definition.clone(),
            scope.bit_width,
        )
        .unwrap_or(definition);
        definitions.insert(*atom, simplified);
    }

    let mut attempts = Vec::new();
    let mut certified_relations = Vec::new();
    let mut substituted_targets = HashSet::new();
    if (3..=4).contains(&remaining_atoms.len()) {
        for first in 0..remaining_atoms.len() - 2 {
            for second in first + 1..remaining_atoms.len() - 1 {
                for third in second + 1..remaining_atoms.len() {
                    let selected = [
                        remaining_atoms[first],
                        remaining_atoms[second],
                        remaining_atoms[third],
                    ];
                    let [a, b, c] = selected.map(|atom| Expr::Var(atom));
                    for kind in [
                        GuidedTernaryRelation::OneMinusTwo,
                        GuidedTernaryRelation::TwoMinusOne,
                        GuidedTernaryRelation::ComplementSum,
                    ] {
                        let relation = make_guided_ternary_relation(
                            kind,
                            &definitions[&selected[0]],
                            &definitions[&selected[1]],
                            &definitions[&selected[2]],
                            make_mask(scope.bit_width),
                        );
                        let proof = match simplify_mba_with_cache(
                            &cache,
                            relation.clone(),
                            scope.bit_width,
                        ) {
                            Ok(residual) if residual == Expr::zero() => {
                                let (target, replacement) = match kind {
                                    GuidedTernaryRelation::OneMinusTwo => {
                                        (selected[0], b.clone() + c.clone())
                                    }
                                    GuidedTernaryRelation::TwoMinusOne => {
                                        (selected[2], a.clone() + b.clone())
                                    }
                                    GuidedTernaryRelation::ComplementSum => (
                                        selected[2],
                                        -a.clone() - b.clone() - Expr::make_const(1),
                                    ),
                                };
                                if substituted_targets.insert(target) {
                                    certified_relations.push(
                                        CertifiedGuidedTernaryRelation {
                                            atoms: selected,
                                            kind,
                                            target,
                                            replacement: replacement
                                                .reduce(make_mask(scope.bit_width)),
                                        },
                                    );
                                }
                                SemanticAliasProof::Proved
                            }
                            Ok(residual) => SemanticAliasProof::NotProved { residual },
                            Err(error) => SemanticAliasProof::ProofError(error),
                        };
                        attempts.push(GuidedTernaryAttempt {
                            atoms: selected,
                            kind,
                            relation,
                            proof,
                        });
                    }
                }
            }
        }
    }

    let replacements: HashMap<_, _> = certified_relations
        .iter()
        .map(|relation| (relation.target, relation.replacement.clone()))
        .collect();
    let substituted_residual = replace_vars_simultaneously(
        residual_after_binary_aliases.clone(),
        &replacements,
    );
    let simplified_after_substitution = simplify_mba_with_cache(
        &cache,
        substituted_residual.clone(),
        scope.bit_width,
    )?;

    Ok(Some(GuidedTernaryExperiment {
        scope: scope.scope,
        binary_aliases: binary_experiment.certified_aliases,
        residual_after_binary_aliases,
        remaining_atoms,
        occurrence_counts,
        attempts,
        certified_relations,
        substituted_residual,
        simplified_after_substitution,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ExprCost {
    arithmetic_bitwise_alternations: usize,
    ast_nodes: usize,
    printed_size: usize,
}

#[derive(Default)]
struct BitwiseFrontierMetrics {
    attempts: usize,
    normalized: usize,
    aborts_atom_limit: usize,
    aborts_size_limit: usize,
    max_frontier_atoms: usize,
}

/// Aggregate activity of the bounded bitwise-frontier pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BitwiseFrontierStats {
    pub attempts: u64,
    pub successes: u64,
    pub aborts_atom_limit: u64,
    pub aborts_size_limit: u64,
    pub max_frontier_atoms: u64,
    pub candidate_node_delta: i64,
    pub candidate_cost_delta: i64,
}

static BITWISE_FRONTIER_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static BITWISE_FRONTIER_SUCCESSES: AtomicU64 = AtomicU64::new(0);
static BITWISE_FRONTIER_ABORTS_ATOM_LIMIT: AtomicU64 = AtomicU64::new(0);
static BITWISE_FRONTIER_ABORTS_SIZE_LIMIT: AtomicU64 = AtomicU64::new(0);
static BITWISE_FRONTIER_MAX_ATOMS: AtomicU64 = AtomicU64::new(0);
static BITWISE_FRONTIER_NODE_DELTA: AtomicI64 = AtomicI64::new(0);
static BITWISE_FRONTIER_COST_DELTA: AtomicI64 = AtomicI64::new(0);

/// Read the process-wide bitwise-frontier counters.
pub fn bitwise_frontier_stats() -> BitwiseFrontierStats {
    BitwiseFrontierStats {
        attempts: BITWISE_FRONTIER_ATTEMPTS.load(Ordering::Relaxed),
        successes: BITWISE_FRONTIER_SUCCESSES.load(Ordering::Relaxed),
        aborts_atom_limit: BITWISE_FRONTIER_ABORTS_ATOM_LIMIT.load(Ordering::Relaxed),
        aborts_size_limit: BITWISE_FRONTIER_ABORTS_SIZE_LIMIT.load(Ordering::Relaxed),
        max_frontier_atoms: BITWISE_FRONTIER_MAX_ATOMS.load(Ordering::Relaxed),
        candidate_node_delta: BITWISE_FRONTIER_NODE_DELTA.load(Ordering::Relaxed),
        candidate_cost_delta: BITWISE_FRONTIER_COST_DELTA.load(Ordering::Relaxed),
    }
}

/// Reset bitwise-frontier counters before an isolated measurement.
pub fn reset_bitwise_frontier_stats() {
    BITWISE_FRONTIER_ATTEMPTS.store(0, Ordering::Relaxed);
    BITWISE_FRONTIER_SUCCESSES.store(0, Ordering::Relaxed);
    BITWISE_FRONTIER_ABORTS_ATOM_LIMIT.store(0, Ordering::Relaxed);
    BITWISE_FRONTIER_ABORTS_SIZE_LIMIT.store(0, Ordering::Relaxed);
    BITWISE_FRONTIER_MAX_ATOMS.store(0, Ordering::Relaxed);
    BITWISE_FRONTIER_NODE_DELTA.store(0, Ordering::Relaxed);
    BITWISE_FRONTIER_COST_DELTA.store(0, Ordering::Relaxed);
}

fn record_bitwise_frontier_metrics(
    metrics: &BitwiseFrontierMetrics,
    accepted: bool,
    node_delta: isize,
    cost_delta: isize,
) {
    BITWISE_FRONTIER_ATTEMPTS.fetch_add(metrics.attempts as u64, Ordering::Relaxed);
    BITWISE_FRONTIER_SUCCESSES.fetch_add(u64::from(accepted), Ordering::Relaxed);
    BITWISE_FRONTIER_ABORTS_ATOM_LIMIT
        .fetch_add(metrics.aborts_atom_limit as u64, Ordering::Relaxed);
    BITWISE_FRONTIER_ABORTS_SIZE_LIMIT
        .fetch_add(metrics.aborts_size_limit as u64, Ordering::Relaxed);
    BITWISE_FRONTIER_MAX_ATOMS.fetch_max(metrics.max_frontier_atoms as u64, Ordering::Relaxed);
    BITWISE_FRONTIER_NODE_DELTA.fetch_add(node_delta as i64, Ordering::Relaxed);
    BITWISE_FRONTIER_COST_DELTA.fetch_add(cost_delta as i64, Ordering::Relaxed);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExprDomain {
    Arithmetic,
    Bitwise,
}

fn expr_domain(e: &Expr) -> Option<ExprDomain> {
    match e {
        Expr::Not(_) | Expr::And(_) | Expr::Or(_) | Expr::Xor(_) => Some(ExprDomain::Bitwise),
        Expr::Scale(_, _) | Expr::Add(_) | Expr::Mul(_) => Some(ExprDomain::Arithmetic),
        Expr::Var(_) | Expr::Const(_) => None,
    }
}

fn arithmetic_bitwise_alternations(e: &Expr, parent: Option<ExprDomain>) -> usize {
    let domain = expr_domain(e);
    let alternation = usize::from(domain.is_some() && parent.is_some() && domain != parent);
    let parent = domain.or(parent);

    let children = match e {
        Expr::Var(_) | Expr::Const(_) => 0,
        Expr::Not(e) | Expr::Scale(_, e) => arithmetic_bitwise_alternations(e, parent),
        Expr::And(es)
        | Expr::Or(es)
        | Expr::Xor(es)
        | Expr::Add(es)
        | Expr::Mul(es) => es
            .iter()
            .map(|e| arithmetic_bitwise_alternations(e, parent))
            .sum(),
    };

    alternation + children
}

fn expr_cost(e: &Expr) -> ExprCost {
    ExprCost {
        arithmetic_bitwise_alternations: arithmetic_bitwise_alternations(e, None),
        ast_nodes: e.size(),
        printed_size: e.to_string().len(),
    }
}

/// Why the solver could not simplify an expression.
///
/// Simplifying an MBA is best-effort: a caller that hands over an expression the
/// solver cannot handle should be able to keep its original expression and carry
/// on, not die. Every variant is a "leave this one alone" signal rather than a
/// reason to abort the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveError {
    /// The linear MBA still held more than [`MAX_VARS`] variables after
    /// reduction / PCT expansion, so its truth table is too large to build.
    TooManyVariables { found: usize, max: usize },

    /// A variable produced during reduction had no entry in the restore map.
    /// Indicates an inconsistent variable map rather than a hard input.
    UnknownVariable(VarId),

    /// A solved linear MBA came back in a shape the polynomial reconstruction
    /// does not model.
    UnrecognizedForm(Expr),
}

impl std::fmt::Display for SolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveError::TooManyVariables { found, max } => {
                write!(f, "too many variables: {found} (max {max})")
            }
            SolveError::UnknownVariable(v) => write!(f, "unknown variable v{v}"),
            SolveError::UnrecognizedForm(e) => {
                write!(f, "solved linear MBA is in an unrecognized form: {e}")
            }
        }
    }
}

impl std::error::Error for SolveError {}

/// How a cache has performed. Read with [`MbaCache::stats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    /// Distinct linear MBAs currently memoized.
    pub entries: usize,
}

impl CacheStats {
    /// Fraction of lookups served from the cache, or `None` before any lookup.
    pub fn hit_rate(&self) -> Option<f64> {
        let total = self.hits + self.misses;
        (total > 0).then(|| self.hits as f64 / total as f64)
    }
}

/// A memo of solved linear MBAs.
///
/// The solver reaches its cache at exactly one point
/// ([`MBASolver::solve_linear`]): a [`get`](LinearCache::get), and on a miss an
/// [`insert`](LinearCache::insert), with the expensive solve running *between*
/// them. Two implementations are provided: [`LocalCache`] for a single-threaded
/// caller, and [`MbaCache`] for one shared across threads.
///
/// [`get`](LinearCache::get) hands back an owned `Expr` rather than a guard or a
/// borrow. That is deliberate: the solver recurses into itself (`hide_in_var`
/// re-enters `simplify_mba_inner` with this same cache), and neither [`RefCell`]
/// nor [`Mutex`] tolerates a live guard across such a call — one panics, the
/// other deadlocks. Returning owned values makes that unrepresentable.
pub trait LinearCache {
    /// The memoized solution for `e`, tallying the lookup as a hit or a miss.
    fn get(&self, e: &Expr) -> Option<Expr>;

    /// Memoize `solved` as the solution for `e`.
    fn insert(&self, e: Expr, solved: Expr);

    /// Hit/miss tallies and current size.
    fn stats(&self) -> CacheStats;

    /// Drop every entry and reset the tallies.
    fn clear(&self);
}

/// A [`LinearCache`] for one thread: no locking, no atomics.
///
/// This is what [`simplify_mba`] uses. Accessing it costs a borrow-flag check,
/// so a single-threaded caller pays nothing for a sharing capability it does not
/// use. Not [`Sync`] — use [`MbaCache`] to share one across threads.
#[derive(Debug, Default)]
pub struct LocalCache {
    entries: RefCell<HashMap<Expr, Expr>>,
    hits: Cell<u64>,
    misses: Cell<u64>,
}

impl LocalCache {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LinearCache for LocalCache {
    fn get(&self, e: &Expr) -> Option<Expr> {
        let hit = self.entries.borrow().get(e).cloned();

        let counter = match &hit {
            Some(_) => &self.hits,
            None => &self.misses,
        };
        counter.set(counter.get() + 1);

        hit
    }

    fn insert(&self, e: Expr, solved: Expr) {
        self.entries.borrow_mut().insert(e, solved);
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.get(),
            misses: self.misses.get(),
            entries: self.entries.borrow().len(),
        }
    }

    fn clear(&self) {
        self.entries.borrow_mut().clear();
        self.hits.set(0);
        self.misses.set(0);
    }
}

/// A [`LinearCache`] shareable across threads, and across calls to
/// [`simplify_mba_with_cache`].
///
/// Critical sections are one hash-map operation each — the solve itself runs
/// outside the lock — so contention stays low even with many workers.
///
/// A caller that never shares should still prefer [`LocalCache`]. The lock and
/// the atomics add roughly 35ns per lookup (~99ns to ~134ns, `benches/cache.rs`).
/// Against a miss, which pays for a full solve, that is nothing; against a hit,
/// which is only an `Expr` hash, it is about a third — and a cache exists to be
/// hit. Measured end to end through the solver the difference came out near 8%
/// on an all-hits workload.
#[derive(Debug, Default)]
pub struct MbaCache {
    entries: Mutex<HashMap<Expr, Expr>>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl MbaCache {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LinearCache for MbaCache {
    fn get(&self, e: &Expr) -> Option<Expr> {
        let hit = self
            .entries
            .lock()
            .expect("MBA cache mutex poisoned")
            .get(e)
            .cloned();

        match &hit {
            Some(_) => &self.hits,
            None => &self.misses,
        }
        .fetch_add(1, Ordering::Relaxed);

        hit
    }

    fn insert(&self, e: Expr, solved: Expr) {
        self.entries
            .lock()
            .expect("MBA cache mutex poisoned")
            .insert(e, solved);
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            entries: self.entries.lock().expect("MBA cache mutex poisoned").len(),
        }
    }

    fn clear(&self) {
        self.entries
            .lock()
            .expect("MBA cache mutex poisoned")
            .clear();
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
    }
}

fn sub_coeff(tt: &mut [u64], coeff: u64, index: usize, sublist: &[usize]) {
    let are_vars_true = |i: usize| sublist[1..].iter().copied().all(|v| ((i >> v) & 1) == 1);

    let gp_size = 1usize << sublist[0];
    let period = 2 * gp_size;

    let mut start = index;
    while start < tt.len() {
        for (i, e) in tt.iter_mut().enumerate().skip(start).take(gp_size) {
            if sublist.len() == 1 || are_vars_true(i) {
                *e = e.wrapping_sub(coeff);
            }
        }
        start += period;
    }
}

fn get_signed(x: u64, n: u8) -> i64 {
    let value = x & make_mask(n);
    let shift = 64 - n;
    ((value << shift) as i64) >> shift // arithmetic shift
}

fn find_lambda_int(x: &[u64], y: &[u64], a: i64, b: i64, n: u8) -> Option<u64> {
    let mut valid: Option<(Option<i64>, Option<i64>)> = None;

    for (&xi, &yi) in x.iter().zip(y.iter()) {
        let xi = get_signed(xi, n);
        let yi = get_signed(yi, n);

        if yi == 0 {
            if xi == a || xi == b {
                continue;
            } else {
                return None;
            }
        }

        let mut vals = [None, None];

        if (xi.wrapping_sub(a)) % yi == 0 {
            vals[0] = Some((xi.wrapping_sub(a)) / yi);
        }

        if (xi.wrapping_sub(b)) % yi == 0 {
            vals[1] = Some((xi.wrapping_sub(b)) / yi);
        }

        match valid {
            None => valid = Some((vals[0], vals[1])),
            Some((va, vb)) => {
                let mut new = (None, None);
                for v in vals.iter().flatten() {
                    if va == Some(*v) || vb == Some(*v) {
                        if new.0.is_none() {
                            new.0 = Some(*v);
                        } else {
                            new.1 = Some(*v);
                        }
                    }
                }
                valid = Some(new);
            }
        }

        if let Some((None, None)) = valid {
            debug!("OVERCONSTRAINED");
            return None;
        }
    }

    if let Some(valid) = valid {
        let mask = make_mask(n);

        if let Some(v) = valid.0
            && get_signed(x[0], n).wrapping_sub(v.wrapping_mul(get_signed(y[0], n))) == a
        {
            return Some((v as u64) & mask);
        }

        if let Some(v) = valid.1
            && get_signed(x[0], n).wrapping_sub(v.wrapping_mul(get_signed(y[0], n))) == a
        {
            return Some((v as u64) & mask);
        }

        None
    } else {
        // the vector is null
        Some(0)
    }
}

/// Reduces the number of variables present in the MBA
fn reduce_vars(e: Expr, var_map: &mut BiMap<VarId, VarId>, t: &mut usize) -> Expr {
    match e {
        Expr::Var(v) => {
            let vv = if let Some(v) = var_map.get_by_left(&v) {
                *v
            } else {
                let vv = (*t).into();
                var_map.insert(v, vv);
                *t += 1;
                vv
            };
            Expr::Var(vv)
        }

        _ => e.map(|e| reduce_vars(e, var_map, t)),
    }
}

/// Restores the varialbes in the mba
fn restore_vars(e: Expr, var_map: &BiMap<VarId, VarId>) -> Result<Expr, SolveError> {
    match e {
        Expr::Var(v) => {
            let vv = var_map
                .get_by_right(&v)
                .copied()
                .ok_or(SolveError::UnknownVariable(v))?;
            Ok(Expr::Var(vv))
        }

        _ => e.try_map(|e| restore_vars(e, var_map)),
    }
}

struct MBASolver<'a, C: LinearCache> {
    /// The number of bits being considered
    n: u8,

    /// The mask that corresponds to the given bitsize
    mask: u64,

    /// The map between non linear components and variables
    non_linear_components: BiMap<VarId, Expr>,

    /// The number of variables in the expression
    t: usize,

    /// The degree of the polynomial expression
    degree: usize,

    /// A cache for simplifying linear MBAs
    l_cache: &'a C,

    /// Present only while `diagnose_hidden_atoms` is collecting this invocation.
    trace_scope: Option<usize>,
}

fn is_bitwise_constant(c: VarInt, mask: u64) -> bool {
    let c = c.get(mask);
    c == 0 || c == mask
}

impl<'a, C: LinearCache> MBASolver<'a, C> {
    /// Create a new Solver
    fn new(l_cache: &'a C, e: &Expr, n: u8) -> Self {
        let trace_scope = begin_hidden_trace_scope(e, n);
        Self {
            non_linear_components: BiMap::new(),
            t: e.get_vars().iter().copied().map(|v| v.0).max().unwrap_or(0) + 1,
            degree: 1,
            n,
            mask: make_mask(n),
            l_cache,
            trace_scope,
        }
    }

    /// Replaces non polynomial variables by their hidden expressions
    fn poly_to_nonpoly(&self, e: Expr) -> Expr {
        match &e {
            Expr::Var(v) => {
                if let Some(e) = self.non_linear_components.get_by_left(v) {
                    e.clone()
                } else {
                    e
                }
            }

            _ => e.map(|e| self.poly_to_nonpoly(e)),
        }
    }

    /// Solves a non polynomial MBA
    fn solve(&mut self, e: Expr) -> Result<Expr, SolveError> {
        // TODO: Remove this only needs to be done once
        let e = e.reduce(self.mask);

        let p = self.make_polynomial(e)?;
        let p = self.solve_polynomial(p)?;
        record_pre_restore_result(self.trace_scope, &p);

        // This was a non linear MBA
        if self.non_linear_components.len() != 0 {
            let e = self.poly_to_nonpoly(p);
            debug!("After adding non linear components, found: {}", e);
            Ok(e.reduce(self.mask))
        } else {
            Ok(p)
        }
    }

    /// Calcluates the signature of a linear MBA
    fn calc_signature(&self, e: &Expr, t: usize) -> Vec<u64> {
        e.truth_table(t, self.mask)
    }

    /// Creates a conjuction sum for the given signature
    fn make_conjunction_sum(&self, mut signature: Vec<u64>, t: usize) -> Expr {
        let mut terms: Vec<Expr> = vec![];

        // The constant term
        let constant = signature[0];

        if constant != 0 {
            terms.push(Expr::Const(constant.into()));

            for v in &mut signature {
                *v = v.wrapping_sub(constant);
            }
        }

        let mut sublist = Vec::with_capacity(t);
        for index in 1..(1usize << t) {
            let coeff = signature[index] & self.mask;

            if coeff == 0 {
                continue;
            }

            sublist.clear();
            for i in 0..t {
                if ((index >> i) & 1) == 1 {
                    sublist.push(i);
                }
            }
            let conjunction = Expr::And(
                sublist
                    .iter()
                    .copied()
                    .map(|v| Expr::Var(v.into()))
                    .collect(),
            );

            terms.push(match coeff {
                1 => conjunction,
                c => c * conjunction,
            });

            sub_coeff(&mut signature, coeff, index, &sublist);
        }

        match terms.len() {
            0 => Expr::zero(),
            1 => terms.into_iter().next().unwrap(),
            _ => Expr::Add(terms),
        }
    }

    fn abstract_bitwise_frontier(
        &self,
        e: &Expr,
        atoms: &mut Vec<Expr>,
        atom_ids: &mut HashMap<Expr, VarId>,
        metrics: &mut BitwiseFrontierMetrics,
    ) -> Option<Expr> {
        let map_terms = |terms: &[Expr],
                         atoms: &mut Vec<Expr>,
                         atom_ids: &mut HashMap<Expr, VarId>,
                         metrics: &mut BitwiseFrontierMetrics| {
            terms
                .iter()
                .map(|e| self.abstract_bitwise_frontier(e, atoms, atom_ids, metrics))
                .collect::<Option<Vec<_>>>()
        };

        Some(match e {
            Expr::Not(e) => !self.abstract_bitwise_frontier(e, atoms, atom_ids, metrics)?,
            Expr::And(es) => Expr::And(map_terms(es, atoms, atom_ids, metrics)?),
            Expr::Or(es) => Expr::Or(map_terms(es, atoms, atom_ids, metrics)?),
            Expr::Xor(es) => Expr::Xor(map_terms(es, atoms, atom_ids, metrics)?),
            Expr::Const(c) if is_bitwise_constant(*c, self.mask) => e.clone(),
            _ => {
                let key = e.clone().reduce(self.mask);
                let id = if let Some(id) = atom_ids.get(&key) {
                    *id
                } else {
                    if atoms.len() == MAX_FRONTIER_ATOMS {
                        metrics.aborts_atom_limit += 1;
                        return None;
                    }

                    let id = atoms.len().into();
                    atoms.push(key.clone());
                    atom_ids.insert(key, id);
                    id
                };
                Expr::Var(id)
            }
        })
    }

    fn restore_frontier_atoms(&self, e: Expr, atoms: &[Expr]) -> Option<Expr> {
        match e {
            Expr::Var(v) => atoms.get(v.0).cloned(),
            _ => e
                .try_map(|e| self.restore_frontier_atoms(e, atoms).ok_or(()))
                .ok(),
        }
    }

    fn normalize_one_bitwise_frontier(
        &self,
        e: &Expr,
        metrics: &mut BitwiseFrontierMetrics,
    ) -> Option<Expr> {
        metrics.attempts += 1;

        let mut atoms = Vec::new();
        let mut atom_ids = HashMap::new();
        let abstracted = self.abstract_bitwise_frontier(
            e,
            &mut atoms,
            &mut atom_ids,
            metrics,
        )?;
        metrics.max_frontier_atoms = max(metrics.max_frontier_atoms, atoms.len());

        if !atoms.iter().any(Expr::is_arithmetic) {
            return None;
        }

        let signature_size = 1usize << atoms.len();
        if signature_size > MAX_FRONTIER_SIGNATURE_SIZE {
            metrics.aborts_atom_limit += 1;
            return None;
        }

        let signature = self.calc_signature(&abstracted, atoms.len());
        let normalized = self.make_conjunction_sum(signature, atoms.len());
        let restored = self.restore_frontier_atoms(normalized, &atoms)?.reduce(self.mask);

        if restored.size() > e.size().saturating_mul(MAX_TRANSFORMED_AST_FACTOR) {
            metrics.aborts_size_limit += 1;
            return None;
        }

        if restored == *e {
            return None;
        }

        metrics.normalized += 1;
        Some(restored)
    }

    fn rewrite_bitwise_frontiers(
        &self,
        e: &Expr,
        metrics: &mut BitwiseFrontierMetrics,
    ) -> Expr {
        match e {
            Expr::Not(_) | Expr::And(_) | Expr::Or(_) | Expr::Xor(_) => self
                .normalize_one_bitwise_frontier(e, metrics)
                .unwrap_or_else(|| e.clone()),
            _ => e
                .clone()
                .map(|e| self.rewrite_bitwise_frontiers(&e, metrics)),
        }
    }

    fn normalize_bitwise_frontier(&self, e: &Expr) -> Option<Expr> {
        let mut metrics = BitwiseFrontierMetrics::default();
        let rewritten = self.rewrite_bitwise_frontiers(e, &mut metrics);

        if metrics.normalized == 0 {
            record_bitwise_frontier_metrics(&metrics, false, 0, 0);
            return None;
        }

        let candidate = rewritten.reduce(self.mask);
        if candidate == *e {
            record_bitwise_frontier_metrics(&metrics, false, 0, 0);
            return None;
        }

        let original_cost = expr_cost(e);
        let candidate_cost = expr_cost(&candidate);
        let node_delta = candidate.size() as isize - e.size() as isize;
        let cost_delta = candidate_cost.arithmetic_bitwise_alternations as isize
            - original_cost.arithmetic_bitwise_alternations as isize;
        let exceeds_size_budget =
            candidate.size() > e.size().saturating_mul(MAX_TRANSFORMED_AST_FACTOR);

        if exceeds_size_budget {
            metrics.aborts_size_limit += 1;
        }

        let accepted = !exceeds_size_budget
            && (candidate == Expr::zero() || candidate_cost < original_cost);
        record_bitwise_frontier_metrics(
            &metrics,
            accepted,
            if accepted { node_delta } else { 0 },
            if accepted { cost_delta } else { 0 },
        );

        debug!(
            "bitwise-frontier attempts={} normalized={} atom_aborts={} size_aborts={} max_atoms={} node_delta={} cost_delta={:?}->{:?} accepted={}",
            metrics.attempts,
            metrics.normalized,
            metrics.aborts_atom_limit,
            metrics.aborts_size_limit,
            metrics.max_frontier_atoms,
            node_delta,
            original_cost,
            candidate_cost,
            accepted
        );

        accepted.then_some(candidate)
    }

    /// Solves a linear MBA
    fn solve_linear_inner(&self, e: Expr, t: usize, from_poly: bool) -> Expr {
        let signature = self.calc_signature(&e, t);

        if from_poly {
            // We necessarily want a sum of conjunctions
            return self.make_conjunction_sum(signature, t);
        }

        // TODO: add a refined solution to identify xor etc
        self.make_conjunction_sum(signature, t)
    }

    /// Simplifies a linear MBA
    fn solve_linear(&mut self, e: Expr, from_poly: bool) -> Result<Expr, SolveError> {
        let mut var_map = BiMap::<VarId, VarId>::new();
        let mut t = 0;

        debug!("Solving linear MBA: {}", e);

        // Reduce the number of variables in the expression
        let e = reduce_vars(e, &mut var_map, &mut t);
        debug!("Reduced number of variables to equivalent problem: {}", e);

        let e = if let Some(simplified) = self.l_cache.get(&e) {
            debug!("Found linear MBA in cache");
            simplified
        } else {
            debug!("Solving linear MBA");

            if t > MAX_VARS {
                return Err(SolveError::TooManyVariables {
                    found: t,
                    max: MAX_VARS,
                });
            }

            let simplified = self.solve_linear_inner(e.clone(), t, from_poly);
            self.l_cache.insert(e, simplified.clone());
            simplified
        };

        let e = restore_vars(e, &var_map)?;

        debug!("Found solution to linear MBA: {}", e);

        Ok(e)
    }

    /// Turns a polynomial MBA to a linear one using PCT
    fn poly_to_linear(&self, e: Expr, deg: usize) -> Expr {
        match e {
            Expr::Var(v) => Expr::Var(((deg - 1) * self.t + v.0).into()),

            Expr::Mul(terms) => {
                // The sign correction -> see paper
                let s = if (self.degree - terms.len()) & 1 == 0 {
                    1
                } else {
                    u64::MAX
                };

                s * Expr::And(
                    terms
                        .into_iter()
                        .enumerate()
                        .map(|(i, e)| self.poly_to_linear(e, deg + i + 1))
                        .collect(),
                )
            }

            Expr::Const(c) => {
                // This shouldn't be done in multiplications, only constants in the addition
                if deg != 0 {
                    e
                } else {
                    // The sign correction -> see paper
                    let s = if self.degree & 1 == 0 {
                        VarInt::MAX
                    } else {
                        VarInt::ONE
                    };
                    Expr::Const(s * c)
                }
            }

            _ => e.map(|e| self.poly_to_linear(e, deg)),
        }
    }

    /// Turns a linear MBA into a polynomial one using the inverse PCT
    fn linear_to_poly(&self, e: Expr) -> Result<Expr, SolveError> {
        match &e {
            Expr::And(terms) => {
                // The sign correction -> see paper
                let mut s = 1u64;

                let mut grouped: Vec<Vec<usize>> = vec![vec![]; self.degree];

                for t in terms {
                    if let Expr::Var(v) = t {
                        let d = v.0 / self.t;
                        grouped[d].push(v.0 % self.t);
                    } else {
                        return Err(SolveError::UnrecognizedForm(e.clone()));
                    }
                }

                let mut terms: Vec<Expr> = vec![];

                for g in grouped {
                    if g.is_empty() {
                        s = s.wrapping_mul(u64::MAX);
                        continue;
                    }

                    terms.push(Expr::And(
                        g.into_iter().map(|v| Expr::Var(v.into())).collect(),
                    ));
                }

                Ok(s * Expr::Mul(terms))
            }

            Expr::Add(_) | Expr::Scale(_, _) => e.try_map(|e| self.linear_to_poly(e)),

            Expr::Var(v) => {
                // The sign correction -> see paper
                let s = if self.degree & 1 == 0 { u64::MAX } else { 1 };
                Ok(s * Expr::Var((v.0 % self.t).into()))
            }

            Expr::Const(_) => {
                // The sign correction -> see paper
                let s = if self.degree & 1 == 0 { u64::MAX } else { 1 };
                Ok(s * e)
            }

            _ => Err(SolveError::UnrecognizedForm(e.clone())),
        }
    }

    /// Solves a polynomial MBA
    fn solve_polynomial(&mut self, e: Expr) -> Result<Expr, SolveError> {
        debug!("Solving polynomial MBA: {}", e);

        // This is a linear MBA
        if self.degree == 1 {
            debug!("This is a linear MBA");
            return self.solve_linear(e, false);
        }

        let e = self.poly_to_linear(e, 0);
        let e: Expr = self.solve_linear(e, true)?;
        let e = self.linear_to_poly(e)?.reduce(self.mask);

        debug!("Found polynomial solution: {}", e);

        Ok(e)
    }

    /// Hides a non linear element behind a variable
    fn hide_in_var(&mut self, e: Expr, mask: u64) -> Result<Expr, SolveError> {
        debug!("e={} is not linear and will be replaced by a variable", e);
        let original = e.clone();

        let e = match e {
            Expr::Const(_) => e,
            _ => simplify_mba_inner(self.l_cache, e, mask.count_ones() as u8)?.reduce(mask),
        };

        let note = (-e.clone() - Expr::make_const(1)).reduce(mask);
        debug!("!e would be {}", note);

        if let Some(v) = self.non_linear_components.get_by_right(&e) {
            debug!("Found variable v{} for e", v);
            Ok(Expr::Var(*v))
        } else if let Some(v) = self.non_linear_components.get_by_right(&note) {
            debug!("Found variable v{} for !e", v);
            Ok(!Expr::Var(*v))
        } else {
            let v = self.t.into();
            debug!("Creating variable v{} for e={}", v, e);
            self.t += 1;
            self.non_linear_components.insert(v, e.clone());
            record_hidden_atom(self.trace_scope, v, original, e);
            Ok(Expr::Var(v))
        }
    }

    fn is_signature_bitwise(&self, s: &Vec<u64>, mask: u64) -> bool {
        let minus_one = mask;
        let minus_two = mask - 1;

        if s[0] == 0 {
            if s.iter().all(|&x| x == 0 || x == 1) {
                debug!("Signature is {:?} in [0, 1].", s);
                true
            } else {
                false
            }
        } else if s[0] == minus_one {
            if s.iter().all(|&x| x == minus_one || x == minus_two) {
                debug!("Signature is {:?} in [-1, 2]", s);
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    // Read paper
    fn variable_substitution(&self, e: Expr) -> Option<Expr> {
        let mut vars = vec![];
        let mut sub_vars = vec![];

        for v in e.get_vars() {
            if self.non_linear_components.get_by_left(&v).is_some() {
                sub_vars.push(v);
            } else {
                vars.push(v);
            }
        }

        // TODO: allow 2 variable substitutions
        if sub_vars.len() != 1 {
            return None;
        }

        let sub_var = sub_vars[0];
        let ee = self.non_linear_components.get_by_left(&sub_var).unwrap();

        if !self.is_linear(ee) {
            return None;
        }

        debug!("While checking if {} is linear", e);
        debug!("Proceding with advanced variable substitution");
        debug!("Found substitution v{} = {}", sub_var, ee);

        // This vector is null
        let ee = Expr::Var(sub_var) - ee.clone();
        debug!("Using zero expression {}", ee);

        for v in ee.get_vars() {
            if !vars.contains(&v) {
                vars.push(v);
            }
        }

        let mut var_map = BiMap::new();
        let mut t = 0;

        let reduced_e = reduce_vars(e.clone(), &mut var_map, &mut t);
        let reduced_ee = reduce_vars(ee.clone(), &mut var_map, &mut t);

        let se = self.calc_signature(&reduced_e, t);
        debug!("Using signature {:?}", se);
        let see = self.calc_signature(&reduced_ee, t);
        debug!("Using zero signature {:?}", see);

        if let Some(lambda) = find_lambda_int(&se, &see, 0, 1, self.n) {
            debug!("Found lambda that creates a [0, 1] signature: {:?}", lambda);
            Some(e - lambda * ee)
        } else if let Some(lambda) = find_lambda_int(&se, &see, -1, -2, self.n) {
            debug!(
                "Found lambda that creates a [-1, -2] signature: {:?}",
                lambda
            );
            debug!("Using zero signature {:?}", see);
            Some(e - lambda * ee)
        } else {
            None
        }
    }

    // A Linear MBA might "hide" a bitwise expression
    fn is_linear_bitwise(&self, l: Expr, mask: u64) -> Option<Expr> {
        let mut t = 0;
        let mut var_map = BiMap::new();
        let e = reduce_vars(l.clone(), &mut var_map, &mut t);

        if t > 10 {
            // This would be too expensive
            return None;
        }

        let s = e.truth_table(t, mask);

        if self.is_signature_bitwise(&s, mask) {
            debug!("Will treat {} as a bitwise expression", e);
            Some(l)
        } else {
            // Attempt to "fix" the signature with a variable substitution
            self.variable_substitution(l)
        }
    }

    /// Turns an expression into a bitwise expression
    fn make_bitwise(&mut self, e: Expr, mut mask: u64) -> Result<Expr, SolveError> {
        match e {
            // -1 and 0 are bitwise
            Expr::Const(c) => {
                if c.get(mask) == 0 || c.get(mask) == self.mask {
                    Ok(e)
                } else {
                    self.hide_in_var(e, mask)
                }
            }

            // Variables are bitwise
            Expr::Var(_) => Ok(e),

            Expr::Not(_) | Expr::Or(_) | Expr::Xor(_) => {
                // if only bitwise
                // self
                // if has negatives, try and fix the "biphased" problem
                // worst case
                e.try_map(|e| self.make_bitwise(e, mask))
            }

            // Dynamic masking: if we and with a constant that constant will be are new mask
            Expr::And(terms) => {
                for e in &terms {
                    if let Expr::Const(c) = e {
                        let c = c.get(mask);
                        if c & (c.wrapping_add(1)) == 0 {
                            debug!("Found dynamic mask {} = 2^{} -1", c, c.count_ones());
                            mask = c;
                        }
                    }
                }

                if mask == 0 {
                    return Ok(Expr::zero());
                }

                Ok(Expr::And(
                    terms
                        .into_iter()
                        .map(|e| self.make_bitwise(e, mask))
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            }

            // A Linear MBA might "hide" a bitwise expression
            Expr::Add(_) | Expr::Scale(_, _) => {
                let previous = e.clone();
                let l = e.try_map(|e| self.make_linear(e, mask))?;
                if let Some(l) = self.is_linear_bitwise(l, mask) {
                    Ok(l)
                } else {
                    self.hide_in_var(previous, mask)
                }
            }

            _ => self.hide_in_var(e, mask),
        }
    }

    /// Turns an expression into a bitwise product
    fn make_product(&mut self, e: Expr) -> Result<Expr, SolveError> {
        match &e {
            Expr::Mul(terms) => {
                self.degree = max(self.degree, terms.len());
                e.try_map(|e| self.make_bitwise(e, self.mask))
            }

            // This makes life better on much easier
            _ => Ok(Expr::Mul(vec![self.make_bitwise(e, self.mask)?])),
        }
    }

    /// Turns an expression into a scaled bitwise product
    fn make_scaled_product(&mut self, e: Expr) -> Result<Expr, SolveError> {
        match e {
            Expr::Const(_) => Ok(e),
            Expr::Scale(_, _) => e.try_map(|e| self.make_product(e)),
            _ => self.make_product(e),
        }
    }

    /// Turns an expression into a polynomial expression
    fn make_polynomial(&mut self, e: Expr) -> Result<Expr, SolveError> {
        match e {
            Expr::Add(_) => e.try_map(|e| self.make_scaled_product(e)),
            _ => self.make_scaled_product(e),
        }
    }

    /// Turns an expression into a scaled bitwise expression
    fn make_scaled_bitwise(&mut self, e: Expr, mask: u64) -> Result<Expr, SolveError> {
        match e {
            Expr::Const(_) => Ok(e),
            Expr::Scale(_, _) => e.try_map(|e| self.make_bitwise(e, mask)),
            _ => self.make_bitwise(e, mask),
        }
    }

    /// Turns an expression into a linear expression
    fn make_linear(&mut self, e: Expr, mask: u64) -> Result<Expr, SolveError> {
        match e {
            Expr::Add(_) => e.try_map(|e| self.make_scaled_bitwise(e, mask)),
            _ => self.make_scaled_bitwise(e, mask),
        }
    }

    fn is_linear(&self, e: &Expr) -> bool {
        fn is_bitwise(e: &Expr, mask: u64) -> bool {
            match e {
                // -1 and 0 are bitwise
                Expr::Const(c) => is_bitwise_constant(*c, mask),

                // Variables are bitwise
                Expr::Var(_) => true,

                // A boolean expression is bitwise if all its sub expressions are bitwise
                Expr::Not(e) => is_bitwise(e, mask),

                Expr::And(es) | Expr::Or(es) | Expr::Xor(es) => {
                    es.iter().all(|e| is_bitwise(e, mask))
                }

                _ => false,
            }
        }

        fn is_scaled_bitwise(e: &Expr, mask: u64) -> bool {
            match e {
                Expr::Const(_) => true,
                Expr::Scale(_, e) => is_bitwise(e, mask),
                _ => is_bitwise(e, mask),
            }
        }

        match e {
            Expr::Add(terms) => terms.iter().all(|e| is_scaled_bitwise(e, self.mask)),
            _ => is_scaled_bitwise(e, self.mask),
        }
    }
}

fn simplify_mba_inner<C: LinearCache>(l_cache: &C, e: Expr, n: u8) -> Result<Expr, SolveError> {
    let mut solver = MBASolver::new(l_cache, &e, n);
    solver.solve(e)
}

// I should probably add a flag for recursive simplification
pub fn simplify_mba(e: Expr, n: u8) -> Result<Expr, SolveError> {
    simplify_mba_with_cache(&LocalCache::new(), e, n)
}

/// Simplify one expression while collecting the hidden atoms created by each
/// recursive solver invocation. This is an opt-in diagnostic; ordinary calls
/// to [`simplify_mba`] do not retain traces.
pub fn diagnose_hidden_atoms(
    e: Expr,
    n: u8,
) -> Result<(Expr, Vec<HiddenScopeTrace>), SolveError> {
    HIDDEN_TRACE_STATE.with(|state| {
        let mut state = state.borrow_mut();
        assert!(state.is_none(), "hidden-atom diagnostics cannot be nested");
        *state = Some(HiddenTraceState::default());
    });

    let result = simplify_mba(e, n);
    let state = HIDDEN_TRACE_STATE.with(|state| {
        state
            .borrow_mut()
            .take()
            .expect("hidden-atom diagnostic state disappeared")
    });
    let scopes = finalize_hidden_trace(state)
        .into_iter()
        .filter(|scope| !scope.atoms.is_empty())
        .collect();

    result.map(|simplified| (simplified, scopes))
}

fn simplify_to_fixed_point<F>(mut e: Expr, mut simplify: F) -> Result<Expr, SolveError>
where
    F: FnMut(Expr) -> Result<Expr, SolveError>,
{
    let mut seen = HashSet::from([e.clone()]);
    let mut best = e.clone();

    for _ in 0..MAX_SIMPLIFICATION_PASSES {
        let next = simplify(e.clone())?;
        debug!("e: {}", next);

        if next.size() < best.size() {
            best = next.clone();
        }

        if next == e {
            return Ok(next);
        }

        if !seen.insert(next.clone()) {
            debug!("simplification cycle detected; retaining best expression");
            return Ok(best);
        }

        e = next;
    }

    debug!("simplification pass limit reached; retaining best expression");
    Ok(best)
}

/// [`simplify_mba`] against a caller-owned cache.
///
/// Solved linear MBAs are memoized in `cache`, so a caller that simplifies many
/// expressions — or the same expressions repeatedly across rounds of an analysis
/// — pays for each distinct linear solve once. Pass a [`LocalCache`] to reuse
/// results on one thread, or an [`MbaCache`] to share them across several.
pub fn simplify_mba_with_cache<C: LinearCache>(
    cache: &C,
    e: Expr,
    n: u8,
) -> Result<Expr, SolveError> {
    let mask = make_mask(n);
    let e = e.reduce(mask);

    simplify_to_fixed_point(e, |e| {
        let frontier_solver = MBASolver::new(cache, &e, n);
        let e = frontier_solver.normalize_bitwise_frontier(&e).unwrap_or(e);
        simplify_mba_inner(cache, e, n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_zero_and_all_ones_as_bitwise_constants() {
        let mask = make_mask(8);
        assert!(is_bitwise_constant(VarInt::ZERO, mask));
        assert!(is_bitwise_constant(VarInt::MAX, mask));
    }

    #[test]
    fn inverse_pct_round_trip_preserves_variable_index() {
        let cache = LocalCache::new();
        let mut solver = MBASolver::new(&cache, &Expr::Var(2.into()), 8);
        solver.degree = 2;
        let original = Expr::Var(2.into());
        let encoded = solver.poly_to_linear(original.clone(), 2);

        assert_eq!(encoded, Expr::Var(5.into()));
        let result = solver.linear_to_poly(encoded).unwrap();

        assert_eq!(result, u64::MAX * original);
    }

    #[test]
    fn fixed_point_continues_when_structure_changes_at_equal_size() {
        let result = simplify_to_fixed_point(Expr::Var(0.into()), |e| {
            Ok(match e {
                Expr::Var(VarId(0)) => !Expr::Var(1.into()),
                Expr::Not(inner) if *inner == Expr::Var(1.into()) => {
                    VarInt::from(2u64) * Expr::Var(2.into())
                }
                Expr::Scale(c, inner)
                    if c == VarInt::from(2u64) && *inner == Expr::Var(2.into()) =>
                {
                    Expr::Var(3.into())
                }
                stable => stable,
            })
        })
        .unwrap();

        assert_eq!(result, Expr::Var(3.into()));
    }

    #[test]
    fn fixed_point_cycle_returns_lowest_cost_expression() {
        let start = !Expr::Var(0.into());
        let result = simplify_to_fixed_point(start.clone(), |e| {
            Ok(if e == start {
                Expr::Var(1.into())
            } else {
                start.clone()
            })
        })
        .unwrap();

        assert_eq!(result, Expr::Var(1.into()));
    }

    #[test]
    fn fixed_point_stops_after_maximum_number_of_passes() {
        let calls = Cell::new(0usize);
        let result = simplify_to_fixed_point(Expr::Var(0.into()), |e| {
            calls.set(calls.get() + 1);
            Ok(match e {
                Expr::Var(v) => Expr::Var((v.0 + 1).into()),
                other => other,
            })
        })
        .unwrap();

        assert_eq!(calls.get(), MAX_SIMPLIFICATION_PASSES);
        assert_eq!(result, Expr::Var(0.into()));
    }

    #[test]
    fn bitwise_frontier_normalizes_kernel_over_opaque_operands() {
        let a = Expr::Var(0.into()) + Expr::Var(1.into());
        let b = Expr::Var(2.into()) * Expr::Var(3.into());
        let kernel = (a.clone() & b.clone()) * (a.clone() | b.clone())
            + (a.clone() & !b.clone()) * (!a.clone() & b.clone())
            - a * b;
        let cache = LocalCache::new();
        let solver = MBASolver::new(&cache, &kernel, 64);

        assert_eq!(solver.normalize_bitwise_frontier(&kernel), Some(Expr::zero()));
    }

    #[test]
    fn bitwise_frontier_deduplicates_structurally_equal_atoms() {
        let atom = Expr::Var(0.into()) + Expr::Var(1.into());
        let frontier = atom.clone() | atom.clone();
        let cache = LocalCache::new();
        let solver = MBASolver::new(&cache, &frontier, 64);
        let mut metrics = BitwiseFrontierMetrics::default();

        let normalized = solver
            .normalize_one_bitwise_frontier(&frontier, &mut metrics)
            .unwrap();

        assert_eq!(normalized, atom);
        assert_eq!(metrics.max_frontier_atoms, 1);
    }

    #[test]
    fn bitwise_frontier_aborts_above_four_atoms() {
        let atoms = (0..=MAX_FRONTIER_ATOMS)
            .map(|i| Expr::Var(i.into()) * Expr::Var((i + MAX_FRONTIER_ATOMS + 1).into()))
            .collect();
        let frontier = Expr::Or(atoms);
        let cache = LocalCache::new();
        let solver = MBASolver::new(&cache, &frontier, 64);
        let mut metrics = BitwiseFrontierMetrics::default();

        assert_eq!(
            solver.normalize_one_bitwise_frontier(&frontier, &mut metrics),
            None
        );
        assert_eq!(metrics.aborts_atom_limit, 1);
    }

    #[test]
    fn bitwise_frontier_keeps_partial_word_constants_opaque() {
        let frontier = (Expr::Var(0.into()) + Expr::Var(1.into())) & Expr::make_const(1);
        let cache = LocalCache::new();
        let solver = MBASolver::new(&cache, &frontier, 64);
        let mut metrics = BitwiseFrontierMetrics::default();

        let normalized = solver
            .normalize_one_bitwise_frontier(&frontier, &mut metrics)
            .unwrap();

        assert_eq!(normalized, frontier.clone().reduce(make_mask(64)));
        assert_eq!(metrics.max_frontier_atoms, 2);
    }

    #[test]
    fn hidden_atom_diagnostics_classify_arithmetic_dependencies() {
        let product = Expr::Var(0.into()) * Expr::Var(1.into());
        let expression = product.clone() & (VarInt::from(2u64) * product);

        let (_, scopes) = diagnose_hidden_atoms(expression, 64).unwrap();
        let atoms: Vec<_> = scopes.iter().flat_map(|scope| &scope.atoms).collect();

        assert!(atoms.iter().any(|atom| {
            atom.dependency_kind == HiddenAtomDependencyKind::ArithmeticDependent
                && !atom.dependent_atoms.is_empty()
        }));
        assert!(scopes
            .iter()
            .any(|scope| scope.pre_restore_result.is_some()));
        HIDDEN_TRACE_STATE.with(|state| assert!(state.borrow().is_none()));
    }

    #[test]
    fn hidden_atom_diagnostics_detect_structural_lowbit_candidates() {
        let x = Expr::Var(0.into());
        let lowbit = x.clone() & -x.clone();

        assert!(contains_lowbit_candidate(&lowbit, make_mask(64)));
        assert!(!contains_lowbit_candidate(
            &(x & Expr::Var(1.into())),
            make_mask(64)
        ));
    }

    #[test]
    fn semantic_bitwise_candidate_certifies_arithmetic_not() {
        let q = Expr::Var(0.into());
        let definition = -q.clone() - Expr::make_const(1);

        assert_eq!(certify_bitwise_candidate(&definition, 64), Some(!q));
    }

    #[test]
    fn semantic_bitwise_candidate_certifies_arithmetic_xor() {
        let q = Expr::Var(0.into());
        let r = Expr::Var(1.into());
        let definition = q.clone() + r.clone()
            - VarInt::from(2u64) * (q.clone() & r.clone());

        let candidate = certify_bitwise_candidate(&definition, 64).unwrap();

        assert!(candidate.is_bitwise());
        assert_eq!(simplify_mba(definition - candidate, 64), Ok(Expr::zero()));
    }

    #[test]
    fn semantic_bitwise_candidate_certifies_arithmetic_or() {
        let q = Expr::Var(0.into());
        let r = Expr::Var(1.into());
        let definition = q.clone() + r.clone() - (q.clone() & r.clone());

        let candidate = certify_bitwise_candidate(&definition, 64).unwrap();

        assert!(candidate.is_bitwise());
        assert_eq!(simplify_mba(definition - candidate, 64), Ok(Expr::zero()));
    }

    #[test]
    fn semantic_bitwise_candidate_rejects_boolean_only_polynomial() {
        let x = Expr::Var(0.into());
        let definition = x.clone() * x.clone() - x;

        assert_eq!(certify_bitwise_candidate(&definition, 64), None);
    }

    #[test]
    fn semantic_bitwise_candidate_rejects_partial_word_constant() {
        let x = Expr::Var(0.into());
        let definition = x & Expr::make_const(1);

        assert_eq!(certify_bitwise_candidate(&definition, 64), None);
    }

    #[test]
    fn direct_dependency_experiment_substitutes_only_proved_candidate() {
        let q = Expr::Var(0.into());
        let b = Expr::Var(1.into());
        let d: VarId = 2.into();
        let definition = -q.clone() - Expr::make_const(1);
        let pre_restore = -b.clone()
            + (b.clone() & Expr::Var(d))
            + (b & q);
        let scope = HiddenScopeTrace {
            scope: 0,
            bit_width: 64,
            input: pre_restore.clone(),
            pre_restore_result: Some(pre_restore),
            atoms: vec![HiddenAtomTrace {
                atom: d,
                original: definition.clone(),
                simplified: definition.clone(),
                free_atoms: vec![0.into()],
                dependent_atoms: vec![0.into()],
                dependency_definition: Some(definition),
                dependency_kind: HiddenAtomDependencyKind::ArithmeticDependent,
            }],
        };

        let experiment = experiment_direct_bitwise_dependencies(&scope)
            .unwrap()
            .unwrap();

        assert_eq!(experiment.proofs.len(), 1);
        assert!(experiment.rejections.is_empty());
        assert_eq!(experiment.simplified_after_substitution, Expr::zero());
    }

    #[test]
    fn direct_complement_experiment_proves_and_substitutes_full_definitions() {
        let x = Expr::Var(0.into());
        let y = Expr::Var(1.into());
        let b = Expr::Var(2.into());
        let left: VarId = 3.into();
        let right: VarId = 4.into();
        let left_definition = x + y;
        let right_definition = -left_definition.clone() - Expr::make_const(1);
        let pre_restore = -b.clone()
            + (b.clone() & Expr::Var(left))
            + (b & Expr::Var(right));
        let make_atom = |atom, definition: Expr| HiddenAtomTrace {
            atom,
            original: definition.clone(),
            simplified: definition,
            free_atoms: vec![0.into(), 1.into()],
            dependent_atoms: vec![0.into()],
            dependency_definition: Some(Expr::zero()),
            dependency_kind: HiddenAtomDependencyKind::ArithmeticDependent,
        };
        let scope = HiddenScopeTrace {
            scope: 0,
            bit_width: 64,
            input: pre_restore.clone(),
            pre_restore_result: Some(pre_restore),
            atoms: vec![
                make_atom(left, left_definition),
                make_atom(right, right_definition),
            ],
        };

        let experiment = experiment_direct_complement_relation(&scope)
            .unwrap()
            .unwrap();

        assert_eq!(experiment.proof, ComplementRelationProof::Proved);
        assert_eq!(
            experiment.simplified_after_substitution,
            Some(Expr::zero())
        );
    }

    #[test]
    fn direct_complement_experiment_does_not_substitute_unproved_relation() {
        let x = Expr::Var(0.into());
        let b = Expr::Var(1.into());
        let left: VarId = 2.into();
        let right: VarId = 3.into();
        let pre_restore = -b.clone()
            + (b.clone() & Expr::Var(left))
            + (b & Expr::Var(right));
        let make_atom = |atom, definition: Expr| HiddenAtomTrace {
            atom,
            original: definition.clone(),
            simplified: definition,
            free_atoms: vec![0.into()],
            dependent_atoms: Vec::new(),
            dependency_definition: None,
            dependency_kind: HiddenAtomDependencyKind::Root,
        };
        let scope = HiddenScopeTrace {
            scope: 0,
            bit_width: 64,
            input: pre_restore.clone(),
            pre_restore_result: Some(pre_restore),
            atoms: vec![
                make_atom(left, x.clone()),
                make_atom(right, x),
            ],
        };

        let experiment = experiment_direct_complement_relation(&scope)
            .unwrap()
            .unwrap();

        assert!(matches!(
            experiment.proof,
            ComplementRelationProof::NotProved { .. }
        ));
        assert_eq!(experiment.substituted_pre_restore, None);
        assert_eq!(experiment.simplified_after_substitution, None);
    }

    #[test]
    fn guided_alias_pair_selection_matches_equal_bitwise_contexts() {
        let carrier = Expr::Var(0.into());
        let left: VarId = 1.into();
        let right: VarId = 2.into();
        let pre_restore = VarInt::from(2u64) * (carrier.clone() & Expr::Var(left))
            - VarInt::from(2u64) * (carrier & Expr::Var(right));
        let direct_atoms = HashSet::from([left, right]);

        assert_eq!(
            select_guided_direct_pairs(&pre_restore, &direct_atoms, make_mask(64)),
            vec![(left, right)]
        );
    }

    #[test]
    fn guided_alias_pair_selection_includes_local_copresence_and_obeys_budget() {
        let atoms: Vec<VarId> = (0..6).map(VarId::from).collect();
        let expression = Expr::And(atoms.iter().copied().map(Expr::Var).collect());
        let direct_atoms: HashSet<_> = atoms.into_iter().collect();

        let pairs =
            select_guided_direct_pairs(&expression, &direct_atoms, make_mask(64));

        assert_eq!(pairs.len(), MAX_GUIDED_ALIAS_PAIRS);
        assert!(pairs.contains(&(0.into(), 1.into())));
    }

    #[test]
    fn guided_alias_experiment_substitutes_only_certified_relations() {
        let x = Expr::Var(0.into());
        let y = Expr::Var(1.into());
        let carrier = Expr::Var(2.into());
        let left: VarId = 3.into();
        let right: VarId = 4.into();
        let definition = x + y;
        let pre_restore = VarInt::from(2u64) * (carrier.clone() & Expr::Var(left))
            - VarInt::from(2u64) * (carrier & Expr::Var(right));
        let make_atom = |atom| HiddenAtomTrace {
            atom,
            original: definition.clone(),
            simplified: definition.clone(),
            free_atoms: vec![0.into(), 1.into()],
            dependent_atoms: Vec::new(),
            dependency_definition: None,
            dependency_kind: HiddenAtomDependencyKind::Root,
        };
        let scope = HiddenScopeTrace {
            scope: 0,
            bit_width: 64,
            input: pre_restore.clone(),
            pre_restore_result: Some(pre_restore),
            atoms: vec![make_atom(left), make_atom(right)],
        };

        let experiment = experiment_guided_semantic_aliases(&scope)
            .unwrap()
            .unwrap();

        assert_eq!(experiment.candidate_pairs, vec![(left, right)]);
        assert_eq!(
            experiment.certified_aliases,
            vec![CertifiedSemanticAlias {
                left,
                right,
                alias: SemanticAlias::Equal,
            }]
        );
        assert_eq!(experiment.attempts.len(), 3);
        assert_eq!(experiment.simplified_after_substitution, Expr::zero());
    }

    #[test]
    fn guided_alias_experiment_keeps_unproved_pairs_unchanged() {
        let carrier = Expr::Var(0.into());
        let left: VarId = 1.into();
        let right: VarId = 2.into();
        let pre_restore = (carrier.clone() & Expr::Var(left))
            - (carrier & Expr::Var(right));
        let make_atom = |atom, definition| HiddenAtomTrace {
            atom,
            original: definition,
            simplified: Expr::Var(atom),
            free_atoms: vec![atom],
            dependent_atoms: Vec::new(),
            dependency_definition: None,
            dependency_kind: HiddenAtomDependencyKind::Root,
        };
        let scope = HiddenScopeTrace {
            scope: 0,
            bit_width: 64,
            input: pre_restore.clone(),
            pre_restore_result: Some(pre_restore.clone()),
            atoms: vec![
                make_atom(left, Expr::Var(left)),
                make_atom(right, Expr::Var(right)),
            ],
        };

        let experiment = experiment_guided_semantic_aliases(&scope)
            .unwrap()
            .unwrap();

        assert!(experiment.certified_aliases.is_empty());
        assert_eq!(experiment.substituted_pre_restore, pre_restore);
    }

    #[test]
    fn guided_alias_experiment_certifies_complement_and_opposite() {
        let x = Expr::Var(0.into());
        let carrier = Expr::Var(1.into());
        let left: VarId = 2.into();
        let right: VarId = 3.into();
        let make_scope = |pre_restore: Expr, right_definition: Expr| {
            let pre_restore = pre_restore.reduce(make_mask(64));
            let make_atom = |atom, definition: Expr| HiddenAtomTrace {
                atom,
                original: definition.clone(),
                simplified: definition,
                free_atoms: vec![0.into()],
                dependent_atoms: Vec::new(),
                dependency_definition: None,
                dependency_kind: HiddenAtomDependencyKind::Root,
            };
            HiddenScopeTrace {
                scope: 0,
                bit_width: 64,
                input: pre_restore.clone(),
                pre_restore_result: Some(pre_restore),
                atoms: vec![
                    make_atom(left, x.clone()),
                    make_atom(right, right_definition),
                ],
            }
        };
        let complement_scope = make_scope(
            -carrier.clone()
                + (carrier.clone() & Expr::Var(left))
                + (carrier & Expr::Var(right)),
            -x.clone() - Expr::make_const(1),
        );
        let opposite_scope = make_scope(
            Expr::Var(left) + Expr::Var(right),
            -x.clone(),
        );

        let complement = experiment_guided_semantic_aliases(&complement_scope)
            .unwrap()
            .unwrap();
        let opposite = experiment_guided_semantic_aliases(&opposite_scope)
            .unwrap()
            .unwrap();

        assert_eq!(
            complement.certified_aliases[0].alias,
            SemanticAlias::Complement
        );
        assert_eq!(complement.simplified_after_substitution, Expr::zero());
        assert_eq!(
            opposite.certified_aliases[0].alias,
            SemanticAlias::ArithmeticOpposite
        );
        assert_eq!(opposite.simplified_after_substitution, Expr::zero());
    }

    #[test]
    fn guided_ternary_relation_builds_one_minus_two_identity() {
        let x = Expr::Var(0.into());
        let y = Expr::Var(1.into());
        let a = x.clone() + y.clone();
        let relation = make_guided_ternary_relation(
            GuidedTernaryRelation::OneMinusTwo,
            &a,
            &x,
            &y,
            make_mask(64),
        );

        assert_eq!(simplify_mba(relation, 64), Ok(Expr::zero()));
    }

    #[test]
    fn guided_ternary_relation_builds_all_allowed_forms() {
        let x = Expr::Var(0.into());
        let y = Expr::Var(1.into());
        let sum = x.clone() + y.clone();
        let complement_sum = -sum.clone() - Expr::make_const(1);

        let two_minus_one = make_guided_ternary_relation(
            GuidedTernaryRelation::TwoMinusOne,
            &x,
            &y,
            &sum,
            make_mask(64),
        );
        let complement = make_guided_ternary_relation(
            GuidedTernaryRelation::ComplementSum,
            &x,
            &y,
            &complement_sum,
            make_mask(64),
        );

        assert_eq!(simplify_mba(two_minus_one, 64), Ok(Expr::zero()));
        assert_eq!(simplify_mba(complement, 64), Ok(Expr::zero()));
    }

    #[test]
    fn guided_ternary_experiment_certifies_and_applies_relation() {
        let x = Expr::Var(0.into());
        let y = Expr::Var(1.into());
        let a: VarId = 2.into();
        let b: VarId = 3.into();
        let c: VarId = 4.into();
        let pre_restore = Expr::Var(a) - Expr::Var(b) - Expr::Var(c);
        let make_atom = |atom, definition: Expr| HiddenAtomTrace {
            atom,
            original: definition.clone(),
            simplified: definition,
            free_atoms: vec![0.into(), 1.into()],
            dependent_atoms: Vec::new(),
            dependency_definition: None,
            dependency_kind: HiddenAtomDependencyKind::Root,
        };
        let scope = HiddenScopeTrace {
            scope: 0,
            bit_width: 64,
            input: pre_restore.clone(),
            pre_restore_result: Some(pre_restore),
            atoms: vec![
                make_atom(a, x.clone() + y.clone()),
                make_atom(b, x),
                make_atom(c, y),
            ],
        };

        let experiment = experiment_guided_ternary_relations(&scope)
            .unwrap()
            .unwrap();

        assert_eq!(experiment.remaining_atoms, vec![a, b, c]);
        assert_eq!(experiment.certified_relations.len(), 1);
        assert_eq!(
            experiment.certified_relations[0].kind,
            GuidedTernaryRelation::OneMinusTwo
        );
        assert_eq!(experiment.simplified_after_substitution, Expr::zero());
    }
}
