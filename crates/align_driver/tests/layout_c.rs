//! `layout(C)` struct ABI — slice 1 (draft.md §15). A `layout(C)` attribute marks a struct as having
//! a stable, C-compatible flat byte layout (declaration-order fields, natural alignment, no
//! reordering — Align's default layout, which the marker locks and opts into FFI). Only such a struct
//! may be written to / read from `raw` memory (`raw.store`/`raw.load` of a whole struct at a byte
//! offset), because only it promises a fixed representation. This is the pointer-based FFI pattern:
//! hand C a `raw` buffer and read/write structs in it. (By-value register passing is a later slice.)

mod common;
use common::*;

fn ok(src: &str) -> bool {
    let mut sm = SourceMap::new();
    !check(&mut sm, "layout_c", src).diags.has_errors()
}

#[test]
fn struct_round_trip_through_raw() {
    if !backend_available() {
        return;
    }
    // Store a `layout(C)` struct into raw memory and read it back as a struct. Exit 7 confirms the
    // whole-struct `raw.store`/`raw.load` round-trips the flat bytes.
    let out = build_and_run(
        "layout-c-roundtrip",
        "layout(C) Pair { a: i32, b: i32 }\n\nfn main() -> i32 {\n  unsafe {\n    p := raw.alloc(8)\n    v := Pair { a: 3, b: 4 }\n    raw.store(p, 0, v)\n    w: Pair := raw.load(p, 0)\n    raw.free(p)\n    return w.a + w.b\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn c_writes_bytes_align_reads_struct() {
    if !backend_available() {
        return;
    }
    // C (`memset`) writes the bytes, Align reads them back as a `layout(C)` struct — the flat layout
    // is what makes this sound. memset-to-zero → both fields 0 → exit 42.
    let out = build_and_run(
        "layout-c-memset",
        "layout(C) Pair { a: i32, b: i32 }\nextern \"C\" fn memset(p: raw, c: i32, n: i64) -> raw\n\nfn main() -> i32 {\n  unsafe {\n    p := raw.alloc(8)\n    memset(p, 0, 8)\n    w: Pair := raw.load(p, 0)\n    raw.free(p)\n    return w.a + w.b + 42\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn float_struct_at_nonzero_offset() {
    if !backend_available() {
        return;
    }
    // Float fields + a non-zero byte offset within a larger buffer. (1.5 + 2.5) as i32 == 4.
    let out = build_and_run(
        "layout-c-float",
        "layout(C) Vec2 { x: f64, y: f64 }\n\nfn main() -> i32 {\n  unsafe {\n    p := raw.alloc(64)\n    raw.store(p, 16, Vec2 { x: 1.5, y: 2.5 })\n    w: Vec2 := raw.load(p, 16)\n    raw.free(p)\n    return (w.x + w.y) as i32\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(4));
}

#[test]
fn align_and_layout_combine() {
    if !backend_available() {
        return;
    }
    // `align(N)` and `layout(C)` compose (in either order). Exit 11.
    let out = build_and_run(
        "layout-c-align",
        "align(16) layout(C) A { x: i64, y: i64 }\n\nfn main() -> i32 {\n  unsafe {\n    p := raw.alloc(32)\n    raw.store(p, 0, A { x: 5, y: 6 })\n    a: A := raw.load(p, 0)\n    raw.free(p)\n    return (a.x + a.y) as i32\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(11));
}

#[test]
fn non_layout_c_struct_is_not_raw_storable() {
    // A plain struct has a compiler-private layout, so it cannot be stored/loaded through `raw`.
    assert!(!ok("Pair { a: i32, b: i32 }\nfn main() -> i32 {\n  unsafe {\n    p := raw.alloc(8)\n    raw.store(p, 0, Pair { a: 3, b: 4 })\n    raw.free(p)\n    return 0\n  }\n}\n"));
}

#[test]
fn non_layout_c_struct_load_is_rejected() {
    assert!(!ok("Pair { a: i32, b: i32 }\nfn main() -> i32 {\n  unsafe {\n    p := raw.alloc(8)\n    w: Pair := raw.load(p, 0)\n    raw.free(p)\n    return 0\n  }\n}\n"));
}

#[test]
fn layout_c_with_non_scalar_field_is_rejected() {
    // A `layout(C)` struct's fields must be integers/floats (their C mapping is settled); `str` and
    // other field types are a later slice.
    assert!(!ok("layout(C) Bad { a: i32, s: str }\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn layout_c_on_sum_type_is_rejected() {
    assert!(!ok("layout(C) Color { Red, Green }\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn unknown_layout_kind_is_rejected() {
    // `layout(C)` is the only supported layout attribute.
    assert!(!ok("layout(packed) Pair { a: i32, b: i32 }\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn layout_c_on_generic_struct_is_rejected() {
    // A generic struct has no single C representation (each monomorph is a distinct C type), and its
    // fields never pass the concrete FFI-safe check — so `layout(C)` on it is rejected outright.
    // (Without this, `Pair<str>` could become a `layout(C)` struct and reach `raw.store`/`raw.load`.)
    assert!(!ok("layout(C) Pair<T> { a: i32, b: T }\nfn main() -> i32 {\n  return 0\n}\n"));
}
