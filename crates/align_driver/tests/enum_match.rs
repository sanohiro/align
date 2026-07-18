//! Sum types + exhaustive `match` (slice S1a — tag-only variants). A keyword-less sum type
//! `Color { Red, Green, Blue }`, `Type.Variant` construction, and a mandatory-exhaustive `match`
//! expression. Payloads (S1b) and `match` on `Option`/`Result` (S3) are later slices.


mod common;
use common::*;

#[test]
fn match_over_all_variants() {
    if !backend_available() {
        return;
    }
    // An enum passed to a function, matched exhaustively; `Type.Variant` constructs the value.
    let src = "Color { Red, Green, Blue }\nfn code(c: Color) -> i32 = match c {\n  Red => 1,\n  Green => 2,\n  Blue => 3,\n}\nfn main() -> i32 {\n  c := Color.Green\n  return code(c)\n}\n";
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
fn struct_variant_payload() {
    if !backend_available() {
        return;
    }
    // S2: a plain-data struct as a variant payload — construct, `match`-bind it, access its fields.
    let src = "Point { x: i32, y: i32 }\nShape { Dot(Point), Seg(Point, Point), Empty }\nfn area(s: Shape) -> i32 = match s {\n  Dot(p)    => p.x + p.y,\n  Seg(a, b) => (b.x - a.x) + (b.y - a.y),\n  Empty     => 0,\n}\nfn main() -> i32 {\n  return area(Shape.Seg(Point { x: 1, y: 2 }, Point { x: 5, y: 8 }))\n}\n";
    let out = build_and_run("struct-payload", src);
    assert_eq!(out.status.code(), Some(10));
}

#[test]
fn str_field_struct_payload_accepted() {
    // J1 lifted the old restriction: a `str`-bearing plain-data struct is now a legal variant
    // payload (the enum tracks its region — see `enum_str_bearing_struct_payload`). An OWNED (Move)
    // struct payload stays rejected (that is J2), covered by `non_primitive_payload_rejected`.
    assert!(!check_errs("str-struct-payload", "Name { s: str }\nTag { Named(Name) }\nfn main() -> i32 { return 0 }\n"));
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

// ---- J1: enum `str` payloads (region tracking) --------------------------------------------------

#[test]
fn enum_str_payload_construct_and_match() {
    if !backend_available() {
        return;
    }
    // A sum type may carry a `str`-view payload (J1). Construct, match, and use the bound `str`.
    let src = "Content { Text(str), Count(i64) }\n\
        fn main() -> i32 {\n  \
        a := Content.Text(\"hello\")\n  \
        b := Content.Count(42)\n  \
        n := match a {\n    Text(s) => s.len()\n    Count(k) => k\n  }\n  \
        m := match b {\n    Text(s) => s.len()\n    Count(k) => k\n  }\n  \
        return (n + m) as i32\n}\n";
    let out = build_and_run("enum-str-payload", src);
    assert_eq!(out.status.code(), Some(47)); // len("hello") 5 + 42
}

#[test]
fn enum_str_bearing_struct_payload() {
    if !backend_available() {
        return;
    }
    // A `str`-bearing plain-data struct is now a legal payload (J1: the enum tracks its region).
    let src = "Part { role: str, order: i64 }\n\
        Msg { One(Part), Empty(bool) }\n\
        fn main() -> i32 {\n  \
        m := Msg.One(Part{role: \"user\", order: 3})\n  \
        return match m {\n    One(p) => p.order as i32\n    Empty(b) => 0\n  }\n}\n";
    let out = build_and_run("enum-str-struct-payload", src);
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn enum_str_payload_cannot_escape_arena() {
    // J1 soundness: a sum type holding an arena `str` view is region-tracked, so it cannot escape
    // the arena that backs the view (else the match binding would be a use-after-free).
    assert!(check_errs(
        "enum-str-escape",
        "Content { Text(str), Count(i64) }\n\
         fn main() -> Result<(), Error> {\n  \
         x := 7\n  \
         c := arena {\n    s := template \"hi {x}\"\n    Content.Text(s)\n  }\n  \
         return Ok(())\n}\n"
    ));
}

#[test]
fn scalar_only_enum_still_returnable() {
    if !backend_available() {
        return;
    }
    // A scalar-only sum type is NOT region-tracked — it stays freely returnable across boundaries
    // (the J1 region change must not over-restrict plain enums).
    let src = "Tag { A(i64), B(bool) }\n\
        fn make() -> Tag = Tag.A(5)\n\
        fn main() -> i32 {\n  \
        return match make() {\n    A(n) => n as i32\n    B(b) => 0\n  }\n}\n";
    let out = build_and_run("enum-scalar-returnable", src);
    assert_eq!(out.status.code(), Some(5));
}

#[test]
fn plain_struct_payload_enum_returnable() {
    if !backend_available() {
        return;
    }
    // Precision (the J1 region change must not over-restrict): a plain-data (no-`str`) struct
    // payload borrows nothing, so its enum is `region_of` Static and freely returnable across a
    // function boundary, even though `tracks_region(Struct)` is conservatively true.
    let src = "Point { x: i64, y: i64 }\n\
        Shape { P(Point), Empty(bool) }\n\
        fn make() -> Shape = Shape.P(Point{x: 3, y: 4})\n\
        fn main() -> i32 {\n  \
        return match make() {\n    P(p) => (p.x + p.y) as i32\n    Empty(b) => 0\n  }\n}\n";
    let out = build_and_run("enum-plain-struct-returnable", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn str_bearing_struct_payload_cannot_escape() {
    // Soundness through a struct payload: a `str`-bearing struct payload's inner view cannot escape
    // the arena backing it (the match binding `q.role` is region-tied to the enum's region).
    assert!(check_errs(
        "enum-str-struct-escape",
        "Part { role: str }\n\
         Msg { One(Part), Empty(bool) }\n\
         fn main() -> Result<(), Error> {\n  \
         x := 1\n  \
         r := arena {\n    p := Part{role: template \"u{x}\"}\n    m := Msg.One(p)\n    \
         match m {\n      One(q) => q.role\n      Empty(b) => \"x\"\n    }\n  }\n  \
         return Ok(())\n}\n"
    ));
}

// ---- J1b: enum as a struct field (the JSON union language prerequisite) --------------------------

#[test]
fn struct_with_enum_field_construct_and_match() {
    if !backend_available() {
        return;
    }
    // A struct field may now be a sum type (`Msg { st: Status }`, J1b) — the `Message { content: … }`
    // shape the JSON union needs. Construct the struct with an enum value, read the field, and match.
    let src = "Status { Ok, Bad(i32) }\n\
        Msg { code: i64, st: Status }\n\
        fn eval(m: Msg) -> i64 = match m.st {\n  Ok => m.code\n  Bad(n) => m.code + n as i64\n}\n\
        fn main() -> i32 {\n  \
        a := Msg { code: 40, st: Status.Ok }\n  \
        b := Msg { code: 40, st: Status.Bad(2) }\n  \
        return (eval(a) + eval(b)) as i32\n}\n";
    let out = build_and_run("struct-enum-field", src);
    assert_eq!(out.status.code(), Some(82)); // 40 + (40+2)
}

#[test]
fn struct_enum_field_str_and_object_payloads_in_scope() {
    if !backend_available() {
        return;
    }
    // A struct's enum field over every J1b payload shape used in-scope: `str` view (Named), a struct /
    // object payload (At), and a tag-only/scalar variant (Empty). The `str` view stays inside the
    // arena that backs it, so there is no escape.
    let src = "Point { x: i64, y: i64 }\n\
        Shape { Named(str), At(Point), Empty }\n\
        Holder { s: Shape }\n\
        fn measure(h: Holder) -> i64 = match h.s {\n  \
        Named(name) => name.len() as i64\n  At(p) => p.x + p.y\n  Empty => 0\n}\n\
        fn main() -> i32 {\n  \
        arena {\n    n := template \"hello\"\n    \
        a := Holder { s: Shape.Named(n) }\n    \
        b := Holder { s: Shape.At(Point { x: 3, y: 4 }) }\n    \
        c := Holder { s: Shape.Empty }\n    \
        return (measure(a) + measure(b) + measure(c)) as i32\n  }\n}\n";
    let out = build_and_run("struct-enum-field-shapes", src);
    assert_eq!(out.status.code(), Some(12)); // len("hello") 5 + (3+4) + 0
}

#[test]
fn struct_with_scalar_enum_field_freely_returnable() {
    if !backend_available() {
        return;
    }
    // Precision: a struct whose enum field is scalar-only borrows nothing, so it is freely returnable
    // from an arena (the J1b field admission must not over-restrict — the region change is opt-in on a
    // `str`-bearing payload, matching `scalar_only_enum_still_returnable` at the enum level).
    let src = "Status { Ok, Bad(i32) }\n\
        Msg { code: i64, st: Status }\n\
        fn make() -> Msg = arena { Msg { code: 7, st: Status.Bad(3) } }\n\
        fn main() -> i32 {\n  \
        m := make()\n  \
        return match m.st {\n    Ok => m.code as i32\n    Bad(n) => (m.code as i32) + n\n  }\n}\n";
    let out = build_and_run("struct-scalar-enum-returnable", src);
    assert_eq!(out.status.code(), Some(10)); // 7 + 3
}

#[test]
fn struct_with_str_enum_field_cannot_escape_arena() {
    // Soundness: a struct holding a `str`-bearing enum field is input/arena-region-tied
    // (`struct_has_str_rec` grew an enum arm), so the struct cannot escape the arena backing the view —
    // else `w.c`'s `Text(view)` would be a use-after-free. Mirrors `enum_str_payload_cannot_escape_arena`
    // one wrapper deeper.
    assert!(check_errs(
        "struct-enum-field-escape",
        "Content { Text(str), Count(i64) }\n\
         Wrap { c: Content }\n\
         fn make(x: i64) -> Wrap {\n  \
         return arena {\n    s := template \"hi {x}\"\n    Wrap { c: Content.Text(s) }\n  }\n}\n\
         fn main() -> i32 {\n  w := make(5)\n  return 0\n}\n"
    ));
}

#[test]
fn struct_recursive_through_enum_field_rejected() {
    // A struct cannot contain itself through an enum field either (`Node { c: Wrap }`, `Wrap { N(Node) }`
    // is an infinite inline layout). Caught by the post-0c enum-aware acyclicity pass.
    assert!(check_errs(
        "struct-enum-cycle",
        "Node { c: Wrap }\n\
         Wrap { N(Node), Leaf(i64) }\n\
         fn main() -> i32 = 0\n"
    ));
}

// ---- J2: enum owned `array<T>` payloads + tag-switched drop (the enum becomes Move) --------------
// Standalone (non-JSON) coverage uses an `array<i64>` payload — the only owned array constructible
// outside `json.decode` (`.to_array()` over structs is deferred). The owned `array<Struct>` payload
// (`Parts(array<Part>)`, the multimodal `Content` union) is covered end-to-end in `m5.rs` (J2b).

#[test]
fn enum_owned_array_payload_construct_match_drop() {
    if !backend_available() {
        return;
    }
    // Construct a Move sum type from a bound owned array (moved into the variant — its source slot is
    // nulled so the exit tag-switched `Drop` frees the buffer exactly once). Match distinguishes the
    // variants; the owned variant is covered by a top-level `_` (its payload is never extracted, so
    // the enum owns it until its scope-end drop). A clean exit proves no double-free / leak (the
    // runtime aborts on allocator corruption).
    let src = "Content { Text(str), Nums(array<i64>) }\n\
        fn main() -> i32 {\n  \
        ns := [10, 20, 5].to_array()\n  \
        c := Content.Nums(ns)\n  \
        return match c {\n    Text(t) => t.len() as i32\n    _       => 0\n  }\n}\n";
    let out = build_and_run("enum-owned-arr", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn move_enum_returned_and_consumed() {
    if !backend_available() {
        return;
    }
    // A Move sum type is freely returnable (it owns its buffer — `region_of` Static, like `array`
    // itself), passed by value into a consumer that drops it, on both a scalar-array and a `str`
    // branch joined through an `if`. Exercises return-move, param-move-consume, and the if drop-flag
    // join; a clean exit + correct total proves single-free on every path.
    let src = "Content { Text(str), Nums(array<i64>) }\n\
        fn make(k: i64) -> Content {\n  \
        if k == 0 { return Content.Text(\"zero\") }\n  \
        return Content.Nums([k, k * 2, k * 3].to_array())\n}\n\
        fn size(c: Content) -> i64 = match c {\n  Text(t) => t.len() as i64\n  _       => 100\n}\n\
        fn main() -> i32 {\n  \
        a := make(0)\n  b := make(7)\n  return (size(a) + size(b)) as i32\n}\n";
    let out = build_and_run("move-enum-ret", src);
    assert_eq!(out.status.code(), Some(104)); // 4 ("zero") + 100
}

#[test]
fn move_enum_if_join_scalar_and_str_variant() {
    if !backend_available() {
        return;
    }
    // The two `if` arms install *different* Move-enum variants (an owned-array `Nums` and a borrowed
    // `str` `Text`) into one slot; the drop-flag join + tag-switched drop free whichever variant is
    // live on the taken path (an unconstructed path frees nothing). No leak / double-free either way.
    let src = "Content { Text(str), Nums(array<i64>) }\n\
        fn pick(k: i64) -> i32 {\n  \
        c := if k > 0 { Content.Nums([k].to_array()) } else { Content.Text(\"neg\") }\n  \
        return match c {\n    Text(t) => t.len() as i32\n    _       => 42\n  }\n}\n\
        fn main() -> i32 = pick(3) + pick(-1)\n";
    let out = build_and_run("move-enum-ifjoin", src);
    assert_eq!(out.status.code(), Some(45)); // 42 (Nums) + 3 ("neg")
}

#[test]
fn scalar_only_and_str_enum_stay_non_move() {
    if !backend_available() {
        return;
    }
    // Precision guard: adding the Move arm must not over-classify. A scalar-only enum and a `str`-view
    // enum own no buffer, so they stay Copy (freely constructed/matched with no drop) — verified by a
    // clean run distinguishing both.
    let src = "Tag { A(i64), B }\n\
        Name { N(str), Empty }\n\
        fn main() -> i32 {\n  \
        t := Tag.A(3)\n  n := Name.N(\"hi\")\n  \
        x := match t { A(v) => v, B => 0 }\n  \
        y := match n { N(s) => s.len() as i64, Empty => 0 }\n  \
        return (x + y) as i32\n}\n";
    let out = build_and_run("enum-nonmove", src);
    assert_eq!(out.status.code(), Some(5)); // 3 + 2
}

#[test]
fn binding_owned_enum_payload_in_match_rejected() {
    // Binding a Move (owned `array`) payload in a match arm is deferred (whether it moves the buffer
    // out of the scrutinee or borrows it has no consumer yet, and a naive owned binding double-frees).
    // The value is used whole (move / `json.encode`) or the variant is covered by a `_` arm.
    assert!(check_errs(
        "enum-bind-owned",
        "Content { Text(str), Nums(array<i64>) }\n\
         fn main() -> i32 = match Content.Nums([1].to_array()) {\n  Nums(ns) => ns.len() as i32\n  Text(t)  => -1\n}\n"
    ));
}

#[test]
fn move_enum_struct_field_rejected() {
    // A Move sum-type field (an owned-array payload variant) is deferred — an owned enum struct
    // field's drop-as-a-field has no consumer yet. (A non-Move enum field stays allowed, J1b.)
    assert!(check_errs(
        "enum-movefield",
        "Content { Text(str), Nums(array<i64>) }\n\
         Msg { c: Content }\n\
         fn main() -> i32 = 0\n"
    ));
}

#[test]
fn array_string_and_move_struct_enum_payloads_rejected() {
    // The owned-array payload element must be non-owned (one flat free): `array<string>` (a
    // per-element deep free) and `array<Move-struct>` are deferred, rejected cleanly rather than
    // leaked per element — the exact Slice-C rule, now for enum payloads.
    assert!(check_errs(
        "enum-arrstring",
        "E { V(array<string>), Z }\nfn main() -> i32 = 0\n"
    ));
    assert!(check_errs(
        "enum-arrmovestruct",
        "Owned { s: string }\nE { V(array<Owned>), Z }\nfn main() -> i32 = 0\n"
    ));
}

#[test]
fn move_enum_option_result_payload_rejected() {
    // A Move sum type as an `Option`/`Result` payload is deferred (the drop machinery frees a flat
    // `{ptr,len}` for a payload, not a tag-switched enum drop; `payload_is_move` is table-free and
    // cannot see the Move enum, so admitting it would leak). Rejected at the `Some`/`Ok`/`Err` wrap
    // site — the sole origin of such a value. (A scalar-only enum payload stays allowed.)
    assert!(check_errs(
        "moveenum-some",
        "Content { Text(str), Nums(array<i64>) }\n\
         fn main() -> i32 {\n  o := Some(Content.Nums([1].to_array()))\n  return match o { Some(_) => 0, None => 1 }\n}\n"
    ));
    assert!(check_errs(
        "moveenum-err",
        "Content { Text(str), Nums(array<i64>) }\n\
         fn f() -> Result<i64, Content> = Err(Content.Nums([1].to_array()))\n\
         fn main() -> i32 = 0\n"
    ));
}

#[test]
fn move_enum_array_element_rejected() {
    // A fixed array of a Move sum type is deferred — its per-element tag-switched drop has no
    // consumer, and a fixed `array<Enum>` is not a droppable struct-array, so it would leak each
    // element's buffer. (An owned `array<Enum>` isn't even expressible — enums are not array elements.)
    assert!(check_errs(
        "moveenum-arrelem",
        "Content { Text(str), Nums(array<i64>) }\n\
         fn main() -> i32 {\n  xs := [Content.Nums([1].to_array()), Content.Text(\"a\")]\n  return 0\n}\n"
    ));
}

#[test]
fn move_enum_lambda_capture_rejected() {
    // Capturing a Move sum type by value into a lambda would copy-capture and double-drop it —
    // rejected like any owned capture (`ty_capture_is_move`).
    assert!(check_errs(
        "moveenum-capture",
        "Content { Text(str), Nums(array<i64>) }\n\
         fn main() -> i32 {\n  c := Content.Nums([1].to_array())\n  ys := [1, 2, 3].par_map(fn s { match c { Text(t) => s, _ => s + 1 } })\n  return 0\n}\n"
    ));
}

#[test]
fn nested_array_and_soa_enum_payload_rejected_not_panic() {
    // A non-representable owned-array element — a nested `array<array<T>>` (no payload `Scalar`) — is
    // rejected with a clean diagnostic, never a compiler panic (Gate 3: diagnose, don't crash).
    assert!(check_errs(
        "enum-nestedarr",
        "E { V(array<array<i64>>), Z }\nfn main() -> i32 = 0\n"
    ));
}
