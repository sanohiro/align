//! Nested struct fields (Slice 1 of `docs/impl/08-nested-structs.md`): a scalar-only struct can be a
//! field of another struct; construct (incl. nested literals + a struct-valued field init), read,
//! and write reach depth N via a field path. Owned (`str`-bearing) and recursive nesting are
//! rejected in sema.

mod common;
use common::*;

#[test]
fn nested_struct_construct_read_write() {
    if !backend_available() {
        return;
    }
    // Construct with both a struct-valued field (`a: p`) and an inline nested literal (`b: Point{…}`),
    // write a nested field, then read two nested fields: 100 + 4 = 104.
    let src = concat!(
        "Point { x: i64, y: i64 }\n",
        "Line  { a: Point, b: Point }\n",
        "fn main() -> i32 {\n",
        "  p := Point{x: 1, y: 2}\n",
        "  mut l := Line{a: p, b: Point{x: 3, y: 4}}\n",
        "  l.a.x = 100\n",
        "  return (l.a.x + l.b.y) as i32\n",
        "}\n",
    );
    let out = build_and_run("nested-struct", src);
    assert_eq!(out.status.code(), Some(104));
}

#[test]
fn three_level_nesting() {
    if !backend_available() {
        return;
    }
    // Depth-3 path read + write: c.b.a.v.
    let src = concat!(
        "A { v: i64 }\nB { a: A }\nC { b: B }\n",
        "fn main() -> i32 {\n",
        "  mut c := C{b: B{a: A{v: 5}}}\n",
        "  c.b.a.v = 42\n",
        "  return c.b.a.v as i32\n",
        "}\n",
    );
    let out = build_and_run("nested-3", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn assign_whole_nested_struct_literal() {
    if !backend_available() {
        return;
    }
    // `l.a = Point{…}` — assign a whole inner struct from a literal (expanded in place).
    let src = concat!(
        "Point { x: i64, y: i64 }\nLine { a: Point, b: Point }\n",
        "fn main() -> i32 {\n",
        "  mut l := Line{a: Point{x: 1, y: 2}, b: Point{x: 3, y: 4}}\n",
        "  l.a = Point{x: 10, y: 20}\n",
        "  return (l.a.x + l.a.y) as i32\n",
        "}\n",
    );
    let out = build_and_run("nested-assign", src);
    assert_eq!(out.status.code(), Some(30));
}

#[test]
fn owned_nested_struct_field_now_accepted() {
    // Slice 3 lifted the Slice-1 restriction: a nested struct may carry an owned `string` (making
    // the outer struct a Move type with a recursive Drop) or a `str` borrow (Copy, region-tracked).
    // Runtime drop behavior is covered in `owned_structs.rs`; here we only assert it type-checks.
    assert!(!check_errs("owned-nested-string", "Inner { s: string }\nOuter { i: Inner }\nfn main() -> i32 { return 0 }\n"));
    assert!(!check_errs("owned-nested-str", "Inner { s: str }\nOuter { i: Inner }\nfn main() -> i32 { return 0 }\n"));
}

#[test]
fn recursive_struct_field_rejected() {
    // A self-referential struct field is rejected (infinite layout; needs a `box` indirection).
    assert!(check_errs("recursive-struct", "Node { next: Node, v: i64 }\nfn main() -> i32 { return 0 }\n"));
}

#[test]
fn top_level_str_field_still_ok() {
    if !backend_available() {
        return;
    }
    // A struct may have a direct `str` borrow field (Copy, region-tracked) — unchanged by Slice 3.
    let src = concat!(
        "Foo { s: str, n: i64 }\n",
        "fn main() -> i32 {\n  f := Foo{s: \"hi\", n: 7}\n  return f.n as i32\n}\n",
    );
    let out = build_and_run("top-str-field", src);
    assert_eq!(out.status.code(), Some(7));
}
