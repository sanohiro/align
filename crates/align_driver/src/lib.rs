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
    align_codegen_llvm::emit_object(mir, obj, &target).map_err(|e| e.to_string())
}

/// MIR to LLVM IR text (`alignc emit-llvm`).
pub fn emit_llvm_ir(mir: &align_mir::Program, target: BuildTarget) -> Result<String, String> {
    align_codegen_llvm::emit_llvm_ir(mir, &target).map_err(|e| e.to_string())
}

/// Link an object into an executable. Uses the system C compiler (`cc`); crt0 calls
/// the generated `main` as the entry point (`docs/impl/01-pipeline.md`: driver links).
///
/// The thin runtime (`libalign_runtime.a`, e.g. the builtin `print`) is linked in too.
/// Being a Rust staticlib, it needs the usual std support libraries (`pthread`/`dl`/`m`).
pub fn link_executable(obj: &std::path::Path, exe: &std::path::Path, link_libs: &[String]) -> Result<(), String> {
    link_objects(&[obj], exe, link_libs)
}

/// Link one or more object files (plus the Align runtime and the always-linked C libraries) into an
/// executable. The single-object [`link_executable`] is the common case; multiple objects are used
/// by the FFI tests that link an Align object against a compiled C-helper object (a by-value struct
/// callee), and by any future multi-translation-unit build.
pub fn link_objects(objs: &[&std::path::Path], exe: &std::path::Path, link_libs: &[String]) -> Result<(), String> {
    let runtime = runtime_archive()?;
    let mut cmd = std::process::Command::new("cc");
    for obj in objs {
        cmd.arg(obj);
    }
    cmd.arg(&runtime)
        .arg("-o")
        .arg(exe)
        // Link hygiene (M13 Slice 2). `--gc-sections` drops every unreferenced input section from the
        // final image; combined with the runtime's per-function sections (Rust's default) this
        // garbage-collects the `std.compress`/`std.crypto`/`std.http` code a program does not use,
        // eliminating its `libz`/`libzstd`/`libcrypto`/`libssl` references so those libraries are not
        // needed at all. `--as-needed` then records `DT_NEEDED` only for libraries that actually
        // satisfy a surviving reference (a no-op for the merged-libc support libs below on modern
        // glibc, a portable win on older systems). Both are safe unconditionally — no build profile.
        .args(["-Wl,--gc-sections", "-Wl,--as-needed"])
        // `libpthread`/`libdl`/`libm` are linked unconditionally: they are Rust-std support libraries
        // the runtime *core* may reference (threads, dlopen, math) independent of any Align feature,
        // and modern glibc merges them into libc so they cost nothing (`--as-needed` drops any that
        // resolve nothing). They are NOT capability-gated — the runtime core, not an opt-in feature,
        // is what needs them.
        .args(["-lpthread", "-ldl", "-lm"]);
    // Capability + user libraries. `libz`/`libzstd`/`libcrypto`/`libssl` are NO LONGER linked
    // unconditionally: they now arrive through `link_libs`, which MIR populates from the builtins a
    // program actually uses (`align_mir::Capability`) plus any `extern "C" link("name")` the user
    // declared (validated in sema). All go AFTER the objects/archive that reference them (`-l`
    // resolves left-to-right against preceding inputs). Each name is a single `-l<name>` argv (no
    // shell/flag injection). A program using no gated feature links none of z/zstd/crypto/ssl.
    for lib in link_libs {
        cmd.arg(format!("-l{lib}"));
    }
    let status = cmd
        .status()
        .map_err(|e| format!("cannot launch cc: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("link failed (cc exit code {:?})", status.code()))
    }
}

/// The in-tree `align_runtime` source directory, baked in at build time (relative to this
/// crate's manifest). Present only when `alignc` runs from inside the workspace; an installed
/// binary has no source tree, so the staleness check below simply no-ops there.
const RUNTIME_SRC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../align_runtime/src");

/// Locate `libalign_runtime.a`, built by `cargo build` alongside the `alignc` binary.
/// The integration tests run from `target/<profile>/deps/`, so the parent is checked too.
fn runtime_archive() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find current exe: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "executable has no parent directory".to_string())?;
    for cand in [dir.join("libalign_runtime.a"), dir.join("../libalign_runtime.a")] {
        if cand.exists() {
            ensure_archive_fresh(&cand)?;
            return Ok(cand);
        }
    }
    Err(format!(
        "cannot find libalign_runtime.a near {}; run `cargo build` first",
        dir.display()
    ))
}

