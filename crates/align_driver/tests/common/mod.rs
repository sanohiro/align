//! Shared test harness for the `align_driver` integration tests. Each test file does
//! `mod common; use common::*;` and calls these helpers (and the re-exported driver API).
//!
//! `dead_code` / `unused_imports` are allowed because each test binary includes this whole module
//! but uses only the subset of helpers / re-exports it needs.
#![allow(dead_code, unused_imports)]

pub use align_driver::{
    backend_available, build_per_unit, check, check_per_unit, emit_llvm_ir, emit_object_file,
    link_executable, link_objects, lower_to_mir, lower_to_mir_per_unit, BuildTarget, Hash128,
    InterfaceSummary, ObjectFormat, PerUnitArtifact, PerUnitCheck, PerUnitWalk, Profile,
};
pub use align_span::SourceMap;

use std::path::PathBuf;

/// Whether a C compiler (`cc`) is available — the FFI by-value-struct tests compile a small C helper
/// (the by-value struct callee) and link it against the Align object. Skip those tests where `cc`
/// is absent (the backend itself may still be available for pure-Align tests).
pub fn cc_available() -> bool {
    std::process::Command::new("cc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `llvm-readobj`, if discoverable — the gate for binary-inspection assertions (the version-matched
/// LLVM tool the driver's `llvm_tool` locates; reads both ELF and Mach-O, unlike GNU `readelf`).
pub fn llvm_readobj() -> Option<PathBuf> {
    align_driver::llvm_tool("llvm-readobj")
}

/// The dynamic-dependency names of the binary at `path` (via `llvm-readobj --needed-libs`):
/// `DT_NEEDED` sonames on ELF, `LC_LOAD_DYLIB` install names (full paths) on Mach-O.
pub fn needed_libs(tool: &std::path::Path, path: &std::path::Path) -> Vec<String> {
    let out = std::process::Command::new(tool)
        .arg("--needed-libs")
        .arg(path)
        .env("LC_ALL", "C")
        .output()
        .expect("run llvm-readobj");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut libs = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let l = line.trim();
        if l == "NeededLibraries [" {
            inside = true;
        } else if inside && l == "]" {
            break;
        } else if inside && !l.is_empty() {
            libs.push(l.to_string());
        }
    }
    libs
}

/// Whether a dependency entry names the library `lib` (`is_lib(entry, "z")` ⇔ libz). Matches on the
/// path base name, so a Mach-O full install name (`/usr/lib/libz.1.dylib`) and an ELF soname
/// (`libz.so.1`) both classify.
pub fn is_lib(entry: &str, lib: &str) -> bool {
    let base = entry.rsplit('/').next().unwrap_or(entry);
    base.starts_with(&format!("lib{lib}."))
}

/// Compile `align_src` and `c_src` (a C helper defining the `extern "C"` callee), link them
/// together, run the result, and return its `Output`. This is the compiled-C-helper harness for
/// by-value struct FFI: the C side is built by the system `cc` (clang/gcc), so the round trip
/// validates Align's SysV register coercion against a real C ABI. Asserts the Align source
/// type-checks; caller should gate on [`backend_available`] and [`cc_available`].
pub fn build_and_run_with_c(name: &str, align_src: &str, c_src: &str) -> std::process::Output {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, align_src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let a_obj = dir.join(format!("align-ffic-{pid}-{name}.o"));
    let c_src_path = dir.join(format!("align-ffic-{pid}-{name}.c"));
    let c_obj = dir.join(format!("align-ffic-{pid}-{name}-helper.o"));
    let exe = dir.join(format!("align-ffic-{pid}-{name}{}", std::env::consts::EXE_SUFFIX));
    struct Cleanup(Vec<PathBuf>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for p in &self.0 {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    let _guard = Cleanup(vec![a_obj.clone(), c_src_path.clone(), c_obj.clone(), exe.clone()]);
    emit_object_file(&mir, &a_obj, BuildTarget::Baseline, Profile::Release, &[], false).expect("codegen");
    std::fs::write(&c_src_path, c_src).expect("write c helper");
    let cc_status = std::process::Command::new("cc")
        .args(["-c", "-O0"])
        .arg(&c_src_path)
        .arg("-o")
        .arg(&c_obj)
        .status()
        .expect("launch cc");
    assert!(cc_status.success(), "compiling the C helper failed");
    link_objects(&[&a_obj, &c_obj], &exe, &mir.link_libs, Profile::Release).expect("link");
    std::process::Command::new(&exe).output().expect("run")
}

/// Removes a test's temporary object + executable on scope exit — including on a panic (an
/// `assert_eq!` failure), so a failing test does not leak files into the temp directory.
struct TempArtifacts {
    obj: PathBuf,
    exe: PathBuf,
}

impl Drop for TempArtifacts {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.obj);
        let _ = std::fs::remove_file(&self.exe);
    }
}

/// Compile `src` to a native executable, run it, and return its `Output`. Asserts the program
/// type-checks. The temp object/exe are cleaned up even if the test later panics.
pub fn build_and_run(name: &str, src: &str) -> std::process::Output {
    build_and_run_args(name, src, &[])
}

/// [`build_and_run`] with trailing arguments forwarded to the compiled program.
pub fn build_and_run_args(name: &str, src: &str, prog_args: &[&str]) -> std::process::Output {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    // Lower with only `sm`/`checked` live in this frame, then hand off — the path/emit/link locals
    // live in `emit_link_run`'s frame, not this one. MIR lowering recurses per expression nesting
    // level, so keeping this frame lean is what lets a within-limit deep expression (`expr_depth`,
    // ~40-deep) lower on the 2 MB test thread with margin.
    let mir = lower_to_mir(&checked.hir);
    emit_link_run(&mir, name, prog_args)
}

/// Object-emit + link + run for an already-lowered program — its own stack frame, so the
/// (potentially deep) MIR lowering in the caller does not compete with these locals.
#[inline(never)]
fn emit_link_run(mir: &align_driver::MirProgram, name: &str, prog_args: &[&str]) -> std::process::Output {
    let dir = std::env::temp_dir();
    // Include the process id so two concurrent test-suite runs on one machine (e.g. parallel CI)
    // don't collide on these temp paths.
    let pid = std::process::id();
    let obj = dir.join(format!("align-test-{pid}-{name}.o"));
    let exe = dir.join(format!("align-test-{pid}-{name}{}", std::env::consts::EXE_SUFFIX));
    let _artifacts = TempArtifacts { obj: obj.clone(), exe: exe.clone() };
    emit_object_file(mir, &obj, BuildTarget::Baseline, Profile::Release, &[], false).expect("codegen");
    link_executable(&obj, &exe, &mir.link_libs, Profile::Release).expect("link");
    std::process::Command::new(&exe).args(prog_args).output().expect("run")
}

/// A compiled Align executable plus a guard that removes its object + exe on drop. Returned by
/// [`build_exe`] for tests that must **spawn** the program (rather than run-to-completion) — e.g. a
/// long-running server the test drives as a client on another thread.
pub struct BuiltExe {
    pub exe: PathBuf,
    _artifacts: TempArtifacts,
}

/// Compile `src` to a native executable and return its path (without running it). Unlike
/// [`build_and_run`], the caller spawns the process itself, so a server program that blocks on
/// `accept` can be driven by a client in the same test. Asserts the program type-checks; the temp
/// object/exe are cleaned up when the returned [`BuiltExe`] is dropped (even on a later panic).
pub fn build_exe(name: &str, src: &str) -> BuiltExe {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let obj = dir.join(format!("align-test-{pid}-{name}.o"));
    let exe = dir.join(format!("align-test-{pid}-{name}{}", std::env::consts::EXE_SUFFIX));
    emit_object_file(&mir, &obj, BuildTarget::Baseline, Profile::Release, &[], false).expect("codegen");
    link_executable(&obj, &exe, &mir.link_libs, Profile::Release).expect("link");
    BuiltExe { exe: exe.clone(), _artifacts: TempArtifacts { obj, exe } }
}

/// The LLVM IR text for `src` (for asserting on the generated instructions).
pub fn emit_llvm(src: &str) -> String {
    emit_llvm_with_exports(src, &[])
}

/// [`emit_llvm`] with explicit export roots (`--export`) — the names in `exports` keep external
/// linkage instead of the default whole-program `internal` (M13 Slice 1 / the export-roots
/// mechanism, `docs/impl/07-roadmap.md` M13 Codex-audit item 1).
pub fn emit_llvm_with_exports(src: &str, exports: &[&str]) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "ir", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let exports: Vec<String> = exports.iter().map(|s| s.to_string()).collect();
    align_driver::emit_llvm_ir(&mir, BuildTarget::Baseline, false, &exports, false).expect("emit llvm ir")
}

