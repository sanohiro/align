//! `map_into(out dst)` — the materializing pipeline terminal that writes each post-stage element
//! into a caller-provided writable slice instead of allocating a fresh buffer (the `to_array`
//! sibling). Covers the runtime behaviour (correct writes, the `dst.len() == src.len()` guard), the
//! alias soundness gate (`dst` must be a distinct, known buffer — the precondition for the
//! scoped-`noalias` metadata), and that the metadata is actually emitted on the fused loop.

mod common;
use common::*;

// --- runtime behaviour ---

#[test]
fn map_into_out_param_writes_result() {
    if !backend_available() {
        return;
    }
    // The canonical shape (draft.md §7): a callee fills an `out` slice from a source slice. The
    // caller sees the writes. dbl over [1,2,3,4] → [2,4,6,8], sum 20.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn scale(src: slice<i64>, out dst: slice<i64>) {\n  src.map(dbl).map_into(dst)\n}\nfn main() -> i32 {\n  xs := [1, 2, 3, 4]\n  mut ys := [0, 0, 0, 0]\n  mut d : slice<i64> := ys\n  scale(xs, d)\n  return ys.sum() as i32\n}\n";
    let out = build_and_run("mi-out", src);
    assert_eq!(out.status.code(), Some(20));
}

#[test]
fn map_into_no_stages_is_a_copy() {
    if !backend_available() {
        return;
    }
    // A stageless `map_into` copies the source elements verbatim. 5+6+7 = 18.
    let src = "fn main() -> i32 {\n  xs := [5, 6, 7]\n  mut ys := [0, 0, 0]\n  mut d : slice<i64> := ys\n  xs.map_into(d)\n  return ys.sum() as i32\n}\n";
    let out = build_and_run("mi-copy", src);
    assert_eq!(out.status.code(), Some(18));
}

#[test]
fn map_into_field_projection_source() {
    if !backend_available() {
        return;
    }
    // A struct-array projection (`.pay`) feeds `map_into`. 10+20 = 30.
    let src = "U { age: i64, pay: i64 }\nfn main() -> i32 {\n  us := [U{age: 1, pay: 10}, U{age: 2, pay: 20}]\n  mut ys := [0, 0]\n  mut d : slice<i64> := ys\n  us.pay.map_into(d)\n  return ys.sum() as i32\n}\n";
    let out = build_and_run("mi-proj", src);
    assert_eq!(out.status.code(), Some(30));
}

#[test]
fn map_into_fixed_array_literal_source() {
    if !backend_available() {
        return;
    }
    // An inline array-literal source is fresh stack storage — allowed (disjoint from any caller
    // slice) and lowered without the slice-load metadata. [1,2,3,4]*2 → sum 20.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn fill(out d: slice<i64>) {\n  [1, 2, 3, 4].map(dbl).map_into(d)\n}\nfn main() -> i32 {\n  mut ys := [0, 0, 0, 0]\n  mut d : slice<i64> := ys\n  fill(d)\n  return ys.sum() as i32\n}\n";
    let out = build_and_run("mi-lit", src);
    assert_eq!(out.status.code(), Some(20));
}

#[test]
fn map_into_length_mismatch_aborts() {
    if !backend_available() {
        return;
    }
    // `dst.len() != src.len()` aborts (the length guard fires), like an out-of-bounds store.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn scale(src: slice<i64>, out dst: slice<i64>) {\n  src.map(dbl).map_into(dst)\n}\nfn main() -> i32 {\n  xs := [1, 2, 3, 4]\n  mut ys := [0, 0]\n  mut d : slice<i64> := ys\n  scale(xs, d)\n  return 0\n}\n";
    let out = build_and_run("mi-lenmismatch", src);
    assert_ne!(out.status.code(), Some(0), "a length mismatch must abort");
}

// --- scoped-`noalias` metadata (the disjoint-buffer claim) ---

#[test]
fn map_into_emits_scoped_noalias_metadata() {
    // The fused loop's source load and `dst` store carry the loop's disjoint `in`/`out` alias
    // scopes — the metadata that lets the vectorizer drop its runtime overlap guard at -O2.
    let ir = emit_llvm("fn dbl(x: i64) -> i64 = x * 2\nfn scale(src: slice<i64>, out dst: slice<i64>) {\n  src.map(dbl).map_into(dst)\n}\nfn main() -> i32 = 0\n");
    // The source load claims the `in` scope, no-alias vs `out`; the store the reverse.
    assert!(ir.contains("!alias.scope"), "want alias.scope metadata:\n{ir}");
    assert!(ir.contains("!noalias"), "want noalias metadata:\n{ir}");
    // The scope nodes are named per (function, loop) so distinct loops never collide.
    assert!(ir.contains("align.in.scale.mapinto"), "want an `in` scope node:\n{ir}");
    assert!(ir.contains("align.out.scale.mapinto"), "want an `out` scope node:\n{ir}");
    assert!(ir.contains("align.domain.scale.mapinto"), "want a shared domain node:\n{ir}");
}

