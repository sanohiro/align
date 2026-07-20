//! A **function value as a sum-type variant payload** — streaming enabler ② (pkg.web W6,
//! `docs/impl/pkg-design/web.md` → "Streaming" enabler list), plus the enabler-③ soundness check.
//!
//! The stream design shares one route table between unary and stream routes through an or-kind:
//!
//! ```align
//! Handler {
//!   Respond(fn(Ctx) -> Result<response_builder, Error>),
//!   Stream(fn(Ctx, http_stream) -> Result<(), Error>),
//! }
//! ```
//!
//! Before this, an enum variant payload could be a scalar / plain struct / owned `array<T>` but NOT a
//! function value — there was no `Scalar::Fn`. This adds it. A fn value is a **Copy** `{fn_ptr,
//! env_ptr}` closure (16 bytes): an enum carrying only fn payloads is non-Move and never dropped, and
//! the widening is strictly smaller than a Move-handle payload (no `is_move`, no drop dispatch). The
//! `#583` `response_builder` checklist was swept (`scalar_to_ty`, `sort_key_order`, `scalar_bytes`,
//! and the codegen `scalar_type` fn arm that reserves the 16-byte slot instead of a silent `i32`).
//!
//! Deferred (fail-closed, no consumer): a **generic** sum type with a fn payload (`Box<T> { V(fn(T) ->
//! T) }`) is still rejected at the template payload resolver — deliberately, not shipped half-built.

mod common;
use common::*;

/// Construct a fn-payload variant, `match`-extract the fn, and call it indirectly. Both variants of a
/// two-arm `Handler`-shaped enum, each a different arity, dispatch and compute correctly.
#[test]
fn fn_value_variant_dispatches() {
    if !backend_available() {
        return;
    }
    let src = "\
H { Respond(fn(i64) -> i64), Stream(fn(i64, i64) -> i64) }\n\
fn a(x: i64) -> i64 = x + 1\n\
fn b(x: i64, y: i64) -> i64 = x + y\n\
fn call_it(h: H, v: i64) -> i64 = match h {\n\
  Respond(f) => f(v),\n\
  Stream(g) => g(v, v),\n\
}\n\
fn main() -> i32 {\n\
  h1 := H.Respond(a)\n\
  h2 := H.Stream(b)\n\
  return (call_it(h1, 10) + call_it(h2, 5)) as i32\n\
}\n";
    // a(10)=11, b(5,5)=10, sum 21.
    let out = build_and_run("fnvar_dispatch", src);
    assert_eq!(out.status.code(), Some(21), "11 + 10 = 21; stderr {}", String::from_utf8_lossy(&out.stderr));
}

/// The real pkg.web handler signature — a struct parameter and a `Result` return — type-checks in
/// both variants. (Construction compares fn payloads by SIGNATURE, not `fn_types` id: each `fn`
/// expression interns a fresh `FnTy`, so an id-equality check would wrongly reject a matching fn.)
#[test]
fn handler_signature_shape_typechecks() {
    let src = "\
Ctx { id: i64 }\n\
Handler { Respond(fn(Ctx) -> Result<i64, Error>), Stream(fn(Ctx, i64) -> Result<i64, Error>) }\n\
fn unary(c: Ctx) -> Result<i64, Error> = Ok(c.id + 1)\n\
fn dual(c: Ctx, n: i64) -> Result<i64, Error> = Ok(c.id + n)\n\
fn dispatch(h: Handler, c: Ctx) -> Result<i64, Error> = match h {\n\
  Respond(f) => f(c),\n\
  Stream(g) => g(c, 100),\n\
}\n\
fn main() -> i32 { return 0 }\n";
    assert!(
        !check_errs("hshape", src),
        "handler shape must typecheck; diag: {}",
        check_diagnostics("hshape", src)
    );
}

/// An enum whose variants all carry Copy fn payloads is itself **Copy**: usable more than once, never
/// dropped. (A no-op verification that the fn payload adds no owned drop obligation.)
#[test]
fn fn_variant_enum_is_copy() {
    if !backend_available() {
        return;
    }
    let src = "\
H { A(fn(i64) -> i64), B(fn(i64) -> i64) }\n\
fn inc(x: i64) -> i64 = x + 1\n\
fn apply(h: H, v: i64) -> i64 = match h { A(f) => f(v), B(g) => g(v) }\n\
fn main() -> i32 {\n\
  h := H.A(inc)\n\
  return (apply(h, 10) + apply(h, 20)) as i32\n\
}\n";
    // h is used twice (Copy): 11 + 21 = 32.
    let out = build_and_run("fnvar_copy", src);
    assert_eq!(out.status.code(), Some(32), "11 + 21 = 32; stderr {}", String::from_utf8_lossy(&out.stderr));
}

/// A fn-payload sum type defined in one module and constructed / matched in another — a `Handler`
/// crossing an `import` boundary, so `align_interface` must round-trip a fn-typed variant payload.
#[test]
fn fn_variant_crosses_module_boundary() {
    if !backend_available() {
        return;
    }
    let types = "module types\npub H { A(fn(i64) -> i64), B(fn(i64, i64) -> i64) }\n";
    let main = "\
module main\n\
import types\n\
fn inc(x: i64) -> i64 = x + 1\n\
fn add(x: i64, y: i64) -> i64 = x + y\n\
fn apply(h: types.H, v: i64) -> i64 = match h {\n\
  A(f) => f(v),\n\
  B(g) => g(v, v),\n\
}\n\
fn main() -> i32 {\n\
  h1 := types.H.A(inc)\n\
  h2 := types.H.B(add)\n\
  return (apply(h1, 10) + apply(h2, 7)) as i32\n\
}\n";
    // inc(10)=11, add(7,7)=14, sum 25.
    let out = build_and_run_multi("fnvar_xmod", &[("types.align", types), ("main.align", main)], "main.align");
    assert_eq!(out.status.code(), Some(25), "11 + 14 = 25; stderr {}", String::from_utf8_lossy(&out.stderr));
}

