//! pkg.db A1 owner tests: producer-owned common batch storage and borrowed SoA projection.

mod common;
use common::*;
use std::sync::LazyLock;

static DB: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db.align"));
static SQLITE: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db/sqlite.align"));
static POSTGRES: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db/postgres.align"));
static INTERNAL: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db/internal.align"));
static RESOURCE: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/internal/resource.align"));
static DESCRIPTOR: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/internal/descriptor.align"));
static INTERNAL_SQLITE: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/internal/sqlite.align"));
static INTERNAL_POSTGRES: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/internal/postgres.align"));
const POSTGRES_STUB: &str = include_str!("fixtures/pkg_db_q2_postgres_stub.c");

const TEST_HELPER: &str = r#"module pkg.db.a1_test
import pkg.db
import pkg.db.internal.descriptor
import pkg.db.internal.resource

pub fn set_rows_terminal<R>(borrow mut rows: pkg.db.rows<R>, state: u8) {
  unsafe { raw.store(resource.raw(resource.borrow(rows)), 5, state) }
}

pub fn set_batch_tail<R>(borrow values: pkg.db.batch<R>, tail: i64) {
  unsafe { raw.store(resource.raw(resource.borrow(values)), 40, tail) }
}

pub fn clone_batch_plan(plan: raw) -> raw {
  unsafe {
    copied := raw.alloc(72)
    version: u32 := raw.load(plan, 0)
    flags: u8 := raw.load(plan, 4)
    reserved8: u8 := raw.load(plan, 5)
    reserved16: u16 := raw.load(plan, 6)
    fields: u32 := raw.load(plan, 8)
    reserved32: u32 := raw.load(plan, 12)
    create: raw := raw.load(plan, 16)
    append: raw := raw.load(plan, 24)
    finish: raw := raw.load(plan, 32)
    row: raw := raw.load(plan, 40)
    soa: raw := raw.load(plan, 48)
    drop_payload: raw := raw.load(plan, 56)
    tail: i64 := raw.load(plan, 64)
    raw.store(copied, 0, version)
    raw.store(copied, 4, flags)
    raw.store(copied, 5, reserved8)
    raw.store(copied, 6, reserved16)
    raw.store(copied, 8, fields)
    raw.store(copied, 12, reserved32)
    raw.store(copied, 16, create)
    raw.store(copied, 24, append)
    raw.store(copied, 32, finish)
    raw.store(copied, 40, row)
    raw.store(copied, 48, soa)
    raw.store(copied, 56, drop_payload)
    raw.store(copied, 64, tail)
    return copied
  }
}

pub fn zero_column_plan_valid<P, R>(statement: pkg.db.query<P, R>) -> bool {
  unsafe {
    plan := pkg.db.internal.descriptor.batch_plan(statement)
    if !pkg.db.internal.resource.batch_plan_valid(plan) { return false }
    flags: u8 := raw.load(plan, 4)
    fields: u32 := raw.load(plan, 8)
    soa: raw := raw.load(plan, 48)
    return flags == 0 && fields == 0 && soa.is_null()
  }
}

pub fn zero_column_plan_rejects_soa<P, R>(statement: pkg.db.query<P, R>) -> bool {
  unsafe {
    plan := pkg.db.internal.descriptor.batch_plan(statement)
    forged := clone_batch_plan(plan)
    row: raw := raw.load(forged, 40)
    raw.store(forged, 4, 1 as u8)
    raw.store(forged, 48, row)
    accepted := pkg.db.internal.resource.batch_plan_valid(forged)
    raw.free(forged)
    return !accepted
  }
}

pub fn install_non_soa_batch_plan<R>(borrow values: pkg.db.batch<R>) -> raw {
  unsafe {
    wrapper := resource.raw(resource.borrow(values))
    plan: raw := raw.load(wrapper, 8)
    forged := clone_batch_plan(plan)
    raw.store(forged, 4, 0 as u8)
    raw.store(forged, 48, raw.null())
    raw.store(wrapper, 5, 0 as u8)
    raw.store(wrapper, 8, forged)
    return plan
  }
}