/// Fail loudly if `libalign_runtime.a` is older than the `align_runtime` source.
///
/// `align_driver` has no cargo dependency edge to the runtime *staticlib*, and a unit-test
/// build (`cargo test -p align_runtime`) recompiles only the test harness — neither refreshes
/// the `.a`. So editing the runtime and re-running the driver/tests without a full `cargo build`
/// would silently link a *stale* archive: wrong behavior and baffling test failures (this has
/// bitten development; see `open-questions.md`). Converting that into an actionable error is the
/// stable-toolchain fix (an artifact dependency, the clean edge, is still nightly-only).
///
/// No-ops when the source tree is absent (an installed `alignc`) or unreadable — it only ever
/// turns a definitely-stale link into an error, never blocks a legitimate one.
fn ensure_archive_fresh(archive: &std::path::Path) -> Result<(), String> {
    let src = std::path::Path::new(RUNTIME_SRC_DIR);
    if !src.is_dir() {
        return Ok(()); // installed binary: no source tree to compare against
    }
    let Ok(archive_mtime) = archive.metadata().and_then(|m| m.modified()) else {
        return Ok(()); // cannot stat the archive: do not block the build
    };
    if let Some(newest) = newest_rs_mtime(src)
        && newest > archive_mtime {
            return Err(format!(
                "libalign_runtime.a is stale: a source file under {} is newer than the archive \
                 {}.\nThe driver has no cargo edge to the runtime staticlib, so run `cargo build` \
                 to refresh it before linking.",
                src.display(),
                archive.display(),
            ));
        }
    Ok(())
}

/// Newest modification time among `*.rs` files under `dir` (recursive); `None` if there are
/// none or the tree is unreadable. Unreadable subdirectories are skipped, not fatal — the check
/// must never disable itself silently on a single bad entry.
fn newest_rs_mtime(dir: &std::path::Path) -> Option<std::time::SystemTime> {
    let mut newest: Option<std::time::SystemTime> = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            // `file_type()` comes from the `read_dir` iterator with no extra `stat`, and (unlike
            // `path.is_dir()`) does not follow symlinks — so a symlinked dir is not traversed,
            // avoiding cycles / escaping the source tree. We `stat` only actual `.rs` files.
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() && entry.path().extension().is_some_and(|x| x == "rs")
                && let Ok(t) = entry.metadata().and_then(|m| m.modified()) {
                    newest = Some(newest.map_or(t, |n| n.max(t)));
                }
        }
    }
    newest
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_rs_mtime_scans_recursively_and_filters_extension() {
        // Unique temp dir (no Date/rand in-crate; pid + a stack address suffice here).
        let root = std::env::temp_dir().join(format!(
            "align-driver-mtime-{}-{:p}",
            std::process::id(),
            &0u8 as *const _
        ));
        let sub = root.join("nested");
        std::fs::create_dir_all(&sub).expect("create temp tree");

        // Empty (no `.rs`) → None.
        assert_eq!(newest_rs_mtime(&root), None, "no .rs files yet");

        // A non-`.rs` file is ignored.
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        assert_eq!(newest_rs_mtime(&root), None, ".txt is not counted");

        // `.rs` files at the top level and in a subdir are both found; the result is their max
        // mtime. Compare against an independent scan so the test does not depend on write timing.
        std::fs::write(root.join("a.rs"), b"fn a() {}").unwrap();
        std::fs::write(sub.join("b.rs"), b"fn b() {}").unwrap();
        let expect = [root.join("a.rs"), sub.join("b.rs")]
            .iter()
            .map(|p| p.metadata().unwrap().modified().unwrap())
            .max()
            .unwrap();
        assert_eq!(
            newest_rs_mtime(&root),
            Some(expect),
            "finds the newest .rs across the top level and the nested dir"
        );

        // A missing directory yields None (read_dir fails, skipped, not a panic).
        assert_eq!(newest_rs_mtime(&root.join("does-not-exist")), None);

        std::fs::remove_dir_all(&root).ok();
    }
}
