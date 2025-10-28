use std::{cmp::max, usize};

use crate::{bimap::BiMap, expr::Expr};

use log::debug;

// Returns all sorted sublists of [0, t[ ordered by size
fn sorted_sublists(t: usize) -> Vec<Vec<usize>> {
    fn combine(
        nums: &Vec<usize>,
        sz: usize,
        start: usize,
        current: &mut Vec<usize>,
        result: &mut Vec<Vec<usize>>,
    ) {
        if current.len() == sz {
            result.push(current.clone());
            return;
        }
        for i in start..nums.len() {
            current.push(nums[i]);
            combine(nums, sz, i + 1, current, result);
            current.pop();
        }
    }

    let nums: Vec<usize> = (0..t).collect();
    let mut result = Vec::new();

    for sz in 1..=nums.len() {
        combine(&nums, sz, 0, &mut Vec::new(), &mut result);
    }

    result
}

// The amount of leading zeros in x's truth table
fn leading_zeros(x: usize) -> u128 {
    1u128 << x
}

fn sub_coeff(tt: &mut Vec<u128>, coeff: u128, index: usize, sublist: Vec<usize>) {
    let are_vars_true = |i: usize| sublist[1..].iter().copied().all(|v| ((i >> v) & 1) == 1);

    let gp_size = leading_zeros(sublist[0]) as usize;
    let period = 2 * gp_size;

    let mut start = index;
    while start < tt.len() {
        for i in start..(start + gp_size) {
            if sublist.len() == 1 || are_vars_true(i) {
                tt[i] = tt[i].wrapping_sub(coeff);
            }
        }
        start += period;
    }
}

fn get_degree(e: &Expr) -> usize {
    match e {
        Expr::Scale(_, e) => get_degree(e),

        Expr::Mul(terms) => terms.len(),

        Expr::Const(_) => 0,

        _ => 1,
    }
}

struct MBASolver {
    /// The number of bits being considered
    n: u32,

    /// The map between non linear components and variables
    non_linear_components: BiMap<usize, Expr>,

    /// The number of variables in the expression
    t: usize,

    /// The degree of the polynomial expression
    degree: usize,
}

