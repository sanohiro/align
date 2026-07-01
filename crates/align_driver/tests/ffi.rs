//! FFI first slice (draft.md §15): `extern "C" fn name(params) -> ret` declarations + calling them
//! from inside an `unsafe {}` block. FFI-safe signature types are integers, floats, and `raw` (an
//! opaque byte pointer); a `()` return is `void`. The C symbols resolve against the already-linked
//! libc/libm — no extra `-l` flag is needed for those. A call is confined to `unsafe {}` (foreign
//! code is outside the safe core), exactly like a `raw.*` op, and its enclosing function is impure.

mod common;
use common::*;

fn ok(src: &str) -> bool {
    let mut sm = SourceMap::new();
    !check(&mut sm, "ffi", src).diags.has_errors()
}

#[test]
fn extern_c_libc_abs_runs() {
    if !backend_available() {
        return;
    }
    // Declare libc `abs` and call it from `unsafe`. Exit 7 confirms the direct extern `call` ran and
    // resolved against libc (no extra link flag).
    let out = build_and_run(
        "ffi-abs",
        "extern \"C\" fn abs(x: i32) -> i32\n\nfn main() -> i32 {\n  unsafe {\n    return abs(-7)\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn extern_c_block_form_and_float() {
    if !backend_available() {
        return;
    }
    // The braced group form + an `f64` signature resolving against libm (`-lm` is always linked).
    // `sqrt(16.0) as i32` == 4.
    let out = build_and_run(
        "ffi-sqrt",
        "extern \"C\" {\n  fn sqrt(x: f64) -> f64\n}\n\nfn main() -> i32 {\n  unsafe {\n    r := sqrt(16.0)\n    return r as i32\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(4));
}

#[test]
fn extern_c_raw_pointer_round_trip() {
    if !backend_available() {
        return;
    }
    // A `raw` pointer threads through an extern C call: `memset(p, 65, 4)` then `raw.load` reads the
    // byte back. Exit 65 confirms the raw param + raw return interoperate with `raw.*`.
    let out = build_and_run(
        "ffi-memset",
        "extern \"C\" fn memset(p: raw, c: i32, n: i64) -> raw\n\nfn main() -> i32 {\n  unsafe {\n    p := raw.alloc(4)\n    memset(p, 65, 4)\n    x: i8 := raw.load(p, 0)\n    raw.free(p)\n    return x as i32\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(65));
}

#[test]
fn extern_call_outside_unsafe_is_rejected() {
    // A foreign call can violate every safe-core invariant, so it is only valid inside `unsafe {}`.
    assert!(!ok("extern \"C\" fn abs(x: i32) -> i32\nfn main() -> i32 {\n  return abs(-7)\n}\n"));
}

#[test]
fn extern_call_inside_unsafe_is_accepted() {
    assert!(ok("extern \"C\" fn abs(x: i32) -> i32\nfn main() -> i32 {\n  unsafe {\n    return abs(-7)\n  }\n}\n"));
}

#[test]
fn non_c_abi_is_rejected() {
    // Only `extern "C"` is supported in the first slice.
    assert!(!ok("extern \"Rust\" fn foo(x: i32) -> i32\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn non_ffi_safe_param_is_rejected() {
    // `str` (and other aggregates/owned collections) have no settled C mapping yet — deferred.
    assert!(!ok("extern \"C\" fn f(s: str) -> i32\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn non_ffi_safe_return_is_rejected() {
    assert!(!ok("extern \"C\" fn f(x: i32) -> str\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn duplicate_extern_is_rejected() {
    assert!(!ok("extern \"C\" fn abs(x: i32) -> i32\nextern \"C\" fn abs(x: i32) -> i32\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn extern_containing_fn_is_impure_so_not_a_par_map_callee() {
    // A function that calls an extern is inferred impure (it contains an `unsafe {}` block), so it can
    // never be a `par_map` callee. Here we only assert the negative surface holds: the void-return
    // form type-checks.
    assert!(ok("extern \"C\" fn srand(seed: u32)\nfn main() -> i32 {\n  unsafe {\n    srand(1)\n  }\n  return 0\n}\n"));
}
