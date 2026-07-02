//! Default (non-`layout(C)`) struct field reordering (draft.md §9, `docs/impl/05-backend-llvm.md`
//! §2). A non-`layout(C)` struct's field order is language-unspecified, so codegen lays fields out in
//! descending alignment to eliminate padding (Rust's default). Access is by name, so the reorder is
//! invisible in source; codegen keeps a logical→physical index map for every field GEP / byte-offset
//! site. `layout(C)` keeps declaration order (its byte layout is the FFI/`raw`/json boundary).
//!
//! These tests pin: (a) the padding win is real (the LLVM struct type is reordered), (b) every field
//! width round-trips after the reorder, (c) json decode and (d) `to_soa` are correct on a reordered
//! struct, (e) `layout(C)` is *not* reordered, and (f) nested structs reorder at each level.

mod common;
use common::*;

// ---- (a) padding elimination: the LLVM struct type is reordered by descending alignment ----

#[test]
fn padding_is_eliminated_by_reordering() {
    // `{ i8, i64, i8 }` in declaration order is 24 bytes (7 + 1 + 7 padding); reordered to
    // `{ i64, i8, i8 }` it is 16 bytes. The named LLVM type in the emitted IR proves the reorder.
    let ir = emit_llvm(
        "S { a: i8, b: i64, c: i8 }\nfn main() -> i32 {\n  s := S { a: 1, b: 2, c: 3 }\n  return (s.a as i32) + (s.b as i32) + (s.c as i32)\n}\n",
    );
    assert!(
        ir.contains("%S = type { i64, i8, i8 }"),
        "expected the fields reordered to descending alignment; got:\n{ir}"
    );
    // The declaration-order layout (which would pad to 24 bytes) must NOT be emitted.
    assert!(!ir.contains("%S = type { i8, i64, i8 }"), "fields were not reordered:\n{ir}");
}

#[test]
fn ties_keep_declaration_order() {
    // Equal-alignment fields keep declaration order (a stable sort), so a struct that is already in
    // descending-alignment order is unchanged, and equal-width fields never shuffle.
    let ir = emit_llvm(
        "T { a: i32, b: i32, c: i16, d: i16 }\nfn main() -> i32 {\n  t := T { a: 1, b: 2, c: 3, d: 4 }\n  return (t.a + t.b) as i32\n}\n",
    );
    assert!(ir.contains("%T = type { i32, i32, i16, i16 }"), "stable order not preserved:\n{ir}");
}

// ---- (b) every field width round-trips after the reorder ----

#[test]
fn all_widths_round_trip_after_reorder() {
    if !backend_available() {
        return;
    }
    // A struct mixing every primitive width. Construct (per-field stores), then read every field
    // back. If any GEP used a stale (logical) index, a field would read the wrong slot. 1+2+3+4+5+6
    // = 21, +100 for the bool → 121.
    let src = "Mix { a: i8, b: i16, c: i32, d: i64, e: f32, f: f64, g: bool }\nfn main() -> i32 {\n  m := Mix { a: 1, b: 2, c: 3, d: 4, e: 5.0, f: 6.0, g: true }\n  mut acc := 0\n  acc = acc + (m.a as i32)\n  acc = acc + (m.b as i32)\n  acc = acc + (m.c as i32)\n  acc = acc + (m.d as i32)\n  acc = acc + (m.e as i32)\n  acc = acc + (m.f as i32)\n  if m.g {\n    acc = acc + 100\n  }\n  return acc\n}\n";
    assert_eq!(build_and_run("reorder-widths", src).status.code(), Some(121));
}

#[test]
fn field_writes_hit_the_right_slot() {
    if !backend_available() {
        return;
    }
    // Mutate individual fields after construction, then read them back. Exercises `StoreField` on the
    // reordered layout: each write must land in the same physical slot the read later addresses.
    let src = "S { a: i8, b: i64, c: i8, d: i32 }\nfn main() -> i32 {\n  mut s := S { a: 1, b: 2, c: 3, d: 4 }\n  s.a = 10\n  s.b = 20\n  s.c = 30\n  s.d = 40\n  return (s.a as i32) + (s.b as i32) + (s.c as i32) + s.d\n}\n";
    assert_eq!(build_and_run("reorder-writes", src).status.code(), Some(100));
}

#[test]
fn struct_array_element_fields_after_reorder() {
    if !backend_available() {
        return;
    }
    // `[N x %Struct]` element-field GEP (`[0, i, field]`) must map the field index too. Read and
    // write element fields of a reordered struct. 7 + 30 = 37.
    let src = "E { a: i8, b: i64, c: i16 }\nfn main() -> i32 {\n  mut es := [E{a: 1, b: 2, c: 3}, E{a: 4, b: 5, c: 6}]\n  es[0].b = 30\n  return (es[0].a as i32) + (es[0].b as i32) - (es[0].c as i32) + (es[1].a as i32) + (es[1].c as i32)\n}\n";
    // es[0]: a=1, b=30, c=3 -> 1 + 30 - 3 = 28; es[1]: a=4, c=6 -> 10; total 38.
    assert_eq!(build_and_run("reorder-elem", src).status.code(), Some(38));
}

