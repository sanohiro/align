//! Whole-struct **value** semantics (Slice 2 of `docs/impl/08-nested-structs.md`): reading a whole
//! (inner) struct into a local, passing a struct by value, returning a struct by value, and
//! struct-to-struct assignment. Plain-data structs are Copy, so these are sound memcpys. Lifted into
//! working order by the Slice 1 field-path generalization (`Field`/`Load`/`Store` handle a struct
//! value); these tests lock the behavior in across the SysV by-value ABI (mixed widths, floats,
//! nesting).

mod common;
use common::*;

#[test]
fn read_whole_inner_struct() {
    if !backend_available() {
        return;
    }
    // `p := l.a` binds a copy of the inner struct; reading its fields gives 5 + 6 = 11.
    let src = concat!(
        "Point { x: i64, y: i64 }\nLine { a: Point, b: Point }\n",
        "fn main() -> i32 {\n",
        "  l := Line{a: Point{x:5,y:6}, b: Point{x:7,y:8}}\n",
        "  p := l.a\n",
        "  return (p.x + p.y) as i32\n}\n",
    );
    assert_eq!(build_and_run("read-inner", src).status.code(), Some(11));
}

#[test]
fn struct_param_and_return_by_value() {
    if !backend_available() {
        return;
    }
    // Pass a struct by value into `sum`, and build one by value in `mk`; 3 + 4 = 7, then mk(9) → 18.
    let src = concat!(
        "Point { x: i64, y: i64 }\n",
        "fn sum(p: Point) -> i64 = p.x + p.y\n",
        "fn mk(v: i64) -> Point = Point{x: v, y: v}\n",
        "fn main() -> i32 {\n",
        "  a := sum(Point{x:3,y:4})\n",
        "  b := sum(mk(9))\n",
        "  return (a + b) as i32\n}\n",
    );
    assert_eq!(build_and_run("struct-param-ret", src).status.code(), Some(25));
}

#[test]
fn struct_to_struct_assignment() {
    if !backend_available() {
        return;
    }
    // `p = q` copies the whole struct value: 10 + 20 = 30.
    let src = concat!(
        "Point { x: i64, y: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut p := Point{x:1,y:2}\n",
        "  q := Point{x:10,y:20}\n",
        "  p = q\n",
        "  return (p.x + p.y) as i32\n}\n",
    );
    assert_eq!(build_and_run("struct-assign", src).status.code(), Some(30));
}

#[test]
fn nested_struct_by_value_param() {
    if !backend_available() {
        return;
    }
    // A nested struct passed by value; the callee reads its nested fields: 9 - 2 = 7.
    let src = concat!(
        "Point { x: i64, y: i64 }\nLine { a: Point, b: Point }\n",
        "fn span(l: Line) -> i64 = l.b.x - l.a.x\n",
        "fn main() -> i32 {\n",
        "  return span(Line{a: Point{x:2,y:0}, b: Point{x:9,y:0}}) as i32\n}\n",
    );
    assert_eq!(build_and_run("nested-byval", src).status.code(), Some(7));
}

#[test]
fn mixed_width_and_float_struct_by_value() {
    if !backend_available() {
        return;
    }
    // Mixed-width fields (i8/i32/i64/i16) and a float struct exercise the by-value ABI classification.
    let src = concat!(
        "R { a: i8, b: i32, c: i64, d: i16 }\nV { x: f64, y: f64 }\n",
        "fn tot(r: R) -> i64 = (r.a as i64) + (r.b as i64) + r.c + (r.d as i64)\n",
        "fn add(a: V, b: V) -> V = V{x: a.x + b.x, y: a.y + b.y}\n",
        "fn main() -> i32 {\n",
        "  t := tot(R{a:1,b:2,c:3,d:4})\n",
        "  s := add(V{x:1.5,y:2.5}, V{x:3.0,y:4.0})\n",
        "  if s.x > 4.0 { if s.y > 6.0 { return t as i32 } }\n",
        "  return 0\n}\n",
    );
    assert_eq!(build_and_run("mixed-byval", src).status.code(), Some(10));
}

#[test]
fn struct_returned_then_mutated() {
    if !backend_available() {
        return;
    }
    // Return a struct by value, bind it to a mutable local, mutate a field, and read it: 10 + 5 = 15.
    let src = concat!(
        "Point { x: i64, y: i64 }\n",
        "fn mk(v: i64) -> Point = Point{x: v, y: v}\n",
        "fn main() -> i32 {\n",
        "  mut p := mk(5)\n",
        "  p.x = 10\n",
        "  return (p.x + p.y) as i32\n}\n",
    );
    assert_eq!(build_and_run("returned-mutated", src).status.code(), Some(15));
}
