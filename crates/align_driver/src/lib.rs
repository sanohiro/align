//! Driver: connects the stages (`docs/impl/01-pipeline.md`).
//!
//! Exposes the `source.align` -> lexer -> parser -> sema -> MIR -> (codegen)
//! pipeline as library functions. Both the `alignc` binary (`main.rs`) and the
//! integration tests call this.

use align_diag::{Diagnostics, Severity};
use align_span::SourceMap;

/// Result of running the pipeline through sema.
pub struct Checked {
    pub hir: align_sema::Program,
    pub diags: Diagnostics,
}

/// lexer -> parser -> sema. Diagnostics are collected into `Checked.diags`.
pub fn check(source_map: &mut SourceMap, name: &str, src: &str) -> Checked {
    let file = source_map.add_file(name, src);
    let mut diags = Diagnostics::new();

    let tokens = align_lexer::tokenize(file, src, &mut diags);
    let ast = align_parser::parse_file(tokens, &mut diags);
    let hir = align_sema::check_file(&ast, &mut diags);

    Checked { hir, diags }
}

/// Lower the sema-checked HIR down to MIR.
pub fn lower_to_mir(hir: &align_sema::Program) -> align_mir::Program {
    align_mir::lower_program(hir)
}

/// Whether the LLVM backend is available (codegen is wired up).
pub fn backend_available() -> bool {
    align_codegen_llvm::is_available()
}

/// Write MIR out to an object file (codegen).
pub fn emit_object_file(mir: &align_mir::Program, obj: &std::path::Path) -> Result<(), String> {
    align_codegen_llvm::emit_object(mir, obj).map_err(|e| e.to_string())
}

/// MIR to LLVM IR text (`alignc emit-llvm`).
pub fn emit_llvm_ir(mir: &align_mir::Program) -> Result<String, String> {
    align_codegen_llvm::emit_llvm_ir(mir).map_err(|e| e.to_string())
}

/// Link an object into an executable. Uses the system C compiler (`cc`); crt0 calls
/// the generated `main` as the entry point (`docs/impl/01-pipeline.md`: driver links).
pub fn link_executable(obj: &std::path::Path, exe: &std::path::Path) -> Result<(), String> {
    let status = std::process::Command::new("cc")
        .arg(obj)
        .arg("-o")
        .arg(exe)
        .status()
        .map_err(|e| format!("cannot launch cc: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("link failed (cc exit code {:?})", status.code()))
    }
}

/// Format diagnostics for humans (one per line, `file:line:col: severity: message`).
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
