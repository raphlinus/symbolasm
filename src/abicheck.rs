// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Checks that calls and returns follow the ABIs
//!
//! Each check sees the placements just before its statement.

use crate::{
    abi::Abi,
    error::Error,
    parse::Expr,
    regmap::{FnCtx, Regmap, describe, entry_var, is_sp, parse_register, reg_name},
    stmt::{Call, Insn, Stmt},
    types::FnSig,
};

/// Where a call goes
pub enum CallTarget {
    /// A function by name, `bl name`
    Name(String),
    /// A function's address in a register, `blx r`
    Reg(u8),
}

/// The function a call names and its signature: a variable holding a
/// function's address, or a function by name
fn resolve<'a>(
    call: &Call,
    regmap: &Regmap,
    ctx: &'a FnCtx,
) -> Result<(CallTarget, &'a FnSig), Error> {
    let name = call.target.as_ident().unwrap_or_default();
    if let Some(ty) = ctx.typemap.lookup(name) {
        let Some(sig) = ctx.types.sig(ty) else {
            Err(format!(
                "{name} is a {}, not a function",
                ctx.types.display(ty)
            ))?
        };
        let Some(reg) = regmap.lookup(name) else {
            let why = regmap.lost(name).unwrap_or("it isn't placed");
            Err(format!("{name} is no longer in a register: {why}"))?
        };
        return Ok((CallTarget::Reg(reg), sig));
    }
    match ctx.globals.fn_ty(name, ctx.types) {
        Some(ty) => Ok((
            CallTarget::Name(name.to_owned()),
            ctx.types.sig(ty).unwrap(),
        )),
        None => Err(format!(
            "{name} isn't a function here; declare it with symbol {name}: fn(...)"
        ))?,
    }
}

/// Check a call, returning where it goes
pub fn check_call(call: &Call, regmap: &Regmap, ctx: &FnCtx) -> Result<CallTarget, Error> {
    let (target, sig) = resolve(call, regmap, ctx)?;
    let name = call.target.as_ident().unwrap_or_default();
    let callee = format!("{} function {name}", sig.abi.name());
    match (ctx.abi, sig.abi) {
        (Abi::Wild, _) => (),
        (_, Abi::Wild) => Err(format!(
            "{name} is a wild function, so only a wild function can call it"
        ))?,
        (Abi::Loose, Abi::Aapcs) => Err(format!(
            "an aapcs function like {name} can only be called from an aapcs function, \
             since a loose one doesn't keep sp 8 byte aligned"
        ))?,
        (Abi::Aapcs, Abi::Aapcs) => {
            if let Some(sp) = regmap.sp()
                && sp % 8 != 0
            {
                Err(format!(
                    "calling {callee} needs sp 8 byte aligned, but it's {} below where it was at entry",
                    -sp
                ))?;
            }
        }
        _ => (),
    }
    if call.args.len() != sig.args.len() {
        Err(format!(
            "{name} takes {} arguments, not {}",
            sig.args.len(),
            call.args.len()
        ))?;
    }
    let n_regs = sig.abi.n_regs();
    for (i, arg) in call.args.iter().enumerate() {
        if i < n_regs {
            check_in_reg(
                arg,
                i as u8,
                &format!("argument {} of {name}", i + 1),
                regmap,
            )?;
        } else {
            let want = 4 * (i - n_regs) as i32;
            check_on_stack(arg, want, &format!("argument {} of {name}", i + 1), regmap)?;
        }
    }
    if !call.rets.is_empty() && call.rets.len() != sig.rets.len() {
        Err(format!(
            "{name} returns {} results, not {}",
            sig.rets.len(),
            call.rets.len()
        ))?;
    }
    for (i, ret) in call.rets.iter().enumerate() {
        let what = format!("result {} of {name}", i + 1);
        match ret {
            Expr::Binop(var, _, place) => {
                let var = var.as_ident().unwrap_or_default();
                if place.as_ident().and_then(parse_register) != Some(i as u8) {
                    Err(format!("{what} is in r{i}, so {var} must be placed there"))?;
                }
            }
            _ => {
                let var = ret.as_ident().unwrap_or_default();
                let reg = parse_register(var).or_else(|| regmap.lookup(var));
                if reg != Some(i as u8) {
                    Err(format!("{what} is in r{i}; write {var} @r{i} = call ..."))?;
                }
            }
        }
    }
    if call.tail {
        check_tail(call, sig, regmap, ctx)?;
    }
    Ok(target)
}

/// An argument in a register: a variable placed there, or the register
fn check_in_reg(arg: &Expr, reg: u8, what: &str, regmap: &Regmap) -> Result<(), Error> {
    let Some(var) = arg.as_ident() else {
        if let Expr::Field(base, _) = arg
            && is_sp(base)
        {
            Err(format!("{what} goes in r{reg}, not on the stack"))?;
        }
        Err(format!("{what} must be a variable in r{reg}"))?
    };
    if let Some(r) = parse_register(var) {
        if r != reg {
            Err(format!("{what} goes in r{reg}, not {var}"))?;
        }
        return Ok(());
    }
    match regmap.lookup(var) {
        Some(r) if r == reg => Ok(()),
        Some(r) => Err(format!(
            "{what} goes in r{reg}, but {var} is in {}",
            reg_name(r)
        ))?,
        None => match regmap.lost(var) {
            Some(why) => Err(format!("{var} is no longer in a register: {why}"))?,
            None => Err(format!("{what} goes in r{reg}, but {var} isn't placed"))?,
        },
    }
}

