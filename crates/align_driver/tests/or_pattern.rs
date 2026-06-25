//! Sum types — slice S4 (or-patterns). An arm pattern `A | B | ...` matches if the scrutinee is
//! any of the listed variants; it binds nothing (a payload variant may appear, its payload is
//! simply not bound). Bindings in an or-pattern, unknown / duplicate variants, are rejected.


mod common;
use common::*;

#[test]
fn or_pattern_with_wildcard() {
    if !backend_available() {
        return;
    }
    // `Red | Yellow` shares one arm; `_` covers the rest.
    let src = "Signal { Red, Yellow, Green, Off }\nfn rank(s: Signal) -> i32 = match s {\n  Red | Yellow => 1\n  Green => 2\n  _ => 0\n}\nfn main() -> i32 = rank(Signal.Yellow) + rank(Signal.Green) + rank(Signal.Off)\n";
    let out = build_and_run("or-wild", src);
    assert_eq!(out.status.code(), Some(3)); // 1 + 2 + 0
}

#[test]
fn or_pattern_is_exhaustive_cover() {
    if !backend_available() {
        return;
    }
    // Two or-patterns partition all four variants — no `_` needed, still exhaustive.
    let src = "Dir { N, E, S, W }\nfn horiz(d: Dir) -> i32 = match d {\n  E | W => 1\n  N | S => 0\n}\nfn main() -> i32 = horiz(Dir.W) + horiz(Dir.S)\n";
    let out = build_and_run("or-cover", src);
    assert_eq!(out.status.code(), Some(1)); // 1 + 0
}

#[test]
fn or_pattern_ignores_payload() {
    if !backend_available() {
        return;
    }
    // A payload-bearing variant may appear in an or-pattern; its payload is simply not bound.
    let src = "Shape { Circle(f64), Rect(f64, f64), Dot }\nfn has_area(sh: Shape) -> i32 = match sh {\n  Circle | Rect => 1\n  Dot => 0\n}\nfn main() -> i32 = has_area(Shape.Rect(2.0, 3.0)) + has_area(Shape.Dot)\n";
    let out = build_and_run("or-payload", src);
    assert_eq!(out.status.code(), Some(1)); // 1 + 0
}

#[test]
fn or_pattern_three_alternatives() {
    if !backend_available() {
        return;
    }
    let src = "Day { Mon, Tue, Wed, Thu, Fri, Sat, Sun }\nfn weekend(d: Day) -> i32 = match d {\n  Sat | Sun => 1\n  Mon | Tue | Wed | Thu | Fri => 0\n}\nfn main() -> i32 = weekend(Day.Sun) + weekend(Day.Wed)\n";
    let out = build_and_run("or-three", src);
    assert_eq!(out.status.code(), Some(1)); // 1 + 0
}

#[test]
fn unknown_variant_in_or_rejected() {
    let src = "Dir { N, E, S, W }\nfn f(d: Dir) -> i32 = match d {\n  N | Nope => 1\n  _ => 0\n}\nfn main() -> i32 = f(Dir.N)\n";
    assert!(check_errs("or-unknown", src));
}

#[test]
fn duplicate_variant_in_or_rejected() {
    let src = "Dir { N, E, S, W }\nfn f(d: Dir) -> i32 = match d {\n  N | N => 1\n  _ => 0\n}\nfn main() -> i32 = f(Dir.N)\n";
    assert!(check_errs("or-dup", src));
}

#[test]
fn duplicate_across_or_and_simple_rejected() {
    // `N` covered by both the or-pattern and a later simple arm.
    let src = "Dir { N, E, S, W }\nfn f(d: Dir) -> i32 = match d {\n  N | E => 1\n  N => 2\n  _ => 0\n}\nfn main() -> i32 = f(Dir.N)\n";
    assert!(check_errs("or-dup2", src));
}

#[test]
fn binding_in_or_pattern_rejected() {
    // An or-pattern cannot bind a payload.
    let src = "Shape { Circle(f64), Rect(f64, f64) }\nfn f(sh: Shape) -> i32 = match sh {\n  Circle(r) | Rect => 1\n}\nfn main() -> i32 = f(Shape.Rect(1.0, 2.0))\n";
    assert!(check_errs("or-bind", src));
}

#[test]
fn non_exhaustive_with_or_rejected() {
    // `E | W` leaves `S` uncovered, no `_`.
    let src = "Dir { N, E, S, W }\nfn f(d: Dir) -> i32 = match d {\n  N => 0\n  E | W => 1\n}\nfn main() -> i32 = f(Dir.S)\n";
    assert!(check_errs("or-nonexhaustive", src));
}
