//! M0 end-to-end: verify that `.align` source flows through
//! lexer -> parser -> sema -> MIR and produces the expected MIR
//! (`docs/impl/07-roadmap.md` M0).
//! Since the LLVM stage is not wired yet, verify at MIR, the current vertical slice's end.

use align_driver::{check, lower_to_mir};
use align_span::SourceMap;

const M0: &str = "fn main() -> i32 {\n  x := 1\n  return x\n}\n";

#[test]
fn m0_pipeline_to_mir() {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "m0.align", M0);
    assert!(
        !checked.diags.has_errors(),
        "the M0 program produced an error"
    );

    let mir = lower_to_mir(&checked.hir);
    let text = align_mir::print::program_to_string(&mir);

    // x := 1 is bound as an i32 constant, and return returns that value.
    assert!(text.contains("fn main() -> i32"), "got:\n{text}");
    assert!(text.contains("1_i32"), "no i32 constant:\n{text}");
    assert!(text.contains("return %"), "no value return:\n{text}");
}

#[test]
fn m0_compiles_and_runs_with_exit_code() {
    // M0 completion criterion: the `alignc run` equivalent emits an executable and returns an exit code.
    // Requires LLVM/cc, so skip in environments without them.
    if !align_driver::backend_available() {
        eprintln!("skip: LLVM backend not wired");
        return;
    }
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "m0.align", M0);
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);

    let dir = std::env::temp_dir();
    let obj = dir.join("align-test-m0.o");
    let exe = dir.join("align-test-m0");
    align_driver::emit_object_file(&mir, &obj, align_driver::BuildTarget::Baseline, align_driver::Profile::Release).expect("codegen");
    align_driver::link_executable(&obj, &exe, &mir.link_libs, align_driver::Profile::Release).expect("link");

    let status = std::process::Command::new(&exe).status().expect("run");
    assert_eq!(status.code(), Some(1), "main returns x:=1, so exit code 1");
}

#[test]
fn type_mismatch_is_reported() {
    // Returning an integer from a function that returns () is an error.
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "bad.align", "fn f() {\n  return 1\n}\n");
    assert!(checked.diags.has_errors());
}
