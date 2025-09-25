use rand::{Rng, rngs::ThreadRng, seq::IndexedRandom, thread_rng};

use crate::expr::{self, Binop, Expr};

pub struct Node {
    pub id: usize,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub expression: Expr,
    pub score: f64,
    pub visits: usize,
    pub active: bool,
}

impl Node {
    pub fn new(expr: Expr, parent: usize) -> Self {
        Self {
            id: 0,
            parent: Some(parent),
            children: vec![],
            expression: expr,
            score: 0.0,
            visits: 0,
            active: true,
        }
    }
}

impl Default for Node {
    fn default() -> Self {
        Self {
            id: 0,
            parent: None,
            children: vec![],
            expression: Expr::Var(2),
            score: 0.0,
            visits: 0,
            active: true,
        }
    }
}

pub struct IO {
    pub v0: u128,
    pub v1: u128,
    pub out: u128,
}

pub struct MCTS {
    n: usize,
    pub nodes: Vec<Node>,
    pub ios: Vec<IO>,
    next_non_terminal: usize,

    pub playout_depth: usize,
    pub C: f64,
    pub iterations: usize,
}

// Do the 2 expressions match on each non terminal
fn is_equivalent(e1: &Expr, e2: &Expr) -> bool {
    match (e1, e2) {
        (Expr::Var(v1), Expr::Var(v2)) => v1 == v2 || (*v1 > 1 && *v2 > 1),
        (Expr::Const(c1), Expr::Const(c2)) => c1 == c2,

        (Expr::Not(e1), Expr::Not(e2)) | (Expr::Neg(e1), Expr::Neg(e2)) => is_equivalent(e1, e2),

        (Expr::Scale(v1, e1), Expr::Scale(v2, e2)) => v1 == v2 && is_equivalent(e1, e2),

        (Expr::And(e1), Expr::And(e2))
        | (Expr::Or(e1), Expr::Or(e2))
        | (Expr::Xor(e1), Expr::Xor(e2))
        | (Expr::Add(e1), Expr::Add(e2))
        | (Expr::Sub(e1), Expr::Sub(e2))
        | (Expr::Mul(e1), Expr::Mul(e2)) => e1.iter().zip(e2).all(|(e1, e2)| is_equivalent(e1, e2)),

        (Expr::Shl(b1), Expr::Shl(b2))
        | (Expr::Shr(b1), Expr::Shr(b2))
        | (Expr::RshiftS(b1), Expr::RshiftS(b2))
        | (Expr::Le(b1), Expr::Le(b2))
        | (Expr::Lt(b1), Expr::Lt(b2))
        | (Expr::Ge(b1), Expr::Ge(b2))
        | (Expr::Gt(b1), Expr::Gt(b2))
        | (Expr::LeS(b1), Expr::LeS(b2))
        | (Expr::LtS(b1), Expr::LtS(b2))
        | (Expr::GeS(b1), Expr::GeS(b2))
        | (Expr::GtS(b1), Expr::GtS(b2))
        | (Expr::Ne(b1), Expr::Ne(b2))
        | (Expr::Eq(b1), Expr::Eq(b2)) => {
            is_equivalent(&b1.left, &b2.left) && is_equivalent(&b1.right, &b2.right)
        }

        _ => false,
    }
}

fn get_non_terminals(e: &Expr) -> Vec<usize> {
    match e {
        Expr::Var(n) => {
            if *n > 1 {
                vec![*n]
            } else {
                vec![]
            }
        }

        Expr::Const(_) => vec![],

        Expr::Not(expr) | Expr::Neg(expr) | Expr::Scale(_, expr) => get_non_terminals(expr),

        Expr::And(exprs)
        | Expr::Or(exprs)
        | Expr::Xor(exprs)
        | Expr::Add(exprs)
        | Expr::Sub(exprs)
        | Expr::Mul(exprs) => exprs.iter().flat_map(|e| get_non_terminals(e)).collect(),

        Expr::Shl(binop)
        | Expr::Shr(binop)
        | Expr::RshiftS(binop)
        | Expr::Le(binop)
        | Expr::Lt(binop)
        | Expr::Ge(binop)
        | Expr::Gt(binop)
        | Expr::LeS(binop)
        | Expr::LtS(binop)
        | Expr::GeS(binop)
        | Expr::GtS(binop)
        | Expr::Ne(binop)
        | Expr::Eq(binop) => vec![
            get_non_terminals(&binop.left),
            get_non_terminals(&binop.right),
        ]
        .concat(),
    }
}

fn replace_random_non_terminal<R: Rng>(r: &mut R, e: Expr, replacement: &Expr) -> Expr {
    let targets = get_non_terminals(&e);
    let target = *targets.choose(r).unwrap();
    e.replace_var(target, replacement)
}

impl MCTS {
    pub fn new(n: usize, expr: Expr) -> Self {
        let io_count = 20;

        let mut ios = Vec::<IO>::with_capacity(io_count);

        for _ in 0..io_count {
            let v0 = rand::random_range(0..2u128.pow(n as u32));
            let v1 = rand::random_range(0..2u128.pow(n as u32));

            ios.push(IO {
                v0,
                v1,
                out: expr.eval(&[v0, v1]),
            });
        }

        Self {
            n,
            nodes: vec![Node::default()],
            ios,
            playout_depth: 1,
            next_non_terminal: 3,
            iterations: 100,
            C: 0.5,
        }
    }

