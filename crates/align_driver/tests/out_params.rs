//! Mutable element writes `place[i] = v` (a `mut` array local or an `out` slice parameter) and
//! `out` parameters — a writable output buffer the callee fills (the write mechanism; the
//! no-alias / `noalias` optimization is a follow-up). Stores are bounds-checked.


mod common;
use common::*;

#[test]
fn mut_local_array_element_write() {
    if !backend_available() {
        return;
    }
    // `mut a := [...]; a[1] = 32` — write an element of a mutable local array. 10 + 32 == 42.
    let src = "fn main() -> i32 {\n  mut a := [10, 0, 0]\n  a[1] = 32\n  if a[0] + a[1] == 42 { return 42 }\n  return 0\n}\n";
    let out = build_and_run("w-mut-local", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn out_slice_param_write() {
    if !backend_available() {
        return;
    }
    // A callee fills an `out slice` buffer; the caller (which passed its array) sees the writes.
    let src = "fn put(out dst: slice<i64>) {\n  dst[0] = 10\n  dst[1] = 32\n}\nfn main() -> i32 {\n  mut a := [0, 0, 0]\n  put(a)\n  if a[0] + a[1] == 42 { return 42 }\n  return 0\n}\n";
    let out = build_and_run("w-out", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn out_write_computed_index() {
    if !backend_available() {
        return;
    }
    // A computed (non-constant) subscript on the write side.
    let src = "fn put(out dst: slice<i64>, k: i64) {\n  dst[k + 1] = 42\n}\nfn main() -> i32 {\n  mut a := [0, 0, 0]\n  put(a, 1)\n  return a[2] as i32\n}\n";
    let out = build_and_run("w-out-idx", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn out_write_out_of_bounds_aborts() {
    if !backend_available() {
        return;
    }
    // An out-of-range element store aborts (the bounds check fires), like an out-of-range read.
    let src = "fn put(out dst: slice<i64>) {\n  dst[9] = 1\n}\nfn main() -> i32 {\n  mut a := [0, 0]\n  put(a)\n  return 0\n}\n";
    let out = build_and_run("w-oob", src);
    assert_ne!(out.status.code(), Some(0), "an out-of-bounds store must abort");
}

// --- diagnostics ---

#[test]
fn write_to_immutable_array_rejected() {
    assert!(check_errs("w-immut", "fn main() -> i32 {\n  a := [1, 2, 3]\n  a[0] = 9\n  return 0\n}\n"));
}

#[test]
fn out_arg_aliasing_another_arg_rejected() {
    // The no-alias guarantee: an `out` argument must not name the same local as another argument.
    let src = "fn fill(src: slice<i64>, out dst: slice<i64>) {\n  dst[0] = src[0]\n}\nfn main() -> i32 {\n  mut a := [1, 2, 3]\n  fill(a, a)\n  return 0\n}\n";
    assert!(check_errs("na-alias", src));
}

#[test]
fn out_arg_aliasing_via_slice_variable_rejected() {
    // Soundness: even via different locals, aliasing the same buffer must be caught — `s` views
    // `a`, so `fill(a, s)` aliases. (Slice provenance is tracked to the root array.)
    let src = "fn fill(src: slice<i64>, out dst: slice<i64>) {\n  dst[0] = src[0]\n}\nfn main() -> i32 {\n  mut a := [1, 2, 3]\n  s : slice<i64> := a\n  fill(a, s)\n  return 0\n}\n";
    assert!(check_errs("na-alias-slice", src));
}

#[test]
fn out_arg_two_slices_of_same_array_rejected() {
    // Two slice variables borrowing the same array alias each other.
    let src = "fn fill(src: slice<i64>, out dst: slice<i64>) {\n  dst[0] = src[0]\n}\nfn main() -> i32 {\n  mut a := [1, 2, 3]\n  s1 : slice<i64> := a\n  s2 : slice<i64> := a\n  fill(s1, s2)\n  return 0\n}\n";
    assert!(check_errs("na-alias-two-slices", src));
}

#[test]
fn out_arg_inline_subslice_of_out_array_rejected() {
    // Soundness (SubSlice hole): an inline sub-slice argument `xs[0..2]` shares `xs`'s root buffer,
    // so passing it alongside `xs` as the `out` argument aliases — must be rejected.
    let src = "fn fill(out dst: slice<i64>, src: slice<i64>) {\n  dst[0] = src[0]\n}\nfn main() -> i32 {\n  mut xs := [1, 2, 3, 4]\n  fill(xs, xs[0..2])\n  return 0\n}\n";
    assert!(check_errs("na-inline-subslice", src));
}

#[test]
fn out_arg_two_overlapping_subslice_vars_rejected() {
    // Two sub-slice bindings of the same array (overlapping ranges) alias — must be rejected.
    let src = "fn fill(out dst: slice<i64>, src: slice<i64>) {\n  dst[0] = src[0]\n}\nfn main() -> i32 {\n  mut xs := [1, 2, 3, 4]\n  s1 := xs[0..2]\n  s2 := xs[1..3]\n  fill(s1, s2)\n  return 0\n}\n";
    assert!(check_errs("na-two-subslice-vars", src));
}

#[test]
fn out_arg_nested_subslice_rejected() {
    // A nested sub-slice `xs[0..4][1..2]` still roots at `xs`; aliasing `xs` as `out` is rejected.
    let src = "fn fill(out dst: slice<i64>, src: slice<i64>) {\n  dst[0] = src[0]\n}\nfn main() -> i32 {\n  mut xs := [1, 2, 3, 4, 5]\n  fill(xs, xs[0..4][1..2])\n  return 0\n}\n";
    assert!(check_errs("na-nested-subslice", src));
}

#[test]
fn out_arg_subslices_of_distinct_arrays_ok() {
    if !backend_available() {
        return;
    }
    // Sub-slices of *different* arrays are genuinely distinct buffers — must pass. dst[0]=src[0]=7.
    let src = "fn fill(out dst: slice<i64>, src: slice<i64>) {\n  dst[0] = src[0]\n}\nfn main() -> i32 {\n  xs := [7, 2, 3, 4]\n  mut ys := [0, 0, 0, 0]\n  fill(ys[0..2], xs[0..2])\n  return ys[0] as i32\n}\n";
    let out = build_and_run("na-distinct-subslices", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn out_arg_distinct_ok() {
    if !backend_available() {
        return;
    }
    // Distinct buffers satisfy the no-alias rule and run. dst[0] = src[0] = 7.
    let src = "fn fill(src: slice<i64>, out dst: slice<i64>) {\n  dst[0] = src[0]\n}\nfn main() -> i32 {\n  xs := [7, 2, 3]\n  mut ys := [0, 0, 0]\n  fill(xs, ys)\n  return ys[0] as i32\n}\n";
    let out = build_and_run("na-distinct", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn element_write_after_move_rejected() {
    // Writing an element of an owned array that was moved away is a use-after-move (it would
    // write through a nulled slot) — must be a compile error, not a runtime fault.
    let src = "fn main() -> Result<(), Error> {\n  mut ys := [1, 2, 3].to_array()\n  zs := ys\n  ys[0] = 4\n  print(zs.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("w-uam", src));
}

#[test]
fn out_on_non_slice_rejected() {
    assert!(check_errs("w-out-nonslice", "fn f(out x: i32) -> i32 = x\nfn main() -> i32 = 0\n"));
}
