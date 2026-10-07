// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::{collections::HashMap, io::Write};

use crate::{
    error::{Error, WithLoc},
    generate::GenCtx,
    globals::Globals,
    ifthen,
    ir::IrCtx,
    parse::Function,
    regmap::Regmap,
    stmt::{Insn, Stmt},
    typeinf::TypeInferCtx,
    types::TypePool,
};

#[derive(Debug, Default)]
pub struct FnScope {
    // map from label name to stmt number
    labels: HashMap<String, usize>,
    basic_blocks: Vec<BasicBlock>,
    basic_block_starts: HashMap<usize, usize>,
}

#[derive(Debug)]
struct BasicBlock {
    // stmt numbers
    start: usize,
    end: usize,
    pred: Vec<usize>,
    succ: Vec<usize>,
    regmap: Regmap,
}

impl FnScope {
    pub fn analyze(&mut self, func: &Function) -> Result<(), Error> {
        for (ix, stmt) in func.body.iter().enumerate() {
            if let Stmt::Label(l) = stmt {
                if self.labels.insert(l.clone(), ix).is_some() {
                    return Err(format!("duplicate label {l}").into()).at(&func.locs[ix]);
                }
            }
        }
        //println!("{:?}", self.labels);

        // Find all the basic blocks
        // Basic block 0 is function start
        self.start_basic_block(0);
        self.basic_blocks[0].regmap.init_from_args(&func.args);

        self.start_basic_block(0);
        self.add_edge(0, 1);

        let mut last_was_branch = false;
        for (ix, stmt) in func.body.iter().enumerate() {
            let mut is_block_start = last_was_branch;
            if let Stmt::Label(_) = stmt {
                is_block_start = true;
            }
            let (kind, _) = analyze_branch(stmt);
            last_was_branch = matches!(kind, BranchKind::CondBranch | BranchKind::UncondBranch);
            // A label at the very start continues the entry block, which
            // receives the argument placements.
            if is_block_start && self.basic_blocks.last().unwrap().start != ix {
                self.basic_blocks.last_mut().unwrap().end = ix;
                self.start_basic_block(ix);
            }
        }
        self.basic_blocks.last_mut().unwrap().end = func.body.len();

        // Wire up edges in basic block graph
        let mut fallthrough = false;
        let mut this_block = 0;
        for (ix, stmt) in func.body.iter().enumerate() {
            if let Some(&bb) = self.basic_block_starts.get(&ix) {
                if fallthrough {
                    self.add_edge(this_block, bb);
                }
                this_block = bb;
            }
            let (kind, label) = analyze_branch(stmt);
            fallthrough = !matches!(kind, BranchKind::UncondBranch);
            if let Stmt::Insn(Insn::Cbz(_, label) | Insn::Cbnz(_, label)) = stmt {
                self.check_cbz_target(ix, label).at(&func.locs[ix])?;
            }
            if let Some(label) = label {
                if let Some(target_ix) = self.labels.get(label) {
                    let target_block = self.basic_block_starts[target_ix];
                    self.add_edge(this_block, target_block);
                }
                // TODO: probably want to make sure label is a proper global
            }
        }

        // build register maps
        for block in &mut self.basic_blocks[1..] {
            for ix in block.start..block.end {
                let stmt = &func.body[ix];
                block.regmap.apply(stmt);
            }
        }

        self.propagate_regmaps();

        //println!("{:#?}", self.basic_blocks);
        Ok(())
    }

    fn propagate_regmaps(&mut self) {
        // Every block needs at least one visit; a block whose first
        // intersection changes nothing would otherwise never pass its
        // placements on to its successors.
        let mut queue: Vec<usize> = (0..self.basic_blocks.len()).rev().collect();
        while let Some(node) = queue.pop() {
            let n_succ = self.basic_blocks[node].succ.len();
            for i in 0..n_succ {
                let succ = self.basic_blocks[node].succ[i];
                if node != succ {
                    let (lo, hi) = self.basic_blocks.split_at_mut(node.max(succ));
                    let (n, s) = if node < succ {
                        (&lo[node], &mut hi[0])
                    } else {
                        (&hi[0], &mut lo[succ])
                    };
                    if s.regmap.intersect_pred(&n.regmap) {
                        queue.push(succ);
                    }
                }
            }
        }
    }

