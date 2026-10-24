// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::{collections::HashMap, io::Write};

use crate::{
    error::{Error, WithLoc},
    facts::{Def, DefKind, Place},
    flags,
    generate::{GenCtx, LocalLabels},
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
    /// Placements at the end of each basic block, parallel to basic_blocks
    regmaps: Vec<Regmap>,
    basic_block_starts: HashMap<usize, usize>,
}

/// The placements just before and just after a statement, for `facts`
pub struct StmtPlaces {
    /// The statement's offset in the source
    pub offset: usize,
    pub before: Vec<Place>,
    pub after: Vec<Place>,
}

#[derive(Debug)]
pub struct BasicBlock {
    // stmt numbers
    pub start: usize,
    pub end: usize,
    pub pred: Vec<usize>,
    pub succ: Vec<usize>,
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
        self.regmaps[0].init_from_args(&func.args);

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
        for (block, regmap) in self.basic_blocks[1..].iter().zip(&mut self.regmaps[1..]) {
            for ix in block.start..block.end {
                let stmt = &func.body[ix];
                regmap.apply(stmt);
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
                    let (lo, hi) = self.regmaps.split_at_mut(node.max(succ));
                    let (n, s) = if node < succ {
                        (&lo[node], &mut hi[0])
                    } else {
                        (&hi[0], &mut lo[succ])
                    };
                    if s.intersect_pred(n) {
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

    /// The labels written in the source as `name:` (not the ones block
    /// structure brings in), for go to definition
    pub fn label_defs(&self, func: &Function, src: &str, defs: &mut Vec<Def>) {
        let fn_name = func.name.as_ident().unwrap_or_default();
        for (name, &ix) in &self.labels {
            let offset = func.locs[ix].offset;
            let written = src[offset..]
                .strip_prefix(name.as_str())
                .is_some_and(|rest| rest.starts_with(':'));
            if written {
                defs.push(Def {
                    name: name.clone(),
                    kind: DefKind::Label,
                    offset,
                    func: Some(fn_name.to_owned()),
                });
            }
        }
    }

    fn start_basic_block(&mut self, start: usize) {
        self.basic_block_starts
            .insert(start, self.basic_blocks.len());
        self.regmaps.push(Regmap::default());
        self.basic_blocks.push(BasicBlock {
            start,
            end: 0,
            pred: vec![],
            succ: vec![],
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
        mut facts: Option<&mut Vec<StmtPlaces>>,
        w: &mut impl Write,
    ) -> Result<(), Error> {
        let markers = facts.is_some();
        if let Some(name) = func.name.as_ident() {
            match &func.link_section {
                Some(section) => {
                    writeln!(w, "{}", section_directive(section, name, true))?;
                    // A new section starts out byte aligned, and Thumb code
                    // needs at least 2; 4 keeps 32 bit fetches aligned.
                    writeln!(w, ".balign {}", func.align.unwrap_or(1).max(4))?;
                }
                None => {
                    writeln!(w, ".section .text")?;
                    if let Some(align) = func.align {
                        writeln!(w, ".balign {align}")?;
                    }
                }
            }
            writeln!(w, ".global {name}")?;
            // Without a function type the symbol is NOTYPE, and linkers
            // other than GNU ld (rust-lld, for one) then take its address
            // without the Thumb bit, so calling through it faults.
            writeln!(w, ".type {name}, %function")?;
            writeln!(w, ".thumb_func")?;
            writeln!(w, "{name}:")?;
        }
        let labels = LocalLabels {
            fn_name: func.name.as_ident().unwrap_or_default(),
            labels: &self.labels,
        };
        let type_inf_ctx = TypeInferCtx::new(types, globals);
        let typemap = type_inf_ctx.infer(func).at(&func.name.loc)?;
        let if_analysis = ifthen::analyze_ift(&func.body);
        let live_flags = flags::live_after(&func.body, &if_analysis, &self.basic_blocks);
        let places = |regmap: &Regmap, types: &TypePool| -> Vec<Place> {
            regmap
                .placed()
                .into_iter()
                .map(|(var, reg)| {
                    let ty = typemap.lookup(&var).map(|t| types.display(t));
                    Place { var, reg, ty }
                })
                .collect()
        };
        //println!("{if_analysis:?}");
        for block in &self.basic_blocks[1..] {
            let mut regmap = if let Some((pred, tail)) = block.pred.split_first() {
                let mut regmap = self.regmaps[*pred].clone();
                for ix in tail {
                    regmap.intersect(&self.regmaps[*ix]);
                }
                regmap
            } else {
                Regmap::default()
            };
            for ix in block.start..block.end {
                let stmt = &func.body[ix];
                let if_state = &if_analysis[ix];
                // For --analyze, which maps asm lines back to statements.
                // `a` marks a statement that lowers to IR, so one that
                // emits nothing was elided.
                if markers {
                    let kind = if IrCtx::can_lower(stmt) { "a" } else { "o" };
                    writeln!(w, "@sy {} {kind}", func.locs[ix].offset)?;
                }
                let result = if IrCtx::can_lower(stmt) {
                    let mut ir_ctx = IrCtx::new(&regmap, types, &typemap, globals);
                    ir_ctx.lower(stmt).and_then(|ir| {
                        let mut gen_ctx = GenCtx::new(types, w);
                        gen_ctx.set_live_flags(live_flags[ix]);
                        gen_ctx.gen_from_ir(&ir, if_state)
                    })
                } else {
                    let mut gen_ctx = GenCtx::new(types, w);
                    gen_ctx.gen_stmt(stmt, &regmap, &labels, if_state)
                };
                result.at(&func.locs[ix])?;
                let before = facts.as_ref().map(|_| places(&regmap, types));
                regmap.apply(stmt);
                if let (Some(sink), Some(before)) = (facts.as_deref_mut(), before) {
                    sink.push(StmtPlaces {
                        offset: func.locs[ix].offset,
                        before,
                        after: places(&regmap, types),
                    });
                }
                //_ = writeln!(w, "{ix}: {regmap:?}");
            }
        }
        if let Some(name) = func.name.as_ident() {
            writeln!(w, ".size {name}, . - {name}")?;
        }
        Ok(())
    }
}

/// The `.section` directive for an item with `#[link_section = "name"]`.
///
/// Each item gets its own section, `name.item`, as with
/// `-ffunction-sections`, so linker scripts match it with `*(name.*)` and
/// unused items can be dropped with `--gc-sections`. The assemblers infer
/// flags from the standard prefixes, and GNU as warns when they're given
/// explicitly (`.data.*` code is `"aw"`, not `"ax"`), so they're only
/// spelled out for other names.
pub fn section_directive(section: &str, item: &str, is_code: bool) -> String {
    let standard = [".text", ".data", ".rodata", ".bss"]
        .iter()
        .any(|p| section == *p || section.starts_with(&format!("{p}.")));
    if standard {
        format!(".section {section}.{item}")
    } else {
        let flags = if is_code { "ax" } else { "aw" };
        format!(".section {section}.{item},\"{flags}\",%progbits")
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
            Insn::Blx(_) => (BranchKind::FunctionCall, None),
            Insn::Cbz(_, label) => (BranchKind::CondBranch, Some(label)),
            Insn::Cbnz(_, label) => (BranchKind::CondBranch, Some(label)),
            Insn::Pop(regs) if regs.iter().any(|r| r.as_ident() == Some("pc")) => {
                (BranchKind::UncondBranch, None)
            }
            Insn::Push(_) | Insn::Pop(_) | Insn::Cps(_, _) | Insn::NoOperand(_) => {
                (BranchKind::NotBranch, None)
            }
        }
    } else {
        (BranchKind::NotBranch, None)
    }
}
