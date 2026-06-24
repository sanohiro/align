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
fn non_primitive_payload_rejected() {
    // S1b: payloads are primitive scalars only — `string` (owned) is rejected for now.
    assert!(check_errs("enum-strpayload", "Wrap { S(string) }\nfn main() -> i32 { return 0 }\n"));
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
