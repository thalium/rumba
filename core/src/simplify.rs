use std::{
    cell::Cell,
    cmp::max,
    collections::{HashMap, HashSet},
};

use crate::{
    bimap::BiMap,
    expr::{Expr, VarId},
    varint::{VarInt, make_mask},
};

use log::debug;

thread_local! {
    /// Prevents a hidden-component equality query from recursively starting
    /// another query while simplifying its own difference.
    static HIDDEN_EQUALITY_DEPTH: Cell<usize> = const { Cell::new(0) };
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

fn find_two_lambdas_int(
    x: &[u64],
    y: &[u64],
    z: &[u64],
    a: i64,
    b: i64,
    n: u8,
) -> Option<(u64, u64)> {
    let signed: Vec<_> = x
        .iter()
        .zip(y)
        .zip(z)
        .map(|((&x, &y), &z)| (get_signed(x, n), get_signed(y, n), get_signed(z, n)))
        .collect();
    let base = signed.iter().position(|&(_, y, z)| y != 0 || z != 0)?;
    let targets = [a, b];

    for second in 0..signed.len() {
        let (_, y0, z0) = signed[base];
        let (_, y1, z1) = signed[second];
        let determinant = y0 as i128 * z1 as i128 - y1 as i128 * z0 as i128;
        if determinant == 0 {
            continue;
        }

        for target0 in targets {
            for target1 in targets {
                let rhs0 = signed[base].0.wrapping_sub(target0) as i128;
                let rhs1 = signed[second].0.wrapping_sub(target1) as i128;
                let lambda_y_num = rhs0 * z1 as i128 - rhs1 * z0 as i128;
                let lambda_z_num = y0 as i128 * rhs1 - y1 as i128 * rhs0;
                if lambda_y_num % determinant != 0 || lambda_z_num % determinant != 0 {
                    continue;
                }

                let lambda_y = lambda_y_num / determinant;
                let lambda_z = lambda_z_num / determinant;
                let Ok(lambda_y) = i64::try_from(lambda_y) else {
                    continue;
                };
                let Ok(lambda_z) = i64::try_from(lambda_z) else {
                    continue;
                };

                let corrected = |(x, y, z): (i64, i64, i64)| {
                    x.wrapping_sub(lambda_y.wrapping_mul(y))
                        .wrapping_sub(lambda_z.wrapping_mul(z))
                };
                if corrected(signed[0]) == a
                    && signed.iter().copied().all(|values| {
                        let value = corrected(values);
                        value == a || value == b
                    })
                {
                    let mask = make_mask(n);
                    return Some(((lambda_y as u64) & mask, (lambda_z as u64) & mask));
                }
            }
        }
    }

    None
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
        if e.eval(&variables).get(mask) != 0 {
            return false;
        }
    }
    true
}

/// Restores the varialbes in the mba
fn restore_vars(e: Expr, var_map: &BiMap<VarId, VarId>) -> Expr {
    match e {
        Expr::Var(v) => {
            let vv = if let Some(v) = var_map.get_by_right(&v) {
                *v
            } else {
                panic!("Can't find variable");
            };
            Expr::Var(vv)
        }

        _ => e.map(|e| restore_vars(e, var_map)),
    }
}

struct MBASolver<'a> {
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
    l_cache: &'a mut HashMap<Expr, Expr>,
}

