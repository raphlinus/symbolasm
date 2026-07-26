use std::{collections::HashMap, io::Write};

use crate::{
    generate::{GenCtx, gen_stmt},
    ir::IrCtx,
    lex::Error,
    parse::Function,
    regmap::Regmap,
    stmt::{Insn, Stmt},
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
                    return Err(format!("duplicate label {l}"))?;
                }
            }
        }
        println!("{:?}", self.labels);

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
            if is_block_start {
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
            if let Some(label) = label {
                // TODO: deal with label not existing, also note it could be global
                let target = self.basic_block_starts[&self.labels[label]];
                self.add_edge(this_block, target);
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
        let mut queue = vec![0];
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
        w: &mut impl Write,
    ) -> Result<(), Error> {
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
                if IrCtx::can_lower(stmt) {
                    let mut ir_ctx = IrCtx::new(&regmap, types);
                    let ir = ir_ctx.lower(stmt)?;
                    let mut gen_ctx = GenCtx::new(types, w);
                    gen_ctx.gen_from_ir(&ir)?;
                } else {
                    gen_stmt(stmt, &regmap, w)?;
                }
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
        }
    } else {
        (BranchKind::NotBranch, None)
    }
}
