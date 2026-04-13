use std::collections::HashMap;

pub struct BiMap<L, R> {
    left: HashMap<L, R>,
    right: HashMap<R, L>,
}

impl<L: Eq + std::hash::Hash + Clone, R: Eq + std::hash::Hash + Clone> BiMap<L, R> {
    pub fn new() -> Self {
        Self {
            left: HashMap::new(),
            right: HashMap::new(),
        }
    }

    pub fn insert(&mut self, l: L, r: R) {
        if let Some(old_r) = self.left.insert(l.clone(), r.clone()) {
            self.right.remove(&old_r);
        }
        if let Some(old_l) = self.right.insert(r, l) {
            self.left.remove(&old_l);
        }
    }

    pub fn get_by_left(&self, l: &L) -> Option<&R> {
        self.left.get(l)
    }

    pub fn get_by_right(&self, r: &R) -> Option<&L> {
        self.right.get(r)
    }

    pub fn len(&self) -> usize {
        self.left.len()
    }
}
