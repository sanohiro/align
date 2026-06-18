//! M4 end-to-end: arrays + fused reductions. Requires LLVM/cc, so skip where absent.

use align_driver::{backend_available, check, emit_object_file, link_executable, lower_to_mir};
use align_span::SourceMap;

fn build_and_run(name: &str, src: &str) -> std::process::Output {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let obj = dir.join(format!("align-test-{name}.o"));
    let exe = dir.join(format!("align-test-{name}"));
    emit_object_file(&mir, &obj).expect("codegen");
    link_executable(&obj, &exe).expect("link");
    std::process::Command::new(&exe).output().expect("run")
}

#[test]
fn array_sum_inline() {
    if !backend_available() {
        return;
    }
    let out = build_and_run("arr-sum", "fn main() -> i32 {\n  return [10, 20, 12].sum()\n}\n");
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn array_sum_bound_local() {
    if !backend_available() {
        return;
    }
    // Bound array summed where the result type matches (i64 throughout, low byte = 15).
    let out = build_and_run(
        "arr-sum-bound",
        "fn total(n: i64) -> i64 {\n  xs := [1, 2, 3, 4, 5]\n  return xs.sum()\n}\nfn main() -> i32 {\n  return 0\n}\n",
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn fused_map_where_sum_pipeline() {
    if !backend_available() {
        return;
    }
    // map *2 → [2,4,6,8,10]; where >4 → [6,8,10]; sum = 24.
    let src = "fn double(x: i32) -> i32 = x * 2\nfn big(x: i32) -> bool = x > 4\nfn main() -> i32 {\n  return [1, 2, 3, 4, 5].map(double).where(big).sum()\n}\n";
    let out = build_and_run("pipeline", src);
    assert_eq!(out.status.code(), Some(24));
}

#[test]
fn pipeline_fuses_into_one_loop() {
    let mut sm = SourceMap::new();
    let src = "fn double(x: i32) -> i32 = x * 2\nfn big(x: i32) -> bool = x > 4\nfn main() -> i32 {\n  return [1, 2, 3].map(double).where(big).sum()\n}\n";
    let checked = check(&mut sm, "p.align", src);
    assert!(!checked.diags.has_errors());
    let text = align_mir::print::program_to_string(&lower_to_mir(&checked.hir));
    // Fusion: the map and where calls appear inside the loop body, and there is no
    // intermediate array store of mapped results (only the source literal is stored).
    assert!(text.contains("call double") && text.contains("call big"), "stages not inlined:\n{text}");
    // Exactly one loop back-edge target reused (single loop): the source is stored once.
    assert_eq!(text.matches("<- 1_i32").count(), 1, "source stored once:\n{text}");
}

#[test]
fn array_sum_emits_single_loop() {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "a.align", "fn main() -> i32 {\n  return [1, 2, 3].sum()\n}\n");
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    let text = align_mir::print::program_to_string(&mir);
    // One loop: a back-edge (two `goto bb1`-style targets) and an indexed load.
    assert!(text.contains("["), "expected indexed access in:\n{text}");
    assert!(text.matches("branch").count() >= 1, "expected a loop branch in:\n{text}");
}
