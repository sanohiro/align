//! doc-13 §6.6 — the `ALIGN_NEEDLE_HOIST=off` measurement/regression toggle (finding 4).
//!
//! This test sets a **process-global** env var around an in-process compile, so it lives in its OWN
//! test binary (this is the only test here) — never in the shared `needle_plan_hoist` binary, whose
//! other tests compile in parallel and would race on the env. The toggle is read at MIR lowering, so
//! the two shapes are cache-distinct and `CacheContext::from_env` force-disables the object cache
//! when it is set (mirroring `ALIGN_SORT_ADAPTIVE`).

mod common;
use common::*;

#[test]
fn hoist_toggle_flips_finder_vs_str_contains() {
    if !backend_available() {
        return;
    }
    let src = r#"
fn main() -> i32 {
  n := "al"
  xs := ["alpha", "beta", "alfalfa"]
  return xs.where(fn s { s.contains(n) }).count() as i32
}
"#;
    // Default (toggle on): one hoisted finder plan, no per-call str_contains.
    let on = emit_llvm(src);
    assert_eq!(on.matches("call ptr @align_rt_str_finder_new(").count(), 1, "on: one plan:\n{on}");
    assert!(on.contains("call i64 @align_rt_str_finder_find("), "on: finder_find in body:\n{on}");
    assert!(!on.contains("call i32 @align_rt_str_contains("), "on: no str_contains:\n{on}");

    // Toggle off: the SAME recognised pipeline lowers to per-call str_contains with no hoisted plan.
    unsafe { std::env::set_var("ALIGN_NEEDLE_HOIST", "off") };
    let off = emit_llvm(src);
    unsafe { std::env::remove_var("ALIGN_NEEDLE_HOIST") };
    assert_eq!(off.matches("call ptr @align_rt_str_finder_new(").count(), 0, "off: no hoisted plan:\n{off}");
    assert!(!off.contains("call i64 @align_rt_str_finder_find("), "off: no finder_find:\n{off}");
    assert!(off.contains("call i32 @align_rt_str_contains("), "off: per-element str_contains:\n{off}");
}
