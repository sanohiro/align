//! The LLVM middle-end optimization pipeline (`-O2`) is run before object emission, so the lifted
//! lambdas and fused `map`/`where`/`reduce` loops are inlined and vectorized. These tests assert
//! the optimized output is still **correct** — a fused pipeline must compute the same result after
//! the inliner / LICM / vectorizer run on it (a miscompile from latent IR UB would surface here).
//! (Vectorization itself is target-dependent; it is verified out-of-band via `objdump`.)

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
    let out = std::process::Command::new(&exe).output().expect("run");
    let _ = std::fs::remove_file(&obj);
    let _ = std::fs::remove_file(&exe);
    out
}

#[test]
fn fused_map_sum_correct_under_o2() {
    if !backend_available() {
        return;
    }
    // `xs.map(dbl).sum()` fuses to one loop that the optimizer inlines + vectorizes;
    // the result must still be 2*(1+..+8) = 72.
    let src = concat!(
        "fn dbl(x: i64) -> i64 = x * 2\n",
        "fn run(xs: slice<i64>) -> i64 = xs.map(dbl).sum()\n",
        "fn main() -> i32 {\n",
        "  a := [1, 2, 3, 4, 5, 6, 7, 8]\n",
        "  return run(a)\n",
        "}\n",
    );
    let out = build_and_run("opt-map-sum", src);
    assert_eq!(out.status.code(), Some(72));
}

#[test]
fn fused_map_where_sum_correct_under_o2() {
    if !backend_available() {
        return;
    }
    // map + where + sum fused into one loop: keep the doubled values that are > 6, then sum.
    // doubled = 2,4,6,8,10,12; kept (>6) = 8,10,12 → 30.
    let src = concat!(
        "fn dbl(x: i64) -> i64 = x * 2\n",
        "fn big(x: i64) -> bool = x > 6\n",
        "fn run(xs: slice<i64>) -> i64 = xs.map(dbl).where(big).sum()\n",
        "fn main() -> i32 {\n",
        "  a := [1, 2, 3, 4, 5, 6]\n",
        "  return run(a)\n",
        "}\n",
    );
    let out = build_and_run("opt-map-where-sum", src);
    assert_eq!(out.status.code(), Some(30));
}
