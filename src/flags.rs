// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Liveness of the condition flags.
//!
//! A backward dataflow analysis over the basic block graph, tracking each
//! of N, Z, C and V separately, since some instructions write only some of
//! them. Code generation uses it to pick the flag-setting forms of
//! instructions, which have 16 bit encodings, where the flags they write
//! aren't read afterwards.

use crate::{
    compile::BasicBlock,
    ifthen::{Cond, IfState},
    lex::TokBody,
    parse::Expr,
    stmt::{Insn, Stmt},
};

pub const N: u8 = 8;
pub const Z: u8 = 4;
pub const C: u8 = 2;
pub const V: u8 = 1;
pub const NZ: u8 = N | Z;
pub const NZC: u8 = N | Z | C;
pub const NZCV: u8 = N | Z | C | V;

/// The flags a condition reads.
pub fn cond_reads(cond: Cond) -> u8 {
    match cond {
        Cond::Eq | Cond::Ne => Z,
        Cond::Cs | Cond::Cc => C,
        Cond::Mi | Cond::Pl => N,
        Cond::Vs | Cond::Vc => V,
        Cond::Hi | Cond::Ls => C | Z,
        Cond::Ge | Cond::Lt => N | V,
        Cond::Gt | Cond::Le => N | Z | V,
        Cond::Al => 0,
        Cond::Invalid => NZCV,
    }
}

fn cond_str_reads(cond: &str) -> u8 {
    Cond::parse(cond).map_or(NZCV, cond_reads)
}

/// The flags a statement reads, and the flags it certainly writes. Writes
/// may be underestimated, which only keeps flags live longer.
fn effect(stmt: &Stmt, if_state: &IfState) -> (u8, u8) {
    // A conditional instruction reads its condition's flags, and might not
    // write anything.
    if let IfState::Then(cond) | IfState::Else(cond) = if_state {
        return (cond_reads(*cond), 0);
    }
    match stmt {
        Stmt::Insn(Insn::BCond(cond, _)) => (cond_str_reads(cond), 0),
        Stmt::StartIf(cond) => (cond.as_ident().map_or(NZCV, cond_str_reads), 0),
        // The callee may change any flag.
        Stmt::Insn(Insn::Bl(_) | Insn::Blx(_)) | Stmt::Call(_) => (0, NZCV),
        Stmt::WithFlagsExpr(expr) => (0, expr_writes(expr)),
        Stmt::WithFlagsAssign(_, op, rhs) => {
            let writes = match op.tok {
                TokBody::PlusEquals | TokBody::MinusEquals => NZCV,
                TokBody::Equals => expr_writes(rhs),
                _ => NZ,
            };
            (0, writes)
        }
        Stmt::WithAddrUpdate(stmt, ..) => effect(stmt, if_state),
        _ => (0, 0),
    }
}

/// The flags an arithmetic flag-setting instruction writes: all four, or
/// for the others N and Z (C may depend on a shift).
fn expr_writes(expr: &Expr) -> u8 {
    match expr {
        Expr::Binop(_, op, _) if matches!(op.tok, TokBody::Plus | TokBody::Minus) => NZCV,
        Expr::Binop(a, op, _) if op.tok == TokBody::At => expr_writes(a),
        Expr::Unary(op, _) if op.tok == TokBody::Minus => NZCV,
        _ => NZ,
    }
}

/// For each statement, the flags that are live after it. Flags are dead
/// where control leaves the function.
pub fn live_after(body: &[Stmt], if_states: &[IfState], blocks: &[BasicBlock]) -> Vec<u8> {
    let transfer = |ix: usize, live: u8| {
        let (reads, writes) = effect(&body[ix], &if_states[ix]);
        (live & !writes) | reads
    };
    let mut live_in = vec![0u8; blocks.len()];
    loop {
        let mut changed = false;
        for b in (0..blocks.len()).rev() {
            let block = &blocks[b];
            let mut live = block.succ.iter().fold(0, |acc, s| acc | live_in[*s]);
            for ix in (block.start..block.end).rev() {
                live = transfer(ix, live);
            }
            if live != live_in[b] {
                live_in[b] = live;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut result = vec![NZCV; body.len()];
    for block in blocks {
        let mut live = block.succ.iter().fold(0, |acc, s| acc | live_in[*s]);
        for ix in (block.start..block.end).rev() {
            result[ix] = live;
            live = transfer(ix, live);
        }
    }
    result
}
