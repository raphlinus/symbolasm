// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Implementation of register map for placing variables
//!
//! A regmap is what's known at one point in a function: which variable each
//! register holds, where sp is relative to its value at entry, and which
//! variable each stack slot holds. The registers a function must restore
//! hold pseudo-variables at entry (`%r4`, `%lr`), so checking that they're
//! restored is checking that those are still placed.

use std::collections::{BTreeMap, HashMap};

use crate::{
    abi::Abi,
    globals::Globals,
    lex::{TokBody, Token},
    parse::{Args, Expr},
    stmt::{Insn, Stmt},
    typeinf::TypeMap,
    types::{FnSig, TypePool},
};

/// What the analysis needs to know beyond the statements
pub struct FnCtx<'a> {
    /// The function being analyzed
    pub name: &'a str,
    pub abi: Abi,
    /// The function's results, in register order
    pub rets: Vec<String>,
    pub types: &'a TypePool,
    pub globals: &'a Globals,
    pub typemap: &'a TypeMap,
}

impl<'a> FnCtx<'a> {
    pub fn new(
        func: &'a crate::parse::Function,
        types: &'a TypePool,
        globals: &'a Globals,
        typemap: &'a TypeMap,
    ) -> Self {
        let rets = func
            .rets
            .0
            .iter()
            .filter_map(|a| a.var.as_ident().map(|s| s.to_owned()))
            .collect();
        FnCtx {
            name: func.name.as_ident().unwrap_or_default(),
            abi: func.abi,
            rets,
            types,
            globals,
            typemap,
        }
    }

    /// The signature of the function being analyzed
    pub fn own_sig(&self) -> Option<&FnSig> {
        self.types.sig(*self.globals.fns.get(self.name)?)
    }

    /// The signature of a call's target: a variable holding a function's
    /// address, or a function by name
    pub fn callee(&self, target: &str) -> Option<&FnSig> {
        let ty = self
            .typemap
            .lookup(target)
            .filter(|t| self.types.sig(*t).is_some())
            .or_else(|| self.globals.fn_ty(target, self.types))?;
        self.types.sig(ty)
    }

    /// Size and alignment of a variable in a stack slot
    fn layout(&self, var: &str) -> (i32, i32) {
        match self.typemap.lookup(var) {
            Some(ty) => {
                let info = self.types.info(ty);
                (info.size() as i32, info.align().max(1) as i32)
            }
            None => (4, 4),
        }
    }
}

/// A variable in a stack slot
#[derive(Clone, PartialEq, Debug)]
pub struct Slot {
    pub var: String,
    pub size: i32,
}

#[derive(Default, Clone, Debug)]
pub struct Regmap {
    /// What each register holds
    reg_contents: HashMap<u8, String>,
    /// The inverse of `reg_contents`
    var_places: HashMap<String, u8>,
    /// Why each variable that was placed no longer is, for errors
    lost: HashMap<String, String>,
    /// What last happened to each register, for errors
    killed_why: HashMap<u8, String>,
    /// sp, as an offset from its value at entry. None where it isn't known:
    /// in unreachable code, or after a change the analysis can't follow.
    sp: Option<i32>,
    /// The variables in stack slots, by address relative to sp at entry
    slots: BTreeMap<i32, Slot>,
    /// The inverse of `slots`
    stack_places: HashMap<String, i32>,
    /// Why each variable that was on the stack no longer is
    stack_lost: HashMap<String, String>,
}

pub fn reg_name(reg: u8) -> String {
    match reg {
        13 => "sp".into(),
        14 => "lr".into(),
        15 => "pc".into(),
        _ => format!("r{reg}"),
    }
}

/// The pseudo-variable for a register's value at entry
pub fn entry_var(reg: u8) -> String {
    format!("%{}", reg_name(reg))
}

/// How to name a variable or pseudo-variable in a message
pub fn describe(var: &str) -> String {
    match var.strip_prefix('%') {
        Some(reg) => format!("the {reg} it had at entry"),
        None => var.to_owned(),
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
        None => on_other_path(lost.get(var).map(|s| s.as_str())),
    }
}

/// Why a variable isn't placed here, given why it isn't on one path in
fn on_other_path(why: Option<&str>) -> String {
    match why.filter(|s| !s.is_empty()) {
        // From a join on that path
        Some(why) if why.contains("path into here") => why.to_owned(),
        Some(why) => format!("on another path into here, {why}"),
        None => "it isn't placed on another path into here".into(),
    }
}

