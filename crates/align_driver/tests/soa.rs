//! `soa<Struct>` — a struct-of-arrays view (M6 layout lever). A field projection `ps.field` selects
//! one contiguous column as a `slice<FieldTy>`, so a scan reads only the fields it touches (the
//! cache win over array-of-structs). First cut: a borrowed `soa<T>` parameter of a primitive-scalar
//! struct, with `ps.field.<reduction>()` pipelines.
//!
//! Runtime correctness + the speedup vs idiomatic Rust `Vec<Struct>` are covered by `bench/` (the
//! kernel needs externally-provided column data); these tests pin the type/projection rules.

mod common;
use common::*;

fn ok(src: &str) -> bool {
    let mut sm = SourceMap::new();
    !check(&mut sm, "soa", src).diags.has_errors()
}

#[test]
fn a_soa_column_sum_type_checks() {
    assert!(ok(concat!(
        "P { a: i64, b: i64, c: i64 }\n",
        "pub fn col_sum(ps: soa<P>) -> i64 = ps.a.sum()\n",
    )));
}

#[test]
fn a_soa_column_feeds_a_where_map_reduce_pipeline() {
    // `ps.field` is a `slice<FieldTy>`, so the full scalar pipeline runs over the column.
    assert!(ok(concat!(
        "P { a: i64, b: i64 }\n",
        "pub fn k(ps: soa<P>) -> i64 = ps.a.where(pos).map(dbl).sum()\n",
        "fn pos(x: i64) -> bool = x > 0\n",
        "fn dbl(x: i64) -> i64 = x + x\n",
    )));
}

#[test]
fn a_float_column_projects_too() {
    assert!(ok(concat!(
        "Body { mass: f64, x: f64, y: f64 }\n",
        "pub fn total_mass(b: soa<Body>) -> f64 = b.mass.sum()\n",
    )));
}

#[test]
fn soa_of_a_non_struct_is_rejected() {
    assert!(!ok("pub fn k(s: soa<i64>) -> i64 = 0\n"));
}

#[test]
fn soa_with_a_str_field_is_rejected() {
    // First cut is primitive-scalar fields only (no owned/str columns yet).
    assert!(!ok(concat!(
        "Rec { id: i64, name: str }\n",
        "pub fn k(r: soa<Rec>) -> i64 = r.id.sum()\n",
    )));
}

#[test]
fn soa_with_mixed_width_fields_is_allowed() {
    // Mixed widths are fine: each column's start is padded to the field's alignment in codegen, so
    // `i8` then `i64` keeps the i64 column naturally aligned for any length.
    assert!(ok(concat!(
        "P { tag: i8, value: i64 }\n",
        "pub fn k(ps: soa<P>) -> i64 = ps.value.sum()\n",
    )));
}

#[test]
fn a_filtered_aggregate_over_two_columns_type_checks() {
    // `where(.active)` filters by one column, `.pay` reads another — a column-spanning pipeline.
    assert!(ok(concat!(
        "Row { active: bool, pay: i64 }\n",
        "pub fn total(rs: soa<Row>) -> i64 = rs.where(.active).pay.sum()\n",
    )));
}

#[test]
fn a_whole_struct_stage_over_soa_is_rejected() {
    // A `where(fn)` / `map(fn)` taking the whole struct would gather every column — rejected (no
    // panic); filter a field with `where(.field)` instead.
    assert!(!ok(concat!(
        "Row { active: i64, pay: i64 }\n",
        "pub fn total(rs: soa<Row>) -> i64 = rs.where(act).pay.sum()\n",
        "fn act(r: Row) -> bool = r.active > 0\n",
    )));
}

#[test]
fn projecting_an_unknown_column_is_rejected() {
    assert!(!ok(concat!(
        "P { a: i64, b: i64 }\n",
        "pub fn k(ps: soa<P>) -> i64 = ps.z.sum()\n",
    )));
}

#[test]
fn a_soa_pipeline_must_select_a_column_first() {
    // Summing the soa itself is meaningless — a column must be chosen.
    assert!(!ok(concat!(
        "P { a: i64, b: i64 }\n",
        "pub fn k(ps: soa<P>) -> i64 = ps.sum()\n",
    )));
}