/// A mixed enum: one Copy fn payload, one **Move** owned-array payload. The enum is Move (it owns the
/// array in `N`); its tag-switched drop must free the array for `N` yet SKIP the fn slot for `F` (a fn
/// owns nothing). A wrong drop would either double-free (abort) or leak.
#[test]
fn mixed_fn_and_move_payload_drops_correctly() {
    if !backend_available() {
        return;
    }
    let src = "\
H { F(fn(i64) -> i64), N(array<i64>) }\n\
fn inc(x: i64) -> i64 = x + 1\n\
fn label(h: H) -> i64 = match h {\n\
  F(f) => f(41),\n\
  N(xs) => xs.len(),\n\
}\n\
fn main() -> i32 {\n\
  a := H.F(inc)\n\
  b := H.N([10, 20, 30].to_array())\n\
  return (label(a) + label(b)) as i32\n\
}\n";
    // inc(41)=42, len=3, sum 45.
    let out = build_and_run("fnvar_mixed_drop", src);
    assert_eq!(out.status.code(), Some(45), "42 + 3 = 45; stderr {}", String::from_utf8_lossy(&out.stderr));
}

/// The pkg.web W6 shape end-to-end: an `array` of `Route` structs, each carrying a `Handler` enum
/// field with fn payloads, dispatched through `r.h`. Confirms the (conservatively region-tracked)
/// fn-payload enum stays array-eligible — a non-capturing fn's region is `Static`.
#[test]
fn handler_enum_field_in_struct_array() {
    if !backend_available() {
        return;
    }
    let src = "\
Ctx { id: i64 }\n\
Handler { Respond(fn(Ctx) -> i64), Stream(fn(Ctx, i64) -> i64) }\n\
Route { pattern: str, h: Handler }\n\
fn unary(c: Ctx) -> i64 = c.id + 1\n\
fn dual(c: Ctx, n: i64) -> i64 = c.id + n\n\
fn run(r: Route, c: Ctx) -> i64 = match r.h {\n\
  Respond(f) => f(c),\n\
  Stream(g) => g(c, 100),\n\
}\n\
fn main() -> i32 {\n\
  routes := [Route { pattern: \"/a\", h: Handler.Respond(unary) }, Route { pattern: \"/b\", h: Handler.Stream(dual) }]\n\
  c := Ctx { id: 5 }\n\
  return (run(routes[0], c) + run(routes[1], c)) as i32\n\
}\n";
    // unary(5)=6, dual(5,100)=105, sum 111.
    let out = build_and_run("fnvar_route_arr", src);
    assert_eq!(out.status.code(), Some(111), "6 + 105 = 111; stderr {}", String::from_utf8_lossy(&out.stderr));
}

/// Enabler ③ (soundness): a fn value carried by an enum payload takes a **Move** value (an owned
/// array — the `http_stream` stand-in) BY VALUE, invoked through an INDIRECT call (the match-extracted
/// `f`). `#573` nulls the owned arg in the caller's frame after an indirect call, so the caller's drop
/// does not double-free. A 200k-iteration loop makes a double-free abort and a leak balloon RSS.
#[test]
fn move_arg_through_fn_variant_indirect_call_no_double_free() {
    if !backend_available() {
        return;
    }
    let src = "\
H { Consume(fn(array<i64>) -> i64) }\n\
fn total(xs: array<i64>) -> i64 = xs.len()\n\
fn drive(h: H, xs: array<i64>) -> i64 = match h { Consume(f) => f(xs) }\n\
fn main() -> i32 {\n\
  mut acc := 0\n\
  mut i := 0\n\
  r := loop {\n\
    if i >= 200000 { break acc }\n\
    h := H.Consume(total)\n\
    buf := [i, i + 1, i + 2].to_array()\n\
    acc = acc + drive(h, buf)\n\
    i = i + 1\n\
  }\n\
  return (r % 7) as i32\n\
}\n";
    // 200000 * 3 = 600000; 600000 % 7 = 2. Completion (not a signal exit) is the real assertion.
    let out = build_and_run("fnvar_movearg", src);
    assert_eq!(out.status.code(), Some(2), "600000 % 7 = 2; stderr {}", String::from_utf8_lossy(&out.stderr));
}

/// The Move arg IS consumed by the indirect call — using it afterward is a compile error, not a
/// silent second use of a freed handle.
#[test]
fn move_arg_used_after_indirect_call_rejected() {
    let src = "\
H { Consume(fn(array<i64>) -> i64) }\n\
fn total(xs: array<i64>) -> i64 = xs.len()\n\
fn drive(h: H, xs: array<i64>) -> i64 {\n\
  a := match h { Consume(f) => f(xs) }\n\
  b := xs.len()\n\
  return a + b\n\
}\n\
fn main() -> i32 = 0\n";
    assert!(
        check_errs("fnvar_moveuse", src),
        "using a moved array after an indirect call must be rejected"
    );
}

/// Payload-type checking still rejects a fn whose SIGNATURE does not match the declared variant — the
/// widening admits fn payloads, it does not make them structurally interchangeable.
#[test]
fn wrong_fn_signature_payload_rejected() {
    let src = "\
H { Respond(fn(i64) -> i64) }\n\
fn wrong(x: i64, y: i64) -> i64 = x + y\n\
fn main() -> i32 {\n\
  h := H.Respond(wrong)\n\
  return 0\n\
}\n";
    assert!(
        check_errs("fnvar_wrongsig", src),
        "a fn of the wrong signature must be rejected as the payload"
    );
}