/// The register a `pop` or `push` operand names, if it's a bare register
fn operand_reg(expr: &Expr) -> Option<u8> {
    expr.as_ident().and_then(parse_register)
}

/// `x @r1` in a pop
fn pop_binding(expr: &Expr) -> Option<(&str, u8)> {
    if let Expr::Binop(var, op, place) = expr
        && op.tok == TokBody::At
        && let Expr::Ident(place) = &**place
    {
        Some((var.as_ident()?, parse_register(place.as_ident()?)?))
    } else {
        None
    }
}

/// `sp` itself
pub fn is_sp(expr: &Expr) -> bool {
    expr.as_ident() == Some("sp")
}

/// The variable a call result names, `x` or `x @r0`
pub fn ret_var(ret: &Expr) -> Option<&str> {
    match ret {
        Expr::Binop(var, op, _) if op.tok == TokBody::At => var.as_ident(),
        _ => ret.as_ident(),
    }
}

impl Regmap {
    /// The state at a function's entry: arguments in registers and on the
    /// stack, and the registers to restore holding their entry values
    pub fn entry(args: &Args, abi: Abi) -> Regmap {
        let mut map = Regmap {
            sp: Some(0),
            ..Default::default()
        };
        for &r in abi.callee_saved() {
            map.place(&entry_var(r), r);
        }
        if abi != Abi::Wild {
            map.place(&entry_var(14), 14);
        }
        let n_regs = abi.n_regs();
        for (i, arg) in args.0.iter().enumerate() {
            if let Some(id) = arg.var.as_ident() {
                if i < n_regs {
                    map.place(id, i as u8);
                } else {
                    let pos = 4 * (i - n_regs) as i32;
                    map.place_stack(id, pos, 4);
                }
            }
        }
        map
    }

    fn place(&mut self, var: &str, reg: u8) {
        // Moving var kills its old register
        if let Some(&old_reg) = self.var_places.get(var)
            && old_reg != reg
        {
            self.reg_contents.remove(&old_reg);
        }
        self.killed_why.insert(
            reg,
            format!("{} now holds {}", reg_name(reg), describe(var)),
        );
        if let Some(old_var) = self.reg_contents.insert(reg, var.to_owned())
            && old_var != var
        {
            self.lost.insert(
                old_var.clone(),
                format!("{} now holds {}", reg_name(reg), describe(var)),
            );
            self.var_places.remove(&old_var);
        }
        self.var_places.insert(var.to_owned(), reg);
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
            self.var_places.remove(&var);
            self.lost.insert(var, why.clone());
        }
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

    /// Put var in the slot at `pos`, replacing whatever overlaps it
    fn place_stack(&mut self, var: &str, pos: i32, size: i32) {
        let overlapping: Vec<i32> = self
            .slots
            .range(..pos + size)
            .filter(|(p, s)| **p + s.size > pos)
            .map(|(p, _)| *p)
            .collect();
        for p in overlapping {
            let old = self.slots.remove(&p).unwrap();
            self.stack_places.remove(&old.var);
            self.stack_lost
                .insert(old.var, format!("its slot now holds {}", describe(var)));
        }
        if let Some(old_pos) = self.stack_places.remove(var) {
            self.slots.remove(&old_pos);
        }
        self.slots.insert(
            pos,
            Slot {
                var: var.to_owned(),
                size,
            },
        );
        self.stack_places.insert(var.to_owned(), pos);
        self.stack_lost.remove(var);
    }

    /// Move sp up, dropping the slots it leaves below it. A placement split
    /// by the new sp is dropped whole.
    fn set_sp(&mut self, sp: i32, why: &str) {
        self.sp = Some(sp);
        let below: Vec<i32> = self.slots.range(..sp).map(|(p, _)| *p).collect();
        for p in below {
            let slot = self.slots.remove(&p).unwrap();
            self.stack_places.remove(&slot.var);
            self.stack_lost.insert(slot.var, why.to_owned());
        }
    }

    /// Lose track of sp, and with it the stack
    pub fn lose_sp(&mut self, why: &str) {
        self.sp = None;
        for (_, slot) in std::mem::take(&mut self.slots) {
            self.stack_places.remove(&slot.var);
            self.stack_lost.insert(slot.var, why.to_owned());
        }
    }