/// An argument on the stack: `sp.x`, at `want` bytes above sp
fn check_on_stack(arg: &Expr, want: i32, what: &str, regmap: &Regmap) -> Result<(), Error> {
    let var = match arg {
        Expr::Field(base, var) if is_sp(base) => var.as_ident().unwrap_or_default(),
        _ => Err(format!("{what} goes on the stack, as sp.x at sp + {want}"))?,
    };
    let Some(sp) = regmap.sp() else {
        Err(format!("{what} goes on the stack, but sp isn't known here"))?
    };
    match regmap.stack_lookup(var) {
        Some(pos) if pos - sp == want => Ok(()),
        Some(pos) => Err(format!(
            "{what} goes at sp + {want}, but sp.{var} is at sp + {}",
            pos - sp
        ))?,
        None => match regmap.stack_lost(var) {
            Some(why) => Err(format!("{var} is no longer on the stack: {why}"))?,
            None => Err(format!("{var} isn't on the stack"))?,
        },
    }
}

/// A tail call returns to this function's caller, so it's a return as well
/// as a call: what the callee may clobber must be what this function may.
fn check_tail(call: &Call, sig: &FnSig, regmap: &Regmap, ctx: &FnCtx) -> Result<(), Error> {
    if ctx.abi == Abi::Wild {
        return Ok(());
    }
    let name = call.target.as_ident().unwrap_or_default();
    if let Some(ours) = ctx.own_sig()
        && ours.rets != sig.rets
    {
        Err(format!(
            "a tail call returns {name}'s results, which differ from this function's"
        ))?;
    }
    for &r in ctx.abi.callee_saved() {
        if sig.abi.clobbers().get(r as usize) {
            Err(format!(
                "{name} is a {} function, which may clobber {}, but this {} function must keep it",
                sig.abi.name(),
                reg_name(r),
                ctx.abi.name()
            ))?;
        }
    }
    check_exit(regmap, ctx, true, "the tail call")
}

/// At a return: sp back where it was, and the registers to restore holding
/// their entry values. `regmap` is the state as control leaves; `lr` says
/// whether lr must hold the return address, which a pop into pc doesn't
/// need.
fn check_exit(regmap: &Regmap, ctx: &FnCtx, lr: bool, what: &str) -> Result<(), Error> {
    if ctx.abi == Abi::Wild {
        return Ok(());
    }
    let Some(sp) = regmap.sp() else {
        return Ok(());
    };
    if sp != 0 {
        let dir = if sp < 0 { "below" } else { "above" };
        Err(format!(
            "at {what}, sp is {} {dir} where it was at entry",
            sp.abs()
        ))?;
    }
    let mut regs: Vec<u8> = ctx.abi.callee_saved().to_vec();
    if lr {
        regs.push(14);
    }
    for r in regs {
        let want = entry_var(r);
        if regmap.holds(r) == Some(want.as_str()) {
            continue;
        }
        let why = match regmap.lookup(&want) {
            Some(other) => format!("it's in {}", reg_name(other)),
            None => regmap
                .killed_why(r)
                .map(|s| s.to_owned())
                .unwrap_or_else(|| format!("{} was changed", reg_name(r))),
        };
        Err(format!(
            "at {what}, {} should hold {}: {why}",
            reg_name(r),
            describe(&want)
        ))?;
    }
    Ok(())
}

/// The results must be placed at a return
fn check_rets(regmap: &Regmap, ctx: &FnCtx, what: &str) -> Result<(), Error> {
    if ctx.abi == Abi::Wild {
        return Ok(());
    }
    for (i, ret) in ctx.rets.iter().enumerate() {
        if regmap.lookup(ret) != Some(i as u8) {
            let why = regmap
                .lost(ret)
                .unwrap_or("it isn't assigned on every path here");
            Err(format!("at {what}, result {ret} should be in r{i}: {why}"))?;
        }
    }
    Ok(())
}

/// Check a statement that isn't a call
pub fn check_stmt(stmt: &Stmt, regmap: &Regmap, ctx: &FnCtx) -> Result<(), Error> {
    match stmt {
        Stmt::Insn(Insn::Bx(target)) if target.as_ident() == Some("lr") => {
            check_exit(regmap, ctx, true, "this return")?;
            check_rets(regmap, ctx, "this return")
        }
        Stmt::Insn(Insn::Pop(regs)) if regs.iter().any(|r| r.as_ident() == Some("pc")) => {
            if ctx.abi == Abi::Wild {
                return Ok(());
            }
            let n = regs.len() as i32;
            // pc gets the last slot popped
            if let Some(sp) = regmap.sp() {
                let lr = regmap.slot(sp + 4 * (n - 1)).map(|s| s.var.as_str());
                if lr != Some("%lr") {
                    Err(format!(
                        "popping pc returns, but its slot holds {}, not the lr this function had at entry",
                        lr.map_or("something unnamed".to_owned(), describe)
                    ))?;
                }
            }
            let mut after = regmap.clone();
            after.apply(stmt, ctx);
            check_exit(&after, ctx, false, "this return")?;
            check_rets(&after, ctx, "this return")
        }
        Stmt::Insn(Insn::Bl(name)) => {
            if ctx.globals.fn_ty(name, ctx.types).is_none() {
                Err(format!(
                    "bl to {name}, which isn't defined; declare it with symbol {name}: fn(...)"
                ))?;
            }
            Ok(())
        }
        Stmt::Assign(lhs, _, _) | Stmt::WithFlagsAssign(lhs, _, _) if is_sp(lhs) => {
            if ctx.abi != Abi::Wild {
                Err(
                    "sp can only change by a constant (sp -= n), or by push and pop, \
                     outside a wild function",
                )?;
            }
            Ok(())
        }
        Stmt::WithAddrUpdate(_, lhs, _, _) if is_sp(lhs) => {
            if ctx.abi != Abi::Wild {
                Err("sp can't be updated by a load or store yet, outside a wild function")?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
