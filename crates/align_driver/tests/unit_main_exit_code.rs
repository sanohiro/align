//! A `Unit`-returning `fn main()` must produce a **defined**, deterministic exit code (`0`) on
//! every run of the same binary — not a garbage value read from an undefined ABI return register.
//!
//! Found by the M15 S2 adversarial gate (`docs/open-questions.md` "Unit-returning `fn main()`
//! yields a nondeterministic exit code"): before the fix, a `()`-returning Align `main` WAS the C
//! entry `main` directly, declared `void` in LLVM IR — but the C ABI's `main` must return `i32`,
//! so `ret void` left the return register (`eax`/`w0`) undefined and the observed exit code varied
//! run to run (88/216/168/120/104 across five runs of the identical binary). The fix renames a
//! `Unit`-returning `main` to `align_main` (same as the existing `Result`-returning `main` split)
//! and generates a C `main` wrapper that always emits `ret i32 0` after the call.
//!
//! Both the whole-program and per-unit (`build_per_unit`/M15 S2) codegen paths share this wrapper
//! logic, so both are pinned here with a same-binary, run-N-times determinism check.

mod common;
use common::*;

use std::process::Command;

const UNIT_MAIN: &str = "fn main() {\n  x := 1\n  print(x)\n}\n";

#[test]
fn whole_program_unit_main_exit_code_is_deterministic_across_runs() {
    if !backend_available() {
        eprintln!("skip: LLVM backend not wired");
        return;
    }
    let built = build_exe("unit-main-exit-wp", UNIT_MAIN);
    for run in 0..5 {
        let status = Command::new(&built.exe).status().expect("run");
        assert_eq!(status.code(), Some(0), "run {run}: Unit main must exit 0, got {status:?}");
    }
}

#[test]
fn per_unit_unit_main_exit_code_is_deterministic_across_runs() {
    if !backend_available() {
        eprintln!("skip: LLVM backend not wired");
        return;
    }
    let built = build_per_unit_multi("unit-main-exit-pu", &[("main.align", UNIT_MAIN)], "main.align");
    let objs = built.emit_objects(false);
    let obj_refs: Vec<&std::path::Path> = objs.iter().map(|p| p.as_path()).collect();
    let exe = built.dir.join(format!("a{}", std::env::consts::EXE_SUFFIX));
    link_objects(&obj_refs, &exe, &built.link_libs_union(), Profile::Release).expect("link");
    for run in 0..5 {
        let status = Command::new(&exe).status().expect("run");
        assert_eq!(status.code(), Some(0), "run {run}: per-unit Unit main must exit 0, got {status:?}");
    }
}
