//! `alignc` CLI (`docs/impl/01-pipeline.md`).
//!
//! Subcommands:
//!   alignc check     <file>   lexer -> parser -> sema. Print diagnostics
//!   alignc emit-mir  <file>   Print MIR as text
//!   alignc emit-llvm <file>   Print LLVM IR as text
//!   alignc build     <file>   Build an executable (<stem> in cwd)
//!   alignc run       <file>   Build, run, and return its exit code

use std::path::PathBuf;
use std::process::ExitCode;

use align_driver::{
    check, emit_llvm_ir, emit_object_file, format_diagnostics, link_executable, lower_to_mir,
};
use align_span::SourceMap;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str);
    let path = args.get(2);

    match (cmd, path) {
        (Some("check"), Some(p)) => run_check(p),
        (Some("emit-mir"), Some(p)) => run_emit_mir(p),
        (Some("emit-llvm"), Some(p)) => run_emit_llvm(p),
        (Some("build"), Some(p)) => run_build(p),
        (Some("run"), Some(p)) => run_run(p),
        _ => {
            usage();
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!(
        "usage: alignc <command> <file.align>\n\
         \n\
         commands:\n  \
           check      check through lexer/parser/sema\n  \
           emit-mir   print MIR as text\n  \
           emit-llvm  print LLVM IR as text\n  \
           build      build an executable\n  \
           run        build and run (returns the exit code)"
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

fn run_emit_llvm(path: &str) -> ExitCode {
    let Some(mir) = front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    match emit_llvm_ir(&mir) {
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

/// Use the source file name (without extension) as the output name.
fn stem(path: &str) -> String {
    PathBuf::from(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "a".to_string())
}

/// Turn MIR into an object and link it into an executable. Returns the `exe` path.
fn build_to(path: &str, mir: &align_mir::Program, exe: &PathBuf) -> Result<(), ExitCode> {
    let obj = std::env::temp_dir().join(format!("align-{}.o", stem(path)));
    if let Err(e) = emit_object_file(mir, &obj) {
        eprintln!("alignc: codegen failed: {e}");
        return Err(ExitCode::FAILURE);
    }
    if let Err(e) = link_executable(&obj, exe) {
        eprintln!("alignc: {e}");
        return Err(ExitCode::FAILURE);
    }
    Ok(())
}

fn run_build(path: &str) -> ExitCode {
    let Some(mir) = front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    let exe = PathBuf::from(stem(path));
    match build_to(path, &mir, &exe) {
        Ok(()) => {
            println!("alignc: built executable: {}", exe.display());
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

fn run_run(path: &str) -> ExitCode {
    let Some(mir) = front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    let exe = std::env::temp_dir().join(format!("align-{}", stem(path)));
    if let Err(code) = build_to(path, &mir, &exe) {
        return code;
    }
    match std::process::Command::new(&exe).status() {
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
