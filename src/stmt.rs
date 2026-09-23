// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::{lex::Token, parse::Expr};

#[derive(Debug)]
pub enum Stmt {
    Label(String),
    /// Includes assignment ops as well as `Equals`
    Assign(Expr, Token, Expr),
    WithFlagsExpr(Expr),
    WithFlagsAssign(Expr, Token, Expr),
    WithAddrUpdate(Box<Stmt>, Expr, i32, Expr),
    Insn(Insn),
    StartIf(Token),
    Else,
    EndBlock,
}

#[derive(Debug)]
pub enum Insn {
    Bx(Expr),
    BCond(String, String),
    B(String),
    Bl(String),
    Cbz(Expr, String),
    Cbnz(Expr, String),
}
