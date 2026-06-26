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