pub fn restore_batch_plan<R>(borrow values: pkg.db.batch<R>, plan: raw) {
  unsafe {
    wrapper := resource.raw(resource.borrow(values))
    forged: raw := raw.load(wrapper, 8)
    flags: u8 := raw.load(plan, 4)
    raw.store(wrapper, 8, plan)
    raw.store(wrapper, 5, flags)
    raw.free(forged)
  }
}
"#;

const QUERY: &str = r#"module app.batch_query
import pkg.db
import pkg.db.postgres
import pkg.db.sqlite

pub Params { base: i64 }
pub EmptyRow {}
pub PgParams { first_user_id: i64, last_user_id: i64 }
pub PgViewParams { id: i64, label: str, payload: slice<u8> }

pub PlainRow {
  id: i64,
  score: f64,
  label: str,
}

pub RichRow {
  id: i64,
  label: str,
  payload: slice<u8>,
  note: Option<str>,
}

pub PgRow {
  user_id: i64,
  user_name: str,
  group_id: Option<i64>,
  group_name: Option<str>,
}

pub PgViewRow { label: str, payload: slice<u8> }

pub FullParams {
  b: bool,
  nb: Option<bool>,
  i16v: i16,
  ni16: Option<i16>,
  i32v: i32,
  ni32: Option<i32>,
  i64v: i64,
  ni64: Option<i64>,
  f32v: f32,
  nf32: Option<f32>,
  f64v: f64,
  nf64: Option<f64>,
  textv: str,
  ntext: Option<str>,
  bytesv: slice<u8>,
  nbytes: Option<slice<u8>>,
}

pub FullRow {
  b: bool,
  nb: Option<bool>,
  i16v: i16,
  ni16: Option<i16>,
  i32v: i32,
  ni32: Option<i32>,
  i64v: i64,
  ni64: Option<i64>,
  f32v: f32,
  nf32: Option<f32>,
  f64v: f64,
  nf64: Option<f64>,
  textv: str,
  ntext: Option<str>,
  bytesv: slice<u8>,
  nbytes: Option<slice<u8>>,
}

pub fn plain() -> pkg.db.query<Params, PlainRow> = pkg.db.sqlite.query(
  "SELECT CAST(:base AS INTEGER) AS id, CAST(1.25 AS REAL) AS score, 'a' AS label UNION ALL SELECT CAST(:base + 1 AS INTEGER), CAST(2.5 AS REAL), 'medium-label' UNION ALL SELECT CAST(:base + 2 AS INTEGER), CAST(3.75 AS REAL), 'c' UNION ALL SELECT CAST(:base + 3 AS INTEGER), CAST(4.0 AS REAL), 'a-label-longer-than-sixty-four-bytes-to-force-child-block-growth-0123456789' UNION ALL SELECT CAST(:base + 4 AS INTEGER), CAST(5.5 AS REAL), 'tail'",
  [],
  [],
)

pub fn rich() -> pkg.db.query<Params, RichRow> = pkg.db.sqlite.query(
  "SELECT CAST(:base AS INTEGER) AS id, 'zero' AS label, X'00FF' AS payload, NULL AS note UNION ALL SELECT CAST(:base + 1 AS INTEGER), 'one-label-longer-than-sixty-four-bytes-to-force-child-block-growth-abcdefghij', X'01020300', 'present'",
  [],
  [],
)

pub fn zero_column() -> pkg.db.query<Params, EmptyRow> = pkg.db.sqlite.query(
  "SELECT 1 WHERE :base = 0",
  [],
  [],
)

pub fn postgres_rows() -> pkg.db.query<PgParams, PgRow> = pkg.db.query(
  "SELECT u.id AS user_id, u.name AS user_name, g.id AS group_id, g.name AS group_name FROM users AS u LEFT JOIN user_groups AS ug ON ug.user_id = u.id LEFT JOIN groups AS g ON g.id = ug.group_id WHERE u.id >= :first_user_id AND u.id <= :last_user_id ORDER BY u.id, g.id /* Q6_USER_GROUPS */",
  [],
)

