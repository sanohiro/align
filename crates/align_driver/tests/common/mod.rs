//! Shared test harness for the `align_driver` integration tests. Each test file does
//! `mod common; use common::*;` and calls these helpers (and the re-exported driver API).
//!
//! `dead_code` / `unused_imports` are allowed because each test binary includes this whole module
//! but uses only the subset of helpers / re-exports it needs.
#![allow(dead_code, unused_imports)]

pub use align_driver::{
    backend_available, check, emit_llvm_ir, emit_object_file, link_executable, lower_to_mir,
};
pub use align_span::SourceMap;

use std::path::PathBuf;

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
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    // Include the process id so two concurrent test-suite runs on one machine (e.g. parallel CI)
    // don't collide on these temp paths.
    let pid = std::process::id();
    let obj = dir.join(format!("align-test-{pid}-{name}.o"));
    let exe = dir.join(format!("align-test-{pid}-{name}{}", std::env::consts::EXE_SUFFIX));
    let _artifacts = TempArtifacts { obj: obj.clone(), exe: exe.clone() };
    emit_object_file(&mir, &obj).expect("codegen");
    link_executable(&obj, &exe).expect("link");
    std::process::Command::new(&exe).args(prog_args).output().expect("run")
}

/// Whether checking `src` produces any error (for negative tests).
pub fn check_errs(name: &str, src: &str) -> bool {
    let mut sm = SourceMap::new();
    check(&mut sm, name, src).diags.has_errors()
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
    emit_object_file(&mir, &obj).expect("codegen");
    link_executable(&obj, &exe).expect("link");
    std::process::Command::new(&exe).output().expect("run")
}

/// Whether checking a multi-file program (`entry` + the other `files`) produces any error.
pub fn check_multi_errs(name: &str, files: &[(&str, &str)], entry: &str) -> bool {
    let proj = TempProject::new(name, files);
    let entry_path = proj.entry(entry);
    let entry_src = std::fs::read_to_string(&entry_path).expect("read entry");
    let mut sm = SourceMap::new();
    check(&mut sm, &entry_path.display().to_string(), &entry_src).diags.has_errors()
}
