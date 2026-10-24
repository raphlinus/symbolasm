// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Implementation of register map for placing variables

use std::collections::HashMap;

use crate::{
    bitset::BitSet,
    lex::{TokBody, Token},
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
    /// Why each variable that was placed no longer is, for errors
    lost: HashMap<String, String>,
    /// What last happened to each killed register, for errors
    killed_why: HashMap<u8, String>,
}

fn reg_name(reg: u8) -> String {
    match reg {
        13 => "sp".into(),
        14 => "lr".into(),
        15 => "pc".into(),
        _ => format!("r{reg}"),
    }
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

/// Why a variable with different placements on two paths into a block
/// isn't placed there: `other` is its register on one path, `lost` that
/// path's reasons
fn paths_differ(var: &str, other: Option<u8>, lost: &HashMap<String, String>) -> String {
    match other {
        Some(r) => format!("it's in {} on another path into here", reg_name(r)),
        None => match lost.get(var).filter(|s| !s.is_empty()) {
            // From a join on that path
            Some(why) if why.contains("path into here") => why.clone(),
            Some(why) => format!("on another path into here, {why}"),
            None => "it isn't placed on another path into here".into(),
        },
    }
}

impl Regmap {
    fn place(&mut self, var: &str, reg: u8) {
        // Moving var kills its old register
        if let Some(&Some(old_reg)) = self.var_places.get(var)
            && old_reg != reg
        {
            self.kill(old_reg, String::new());
        }
        self.killed_regs.insert(reg as usize);
        self.killed_why
            .insert(reg, format!("{} now holds {var}", reg_name(reg)));
        if let Some(old_var) = self.reg_contents.insert(reg, var.to_owned())
            && old_var != var
        {
            self.lost.insert(
                old_var.clone(),
                format!("{} now holds {var}", reg_name(reg)),
            );
            self.var_places.insert(old_var, None);
        }
        self.var_places.insert(var.to_owned(), Some(reg));
        self.lost.remove(var);
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

    /// `why`: what happened to the register, for an error using what it held
    fn kill(&mut self, reg: u8, why: String) {
        if let Some(var) = self.reg_contents.remove(&reg) {
            self.var_places.insert(var.clone(), None);
            self.lost.insert(var, why.clone());
        }
        self.killed_regs.insert(reg as usize);
        if !why.is_empty() {
            self.killed_why.insert(reg, why);
        }
    }

    fn kill_ident(&mut self, lhs: &Token) {
        if let Some(id) = lhs.as_ident()
            && let Some(n) = parse_register(id)
        {
            self.kill(n, format!("{id} was assigned"));
        }
    }

    pub fn apply(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Assign(lhs, _op, _rhs) | Stmt::WithFlagsAssign(lhs, _op, _rhs) => {
                self.apply_assign(lhs)
            }
            Stmt::WithAddrUpdate(stmt, _lhs, _sign, _incr) => self.apply(stmt),
            Stmt::Insn(Insn::Pop(regs)) => {
                for r in regs {
                    if let Some(id) = r.as_ident()
                        && let Some(n) = parse_register(id)
                    {
                        self.kill(n, format!("pop restores {id}"));
                    }
                }
                // `x @r1` in a pop places x in the restored register
                for r in regs {
                    if let Expr::Binop(var, op, place) = r
                        && op.tok == TokBody::At
                        && let Expr::Ident(place) = &**place
                    {
                        self.place_expr(var, place);
                    }
                }
            }
            Stmt::Insn(insn @ (Insn::Bl(_) | Insn::Blx(_))) => {
                let call = match insn {
                    Insn::Bl(name) => format!("the call to {name}"),
                    _ => "the call".into(),
                };
                // By ABI convention
                for r in [0, 1, 2, 3, 12, 14] {
                    self.kill(r, format!("{call} clobbers {}", reg_name(r)));
                }
            }
            _ => (),
        }
    }

    fn apply_assign(&mut self, lhs: &Expr) {
        match lhs {
            Expr::Ident(id) => self.kill_ident(id),
            Expr::Binop(a, op, b) => {
                if op.tok == TokBody::At
                    && let Expr::Ident(place) = &**b
                {
                    self.place_expr(a, place);
                }
            }
            Expr::Tuple(exps) => {
                for expr in exps {
                    self.apply_assign(expr);
                }
            }
            Expr::Slice(a, _, _) => self.apply_assign(a),
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
                    let why = paths_differ(var, reg, &pred.lost);
                    if let Some(n) = this_reg
                        && !self.killed_regs.get(n as usize)
                    {
                        self.kill(n, why.clone());
                        changed = true;
                    }
                    if let Some(n) = reg
                        && !self.killed_regs.get(n as usize)
                    {
                        self.kill(n, why);
                        changed = true;
                    }
                }
            } else if let Some(n) = reg {
                if let Some(other) = self.reg_contents.get(&n).cloned() {
                    // conflicting placement, different vars to same register
                    self.var_places.insert(var.clone(), None);
                    let r = reg_name(n);
                    self.lost.insert(
                        var.clone(),
                        format!("{r} holds {other} on a path into here"),
                    );
                    if !self.killed_regs.get(n as usize) {
                        self.kill(n, format!("{r} holds {var} on a path into here"));
                    }
                } else if !self.killed_regs.get(n as usize) {
                    self.var_places.insert(var.clone(), reg);
                    self.reg_contents.insert(n, var.clone());
                } else {
                    self.var_places.insert(var.clone(), None);
                    let why = match self.killed_why.get(&n) {
                        Some(w) => format!("on another path into here, {w}"),
                        None => format!("{} is reused", reg_name(n)),
                    };
                    self.lost.insert(var.clone(), why);
                }
                changed = true;
            } else {
                self.var_places.insert(var.clone(), None);
                if let Some(why) = pred.lost.get(var) {
                    self.lost.insert(var.clone(), why.clone());
                }
                changed = true;
            }
        }
        for reg in pred.killed_regs & !self.killed_regs {
            if !pred.reg_contents.contains_key(&(reg as u8)) {
                self.killed_regs.insert(reg);
                if let Some(w) = pred.killed_why.get(&(reg as u8)) {
                    self.killed_why.insert(reg as u8, w.clone());
                }
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
                        let why = paths_differ(var, *their_reg, &sibling.lost);
                        self.lost.insert(var.clone(), why);
                    }
                }
            }
        }
    }

    /// Why a variable isn't placed, if it was
    pub fn lost(&self, var: &str) -> Option<&str> {
        self.lost
            .get(var)
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
    }

    pub fn init_from_args(&mut self, args: &Args) {
        for i in 0..args.0.len().min(4) {
            if let Some(id) = args.0[i].var.as_ident() {
                self.place(id, i as u8);
            }
        }
    }

    /// The variables placed, and their registers, in register order
    pub fn placed(&self) -> Vec<(String, u8)> {
        let mut v: Vec<(String, u8)> = self
            .reg_contents
            .iter()
            .map(|(&reg, var)| (var.clone(), reg))
            .collect();
        v.sort_by_key(|&(_, reg)| reg);
        v
    }

    pub fn lookup(&self, id: &str) -> Option<u8> {
        if let Some(reg) = self.var_places.get(id) {
            *reg
        } else {
            None
        }
    }
}