impl<'a> MBASolver<'a> {
    /// Create a new Solver
    fn new(l_cache: &'a mut HashMap<Expr, Expr>, e: &Expr, n: u8) -> Self {
        Self {
            non_linear_components: BiMap::new(),
            t: e.get_vars().iter().copied().map(|v| v.0).max().unwrap_or(0) + 1,
            degree: 1,
            n,
            mask: make_mask(n),
            l_cache,
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

    /// Expands hidden variables recursively. This is used when validating a
    /// relation suggested by signatures: the proof must concern the original
    /// expressions, not independent placeholder variables.
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
        let difference = self.expand_hidden_components(difference).reduce(self.mask);
        if !passes_quick_zero_check(&difference, self.mask) {
            return false;
        }

        HIDDEN_EQUALITY_DEPTH.with(|depth| {
            depth.set(depth.get() + 1);
            let mut solver = MBASolver::new(self.l_cache, &difference, self.n);
            let is_zero = solver.solve(difference) == Expr::zero();
            depth.set(depth.get() - 1);
            is_zero
        })
    }

    /// Solves a non polynomial MBA
    fn solve(&mut self, e: Expr) -> Expr {
        // TODO: Remove this only needs to be done once
        let e = e.reduce(self.mask);

        // Non-polynomial stage: structural pattern rewrites on the canonical
        // expression, before it is lifted to a polynomial.
        let e = crate::patterns::apply_patterns(e, self.mask);

        let p = self.make_polynomial(e);
        let p = self.merge_equal_hidden_components(p);
        self.degree = 1;
        let p = self.make_polynomial(p);
        let p = self.solve_polynomial(p);

        // This was a non linear MBA
        if self.non_linear_components.len() != 0 {
            let e = self.poly_to_nonpoly(p);
            debug!("After adding non linear components, found: {}", e);
            e.reduce(self.mask)
        } else {
            p
        }
    }

    /// Interns semantically equal nonlinear components under the same hidden
    /// variable. Syntactically equal components are already shared by `BiMap`;
    /// this catches independently written expressions whose difference has a
    /// zero signature.
    fn merge_equal_hidden_components(&mut self, e: Expr) -> Expr {
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

        let mut aliases = HashMap::<VarId, Expr>::new();

        // Signatures of nonlinear expressions are not proofs, but they are a
        // useful way to nominate small relations worth proving. Search a
        // bounded grammar: an atom or its complement, and one binary bitwise
        // operation with an optional complement.
        let mut visible_variables = HashSet::new();
        for (_, definition) in &components {
            for variable in definition.get_vars() {
                if self.non_linear_components.get_by_left(&variable).is_none() {
                    visible_variables.insert(variable);
                }
            }
        }
        let mut visible_variables: Vec<_> = visible_variables.into_iter().collect();
        visible_variables.sort_unstable_by_key(|variable| variable.0);

        let mut signature_expressions = Vec::<(VarId, Expr)>::new();
        for (variable, definition) in &components {
            signature_expressions
                .push((*variable, self.expand_hidden_components(definition.clone())));
        }
        let mut signature_atoms = Vec::<(Expr, Expr)>::new();
        for (variable, definition) in &signature_expressions {
            signature_atoms.push((Expr::Var(*variable), definition.clone()));
        }
        for variable in visible_variables {
            signature_atoms.push((Expr::Var(variable), Expr::Var(variable)));
        }

        let mut variable_map = BiMap::<VarId, VarId>::new();
        let mut variable_count = 0;
        let reduced_definitions: Vec<_> = signature_expressions
            .iter()
            .map(|(_, definition)| {
                reduce_vars(definition.clone(), &mut variable_map, &mut variable_count)
            })
            .collect();
        let reduced_atoms: Vec<_> = signature_atoms
            .iter()
            .map(|(_, definition)| {
                reduce_vars(definition.clone(), &mut variable_map, &mut variable_count)
            })
            .collect();

        if variable_count <= 10 {
            let mask = self.mask;
            let definition_signatures: Vec<_> = reduced_definitions
                .iter()
                .map(|definition| definition.truth_table(variable_count, self.mask))
                .collect();
            let atom_signatures: Vec<_> = reduced_atoms
                .iter()
                .map(|atom| atom.truth_table(variable_count, self.mask))
                .collect();

            'targets: for (target_index, (target_variable, _)) in
                signature_expressions.iter().enumerate()
            {
                if aliases.contains_key(target_variable) {
                    continue;
                }
                let target_signature = &definition_signatures[target_index];
                let usable_atoms: Vec<_> = signature_atoms
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

                for (atom_index, (atom, _)) in &usable_atoms {
                    for complemented in [false, true] {
                        let matches = target_signature
                            .iter()
                            .zip(&atom_signatures[*atom_index])
                            .all(|(&target, &value)| {
                                target
                                    == if complemented {
                                        (!value) & self.mask
                                    } else {
                                        value
                                    }
                            });
                        if !matches {
                            continue;
                        }
                        let candidate = if complemented {
                            !atom.clone()
                        } else {
                            atom.clone()
                        };
                        debug!(
                            "Signature nominated hidden relation v{} = {}",
                            target_variable, candidate
                        );
                        if self
                            .prove_hidden_relation(Expr::Var(*target_variable), candidate.clone())
                        {
                            aliases.insert(*target_variable, candidate);
                            continue 'targets;
                        }
                    }
                }

                for left_position in 0..usable_atoms.len() {
                    let (left_index, (left, _)) = usable_atoms[left_position];
                    for (right_index, (right, _)) in &usable_atoms[left_position + 1..] {
                        for operation in 0..3 {
                            let signature_matches = |complemented: bool| {
                                target_signature
                                    .iter()
                                    .zip(&atom_signatures[left_index])
                                    .zip(&atom_signatures[*right_index])
                                    .all(|((&target, &left_value), &right_value)| {
                                        let value = match operation {
                                            0 => left_value & right_value,
                                            1 => left_value | right_value,
                                            _ => left_value ^ right_value,
                                        };
                                        target == if complemented { (!value) & mask } else { value }
                                    })
                            };

                            for complemented in [false, true] {
                                if !signature_matches(complemented) {
                                    continue;
                                }
                                let base = match operation {
                                    0 => left.clone() & right.clone(),
                                    1 => left.clone() | right.clone(),
                                    _ => left.clone() ^ right.clone(),
                                };
                                let candidate = if complemented { !base } else { base };
                                debug!(
                                    "Signature nominated hidden relation v{} = {}",
                                    target_variable, candidate
                                );
                                if self.prove_hidden_relation(
                                    Expr::Var(*target_variable),
                                    candidate.clone(),
                                ) {
                                    aliases.insert(*target_variable, candidate);
                                    continue 'targets;
                                }
                            }
                        }
                    }
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

        replace_aliases(e, &aliases).reduce(self.mask)
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
    fn solve_linear(&mut self, e: Expr, from_poly: bool) -> Expr {
        let mut var_map = BiMap::<VarId, VarId>::new();
        let mut t = 0;

        debug!("Solving linear MBA: {}", e);

        // Reduce the number of variables in the expression
        let e = reduce_vars(e, &mut var_map, &mut t);
        debug!("Reduced number of variables to equivalent problem: {}", e);

        let e = if let Some(simplified) = self.l_cache.get(&e) {
            debug!("Found linear MBA in cache");
            simplified.clone()
        } else {
            debug!("Solving linear MBA");

            if t > 20 {
                panic!("Too many variables");
            }

            let simplified = self.solve_linear_inner(e.clone(), t, from_poly);
            self.l_cache.insert(e, simplified.clone());
            simplified
        };

        let e = restore_vars(e, &var_map);

        debug!("Found solution to linear MBA: {}", e);

        e
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
    fn linear_to_poly(&self, e: Expr) -> Expr {
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
                        panic!("Solved linear MBA is in an unrecognized form")
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

                s * Expr::Mul(terms)
            }

            Expr::Add(_) | Expr::Scale(_, _) => e.map(|e| self.linear_to_poly(e)),

            Expr::Var(v) => {
                // The sign correction -> see paper
                let s = if self.degree & 1 == 0 { u64::MAX } else { 1 };
                // ERROR: this should be a t
                s * Expr::Var((v.0 % self.degree).into())
            }

            Expr::Const(_) => {
                // The sign correction -> see paper
                let s = if self.degree & 1 == 0 { u64::MAX } else { 1 };
                s * e
            }

            _ => panic!("Solved linear MBA is in an unrecognized form"),
        }
    }

    /// Solves a polynomial MBA
    fn solve_polynomial(&mut self, e: Expr) -> Expr {
        debug!("Solving polynomial MBA: {}", e);

        // This is a linear MBA
        if self.degree == 1 {
            debug!("This is a linear MBA");
            return self.solve_linear(e, false);
        }

        let e = self.poly_to_linear(e, 0);
        let e: Expr = self.solve_linear(e, true);
        let e = self.linear_to_poly(e).reduce(self.mask);

        debug!("Found polynomial solution: {}", e);

        e
    }

    /// Hides a non linear element behind a variable
    fn hide_in_var(&mut self, e: Expr, mask: u64) -> Expr {
        debug!("e={} is not linear and will be replaced by a variable", e);

        let e = match e {
            Expr::Const(_) => e,
            _ => simplify_mba_inner(self.l_cache, e, mask.count_ones() as u8).reduce(mask),
        };

        let note = (-e.clone() - Expr::make_const(1)).reduce(mask);
        debug!("!e would be {}", note);

        if let Some(v) = self.non_linear_components.get_by_right(&e) {
            debug!("Found variable v{} for e", v);
            Expr::Var(*v)
        } else if let Some(v) = self.non_linear_components.get_by_right(&note) {
            debug!("Found variable v{} for !e", v);
            !Expr::Var(*v)
        } else {
            let v = self.t.into();
            debug!("Creating variable v{} for e={}", v, e);
            self.t += 1;
            self.non_linear_components.insert(v, e);
            Expr::Var(v)
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
        let mut sub_vars = vec![];

        for v in e.get_vars() {
            if self
                .non_linear_components
                .get_by_left(&v)
                .is_some_and(|definition| self.is_linear(definition))
            {
                sub_vars.push(v);
            }
        }
        sub_vars.sort_unstable_by_key(|variable| variable.0);

        if sub_vars.is_empty() || sub_vars.len() > 2 {
            return None;
        }

        debug!("While checking if {} is linear", e);
        debug!("Proceding with advanced variable substitution");
        let mut zero_expressions = Vec::with_capacity(sub_vars.len());
        for sub_var in sub_vars {
            let definition = self.non_linear_components.get_by_left(&sub_var).unwrap();
            debug!("Found substitution v{} = {}", sub_var, definition);
            let zero = Expr::Var(sub_var) - definition.clone();
            debug!("Using zero expression {}", zero);
            zero_expressions.push(zero);
        }

        let mut var_map = BiMap::new();
        let mut t = 0;

        let reduced_e = reduce_vars(e.clone(), &mut var_map, &mut t);
        let reduced_zeros: Vec<_> = zero_expressions
            .iter()
            .cloned()
            .map(|zero| reduce_vars(zero, &mut var_map, &mut t))
            .collect();
        if t > 10 {
            return None;
        }

        let se = self.calc_signature(&reduced_e, t);
        debug!("Using signature {:?}", se);
        let zero_signatures: Vec<_> = reduced_zeros
            .iter()
            .map(|zero| self.calc_signature(zero, t))
            .collect();

        if zero_expressions.len() == 1 {
            let see = &zero_signatures[0];
            debug!("Using zero signature {:?}", see);
            if let Some(lambda) = find_lambda_int(&se, see, 0, 1, self.n) {
                debug!("Found lambda that creates a [0, 1] signature: {:?}", lambda);
                return Some(e - lambda * zero_expressions.remove(0));
            }
            if let Some(lambda) = find_lambda_int(&se, see, -1, -2, self.n) {
                debug!(
                    "Found lambda that creates a [-1, -2] signature: {:?}",
                    lambda
                );
                return Some(e - lambda * zero_expressions.remove(0));
            }
            return None;
        }

        for (a, b) in [(0, 1), (-1, -2)] {
            if let Some((left, right)) =
                find_two_lambdas_int(&se, &zero_signatures[0], &zero_signatures[1], a, b, self.n)
            {
                debug!("Found two substitution lambdas: {}, {}", left, right);
                return Some(
                    e - left * zero_expressions[0].clone() - right * zero_expressions[1].clone(),
                );
            }
        }
        None
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
    fn make_bitwise(&mut self, e: Expr, mut mask: u64) -> Expr {
        match e {
            // -1 and 0 are bitwise
            Expr::Const(c) => {
                if c.get(mask) == 0 || c.get(mask) == self.mask {
                    e
                } else {
                    self.hide_in_var(e, mask)
                }
            }

            // Variables are bitwise
            Expr::Var(_) => e,

            Expr::Not(_) | Expr::Or(_) | Expr::Xor(_) => {
                // if only bitwise
                // self
                // if has negatives, try and fix the "biphased" problem
                // worst case
                e.map(|e| self.make_bitwise(e, mask))
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
                    return Expr::zero();
                }

                Expr::And(
                    terms
                        .into_iter()
                        .map(|e| self.make_bitwise(e, mask))
                        .collect(),
                )
            }

            // A Linear MBA might "hide" a bitwise expression
            Expr::Add(_) | Expr::Scale(_, _) => {
                let previous = e.clone();
                let l = e.map(|e| self.make_linear(e, mask));
                if let Some(l) = self.is_linear_bitwise(l, mask) {
                    l
                } else {
                    self.hide_in_var(previous, mask)
                }
            }

            _ => self.hide_in_var(e, mask),
        }
    }

    /// Turns an expression into a bitwise product
    fn make_product(&mut self, e: Expr) -> Expr {
        match &e {
            Expr::Mul(terms) => {
                self.degree = max(self.degree, terms.len());
                e.map(|e| self.make_bitwise(e, self.mask))
            }

            // This makes life better on much easier
            _ => Expr::Mul(vec![self.make_bitwise(e, self.mask)]),
        }
    }

    /// Turns an expression into a scaled bitwise product
    fn make_scaled_product(&mut self, e: Expr) -> Expr {
        match e {
            Expr::Const(_) => e,
            Expr::Scale(_, _) => e.map(|e| self.make_product(e)),
            _ => self.make_product(e),
        }
    }

    /// Turns an expression into a polynomial expression
    fn make_polynomial(&mut self, e: Expr) -> Expr {
        match e {
            Expr::Add(_) => e.map(|e| self.make_scaled_product(e)),
            _ => self.make_scaled_product(e),
        }
    }

    /// Turns an expression into a scaled bitwise expression
    fn make_scaled_bitwise(&mut self, e: Expr, mask: u64) -> Expr {
        match e {
            Expr::Const(_) => e,
            Expr::Scale(_, _) => e.map(|e| self.make_bitwise(e, mask)),
            _ => self.make_bitwise(e, mask),
        }
    }

    /// Turns an expression into a linear expression
    fn make_linear(&mut self, e: Expr, mask: u64) -> Expr {
        match e {
            Expr::Add(_) => e.map(|e| self.make_scaled_bitwise(e, mask)),
            _ => self.make_scaled_bitwise(e, mask),
        }
    }

    fn is_linear(&self, e: &Expr) -> bool {
        fn is_bitwise(e: &Expr, mask: u64) -> bool {
            match e {
                // -1 and 0 are bitwise
                Expr::Const(c) => (c.get(mask) == 0) || (c.get(mask) == mask),

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

fn simplify_mba_inner(l_cache: &mut HashMap<Expr, Expr>, e: Expr, n: u8) -> Expr {
    let mut solver = MBASolver::new(l_cache, &e, n);
    solver.solve(e)
}

// I should probably add a flag for recursive simplification
pub fn simplify_mba(e: Expr, n: u8) -> Expr {
    let mask = make_mask(n);
    let mut e = e.reduce(mask);

    let mut l_cache = HashMap::new();

    let mut size = usize::MAX;
    loop {
        e = simplify_mba_inner(&mut l_cache, e, n);
        debug!("e: {}", e);
        let sz = e.size();

        if size == sz {
            break;
        }

        size = sz;
    }

    e
}