// ---- (c) json decode is correct on a reordered struct ----

#[test]
fn json_decode_into_reordered_struct() {
    if !backend_available() {
        return;
    }
    // The decode field-offset table must point at physical offsets. A struct whose declaration order
    // differs from its alignment order (`{ i8, i64, i16 }`) decodes correctly only if the offsets are
    // mapped. score + flag + tag = 20 + 100 + 3 = 123.
    let src = "import core.json\nRec { flag: i8, score: i64, tag: i16 }\nfn main() -> Result<(), Error> {\n  arena {\n    rs: array<Rec> := json.decode(\"[{\\\"flag\\\":1,\\\"score\\\":20,\\\"tag\\\":3}]\")?\n    r := rs[0]\n    mut acc := 0\n    acc = acc + (r.score as i32)\n    acc = acc + (r.tag as i32)\n    if r.flag > 0 {\n      acc = acc + 100\n    }\n    print(acc)\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("reorder-json", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "123\n");
}

// ---- (d) to_soa (AoS -> column-major) is correct on a reordered struct ----

#[test]
fn to_soa_over_reordered_struct() {
    if !backend_available() {
        return;
    }
    // `to_soa()` reads each AoS field (physical GEP, mapped) and scatters it into its soa column
    // (logical order); `s[0]` gathers a whole struct back (physical insert, mapped). The mixed-width
    // struct forces a reorder. age.sum() = 30 + 25 = 55; s[0].a (i8) = 1 -> 56.
    let src = "P { a: i8, age: i64, b: i16 }\nfn main() -> i32 {\n  arena {\n    rows := [P{a: 1, age: 30, b: 7}, P{a: 2, age: 25, b: 8}]\n    s := rows.to_soa()\n    return (s.age.sum() + (s[0].a as i64)) as i32\n  }\n}\n";
    assert_eq!(build_and_run("reorder-soa", src).status.code(), Some(56));
}

// ---- (e) layout(C) is NOT reordered (FFI/raw byte-layout boundary is unchanged) ----

#[test]
fn layout_c_is_not_reordered() {
    // A `layout(C)` struct keeps declaration order even when reordering would save padding — its byte
    // layout is the FFI/`raw`/json boundary. The two structs differ only by the attribute.
    let ir = emit_llvm(
        "layout(C) C { a: i8, b: i64, c: i8 }\nS { a: i8, b: i64, c: i8 }\nfn main() -> i32 {\n  unsafe {\n    p := raw.alloc(64)\n    raw.store(p, 0, C { a: 1, b: 2, c: 3 })\n    w: C := raw.load(p, 0)\n    raw.free(p)\n    s := S { a: 4, b: 5, c: 6 }\n    return (w.a as i32) + (w.b as i32) + (s.a as i32) + (s.b as i32)\n  }\n}\n",
    );
    assert!(ir.contains("%C = type { i8, i64, i8 }"), "layout(C) must keep declaration order:\n{ir}");
    assert!(ir.contains("%S = type { i64, i8, i8 }"), "the default struct should reorder:\n{ir}");
}

#[test]
fn layout_c_raw_round_trip_still_works() {
    if !backend_available() {
        return;
    }
    // The `raw.store`/`raw.load` round trip depends on the flat, unreordered byte layout. 3 + 4 = 7.
    let src = "layout(C) Pair { a: i8, b: i64 }\nfn main() -> i32 {\n  unsafe {\n    p := raw.alloc(16)\n    raw.store(p, 0, Pair { a: 3, b: 4 })\n    w: Pair := raw.load(p, 0)\n    raw.free(p)\n    return (w.a as i32) + (w.b as i32)\n  }\n}\n";
    assert_eq!(build_and_run("reorder-layoutc-raw", src).status.code(), Some(7));
}

// ---- (f) nested structs reorder at each level ----

#[test]
fn nested_struct_reorders_at_each_level() {
    if !backend_available() {
        return;
    }
    // Inner and outer are both reordered; a nested field path `o.inner.x` must map the index at each
    // level of the GEP walk. (100 + 2) + (3 + 40) = 145.
    let src = "Inner { x: i8, y: i64 }\nOuter { p: i16, inner: Inner, q: i64 }\nfn main() -> i32 {\n  mut o := Outer { p: 3, inner: Inner { x: 1, y: 2 }, q: 5 }\n  o.inner.x = 100\n  o.q = 40\n  return (o.inner.x as i32) + (o.inner.y as i32) + (o.p as i32) + (o.q as i32)\n}\n";
    // inner.x = 100, inner.y = 2, p = 3, q = 40 -> 145.
    assert_eq!(build_and_run("reorder-nested", src).status.code(), Some(145));
}