pub fn postgres_bad_view() -> pkg.db.query<PgViewParams, PgViewRow> = pkg.db.postgres.query(
  "SELECT :label AS label, :payload AS payload WHERE :id = :id /* VIEW_FAULT TEXT_UTF8 */",
  [],
  [
    pkg.db.postgres.QueryOption.ParameterType("id", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("label", "text"),
    pkg.db.postgres.QueryOption.ParameterType("payload", "bytea"),
  ],
)

pub fn postgres_full() -> pkg.db.query<FullParams, FullRow> = pkg.db.postgres.query(
  "SELECT :b AS b, :nb AS nb, :i16v AS i16v, :ni16 AS ni16, :i32v AS i32v, :ni32 AS ni32, :i64v AS i64v, :ni64 AS ni64, :f32v AS f32v, :nf32 AS nf32, :f64v AS f64v, :nf64 AS nf64, :textv AS textv, :ntext AS ntext, :bytesv AS bytesv, :nbytes AS nbytes /* FULL_MATRIX */",
  [],
  [
    pkg.db.postgres.QueryOption.ParameterType("b", "bool"),
    pkg.db.postgres.QueryOption.ParameterType("nb", "bool"),
    pkg.db.postgres.QueryOption.ParameterType("i16v", "int2"),
    pkg.db.postgres.QueryOption.ParameterType("ni16", "int2"),
    pkg.db.postgres.QueryOption.ParameterType("i32v", "int4"),
    pkg.db.postgres.QueryOption.ParameterType("ni32", "int4"),
    pkg.db.postgres.QueryOption.ParameterType("i64v", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("ni64", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("f32v", "float4"),
    pkg.db.postgres.QueryOption.ParameterType("nf32", "float4"),
    pkg.db.postgres.QueryOption.ParameterType("f64v", "float8"),
    pkg.db.postgres.QueryOption.ParameterType("nf64", "float8"),
    pkg.db.postgres.QueryOption.ParameterType("textv", "text"),
    pkg.db.postgres.QueryOption.ParameterType("ntext", "text"),
    pkg.db.postgres.QueryOption.ParameterType("bytesv", "bytea"),
    pkg.db.postgres.QueryOption.ParameterType("nbytes", "bytea"),
  ],
)
"#;

fn package_files(main: &str) -> Vec<(&'static str, &str)> {
    vec![
        ("pkg/db.align", *DB),
        ("pkg/db/sqlite.align", *SQLITE),
        ("pkg/db/postgres.align", *POSTGRES),
        ("pkg/db/internal.align", *INTERNAL),
        ("pkg/db/internal/resource.align", *RESOURCE),
        ("pkg/db/internal/descriptor.align", *DESCRIPTOR),
        ("pkg/db/internal/sqlite.align", *INTERNAL_SQLITE),
        ("pkg/db/internal/postgres.align", *INTERNAL_POSTGRES),
        ("pkg/db/a1_test.align", TEST_HELPER),
        ("app/batch_query.align", QUERY),
        ("main.align", main),
    ]
}

#[test]
fn common_batch_surface_typechecks_whole_and_per_unit() {
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.batch_query

fn consume(borrow mut stream: pkg.db.rows<app.batch_query.PlainRow>) -> Result<i64, pkg.db.Error> {
  values := pkg.db.next_batch(stream, 32)?
  batch := values else { return Ok(0) }
  rows := pkg.db.batch_soa(batch)?
  first := pkg.db.batch_row(batch, 0)?
  return Ok(rows.id.sum() + match first { Some(row) => row.id, None => 0 })
}

fn main() -> i32 = 0
"#;
    let checked = diff_check_multi(
        "pkg-db-a1-common-batch-surface",
        &package_files(main),
        "main.align",
    );
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole diagnostics:\n{}\nper-unit diagnostics:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
}

#[test]
fn nested_abstract_batch_row_application_is_rejected() {
    let main = r#"module main
import pkg.db

Wrap<T> { value: T }

fn keep<T>(value: Option<pkg.db.batch<Wrap<T>>>) -> Option<pkg.db.batch<Wrap<T>>> = value
fn main() -> i32 = 0
"#;
    let checked = diff_check_multi(
        "pkg-db-a1-nested-abstract-batch-row",
        &package_files(main),
        "main.align",
    );
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "nested abstract batch Row unexpectedly accepted:\nwhole diagnostics:\n{}\nper-unit diagnostics:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
}

#[test]
fn zero_column_query_keeps_a_valid_non_soa_batch_plan() {
    if !backend_available() {
        return;
    }
    let main = r#"module main
import pkg.db.a1_test
import pkg.db
import app.batch_query

fn read_empty(borrow values: pkg.db.batch<app.batch_query.EmptyRow>) -> Result<Option<app.batch_query.EmptyRow>, pkg.db.Error> {
  return pkg.db.batch_row(values, 0)
}

fn main() -> i32 {
  if !pkg.db.a1_test.zero_column_plan_valid(app.batch_query.zero_column()) { return 1 }
  if !pkg.db.a1_test.zero_column_plan_rejects_soa(app.batch_query.zero_column()) { return 2 }
  return 42
}
"#;
    let output = build_and_run_multi_with_static_descriptors(
        "pkg-db-a1-zero-column-plan",
        &package_files(main),
        "main.align",
    );
    assert_eq!(
        output.status.code(),
        Some(42),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn batch_rows_and_soa_views_cannot_escape_or_survive_move() {
    let cases = [
        (
            "return-row-view",
            r#"fn bad(borrow connection: pkg.db.conn) -> str {
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.batch_query.plain(),
    app.batch_query.Params { base: 1 }, [],
  ) else { return "" }
  values := pkg.db.next_batch(stream, 2) else { return "" }
  batch := values else { return "" }
  selected := pkg.db.batch_row(batch, 0) else { return "" }
  row := selected else { return "" }
  return row.label
}"#,
        ),
        (
            "post-move-row-view",
            r#"fn consume(values: pkg.db.batch<app.batch_query.PlainRow>) -> i32 = 0

fn bad(borrow connection: pkg.db.conn) -> i32 {
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.batch_query.plain(),
    app.batch_query.Params { base: 1 }, [],
  ) else { return 0 }
  values := pkg.db.next_batch(stream, 2) else { return 0 }
  batch := values else { return 0 }
  selected := pkg.db.batch_row(batch, 0) else { return 0 }
  row := selected else { return 0 }
  ignored := consume(batch)
  return row.label.len() as i32
}"#,
        ),
        (
            "return-soa-column-view",
            r#"fn bad(borrow connection: pkg.db.conn) -> str {
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.batch_query.plain(),
    app.batch_query.Params { base: 1 }, [],
  ) else { return "" }
  values := pkg.db.next_batch(stream, 2) else { return "" }
  batch := values else { return "" }
  columns := pkg.db.batch_soa(batch) else { return "" }
  return columns.label[0]
}"#,
        ),
    ];
    for (name, body) in cases {
        let main = format!(
            "module main\nimport pkg.db\nimport app.batch_query\n{body}\nfn main() -> i32 = 0\n"
        );
        let checked = diff_check_multi(
            &format!("pkg-db-a1-batch-view-{name}"),
            &package_files(&main),
            "main.align",
        );
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{name} unexpectedly accepted (whole_errors={}, per_unit_errors={}):\nwhole diagnostics:\n{}\nper-unit diagnostics:\n{}",
            checked.whole_errors,
            checked.per_unit_errors,
            checked.whole_diags,
            checked.per_unit_diags,
        );
    }
}