#[test]
fn map_into_fixed_source_omits_load_metadata() {
    // A fixed-array-literal source can't alias the `out` slice, so its (stack) loads are not
    // tagged — no over-emission. The store may still carry the (vacuous) `out` scope, but there is
    // no `align.in` scope because no slice load exists.
    let ir = emit_llvm("fn dbl(x: i64) -> i64 = x * 2\nfn fill(out d: slice<i64>) {\n  [1, 2, 3, 4].map(dbl).map_into(d)\n}\nfn main() -> i32 = 0\n");
    assert!(!ir.contains("align.in.fill.mapinto"), "a fixed-array source must not tag an `in` scope:\n{ir}");
}

// --- alias soundness gate (diagnostics) ---

#[test]
fn map_into_dst_aliasing_source_rejected() {
    // Two slice views of the same array — writing one while reading the other aliases; rejected.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> i32 {\n  mut xs := [1, 2, 3, 4]\n  s1 : slice<i64> := xs\n  mut s2 : slice<i64> := xs\n  s1.map(dbl).map_into(s2)\n  return 0\n}\n";
    assert!(check_errs("mi-alias", src));
}

#[test]
fn map_into_unknown_provenance_source_rejected() {
    // A source bound to a fn-returned slice has unknown origin — it could alias the `out` buffer,
    // so it cannot back a `noalias` claim; rejected conservatively.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn ident(x: slice<i64>) -> slice<i64> = x\nfn f(out b: slice<i64>) {\n  s := ident(b)\n  s.map(dbl).map_into(b)\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("mi-unknown-src", src));
}

#[test]
fn map_into_caller_alias_via_unknown_view_rejected() {
    // CONFIRMED-miscompile regression: `scale(src, out dst)` runs `src.map(f).map_into(dst)` and
    // emits `noalias` trusting its two slice *params* are disjoint (the caller's `out` contract).
    // A caller that binds `src` to a fn-returned view of the same array as `dst`
    // (`src := ident(ys[0..4])`, `dst := ys[1..5]`, overlapping) must be **rejected** — the source
    // root is a slice local of unknown origin, so the caller check cannot prove it disjoint from the
    // `out` buffer. (Before the fix this passed undiagnosed and the vectorizer reordered the
    // aliasing write → wrong result.)
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn ident(x: slice<i64>) -> slice<i64> = x\nfn scale(src: slice<i64>, out dst: slice<i64>) {\n  src.map(dbl).map_into(dst)\n}\nfn main() -> i32 {\n  mut ys := [1, 2, 3, 4, 5]\n  src : slice<i64> := ident(ys[0..4])\n  mut dst : slice<i64> := ys[1..5]\n  scale(src, dst)\n  return ys.sum() as i32\n}\n";
    assert!(check_errs("mi-caller-unknown-alias", src));
}

#[test]
fn map_into_caller_alias_inline_call_arg_rejected() {
    // The same miscompile via an inline fn-call argument (unresolvable root) — also rejected.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn ident(x: slice<i64>) -> slice<i64> = x\nfn scale(src: slice<i64>, out dst: slice<i64>) {\n  src.map(dbl).map_into(dst)\n}\nfn main() -> i32 {\n  mut ys := [1, 2, 3, 4, 5]\n  mut dst : slice<i64> := ys[1..5]\n  scale(ident(ys[0..4]), dst)\n  return ys.sum() as i32\n}\n";
    assert!(check_errs("mi-caller-inline-alias", src));
}

#[test]
fn map_into_where_stage_rejected() {
    // v1 supports only length-preserving stages; a filtering `where` is deferred.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn big(x: i64) -> bool = x > 1\nfn f(src: slice<i64>, out b: slice<i64>) {\n  src.where(big).map(dbl).map_into(b)\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("mi-where", src));
}

#[test]
fn map_into_non_slice_dst_rejected() {
    // The destination must be a slice place, not a fixed array value.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> i32 {\n  xs := [1, 2, 3]\n  mut ys := [0, 0, 0]\n  xs.map(dbl).map_into(ys)\n  return 0\n}\n";
    assert!(check_errs("mi-nonslice", src));
}

#[test]
fn map_into_immutable_dst_rejected() {
    // A non-`mut` slice binding is not writable.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> i32 {\n  xs := [1, 2, 3]\n  ys := [0, 0, 0]\n  d : slice<i64> := ys\n  xs.map(dbl).map_into(d)\n  return 0\n}\n";
    assert!(check_errs("mi-immut", src));
}

#[test]
fn map_into_element_type_mismatch_rejected() {
    // The pipeline element must match the destination slice element.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn f(src: slice<i64>, out b: slice<i32>) {\n  src.map(dbl).map_into(b)\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("mi-elem-mismatch", src));
}