    pub fn apply(&mut self, stmt: &Stmt, ctx: &FnCtx) {
        match stmt {
            Stmt::Assign(lhs, _op, _rhs) | Stmt::WithFlagsAssign(lhs, _op, _rhs) => {
                if is_sp(lhs) {
                    self.lose_sp("sp was assigned");
                }
                self.apply_assign(lhs)
            }
            Stmt::WithAddrUpdate(stmt, lhs, _sign, _incr) => {
                if is_sp(lhs) {
                    self.lose_sp("sp was updated");
                }
                self.apply(stmt, ctx)
            }
            Stmt::Insn(Insn::Push(regs)) => self.apply_push(regs),
            Stmt::Insn(Insn::Pop(regs)) => self.apply_pop(regs),
            Stmt::Insn(Insn::Bl(name)) => {
                let abi = ctx.callee(name).map_or(Abi::Aapcs, |sig| sig.abi);
                self.clobber(abi, &format!("the call to {name}"));
            }
            Stmt::Insn(Insn::Blx(_)) => self.clobber(Abi::Aapcs, "the call"),
            Stmt::Call(call) => {
                let name = call.target.as_ident().unwrap_or_default();
                let Some(sig) = ctx.callee(name) else {
                    return;
                };
                let what = format!("the call to {name}");
                self.clobber(sig.abi, &what);
                // The callee owns its stack arguments
                let n_stack = call.args.len().saturating_sub(sig.abi.n_regs());
                if let Some(sp) = self.sp
                    && n_stack > 0
                {
                    let top = sp + 4 * n_stack as i32;
                    let owned: Vec<i32> = self.slots.range(sp..top).map(|(p, _)| *p).collect();
                    for p in owned {
                        let slot = self.slots.remove(&p).unwrap();
                        self.stack_places.remove(&slot.var);
                        self.stack_lost
                            .insert(slot.var, format!("{what} may change its arguments"));
                    }
                }
                for (i, ret) in call.rets.iter().enumerate() {
                    if let Some(var) = ret_var(ret)
                        && parse_register(var).is_none()
                    {
                        self.place(var, i as u8);
                    }
                }
            }
            Stmt::SpAdjust(delta, places) => {
                let (Some(sp), Expr::Literal(lit)) = (self.sp, delta) else {
                    return;
                };
                let TokBody::Number(n) = lit.tok else {
                    return;
                };
                let new_sp = sp + n as i32;
                if n > 0 {
                    self.set_sp(new_sp, "sp moved above it");
                } else {
                    self.sp = Some(new_sp);
                    let mut offset = 0;
                    for (var, _) in places {
                        let Some(var) = var.as_ident() else { continue };
                        let (size, align) = ctx.layout(var);
                        offset = (offset + align - 1) / align * align;
                        self.place_stack(var, new_sp + offset, size);
                        offset += size;
                    }
                }
            }
            _ => (),
        }
    }

    fn clobber(&mut self, abi: Abi, what: &str) {
        for r in abi.clobbers() {
            let r = r as u8;
            self.kill(r, format!("{what} clobbers {}", reg_name(r)));
        }
    }

    /// The registers a push or pop names, in order, each with the variable
    /// it names: a variable's register, or for a pop, `x @r1`
    fn reglist(&self, regs: &[Expr]) -> Vec<(u8, Option<String>)> {
        regs.iter()
            .filter_map(|r| {
                if let Some(n) = operand_reg(r) {
                    Some((n, None))
                } else if let Some((var, n)) = pop_binding(r) {
                    Some((n, Some(var.to_owned())))
                } else {
                    let id = r.as_ident()?;
                    Some((self.lookup(id)?, Some(id.to_owned())))
                }
            })
            .collect()
    }

    fn apply_push(&mut self, regs: &[Expr]) {
        let list = self.reglist(regs);
        let Some(sp) = self.sp else {
            return;
        };
        let new_sp = sp - 4 * list.len() as i32;
        self.sp = Some(new_sp);
        for (i, (reg, var)) in list.iter().enumerate() {
            let pos = new_sp + 4 * i as i32;
            let var = var.clone().or_else(|| self.reg_contents.get(reg).cloned());
            match var {
                Some(var) => self.place_stack(&var, pos, 4),
                None => {
                    // The slot holds something unnamed
                    if let Some(old) = self.slots.remove(&pos) {
                        self.stack_places.remove(&old.var);
                    }
                }
            }
        }
    }

