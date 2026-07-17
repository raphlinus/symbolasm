use std::collections::HashMap;

use crate::{lex::Error, parse::{Function, Stmt}};

#[derive(Debug, Default)]
pub struct FnScope {
    // map from label name to stmt number
    labels: HashMap<String, usize>,
    basic_blocks: Vec<BasicBlock>,
}

#[derive(Debug)]
struct BasicBlock {
    // stmt numbers
    start: usize,
    end: usize,
}

impl FnScope {
    pub fn compile(&mut self, func: &Function) -> Result<(), Error> {
        for (ix, stmt) in func.body.iter().enumerate() {
            if let Stmt::Label(l) = stmt {
                if self.labels.insert(l.clone(), ix).is_some() {
                    return Err(format!("duplicate label {l}"))?;
                }
            }
        }
        println!("{:?}", self.labels);
        Ok(())
    }
}

enum BranchKind {
    NotBranch,
    CondBranch,
    UncondBranch,
    FunctionCall,
}

// Return values
fn analyze_branch(stmt: &Stmt) -> (BranchKind, Option<&str>) {
    if let Stmt::Insn(insn) = stmt {
        match insn {
            crate::parse::Insn::Bx(expr) => (BranchKind::UncondBranch, None),
            crate::parse::Insn::BCond(_, label) => (BranchKind::CondBranch, Some(label)),
            crate::parse::Insn::B(label) => (BranchKind::UncondBranch, Some(label)),
            crate::parse::Insn::Bl(label) => (BranchKind::FunctionCall, Some(label)),
        }
    } else {
        (BranchKind::NotBranch, None)
    }
}