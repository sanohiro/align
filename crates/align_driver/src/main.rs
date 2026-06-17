//! `alignc` CLI (`docs/impl/01-pipeline.md`)。
//!
//! サブコマンド:
//!   alignc check     <file>   lexer → parser → sema まで。診断を表示
//!   alignc emit-mir  <file>   MIR をテキスト表示
//!   alignc emit-llvm <file>   LLVM IR をテキスト表示
//!   alignc build     <file>   実行ファイルを生成 (カレントに <stem>)
//!   alignc run       <file>   build して実行し、その終了コードを返す

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
           check      lexer/parser/sema まで検査\n  \
           emit-mir   MIR をテキスト出力\n  \
           emit-llvm  LLVM IR をテキスト出力\n  \
           build      実行ファイルを生成\n  \
           run        build して実行 (終了コードを返す)"
    );
}

fn read(path: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("alignc: '{path}' を読めません: {e}");
            None
        }
    }
}

/// check → MIR まで。エラーがあれば診断を出して `None`。
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
        println!("ok: {} 個の関数を検査しました", checked.hir.fns.len());
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

/// ソースのファイル名 (拡張子なし) を出力名にする。
fn stem(path: &str) -> String {
    PathBuf::from(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "a".to_string())
}

/// MIR を object にして実行ファイルへリンクする。`exe` のパスを返す。
fn build_to(path: &str, mir: &align_mir::Program, exe: &PathBuf) -> Result<(), ExitCode> {
    let obj = std::env::temp_dir().join(format!("align-{}.o", stem(path)));
    if let Err(e) = emit_object_file(mir, &obj) {
        eprintln!("alignc: codegen 失敗: {e}");
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
            println!("alignc: 実行ファイルを生成しました: {}", exe.display());
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
                eprintln!("alignc: プロセスがシグナルで終了しました");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("alignc: 実行できません: {e}");
            ExitCode::FAILURE
        }
    }
}