#[test]
fn sqlite_batches_copy_rows_grow_views_and_project_soa() {
    if !backend_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.a1_test
import pkg.db.sqlite
import app.batch_query

fn main() -> i32 {
  opened := pkg.db.sqlite.connect(":memory:", [])
  connection := opened else { return 1 }
  rows_result := pkg.db.rows(
    pkg.db.exec_conn(connection),
    app.batch_query.plain(),
    app.batch_query.Params { base: 100 },
    [],
  )
  mut stream := rows_result else { return 2 }

  pkg.db.a1_test.set_rows_terminal(stream, 3 as u8)
  malformed := pkg.db.next_batch(stream, 0)
  malformed_ok := match malformed {
    Err(error) => match error {
      InvalidQuery(contract) => contract.item == "db.rows.header" && match contract.query_id {
        None => true
        Some(_) => false
      }
      _ => false
    }
    Ok(_) => false
  }
  if !malformed_ok { return 41 }
  pkg.db.a1_test.set_rows_terminal(stream, 0 as u8)
  invalid := pkg.db.next_batch(stream, 0)
  match invalid { Ok(_) => { return 3 } Err(_) => {} }

  first_result := pkg.db.next_batch(stream, 2) else { return 4 }
  first := first_result else { return 5 }
  first_len := pkg.db.batch_len(first) else { return 6 }
  if first_len != 2 { return 7 }
  row0_result := pkg.db.batch_row(first, 0) else { return 8 }
  row0 := row0_result else { return 9 }
  row1_result := pkg.db.batch_row(first, 1) else { return 10 }
  row1 := row1_result else { return 11 }
  if row0.id != 100 || row0.label != "a" || row1.id != 101
    || row1.label != "medium-label" { return 12 }
  outside := pkg.db.batch_row(first, -1) else { return 13 }
  match outside { Some(_) => { return 14 } None => {} }

  second_result := pkg.db.next_batch(stream, 2) else { return 15 }
  second := second_result else { return 16 }
  third_result := pkg.db.next_batch(stream, 2) else { return 17 }
  third := third_result else { return 18 }
  exhausted := pkg.db.next_batch(stream, 2) else { return 19 }
  match exhausted { Some(_) => { return 20 } None => {} }

  retained_result := pkg.db.batch_row(first, 1) else { return 21 }
  retained := retained_result else { return 22 }
  if retained.label != "medium-label" { return 23 }
  second1_result := pkg.db.batch_row(second, 1) else { return 24 }
  second1 := second1_result else { return 25 }
  if second1.id != 103 || second1.label.len() <= 64 { return 26 }
  third_len := pkg.db.batch_len(third) else { return 27 }
  if third_len != 1 { return 28 }

  columns := pkg.db.batch_soa(first) else { return 29 }
  if columns.id.sum() != 201 || columns[1].label != "medium-label" { return 30 }
  soa_plan := pkg.db.a1_test.install_non_soa_batch_plan(first)
  malformed_soa := pkg.db.batch_soa(first)
  pkg.db.a1_test.restore_batch_plan(first, soa_plan)
  malformed_soa_ok := match malformed_soa {
    Err(error) => match error {
      InvalidQuery(contract) => contract.item == "db.batch.header"
      _ => false
    }
    Ok(_) => false
  }
  if !malformed_soa_ok { return 44 }

  rich_result := pkg.db.rows(
    pkg.db.exec_conn(connection),
    app.batch_query.rich(),
    app.batch_query.Params { base: 7 },
    [],
  )
  mut rich_stream := rich_result else { return 31 }
  rich_batch_result := pkg.db.next_batch(rich_stream, 8) else { return 32 }
  rich_batch := rich_batch_result else { return 33 }
  rich0_result := pkg.db.batch_row(rich_batch, 0) else { return 34 }
  rich0 := rich0_result else { return 35 }
  rich1_result := pkg.db.batch_row(rich_batch, 1) else { return 36 }
  rich1 := rich1_result else { return 37 }
  match rich0.note { Some(_) => { return 38 } None => {} }
  note := rich1.note else { return 39 }
  if rich0.id != 7 || rich0.payload.len() != 2 || rich0.payload[0] != 0
    || rich0.payload[1] != 255 || rich1.id != 8 || rich1.label.len() <= 64
    || rich1.payload.len() != 4 || rich1.payload[3] != 0 || note != "present" { return 40 }
  pkg.db.a1_test.set_batch_tail(rich_batch, 1)
  malformed_batch := pkg.db.batch_len(rich_batch)
  malformed_batch_ok := match malformed_batch {
    Err(error) => match error {
      InvalidQuery(contract) => contract.item == "db.batch.header"
      _ => false
    }
    Ok(_) => false
  }
  pkg.db.a1_test.set_batch_tail(rich_batch, 0)
  if !malformed_batch_ok { return 43 }
  return 42
}
"#;
    let output = build_and_run_multi_with_static_descriptors(
        "pkg-db-a1-sqlite-batches",
        &package_files(main),
        "main.align",
    );
    assert_eq!(
        output.status.code(),
        Some(42),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn postgres_buffered_batches_split_without_resend_and_own_copied_rows() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_q6_delivered_rows() -> i32
  fn align_pg_protocol_ok() -> i32
  fn align_pg_protocol_error() -> i32
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/a1", []) else { return 1 }
  rows_result := pkg.db.rows(
    pkg.db.exec_conn(connection),
    app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 },
    [],
  )
  mut stream := rows_result else { return 2 }

  first_result := pkg.db.next_batch(stream, 2) else { return 3 }
  first := first_result else { return 4 }
  second_result := pkg.db.next_batch(stream, 2) else { return 5 }
  second := second_result else { return 6 }
  exhausted := pkg.db.next_batch(stream, 2) else { return 7 }
  match exhausted { Some(_) => { return 8 } None => {} }

  first0_result := pkg.db.batch_row(first, 0) else { return 9 }
  first0 := first0_result else { return 10 }
  first1_result := pkg.db.batch_row(first, 1) else { return 11 }
  first1 := first1_result else { return 12 }
  second0_result := pkg.db.batch_row(second, 0) else { return 13 }
  second0 := second0_result else { return 14 }
  second1_result := pkg.db.batch_row(second, 1) else { return 15 }
  second1 := second1_result else { return 16 }
  first1_group_id := first1.group_id else { return 17 }
  first1_group_name := first1.group_name else { return 18 }
  match second0.group_id { Some(_) => { return 19 } None => {} }
  match second0.group_name { Some(_) => { return 20 } None => {} }
  second1_group_id := second1.group_id else { return 21 }
  second1_group_name := second1.group_name else { return 22 }
  if first0.user_id != 1 || first0.user_name != "Alice"
    || first1_group_id != 20 || first1_group_name != "Dev"
    || second0.user_id != 2
    || second1.user_id != 3 || second1.user_name != "Cara"
    || second1_group_id != 40 || second1_group_name != "Ops" { return 23 }

  retained_result := pkg.db.batch_row(first, 0) else { return 24 }
  retained := retained_result else { return 25 }
  if retained.user_name != "Alice" { return 26 }
  return unsafe {
    if align_pg_protocol_ok() != 1 { return 30 + align_pg_protocol_error() }
    if align_pg_execute_calls() != 1 { return 27 }
    if align_pg_clear_calls() != 1 { return 28 }
    if align_pg_q6_delivered_rows() != 4 { return 29 }
    42
  }
}
"#;
    let output = build_and_run_multi_with_c(
        "pkg-db-a1-postgres-buffered-batches",
        &package_files(main),
        "main.align",
        POSTGRES_STUB,
    );
    assert_eq!(
        output.status.code(),
        Some(42),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn postgres_batch_all_value_kinds_and_null_bitmaps_are_exact() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_protocol_ok() -> i32
  fn align_pg_protocol_error() -> i32
}

