// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::{collections::HashMap, io::Read, io::Write};

use clap::Parser;

use symbolasm::{Source, analyze, compile, error::report_error, load};

#[derive(Parser)]
struct Args {
    /// Source files, compiled as one program, as if concatenated in order,
    /// each after the files it names with `use "file"`.
    #[arg(required = true)]
    paths: Vec<String>,

    /// Read this file's text from stdin instead of from disk (for an
    /// editor's unsaved buffer). Its uses still resolve from its path.
    #[arg(long)]
    stdin: Option<String>,

    #[arg(short, long)]
    svd: Option<String>,

    /// Instead of asm, print a per-line size and cycle analysis of the
    /// functions in the files named here, not those only reached by use.
    #[arg(long)]
    analyze: bool,

    /// With --analyze, only this function.
    #[arg(long = "fn")]
    func: Option<String>,

    /// With --analyze, the size column: n (2 or 4), nib (first hex digit
    /// of each halfword), or hex.
    #[arg(long, default_value = "nib")]
    size: String,

    /// With --analyze, the columns to show, comma separated, from
    /// line,size,szwhy,cycwhy,cyc,asm,src.
    #[arg(long, default_value = "line,size,szwhy,cycwhy,cyc,asm,src")]
    cols: String,

    /// With --analyze, print the legend for the why columns first.
    #[arg(long)]
    legend: bool,
}

fn main() {
    let args = Args::parse();
    let mut overrides = HashMap::new();
    if let Some(path) = &args.stdin {
        let canon = load::canonical(path, None).unwrap_or_else(|e| {
            e.report();
            std::process::exit(1);
        });
        let mut text = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut text) {
            eprintln!("error: reading stdin: {e}");
            std::process::exit(1);
        }
        overrides.insert(canon, text);
    }
    let source = Source::load(&args.paths, overrides).unwrap_or_else(|e| {
        e.report();
        std::process::exit(1);
    });
    if let Err(err) = run(&args, &source) {
        report_error(&source.files, &source.src, &*err);
        std::process::exit(1);
    }
}

fn run(args: &Args, source: &Source) -> Result<(), symbolasm::error::Error> {
    let out = compile(source, args.svd.as_deref(), args.analyze)?;
    if args.analyze {
        let opts = analyze::Opts::parse(&args.size, &args.cols, args.func.clone(), args.legend)?;
        let asm = String::from_utf8(out.asm)?;
        analyze::analyze(
            &asm,
            &source.src,
            &source.files,
            &source.named,
            &out.fns,
            &opts,
        )?;
    } else {
        std::io::stdout().write_all(&out.asm)?;
    }
    Ok(())
}
