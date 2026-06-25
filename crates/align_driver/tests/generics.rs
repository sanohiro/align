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

#[test]
fn generic_main_rejected() {
    // `main` is the entry point and cannot be a generic template.
    let src = "fn main<T>() -> i32 = 0\n";
    assert!(check_errs("gen-main", src));
}

// ---- 4c-2: builtin bounds (Num / Ord / Eq) ----

#[test]
fn num_bound_enables_arithmetic() {
    if !backend_available() {
        return;
    }
    let src = "fn add<T: Num>(a: T, b: T) -> T = a + b\nfn main() -> i32 = add(10, 20) + add(5, 7)\n";
    let out = build_and_run("gen-num", src);
    assert_eq!(out.status.code(), Some(42)); // 30 + 12
}

#[test]
fn ord_bound_enables_comparison() {
    if !backend_available() {
        return;
    }
    let src = "fn max<T: Ord>(a: T, b: T) -> T = if a > b { a } else { b }\nfn main() -> i32 = max(7, 12) + max(30, 2)\n";
    let out = build_and_run("gen-ord", src);
    assert_eq!(out.status.code(), Some(42)); // 12 + 30
}

#[test]
fn eq_bound_enables_equality_on_char() {
    if !backend_available() {
        return;
    }
    let src = "fn same<T: Eq>(a: T, b: T) -> bool = a == b\nfn main() -> i32 = if same('x', 'x') { 42 } else { 0 }\n";
    let out = build_and_run("gen-eq", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn arithmetic_without_num_bound_rejected() {
    let src = "fn add<T>(a: T, b: T) -> T = a + b\nfn main() -> i32 = add(1, 2)\n";
    assert!(check_errs("gen-noarith", src));
}

#[test]
fn ordering_with_only_eq_rejected() {
    let src = "fn gt<T: Eq>(a: T, b: T) -> bool = a > b\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-eq-noord", src));
}

#[test]
fn ord_instantiated_with_non_ord_rejected() {
    // `bool` is not Ord — the instantiation must fail.
    let src = "fn max<T: Ord>(a: T, b: T) -> T = if a > b { a } else { b }\nfn main() -> i32 = if max(true, false) { 1 } else { 0 }\n";
    assert!(check_errs("gen-ord-bool", src));
}

#[test]
fn num_instantiated_with_char_rejected() {
    // `char` is Ord/Eq but not Num.
    let src = "fn add<T: Num>(a: T, b: T) -> T = a + b\nfn main() -> i32 {\n  add('a', 'b')\n  return 0\n}\n";
    assert!(check_errs("gen-num-char", src));
}

#[test]
fn unknown_bound_rejected() {
    let src = "fn f<T: Display>(x: T) -> T = x\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-badbound", src));
}

#[test]
fn equality_without_eq_bound_rejected() {
    // Regression: in 4c-1 `==` on an unconstrained `T` slipped through ungated; 4c-2 closes it.
    let src = "fn eq<T>(a: T, b: T) -> bool = a == b\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-eq-hole", src));
}

// ---- 4c-3: type parameters in Option / Result positions ----

#[test]
fn option_return_position() {
    if !backend_available() {
        return;
    }
    // `T` nested in the return type `Option<T>`; the binding annotation seeds `T = i32`.
    let src = "fn wrap<T>(x: T) -> Option<T> = Some(x)\nfn main() -> i32 {\n  o: Option<i32> := wrap(41)\n  return o else 0\n}\n";
    let out = build_and_run("gen-opt-ret", src);
    assert_eq!(out.status.code(), Some(41));
}

#[test]
fn option_param_position() {
    if !backend_available() {
        return;
    }
    // `T` nested in a parameter type `Option<T>`, inferred from the argument.
    let src = "fn unwrap_or<T>(o: Option<T>, d: T) -> T = o else d\nfn main() -> i32 {\n  a: Option<i32> := Some(7)\n  b: Option<i32> := None\n  return unwrap_or(a, 0) + unwrap_or(b, 5)\n}\n";
    let out = build_and_run("gen-opt-param", src);
    assert_eq!(out.status.code(), Some(12)); // 7 + 5
}

#[test]
fn result_return_and_question_mark() {
    if !backend_available() {
        return;
    }
    // `Result<T, Error>` return position, propagated through `?`.
    let src = concat!(
        "fn ok<T>(x: T) -> Result<T, Error> = Ok(x)\n",
        "fn run() -> Result<i32, Error> {\n",
        "  v: i32 := ok(40)?\n",
        "  return Ok(v + 2)\n",
        "}\n",
        "fn main() -> i32 = match run() { Ok(v) => v, Err(e) => 99 }\n",
    );
    let out = build_and_run("gen-result-ret", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn generic_box_param_rejected() {
    // `box<T>` over a type parameter is not supported yet (only Option/Result positions are).
    let src = "fn f<T>(b: box<T>) -> i32 = 0\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-box", src));
}
