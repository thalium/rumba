//! Interning of semantically equal hidden (nonlinear) components.
//!
//! When two nonlinear subexpressions are independently written but semantically
//! equal, they receive distinct hidden variables during polynomialization. This
//! pass nominates candidate relations from cheap word observations, proves the
//! promising ones exactly, and rewrites the aliased variables so the linear
//! engine sees a single shared component.

use std::cell::Cell;

use crate::{
    expr::{Expr, VarId},
    utils::bimap::BiMap,
    utils::cache::LinearCache,
};

use log::debug;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use super::{MBASolver, reduce_vars};

thread_local! {
    /// Prevents a hidden-component equality query from recursively starting
    /// another query while simplifying its own difference.
    static HIDDEN_EQUALITY_DEPTH: Cell<usize> = const { Cell::new(0) };
}

fn passes_quick_zero_check(e: &Expr, mask: u64) -> bool {
    let variable_count = e
        .get_vars()
        .into_iter()
        .map(|variable| variable.0)
        .max()
        .unwrap_or(0)
        + 1;
    let mut variables = vec![0u64; variable_count];

    for sample in 0..3u64 {
        for (index, variable) in variables.iter_mut().enumerate() {
            *variable = match sample {
                0 => 0,
                1 => mask,
                _ => {
                    0x9e37_79b9_7f4a_7c15u64
                        .wrapping_add((index as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9))
                        & mask
                }
            };
        }
        if e.eval_bits(&variables).get(mask) != 0 {
            return false;
        }
    }
    true
}

fn complete_truth_table(observed: &[Option<bool>]) -> Vec<u8> {
    let mut tables = vec![0u8];
    for (input, output) in observed.iter().enumerate() {
        match output {
            Some(true) => {
                for table in &mut tables {
                    *table |= 1 << input;
                }
            }
            Some(false) => {}
            None => {
                let mut with_bit = tables.clone();
                for table in &mut with_bit {
                    *table |= 1 << input;
                }
                tables.extend(with_bit);
            }
        }
    }
    tables
}

fn infer_bitwise_truth_tables(target: &[u64], parents: &[&[u64]], n: u8) -> Vec<u8> {
    let input_count = 1usize << parents.len();
    let mut observed = vec![None; input_count];
    for sample in 0..target.len() {
        for bit in 0..n {
            let mut input = 0usize;
            for (index, parent) in parents.iter().enumerate() {
                input |= (((parent[sample] >> bit) & 1) as usize) << index;
            }
            let output = ((target[sample] >> bit) & 1) != 0;
            match observed[input] {
                Some(previous) if previous != output => return Vec::new(),
                Some(_) => {}
                None => observed[input] = Some(output),
            }
        }
    }
    complete_truth_table(&observed)
}

fn synthesize_unary_bitwise(parent: Expr, truth_table: u8, mask: u64) -> Expr {
    match truth_table {
        0b00 => Expr::zero(),
        0b01 => !parent,
        0b10 => parent,
        0b11 => Expr::make_const(mask),
        _ => unreachable!("a unary truth table has two bits"),
    }
}

fn synthesize_binary_bitwise(left: Expr, right: Expr, truth_table: u8, mask: u64) -> Expr {
    match truth_table {
        0b0000 => Expr::zero(),
        0b0001 => !(left | right),
        0b0010 => left & !right,
        0b0011 => !right,
        0b0100 => !left & right,
        0b0101 => !left,
        0b0110 => left ^ right,
        0b0111 => !(left & right),
        0b1000 => left & right,
        0b1001 => !(left ^ right),
        0b1010 => left,
        0b1011 => left | !right,
        0b1100 => right,
        0b1101 => !left | right,
        0b1110 => left | right,
        0b1111 => Expr::make_const(mask),
        _ => unreachable!("a binary truth table has four bits"),
    }
}