    fn score(&self, node: usize) -> f64 {
        let e = &self.nodes[node].expression;

        let hamming_distance = |a: u128, b: u128| -> u32 { (a ^ b).count_ones() };

        let mut cost = 0.0;

        for io in &self.ios {
            let measured = e.eval(&[io.v0, io.v1]);

            let h = hamming_distance(measured, io.out) as f64 / self.n as f64;
            let lo = measured.leading_ones().abs_diff(io.out.leading_ones()) as f64 / self.n as f64;
            let lz =
                measured.leading_zeros().abs_diff(io.out.leading_zeros()) as f64 / self.n as f64;
            let to =
                measured.trailing_ones().abs_diff(io.out.trailing_ones()) as f64 / self.n as f64;
            let tz =
                measured.trailing_zeros().abs_diff(io.out.trailing_zeros()) as f64 / self.n as f64;
            let d = measured.abs_diff(io.out) as f64 / 2.0f64.powf(self.n as f64);

            cost += (h + lo + lz + to + tz + d) / 6.0;
        }

        cost /= self.ios.len() as f64;

        1.0 - cost.min(1.0)
    }

    fn get_or_create_child(&mut self, parent: usize, expr: Expr) -> (usize, bool) {
        for &child in &self.nodes[parent].children {
            if is_equivalent(&self.nodes[child].expression, &expr) {
                return (child, false);
            }
        }

        let mut child = Node::new(expr, parent);
        let id = self.nodes.len();
        child.id = id;
        self.nodes.push(child);

        self.nodes[parent].children.push(id);

        (id, true)
    }

    fn get_non_terminal(&mut self) -> Expr {
        let e = Expr::Var(self.next_non_terminal);
        self.next_non_terminal += 1;
        e
    }

    fn random_playout<R: Rng>(&mut self, rng: &mut R, node: usize, depth: usize) -> usize {
        let non_terminals = get_non_terminals(&self.nodes[node].expression);

        if non_terminals.is_empty() {
            return node;
        }

        let replacement = if depth == 0 {
            // We only terminals
            match rng.random_range(0..=2) {
                0 => Expr::Var(0),
                1 => Expr::Var(1),
                2 => Expr::Const(1),

                _ => unreachable!(),
            }
        } else {
            match rng.random_range(0..=12) {
                0 => Expr::Var(0),
                1 => Expr::Var(1),
                2 => Expr::Const(1),

                3 => !self.get_non_terminal(),
                4 => -self.get_non_terminal(),

                5 => self.get_non_terminal() ^ self.get_non_terminal(),
                6 => self.get_non_terminal() | self.get_non_terminal(),
                7 => self.get_non_terminal() & self.get_non_terminal(),

                8 => self.get_non_terminal() << self.get_non_terminal(),
                9 => self.get_non_terminal() >> self.get_non_terminal(),

                10 => self.get_non_terminal() + self.get_non_terminal(),
                11 => self.get_non_terminal() - self.get_non_terminal(),
                12 => self.get_non_terminal() * self.get_non_terminal(),

                _ => unreachable!(),
            }
        };

        let e = replace_random_non_terminal(rng, self.nodes[node].expression.clone(), &replacement);

        let (node, created) = self.get_or_create_child(node, e);

        self.random_playout(
            rng,
            node,
            if created {
                depth.saturating_sub(1)
            } else {
                depth
            },
        )
    }

    pub fn run(&mut self) -> Expr {
        let mut rng = rand::rng();

        for i in 0..self.iterations {
            let uct = |n: &Node| {
                n.score
                    + self.C * (self.iterations - i) as f64 / self.iterations as f64
                        * ((n.parent.map_or(100 * n.visits, |p| self.nodes[p].visits) as f64)
                            .log2()
                            / n.visits as f64)
                            .sqrt()
            };

            // Gets the most promising node
            let mut node = self
                .nodes
                .iter()
                .enumerate()
                .filter(|&(_, n)| n.active)
                .max_by(|a: &(usize, &Node), b| {
                    uct(a.1)
                        .partial_cmp(&uct(b.1))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap();

            // Random playout
            node = self.random_playout(&mut rng, node, self.playout_depth);

            // Leaf node
            let leaf = node;
            self.nodes[node].active = false;

            // Calculates the score
            let score = self.score(node);

            // Propagate the score
            loop {
                let n = &mut self.nodes[node];
                n.score = (n.score * n.visits as f64 + score) / (n.visits + 1) as f64;
                n.visits += 1;

                if let Some(id) = n.parent {
                    node = id;
                } else {
                    break;
                }
            }

            if 1.0 - score < 0.001 {
                // We found the correct value
                let sol = self
                    .get_or_create_child(leaf, self.nodes[leaf].expression.clone().arith_reduce())
                    .0;
                self.nodes[sol].score = 2.0;
                return self.nodes[sol].expression.clone();
            }
        }

        // Find the terminal node with the highest score
        let leaf = self
            .nodes
            .iter()
            .enumerate()
            .filter(|&(_, n)| !n.active)
            .max_by(|a, b| a.1.score.partial_cmp(&b.1.score).unwrap())
            .map(|(i, _)| i)
            .unwrap();

        let sol = self
            .get_or_create_child(leaf, self.nodes[leaf].expression.clone().arith_reduce())
            .0;
        self.nodes[sol].score = 2.0;
        return self.nodes[sol].expression.clone();
    }
}
