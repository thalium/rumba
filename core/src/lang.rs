use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Display},
    mem,
};

use indexmap::IndexMap;

use crate::{
    expr::{Expr, VarId},
    simplify::simplify_mba,
    varint::make_mask,
};

#[derive(Debug, PartialEq, Eq)]
pub struct Insn {
    pub id: VarId,
    pub ty: u8,
    pub kind: InsnKind,
}

impl Display for Insn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            InsnKind::Unknown(args) => f.write_fmt(format_args!(
                "u{} v{} = unknown({})",
                self.ty,
                self.id,
                args.iter()
                    .map(|v| format!("v{}", v))
                    .collect::<Vec<String>>()
                    .join(",")
            )),

            InsnKind::Assign(e) => f.write_fmt(format_args!(
                "u{} v{} = {}",
                self.ty,
                self.id,
                e.repr(self.ty, make_mask(self.ty), false, false)
            )),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum InsnKind {
    Assign(Expr),
    Unknown(Vec<VarId>),
}

type Uses = HashSet<VarId>;

#[derive(Debug)]
pub struct Program {
    pub insns: IndexMap<VarId, Insn>,
    users: HashMap<VarId, Uses>,
}

impl Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (_, insn) in &self.insns {
            f.write_fmt(format_args!("{}\n", insn))?;
        }
        Ok(())
    }
}

impl<'a> IntoIterator for &'a Program {
    type Item = &'a Insn;

    type IntoIter = indexmap::map::Values<'a, VarId, Insn>;

    fn into_iter(self) -> Self::IntoIter {
        self.insns.values()
    }
}

impl Default for Program {
    fn default() -> Self {
        Self {
            insns: IndexMap::new(),
            users: HashMap::new(),
        }
    }
}

macro_rules! Errf {
    ($($arg:tt)*) => {
        Err(format!($($arg)*))
    };
}

/// A Rumba IR program
impl Program {
    fn get_mut_uses(&mut self, caller: VarId, var: VarId) -> Result<&mut Uses, String> {
        self.users.get_mut(&var).ok_or(format!(
            "ERROR[v{}] Variable {} is not defined",
            caller, var
        ))
    }

    fn get_insn(&self, caller: VarId, id: VarId) -> Result<&Insn, String> {
        self.insns
            .get(&id)
            .ok_or(format!("ERROR[v{}] Instruction {} is missing", caller, id))
    }

    /// Adds the uses of a instruction into the program
    fn add_uses(&mut self, insn: &Insn) -> Result<(), String> {
        let id = insn.id;

        let mut add_use = |var: VarId| -> Result<(), String> {
            let uses = self.get_mut_uses(id, var)?;
            uses.insert(id);
            Ok(())
        };

        match &insn.kind {
            InsnKind::Unknown(args) => {
                for arg in args {
                    add_use(*arg)?;
                }
            }

            InsnKind::Assign(expr) => {
                let vars = expr.get_vars();
                for var in vars {
                    add_use(var)?;
                }
            }
        }

        Ok(())
    }

    /// Adds the definition of an instruction into the program
    fn add_def(&mut self, insn: &Insn) -> Result<(), String> {
        if self.users.insert(insn.id, HashSet::new()).is_some() {
            return Errf!("Redeclaring previously declared var: v{}", insn.id);
        }

        Ok(())
    }

    /// Adds an instruction at the end of the current program
    pub fn push(&mut self, insn: Insn) -> Result<(), String> {
        // Updates the users
        self.add_uses(&insn)?;

        // Update the defs and creates an empty uses set
        self.add_def(&insn)?;

        self.insns.insert(insn.id, insn);

        Ok(())
    }

    /// Adds multiple instructions at the end of the current program
    pub fn append(&mut self, insns: Vec<Insn>) -> Result<(), String> {
        for insn in insns {
            self.push(insn)?;
        }
        Ok(())
    }

    /// Removes an unused assignement instruction
    fn remove(&mut self, id: VarId) -> Result<(), String> {
        let e = match self
            .insns
            .shift_remove(&id)
            .ok_or(format!(
                "Attempting to remove instruction with id `{}` which does not exist!",
                id
            ))?
            .kind
        {
            InsnKind::Assign(e) => e,

            _ => {
                return Errf!("Attempting to remove an unknown instruction: v{}", id);
            }
        };

        // Verifies this instruction was indeed unused
        if cfg!(debug_assertions) && self.users.get(&id).is_none_or(|uses| !uses.is_empty()) {
            return Errf!("Attempting to remove v{} which is still used", id);
        }
        self.users.remove(&id);

        // Remove this instruction's uses
        let vars = e.get_vars();
        for var in vars {
            let uses = self.get_mut_uses(id, var)?;
            uses.remove(&id);
        }

        Ok(())
    }

    /// Removes all unused assignements
    fn remove_dead(&mut self) -> Result<(), String> {
        loop {
            let mut queue = vec![];

            for (id, insn) in &self.insns {
                if matches!(&insn.kind, InsnKind::Assign(_))
                    && self.users.get(id).is_none_or(|uses| uses.is_empty())
                {
                    queue.push(*id);
                }
            }

            if queue.is_empty() {
                break;
            }

            for id in queue {
                self.remove(id)?;
            }
        }

        Ok(())
    }

    /// Attempts to simplify the given instruction
    /// This only works on assignements
    fn simplify_insn(&mut self, insn: &mut Insn) -> Result<(), String> {
        let e = match &mut insn.kind {
            InsnKind::Assign(e) => e,
            _ => {
                return Ok(());
            }
        };

        let id = insn.id;

        // Take ownership of the expression
        let mut expr = mem::replace(e, Expr::zero());
        let vars = expr.get_vars();

        for var in vars {
            // Replace variables by their expressions if we know their values
            let insn = self.get_insn(id, var)?;

            if let InsnKind::Assign(replacement) = &insn.kind {
                // TODO: handle operations between varaibles of different sizes
                expr = expr.replace_var(var, replacement);
            }

            // Removes this use, we will add all remaining uses after we modifying the expression
            let uses = self.get_mut_uses(id, var)?;
            uses.remove(&id);
        }

        // Simplify the expression
        *e = simplify_mba(expr, insn.ty);

        // Update this instruction's uses
        for var in e.get_vars() {
            let uses = self.get_mut_uses(id, var)?;
            uses.insert(id);
        }

        Ok(())
    }

    /// Simplifies the given program
    pub fn simplify(&mut self) -> Result<(), String> {
        let insns = std::mem::take(&mut self.insns);

        for (id, mut insn) in insns {
            self.simplify_insn(&mut insn)?;
            self.insns.insert(id, insn);
        }

        self.remove_dead()?;

        Ok(())
    }
}