fn make_signature_samples(expressions: &[Expr], mask: u64) -> Option<Vec<Vec<u64>>> {
    let mut variable_map = BiMap::<VarId, VarId>::new();
    let mut variable_count = 0;
    let reduced: Vec<_> = expressions
        .iter()
        .cloned()
        .map(|expression| reduce_vars(expression, &mut variable_map, &mut variable_count))
        .collect();
    if variable_count > 10 {
        return None;
    }
    let mut samples: Vec<_> = reduced
        .iter()
        .map(|expression| expression.truth_table_masked(variable_count, mask))
        .collect();
    for sample in 0..3u64 {
        let variables: Vec<_> = (0..variable_count)
            .map(|variable| {
                0x9e37_79b9_7f4a_7c15u64
                    .wrapping_mul(sample + 1)
                    .wrapping_add(0xbf58_476d_1ce4_e5b9u64.wrapping_mul((variable + 1) as u64))
                    & mask
            })
            .collect();
        for (values, expression) in samples.iter_mut().zip(&reduced) {
            values.push(expression.eval_bits(&variables).get(mask));
        }
    }
    Some(samples)
}

impl<'a, C: LinearCache> MBASolver<'a, C> {
    /// Rewrites hidden variables back to the fixed-width expressions they stand
    /// for, so a proof concerns the original expressions and not independent
    /// placeholder variables.
    fn expand_hidden_components(&self, e: Expr) -> Expr {
        match e {
            Expr::Var(variable) => self
                .non_linear_components
                .get_by_left(&variable)
                .cloned()
                .map(|definition| self.expand_hidden_components(definition))
                .unwrap_or(Expr::Var(variable)),
            _ => e.map(|child| self.expand_hidden_components(child)),
        }
    }

    /// Validates a signature-nominated equality using the expanded fixed-width
    /// expressions. Signatures are only a search heuristic here; this proof is
    /// the correctness boundary.
    fn prove_hidden_relation(&mut self, left: Expr, right: Expr) -> bool {
        let difference = match right {
            // Use the canonical additive form for a complement relation. It is
            // algebraically identical to `left - !right`, but exposes the
            // relation directly to the linear engine.
            Expr::Not(inner) => left + *inner + Expr::make_const(1),
            right => left - right,
        };
        let difference = self
            .expand_hidden_components(difference)
            .reduce_masked(self.mask);
        if !passes_quick_zero_check(&difference, self.mask) {
            return false;
        }

        HIDDEN_EQUALITY_DEPTH.with(|depth| {
            depth.set(depth.get() + 1);
            let mut solver = MBASolver::new(self.l_cache, &difference, self.n, self.options);
            let is_zero = solver.solve(difference).is_ok_and(|e| e == Expr::zero());
            depth.set(depth.get() - 1);
            is_zero
        })
    }

