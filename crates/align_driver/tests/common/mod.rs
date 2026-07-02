//! Shared test harness for the `align_driver` integration tests. Each test file does
//! `mod common; use common::*;` and calls these helpers (and the re-exported driver API).
//!
//! `dead_code` / `unused_imports` are allowed because each test binary includes this whole module
//! but uses only the subset of helpers / re-exports it needs.
#![allow(dead_code, unused_imports)]

pub use align_driver::{
    backend_available, check, emit_llvm_ir, emit_object_file, link_executable, link_objects,
    lower_to_mir, BuildTarget,
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
    emit_object_file(&mir, &a_obj, BuildTarget::Baseline).expect("codegen");
    std::fs::write(&c_src_path, c_src).expect("write c helper");
    let cc_status = std::process::Command::new("cc")
        .args(["-c", "-O0"])
        .arg(&c_src_path)
        .arg("-o")
        .arg(&c_obj)
        .status()
        .expect("launch cc");
    assert!(cc_status.success(), "compiling the C helper failed");
    link_objects(&[&a_obj, &c_obj], &exe, &mir.link_libs).expect("link");
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
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    // Include the process id so two concurrent test-suite runs on one machine (e.g. parallel CI)
    // don't collide on these temp paths.
    let pid = std::process::id();
    let obj = dir.join(format!("align-test-{pid}-{name}.o"));
    let exe = dir.join(format!("align-test-{pid}-{name}{}", std::env::consts::EXE_SUFFIX));
    let _artifacts = TempArtifacts { obj: obj.clone(), exe: exe.clone() };
    emit_object_file(&mir, &obj, BuildTarget::Baseline).expect("codegen");
    link_executable(&obj, &exe, &mir.link_libs).expect("link");
    std::process::Command::new(&exe).args(prog_args).output().expect("run")
}

/// The LLVM IR text for `src` (for asserting on the generated instructions).
pub fn emit_llvm(src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "ir", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    align_driver::emit_llvm_ir(&mir, BuildTarget::Baseline).expect("emit llvm ir")
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
    emit_object_file(&mir, &obj, BuildTarget::Baseline).expect("codegen");
    link_executable(&obj, &exe, &mir.link_libs).expect("link");
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
