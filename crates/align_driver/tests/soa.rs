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
fn soa_with_a_str_field_is_allowed() {
    // A `str` column is a 16-byte `{ptr,len}` view column (zero-copy into the decode input); the
    // soa is well-formed, and a primitive column still projects/reduces as before.
    assert!(ok(concat!(
        "Rec { id: i64, name: str }\n",
        "pub fn k(r: soa<Rec>) -> i64 = r.id.sum()\n",
    )));
    // Nested/owned columns (a struct field) stay rejected — only scalars and `str`.
    assert!(!ok(concat!(
        "Inner { a: i64 }\n",
        "Rec { id: i64, sub: Inner }\n",
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
    emit_object_file(&mir, &obj, BuildTarget::Baseline, Profile::Release).expect("codegen");
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
fn to_soa_transposes_a_str_column() {
    if !backend_available() {
        return;
    }
    // Transpose an AoS array with a `str` field into a `soa` — the str views copy into a view
    // column. age.sum() = 30 + 25 = 55; "alice".len() = 5 → exit 60.
    let out = build_and_run(
        "to-soa-str",
        concat!(
            "User { name: str, age: i64 }\n",
            "fn main() -> i32 {\n  arena {\n    rows := [User { name: \"alice\", age: 30 }, User { name: \"bob\", age: 25 }]\n    s := rows.to_soa()\n    return (s.age.sum() + s[0].name.len()) as i32\n  }\n}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(60));
}

#[test]
fn to_soa_with_a_nested_field_struct_is_rejected() {
    // A `str` column is allowed; a nested/owned column (a struct field) is not.
    assert!(!ok(concat!(
        "Inner { a: i32 }\n",
        "Rec { id: i32, sub: Inner }\n",
        "fn main() -> i32 {\n  arena {\n    s := [Rec { id: 1, sub: Inner { a: 2 } }].to_soa()\n    return 0\n  }\n}\n",
    )));
}

#[test]
fn to_soa_str_column_view_cannot_escape_the_arena() {
    // A str-bearing `to_soa` soa borrows the source's string storage and the arena buffer, so a str
    // element read is region-tied and must not escape to an outer binding.
    assert!(!ok(concat!(
        "User { name: str, age: i64 }\n",
        "fn get() -> i64 {\n  mut out: str := \"\"\n  arena {\n    rows := [User { name: \"alice\", age: 30 }]\n    s := rows.to_soa()\n    out = s[0].name\n  }\n  return out.len()\n}\n",
        "fn main() -> i32 = 0\n",
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

// ---- `json.decode` → `soa<Struct>` (decode straight into columns — no AoS intermediate, #228) ----

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
    // Decode the JSON array of objects straight into columns, then scan one column:
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

#[test]
fn json_decode_resolves_fields_via_perfect_hash() {
    if !backend_available() {
        return;
    }
    // A 4-field struct triggers the compile-time perfect-hash field dispatch. Unknown keys
    // (`junk`/`extra`) are skipped, and field order varies between objects — the hash lookup must
    // resolve each by name regardless. score=30+12=42, age=7+8=15, rank=2+3=5.
    let out = build_and_run(
        "soa-json-phf",
        concat!(
            "import core.json\n",
            "Rec { id: i64, score: i32, age: i32, rank: i32 }\n",
            "fn main() -> Result<(), Error> {\n  arena {\n    s: soa<Rec> := json.decode(\"[{\\\"id\\\":1,\\\"junk\\\":9,\\\"score\\\":30,\\\"age\\\":7,\\\"rank\\\":2},{\\\"rank\\\":3,\\\"id\\\":2,\\\"score\\\":12,\\\"extra\\\":0,\\\"age\\\":8}]\")?\n    print(s.score.sum())\n    print(s.age.sum())\n    print(s.rank.sum())\n  }\n  return Ok(())\n}\n",
        ),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n15\n5\n");
}

// ---- `str` columns in `soa<T>` — a 16-byte `{ptr,len}` view column borrowing the JSON input ----

#[test]
fn json_decode_into_soa_with_a_str_column() {
    if !backend_available() {
        return;
    }
    // A `str` column decodes as a zero-copy view into the input. Read a str element field
    // (`s[1].name` = "bob" → len 3) alongside an int column reduction (age 30+25 = 55) to prove
    // both the str view and the mixed str/int column layout are correct.
    let out = build_and_run(
        "soa-json-str",
        concat!(
            "import core.json\n",
            "User { name: str, age: i64 }\n",
            "fn main() -> Result<(), Error> {\n  arena {\n    s: soa<User> := json.decode(\"[{\\\"name\\\":\\\"alice\\\",\\\"age\\\":30},{\\\"name\\\":\\\"bob\\\",\\\"age\\\":25}]\")?\n    print(s[1].name.len())\n    print(s.age.sum())\n  }\n  return Ok(())\n}\n",
        ),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n55\n");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn str_column_view_cannot_escape_the_arena() {
    // A `str` column holds views borrowing the JSON input / arena buffer, so a str element read
    // (`s[0].name`) is region-tied and must not escape to an outer binding — else use-after-free.
    assert!(!ok(concat!(
        "import core.json\n",
        "User { name: str, age: i64 }\n",
        "fn get(data: str) -> Result<i64, Error> {\n  mut out: str := \"\"\n  arena {\n    s: soa<User> := json.decode(data)?\n    out = s[0].name\n  }\n  return Ok(out.len())\n}\n",
    )));
}

#[test]
fn str_struct_gather_cannot_escape_the_arena() {
    // A whole-element gather `s[0]` of a str-bearing struct carries the `str` view, so the gathered
    // struct is region-tied too (the `Index`-on-soa region arm, distinct from the `s[i].field` path).
    assert!(!ok(concat!(
        "import core.json\n",
        "User { name: str, age: i64 }\n",
        "fn get(data: str) -> Result<i64, Error> {\n  mut out: User := User { name: \"\", age: 0 }\n  arena {\n    s: soa<User> := json.decode(data)?\n    out = s[0]\n  }\n  return Ok(out.age)\n}\n",
    )));
}

#[test]
fn primitive_soa_stays_self_contained() {
    // Regression guard: a primitive-only soa borrows nothing, so an `i64` element field read still
    // escapes freely (no over-restriction from the str-column region tie).
    assert!(ok(concat!(
        "import core.json\n",
        "P { x: i64, y: i64 }\n",
        "fn get(data: str) -> Result<i64, Error> {\n  mut out: i64 := 0\n  arena {\n    s: soa<P> := json.decode(data)?\n    out = s[0].x\n  }\n  return Ok(out)\n}\n",
    )));
}

// ---- writing a `str` column (`s[i].name = v` / `s[i] = value`) ----
// A stored `str` view escapes if it does not outlive the soa, so every write is region-checked by
// the `AssignElem`/`AssignElemField` store rule (a str view is Copy — no per-field drop needed).

#[test]
fn str_column_single_field_write() {
    if !backend_available() {
        return;
    }
    // `s[i].name = v` replaces one str column's element (`StoreColumn`). The `str` literal is
    // `'static`, so it outlives the arena soa — the store is in-region. Read the new length back.
    let out = build_and_run(
        "soa-str-field-write",
        concat!(
            "import core.json\n",
            "User { name: str, age: i64 }\n",
            "fn main() -> Result<(), Error> {\n  arena {\n    mut s: soa<User> := json.decode(\"[{\\\"name\\\":\\\"alice\\\",\\\"age\\\":30}]\")?\n    s[0].name = \"changed\"\n    print(s[0].name.len())\n  }\n  return Ok(())\n}\n",
        ),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn str_column_field_write_cannot_store_shorter_lived() {
    // The escape dual of the read check: storing a str view from an *inner* arena into a soa that
    // lives in an *outer* arena would leave a dangling view once the inner arena frees — rejected.
    assert!(!ok(concat!(
        "import core.json\n",
        "User { name: str, age: i64 }\n",
        "fn get(inner: str) -> Result<i64, Error> {\n  arena {\n    mut s: soa<User> := json.decode(\"[{\\\"name\\\":\\\"a\\\",\\\"age\\\":1}]\")?\n    arena {\n      t: soa<User> := json.decode(inner)?\n      s[0].name = t[0].name\n    }\n    return Ok(s[0].name.len())\n  }\n}\n",
    )));
}

#[test]
fn str_column_whole_elem_write_scatters() {
    if !backend_available() {
        return;
    }
    // `s[0] = s[1]` on a str-bearing soa gathers row 1 and scatters every column (str + int) into
    // row 0 (`StoreColumn` per field). Read the new str length (5 = "bobby") and int (25) back.
    let out = build_and_run(
        "soa-str-whole-elem-write",
        concat!(
            "import core.json\n",
            "User { name: str, age: i64 }\n",
            "fn main() -> Result<(), Error> {\n  arena {\n    mut s: soa<User> := json.decode(\"[{\\\"name\\\":\\\"alice\\\",\\\"age\\\":30},{\\\"name\\\":\\\"bobby\\\",\\\"age\\\":25}]\")?\n    s[0] = s[1]\n    print(s[0].name.len())\n    print(s[0].age)\n  }\n  return Ok(())\n}\n",
        ),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n25\n");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn str_column_whole_elem_write_cannot_store_shorter_lived() {
    // Whole-element store dual: a gathered struct from an inner arena carries str views that would
    // dangle once the inner arena frees, so scattering it into the outer soa is rejected.
    assert!(!ok(concat!(
        "import core.json\n",
        "User { name: str, age: i64 }\n",
        "fn get(inner: str) -> Result<i64, Error> {\n  arena {\n    mut s: soa<User> := json.decode(\"[{\\\"name\\\":\\\"a\\\",\\\"age\\\":1}]\")?\n    arena {\n      t: soa<User> := json.decode(inner)?\n      s[0] = t[0]\n    }\n    return Ok(s[0].name.len())\n  }\n}\n",
    )));
}

#[test]
fn str_column_whole_elem_write_via_literal_cannot_store_shorter_lived() {
    // The same escape via a struct *literal* whose str field is a shorter-lived view — the value's
    // region is the shortest of its fields (`StructLit` region fold), so the store is still rejected.
    assert!(!ok(concat!(
        "import core.json\n",
        "User { name: str, age: i64 }\n",
        "fn get(inner: str) -> Result<i64, Error> {\n  arena {\n    mut s: soa<User> := json.decode(\"[{\\\"name\\\":\\\"a\\\",\\\"age\\\":1}]\")?\n    arena {\n      t: soa<User> := json.decode(inner)?\n      s[0] = User { name: t[0].name, age: 9 }\n    }\n    return Ok(s[0].name.len())\n  }\n}\n",
    )));
}

// ---- `group_by(.key).sum(.value)` — column-oriented grouped sum over a soa ----

#[test]
fn group_by_sum_type_checks() {
    assert!(ok(concat!(
        "P { k: i64, v: i64 }\n",
        "fn main() -> i32 {\n  arena {\n    s := [P{k:1,v:2}].to_soa()\n    g := s.group_by(.k).sum(.v)\n    return g.1.sum() as i32\n  }\n}\n",
    )));
}

#[test]
fn group_by_alone_is_rejected() {
    assert!(!ok("P { k: i64, v: i64 }\nfn main() -> i32 { arena { s := [P{k:1,v:2}].to_soa()\n g := s.group_by(.k)\n return 0 } }\n"));
}

#[test]
fn sum_field_without_group_by_is_rejected() {
    assert!(!ok("fn main() -> i32 { return [1,2,3].sum(.x) }\n"));
}

#[test]
fn group_by_non_i64_key_is_rejected() {
    assert!(!ok("P { k: i32, v: i64 }\nfn main() -> i32 { arena { s := [P{k:1,v:2}].to_soa()\n g := s.group_by(.k).sum(.v)\n return 0 } }\n"));
}

// ---- fused multi-aggregate `group_by(.key).agg(sum(.a), max(.b), count())` ----

#[test]
fn group_by_agg_multi_type_checks() {
    // A str-key AoS array, multiple aggregates in one `.agg(...)`: keys + one i64 column per aggregate.
    assert!(ok(concat!(
        "Row { name: str, a: i64, b: i64 }\n",
        "pub fn k(xs: array<Row>) -> i64 {\n  g := xs.group_by(.name).agg(sum(.a), max(.b), count())\n  return g.1.sum()\n}\n",
    )));
}

#[test]
fn group_by_agg_empty_is_rejected() {
    assert!(!ok("Row { name: str, a: i64 }\npub fn k(xs: array<Row>) -> i64 = xs.group_by(.name).agg().1.sum()\n"));
}

#[test]
fn group_by_agg_unknown_aggregate_is_rejected() {
    assert!(!ok("Row { name: str, a: i64 }\npub fn k(xs: array<Row>) -> i64 = xs.group_by(.name).agg(median(.a)).1.sum()\n"));
}

#[test]
fn group_by_agg_non_i64_value_is_rejected() {
    // `sum(.name)` over a str field — values must be i64 (first cut).
    assert!(!ok("Row { name: str, a: i64 }\npub fn k(xs: array<Row>) -> i64 = xs.group_by(.name).agg(sum(.name)).1.sum()\n"));
}

#[test]
fn group_by_agg_soa_source_is_rejected() {
    // First cut is the AoS str key; a soa i64-key multi-aggregate is a deferred follow-up.
    assert!(!ok("P { k: i64, v: i64 }\nfn main() -> i32 { arena { s := [P{k:1,v:2}].to_soa()\n g := s.group_by(.k).agg(sum(.v))\n return 0 } }\n"));
}

#[test]
fn group_by_sum_aggregates_by_key() {
    if !backend_available() {
        return;
    }
    // Rows: (k=1,v=10),(2,20),(1,5),(2,7),(3,100). Groups: {1:15, 2:27, 3:100}; 3 distinct keys.
    // The per-group sums total the overall value sum (10+20+5+7+100 = 142), and the key count is 3 —
    // checked order-independently (group output order is the hash table's).
    let out = build_and_run(
        "soa-group-sum",
        concat!(
            "P { k: i64, v: i64 }\n",
            "fn main() -> i32 {\n  arena {\n",
            "    s := [P{k:1,v:10}, P{k:2,v:20}, P{k:1,v:5}, P{k:2,v:7}, P{k:3,v:100}].to_soa()\n",
            "    g := s.group_by(.k).sum(.v)\n",
            "    print(g.0.len())\n",   // distinct keys = 3
            "    print(g.1.sum())\n",   // sum of per-group sums = 142
            "  }\n  return 0\n}\n",
        ),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n142\n");
}

#[test]
fn group_by_min_max_count_type_check() {
    let agg = |m: &str| {
        format!(
            "P {{ k: i64, v: i64 }}\nfn main() -> i32 {{ arena {{ s := [P{{k:1,v:2}}].to_soa()\n g := s.group_by(.k).{m}\n return g.1.sum() as i32 }} }}\n"
        )
    };
    assert!(ok(&agg("min(.v)")));
    assert!(ok(&agg("max(.v)")));
    assert!(ok(&agg("count()")));
    // `count` takes no value field; an unknown aggregate is rejected.
    assert!(!ok(&agg("count(.v)")));
    assert!(!ok(&agg("avg(.v)")));
}

#[test]
fn group_by_min_max_count_aggregate_by_key() {
    if !backend_available() {
        return;
    }
    // Rows: (k=1,v=10),(1,30),(2,5). Groups → min{1:10,2:5}=15, max{1:30,2:5}=35, count{1:2,2:1}=3.
    // Checked order-independently (sum of the per-group aggregate column).
    let out = build_and_run(
        "soa-group-mmc",
        concat!(
            "P { k: i64, v: i64 }\n",
            "fn main() -> i32 {\n  arena {\n",
            "    s := [P{k:1,v:10}, P{k:1,v:30}, P{k:2,v:5}].to_soa()\n",
            "    mn := s.group_by(.k).min(.v)\n",
            "    print(mn.1.sum())\n",   // 10 + 5 = 15
            "    mx := s.group_by(.k).max(.v)\n",
            "    print(mx.1.sum())\n",   // 30 + 5 = 35
            "    c := s.group_by(.k).count()\n",
            "    print(c.1.sum())\n",   // 2 + 1 = 3
            "  }\n  return 0\n}\n",
        ),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "15\n35\n3\n");
}

#[test]
fn group_by_str_key_dictionary_encodes_and_sums() {
    if !backend_available() {
        return;
    }
    // A str-keyed grouped sum over a decoded AoS `array<User>` (a `soa` can't hold a `str` column).
    // Rows: (a,10),(b,20),(a,5),(c,7),(b,3). Groups {a:15, b:23, c:7}. The runtime interns keys in
    // first-occurrence order (a,b,c), so the output is deterministic — assert keys + sums in order.
    let out = build_and_run(
        "group-str",
        concat!(
            "import core.json\n",
            "User { name: str, score: i64 }\n",
            "fn main() -> Result<(), Error> {\n  arena {\n",
            "    us: array<User> := json.decode(\"[{\\\"name\\\":\\\"a\\\",\\\"score\\\":10},{\\\"name\\\":\\\"b\\\",\\\"score\\\":20},{\\\"name\\\":\\\"a\\\",\\\"score\\\":5},{\\\"name\\\":\\\"c\\\",\\\"score\\\":7},{\\\"name\\\":\\\"b\\\",\\\"score\\\":3}]\")?\n",
            "    g := us.group_by(.name).sum(.score)\n",
            "    print(g.0.len())\n", // distinct keys = 3
            "    print(g.0[0])\n",    // a
            "    print(g.1[0])\n",    // 15
            "    print(g.0[1])\n",    // b
            "    print(g.1[1])\n",    // 23
            "    print(g.0[2])\n",    // c
            "    print(g.1[2])\n",    // 7
            "  }\n  return Ok(())\n}\n",
        ),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\na\n15\nb\n23\nc\n7\n");
}

#[test]
fn group_by_str_key_min_max_count_aggregate() {
    if !backend_available() {
        return;
    }
    // Same rows as the sum test (a:[10,5], b:[20,3], c:[7]), now min/max/count per string key.
    // Keys intern in first-occurrence order (a,b,c), so the output is deterministic.
    let prog = |agg: &str| {
        format!(
            concat!(
                "import core.json\n",
                "User {{ name: str, score: i64 }}\n",
                "fn main() -> Result<(), Error> {{\n  arena {{\n",
                "    us: array<User> := json.decode(\"[{{\\\"name\\\":\\\"a\\\",\\\"score\\\":10}},{{\\\"name\\\":\\\"b\\\",\\\"score\\\":20}},{{\\\"name\\\":\\\"a\\\",\\\"score\\\":5}},{{\\\"name\\\":\\\"c\\\",\\\"score\\\":7}},{{\\\"name\\\":\\\"b\\\",\\\"score\\\":3}}]\")?\n",
                "    g := us.group_by(.name).{agg}\n",
                "    print(g.1[0])\n    print(g.1[1])\n    print(g.1[2])\n",
                "  }}\n  return Ok(())\n}}\n",
            ),
            agg = agg,
        )
    };
    // min: a:min(10,5)=5, b:min(20,3)=3, c:7
    assert_eq!(String::from_utf8_lossy(&build_and_run("group-str-min", &prog("min(.score)")).stdout), "5\n3\n7\n");
    // max: a:max(10,5)=10, b:max(20,3)=20, c:7
    assert_eq!(String::from_utf8_lossy(&build_and_run("group-str-max", &prog("max(.score)")).stdout), "10\n20\n7\n");
    // count: a:2, b:2, c:1 (no value field)
    assert_eq!(String::from_utf8_lossy(&build_and_run("group-str-count", &prog("count()")).stdout), "2\n2\n1\n");
}

// ---- str-KEY group_by over a `soa<Struct>` (columnar; the two-contiguous-column runtime path) ----

#[test]
fn soa_str_key_group_by_all_aggregates() {
    if !backend_available() {
        return;
    }
    // The soa counterpart of the AoS str-key test: the same rows decoded straight into a `soa<User>`
    // (a `str` name column + an i64 score column), grouped by the str column. Keys intern in
    // first-occurrence order (a,b,c). Must match the AoS results exactly.
    let prog = |agg: &str| {
        format!(
            concat!(
                "import core.json\n",
                "User {{ name: str, score: i64 }}\n",
                "fn main() -> Result<(), Error> {{\n  arena {{\n",
                "    us: soa<User> := json.decode(\"[{{\\\"name\\\":\\\"a\\\",\\\"score\\\":10}},{{\\\"name\\\":\\\"b\\\",\\\"score\\\":20}},{{\\\"name\\\":\\\"a\\\",\\\"score\\\":5}},{{\\\"name\\\":\\\"c\\\",\\\"score\\\":7}},{{\\\"name\\\":\\\"b\\\",\\\"score\\\":3}}]\")?\n",
                "    g := us.group_by(.name).{agg}\n",
                "    print(g.1[0])\n    print(g.1[1])\n    print(g.1[2])\n",
                "  }}\n  return Ok(())\n}}\n",
            ),
            agg = agg,
        )
    };
    // sum: a:15, b:23, c:7 ; min: a:5, b:3, c:7 ; max: a:10, b:20, c:7 ; count: a:2, b:2, c:1.
    assert_eq!(String::from_utf8_lossy(&build_and_run("soa-group-str-sum", &prog("sum(.score)")).stdout), "15\n23\n7\n");
    assert_eq!(String::from_utf8_lossy(&build_and_run("soa-group-str-min", &prog("min(.score)")).stdout), "5\n3\n7\n");
    assert_eq!(String::from_utf8_lossy(&build_and_run("soa-group-str-max", &prog("max(.score)")).stdout), "10\n20\n7\n");
    assert_eq!(String::from_utf8_lossy(&build_and_run("soa-group-str-count", &prog("count()")).stdout), "2\n2\n1\n");
}

#[test]
fn soa_str_key_group_by_type_checks_and_selects_by_key_column() {
    // A str key column routes to the str path (SoaStr); an i64 key column stays on the i64 path.
    // (Bind the result tuple before `.1` — reading `.1` off a temporary tuple would move it, an
    // unrelated rule; binding keeps the check about key/value type resolution.)
    assert!(ok(concat!(
        "P { name: str, v: i64 }\n",
        "fn k(s: soa<P>) -> i64 {\n  g := s.group_by(.name).sum(.v)\n  return g.1.len()\n}\n",
    )));
    assert!(ok(concat!(
        "P { id: i64, v: i64 }\n",
        "fn k(s: soa<P>) -> i64 {\n  g := s.group_by(.id).sum(.v)\n  return g.1.len()\n}\n",
    )));
    // A str key still needs an i64 value (same rule as the AoS str key).
    assert!(!ok(concat!(
        "P { name: str, tag: str }\n",
        "fn k(s: soa<P>) -> i64 {\n  g := s.group_by(.name).sum(.tag)\n  return g.1.len()\n}\n",
    )));
}

#[test]
fn soa_str_key_group_by_result_cannot_escape_the_arena() {
    // The str keys of the result `(array<str>, array<i64>)` borrow the soa's string storage, so the
    // owned key array cannot be returned out of the arena (its `str` elements would dangle). The
    // i64-key soa group_by, by contrast, yields owned i64 keys that borrow nothing and CAN escape.
    assert!(!ok(concat!(
        "import core.json\n",
        "Rec { name: str, pay: i64 }\n",
        "fn keys_of(data: str) -> Result<array<str>, Error> {\n  arena {\n    s: soa<Rec> := json.decode(data)?\n    g := s.group_by(.name).sum(.pay)\n    return Ok(g.0)\n  }\n}\n",
    )));
    assert!(ok(concat!(
        "import core.json\n",
        "Rec { id: i64, pay: i64 }\n",
        "fn keys_of(data: str) -> Result<array<i64>, Error> {\n  arena {\n    s: soa<Rec> := json.decode(data)?\n    g := s.group_by(.id).sum(.pay)\n    return Ok(g.0)\n  }\n}\n",
    )));
}

#[test]
fn dict_encode_reuse_matches_a1_string_group_by() {
    if !backend_available() {
        return;
    }
    // The A2 reuse rail: `dict_encode(.name)` once, then reuse the encoded value for several
    // aggregates. Each `e.group_by(.name).<agg>(.score)` must equal the one-shot A1 str-key group_by.
    // Rows: (a,10),(b,20),(a,5),(c,7),(b,3). Keys intern in first-occurrence order (a,b,c).
    let out = build_and_run(
        "dict-encode-reuse",
        concat!(
            "import core.json\n",
            "User { name: str, score: i64 }\n",
            "fn main() -> Result<(), Error> {\n  arena {\n",
            "    us: array<User> := json.decode(\"[{\\\"name\\\":\\\"a\\\",\\\"score\\\":10},{\\\"name\\\":\\\"b\\\",\\\"score\\\":20},{\\\"name\\\":\\\"a\\\",\\\"score\\\":5},{\\\"name\\\":\\\"c\\\",\\\"score\\\":7},{\\\"name\\\":\\\"b\\\",\\\"score\\\":3}]\")?\n",
            "    e := us.dict_encode(.name)\n",
            "    s := e.group_by(.name).sum(.score)\n",
            "    print(s.0.len())\n", // 3 distinct keys
            "    print(s.0[0])\n    print(s.1[0])\n", // a 15
            "    print(s.0[1])\n    print(s.1[1])\n", // b 23
            "    print(s.0[2])\n    print(s.1[2])\n", // c 7
            "    m := e.group_by(.name).max(.score)\n", // reuse the SAME encoded value
            "    print(m.1[0])\n    print(m.1[1])\n    print(m.1[2])\n", // 10 20 7
            "    c := e.group_by(.name).count()\n",
            "    print(c.1[0])\n    print(c.1[1])\n    print(c.1[2])\n", // 2 2 1
            "  }\n  return Ok(())\n}\n",
        ),
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "3\na\n15\nb\n23\nc\n7\n10\n20\n7\n2\n2\n1\n"
    );
}

#[test]
fn dict_encode_group_by_key_must_match_encoded_key() {
    // `dict_encode(.name)` builds ids for `.name`; grouping the encoded value by a different str
    // field has no precomputed ids, so it is rejected.
    assert!(!ok(concat!(
        "import core.json\n",
        "User { name: str, tag: str, score: i64 }\n",
        "fn k(us: array<User>) -> i64 {\n",
        "  e := us.dict_encode(.name)\n",
        "  return e.group_by(.tag).sum(.score).1.len()\n",
        "}\n",
        "fn main() -> i32 = 0\n",
    )));
}

#[test]
fn dict_encode_requires_a_str_key() {
    // The encoded key must be a str field (first cut).
    assert!(!ok(concat!(
        "P { k: i64, v: i64 }\n",
        "fn k(ps: array<P>) -> i64 = ps.dict_encode(.k).group_by(.k).sum(.v).1.len()\n",
        "fn main() -> i32 = 0\n",
    )));
}

#[test]
fn group_by_str_key_requires_an_i64_value() {
    // The value field must be i64 (first cut) even when the key is a str.
    assert!(!ok(concat!(
        "import core.json\n",
        "User { name: str, tag: str }\n",
        "fn k(us: array<User>) -> i64 = us.group_by(.name).sum(.tag).1.len()\n",
        "fn main() -> i32 = 0\n",
    )));
}

#[test]
fn soa_whole_element_gather() {
    if !backend_available() {
        return;
    }
    // `s[i]` gathers a whole struct value from the soa columns at index `i` (a Copy of primitives).
    // r = s[1] = {2, 20, 200}; r.a + r.b + r.c = 222.
    let out = build_and_run(
        "soa-gather",
        concat!(
            "Rec { a: i64, b: i64, c: i64 }\n",
            "fn main() -> i32 {\n",
            "  arena {\n",
            "    rows := [Rec { a: 1, b: 10, c: 100 }, Rec { a: 2, b: 20, c: 200 }, Rec { a: 3, b: 30, c: 300 }]\n",
            "    s := rows.to_soa()\n",
            "    r := s[1]\n",
            "    return (r.a + r.b + r.c) as i32\n",
            "  }\n",
            "}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(222));
}

#[test]
fn a_gathered_struct_is_a_free_copy_returnable_from_its_arena() {
    if !backend_available() {
        return;
    }
    // The gather copies the primitive columns, so the result does not borrow the soa — it can
    // escape the arena it was built in (unlike the borrowed soa view itself). pick() returns s[1].
    let out = build_and_run(
        "soa-gather-escape",
        concat!(
            "Rec { a: i64, b: i64 }\n",
            "fn pick() -> Rec {\n",
            "  arena {\n",
            "    rows := [Rec { a: 1, b: 10 }, Rec { a: 2, b: 20 }]\n",
            "    s := rows.to_soa()\n",
            "    return s[1]\n",
            "  }\n",
            "}\n",
            "fn main() -> i32 {\n",
            "  r := pick()\n",
            "  return (r.a + r.b) as i32\n",
            "}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(22));
}

#[test]
fn out_of_range_gather_is_bounds_checked() {
    // A constant out-of-range gather still type-checks (the bound is a runtime check, like any
    // index) — `s[1]` is out of range for a 1-element soa, but the type checker accepts it.
    assert!(ok(concat!(
        "Rec { a: i64 }\n",
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    rows := [Rec { a: 1 }]\n",
        "    s := rows.to_soa()\n",
        "    r := s[1]\n",
        "    return r.a as i32\n",
        "  }\n",
        "}\n",
    )));
}

#[test]
fn soa_len_is_the_row_count() {
    if !backend_available() {
        return;
    }
    // `s.len()` is the soa's row count (its `{ptr,len}` length), like a slice's `.len()`.
    let out = build_and_run(
        "soa-len",
        concat!(
            "R { a: i64, b: i64 }\n",
            "fn main() -> i32 {\n",
            "  arena {\n",
            "    rows := [R { a: 1, b: 9 }, R { a: 2, b: 8 }, R { a: 3, b: 7 }]\n",
            "    s := rows.to_soa()\n",
            "    return s.len() as i32\n",
            "  }\n",
            "}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn soa_indexed_field_reads_one_column() {
    if !backend_available() {
        return;
    }
    // `s[i].field` reads one column's element directly (lowered to `IndexColumn`), the column-major
    // analogue of AoS `arr[i].field`. s[1].a + s[2].b = 7 + 9 = 16.
    let out = build_and_run(
        "soa-elem-field",
        concat!(
            "R { a: i64, b: i64 }\n",
            "fn main() -> i32 {\n",
            "  arena {\n",
            "    rows := [R { a: 3, b: 1 }, R { a: 7, b: 2 }, R { a: 5, b: 9 }]\n",
            "    s := rows.to_soa()\n",
            "    return (s[1].a + s[2].b) as i32\n",
            "  }\n",
            "}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(16));
}

#[test]
fn soa_materialized_column_uses_padded_offset() {
    if !backend_available() {
        return;
    }
    // Regression: materialising a column as a value (`c := s.field`) lowers to `Rvalue::SoaColumn`,
    // which must use the SAME `align_up`-padded column offset as the per-element `IndexColumn` read.
    // Here `score`/`age` sit after a 1-byte `bool` column, so a flat `len*prefix` offset (the old
    // bug) would land mid-padding and read garbage. score: 10+99+20 = 129; age: 30+40+25 = 95.
    let out = build_and_run(
        "soa-mat-col",
        concat!(
            "User { active: bool, score: i64, age: i64 }\n",
            "fn main() -> i32 {\n",
            "  arena {\n",
            "    rows := [\n",
            "      User { active: true, score: 10, age: 30 },\n",
            "      User { active: false, score: 99, age: 40 },\n",
            "      User { active: true, score: 20, age: 25 },\n",
            "    ]\n",
            "    s := rows.to_soa()\n",
            "    sc := s.score\n",
            "    ag := s.age\n",
            "    return (sc.sum() + ag.sum()) as i32\n",
            "  }\n",
            "}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(224)); // (129 + 95) & 0xff
}

#[test]
fn soa_column_window_sums_a_subrange() {
    if !backend_available() {
        return;
    }
    // `s.field[a..b]` windows one column (the column projection is a `slice<FieldTy>`, so the
    // existing slice sub-range applies). `score[1..3]` = {99, 20}, sum = 119. The window is over a
    // padded column (after a `bool`), so this also exercises the corrected `SoaColumn` offset.
    let out = build_and_run(
        "soa-col-window",
        concat!(
            "User { active: bool, score: i64, age: i64 }\n",
            "fn main() -> i32 {\n",
            "  arena {\n",
            "    rows := [\n",
            "      User { active: true, score: 10, age: 30 },\n",
            "      User { active: false, score: 99, age: 40 },\n",
            "      User { active: true, score: 20, age: 25 },\n",
            "      User { active: true, score: 5, age: 50 },\n",
            "    ]\n",
            "    s := rows.to_soa()\n",
            "    return s.score[1..3].sum() as i32\n",
            "  }\n",
            "}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(119));
}

#[test]
fn soa_column_window_len_and_open_ends() {
    if !backend_available() {
        return;
    }
    // A windowed column keeps a correct `{ptr,len}`: `score[1..3].len()` = 2, `score[2..].sum()` =
    // 20 + 5 = 25, `score[..2].sum()` = 10 + 99 = 109. Returns 2 + 25 + 109 = 136.
    let out = build_and_run(
        "soa-col-window-ends",
        concat!(
            "User { active: bool, score: i64, age: i64 }\n",
            "fn main() -> i32 {\n",
            "  arena {\n",
            "    rows := [\n",
            "      User { active: true, score: 10, age: 30 },\n",
            "      User { active: false, score: 99, age: 40 },\n",
            "      User { active: true, score: 20, age: 25 },\n",
            "      User { active: true, score: 5, age: 50 },\n",
            "    ]\n",
            "    s := rows.to_soa()\n",
            "    n := s.score[1..3].len()\n",
            "    tail := s.score[2..].sum()\n",
            "    head := s.score[..2].sum()\n",
            "    return (n + tail + head) as i32\n",
            "  }\n",
            "}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(136));
}

#[test]
fn soa_elem_field_assign_writes_one_column() {
    if !backend_available() {
        return;
    }
    // `s[i].field = v` — the write counterpart of the `s[i].field` read: a column store
    // (`StoreColumn`) at row `i`, the soa analogue of AoS `arr[i].field = v`. The padded `score`/
    // `age` columns (after a `bool`) exercise the `align_up` offset on the write side too.
    // s[0].score=77, s[1].age=5, s[0].active stays true(1) → 77 + 5 + 1 = 83.
    let out = build_and_run(
        "soa-elem-field-assign",
        concat!(
            "User { active: bool, score: i64, age: i64 }\n",
            "fn main() -> i32 {\n",
            "  arena {\n",
            "    rows := [\n",
            "      User { active: true, score: 1, age: 9 },\n",
            "      User { active: false, score: 3, age: 7 },\n",
            "    ]\n",
            "    mut s := rows.to_soa()\n",
            "    s[0].score = 77\n",
            "    s[1].age = 5\n",
            "    active0 := s[0].active\n",
            "    return (s[0].score + s[1].age + (if active0 { 1 } else { 0 })) as i32\n",
            "  }\n",
            "}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(83));
}

#[test]
fn soa_elem_field_assign_dynamic_index() {
    if !backend_available() {
        return;
    }
    // A runtime row index into the column store, bounds-checked at the write.
    let out = build_and_run(
        "soa-elem-field-assign-dyn",
        concat!(
            "P { a: i64, b: i64 }\n",
            "fn main() -> i32 {\n",
            "  arena {\n",
            "    rows := [P { a: 1, b: 2 }, P { a: 3, b: 4 }, P { a: 5, b: 6 }]\n",
            "    mut s := rows.to_soa()\n",
            "    mut i := 2\n",
            "    s[i].a = 40\n",
            "    return s[2].a as i32\n",
            "  }\n",
            "}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(40));
}

#[test]
fn soa_elem_field_assign_immutable_rejected() {
    // Writing a column through a soa view requires the view local to be `mut`.
    assert!(!ok(concat!(
        "P { a: i64, b: i64 }\n",
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    rows := [P { a: 1, b: 2 }]\n",
        "    s := rows.to_soa()\n",
        "    s[0].a = 9\n",
        "    return 0\n",
        "  }\n",
        "}\n",
    )));
}

#[test]
fn soa_whole_elem_assign_scatters_columns() {
    if !backend_available() {
        return;
    }
    // `s[i] = structval` — the write counterpart of the `s[i]` gather: the value's fields scatter
    // into their columns (`StoreColumn` per field) at row `i`. s[1]={70,80}, s[0]/s[2] untouched →
    // 70 + 80 + (s[0].a=1) + (s[2].b=6) = 157.
    let out = build_and_run(
        "soa-whole-elem-assign",
        concat!(
            "P { a: i64, b: i64 }\n",
            "fn main() -> i32 {\n",
            "  arena {\n",
            "    rows := [P { a: 1, b: 2 }, P { a: 3, b: 4 }, P { a: 5, b: 6 }]\n",
            "    mut s := rows.to_soa()\n",
            "    s[1] = P { a: 70, b: 80 }\n",
            "    return (s[1].a + s[1].b + s[0].a + s[2].b) as i32\n",
            "  }\n",
            "}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(157));
}

#[test]
fn soa_whole_elem_assign_from_gather() {
    if !backend_available() {
        return;
    }
    // The value can be another gathered element: `s[0] = s[1]` (gather row 1, scatter into row 0).
    let out = build_and_run(
        "soa-whole-elem-assign-gather",
        concat!(
            "P { a: i64, b: i64 }\n",
            "fn main() -> i32 {\n",
            "  arena {\n",
            "    rows := [P { a: 1, b: 2 }, P { a: 9, b: 9 }]\n",
            "    mut s := rows.to_soa()\n",
            "    s[0] = s[1]\n",
            "    return (s[0].a + s[0].b) as i32\n",
            "  }\n",
            "}\n",
        ),
    );
    assert_eq!(out.status.code(), Some(18));
}

#[test]
fn soa_whole_elem_assign_immutable_rejected() {
    // Scattering a whole element through a soa view requires the view local to be `mut`.
    assert!(!ok(concat!(
        "P { a: i64, b: i64 }\n",
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    rows := [P { a: 1, b: 2 }]\n",
        "    s := rows.to_soa()\n",
        "    s[0] = P { a: 9, b: 9 }\n",
        "    return 0\n",
        "  }\n",
        "}\n",
    )));
}
