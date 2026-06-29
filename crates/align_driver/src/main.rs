//! `alignc` CLI (`docs/impl/01-pipeline.md`).
//!
//! Subcommands:
//!   alignc check     <file>   lexer -> parser -> sema. Print diagnostics
//!   alignc emit-mir  <file>   Print MIR as text
//!   alignc emit-llvm <file>   Print LLVM IR as text
//!   alignc emit-obj  <file>   Write an object file (no link, no `main` required)
//!   alignc build     <file>   Build an executable (<stem> in cwd)
//!   alignc run       <file>   Build, run, and return its exit code

use std::path::PathBuf;
use std::process::ExitCode;

use align_driver::{
    check, emit_llvm_ir, emit_object_file, format_diagnostics, link_executable, lower_to_mir,
    BuildTarget,
};
use align_span::SourceMap;

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().collect();
    // Pull the `--target-cpu` flag out before positional parsing (so it may sit anywhere up to the
    // program's own args, and `run` does not forward it to the built program).
    let (target, args) = parse_target(&raw);
    let cmd = args.get(1).map(String::as_str);
    let path = args.get(2);

    match (cmd, path) {
        (Some("check"), Some(p)) => run_check(p),
        (Some("emit-mir"), Some(p)) => run_emit_mir(p),
        (Some("emit-llvm"), Some(p)) => run_emit_llvm(p, target),
        // `emit-obj <file> [out.o]` — codegen to an object file, no linking and no `main` required
        // (a library / benchmark kernel). Default output is `<stem>.o`.
        (Some("emit-obj"), Some(p)) => run_emit_obj(p, args.get(3).map(String::as_str), target),
        // `fmt <file> [--write]` — format source; prints to stdout, or rewrites in place with --write.
        (Some("fmt"), Some(p)) => run_fmt(p, &args[3..]),
        (Some("build"), Some(p)) => run_build(p, target),
        // `run` forwards any trailing arguments to the built program (its `main(args)`).
        (Some("run"), Some(p)) => run_run(p, &args[3..], target),
        _ => {
            usage();
            ExitCode::FAILURE
        }
    }
}

/// Pull `--target-cpu <baseline|native>` (or `--target-cpu=…`) out of `args`, returning the chosen
/// target and the remaining (positional) arguments. Default = the portable `Baseline`.
fn parse_target(args: &[String]) -> (BuildTarget, Vec<String>) {
    // `baseline` / `native` are keywords; anything else is passed to LLVM as a CPU name
    // (`x86-64-v3`, `znver3`, …) — the portable-performance tier for a fleet you control.
    let value = |v: &str| match v {
        "native" => BuildTarget::Native,
        "baseline" => BuildTarget::Baseline,
        other => BuildTarget::Cpu(other.to_string()),
    };
    let mut target = BuildTarget::Baseline;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(v) = a.strip_prefix("--target-cpu=") {
            target = value(v);
        } else if a == "--target-cpu" {
            if let Some(v) = args.get(i + 1) {
                target = value(v);
                i += 1;
            } else {
                eprintln!("alignc: missing value for --target-cpu (expected `baseline` or `native`); using baseline");
            }
        } else {
            rest.push(a.clone());
        }
        i += 1;
    }
    (target, rest)
}

fn usage() {
    eprintln!(
        "usage: alignc <command> <file.align> [--target-cpu baseline|native]\n\
         \n\
         commands:\n  \
           check      check through lexer/parser/sema\n  \
           emit-mir   print MIR as text\n  \
           emit-llvm  print LLVM IR as text\n  \
           emit-obj   write an object file (<file> [out.o]; no link, no `main` needed)\n  \
           fmt        format source (prints to stdout; --write rewrites in place)\n  \
           build      build an executable\n  \
           run        build and run (returns the exit code)\n  \
         \n\
         --target-cpu  baseline (default; portable per-arch floor), native (this host's CPU),\n  \
                       or an LLVM CPU name like x86-64-v3 (a portable fast tier for a known fleet)"
    );
}

fn read(path: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("alignc: cannot read '{path}': {e}");
            None
        }
    }
}

/// check -> MIR. On error, print diagnostics and return `None`.
fn front_to_mir(path: &str) -> Option<align_mir::Program> {
    let src = read(path)?;
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, path, &src);
    if checked.diags.has_errors() {
        print!("{}", format_diagnostics(&sm, &checked.diags));
        return None;
    }
    if !checked.diags.is_empty() {
        print!("{}", format_diagnostics(&sm, &checked.diags));
    }
    Some(lower_to_mir(&checked.hir))
}

