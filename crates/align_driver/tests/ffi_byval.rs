//! `extern "C"` by-value struct passing/returning — SysV AMD64 only (draft.md §15). A `layout(C)`
//! struct (declaration-order, natural-alignment, scalar int/float fields) crosses the C boundary in
//! registers using the System V AMD64 classification: each eightbyte is INTEGER (→ a GP register /
//! `i64` slot) or SSE (→ an XMM register / `double` slot); a two-register value returns as an
//! `{T0,T1}` aggregate. Align reproduces exactly the coerced IR types clang emits, so each call is
//! binary-compatible with a real C callee.
//!
//! Every value test compiles a small C helper (via `cc`) that defines the by-value callee and links
//! it against the Align object — the round trip validates the register coercion against a genuine C
//! ABI, not a self-consistent guess. Tests are gated on both a working backend and `cc`.
//!
//! Coverage of the eightbyte patterns: `{i32,i32}` (1×INTEGER), `{i64,i64}` (2×INTEGER),
//! `{f64,f64}` (2×SSE), `{f32,f32}` (1×SSE, packed — clang's `<2 x float>`, we use `double`),
//! `{i32,f32}` (1×INTEGER by the merge rule), a mixed `{i64,f64}` return (INTEGER,SSE → RAX,XMM0),
//! single-register returns, a full param+return round trip, and the rejections (> 16-byte MEMORY,
//! non-`layout(C)` struct).
//!
//! **This suite is x86-64-only**, because the feature is. On any other target codegen refuses to
//! emit a by-value struct call at all, and `sysv_only_targets_fail_closed` is what runs instead —
//! pinning that the refusal is a clear diagnostic rather than some other target's ABI applied
//! silently.

mod common;
use common::*;

fn ok(src: &str) -> bool {
    let mut sm = SourceMap::new();
    !check(&mut sm, "ffi_byval", src).diags.has_errors()
}

/// By-value struct FFI is SysV-AMD64-only by design, so every test below is x86-64-only too — on
/// any other target codegen refuses to emit the call at all (see `sysv_only_targets_fail_closed`,
/// which is what runs there instead). Without the arch condition this whole suite fails on arm64
/// against a compiler that is behaving exactly as specified.
fn gated() -> bool {
    sysv_target() && backend_available() && cc_available()
}

/// Whether this host is the one target where by-value struct passing is implemented.
fn sysv_target() -> bool {
    cfg!(target_arch = "x86_64")
}

