use std::{cmp::max, collections::HashSet};

use crate::{
    utils::bimap::BiMap,
    utils::cache::{LinearCache, LocalCache, MbaCache},
    expr::{Expr, VarId},
    varint::make_mask,
};

use log::debug;

pub use crate::utils::error::SolveError;

mod lambda;
mod merge_hidden;

use lambda::{find_lambda_int, find_two_lambdas_int};

/// The largest number of variables a linear MBA may carry into the truth-table
/// solve. The signature is `2^t` wide, so this bounds one solve at 1M entries.
pub(crate) const MAX_VARS: usize = 20;

const MAX_SIMPLIFICATION_PASSES: usize = 8;

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
}

impl<'a, C: LinearCache> MBASolver<'a, C> {
    /// Create a new Solver
    fn new(l_cache: &'a C, e: &Expr, n: u8) -> Self {
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

    /// Solves a non polynomial MBA
    fn solve(&mut self, e: Expr) -> Result<Expr, SolveError> {
        // TODO: Remove this only needs to be done once
        let e = e.reduce_masked(self.mask);

        // Non-polynomial stage: structural pattern rewrites on the canonical
        // expression, before it is lifted to a polynomial.
        let e = crate::patterns::apply_patterns(e, self.mask);

        let p = self.make_polynomial(e)?;
        let p = self.merge_equal_hidden_components(p);
        self.degree = 1;
        let p = self.make_polynomial(p)?;
        let p = self.solve_polynomial(p)?;

        // This was a non linear MBA
        if self.non_linear_components.len() != 0 {
            let e = self.poly_to_nonpoly(p);
            debug!("After adding non linear components, found: {}", e);
            Ok(e.reduce_masked(self.mask))
        } else {
            Ok(p)
        }
    }

    /// Calcluates the signature of a linear MBA
    fn calc_signature(&self, e: &Expr, t: usize) -> Vec<u64> {
        e.truth_table_masked(t, self.mask)
    }

    /// Creates a conjuction sum for the given signature
    fn make_conjunction_sum(&self, mut signature: Vec<u64>, t: usize) -> Expr {
        let mut terms: Vec<Expr> = vec![];

        // The constant term
        let constant = signature[0];

        if constant != 0 {
            terms.push(Expr::Const(constant));

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
            1 => terms.remove(0),
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
                    let s: u64 = if self.degree & 1 == 0 { u64::MAX } else { 1 };
                    Expr::Const(s.wrapping_mul(c))
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
        let e = self.linear_to_poly(e)?.reduce_masked(self.mask);

        debug!("Found polynomial solution: {}", e);

        Ok(e)
    }

    /// Hides a non linear element behind a variable
    fn hide_in_var(&mut self, e: Expr, mask: u64) -> Result<Expr, SolveError> {
        debug!("e={} is not linear and will be replaced by a variable", e);

        let e = match e {
            Expr::Const(_) => e,
            _ => simplify_mba_inner(self.l_cache, e, mask.count_ones() as u8)?.reduce_masked(mask),
        };

        let note = (-e.clone() - Expr::make_const(1)).reduce_masked(mask);
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
            self.non_linear_components.insert(v, e);
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
        let mut sub_vars = vec![];

        for v in e.get_vars() {
            if let Some(definition) = self.non_linear_components.get_by_left(&v)
                && self.is_linear(definition)
            {
                sub_vars.push((v, definition));
            }
        }
        sub_vars.sort_unstable_by_key(|(variable, _)| variable.0);

        if sub_vars.is_empty() || sub_vars.len() > 2 {
            return None;
        }

        debug!("While checking if {} is linear", e);
        debug!("Proceding with advanced variable substitution");
        let mut zero_expressions = Vec::with_capacity(sub_vars.len());
        for (sub_var, definition) in sub_vars {
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

        let s = e.truth_table_masked(t, mask);

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
                if (c & mask) == 0 || (c & mask) == self.mask {
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
                        let c = c & mask;
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
                Expr::Const(c) => ((c & mask) == 0) || ((c & mask) == mask),

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

/// A reusable memo of solved linear MBAs.
///
/// Simplifying an expression solves many linear sub-MBAs; a `SimplifyCache`
/// remembers those solutions so that a caller simplifying many expressions — or
/// the same ones repeatedly across rounds of an analysis — pays for each distinct
/// linear solve once. Create one with [`SimplifyCache::new`] and hand it to
/// [`simplify_mba_cached`]. It is safe to share one across threads.
#[derive(Debug, Default)]
pub struct SimplifyCache(MbaCache);

impl SimplifyCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Simplifies a Mixed Boolean-Arithmetic expression on `n` bits.
pub fn simplify_mba(e: Expr, n: u8) -> Result<Expr, SolveError> {
    simplify_mba_with_cache(&LocalCache::new(), e, n)
}

/// [`simplify_mba`] against a caller-owned [`SimplifyCache`], so linear solves
/// are reused across calls.
pub fn simplify_mba_cached(e: Expr, n: u8, cache: &SimplifyCache) -> Result<Expr, SolveError> {
    simplify_mba_with_cache(&cache.0, e, n)
}

fn simplify_mba_with_cache<C: LinearCache>(
    cache: &C,
    e: Expr,
    n: u8,
) -> Result<Expr, SolveError> {
    let mask = make_mask(n);
    let e = e.reduce_masked(mask);
    simplify_to_fixed_point(e, |e| simplify_mba_inner(cache, e, n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn inverse_pct_round_trip_preserves_variable_index() {
        let cache = LocalCache::new();
        let mut solver = MBASolver::new(&cache, &Expr::Var(2.into()), 8);
        solver.degree = 2;
        let original = Expr::Var(2.into());
        let encoded = solver.poly_to_linear(original.clone(), 2);

        assert_eq!(encoded, Expr::Var(5.into()));
        assert_eq!(solver.linear_to_poly(encoded), Ok(u64::MAX * original));
    }

    #[test]
    fn fixed_point_continues_when_structure_changes_at_equal_size() {
        let result = simplify_to_fixed_point(Expr::Var(0.into()), |e| {
            Ok(match e {
                Expr::Var(VarId(0)) => !Expr::Var(1.into()),
                Expr::Not(inner) if *inner == Expr::Var(1.into()) => {
                    2u64 * Expr::Var(2.into())
                }
                Expr::Scale(c, inner) if c == 2 && *inner == Expr::Var(2.into()) => {
                    Expr::Var(3.into())
                }
                stable => stable,
            })
        });

        assert_eq!(result, Ok(Expr::Var(3.into())));
    }

    #[test]
    fn fixed_point_cycle_returns_smallest_expression() {
        let start = !Expr::Var(0.into());
        let result = simplify_to_fixed_point(start.clone(), |e| {
            if e == start {
                Ok(Expr::Var(1.into()))
            } else {
                Ok(start.clone())
            }
        });

        assert_eq!(result, Ok(Expr::Var(1.into())));
    }

    #[test]
    fn fixed_point_has_a_pass_limit() {
        let calls = Cell::new(0usize);
        let result = simplify_to_fixed_point(Expr::Var(0.into()), |e| {
            calls.set(calls.get() + 1);
            Ok(match e {
                Expr::Var(variable) => Expr::Var((variable.0 + 1).into()),
                other => other,
            })
        });

        assert_eq!(calls.get(), MAX_SIMPLIFICATION_PASSES);
        assert_eq!(result, Ok(Expr::Var(0.into())));
    }
}