fn execute_once(borrow connection: pkg.db.conn, present: bool) -> i32 {
  bytes := [0 as u8, 127 as u8, 255 as u8]
  nb: Option<bool> := if present { Some(true) } else { None }
  ni16: Option<i16> := if present { Some(-1234 as i16) } else { None }
  ni32: Option<i32> := if present { Some(-123456 as i32) } else { None }
  ni64: Option<i64> := if present { Some(-123456789) } else { None }
  nf32: Option<f32> := if present { Some(2.5 as f32) } else { None }
  nf64: Option<f64> := if present { Some(-3.25) } else { None }
  ntext: Option<str> := if present { Some("nullable") } else { None }
  nbytes: Option<slice<u8>> := if present { Some(bytes[..]) } else { None }
  opened := pkg.db.rows(
    pkg.db.exec_conn(connection),
    app.batch_query.postgres_full(),
    app.batch_query.FullParams {
      b: true, nb: nb,
      i16v: -12 as i16, ni16: ni16,
      i32v: 3456 as i32, ni32: ni32,
      i64v: 7890123, ni64: ni64,
      f32v: 1.5 as f32, nf32: nf32,
      f64v: -9.75, nf64: nf64,
      textv: "hello", ntext: ntext,
      bytesv: bytes[..], nbytes: nbytes,
    },
    [],
  )
  mut stream := opened else { return 1 }
  result := pkg.db.next_batch(stream, 1) else { return 2 }
  batch := result else { return 3 }
  exhausted := pkg.db.next_batch(stream, 1) else { return 4 }
  match exhausted { Some(_) => { return 5 } None => {} }
  selected := pkg.db.batch_row(batch, 0) else { return 6 }
  row := selected else { return 7 }
  if !row.b || row.i16v != (-12 as i16) || row.i32v != (3456 as i32)
    || row.i64v != 7890123 || row.f32v != (1.5 as f32) || row.f64v != -9.75
    || row.textv != "hello" || row.bytesv.len() != 3 || row.bytesv[0] != 0
    || row.bytesv[1] != 127 || row.bytesv[2] != 255 { return 8 }
  if present {
    vb := row.nb else { return 9 }
    vi16 := row.ni16 else { return 10 }
    vi32 := row.ni32 else { return 11 }
    vi64 := row.ni64 else { return 12 }
    vf32 := row.nf32 else { return 13 }
    vf64 := row.nf64 else { return 14 }
    vt := row.ntext else { return 15 }
    vx := row.nbytes else { return 16 }
    if !vb || vi16 != (-1234 as i16) || vi32 != (-123456 as i32)
      || vi64 != -123456789 || vf32 != (2.5 as f32) || vf64 != -3.25
      || vt != "nullable" || vx.len() != 3 || vx[2] != 255 { return 17 }
  } else {
    match row.nb { Some(_) => { return 18 } None => {} }
    match row.ni16 { Some(_) => { return 19 } None => {} }
    match row.ni32 { Some(_) => { return 20 } None => {} }
    match row.ni64 { Some(_) => { return 21 } None => {} }
    match row.nf32 { Some(_) => { return 22 } None => {} }
    match row.nf64 { Some(_) => { return 23 } None => {} }
    match row.ntext { Some(_) => { return 24 } None => {} }
    match row.nbytes { Some(_) => { return 25 } None => {} }
  }
  return 0
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/a1-matrix", []) else { return 30 }
  first := execute_once(connection, true)
  if first != 0 { return first }
  second := execute_once(connection, false)
  if second != 0 { return second }
  return unsafe {
    if align_pg_protocol_ok() != 1 { return 40 + align_pg_protocol_error() }
    if align_pg_execute_calls() != 2 { return 31 }
    if align_pg_clear_calls() != 2 { return 32 }
    42
  }
}
"#;
    let output = build_and_run_multi_with_c(
        "pkg-db-a1-postgres-value-matrix",
        &package_files(main),
        "main.align",
        POSTGRES_STUB,
    );
    assert_eq!(
        output.status.code(),
        Some(42),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn postgres_batch_decode_failure_drops_partial_payload_and_closes_once() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/a1-error", []) else { return 1 }
  bytes := [1 as u8, 2 as u8, 3 as u8]
  rows_result := pkg.db.rows(
    pkg.db.exec_conn(connection),
    app.batch_query.postgres_bad_view(),
    app.batch_query.PgViewParams { id: 7, label: "first", payload: bytes[..] },
    [],
  )
  mut stream := rows_result else { return 2 }
  failed := pkg.db.next_batch(stream, 8)
  failure_ok := match failed {
    Err(error) => match error { Decode(_) => true, _ => false }
    Ok(_) => false
  }
  if !failure_ok { return 3 }
  repeated := pkg.db.next_batch(stream, 8)
  repeated_ok := match repeated {
    Err(error) => match error {
      InvalidQuery(contract) => contract.item == "db.rows.state"
      _ => false
    }
    Ok(_) => false
  }
  if !repeated_ok { return 4 }
  return unsafe {
    if align_pg_protocol_ok() != 1 { return 5 }
    if align_pg_execute_calls() != 1 { return 6 }
    if align_pg_clear_calls() != 1 { return 7 }
    42
  }
}
"#;
    let output = build_and_run_multi_with_c(
        "pkg-db-a1-postgres-batch-decode-failure",
        &package_files(main),
        "main.align",
        POSTGRES_STUB,
    );
    assert_eq!(
        output.status.code(),
        Some(42),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
