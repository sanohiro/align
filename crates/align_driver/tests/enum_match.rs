//! Sum types + exhaustive `match` (slice S1a — tag-only variants). A keyword-less sum type
//! `Color { Red, Green, Blue }`, `Type.Variant` construction, and a mandatory-exhaustive `match`
//! expression. Payloads (S1b) and `match` on `Option`/`Result` (S3) are later slices.

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
fn match_over_all_variants() {
    if !backend_available() {
        return;
    }
    // An enum passed to a function, matched exhaustively; `Type.Variant` constructs the value.
    let src = "Color { Red, Green, Blue }\nfn code(c: Color) -> i64 = match c {\n  Red => 1,\n  Green => 2,\n  Blue => 3,\n}\nfn main() -> i32 {\n  c := Color.Green\n  return code(c)\n}\n";
    let out = build_and_run("enum-allvariants", src);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn wildcard_arm() {
    if !backend_available() {
        return;
    }
    // A `_` wildcard covers the uncovered variants; `match` is an expression.
    let src = "Dir { North, South, East, West }\nfn main() -> i32 {\n  d := Dir.West\n  return match d {\n    North => 10,\n    _ => 99,\n  }\n}\n";
    let out = build_and_run("enum-wildcard", src);
    assert_eq!(out.status.code(), Some(99));
}

#[test]
fn enum_returned_and_rematched() {
    if !backend_available() {
        return;
    }
    // A function returns an enum value; the caller matches it. Exercises enum-typed params/returns.
    let src = "State { On, Off }\nfn toggle(s: State) -> State = match s {\n  On => State.Off,\n  Off => State.On,\n}\nfn main() -> i32 {\n  return match toggle(State.Off) {\n    On => 7,\n    Off => 0,\n  }\n}\n";
    let out = build_and_run("enum-roundtrip", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn payload_construct_and_match_bind() {
    if !backend_available() {
        return;
    }
    // Multi-arity scalar payloads (`Both(i32,i32)`, `One(i32)`) + tag-only (`Zero`) in one enum;
    // construction with args and a `match` binding each payload positionally.
    let src = "Pair { Both(i32, i32), One(i32), Zero }\nfn val(p: Pair) -> i32 = match p {\n  Both(a, b) => a + b,\n  One(x)     => x,\n  Zero       => 0,\n}\nfn main() -> i32 {\n  return val(Pair.Both(30, 12)) + val(Pair.One(0)) + val(Pair.Zero)\n}\n";
    let out = build_and_run("enum-payload", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn single_payload_option_like() {
    if !backend_available() {
        return;
    }
    let src = "Maybe { Nothing, Just(i32) }\nfn get(m: Maybe) -> i32 = match m {\n  Nothing => -1,\n  Just(n) => n,\n}\nfn main() -> i32 {\n  return get(Maybe.Just(42)) - get(Maybe.Nothing) - 1\n}\n";
    let out = build_and_run("enum-maybe", src);
    assert_eq!(out.status.code(), Some(42)); // 42 - (-1) - 1
}

#[test]
fn float_payload_type_checks() {
    // `f64` payloads + a `match` binding used in float arithmetic — type-checks (no backend needed).
    let src = "Shape { Circle(f64), Rect(f64, f64) }\nfn area(s: Shape) -> f64 = match s {\n  Circle(r)  => 3.0 * r * r,\n  Rect(w, h) => w * h,\n}\nfn main() -> i32 {\n  x := area(Shape.Rect(3.0, 4.0))\n  return 0\n}\n";
    assert!(!check_errs("enum-float", src));
}

#[test]
fn ctor_arity_mismatch_rejected() {
    assert!(check_errs(
        "enum-ctor-arity",
        "Shape { Circle(f64) }\nfn main() -> i32 {\n  s := Shape.Circle(1.0, 2.0)\n  return 0\n}\n"
    ));
}

#[test]
fn no_paren_payload_ctor_rejected() {
    // A payload variant cannot be constructed bare (`Shape.Circle`); it needs its argument.
    assert!(check_errs(
        "enum-noparen",
        "Shape { Circle(f64) }\nfn main() -> i32 {\n  s := Shape.Circle\n  return 0\n}\n"
    ));
}

#[test]
fn binding_count_mismatch_rejected() {
    assert!(check_errs(
        "enum-bindcount",
        "Shape { Circle(f64) }\nfn main() -> f64 {\n  s := Shape.Circle(1.0)\n  return match s { Circle => 0.0 }\n}\n"
    ));
}

#[test]
fn duplicate_binding_rejected() {
    // Two bindings with the same name in one pattern (`Both(w, w)`) would silently shadow.
    assert!(check_errs(
        "enum-dupbind",
        "P { Both(i32, i32) }\nfn main() -> i32 {\n  p := P.Both(1, 2)\n  return match p { Both(w, w) => w }\n}\n"
    ));
}

#[test]
fn builtin_error_categories_and_match() {
    if !backend_available() {
        return;
    }
    // 4b-2: the canonical `Error` is a sum type — `Error.NotFound` / `Error.Code(c)` construct it,
    // and `match` discriminates the categories. Here the error is matched into an exit code.
    let src = "fn check(n: i32) -> Result<i32, Error> {\n  if n == 0 { return Err(Error.NotFound) }\n  if n < 0  { return Err(Error.Code(n)) }\n  return Ok(n)\n}\nfn describe(r: Result<i32, Error>) -> i32 = match r {\n  Ok(v)  => v,\n  Err(e) => match e {\n    NotFound => 100,\n    Invalid  => 101,\n    Denied   => 102,\n    Code(c)  => c,\n  },\n}\nfn main() -> i32 {\n  return describe(check(5)) + describe(check(0))\n}\n";
    let out = build_and_run("error-categories", src);
    assert_eq!(out.status.code(), Some(105)); // 5 + NotFound(100)
}

#[test]
fn error_code_maps_to_exit_code() {
    if !backend_available() {
        return;
    }
    // `Err(Error.Code(c))` propagated out of main → exit code `c` (the payload). `error(c)` is sugar.
    let src = "fn f() -> Result<(), Error> { return Err(error(42)) }\nfn main() -> Result<(), Error> {\n  f()?\n  return Ok(())\n}\n";
    let out = build_and_run("error-exit-code", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn error_category_maps_to_distinct_exit_code() {
    if !backend_available() {
        return;
    }
    // A categorical `Error` propagated out of main → a small distinct nonzero code (tag + 1).
    let src = "fn f() -> Result<(), Error> { return Err(Error.Denied) }\nfn main() -> Result<(), Error> {\n  f()?\n  return Ok(())\n}\n";
    let out = build_and_run("error-category-exit", src);
    assert_eq!(out.status.code(), Some(3)); // Denied tag 2 → exit 3
}

#[test]
fn error_is_reserved_type_name() {
    assert!(check_errs("error-reserved", "Error { Oops }\nfn main() -> i32 { return 0 }\n"));
}

#[test]
fn map_err_converts_then_propagates() {
    if !backend_available() {
        return;
    }
    // 4b-3: explicit error conversion. `inner().map_err(f)?` turns a `Result<_, MyErr>` into
    // `Result<_, Error>` and propagates — no implicit `?` coercion (that would be hidden).
    let src = "MyErr { Bad, Worse }\nfn to_error(e: MyErr) -> Error = match e {\n  Bad   => Error.Code(7),\n  Worse => Error.Code(9),\n}\nfn inner(n: i32) -> Result<i32, MyErr> {\n  if n < 0 { return Err(MyErr.Worse) }\n  return Ok(n)\n}\nfn outer(n: i32) -> Result<i32, Error> {\n  v := inner(n).map_err(to_error)?\n  return Ok(v + 1)\n}\nfn main() -> Result<(), Error> {\n  print(outer(5)?)\n  return Ok(())\n}\n";
    let out = build_and_run("maperr-ok", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n");
}

#[test]
fn map_err_error_path_propagates_converted() {
    if !backend_available() {
        return;
    }
    // The converted error propagates: `MyErr.Worse` → `Error.Code(9)` → exit 9, nothing printed.
    let src = "MyErr { Bad, Worse }\nfn to_error(e: MyErr) -> Error = match e {\n  Bad   => Error.Code(7),\n  Worse => Error.Code(9),\n}\nfn inner(n: i32) -> Result<i32, MyErr> {\n  if n < 0 { return Err(MyErr.Worse) }\n  return Ok(n)\n}\nfn outer(n: i32) -> Result<i32, Error> {\n  v := inner(n).map_err(to_error)?\n  return Ok(v)\n}\nfn main() -> Result<(), Error> {\n  print(outer(-1)?)\n  return Ok(())\n}\n";
    let out = build_and_run("maperr-err", src);
    assert_eq!(out.status.code(), Some(9));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn map_err_owned_ok_payload_no_double_free() {
    if !backend_available() {
        return;
    }
    // `map_err` on a `Result<string, E>` moves the owned `Ok` payload through; the source local's
    // slot must be nulled so the exit cleanup doesn't double-free it.
    let src = "MyErr { Bad }\nfn to_error(e: MyErr) -> Error = Error.Code(1)\nfn load(ok: bool) -> Result<string, MyErr> {\n  if ok { return Ok(\"hello\".clone()) }\n  return Err(MyErr.Bad)\n}\nfn main() -> Result<(), Error> {\n  s := load(true).map_err(to_error)?\n  print(s.len())\n  return Ok(())\n}\n";
    let out = build_and_run("maperr-owned", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}

#[test]
fn map_err_wrong_function_signature_rejected() {
    assert!(check_errs(
        "maperr-badsig",
        "MyErr { Bad }\nfn inner() -> Result<i32, MyErr> { return Err(MyErr.Bad) }\nfn wrong(x: i32) -> Error = Error.NotFound\nfn outer() -> Result<i32, Error> { return inner().map_err(wrong) }\nfn main() -> i32 { return 0 }\n"
    ));
}

#[test]
fn enum_box_payload_rejected() {
    // A sum type is a Copy scalar (so it's an Option/Result payload), but not a box payload —
    // both the `box<Enum>` annotation and `heap.new(Enum.X)` must be rejected, not panic codegen.
    assert!(check_errs("enum-box-ann", "C { R }\nfn f(b: box<C>) -> i32 = 0\nfn main() -> i32 { return 0 }\n"));
    assert!(check_errs(
        "enum-box-new",
        "C { R }\nfn main() -> i32 {\n  b := arena { heap.new(C.R) }\n  return 0\n}\n"
    ));
}

#[test]
fn non_primitive_payload_rejected() {
    // S1b: payloads are primitive scalars only — `string` (owned) is rejected for now.
    assert!(check_errs("enum-strpayload", "Wrap { S(string) }\nfn main() -> i32 { return 0 }\n"));
}

#[test]
fn match_on_option() {
    if !backend_available() {
        return;
    }
    // S3: `match` works on the builtin `Option`, binding the `Some` payload.
    let src = "fn unwrap_or(o: Option<i32>, d: i32) -> i32 = match o {\n  Some(x) => x,\n  None    => d,\n}\nfn main() -> i32 {\n  return unwrap_or(Some(40), 0) + unwrap_or(None, 2)\n}\n";
    let out = build_and_run("match-option", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn match_on_result_arm_order_and_wildcard() {
    if !backend_available() {
        return;
    }
    // `match` on `Result`; `Err`-first arm order (the 2-way branch is order-independent) and a
    // wildcard covering the error case.
    let src = "fn code(r: Result<i32, Error>) -> i32 = match r {\n  Err(e) => 99,\n  Ok(v)  => v,\n}\nfn first_ok(r: Result<i32, Error>) -> i32 = match r {\n  Ok(v) => v,\n  _     => -1,\n}\nfn main() -> i32 {\n  return code(Ok(20)) + first_ok(Ok(22))\n}\n";
    let out = build_and_run("match-result", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn match_option_non_exhaustive_rejected() {
    assert!(check_errs(
        "match-opt-nonexhaustive",
        "fn f(o: Option<i32>) -> i32 = match o { Some(x) => x }\nfn main() -> i32 { return f(None) }\n"
    ));
}

#[test]
fn enum_as_result_error_type() {
    if !backend_available() {
        return;
    }
    // 4b foundation: a user sum type as the `E` in `Result<T, E>` — construct `Err(MyError.X)`,
    // `match` the Result, and `match` the error enum itself.
    let src = "MyError { NotFound, Io(i32) }\nfn find(ok: bool) -> Result<i32, MyError> {\n  if ok { return Ok(42) }\n  return Err(MyError.Io(5))\n}\nfn handle(r: Result<i32, MyError>) -> i32 = match r {\n  Ok(v)  => v,\n  Err(e) => match e { NotFound => -1, Io(code) => code },\n}\nfn main() -> i32 {\n  return handle(find(true)) + handle(find(false))\n}\n";
    let out = build_and_run("enum-result-err", src);
    assert_eq!(out.status.code(), Some(47));
}

#[test]
fn enum_error_propagated_with_question() {
    if !backend_available() {
        return;
    }
    // `?` propagates a user enum error type (same `E` through the call chain).
    let src = "E { Bad }\nfn inner(ok: bool) -> Result<i32, E> {\n  if ok { return Ok(7) }\n  return Err(E.Bad)\n}\nfn outer(ok: bool) -> Result<i32, E> {\n  v := inner(ok)?\n  return Ok(v + 1)\n}\nfn main() -> i32 = match outer(true) { Ok(v) => v, Err(e) => -1 }\n";
    let out = build_and_run("enum-result-q", src);
    assert_eq!(out.status.code(), Some(8));
}

#[test]
fn enum_as_option_payload() {
    if !backend_available() {
        return;
    }
    let src = "Color { Red, Green }\nfn pick(some: bool) -> Option<Color> {\n  if some { return Some(Color.Green) }\n  return None\n}\nfn main() -> i32 = match pick(true) {\n  Some(c) => match c { Red => 1, Green => 2 },\n  None    => 0,\n}\n";
    let out = build_and_run("enum-option", src);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn non_exhaustive_rejected() {
    // Every variant must be covered (or a `_`); a missing variant is a compile error.
    assert!(check_errs(
        "enum-nonexhaustive",
        "Color { Red, Green, Blue }\nfn main() -> i32 {\n  c := Color.Red\n  return match c { Red => 1, Green => 2 }\n}\n"
    ));
}

#[test]
fn unknown_variant_in_pattern_rejected() {
    assert!(check_errs(
        "enum-unknown-pat",
        "Color { Red, Green }\nfn main() -> i32 {\n  c := Color.Red\n  return match c { Red => 1, Purple => 2 }\n}\n"
    ));
}

#[test]
fn unknown_variant_in_construction_rejected() {
    assert!(check_errs(
        "enum-unknown-ctor",
        "Color { Red, Green }\nfn main() -> i32 {\n  c := Color.Cyan\n  return 0\n}\n"
    ));
}

#[test]
fn match_on_non_enum_rejected() {
    assert!(check_errs(
        "enum-nonenum",
        "fn main() -> i32 {\n  x: i32 := 5\n  return match x { Red => 1 }\n}\n"
    ));
}

#[test]
fn arm_type_mismatch_rejected() {
    assert!(check_errs(
        "enum-armty",
        "Color { Red, Green }\nfn main() -> i32 {\n  c := Color.Red\n  return match c { Red => 1, Green => true }\n}\n"
    ));
}

#[test]
fn duplicate_variant_rejected() {
    assert!(check_errs("enum-dupvariant", "Color { Red, Red }\nfn main() -> i32 { return 0 }\n"));
}

#[test]
fn duplicate_arm_rejected() {
    assert!(check_errs(
        "enum-duparm",
        "Color { Red, Green }\nfn main() -> i32 {\n  c := Color.Red\n  return match c { Red => 1, Red => 2, Green => 3 }\n}\n"
    ));
}
