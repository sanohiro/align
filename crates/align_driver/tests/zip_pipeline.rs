//! Lazy multi-source `zip`: equal-length Copy-scalar arrays/slices become one per-index SSA tuple
//! inside the existing fused pipeline. No tuple array is materialized, and `map_into` proves its
//! destination disjoint from every source without claiming the sources are mutually disjoint.

mod common;
use common::*;

fn mir_text(src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "zip-pipeline.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    align_mir::print::program_to_string(&lower_to_mir(&checked.hir))
}

fn optimized_llvm(src: &str, exports: &[&str]) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "zip-pipeline-ir.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let exports = exports.iter().map(|name| (*name).to_string()).collect::<Vec<_>>();
    emit_llvm_ir(&lower_to_mir(&checked.hir), BuildTarget::Baseline, true, &exports, false).expect("optimized LLVM IR")
}

fn function<'a>(mir: &'a str, name: &str) -> &'a str {
    let marker = format!("fn {name}");
    let start = mir.find(&marker).unwrap_or_else(|| panic!("missing {marker} in MIR:\n{mir}"));
    let body = &mir[start..];
    let end = body.find("\n}").map_or(body.len(), |i| i + 2);
    &body[..end]
}

#[test]
fn three_sources_map_into_without_materialization() {
    let src = r#"
fn fill(a: slice<i64>, b: slice<i64>, c: slice<i64>, out dst: slice<i64>) {
  zip(a, b, c).map(fn v { v.0 + v.1 * v.2 }).map_into(dst)
}
fn main() -> i32 {
  a := [1, 2, 3, 4]
  b := [2, 3, 4, 5]
  c := [10, 10, 10, 10]
  mut out := [0, 0, 0, 0]
  mut dst: slice<i64> := out
  fill(a, b, c, dst)
  return out.sum() as i32
}
"#;
    let mir = mir_text(src);
    let fill = function(&mir, "fill");
    assert!(fill.contains(" = tuple#"), "zip should assemble only an SSA tuple:\n{fill}");
    assert_eq!(fill.matches(" < ").count(), 1, "zip must lower to one counted loop:\n{fill}");
    assert!(!fill.contains("heap_alloc"), "zip itself must not allocate:\n{fill}");
    if backend_available() {
        assert_eq!(build_and_run("zip-map-into", src).status.code(), Some(150));
        let ir = optimized_llvm(
            "fn fill(a: slice<f32>, b: slice<f32>, c: slice<f32>, out dst: slice<f32>) {\n  zip(a, b, c).map(fn v { v.0 + v.1 * v.2 }).map_into(dst)\n}\nfn main() -> i32 = 0\n",
            &["fill"],
        );
        assert!(ir.contains("vector.body"), "the canonical three-source loop should vectorize:\n{ir}");
        assert!(!ir.contains("call ptr @align_rt_alloc"), "zip must not allocate tuple storage:\n{ir}");
    }
}

#[test]
fn reducers_and_guarded_stages_reuse_pipeline_semantics() {
    let src = r#"
fn main() -> i32 {
  nums := [6, 99, 8]
  dens := [2, 0, 4]
  total := zip(nums, dens)
    .where(fn v { v.1 != 0 })
    .map(fn v { v.0 / v.1 })
    .sum()
  return total as i32
}
"#;
    let mir = mir_text(src);
    let main = function(&mir, "main");
    assert_eq!(main.matches(" < ").count(), 1, "reduction must stay in one loop:\n{main}");
    assert!(!main.contains("heap_alloc"), "zip reduction must be allocation-free:\n{main}");
    if backend_available() {
        assert_eq!(build_and_run("zip-guarded-reduce", src).status.code(), Some(5));
    }
}

#[test]
fn runtime_source_length_mismatch_aborts_before_iteration() {
    if !backend_available() {
        return;
    }
    let src = r#"
fn run(a: slice<i64>, b: slice<i64>) -> i64 = zip(a, b).map(fn v { v.0 + v.1 }).sum()
fn main() -> i32 {
  a := [1, 2, 3]
  b := [4, 5]
  return run(a, b) as i32
}
"#;
    assert_ne!(build_and_run("zip-length-mismatch", src).status.code(), Some(0));
}

#[test]
fn source_inputs_may_alias_but_destination_may_not_alias_any_source() {
    let safe = r#"
fn fill(a: slice<i64>, out dst: slice<i64>) {
  zip(a, a).map(fn v { v.0 + v.1 }).map_into(dst)
}
fn main() -> i32 {
  a := [1, 2, 3]
  mut out := [0, 0, 0]
  mut dst: slice<i64> := out
  fill(a, dst)
  return out.sum() as i32
}
"#;
    let ir = emit_llvm(safe);
    assert!(ir.contains("align.in.fill.mapinto"), "runtime sources need the shared input scope:\n{ir}");
    assert!(!ir.contains("align.in1"), "sources must not receive mutually-disjoint scopes:\n{ir}");
    if backend_available() {
        assert_eq!(build_and_run("zip-source-alias", safe).status.code(), Some(12));
    }

    let bad = r#"
fn main() -> i32 {
  mut a := [1, 2, 3]
  b := [4, 5, 6]
  sa: slice<i64> := a
  sb: slice<i64> := b
  mut dst: slice<i64> := a
  zip(sa, sb).map(fn v { v.0 + v.1 }).map_into(dst)
  return 0
}
"#;
    assert!(check_errs("zip-destination-alias", bad));
}

#[test]
fn invalid_zip_surfaces_are_rejected_cleanly() {
    let cases = [
        ("arity", "fn main() -> i32 = zip([1, 2]).map(fn v { v.0 }).sum() as i32\n"),
        ("standalone", "fn main() -> i32 { zip([1], [2]) return 0 }\n"),
        (
            "fixed-length",
            "fn main() -> i32 = zip([1, 2], [3]).map(fn v { v.0 + v.1 }).sum() as i32\n",
        ),
    ];
    for (name, src) in cases {
        assert!(check_errs(name, src), "{name} should be rejected");
    }
}