    fn check_cbz_target(&self, ix: usize, label: &str) -> Result<(), Error> {
        match self.labels.get(label) {
            Some(&target_ix) if target_ix > ix => Ok(()),
            Some(_) => Err(format!(
                "cbz/cbnz can only branch forward, {label} is behind"
            ))?,
            None => Err(format!(
                "cbz/cbnz target {label} must be a label in this function"
            ))?,
        }
    }

    fn start_basic_block(&mut self, start: usize) {
        self.basic_block_starts
            .insert(start, self.basic_blocks.len());
        self.basic_blocks.push(BasicBlock {
            start,
            end: 0,
            pred: vec![],
            succ: vec![],
            regmap: Regmap::default(),
        });
    }

    fn add_edge(&mut self, pred: usize, succ: usize) {
        self.basic_blocks[pred].succ.push(succ);
        self.basic_blocks[succ].pred.push(pred);
    }

    pub fn gen_function(
        &self,
        func: &Function,
        types: &mut TypePool,
        globals: &Globals,
        w: &mut impl Write,
    ) -> Result<(), Error> {
        if let Some(name) = func.name.as_ident() {
            // section should be controllable
            writeln!(w, ".section .text")?;
            writeln!(w, ".global {name}")?;
            writeln!(w, ".thumb_func")?;
            writeln!(w, "{name}:")?;
            // might also consider .function / .endfunc; but this
        }
        let type_inf_ctx = TypeInferCtx::new(types, globals);
        let typemap = type_inf_ctx.infer(func).at(&func.name.loc)?;
        let if_analysis = ifthen::analyze_ift(&func.body);
        //println!("{if_analysis:?}");
        for block in &self.basic_blocks[1..] {
            let mut regmap = if let Some((pred, tail)) = block.pred.split_first() {
                let mut regmap = self.basic_blocks[*pred].regmap.clone();
                for ix in tail {
                    regmap.intersect(&self.basic_blocks[*ix].regmap);
                }
                regmap
            } else {
                Regmap::default()
            };
            for ix in block.start..block.end {
                let stmt = &func.body[ix];
                let if_state = &if_analysis[ix];
                let result = if IrCtx::can_lower(stmt) {
                    let mut ir_ctx = IrCtx::new(&regmap, types, &typemap, globals);
                    ir_ctx.lower(stmt).and_then(|ir| {
                        let mut gen_ctx = GenCtx::new(types, w);
                        gen_ctx.gen_from_ir(&ir, if_state)
                    })
                } else {
                    let mut gen_ctx = GenCtx::new(types, w);
                    gen_ctx.gen_stmt(stmt, &regmap, if_state)
                };
                result.at(&func.locs[ix])?;
                regmap.apply(stmt);
                //_ = writeln!(w, "{ix}: {regmap:?}");
            }
        }
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
            Insn::Bx(_expr) => (BranchKind::UncondBranch, None),
            Insn::BCond(_, label) => (BranchKind::CondBranch, Some(label)),
            Insn::B(label) => (BranchKind::UncondBranch, Some(label)),
            Insn::Bl(label) => (BranchKind::FunctionCall, Some(label)),
            Insn::Cbz(_, label) => (BranchKind::CondBranch, Some(label)),
            Insn::Cbnz(_, label) => (BranchKind::CondBranch, Some(label)),
            Insn::Pop(regs) if regs.iter().any(|r| r.as_ident() == Some("pc")) => {
                (BranchKind::UncondBranch, None)
            }
            Insn::Push(_) | Insn::Pop(_) | Insn::Cps(_, _) => (BranchKind::NotBranch, None),
        }
    } else {
        (BranchKind::NotBranch, None)
    }
}
