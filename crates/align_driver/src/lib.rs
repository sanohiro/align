//! Driver: connects the stages (`docs/impl/01-pipeline.md`).
//!
//! Exposes the `source.align` -> lexer -> parser -> sema -> MIR -> (codegen)
//! pipeline as library functions. Both the `alignc` binary (`main.rs`) and the
//! integration tests call this.

use align_diag::{Diagnostics, Severity};
use align_span::SourceMap;
pub use align_codegen_llvm::BuildTarget;

/// Result of running the pipeline through sema.
pub struct Checked {
    pub hir: align_sema::Program,
    pub diags: Diagnostics,
}

/// lexer -> parser -> sema for the entry file plus its transitively-imported **user** modules
/// (multi-file, slice B1). User modules resolve by filename convention: `import geom` →
/// `<entry-dir>/geom.align`, which must declare `module geom`. Builtin imports (`core.*`/`std.*`)
/// are not files. Diagnostics are collected into `Checked.diags`.
pub fn check(source_map: &mut SourceMap, name: &str, src: &str) -> Checked {
    let mut diags = Diagnostics::new();
    let entry_dir = std::path::Path::new(name).parent().map(|p| p.to_path_buf());

    /// A parsed source module awaiting checking (kept alive so the `Module` borrows are valid).
    struct Loaded {
        path: String,
        ast: align_ast::File,
        is_entry: bool,
    }

    // A user-module import is one whose first segment is neither `core` nor `std` (builtins).
    fn user_import(p: &align_ast::Path) -> bool {
        p.segments.first().is_some_and(|s| s.name != "core" && s.name != "std")
    }

    // The entry module's own name is its `module` decl, or `main` by default.
    let entry_tokens = align_lexer::tokenize(source_map.add_file(name, src), src, &mut diags);
    let entry_ast = align_parser::parse_file(entry_tokens, &mut diags);
    let entry_path = entry_ast
        .module
        .as_ref()
        .and_then(|m| m.segments.last())
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "main".to_string());

    let mut loaded = vec![Loaded { path: entry_path.clone(), ast: entry_ast, is_entry: true }];
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::from([entry_path]);

    // Breadth-first over user-module imports, resolving each to `<entry-dir>/<name>.align`.
    let mut i = 0;
    while i < loaded.len() {
        let imports: Vec<align_ast::Path> =
            loaded[i].ast.imports.iter().filter(|p| user_import(p)).cloned().collect();
        i += 1;
        for imp in imports {
            // The dotted module path (`util.math`) and the matching file path under the entry
            // directory (`util/math.align`): each segment is a directory, the last names the file.
            let modpath = imp.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(".");
            if !seen.insert(modpath.clone()) {
                continue; // already loaded (shared / cyclic import)
            }
            let Some(dir) = &entry_dir else {
                diags.error(format!("cannot resolve `import {modpath}`: the entry file has no directory"), imp.span);
                continue;
            };
            let mut file_path = dir.clone();
            for seg in &imp.segments {
                file_path.push(&seg.name);
            }
            file_path.set_extension("align");
            let msrc = match std::fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(e) => {
                    diags.error(format!("cannot find module `{modpath}` (expected {}): {e}", file_path.display()), imp.span);
                    continue;
                }
            };
            let fid = source_map.add_file(file_path.display().to_string(), msrc.clone());
            let toks = align_lexer::tokenize(fid, &msrc, &mut diags);
            let mast = align_parser::parse_file(toks, &mut diags);
            // The file must declare the full `module util.math` (path ↔ filename agreement).
            let declared = mast.module.as_ref().map(|m| m.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("."));
            if declared.as_deref() != Some(modpath.as_str()) {
                diags.error(
                    format!("module file `{}` must declare `module {modpath}` (found {})", file_path.display(),
                        declared.map(|d| format!("`module {d}`")).unwrap_or_else(|| "no module declaration".to_string())),
                    imp.span,
                );
            }
            loaded.push(Loaded { path: modpath, ast: mast, is_entry: false });
        }
    }

    let modules: Vec<align_sema::Module> = loaded
        .iter()
        .map(|l| align_sema::Module { path: l.path.clone(), file: &l.ast, is_entry: l.is_entry })
        .collect();
    let hir = align_sema::check_program(&modules, &mut diags);

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

/// Write MIR out to an object file (codegen). `target` selects the CPU baseline (portable default
/// vs. host-`native`).
pub fn emit_object_file(mir: &align_mir::Program, obj: &std::path::Path, target: BuildTarget) -> Result<(), String> {
    align_codegen_llvm::emit_object(mir, obj, target).map_err(|e| e.to_string())
}

/// MIR to LLVM IR text (`alignc emit-llvm`).
pub fn emit_llvm_ir(mir: &align_mir::Program, target: BuildTarget) -> Result<String, String> {
    align_codegen_llvm::emit_llvm_ir(mir, target).map_err(|e| e.to_string())
}

/// Link an object into an executable. Uses the system C compiler (`cc`); crt0 calls
/// the generated `main` as the entry point (`docs/impl/01-pipeline.md`: driver links).
///
/// The thin runtime (`libalign_runtime.a`, e.g. the builtin `print`) is linked in too.
/// Being a Rust staticlib, it needs the usual std support libraries (`pthread`/`dl`/`m`).
pub fn link_executable(obj: &std::path::Path, exe: &std::path::Path) -> Result<(), String> {
    let runtime = runtime_archive()?;
    let status = std::process::Command::new("cc")
        .arg(obj)
        .arg(&runtime)
        .arg("-o")
        .arg(exe)
        .args(["-lpthread", "-ldl", "-lm"])
        .status()
        .map_err(|e| format!("cannot launch cc: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("link failed (cc exit code {:?})", status.code()))
    }
}

/// Locate `libalign_runtime.a`, built by `cargo build` alongside the `alignc` binary.
/// The integration tests run from `target/<profile>/deps/`, so the parent is checked too.
fn runtime_archive() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find current exe: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "executable has no parent directory".to_string())?;
    for cand in [dir.join("libalign_runtime.a"), dir.join("../libalign_runtime.a")] {
        if cand.exists() {
            return Ok(cand);
        }
    }
    Err(format!(
        "cannot find libalign_runtime.a near {}; run `cargo build` first",
        dir.display()
    ))
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