impl MBASolver {
    /// Create a new Solver
    fn new(e: &Expr, n: u32) -> Self {
        Self {
            non_linear_components: BiMap::new(),
            t: e.get_vars().iter().max().unwrap_or(&0) + 1,
            degree: 1,
            n,
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
    fn solve(&mut self, e: Expr) -> Expr {
        // TODO: Remove this only needs to be done once
        let e = e.arith_reduce_mod(self.mask());

        let p = self.make_polynomial(e);
        let p = self.solve_polynomial(p);

        // This was a non linear MBA
        if self.non_linear_components.len() != 0 {
            let e = self.poly_to_nonpoly(p);
            e.arith_reduce_mod(self.mask())
        } else {
            p
        }
    }

    /// Reduces the number of variables present in the MBA
    fn reduce_vars(&self, e: Expr, var_map: &mut BiMap<usize, usize>, t: &mut usize) -> Expr {
        match e {
            Expr::Var(v) => {
                let vv = if let Some(v) = var_map.get_by_left(&v) {
                    *v
                } else {
                    let vv = *t;
                    var_map.insert(v, vv);
                    *t += 1;
                    vv
                };
                Expr::Var(vv)
            }

            _ => e.map(|e| self.reduce_vars(e, var_map, t)),
        }
    }

    /// Restores the varialbes in the mba
    fn restore_vars(&self, e: Expr, var_map: &BiMap<usize, usize>) -> Expr {
        match e {
            Expr::Var(v) => {
                let vv = if let Some(v) = var_map.get_by_right(&v) {
                    *v
                } else {
                    panic!("Can't find variable");
                };
                Expr::Var(vv)
            }

            _ => e.map(|e| self.restore_vars(e, var_map)),
        }
    }

    /// Calcluates the signature of a linear MBA
    fn calc_signature(&self, e: &Expr, t: usize) -> Vec<u128> {
        e.truth_table(2, t)
            .iter()
            .map(|&x| x & self.mask())
            .collect()
    }

    /// Solves a linear MBA
    fn solve_linear_inner(&self, e: Expr, t: usize) -> Expr {
        let mut signature = self.calc_signature(&e, t);

        let mut terms: Vec<Expr> = vec![];

        // The constant term
        let constant = signature[0];

        if constant != 0 {
            // technically it's -constant * (-1)
            terms.push(Expr::Const(constant));

            for v in &mut signature {
                *v = v.wrapping_sub(constant);
            }
        }

        for sublist in sorted_sublists(t) {
            // The index of the first non zero value of this conjuction in the truth table
            let index: u128 = sublist.iter().copied().map(leading_zeros).sum();
            let coeff = signature[index as usize] & self.mask();

            if coeff == 0 {
                continue;
            }

            let conjunction = Expr::And(sublist.iter().copied().map(|v| Expr::Var(v)).collect());

            terms.push(match coeff {
                1 => conjunction,
                c => c * conjunction,
            });

            sub_coeff(&mut signature, coeff, index as usize, sublist);
        }

        match terms.len() {
            0 => Expr::Const(0),
            1 => terms.into_iter().next().unwrap(),
            _ => Expr::Add(terms),
        }
    }

    /// Simplifies a linear MBA
    fn solve_linear(&self, e: Expr) -> Expr {
        let mut var_map = BiMap::<usize, usize>::new();
        let mut t = 0;

        debug!("Solving linear MBA: {}", e);

        // Reduce the number of variables in the expression
        let e = self.reduce_vars(e, &mut var_map, &mut t);

        debug!("Reduced number of variables to equivalent problem: {}", e);

        let e = self.solve_linear_inner(e, t);

        let e = self.restore_vars(e, &var_map);

        debug!("Found solution to linear MBA: {}", e);

        e
    }

    /// Turns a polynomial MBA to a linear one using PCT
    fn poly_to_linear(&self, e: Expr, deg: usize) -> Expr {
        match e {
            Expr::Var(v) => Expr::Var(deg * self.t + v),
            Expr::Mul(terms) => Expr::And(
                terms
                    .into_iter()
                    .enumerate()
                    .map(|(i, e)| self.poly_to_linear(e, deg + i))
                    .collect(),
            ),
            _ => e.map(|e| self.poly_to_linear(e, deg)),
        }
    }

    fn poly_sum_to_linear(&self, e: Expr) -> Vec<Vec<Expr>> {
        let mut polynomials = vec![vec![]; self.degree + 1];

        match e {
            Expr::Add(terms) => {
                for term in terms {
                    let deg = get_degree(&term);
                    polynomials[deg].push(if deg >= 2 {
                        self.poly_to_linear(term, 0)
                    } else {
                        term
                    });
                }
            }

            _ => {
                let deg = get_degree(&e);
                polynomials[deg].push(if deg >= 2 {
                    self.poly_to_linear(e, 0)
                } else {
                    e
                });
            }
        };

        polynomials
    }

    /// Turns a linear MBA into a polynomial one using the inverse PCT
    fn linear_to_poly(&self, e: Expr) -> Expr {
        match &e {
            Expr::And(terms) => {
                // The sign correction -> see paper
                let mut s = 1u128;

                let mut grouped: Vec<Vec<usize>> = vec![vec![]; self.degree];

                for t in terms {
                    if let Expr::Var(v) = t {
                        let d = v / self.t;
                        grouped[d].push(v % self.t);
                    } else {
                        panic!("Solved linear MBA is in an unrecognized form")
                    }
                }

                let mut terms: Vec<Expr> = vec![];

                for g in grouped {
                    if g.len() == 0 {
                        s = s.wrapping_mul(u128::MAX);
                        continue;
                    }

                    terms.push(Expr::And(g.into_iter().map(|v| Expr::Var(v)).collect()));
                }

                s * Expr::Mul(terms)
            }

            Expr::Add(_) | Expr::Scale(_, _) => e.map(|e| self.linear_to_poly(e)),

            Expr::Var(v) => {
                // The sign correction -> see paper
                let s = if self.degree & 1 == 0 { u128::MAX } else { 1 };
                s * Expr::Var(v % self.degree)
            }

            Expr::Const(_) => {
                // The sign correction -> see paper
                let s = if self.degree & 1 == 0 { u128::MAX } else { 1 };
                s * e
            }

            _ => panic!("Solved linear MBA is in an unrecognized form"),
        }
    }

    /// Solves a polynomial MBA
    fn solve_polynomial(&self, e: Expr) -> Expr {
        debug!("Solving polynomial MBA: {}", e);

        // This is a linear MBA
        if self.degree == 1 {
            debug!("This is a linear MBA");
            return self.solve_linear(e);
        }

        let polys = self.poly_sum_to_linear(e);

        let mut terms = vec![];

        for (i, poly) in polys.into_iter().enumerate() {
            if poly.is_empty() {
                continue;
            }

            match i {
                0 => terms.push(Expr::Add(poly)),
                1 => terms.push(self.solve_linear(Expr::Add(poly))),
                _ => terms.push(self.linear_to_poly(self.solve_linear(Expr::Add(poly)))),
            }
        }

        let e = Expr::Add(terms).arith_reduce_mod(self.mask());

        debug!("Found polynomial solution: {}", e);

        e
    }

    /// The bitmask for the problem size
    fn mask(&self) -> u128 {
        (1u128 << self.n) - 1
    }

    /// Hides a non linear element behind a variable
    fn to_var(&mut self, e: Expr) -> Expr {
        let e = match e {
            Expr::Const(_) => e,
            _ => simplify_mba_inner(e, self.n),
        };

        if let Some(v) = self.non_linear_components.get_by_right(&e) {
            Expr::Var(*v)
        } else {
            if let Expr::Const(c) = e {
                if let Some(v) = self
                    .non_linear_components
                    .get_by_right(&Expr::Const((!c) & self.mask()))
                {
                    return !Expr::Var(*v);
                }
            }
            let v = self.t;
            self.t += 1;
            self.non_linear_components.insert(v, e);
            Expr::Var(v)
        }
    }

    // A Linear MBA might "hide" a bitwise expression
    fn is_linear_bitwise(&self, l: &Expr) -> bool {
        let mut t = 0;
        let mut var_map = BiMap::new();
        let e = self.reduce_vars(l.clone(), &mut var_map, &mut t);

        let s = self.calc_signature(&e, t);

        let minus_one = (1u128 << self.degree) - 1;
        let minus_two = (1u128 << self.degree) - 2;

        if s[0] == 0 {
            s.iter().all(|&x| x == 0 || x == 1)
        } else if s[0] == minus_one {
            s.iter().all(|&x| x == minus_one || x == minus_two)
        } else {
            false
        }
    }

    /// Turns an expression into a bitwise expression
    fn make_bitwise(&mut self, e: Expr) -> Expr {
        match e {
            // -1 and 0 are bitwise
            Expr::Const(c) => {
                if c & self.mask() == 0 {
                    e
                } else if c.wrapping_add(1) & self.mask() == 0 {
                    e
                } else {
                    self.to_var(e)
                }
            }

            // Variables are bitwise
            Expr::Var(_) => e,

            // A boolean expression is bitwise if all its sub expressions are bitwise
            Expr::Not(_) | Expr::And(_) | Expr::Or(_) | Expr::Xor(_) => {
                e.map(|e| self.make_bitwise(e))
            }

            // A Linear MBA might "hide" a bitwise expression
            Expr::Add(_) | Expr::Scale(_, _) => {
                let previous = e.clone();
                let l = e.map(|e| self.make_linear(e));
                if self.is_linear_bitwise(&l) {
                    l
                } else {
                    self.to_var(previous)
                }
            }

            _ => self.to_var(e),
        }
    }

    /// Turns an expression into a bitwise product
    fn make_product(&mut self, e: Expr) -> Expr {
        match &e {
            Expr::Mul(terms) => {
                self.degree = max(self.degree, terms.len());
                e.map(|e| self.make_bitwise(e))
            }

            // This makes life better on much easier
            _ => Expr::Mul(vec![self.make_bitwise(e)]),
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
    fn make_scaled_bitwise(&mut self, e: Expr) -> Expr {
        match e {
            Expr::Const(_) => e,
            Expr::Scale(_, _) => e.map(|e| self.make_bitwise(e)),
            _ => self.make_bitwise(e),
        }
    }

    /// Turns an expression into a linear expression
    fn make_linear(&mut self, e: Expr) -> Expr {
        match e {
            Expr::Add(_) => e.map(|e| self.make_scaled_bitwise(e)),
            _ => self.make_scaled_bitwise(e),
        }
    }
}

fn simplify_mba_inner(e: Expr, n: u32) -> Expr {
    let mut solver = MBASolver::new(&e, n);
    solver.solve(e)
}

pub fn simplify_mba(e: Expr, n: u32) -> Expr {
    simplify_mba_inner(e.arith_reduce_mod((1 << n) - 1), n)
}
