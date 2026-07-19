//! Implementation of register map for placing variables

use std::collections::HashMap;

use crate::{
    bitset::BitSet,
    lex::Token,
    parse::{Args, Expr},
    stmt::{Insn, Stmt},
};

#[derive(Default, Clone, Debug)]
pub struct Regmap {
    var_places: HashMap<String, Option<u8>>,
    reg_contents: HashMap<u8, String>,
    // If a register is killed, then it may contain something other than
    // the placement from a basic block predecessor.
    killed_regs: BitSet,
}

pub fn parse_register(s: &str) -> Option<u8> {
    if let Some(tail) = s.strip_prefix('r') {
        if let Ok(n) = tail.parse()
            && n < 16
        {
            return Some(n);
        }
    }
    match s {
        // less sure about other aliases such as ip etc. Needed for asm compat tho
        "sp" => Some(13),
        "lr" => Some(14),
        "pc" => Some(15),
        _ => None,
    }
}

impl Regmap {
    fn place(&mut self, var: &str, reg: u8) {
        self.killed_regs.insert(reg as usize);
        if let Some(old_var) = self.reg_contents.insert(reg, var.to_owned()) {
            self.var_places.insert(old_var, None);
        }
        self.var_places.insert(var.to_owned(), Some(reg));
    }

    fn place_expr(&mut self, var: &Expr, place: &Token) {
        if let Some(id) = var.as_ident()
            && let Some(place_id) = place.as_ident()
        {
            if let Some(reg) = parse_register(place_id) {
                self.place(id, reg);
            }
        }
    }

    fn kill(&mut self, reg: u8) {
        if let Some(var) = self.reg_contents.remove(&reg) {
            self.var_places.insert(var, None);
        }
        self.killed_regs.insert(reg as usize);
    }

    fn kill_expr(&mut self, lhs: &Expr) {
        if let Some(id) = lhs.as_ident()
            && let Some(n) = parse_register(id)
        {
            self.kill(n);
        }
        // TODO: also handle slice
    }

    pub fn apply(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Assign(lhs, _op, _rhs) => self.kill_expr(lhs),
            Stmt::AssignPlace(lhs, place, _rhs) => self.place_expr(lhs, place),
            Stmt::WithFlagsAssignPlace(lhs, place, _rhs) => self.place_expr(lhs, place),
            Stmt::Insn(Insn::Bl(_)) => {
                // By ABI convention
                for r in [0, 1, 2, 3, 12, 14] {
                    self.kill(r);
                }
            }
            _ => (),
        }
    }

    /// Intersect predecessor regmap.
    ///
    /// Return true if any changes.
    pub fn intersect_pred(&mut self, pred: &Self) -> bool {
        let mut changed = false;
        for (var, &reg) in &pred.var_places {
            // TODO: entry would be a bit of an optimization
            if let Some(&this_reg) = self.var_places.get(var) {
                if reg != this_reg {
                    // conflicting placements of same var with different registers
                    if let Some(n) = this_reg
                        && !self.killed_regs.get(n as usize)
                    {
                        self.kill(n);
                        changed = true;
                    }
                    if let Some(n) = reg
                        && !self.killed_regs.get(n as usize)
                    {
                        self.kill(n);
                        changed = true;
                    }
                }
            } else if let Some(n) = reg {
                if self.reg_contents.contains_key(&n) {
                    // conflicting placement, different vars to same register
                    self.var_places.insert(var.clone(), None);
                    if !self.killed_regs.get(n as usize) {
                        self.kill(n);
                    }
                } else if !self.killed_regs.get(n as usize) {
                    self.var_places.insert(var.clone(), reg);
                    self.reg_contents.insert(n, var.clone());
                } else {
                    self.var_places.insert(var.clone(), None);
                }
                changed = true;
            } else {
                self.var_places.insert(var.clone(), None);
                changed = true;
            }
        }
        for reg in pred.killed_regs & !self.killed_regs {
            if !pred.reg_contents.contains_key(&(reg as u8)) {
                self.killed_regs.insert(reg);
                changed = true;
            }
        }
        changed
    }

    pub fn intersect(&mut self, sibling: &Self) {
        for (var, reg) in &mut self.var_places {
            if let Some(n) = reg {
                if let Some(their_reg) = sibling.var_places.get(var) {
                    if their_reg != &Some(*n) {
                        self.reg_contents.remove(n);
                        *reg = None;
                    }
                }
            }
        }
    }

    pub fn init_from_args(&mut self, args: &Args) {
        for i in 0..args.0.len().min(4) {
            if let Some(id) = args.0[i].var.as_ident() {
                self.place(id, i as u8);
            }
        }
    }

    pub fn lookup(&self, id: &str) -> Option<u8> {
        if let Some(reg) = self.var_places.get(id) {
            *reg
        } else {
            None
        }
    }
}