/// Whether checking `src` produces any error (for negative tests).
pub fn check_errs(name: &str, src: &str) -> bool {
    let mut sm = SourceMap::new();
    check(&mut sm, name, src).diags.has_errors()
}

/// The rendered diagnostics from checking `src` (for negative tests that assert the *message*, not
/// just that some error occurred).
pub fn check_diagnostics(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    align_driver::format_diagnostics(&sm, &checked.diags)
}

/// A temp directory written with the given `(filename, source)` files, removed on scope exit.
/// Used by the multi-file (module-system) tests: the driver resolves `import`s from disk relative
/// to the entry file, so the modules must be real files in one directory.
struct TempProject {
    dir: PathBuf,
}

impl TempProject {
    fn new(name: &str, files: &[(&str, &str)]) -> TempProject {
        let dir = std::env::temp_dir().join(format!("align-mtest-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir temp project");
        for (fname, src) in files {
            let path = dir.join(fname);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("mkdir module subdir");
            }
            std::fs::write(path, src).expect("write module file");
        }
        TempProject { dir }
    }
    fn entry(&self, entry: &str) -> PathBuf {
        self.dir.join(entry)
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Compile + run a multi-file program. `files` are `(filename, source)` written to a fresh temp
/// directory; `entry` is the entry filename. The entry is compiled by path so the driver resolves
/// `import`s from disk. Asserts it type-checks; returns the program `Output`.
pub fn build_and_run_multi(name: &str, files: &[(&str, &str)], entry: &str) -> std::process::Output {
    let proj = TempProject::new(name, files);
    let entry_path = proj.entry(entry);
    let entry_src = std::fs::read_to_string(&entry_path).expect("read entry");
    let entry_name = entry_path.display().to_string();
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, &entry_name, &entry_src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let pid = std::process::id();
    let obj = proj.dir.join(format!("align-mtest-{pid}-{name}.o"));
    let exe = proj.dir.join(format!("align-mtest-{pid}-{name}{}", std::env::consts::EXE_SUFFIX));
    emit_object_file(&mir, &obj, BuildTarget::Baseline, Profile::Release, &[], false).expect("codegen");
    link_executable(&obj, &exe, &mir.link_libs, Profile::Release).expect("link");
    std::process::Command::new(&exe).output().expect("run")
}

/// The outcome of running BOTH the whole-program checker and the M15 S1b per-unit checker on the
/// same multi-file program — the differential-equivalence gate's raw material.
pub struct DiffCheck {
    /// Whole-program `check` reported at least one error.
    pub whole_errors: bool,
    /// Per-unit `check_per_unit` reported at least one error.
    pub per_unit_errors: bool,
    /// Rendered whole-program diagnostics (for a failing assertion's message).
    pub whole_diags: String,
    /// Rendered per-unit diagnostics (for a failing assertion's message).
    pub per_unit_diags: String,
    /// The full per-unit result (summaries + transitive hash sets).
    pub per_unit: PerUnitCheck,
}

/// Run the whole-program checker and the per-unit checker on the same multi-file program and return
/// both verdicts + rendered diagnostics. The entry is compiled by path so `import`s resolve from disk.
pub fn diff_check_multi(name: &str, files: &[(&str, &str)], entry: &str) -> DiffCheck {
    let proj = TempProject::new(name, files);
    let entry_path = proj.entry(entry);
    let entry_src = std::fs::read_to_string(&entry_path).expect("read entry");
    let entry_name = entry_path.display().to_string();

    let mut sm_whole = SourceMap::new();
    let whole = check(&mut sm_whole, &entry_name, &entry_src);

    let mut sm_per = SourceMap::new();
    let per_unit = check_per_unit(&mut sm_per, &entry_name, &entry_src);

    DiffCheck {
        whole_errors: whole.diags.has_errors(),
        per_unit_errors: per_unit.diags.has_errors(),
        whole_diags: align_driver::format_diagnostics(&sm_whole, &whole.diags),
        per_unit_diags: align_driver::format_diagnostics(&sm_per, &per_unit.diags),
        per_unit,
    }
}

/// Assert whole-program and per-unit checking agree on the accept/reject verdict for a multi-file
/// program, and return the per-unit result for further inspection.
pub fn assert_same_verdict(name: &str, files: &[(&str, &str)], entry: &str) -> PerUnitCheck {
    let d = diff_check_multi(name, files, entry);
    assert_eq!(
        d.whole_errors, d.per_unit_errors,
        "verdict mismatch for `{name}`:\n== whole-program ({}) ==\n{}\n== per-unit ({}) ==\n{}",
        d.whole_errors, d.whole_diags, d.per_unit_errors, d.per_unit_diags
    );
    d.per_unit
}

/// Run the M15 S1b per-unit checker on a multi-file program written to a fresh temp directory.
pub fn check_per_unit_multi(name: &str, files: &[(&str, &str)], entry: &str) -> PerUnitCheck {
    let proj = TempProject::new(name, files);
    let entry_path = proj.entry(entry);
    let entry_src = std::fs::read_to_string(&entry_path).expect("read entry");
    let mut sm = SourceMap::new();
    check_per_unit(&mut sm, &entry_path.display().to_string(), &entry_src)
}

/// M15 S2 per-unit build harness: write `files` to a fresh temp dir, run [`build_per_unit`], and keep
/// the project alive so the per-unit objects can be emitted, linked, run, and inspected. The temp
/// directory (and every artifact under it) is removed when the returned value is dropped.
pub struct PerUnitBuilt {
    pub walk: PerUnitWalk,
    dir: PathBuf,
    _proj: TempProject,
}

impl PerUnitBuilt {
    /// Emit each cleanly-checked unit's object into the project directory (`unit<i>.o`, unit order),
    /// returning the object paths. Uses the release profile / baseline target (the suite default).
    pub fn emit_objects(&self, rt_lto: bool) -> Vec<PathBuf> {
        self.emit_objects_with(Profile::Release, rt_lto)
    }

    /// [`emit_objects`] with an explicit profile — `Dev` (O0) keeps internal symbols (privates,
    /// consumer-side monomorphs) observable for a visibility assertion that `Release` would inline away.
    pub fn emit_objects_with(&self, profile: Profile, rt_lto: bool) -> Vec<PathBuf> {
        let mut objs = Vec::with_capacity(self.walk.units.len());
        for (i, u) in self.walk.units.iter().enumerate() {
            let obj = self.dir.join(format!("unit{i}.o"));
            emit_object_file(&u.mir, &obj, BuildTarget::Baseline, profile, &[], rt_lto)
                .unwrap_or_else(|e| panic!("codegen for unit `{}`: {e}", u.unit));
            objs.push(obj);
        }
        objs
    }

    /// The deterministic capability-library union across units (first-seen, unit order).
    pub fn link_libs_union(&self) -> Vec<String> {
        let mut libs: Vec<String> = Vec::new();
        for u in &self.walk.units {
            for l in &u.mir.link_libs {
                if !libs.contains(l) {
                    libs.push(l.clone());
                }
            }
        }
        libs
    }

    /// Emit + link the N objects into an executable, run it, and return its `Output`.
    pub fn link_and_run(&self) -> std::process::Output {
        let objs = self.emit_objects(false);
        let obj_refs: Vec<&std::path::Path> = objs.iter().map(|p| p.as_path()).collect();
        let exe = self.dir.join(format!("a{}", std::env::consts::EXE_SUFFIX));
        link_objects(&obj_refs, &exe, &self.link_libs_union(), Profile::Release).expect("link");
        std::process::Command::new(&exe).output().expect("run")
    }

    /// The artifact for the named unit.
    pub fn unit(&self, name: &str) -> &PerUnitArtifact {
        self.walk.units.iter().find(|u| u.unit == name).unwrap_or_else(|| panic!("unit `{name}` not built"))
    }
}

/// Write a multi-file program to a fresh temp dir and run the M15 S2 per-unit build over it.
pub fn build_per_unit_multi(name: &str, files: &[(&str, &str)], entry: &str) -> PerUnitBuilt {
    let proj = TempProject::new(name, files);
    let entry_path = proj.entry(entry);
    let entry_src = std::fs::read_to_string(&entry_path).expect("read entry");
    let dir = proj.dir.clone();
    let mut sm = SourceMap::new();
    let walk = build_per_unit(&mut sm, &entry_path.display().to_string(), &entry_src);
    assert!(
        !walk.diags.has_errors(),
        "unexpected per-unit errors:\n{}",
        align_driver::format_diagnostics(&sm, &walk.diags)
    );
    PerUnitBuilt { walk, dir, _proj: proj }
}

/// The defined/undefined symbols of an object file via `llvm-nm`, one `(kind, name)` per line
/// (kind is nm's single-letter type: `T`/`t` text, `U` undefined, etc.). `None` if `llvm-nm` is
/// not discoverable (the caller skips the assertion).
pub fn nm_symbols(obj: &std::path::Path) -> Option<Vec<(char, String)>> {
    let tool = align_driver::llvm_tool("llvm-nm")?;
    let out = std::process::Command::new(&tool)
        .arg(obj)
        .env("LC_ALL", "C")
        .output()
        .expect("run llvm-nm");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut syms = Vec::new();
    for line in text.lines() {
        // Format: "<addr-or-blank> <kind> <name>" — an undefined symbol has a blank address column.
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            [kind, name] if kind.len() == 1 => {
                syms.push((kind.chars().next().unwrap(), (*name).to_string()));
            }
            [_addr, kind, name] if kind.len() == 1 => {
                syms.push((kind.chars().next().unwrap(), (*name).to_string()));
            }
            _ => {}
        }
    }
    Some(syms)
}

/// Whether checking a multi-file program (`entry` + the other `files`) produces any error.
pub fn check_multi_errs(name: &str, files: &[(&str, &str)], entry: &str) -> bool {
    let proj = TempProject::new(name, files);
    let entry_path = proj.entry(entry);
    let entry_src = std::fs::read_to_string(&entry_path).expect("read entry");
    let mut sm = SourceMap::new();
    check(&mut sm, &entry_path.display().to_string(), &entry_src).diags.has_errors()
}

/// The rendered diagnostics from checking a multi-file program (`entry` + the other `files`) — the
/// multi-file counterpart of [`check_diagnostics`], for negative tests that assert the *message*
/// (e.g. a cyclic-import error naming the cycle), not just that some error occurred.
pub fn check_multi_diagnostics(name: &str, files: &[(&str, &str)], entry: &str) -> String {
    let proj = TempProject::new(name, files);
    let entry_path = proj.entry(entry);
    let entry_src = std::fs::read_to_string(&entry_path).expect("read entry");
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, &entry_path.display().to_string(), &entry_src);
    align_driver::format_diagnostics(&sm, &checked.diags)
}