    fn apply_pop(&mut self, regs: &[Expr]) {
        let list = self.reglist(regs);
        let sp = self.sp;
        for (i, (reg, var)) in list.iter().enumerate() {
            let slot = sp.and_then(|sp| self.slots.get(&(sp + 4 * i as i32)));
            let slot_var = slot.filter(|s| s.size == 4).map(|s| s.var.clone());
            let name = reg_name(*reg);
            match var.clone().or(slot_var) {
                // pc is the return; it doesn't hold a variable
                Some(var) if *reg != 15 => self.place(&var, *reg),
                _ => self.kill(*reg, format!("pop restores {name}")),
            }
        }
        if let Some(sp) = sp {
            self.set_sp(sp + 4 * list.len() as i32, "it was popped");
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

    /// Combine the state on another path into a block: what's placed is
    /// what's placed the same way on both. Errors with both sp offsets when
    /// they differ.
    pub fn meet(&mut self, other: &Regmap) -> Result<(), (i32, i32)> {
        let regs: Vec<u8> = self.reg_contents.keys().copied().collect();
        for r in regs {
            let var = &self.reg_contents[&r];
            if other.reg_contents.get(&r) == Some(var) {
                continue;
            }
            let var = self.reg_contents.remove(&r).unwrap();
            self.var_places.remove(&var);
            let why = paths_differ(&var, other.var_places.get(&var).copied(), &other.lost);
            self.lost.insert(var, why);
            let why = on_other_path(other.killed_why.get(&r).map(|s| s.as_str()));
            self.killed_why.insert(r, why);
        }
        for var in other.var_places.keys() {
            if !self.var_places.contains_key(var) {
                let why = on_other_path(self.lost.get(var).map(|s| s.as_str()));
                self.lost.insert(var.clone(), why);
            }
        }
        for (var, why) in &other.lost {
            self.lost.entry(var.clone()).or_insert_with(|| why.clone());
        }

        let positions: Vec<i32> = self.slots.keys().copied().collect();
        for p in positions {
            if other.slots.get(&p) != self.slots.get(&p) {
                let slot = self.slots.remove(&p).unwrap();
                self.stack_places.remove(&slot.var);
                let why = on_other_path(other.stack_lost.get(&slot.var).map(|s| s.as_str()));
                self.stack_lost.insert(slot.var, why);
            }
        }
        for (var, why) in &other.stack_lost {
            self.stack_lost
                .entry(var.clone())
                .or_insert_with(|| why.clone());
        }
        match (self.sp, other.sp) {
            (Some(a), Some(b)) if a != b => return Err((a, b)),
            (Some(_), None) => self.lose_sp("sp isn't known on another path into here"),
            _ => (),
        }
        Ok(())
    }

    /// Whether two states place the same things, ignoring the reasons
    pub fn same(&self, other: &Regmap) -> bool {
        self.reg_contents == other.reg_contents && self.sp == other.sp && self.slots == other.slots
    }

    /// Why a variable isn't placed, if it was
    pub fn lost(&self, var: &str) -> Option<&str> {
        self.lost
            .get(var)
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
    }

    /// Why a register no longer holds what it did
    pub fn killed_why(&self, reg: u8) -> Option<&str> {
        self.killed_why.get(&reg).map(|s| s.as_str())
    }

    /// The variables placed, and their registers, in register order,
    /// without the pseudo-variables
    pub fn placed(&self) -> Vec<(String, u8)> {
        let mut v: Vec<(String, u8)> = self
            .reg_contents
            .iter()
            .filter(|(_, var)| !var.starts_with('%'))
            .map(|(&reg, var)| (var.clone(), reg))
            .collect();
        v.sort_by_key(|&(_, reg)| reg);
        v
    }

    pub fn lookup(&self, id: &str) -> Option<u8> {
        self.var_places.get(id).copied()
    }

    /// What a register holds
    pub fn holds(&self, reg: u8) -> Option<&str> {
        self.reg_contents.get(&reg).map(|s| s.as_str())
    }

    /// sp relative to its value at entry, where known
    pub fn sp(&self) -> Option<i32> {
        self.sp
    }

    /// Where a variable is on the stack, relative to sp at entry
    pub fn stack_lookup(&self, var: &str) -> Option<i32> {
        self.stack_places.get(var).copied()
    }

    /// Why a variable is no longer on the stack
    pub fn stack_lost(&self, var: &str) -> Option<&str> {
        self.stack_lost.get(var).map(|s| s.as_str())
    }

    /// The slot at a position relative to sp at entry
    pub fn slot(&self, pos: i32) -> Option<&Slot> {
        self.slots.get(&pos)
    }

    /// The variables on the stack, with their offsets from sp, lowest first
    pub fn stack_placed(&self) -> Vec<(String, i32)> {
        let sp = self.sp.unwrap_or(0);
        self.slots
            .iter()
            .filter(|(_, s)| !s.var.starts_with('%'))
            .map(|(p, s)| (s.var.clone(), p - sp))
            .collect()
    }
}