/// `fmt <file> [--write]` — format the source. Without `--write`, print the formatted text to
/// stdout (a read-only default); with `--write`/`-w`, rewrite the file in place only if it changed.
/// If the source does not parse cleanly, it is left untouched (and `--write` is a no-op) — the
/// formatter never emits from a partial parse.
fn run_fmt(path: &str, flags: &[String]) -> ExitCode {
    let write = flags.iter().any(|f| f == "--write" || f == "-w");
    let Some(src) = read(path) else {
        return ExitCode::FAILURE;
    };
    let Some(formatted) = align_fmt::format_source(0, &src) else {
        eprintln!("alignc: cannot format '{path}' (it does not parse cleanly); left unchanged");
        return ExitCode::FAILURE;
    };
    if !write {
        print!("{formatted}");
    } else if formatted != src && let Err(e) = std::fs::write(path, &formatted) {
        eprintln!("alignc: cannot write '{path}': {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run_check(path: &str) -> ExitCode {
    let Some(src) = read(path) else {
        return ExitCode::FAILURE;
    };
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, path, &src);
    if !checked.diags.is_empty() {
        print!("{}", format_diagnostics(&sm, &checked.diags));
    }
    if checked.diags.has_errors() {
        ExitCode::FAILURE
    } else {
        println!("ok: checked {} function(s)", checked.hir.fns.len());
        ExitCode::SUCCESS
    }
}

fn run_emit_mir(path: &str) -> ExitCode {
    match front_to_mir(path) {
        Some(mir) => {
            print!("{}", align_mir::print::program_to_string(&mir));
            ExitCode::SUCCESS
        }
        None => ExitCode::FAILURE,
    }
}

fn run_emit_llvm(path: &str, target: BuildTarget) -> ExitCode {
    let Some(mir) = front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    match emit_llvm_ir(&mir, target) {
        Ok(ir) => {
            print!("{ir}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("alignc: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_emit_obj(path: &str, out: Option<&str>, target: BuildTarget) -> ExitCode {
    let Some(mir) = front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    let obj = PathBuf::from(out.map(String::from).unwrap_or_else(|| format!("{}.o", stem(path))));
    match emit_object_file(&mir, &obj, target) {
        Ok(()) => {
            println!("alignc: wrote object: {}", obj.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("alignc: codegen failed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Use the source file name (without extension) as the output name.
fn stem(path: &str) -> String {
    PathBuf::from(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "a".to_string())
}

/// Turn MIR into an object and link it into an executable. Returns the `exe` path.
fn build_to(path: &str, mir: &align_mir::Program, exe: &PathBuf, target: BuildTarget) -> Result<(), ExitCode> {
    let obj = std::env::temp_dir().join(format!("align-{}.o", stem(path)));
    if let Err(e) = emit_object_file(mir, &obj, target) {
        eprintln!("alignc: codegen failed: {e}");
        return Err(ExitCode::FAILURE);
    }
    if let Err(e) = link_executable(&obj, exe) {
        eprintln!("alignc: {e}");
        return Err(ExitCode::FAILURE);
    }
    Ok(())
}

fn run_build(path: &str, target: BuildTarget) -> ExitCode {
    let Some(mir) = front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    let exe = PathBuf::from(stem(path));
    match build_to(path, &mir, &exe, target) {
        Ok(()) => {
            println!("alignc: built executable: {}", exe.display());
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

fn run_run(path: &str, prog_args: &[String], target: BuildTarget) -> ExitCode {
    let Some(mir) = front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    let exe = std::env::temp_dir().join(format!("align-{}", stem(path)));
    if let Err(code) = build_to(path, &mir, &exe, target) {
        return code;
    }
    // Forward trailing args so they reach the program's `main(args: array<str>)` (argv[0] is the
    // executable, then `prog_args`).
    match std::process::Command::new(&exe).args(prog_args).status() {
        Ok(status) => match status.code() {
            Some(code) => ExitCode::from(code as u8),
            None => {
                eprintln!("alignc: process terminated by a signal");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("alignc: cannot run: {e}");
            ExitCode::FAILURE
        }
    }
}
