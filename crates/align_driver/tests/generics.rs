//! Minimal generics — slice 4c-1 (the unconstrained walking skeleton). A generic function
//! `fn f<T>(...)` is monomorphized per distinct concrete instantiation (`id$i32`, `id$i64`, …):
//! type arguments are inferred (no turbofish), `Ty::Param` is substituted before the flow analyses
//! and MIR run, so move/drop and codegen only ever see concrete types. A type parameter is opaque
//! (operations on it — arithmetic, fields — are rejected; the `Num`/`Ord`/`Eq` constraint model is
//! a later slice). Uninstantiated generics are not type-checked (like a C++ template).

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

fn check_errs(name: &str, src: &str) -> bool {
    let mut sm = SourceMap::new();
    check(&mut sm, name, src).diags.has_errors()
}

#[test]
fn identity_and_pick() {
    if !backend_available() {
        return;
    }
    let src = "fn id<T>(x: T) -> T = x\nfn pick<T>(a: T, b: T) -> T = a\nfn main() -> i32 {\n  x := id(5)\n  y := pick(10, 20)\n  return x + y\n}\n";
    let out = build_and_run("gen-id-pick", src);
    assert_eq!(out.status.code(), Some(15));
}

#[test]
fn same_generic_two_instantiations() {
    if !backend_available() {
        return;
    }
    // `id` at i32 and i64 — two distinct monomorphs (`id$i32`, `id$i64`).
    let src = "fn id<T>(x: T) -> T = x\nfn use_i64(n: i64) -> i64 = n\nfn main() -> i32 {\n  a := id(3)\n  b := use_i64(id(40))\n  return a + 9\n}\n";
    let out = build_and_run("gen-two-inst", src);
    assert_eq!(out.status.code(), Some(12)); // 3 + 9; exercises id$i32 and id$i64
}

#[test]
fn multi_type_params() {
    if !backend_available() {
        return;
    }
    let src = "fn fst<A, B>(a: A, b: B) -> A = a\nfn main() -> i32 = fst(7, true) + fst(5, 100)\n";
    let out = build_and_run("gen-multi", src);
    assert_eq!(out.status.code(), Some(12)); // 7 + 5
}

#[test]
fn transitive_instantiation() {
    if !backend_available() {
        return;
    }
    // `wrap<T>` calls `id<T>`; instantiating `wrap` at i32 must instantiate `id` at i32.
    let src = "fn id<T>(x: T) -> T = x\nfn wrap<T>(x: T) -> T = id(x)\nfn main() -> i32 = wrap(42)\n";
    let out = build_and_run("gen-transitive", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn struct_type_argument() {
    if !backend_available() {
        return;
    }
    let src = "Point { x: i32, y: i32 }\nfn id<T>(v: T) -> T = v\nfn main() -> i32 {\n  p := id(Point { x: 4, y: 9 })\n  return p.x + p.y\n}\n";
    let out = build_and_run("gen-struct", src);
    assert_eq!(out.status.code(), Some(13));
}

#[test]
fn return_type_inferred_from_context() {
    if !backend_available() {
        return;
    }
    // The literal argument's type flows from the `-> i32` return through the generic result.
    let src = "fn id<T>(x: T) -> T = x\nfn main() -> i32 = id(99)\n";
    let out = build_and_run("gen-ret-infer", src);
    assert_eq!(out.status.code(), Some(99));
}

#[test]
fn owned_value_through_generic_drops() {
    if !backend_available() {
        return;
    }
    // An owned (Move) array flows through `id`; the monomorph + caller drop it correctly (no leak /
    // double-free) — the flow analyses run on the concrete instance.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn id<T>(x: T) -> T = x\nfn main() -> i32 {\n  xs := [1, 2, 3].map(dbl).to_array()\n  ys := id(xs)\n  return 0\n}\n";
    let out = build_and_run("gen-owned-drop", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn use_after_move_through_generic_rejected() {
    // Passing an owned value to a generic call moves it; a second use is use-after-move.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn id<T>(x: T) -> T = x\nfn main() -> i32 {\n  xs := [1, 2, 3].map(dbl).to_array()\n  ys := id(xs)\n  zs := id(xs)\n  return 0\n}\n";
    assert!(check_errs("gen-uam", src));
}

#[test]
fn operation_on_type_param_rejected() {
    // A type parameter is opaque in the skeleton: arithmetic on it has no constraint and is rejected.
    let src = "fn bad<T>(x: T) -> T = x + x\nfn main() -> i32 = bad(3)\n";
    assert!(check_errs("gen-op", src));
}

#[test]
fn uninferable_type_param_rejected() {
    let src = "fn make<T>() -> T = make()\nfn main() -> i32 {\n  make()\n  return 0\n}\n";
    assert!(check_errs("gen-uninfer", src));
}

#[test]
fn generic_array_param_rejected() {
    // A type parameter may only appear in a bare position (skeleton cut): `array<T>` is rejected.
    let src = "fn f<T>(xs: array<T>) -> i32 = 0\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-arrayparam", src));
}

#[test]
fn duplicate_type_param_rejected() {
    let src = "fn f<T, T>(a: T, b: T) -> T = a\nfn main() -> i32 = f(1, 2)\n";
    assert!(check_errs("gen-duptp", src));
}

#[test]
fn type_param_shadowing_type_rejected() {
    let src = "Point { x: i32, y: i32 }\nfn f<Point>(x: Point) -> Point = x\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-shadow", src));
}

#[test]
fn generic_call_arity_mismatch_rejected() {
    let src = "fn pick<T>(a: T, b: T) -> T = a\nfn main() -> i32 = pick(1)\n";
    assert!(check_errs("gen-arity", src));
}
