//! Analysis of ift blocks.

use crate::stmt::Stmt;

#[derive(Clone, Debug)]
pub enum IfState {
    Default,
    // Obviously could have a more concise repr, but is it worth it?
    Ift(Vec<bool>),
    Then(Cond),
    Else(Cond),
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum Cond {
    Eq,
    Ne,
    Cs,
    Cc,
    Mi,
    Pl,
    Vs,
    Vc,
    Hi,
    Ls,
    Ge,
    Lt,
    Gt,
    Le,
    Al,
    Invalid,
}

// Note: this signature is problematic, we need the placements to count
// elided moves. Maybe the best thing to do is fix that up afterward.
pub fn analyze_ift(body: &[Stmt]) -> Vec<IfState> {
    let mut result = vec![IfState::Default; body.len()];
    for (i, stmt) in body.iter().enumerate() {
        let mut bools = vec![];
        if let Stmt::StartIf(cond_tok) = stmt {
            let cond = cond_tok
                .as_ident()
                .and_then(Cond::parse)
                .unwrap_or(Cond::Invalid);
            let mut j = i + 1;
            let mut is_then = true;
            loop {
                match &body[j] {
                    Stmt::EndBlock => break,
                    // TODO: forbid multiple else (if returning result).
                    Stmt::Else => is_then = false,
                    // This should be caught by validation earlier. Alternatively,
                    // return a result (can also catch invalid condition codes).
                    Stmt::StartIf(_) => panic!("nested ift not allowed"),
                    Stmt::Assign(_, _, _)
                    | Stmt::WithFlagsExpr(_)
                    | Stmt::WithFlagsAssign(_, _, _)
                    | Stmt::WithAddrUpdate(_, _, _, _)
                    | Stmt::Insn(_) => {
                        result[j] = if is_then {
                            IfState::Then(cond)
                        } else {
                            IfState::Else(cond)
                        };
                        bools.push(is_then);
                    }
                    Stmt::Label(_) => (),
                }
                j += 1;
            }
            result[i] = IfState::Ift(bools);
        }
    }
    result
}

impl Cond {
    pub fn to_str(self) -> &'static str {
        match self {
            Cond::Eq => "eq",
            Cond::Ne => "ne",
            Cond::Cs => "cs",
            Cond::Cc => "cc",
            Cond::Mi => "mi",
            Cond::Pl => "pl",
            Cond::Vs => "vs",
            Cond::Vc => "vc",
            Cond::Hi => "hi",
            Cond::Ls => "ls",
            Cond::Ge => "ge",
            Cond::Lt => "lt",
            Cond::Gt => "gt",
            Cond::Le => "le",
            Cond::Al => "al",
            Cond::Invalid => "??",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "eq" => Some(Cond::Eq),
            "ne" => Some(Cond::Ne),
            "cs" => Some(Cond::Cs),
            "cc" => Some(Cond::Cc),
            "mi" => Some(Cond::Mi),
            "pl" => Some(Cond::Pl),
            "vs" => Some(Cond::Vs),
            "vc" => Some(Cond::Vc),
            "hi" => Some(Cond::Hi),
            "ls" => Some(Cond::Ls),
            "ge" => Some(Cond::Ge),
            "lt" => Some(Cond::Lt),
            "gt" => Some(Cond::Gt),
            "le" => Some(Cond::Le),
            "al" => Some(Cond::Al),
            _ => None,
        }
    }
}

impl core::ops::Not for Cond {
    type Output = Self;

    fn not(self) -> Self {
        match self {
            Cond::Eq => Cond::Ne,
            Cond::Ne => Cond::Eq,
            Cond::Cs => Cond::Cc,
            Cond::Cc => Cond::Cs,
            Cond::Mi => Cond::Pl,
            Cond::Pl => Cond::Mi,
            Cond::Vs => Cond::Vc,
            Cond::Vc => Cond::Vs,
            Cond::Hi => Cond::Ls,
            Cond::Ls => Cond::Hi,
            Cond::Ge => Cond::Lt,
            Cond::Lt => Cond::Ge,
            Cond::Gt => Cond::Le,
            Cond::Le => Cond::Gt,
            Cond::Al => Cond::Invalid,
            Cond::Invalid => Cond::Al,
        }
    }
}
