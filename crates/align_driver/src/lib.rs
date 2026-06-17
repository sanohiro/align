//! ドライバ: 各段を繋ぐ (`docs/impl/01-pipeline.md`)。
//!
//! `source.align` → lexer → parser → sema → MIR → (codegen) のパイプラインを
//! ライブラリ関数として公開する。`alignc` バイナリ (`main.rs`) と統合テストの両方が
//! これを呼ぶ。

use align_diag::{Diagnostics, Severity};
use align_span::SourceMap;

/// パイプラインを sema まで通した結果。
pub struct Checked {
    pub hir: align_sema::Program,
    pub diags: Diagnostics,
}

/// lexer → parser → sema。診断は `Checked.diags` に集約する。
pub fn check(source_map: &mut SourceMap, name: &str, src: &str) -> Checked {
    let file = source_map.add_file(name, src);
    let mut diags = Diagnostics::new();

    let tokens = align_lexer::tokenize(file, src, &mut diags);
    let ast = align_parser::parse_file(tokens, &mut diags);
    let hir = align_sema::check_file(&ast, &mut diags);

    Checked { hir, diags }
}

/// sema を通った HIR を MIR まで降ろす。
pub fn lower_to_mir(hir: &align_sema::Program) -> align_mir::Program {
    align_mir::lower_program(hir)
}

/// LLVM バックエンドが利用可能か (codegen が結線済みか)。
pub fn backend_available() -> bool {
    align_codegen_llvm::is_available()
}

/// MIR を object ファイルへ書き出す (codegen)。
pub fn emit_object_file(mir: &align_mir::Program, obj: &std::path::Path) -> Result<(), String> {
    align_codegen_llvm::emit_object(mir, obj).map_err(|e| e.to_string())
}

/// MIR を LLVM IR テキストへ (`alignc emit-llvm`)。
pub fn emit_llvm_ir(mir: &align_mir::Program) -> Result<String, String> {
    align_codegen_llvm::emit_llvm_ir(mir).map_err(|e| e.to_string())
}

/// object を実行ファイルへリンクする。システムの C コンパイラ (`cc`) を使い、crt0 が
/// 生成コードの `main` をエントリとして呼ぶ (`docs/impl/01-pipeline.md`: driver がリンク)。
pub fn link_executable(obj: &std::path::Path, exe: &std::path::Path) -> Result<(), String> {
    let status = std::process::Command::new("cc")
        .arg(obj)
        .arg("-o")
        .arg(exe)
        .status()
        .map_err(|e| format!("cc を起動できません: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("リンク失敗 (cc 終了コード {:?})", status.code()))
    }
}

/// 診断を人間向けに整形する (1行1件、`file:line:col: severity: message`)。
pub fn format_diagnostics(source_map: &SourceMap, diags: &Diagnostics) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for d in diags.iter() {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        if let Some(span) = d.span {
            let f = source_map.get(span.file);
            let (line, col) = f.line_col(span.lo);
            let _ = writeln!(out, "{}:{}:{}: {}: {}", f.name, line, col, sev, d.message);
        } else {
            let _ = writeln!(out, "{}: {}", sev, d.message);
        }
    }
    out
}