#[test]
fn the_compiled_object_exports_the_kernel() {
    if !backend_available() {
        return;
    }
    // The kernel compiles all the way through codegen (the column projection + reduction lower).
    let src = concat!(
        "P { a: i64, b: i64, c: i64, d: i64 }\n",
        "pub fn col_sum(ps: soa<P>) -> i64 = ps.c.sum()\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "soa-obj", src);
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let obj = dir.join(format!("align-soa-{}.o", std::process::id()));
    emit_object_file(&mir, &obj, BuildTarget::Baseline).expect("codegen");
    assert!(obj.exists());
    let _ = std::fs::remove_file(&obj);
}

// ---- `to_soa()` construction (transpose AoS → column-major soa) ----

#[test]
fn to_soa_type_checks_inside_an_arena() {
    assert!(ok(concat!(
        "P { a: i32, b: i32 }\n",
        "fn main() -> i32 {\n  arena {\n    s := [P { a: 1, b: 2 }].to_soa()\n    return s.a.sum()\n  }\n}\n",
    )));
}

#[test]
fn to_soa_outside_an_arena_is_rejected() {
    // The column buffer is arena-bump-allocated (no owned-soa type yet), so it needs an arena.
    assert!(!ok(concat!(
        "P { a: i32, b: i32 }\n",
        "fn main() -> i32 {\n  s := [P { a: 1, b: 2 }].to_soa()\n  return s.a.sum()\n}\n",
    )));
}

#[test]
fn a_built_soa_cannot_escape_its_arena() {
    // The view borrows the arena buffer (region-tied), so returning it out of the arena is an escape.
    assert!(!ok(concat!(
        "P { a: i32, b: i32 }\n",
        "fn build() -> soa<P> {\n  arena {\n    return [P { a: 1, b: 2 }].to_soa()\n  }\n}\n",
        "fn main() -> i32 = 0\n",
    )));
}

#[test]
fn to_soa_over_a_non_struct_array_is_rejected() {
    assert!(!ok("fn main() -> i32 {\n  arena {\n    s := [1, 2, 3].to_soa()\n    return 0\n  }\n}\n"));
}

#[test]
fn to_soa_over_a_str_field_struct_is_rejected() {
    assert!(!ok(concat!(
        "Rec { id: i32, name: str }\n",
        "fn main() -> i32 {\n  arena {\n    s := [Rec { id: 1, name: \"x\" }].to_soa()\n    return 0\n  }\n}\n",
    )));
}

#[test]
fn to_soa_with_a_pipeline_stage_is_rejected() {
    // First cut is a pure transpose of the whole struct — `where`/`map`/`.field` before it is not
    // supported yet.
    assert!(!ok(concat!(
        "P { a: i32, b: i32 }\n",
        "fn main() -> i32 {\n  arena {\n    s := [P { a: 1, b: 2 }].where(pa).to_soa()\n    return 0\n  }\n}\n",
        "fn pa(p: P) -> bool = p.a > 0\n",
    )));
}

#[test]
fn to_soa_builds_and_sums_two_columns() {
    if !backend_available() {
        return;
    }
    // a.sum()=1+2+3=6, b.sum()=10+20+30=60 → 66. The transpose scatters each element's fields into
    // their columns, then two column scans read them back.
    let out = build_and_run(
        "soa-build",
        concat!(
            "P { a: i32, b: i32 }\n",
            "fn main() -> i32 {\n  arena {\n    s := [P { a: 1, b: 10 }, P { a: 2, b: 20 }, P { a: 3, b: 30 }].to_soa()\n    return s.a.sum() + s.b.sum()\n  }\n}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(66));
}

#[test]
fn to_soa_keeps_mixed_width_columns_aligned() {
    if !backend_available() {
        return;
    }
    // `i8` then `i32`: the `n` column starts at `align_up(2*1, 4) = 4`, so the write and the read
    // must agree on the padded offset. n.sum()=40+2=42.
    let out = build_and_run(
        "soa-build-mixed",
        concat!(
            "P { flag: i8, n: i32 }\n",
            "fn main() -> i32 {\n  arena {\n    s := [P { flag: 1, n: 40 }, P { flag: 1, n: 2 }].to_soa()\n    return s.n.sum()\n  }\n}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn a_built_soa_feeds_a_filtered_multi_column_aggregate() {
    if !backend_available() {
        return;
    }
    // The headline flow: build the soa, then `where(.active).pay.sum()` streams two columns —
    // 10 + 5 = 15 (the inactive 20 is masked out).
    let out = build_and_run(
        "soa-build-where",
        concat!(
            "Row { active: bool, pay: i32 }\n",
            "fn main() -> i32 {\n  arena {\n    s := [Row { active: true, pay: 10 }, Row { active: false, pay: 20 }, Row { active: true, pay: 5 }].to_soa()\n    return s.where(.active).pay.sum()\n  }\n}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(15));
}

// ---- `json.decode` → `soa<Struct>` (decode-direct-to-columns; AoS-then-transpose) ----

#[test]
fn json_decode_into_soa_type_checks() {
    assert!(ok(concat!(
        "import core.json\n",
        "User { id: i64, age: i32 }\n",
        "fn main() -> Result<(), Error> {\n  arena {\n    s: soa<User> := json.decode(\"[]\")?\n    print(s.age.sum())\n  }\n  return Ok(())\n}\n",
    )));
}

#[test]
fn json_decode_into_soa_outside_an_arena_is_rejected() {
    assert!(!ok(concat!(
        "import core.json\n",
        "User { id: i64, age: i32 }\n",
        "fn main() -> Result<(), Error> {\n  s: soa<User> := json.decode(\"[]\")?\n  return Ok(())\n}\n",
    )));
}

#[test]
fn a_decoded_soa_cannot_escape_its_arena() {
    assert!(!ok(concat!(
        "import core.json\n",
        "User { id: i64, age: i32 }\n",
        "fn build() -> Result<soa<User>, Error> {\n  arena {\n    return Ok(json.decode(\"[]\")?)\n  }\n}\n",
        "fn main() -> i32 = 0\n",
    )));
}

#[test]
fn json_decode_into_soa_sums_a_column() {
    if !backend_available() {
        return;
    }
    // Decode the JSON array of objects to AoS, transpose to columns, then scan one column:
    // age.sum() = 30 + 40 + 5 = 75.
    let out = build_and_run(
        "soa-json-sum",
        concat!(
            "import core.json\n",
            "User { id: i64, age: i32 }\n",
            "fn main() -> Result<(), Error> {\n  arena {\n    s: soa<User> := json.decode(\"[{\\\"id\\\":1,\\\"age\\\":30},{\\\"id\\\":2,\\\"age\\\":40},{\\\"id\\\":3,\\\"age\\\":5}]\")?\n    print(s.age.sum())\n  }\n  return Ok(())\n}\n",
        ),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "75\n");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn json_decode_into_soa_feeds_a_filtered_aggregate() {
    if !backend_available() {
        return;
    }
    // The headline real-world flow: decode straight to columns, then `where(.active).pay.sum()`
    // streams only the two touched columns — 10 + 5 = 15 (the inactive 20 is masked out).
    let out = build_and_run(
        "soa-json-where",
        concat!(
            "import core.json\n",
            "Row { active: bool, pay: i32 }\n",
            "fn main() -> Result<(), Error> {\n  arena {\n    s: soa<Row> := json.decode(\"[{\\\"active\\\":true,\\\"pay\\\":10},{\\\"active\\\":false,\\\"pay\\\":20},{\\\"active\\\":true,\\\"pay\\\":5}]\")?\n    print(s.where(.active).pay.sum())\n  }\n  return Ok(())\n}\n",
        ),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "15\n");
}

#[test]
fn json_decode_into_soa_propagates_a_parse_error() {
    if !backend_available() {
        return;
    }
    // Malformed JSON → the decode `Result` is `Err`, `?` propagates it out of `main`, which maps to
    // a non-zero exit code and prints nothing.
    let out = build_and_run(
        "soa-json-err",
        concat!(
            "import core.json\n",
            "Row { active: bool, pay: i32 }\n",
            "fn main() -> Result<(), Error> {\n  arena {\n    s: soa<Row> := json.decode(\"not json\")?\n    print(s.pay.sum())\n  }\n  return Ok(())\n}\n",
        ),
    );
    assert_ne!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}