    /// Interns semantically equal nonlinear components under the same hidden
    /// variable. Syntactically equal components are already shared by `BiMap`;
    /// this catches independently written expressions whose difference has a
    /// zero signature.
    pub(super) fn merge_equal_hidden_components(&mut self, e: Expr) -> Expr {
        if HIDDEN_EQUALITY_DEPTH.with(|depth| depth.get() != 0)
            || self.non_linear_components.len() == 0
        {
            return e;
        }

        let mut components: Vec<_> = self
            .non_linear_components
            .iter()
            .filter(|(_, expression)| !matches!(expression, Expr::Const(_)))
            .map(|(variable, expression)| (*variable, expression.clone()))
            .collect();
        components.sort_unstable_by_key(|(variable, _)| variable.0);
        if components.is_empty() {
            return e;
        }

        let mut aliases = HashMap::<VarId, Expr>::default();

        // Word observations are not proofs, but they cheaply reject impossible
        // unary and binary bitwise dependencies before exact validation.
        let mut visible_variables = HashSet::default();
        for (_, definition) in &components {
            for variable in definition.get_vars() {
                if self.non_linear_components.get_by_left(&variable).is_none() {
                    visible_variables.insert(variable);
                }
            }
        }
        let mut visible_variables: Vec<_> = visible_variables.into_iter().collect();
        visible_variables.sort_unstable_by_key(|variable| variable.0);

        let mut observed_expressions = Vec::<(VarId, Expr)>::new();
        for (variable, definition) in &components {
            observed_expressions
                .push((*variable, self.expand_hidden_components(definition.clone())));
        }
        let mut observed_atoms = Vec::<(Expr, Expr)>::new();
        for (variable, definition) in &observed_expressions {
            observed_atoms.push((Expr::Var(*variable), definition.clone()));
        }
        for variable in visible_variables {
            observed_atoms.push((Expr::Var(variable), Expr::Var(variable)));
        }

        let sampled_expressions: Vec<_> = observed_expressions
            .iter()
            .map(|(_, expression)| expression.clone())
            .chain(
                observed_atoms
                    .iter()
                    .map(|(_, expression)| expression.clone()),
            )
            .collect();
        let Some(samples) = make_signature_samples(&sampled_expressions, self.mask) else {
            return e;
        };
        let (definition_samples, atom_samples) = samples.split_at(observed_expressions.len());

        'targets: for (target_index, (target_variable, _)) in
            observed_expressions.iter().enumerate()
        {
            let usable_atoms: Vec<_> = observed_atoms
                .iter()
                .enumerate()
                .filter(|(_, (atom, _))| match atom {
                    // Earlier hidden components cannot introduce an alias
                    // cycle. Ordinary variables are always safe.
                    Expr::Var(variable)
                        if self.non_linear_components.get_by_left(variable).is_some() =>
                    {
                        variable.0 < target_variable.0
                    }
                    Expr::Var(_) => true,
                    _ => false,
                })
                .collect();
            let target_samples = &definition_samples[target_index];
            let mut candidates = Vec::new();

            for table in infer_bitwise_truth_tables(target_samples, &[], self.n) {
                candidates.push(synthesize_unary_bitwise(
                    Expr::zero(),
                    if table == 0 { 0 } else { 3 },
                    self.mask,
                ));
            }
            for (atom_index, (atom, _)) in &usable_atoms {
                for table in infer_bitwise_truth_tables(
                    target_samples,
                    &[&atom_samples[*atom_index]],
                    self.n,
                ) {
                    candidates.push(synthesize_unary_bitwise(atom.clone(), table, self.mask));
                }
            }
            for left_position in 0..usable_atoms.len() {
                let (left_index, (left, _)) = usable_atoms[left_position];
                for (right_index, (right, _)) in &usable_atoms[left_position + 1..] {
                    for table in infer_bitwise_truth_tables(
                        target_samples,
                        &[&atom_samples[left_index], &atom_samples[*right_index]],
                        self.n,
                    ) {
                        candidates.push(synthesize_binary_bitwise(
                            left.clone(),
                            right.clone(),
                            table,
                            self.mask,
                        ));
                    }
                }
            }

            candidates.sort_unstable_by_key(Expr::size);
            candidates.dedup();
            for candidate in candidates {
                debug!(
                    "Word observations nominated hidden relation v{} = {}",
                    target_variable, candidate
                );
                if self.prove_hidden_relation(Expr::Var(*target_variable), candidate.clone()) {
                    aliases.insert(*target_variable, candidate);
                    continue 'targets;
                }
            }
        }

        if aliases.is_empty() {
            return e;
        }

        fn replace_aliases(e: Expr, aliases: &HashMap<VarId, Expr>) -> Expr {
            match e {
                Expr::Var(variable) => {
                    if let Some(alias) = aliases.get(&variable) {
                        replace_aliases(alias.clone(), aliases)
                    } else {
                        Expr::Var(variable)
                    }
                }
                _ => e.map(|child| replace_aliases(child, aliases)),
            }
        }

        replace_aliases(e, &aliases).reduce_masked(self.mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::varint::make_mask;

    #[test]
    fn synthesizes_every_binary_bitwise_truth_table() {
        let mask = make_mask(8);
        for table in 0..16u8 {
            let expression =
                synthesize_binary_bitwise(Expr::Var(0.into()), Expr::Var(1.into()), table, mask);
            for assignment in 0..4usize {
                let variables = [
                    if assignment & 1 == 0 { 0 } else { mask },
                    if assignment & 2 == 0 { 0 } else { mask },
                ];
                let expected = if table & (1 << assignment) == 0 {
                    0
                } else {
                    mask
                };
                assert_eq!(expression.eval_bits(&variables).get(mask), expected);
            }
        }
    }
}
