//! First-class function values (slice ①): a top-level function used as a value is a function
//! pointer (`Ty::Fn`), and calling such a local is an indirect call. Non-capturing only — no
//! environment yet (lambda-as-value and captures are later slices).


mod common;
use common::*;

#[test]
fn named_fn_as_value_and_call() {
    if !backend_available() {
        return;
    }
    let src = "fn double(x: i32) -> i32 = x * 2\n\nfn main() -> Result<(), Error> {\n  f := double\n  print(f(5))\n  print(f(21))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-basic", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n42\n");
}

#[test]
fn reassign_fn_value_same_signature() {
    if !backend_available() {
        return;
    }
    // add and sub share a signature → the same Ty::Fn → a `mut` fn value can hold either.
    let src = "fn add(a: i64, b: i64) -> i64 = a + b\nfn sub(a: i64, b: i64) -> i64 = a - b\n\nfn main() -> Result<(), Error> {\n  mut op := add\n  print(op(10, 3))\n  op = sub\n  print(op(10, 3))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-reassign", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "13\n7\n");
}

#[test]
fn dynamically_selected_unit_fn_value_uses_the_selected_target() {
    if !backend_available() {
        return;
    }
    // Keep both targets live through optimization: the selected function depends on runtime argv.
    // Unit-returning fn-value thunks use LLVM `void`, so the indirect call must use the same ABI.
    let src = "fn first() { print(1) }\nfn second() { print(2) }\n\nfn main(args: array<str>) -> Result<(), Error> {\n  mut f := first\n  if args.len() > 1 { f = second }\n  f()\n  return Ok(())\n}\n";
    let first = build_and_run_args("fv-unit-dynamic-first", src, &[]);
    assert_eq!(first.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&first.stdout), "1\n");

    let second = build_and_run_args("fv-unit-dynamic-second", src, &["choose-second"]);
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&second.stdout), "2\n");
}

#[test]
fn higher_order_named_fn() {
    if !backend_available() {
        return;
    }
    // A fn-typed parameter (`fn(i64) -> i64`) receiving a named function.
    let src = "fn apply(f: fn(i64) -> i64, x: i64) -> i64 = f(x)\nfn dbl(n: i64) -> i64 = n * 2\n\nfn main() -> Result<(), Error> {\n  print(apply(dbl, 21))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-hof", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn higher_order_capturing_closure() {
    if !backend_available() {
        return;
    }
    // A capturing closure passed to a HOF — its env lives in the caller's frame, alive for the call.
    let src = "fn apply(f: fn(i64) -> i64, x: i64) -> i64 = f(x)\n\nfn main() -> Result<(), Error> {\n  k: i64 := 100\n  print(apply(fn n: i64 { n + k }, 5))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-hof-cap", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "105\n");
}

#[test]
fn returning_fn_value_rejected() {
    // A returned function value would carry a frame-local env out of the frame — rejected for now.
    assert!(check_errs(
        "fv-ret",
        "fn pick() -> fn(i64) -> i64 = dbl\nfn dbl(n: i64) -> i64 = n * 2\nfn main() -> i32 { return 0 }\n"
    ));
}

#[test]
fn lambda_returning_fn_value_rejected() {
    // A stage/value lambda whose body yields a function value would let a frame-local closure
    // env escape via the lift — rejected in lift_lambda (mirrors the top-level return check).
    assert!(check_errs(
        "fv-lam-ret",
        "fn main() -> Result<(), Error> {\n  print([1,2,3].map(fn x: i64 { fn y: i64 { x + y } }).sum())\n  return Ok(())\n}\n"
    ));
}

#[test]
fn arg_type_mismatch_rejected() {
    // Calling a fn value with the wrong argument type is a type error.
    assert!(check_errs(
        "fv-argty",
        "fn takes_float(x: f64) -> f64 = x\n\nfn main() -> i32 {\n  f := takes_float\n  if f(1) > 0.0 { return 1 }\n  return 0\n}\n"
    ));
}

#[test]
fn lambda_as_value_and_call() {
    if !backend_available() {
        return;
    }
    // A lambda used as a value (typed params) is lifted to a function pointer (slice ②a).
    let src = "fn main() -> Result<(), Error> {\n  f := fn x: i32 { x * 2 }\n  print(f(5))\n  g := fn a: i64, b: i64 { a + b }\n  print(g(10, 32))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-lambda", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n42\n");
}

#[test]
fn untyped_lambda_param_rejected_as_value() {
    // A lambda value has no use site to infer from, so params need explicit types.
    assert!(check_errs(
        "fv-untyped",
        "fn main() -> Result<(), Error> {\n  f := fn x { x * 2 }\n  print(f(5))\n  return Ok(())\n}\n"
    ));
}

#[test]
fn capturing_closure_value_and_call() {
    if !backend_available() {
        return;
    }
    // A capturing lambda copies the captured value into a frame-local env (slice ②b-2).
    let src = "fn main() -> Result<(), Error> {\n  k: i32 := 100\n  f := fn x: i32 { x + k }\n  print(f(5))\n  print(f(20))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-capture", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "105\n120\n");
}

