//! The `align(N) data := […]` over-alignment binding form over a scalar array (M6) — the
//! aligned-vector-load enabler. The prefix over-aligns the array's stack storage; a `vecN<T>` load
//! of a whole borrow of the binding at a provably `N`-aligned offset is emitted as an *aligned*
//! load, while every other offset (and any cross-function `slice<T>`) stays the always-safe
//! element-aligned load. See `draft.md` §9 / `docs/open-questions.md` "align(N)".

mod common;
use common::*;

#[test]
fn an_aligned_binding_compiles_and_runs() {
    if !backend_available() {
        return;
    }
    // Two vec loads of an `align(64)` array: `load(0)` (aligned) + `load(4)` (element-aligned).
    let out = build_and_run("aligned-bind-run", ALIGNED_LOAD);
    assert_eq!(out.status.code(), Some(36));
}

/// `align(64) xs := [1..8]`; `a = xs[..].load(0)` (offset 0), `b = xs[..].load(4)` (offset 32).
const ALIGNED_LOAD: &str = concat!(
    "fn main() -> i32 {\n",
    "  align(64) xs := [1, 2, 3, 4, 5, 6, 7, 8]\n",
    "  a: vec4<i64> := xs[..].load(0)\n",
    "  b: vec4<i64> := xs[..].load(4)\n",
    "  s := a + b\n",
    "  return (s[0] + s[1] + s[2] + s[3]) as i32\n",
    "}\n",
);

#[test]
fn the_binding_slot_is_over_aligned() {
    // The scalar-array alloca picks up the declared alignment (over `[8 x i64]`'s natural 8).
    let ir = emit_llvm(ALIGNED_LOAD);
    assert!(
        ir.contains("alloca [8 x i64], align 64"),
        "expected an over-aligned array slot, got:\n{ir}"
    );
}

/// The `.load(i)` vector loads (which read through a `%vloadgep*` element pointer — distinct from
/// the ordinary `%_slot` reloads LLVM inserts) whose alignment is exactly `align {n}`. Filtering on
/// the GEP operand is robust against LLVM's auto-numbered SSA names.
fn vec_gep_loads_with_align(ir: &str, n: u32) -> usize {
    ir.lines()
        .filter(|l| l.contains("load <4 x i64>, ptr %vloadgep") && l.ends_with(&format!("align {n}")))
        .count()
}

fn vec_gep_loads(ir: &str) -> usize {
    ir.lines().filter(|l| l.contains("load <4 x i64>, ptr %vloadgep")).count()
}

#[test]
fn exactly_one_load_is_over_aligned_the_other_stays_element_aligned() {
    // `load(0)` from the 64-aligned base is provably 64-aligned → one `align 64` vector load.
    // `load(4)` sits at byte offset 32, not a multiple of 64 → the conservative element alignment
    // (8), never a wrong over-alignment (which would be UB). Two `.load()` sites, one of each.
    let ir = emit_llvm(ALIGNED_LOAD);
    assert_eq!(vec_gep_loads(&ir), 2, "expected exactly two `.load` vector loads, got:\n{ir}");
    assert_eq!(
        vec_gep_loads_with_align(&ir, 64),
        1,
        "expected exactly one aligned (align 64) vector load, got:\n{ir}"
    );
    assert_eq!(
        vec_gep_loads_with_align(&ir, 8),
        1,
        "expected exactly one element-aligned (align 8) fallback vector load, got:\n{ir}"
    );
}

#[test]
fn a_runtime_index_stays_element_aligned() {
    if !backend_available() {
        return;
    }
    // A non-constant load index can't be proven `N`-aligned → element-aligned (correct at runtime).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  align(64) xs := [1, 2, 3, 4, 5, 6, 7, 8]\n",
        "  mut i := 0\n",
        "  v: vec4<i64> := xs[..].load(i)\n",
        "  return (v[0] + v[3]) as i32\n", // 1 + 4 = 5
        "}\n",
    );
    let ir = emit_llvm(src);
    // No `align 64` vector load — the index is a runtime value, so the offset can't be proven.
    assert_eq!(
        vec_gep_loads_with_align(&ir, 64),
        0,
        "a runtime index must not get an aligned load:\n{ir}"
    );
    let out = build_and_run("aligned-bind-dyn", src);
    assert_eq!(out.status.code(), Some(5));
}

#[test]
fn no_align_binding_is_element_aligned_regression() {
    // Without the prefix, nothing changes: the slot is naturally aligned and the load is element-
    // aligned (the pre-existing behavior).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  xs := [1, 2, 3, 4, 5, 6, 7, 8]\n",
        "  v: vec4<i64> := xs[..].load(0)\n",
        "  return (v[0] + v[3]) as i32\n",
        "}\n",
    );
    let ir = emit_llvm(src);
    assert!(
        ir.contains("alloca [8 x i64], align 8"),
        "an un-annotated array slot stays naturally aligned:\n{ir}"
    );
    assert!(
        !ir.contains("align 64"),
        "no over-alignment without the prefix:\n{ir}"
    );
}

#[test]
fn align_on_a_non_array_binding_is_rejected() {
    // The binding form applies to a scalar fixed array only — a scalar target is a clean error.
    let src = "fn main() -> i32 {\n  align(64) x := 5\n  return x\n}\n";
    assert!(check_errs("aligned-bind-scalar", src));
}

#[test]
fn a_non_power_of_two_binding_alignment_is_rejected() {
    let src = "fn main() -> i32 {\n  align(24) xs := [1, 2, 3, 4]\n  return 0\n}\n";
    assert!(check_errs("aligned-bind-pow2", src));
}
