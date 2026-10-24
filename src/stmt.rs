// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::{
    lex::Token,
    parse::{Expr, Type},
};

#[derive(Debug)]
pub enum Stmt {
    Label(String),
    /// Includes assignment ops as well as `Equals`
    Assign(Expr, Token, Expr),
    WithFlagsExpr(Expr),
    WithFlagsAssign(Expr, Token, Expr),
    WithAddrUpdate(Box<Stmt>, Expr, i32, Expr),
    Insn(Insn),
    /// `(results) = call target(args)`
    Call(Call),
    /// `sp -= n (x, p: Point)` or `sp += n`: the change to sp, a constant,
    /// and the variables placed in the space made, lowest first
    SpAdjust(Expr, Vec<(Token, Option<Type>)>),
    StartIf(Token),
    Else,
    /// `} loop {`, between the parts of a `while { } loop { }` block
    LoopPart,
    EndBlock,
}

#[derive(Debug)]
pub struct Call {
    /// Each `x` or `x @r0`; empty to ignore the results
    pub rets: Vec<Expr>,
    /// A function's name, or a variable holding a function's address
    pub target: Token,
    /// Each a variable in a register, or `sp.x` for one on the stack
    pub args: Vec<Expr>,
    /// `#[tail] call`, which jumps instead of returning here
    pub tail: bool,
}

#[derive(Debug)]
pub enum Insn {
    Bx(Expr),
    BCond(String, String),
    B(String),
    Bl(String),
    /// A call through a register
    Blx(Expr),
    Cbz(Expr, String),
    Cbnz(Expr, String),
    Push(Vec<Expr>),
    Pop(Vec<Expr>),
    /// cpsie/cpsid, with the interrupt mask flags (`i` and/or `f`)
    Cps(bool, String),
    /// Instructions with no operands: hints (`wfe`, `wfi`, `sev`, `nop`)
    /// and barriers (`dmb`, `dsb`, `isb`)
    NoOperand(String),
}