#[test]
fn closure_multiple_captures() {
    if !backend_available() {
        return;
    }
    // Two captures (a, b) + two explicit params: (x+y)*a - b.
    let src = "fn main() -> Result<(), Error> {\n  a: i64 := 10\n  b: i64 := 3\n  g := fn x: i64, y: i64 { (x + y) * a - b }\n  print(g(1, 2))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-multicap", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "27\n");
}

#[test]
fn non_scalar_signature_rejected_as_value() {
    // slice ①: only scalar params/return may become a function value.
    assert!(check_errs(
        "fv-nonscalar",
        "fn sum(xs: slice<i64>) -> i64 = 0\n\nfn main() -> i32 {\n  f := sum\n  return 0\n}\n"
    ));
}

// ── F1①: function-value struct fields (the pkg.web `Route.handler` gate) ──────────────────────

#[test]
fn fn_value_struct_field_stored_and_called() {
    if !backend_available() {
        return;
    }
    // A `Ty::Fn` field on a struct: build the struct with a named function, read the field back,
    // and indirect-call it (`r.handler(arg)`). This is the shape `pkg.web`'s `Route` needs.
    let src = "fn h(x: i64) -> i64 = x + 100\n\nRoute { pattern: str, handler: fn(i64) -> i64 }\n\nfn main() -> Result<(), Error> {\n  r := Route { pattern: \"/a\", handler: h }\n  print(r.handler(5))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-field", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "105\n");
}

#[test]
fn fn_value_struct_field_array_dispatch() {
    if !backend_available() {
        return;
    }
    // An `array<Route>` dispatched by index — the router acceptance shape: each element carries a
    // distinct handler, called through the field after an indexed read.
    let src = concat!(
        "fn list_models(n: i64) -> i64 = n + 100\n",
        "fn get_model(n: i64) -> i64 = n + 200\n\n",
        "Route { pattern: str, handler: fn(i64) -> i64 }\n\n",
        "fn main() -> Result<(), Error> {\n",
        "  routes := [\n",
        "    Route { pattern: \"/models\", handler: list_models },\n",
        "    Route { pattern: \"/models/:id\", handler: get_model },\n",
        "  ]\n",
        "  mut i := 0\n",
        "  loop {\n",
        "    if i >= routes.len() { break }\n",
        "    r := routes[i]\n",
        "    print(r.handler(i))\n",
        "    i = i + 1\n",
        "  }\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("fv-field-array", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "100\n201\n");
}

#[test]
fn fn_value_struct_field_wrong_arity_rejected() {
    // Calling a fn-typed field with the wrong argument count is a clean type error, not a panic.
    assert!(check_errs(
        "fv-field-arity",
        "fn h(x: i64) -> i64 = x\nR { f: fn(i64) -> i64 }\nfn main() -> Result<(), Error> {\n  r := R { f: h }\n  print(r.f(1, 2))\n  return Ok(())\n}\n"
    ));
}

#[test]
fn non_fn_struct_field_called_rejected() {
    // A non-function field called as `r.field(args)` still reports "unknown method", unchanged.
    assert!(check_errs(
        "fv-field-nonfn",
        "R { x: i64 }\nfn main() -> Result<(), Error> {\n  r := R { x: 1 }\n  print(r.x(1))\n  return Ok(())\n}\n"
    ));
}

#[test]
fn fn_field_beside_owned_field_drops_cleanly() {
    if !backend_available() {
        return;
    }
    // A **Move** struct (it owns a `string`) that also carries a fn-value field and a scalar: the fn
    // field is Copy and owns nothing, so `drop_struct_fields` must free only the `string` and skip
    // the fn field. Call the fn field, read the scalar, then let the struct drop at scope exit — a
    // clean exit (0) proves the drop freed exactly once (no double-free / leak on the fn field).
    let src = concat!(
        "fn a(n: i64) -> i64 = n + 1\n",
        "Holder { name: string, handler: fn(i64) -> i64, age: i64 }\n",
        "fn main() -> Result<(), Error> {\n",
        "  h := Holder { name: \"hi\".clone(), handler: a, age: 7 }\n",
        "  print(h.handler(41))\n",
        "  print(h.age)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("fv-field-owned-drop", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n7\n");
}

#[test]
fn owned_local_moved_through_an_indirect_call_no_double_free() {
    if !backend_available() {
        return;
    }
    // A by-value owned argument is MOVED into the callee through an INDIRECT call too, so the
    // caller's source local must be nulled — otherwise the caller's exit Drop and the callee's both
    // free the same buffer. An inline temporary was always safe (no source local to null); a BOUND
    // local was double-freed. Here the callee additionally moves the handle out of its parameter,
    // the pkg.web responder shape. A clean exit proves each buffer is freed exactly once.
    let src = concat!(
        "H { buf: buffer, tag: i64 }\n",
        "R { handler: fn(H) -> i64 }\n",
        "fn take(h: H) -> i64 {\n",
        "  b := h.buf\n",
        "  return h.tag\n",
        "}\n",
        "fn main() -> Result<(), Error> {\n",
        "  rs := [R { handler: take }]\n",
        "  r := rs[0]\n",
        "  c := H { buf: buffer(32), tag: 7 }\n",
        "  print(r.handler(c))\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("fv-indirect-move", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}