#[test]
fn param_two_i32_one_eightbyte_integer() {
    if !gated() {
        return;
    }
    // `{i32,i32}` = 8 bytes = one INTEGER eightbyte → passed as a single `i64`. 3 + 4 = 7.
    let out = build_and_run_with_c(
        "byval-i32i32",
        "layout(C) Pt { a: i32, b: i32 }\nextern \"C\" fn sum_pt(p: Pt) -> i32\n\nfn main() -> i32 {\n  unsafe {\n    return sum_pt(Pt { a: 3, b: 4 })\n  }\n}\n",
        "struct Pt { int a; int b; };\nint sum_pt(struct Pt p) { return p.a + p.b; }\n",
    );
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn param_two_i64_two_eightbytes_integer() {
    if !gated() {
        return;
    }
    // `{i64,i64}` = 16 bytes = two INTEGER eightbytes → two `i64` args (RDI, RSI). 10 + 11 = 21.
    let out = build_and_run_with_c(
        "byval-i64i64",
        "layout(C) Wide { a: i64, b: i64 }\nextern \"C\" fn sum_wide(w: Wide) -> i64\n\nfn main() -> i32 {\n  unsafe {\n    return sum_wide(Wide { a: 10, b: 11 }) as i32\n  }\n}\n",
        "#include <stdint.h>\nstruct Wide { int64_t a; int64_t b; };\nint64_t sum_wide(struct Wide w) { return w.a + w.b; }\n",
    );
    assert_eq!(out.status.code(), Some(21));
}

#[test]
fn param_two_f64_two_eightbytes_sse() {
    if !gated() {
        return;
    }
    // `{f64,f64}` = two SSE eightbytes → two `double` args (XMM0, XMM1). 1.5 + 2.5 = 4.0.
    let out = build_and_run_with_c(
        "byval-f64f64",
        "layout(C) V2 { x: f64, y: f64 }\nextern \"C\" fn sum_v2(v: V2) -> f64\n\nfn main() -> i32 {\n  unsafe {\n    return sum_v2(V2 { x: 1.5, y: 2.5 }) as i32\n  }\n}\n",
        "struct V2 { double x; double y; };\ndouble sum_v2(struct V2 v) { return v.x + v.y; }\n",
    );
    assert_eq!(out.status.code(), Some(4));
}

#[test]
fn param_two_f32_one_eightbyte_sse_packed() {
    if !gated() {
        return;
    }
    // `{f32,f32}` = 8 bytes = one SSE eightbyte holding two packed floats. clang coerces this to a
    // `<2 x float>`; we pass it as a `double` — both are 8 bytes in the same XMM register with
    // identical bytes, so this is the critical packed-float ABI-compat check. 1.5 + 2.5 = 4.0.
    let out = build_and_run_with_c(
        "byval-f32f32",
        "layout(C) F2 { a: f32, b: f32 }\nextern \"C\" fn sum_f2(s: F2) -> f32\n\nfn main() -> i32 {\n  unsafe {\n    return sum_f2(F2 { a: 1.5, b: 2.5 }) as i32\n  }\n}\n",
        "struct F2 { float a; float b; };\nfloat sum_f2(struct F2 s) { return s.a + s.b; }\n",
    );
    assert_eq!(out.status.code(), Some(4));
}

#[test]
fn param_i32_f32_one_eightbyte_merges_to_integer() {
    if !gated() {
        return;
    }
    // `{i32,f32}` = 8 bytes = one eightbyte with an integer *and* a float field → the merge rule
    // makes it INTEGER (passed as one `i64`, not a `double`). Getting this wrong would put the value
    // in the wrong register class. 5 + (int)2.0 = 7.
    let out = build_and_run_with_c(
        "byval-i32f32",
        "layout(C) Mix { a: i32, b: f32 }\nextern \"C\" fn f_mix(m: Mix) -> i32\n\nfn main() -> i32 {\n  unsafe {\n    return f_mix(Mix { a: 5, b: 2.0 })\n  }\n}\n",
        "struct Mix { int a; float b; };\nint f_mix(struct Mix m) { return m.a + (int)m.b; }\n",
    );
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn return_struct_one_eightbyte() {
    if !gated() {
        return;
    }
    // A C function returns an 8-byte struct by value (one INTEGER eightbyte → returned in RAX as
    // `i64`). Align reconstructs the struct and reads both fields. 3 + 4 = 7.
    let out = build_and_run_with_c(
        "byval-ret-pt",
        "layout(C) Pt { a: i32, b: i32 }\nextern \"C\" fn make_pt(a: i32, b: i32) -> Pt\n\nfn main() -> i32 {\n  unsafe {\n    p := make_pt(3, 4)\n    return p.a + p.b\n  }\n}\n",
        "struct Pt { int a; int b; };\nstruct Pt make_pt(int a, int b) { struct Pt p = { a, b }; return p; }\n",
    );
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn return_struct_two_eightbytes_integer() {
    if !gated() {
        return;
    }
    // A 16-byte all-integer struct returns in RAX:RDX as `{i64,i64}`. 100 + 23 = 123.
    let out = build_and_run_with_c(
        "byval-ret-wide",
        "layout(C) Wide { a: i64, b: i64 }\nextern \"C\" fn make_wide(a: i64, b: i64) -> Wide\n\nfn main() -> i32 {\n  unsafe {\n    w := make_wide(100, 23)\n    return (w.a + w.b) as i32\n  }\n}\n",
        "#include <stdint.h>\nstruct Wide { int64_t a; int64_t b; };\nstruct Wide make_wide(int64_t a, int64_t b) { struct Wide w = { a, b }; return w; }\n",
    );
    assert_eq!(out.status.code(), Some(123));
}

#[test]
fn return_struct_two_eightbytes_sse() {
    if !gated() {
        return;
    }
    // A 16-byte all-float struct returns in XMM0:XMM1 as `{double,double}`. 1.5 + 5.5 = 7.0.
    let out = build_and_run_with_c(
        "byval-ret-v2",
        "layout(C) V2 { x: f64, y: f64 }\nextern \"C\" fn make_v2(x: f64, y: f64) -> V2\n\nfn main() -> i32 {\n  unsafe {\n    v := make_v2(1.5, 5.5)\n    return (v.x + v.y) as i32\n  }\n}\n",
        "struct V2 { double x; double y; };\nstruct V2 make_v2(double x, double y) { struct V2 v = { x, y }; return v; }\n",
    );
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn return_struct_two_eightbytes_mixed_int_then_sse() {
    if !gated() {
        return;
    }
    // A mixed `{i64, f64}` = INTEGER,SSE eightbytes → returned in RAX (i64) and XMM0 (double). The
    // aggregate return type must be `{i64, double}` (not `{double,double}`) so LLVM assigns the
    // right register classes. a=40, b=2.0 → 42.
    let out = build_and_run_with_c(
        "byval-ret-idf",
        "layout(C) IdF { a: i64, b: f64 }\nextern \"C\" fn make_idf(a: i64, b: f64) -> IdF\n\nfn main() -> i32 {\n  unsafe {\n    v := make_idf(40, 2.0)\n    return (v.a + (v.b as i64)) as i32\n  }\n}\n",
        "#include <stdint.h>\nstruct IdF { int64_t a; double b; };\nstruct IdF make_idf(int64_t a, double b) { struct IdF v = { a, b }; return v; }\n",
    );
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn round_trip_param_and_return() {
    if !gated() {
        return;
    }
    // Pass a struct by value; C returns a modified struct by value; Align reads it back. Exercises
    // the param coerce and the return reconstruct in one call. {a=3,b=4} → {a=4,b=6} → 10.
    let out = build_and_run_with_c(
        "byval-roundtrip",
        "layout(C) Pt { a: i32, b: i32 }\nextern \"C\" fn bump(p: Pt) -> Pt\n\nfn main() -> i32 {\n  unsafe {\n    p := bump(Pt { a: 3, b: 4 })\n    return p.a + p.b\n  }\n}\n",
        "struct Pt { int a; int b; };\nstruct Pt bump(struct Pt p) { struct Pt r = { p.a + 1, p.b + 2 }; return r; }\n",
    );
    assert_eq!(out.status.code(), Some(10));
}

#[test]
fn oversized_struct_param_is_rejected_in_codegen() {
    // A > 16-byte struct is MEMORY class (would need a `byval` pointer). FFI v1 rejects it — pass by
    // pointer instead. It type-checks (the language accepts a `layout(C)` struct as an FFI type) but
    // codegen refuses to emit a wrong/unsupported ABI. Gated on the backend so an unrelated
    // target-machine failure can't masquerade as this rejection.
    if !(sysv_target() && backend_available()) {
        return;
    }
    let mut sm = SourceMap::new();
    let src = "layout(C) Big { a: i64, b: i64, c: i64 }\nextern \"C\" fn f(b: Big) -> i32\nfn main() -> i32 {\n  unsafe { return f(Big { a: 1, b: 2, c: 3 }) }\n}\n";
    let checked = check(&mut sm, "byval-big", src);
    assert!(!checked.diags.has_errors(), "a `layout(C)` struct is a valid FFI type at the language level");
    let mir = lower_to_mir(&checked.hir);
    let ir = emit_llvm_ir(&mir, BuildTarget::Baseline, false, &[], false);
    assert!(ir.is_err(), "a > 16-byte by-value struct param must be rejected in codegen");
    assert!(
        ir.unwrap_err().contains("16-byte"),
        "the diagnostic should explain the MEMORY-class size limit"
    );
}

#[test]
fn oversized_struct_return_is_rejected_in_codegen() {
    if !(sysv_target() && backend_available()) {
        return;
    }
    let mut sm = SourceMap::new();
    let src = "layout(C) Big { a: i64, b: i64, c: i64 }\nextern \"C\" fn f() -> Big\nfn main() -> i32 {\n  unsafe { b := f(); return b.a as i32 }\n}\n";
    let checked = check(&mut sm, "byval-big-ret", src);
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    let ir = emit_llvm_ir(&mir, BuildTarget::Baseline, false, &[], false);
    assert!(ir.is_err(), "a > 16-byte by-value struct return must be rejected in codegen");
    assert!(
        ir.unwrap_err().contains("16-byte"),
        "the diagnostic should explain the MEMORY-class size limit, not fail for an unrelated reason"
    );
}

#[test]
fn empty_struct_extern_is_rejected_in_sema() {
    // A zero-field struct has no C ABI representation — rejected in sema with an accurate message (not
    // the wrong "> 16-byte MEMORY" diagnostic, and not silently accepted).
    let msg = check_diagnostics(
        "byval-empty",
        "layout(C) E {}\nextern \"C\" fn f(e: E) -> i32\nfn main() -> i32 { return 0 }\n",
    );
    assert!(msg.contains("empty struct") && msg.contains("no C ABI representation"), "got: {msg}");
    // And as a return type.
    assert!(!ok("layout(C) E {}\nextern \"C\" fn g() -> E\nfn main() -> i32 { return 0 }\n"));
}

// ── SysV register-pressure (the all-or-nothing rule) ────────────────────────────────────────────
//
// A two-eightbyte struct is passed in registers only if *both* eightbytes fit in the class registers
// free after the preceding arguments; otherwise the whole struct goes to memory (`byval`). We accept
// the fits-in-registers cases (round-trip against a clang callee) and reject the exhaustion boundary
// (which we cannot lower correctly by flattening).

#[test]
fn pressure_fits_zero_preceding_int() {
    if !gated() {
        return;
    }
    // 0 preceding integer args + `{i64,i64}` (needs 2 GP; 6 free) → registers. 10 + 20 = 30.
    let out = build_and_run_with_c(
        "byval-press-0",
        "layout(C) L2 { a: i64, b: i64 }\nextern \"C\" fn f0(s: L2) -> i64\n\nfn main() -> i32 {\n  unsafe {\n    return f0(L2 { a: 10, b: 20 }) as i32\n  }\n}\n",
        "#include <stdint.h>\nstruct L2 { int64_t a; int64_t b; };\nint64_t f0(struct L2 s) { return s.a + s.b; }\n",
    );
    assert_eq!(out.status.code(), Some(30));
}

#[test]
fn pressure_fits_four_preceding_int_boundary() {
    if !gated() {
        return;
    }
    // 4 preceding integer args (RDI..RCX) + `{i64,i64}` needs 2 GP → exactly fills R8,R9 (6 total).
    // The tight boundary that must still pass in registers. 1+2+3+4+10+20 = 40.
    let out = build_and_run_with_c(
        "byval-press-4",
        "layout(C) L2 { a: i64, b: i64 }\nextern \"C\" fn f4(a: i64, b: i64, c: i64, d: i64, s: L2) -> i64\n\nfn main() -> i32 {\n  unsafe {\n    return f4(1, 2, 3, 4, L2 { a: 10, b: 20 }) as i32\n  }\n}\n",
        "#include <stdint.h>\nstruct L2 { int64_t a; int64_t b; };\nint64_t f4(int64_t a, int64_t b, int64_t c, int64_t d, struct L2 s) { return a + b + c + d + s.a + s.b; }\n",
    );
    assert_eq!(out.status.code(), Some(40));
}

#[test]
fn pressure_fits_six_preceding_sse_boundary() {
    if !gated() {
        return;
    }
    // 6 preceding double args (XMM0..5) + `{f64,f64}` needs 2 SSE → exactly fills XMM6,XMM7 (8 total).
    // 1+1+1+1+1+1 + 1.5+2.5 = 10.0.
    let out = build_and_run_with_c(
        "byval-press-sse6",
        "layout(C) D2 { x: f64, y: f64 }\nextern \"C\" fn h6(a: f64, b: f64, c: f64, d: f64, e: f64, g: f64, s: D2) -> f64\n\nfn main() -> i32 {\n  unsafe {\n    return h6(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, D2 { x: 1.5, y: 2.5 }) as i32\n  }\n}\n",
        "struct D2 { double x; double y; };\ndouble h6(double a, double b, double c, double d, double e, double g, struct D2 s) { return a + b + c + d + e + g + s.x + s.y; }\n",
    );
    assert_eq!(out.status.code(), Some(10));
}

#[test]
fn pressure_five_preceding_int_is_rejected() {
    if !(sysv_target() && backend_available()) {
        return;
    }
    // 5 preceding integer args (only R9 free) + `{i64,i64}` needs 2 GP → the struct would go to
    // memory (`byval`). clang lowers this as a `byval` pointer; flattening cannot mimic it, so we
    // reject rather than silently miscompile.
    let mut sm = SourceMap::new();
    let src = "layout(C) L2 { a: i64, b: i64 }\nextern \"C\" fn f5(a: i64, b: i64, c: i64, d: i64, e: i64, s: L2) -> i64\nfn main() -> i32 {\n  unsafe { return f5(1, 2, 3, 4, 5, L2 { a: 10, b: 20 }) as i32 }\n}\n";
    let checked = check(&mut sm, "byval-press-5", src);
    assert!(!checked.diags.has_errors(), "the signature type-checks; the ABI limit is a codegen concern");
    let mir = lower_to_mir(&checked.hir);
    let ir = emit_llvm_ir(&mir, BuildTarget::Baseline, false, &[], false);
    assert!(ir.is_err(), "a struct arg that falls to MEMORY under register pressure must be rejected");
    let err = ir.unwrap_err();
    assert!(err.contains("passed in memory") && err.contains("register"), "got: {err}");
}

#[test]
fn pressure_seven_preceding_sse_is_rejected() {
    if !(sysv_target() && backend_available()) {
        return;
    }
    // 7 preceding double args (only XMM7 free) + `{f64,f64}` needs 2 SSE → MEMORY. Rejected.
    let mut sm = SourceMap::new();
    let src = "layout(C) D2 { x: f64, y: f64 }\nextern \"C\" fn h7(a: f64, b: f64, c: f64, d: f64, e: f64, g: f64, h: f64, s: D2) -> f64\nfn main() -> i32 {\n  unsafe { return h7(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, D2 { x: 1.5, y: 2.5 }) as i32 }\n}\n";
    let checked = check(&mut sm, "byval-press-sse7", src);
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    let ir = emit_llvm_ir(&mir, BuildTarget::Baseline, false, &[], false);
    assert!(ir.is_err(), "an SSE struct arg that falls to MEMORY under register pressure must be rejected");
    assert!(ir.unwrap_err().contains("passed in memory"));
}

#[test]
fn pressure_single_eightbyte_after_five_ints_still_fits() {
    if !gated() {
        return;
    }
    // A *single*-eightbyte struct needs only 1 GP, so after 5 ints (R9 free) it still fits — the
    // atomicity rule only bites two-eightbyte structs. 1+2+3+4+5 + (6+7) = 28.
    let out = build_and_run_with_c(
        "byval-press-single",
        "layout(C) Pt { a: i32, b: i32 }\nextern \"C\" fn s5(a: i64, b: i64, c: i64, d: i64, e: i64, p: Pt) -> i64\n\nfn main() -> i32 {\n  unsafe {\n    return s5(1, 2, 3, 4, 5, Pt { a: 6, b: 7 }) as i32\n  }\n}\n",
        "#include <stdint.h>\nstruct Pt { int a; int b; };\nint64_t s5(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, struct Pt p) { return a + b + c + d + e + p.a + p.b; }\n",
    );
    assert_eq!(out.status.code(), Some(28));
}

#[test]
fn non_layout_c_struct_param_is_rejected_in_sema() {
    // A struct without `layout(C)` has a compiler-private (reorderable) layout, so it has no stable C
    // representation — rejected in sema with an actionable message.
    assert!(!ok("Pt { a: i32, b: i32 }\nextern \"C\" fn f(p: Pt) -> i32\nfn main() -> i32 { return 0 }\n"));
}

#[test]
fn non_layout_c_struct_return_is_rejected_in_sema() {
    assert!(!ok("Pt { a: i32, b: i32 }\nextern \"C\" fn f() -> Pt\nfn main() -> i32 { return 0 }\n"));
}

#[test]
fn layout_c_struct_extern_type_checks() {
    // The positive sema surface: a `layout(C)` struct is accepted as both a parameter and a return
    // type (codegen enforces the SysV/target/size limits separately).
    assert!(ok("layout(C) Pt { a: i32, b: i32 }\nextern \"C\" fn f(p: Pt) -> Pt\nfn main() -> i32 { return 0 }\n"));
}

/// The other side of the same contract: on a target where by-value struct passing is NOT
/// implemented, codegen must **fail closed** rather than emit some other target's ABI.
///
/// This is the assertion that runs where the suite above does not, and it is a real safety
/// property, not a placeholder: silently applying the SysV eightbyte classification on AArch64
/// (whose AAPCS rules differ) would produce a call that links fine and corrupts arguments at run
/// time. The diagnostic must also name the offending target and the way out, since "your struct
/// crosses the C boundary wrongly" is invisible otherwise.
#[test]
fn sysv_only_targets_fail_closed() {
    if sysv_target() || !backend_available() {
        return;
    }
    let mut sm = SourceMap::new();
    // A 16-byte struct: well within SysV's register-passing range, so nothing but the TARGET can
    // be the reason this is refused.
    let src = "layout(C) Pair { a: i64, b: i64 }\nextern \"C\" fn g(p: Pair) -> i32\nfn main() -> i32 {\n  unsafe { return g(Pair { a: 1, b: 2 }) }\n}\n";
    let checked = check(&mut sm, "byval-non-sysv", src);
    assert!(
        !checked.diags.has_errors(),
        "a `layout(C)` struct stays a valid FFI type at the language level on every target"
    );
    let mir = lower_to_mir(&checked.hir);
    let err = emit_llvm_ir(&mir, BuildTarget::Baseline, false, &[], false)
        .expect_err("by-value struct FFI must be refused on a non-SysV target");
    assert!(
        err.contains("x86-64 SysV"),
        "the diagnostic must say which ABI is the supported one:\n{err}"
    );
    assert!(
        err.contains("pointer"),
        "the diagnostic must point at the supported alternative:\n{err}"
    );
}
