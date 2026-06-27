//! Cross-module field / payload types (`field: other.Type`) — Slice 5 of
//! `docs/impl/08-nested-structs.md`, the module-system "B3" leftover. A struct field, enum payload,
//! or generic-template member may name a `pub` type exported by an imported module. The resolver
//! already handled `mod.Type` in function signatures / `let`s; this threads each module's imports
//! into the type-declaration passes (0b/0c, templates) too. Reaches only `pub` types of `import`ed
//! modules — the same visibility rule as functions.

mod common;
use common::*;

const GEOM: &str = "module geom\npub Point { x: i64, y: i64 }\n";

#[test]
fn struct_field_of_imported_type() {
    if !backend_available() {
        return;
    }
    // `Line` has fields of the imported `geom.Point`; construct (incl. a `geom.Point{…}` literal) and
    // read nested fields across the module boundary: 1 + 4 = 5.
    let main = concat!(
        "module main\n",
        "import geom\n",
        "Line { a: geom.Point, b: geom.Point }\n",
        "fn main() -> i32 {\n",
        "  l := Line{a: geom.Point{x: 1, y: 2}, b: geom.Point{x: 3, y: 4}}\n",
        "  return (l.a.x + l.b.y) as i32\n",
        "}\n",
    );
    let out = build_and_run_multi("xmod-field", &[("geom.align", GEOM), ("main.align", main)], "main.align");
    assert_eq!(out.status.code(), Some(5));
}

#[test]
fn struct_field_of_imported_move_struct() {
    if !backend_available() {
        return;
    }
    // An imported struct that owns a `string` (a Move type) as a field makes the outer a Move type
    // too — its recursive Drop frees the imported struct's buffer. Borrow the owned field across the
    // boundary: len("abcd") = 4, dropped once.
    let rec = "module rec\npub User { name: string, age: i64 }\n";
    let main = concat!(
        "module main\n",
        "import rec\n",
        "Wrapper { u: rec.User }\n",
        "fn main() -> i32 {\n",
        "  w := Wrapper{u: rec.User{name: \"abcd\".clone(), age: 7}}\n",
        "  return w.u.name.len() as i32\n",
        "}\n",
    );
    let out = build_and_run_multi("xmod-move", &[("rec.align", rec), ("main.align", main)], "main.align");
    assert_eq!(out.status.code(), Some(4));
}

#[test]
fn enum_payload_of_imported_type_resolves() {
    // A sum-type payload may be an imported `pub` plain struct — it resolves and type-checks (the
    // resolution path this slice unblocks). (Runtime extraction of a struct payload is exercised by
    // the single-module sum-type tests.)
    let main = concat!(
        "module main\n",
        "import geom\n",
        "Shape { Dot(geom.Point), Empty }\n",
        "fn main() -> i32 = 0\n",
    );
    assert!(!check_multi_errs("xmod-enum", &[("geom.align", GEOM), ("main.align", main)], "main.align"));
}

#[test]
fn field_of_unimported_type_rejected() {
    // Naming `geom.Point` without `import geom` is an error (same rule as a `geom.fn()` call).
    let main = "module main\nLine { a: geom.Point }\nfn main() -> i32 = 0\n";
    assert!(check_multi_errs("xmod-noimport", &[("geom.align", GEOM), ("main.align", main)], "main.align"));
}

#[test]
fn field_of_private_type_rejected() {
    // A non-`pub` type is not exported: a field naming it across modules is rejected.
    let geom_priv = "module geom\nPoint { x: i64, y: i64 }\n";
    let main = "module main\nimport geom\nLine { a: geom.Point }\nfn main() -> i32 = 0\n";
    assert!(check_multi_errs("xmod-private", &[("geom.align", geom_priv), ("main.align", main)], "main.align"));
}
