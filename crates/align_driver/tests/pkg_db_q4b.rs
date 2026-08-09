//! pkg.db Q4b/D8+D9 typed streaming, deadline, cancellation, and cleanup owners.

mod common;
use common::*;

const DB: &str = include_str!("../../../apps/db/pkg/db.align");
const SQLITE: &str = include_str!("../../../apps/db/pkg/db/sqlite.align");
const POSTGRES: &str = include_str!("../../../apps/db/pkg/db/postgres.align");
const INTERNAL: &str = include_str!("../../../apps/db/pkg/db/internal.align");
const RESOURCE: &str = include_str!("../../../apps/db/pkg/db/internal/resource.align");
const DESCRIPTOR: &str = include_str!("../../../apps/db/pkg/db/internal/descriptor.align");
const INTERNAL_SQLITE: &str = include_str!("../../../apps/db/pkg/db/internal/sqlite.align");
const INTERNAL_POSTGRES: &str = include_str!("../../../apps/db/pkg/db/internal/postgres.align");
const POSTGRES_STUB: &str = include_str!("fixtures/pkg_db_q2_postgres_stub.c");
const SQLITE_STUB: &str = include_str!("fixtures/pkg_db_q4a_sqlite_stub.c");

const QUERY: &str = r#"module app.q4b_query
import pkg.db
import pkg.db.sqlite
import pkg.db.postgres

pub Params {
  id: i64,
  label: str,
  payload: slice<u8>,
}

pub Row { id: i64 }

pub ViewRow {
  label: str,
  payload: slice<u8>,
}

pub DeadlineParams { value: i64 }
pub DeadlineRow { value: i64 }

pub OwnedParams {
  textv: string,
  ntext: Option<string>,
  bytesv: array<u8>,
  nbytes: Option<array<u8>>,
}

pub OwnedRow {
  textv: str,
  ntext: Option<str>,
  bytesv: slice<u8>,
  nbytes: Option<slice<u8>>,
}

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

pub fn selected() -> pkg.db.query<Params, Row> = pkg.db.sqlite.query(
  "SELECT :id AS id WHERE :label = :label AND :payload = :payload",
  [],
  [],
)

pub fn viewed() -> pkg.db.query<Params, ViewRow> = pkg.db.sqlite.query(
  "SELECT :label AS label, :payload AS payload WHERE :id = :id",
  [],
  [],
)

pub fn viewed_postgres_text_null() -> pkg.db.query<Params, ViewRow> = pkg.db.postgres.query(
  "SELECT :label AS label, :payload AS payload WHERE :id = :id /* VIEW_FAULT TEXT_NULL */",
  [],
  [
    pkg.db.postgres.QueryOption.ParameterType("id", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("label", "text"),
    pkg.db.postgres.QueryOption.ParameterType("payload", "bytea"),
  ],
)

pub fn viewed_postgres_text_length() -> pkg.db.query<Params, ViewRow> = pkg.db.postgres.query(
  "SELECT :label AS label, :payload AS payload WHERE :id = :id /* VIEW_FAULT TEXT_LENGTH */",
  [],
  [
    pkg.db.postgres.QueryOption.ParameterType("id", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("label", "text"),
    pkg.db.postgres.QueryOption.ParameterType("payload", "bytea"),
  ],
)

pub fn viewed_postgres_text_utf8() -> pkg.db.query<Params, ViewRow> = pkg.db.postgres.query(
  "SELECT :label AS label, :payload AS payload WHERE :id = :id /* VIEW_FAULT TEXT_UTF8 */",
  [],
  [
    pkg.db.postgres.QueryOption.ParameterType("id", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("label", "text"),
    pkg.db.postgres.QueryOption.ParameterType("payload", "bytea"),
  ],
)

pub fn viewed_postgres_bytes_null() -> pkg.db.query<Params, ViewRow> = pkg.db.postgres.query(
  "SELECT :label AS label, :payload AS payload WHERE :id = :id /* VIEW_FAULT BYTES_NULL */",
  [],
  [
    pkg.db.postgres.QueryOption.ParameterType("id", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("label", "text"),
    pkg.db.postgres.QueryOption.ParameterType("payload", "bytea"),
  ],
)

pub fn viewed_postgres_bytes_length() -> pkg.db.query<Params, ViewRow> = pkg.db.postgres.query(
  "SELECT :label AS label, :payload AS payload WHERE :id = :id /* VIEW_FAULT BYTES_LENGTH */",
  [],
  [
    pkg.db.postgres.QueryOption.ParameterType("id", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("label", "text"),
    pkg.db.postgres.QueryOption.ParameterType("payload", "bytea"),
  ],
)

pub fn viewed_postgres_bytes_hex() -> pkg.db.query<Params, ViewRow> = pkg.db.postgres.query(
  "SELECT :label AS label, :payload AS payload WHERE :id = :id /* VIEW_FAULT BYTES_HEX */",
  [],
  [
    pkg.db.postgres.QueryOption.ParameterType("id", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("label", "text"),
    pkg.db.postgres.QueryOption.ParameterType("payload", "bytea"),
  ],
)

pub fn viewed_postgres_clean() -> pkg.db.query<Params, ViewRow> = pkg.db.postgres.query(
  "SELECT :label AS label, :payload AS payload WHERE :id = :id /* VIEW_FAULT CLEAN */",
  [],
  [
    pkg.db.postgres.QueryOption.ParameterType("id", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("label", "text"),
    pkg.db.postgres.QueryOption.ParameterType("payload", "bytea"),
  ],
)

pub fn selected_postgres() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:id AS BIGINT) AS id WHERE :label = :label AND :payload = :payload",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("id", "int8")],
)

pub fn selected_postgres_timeout() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:id AS BIGINT) AS id WHERE :label = :label AND :payload = :payload /* TIMEOUT_WAIT */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("id", "int8")],
)

pub fn owned() -> pkg.db.query<OwnedParams, OwnedRow> = pkg.db.sqlite.query(
  "SELECT :textv AS textv, :ntext AS ntext, :bytesv AS bytesv, :nbytes AS nbytes",
  [],
  [],
)

pub fn full() -> pkg.db.query<FullParams, FullRow> = pkg.db.sqlite.query(
  "SELECT :b AS b, :nb AS nb, :i16v AS i16v, :ni16 AS ni16, :i32v AS i32v, :ni32 AS ni32, :i64v AS i64v, :ni64 AS ni64, :f32v AS f32v, :nf32 AS nf32, :f64v AS f64v, :nf64 AS nf64, :textv AS textv, :ntext AS ntext, :bytesv AS bytesv, :nbytes AS nbytes",
  [],
  [],
)

pub fn full_postgres() -> pkg.db.query<FullParams, FullRow> = pkg.db.postgres.query(
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

pub fn deadline_success() -> pkg.db.query<DeadlineParams, DeadlineRow> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn command_success() -> pkg.db.command<DeadlineParams> = pkg.db.postgres.command(
  "UPDATE q4b SET value = :value /* COMMAND_OK */",
  [],
  [pkg.db.postgres.CommandOption.ParameterType("value", "int8")],
)

pub fn command_wait() -> pkg.db.command<DeadlineParams> = pkg.db.postgres.command(
  "UPDATE q4b SET value = :value /* COMMAND_OK TIMEOUT_WAIT */",
  [],
  [pkg.db.postgres.CommandOption.ParameterType("value", "int8")],
)

pub fn deadline_wait() -> pkg.db.query<DeadlineParams, DeadlineRow> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* TIMEOUT_WAIT */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn deadline_cancel_fail() -> pkg.db.query<DeadlineParams, DeadlineRow> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* TIMEOUT_WAIT CANCEL_FAIL */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn deadline_drain_fail() -> pkg.db.query<DeadlineParams, DeadlineRow> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* TIMEOUT_WAIT DRAIN_FAIL */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn deadline_send_fail() -> pkg.db.query<DeadlineParams, DeadlineRow> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* SEND_FAIL */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn deadline_flush_fail() -> pkg.db.query<DeadlineParams, DeadlineRow> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* FLUSH_FAIL */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn deadline_cancel_resource_fail() -> pkg.db.query<DeadlineParams, DeadlineRow> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* TIMEOUT_WAIT CANCEL_RESOURCE_FAIL */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn deadline_transaction_unknown() -> pkg.db.query<DeadlineParams, DeadlineRow> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* TIMEOUT_WAIT TX_UNKNOWN */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn malformed_postgres() -> pkg.db.query<DeadlineParams, DeadlineRow> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* BAD_FIRST */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)
"#;

fn package_files(main: &str) -> Vec<(&'static str, &str)> {
    vec![
        ("pkg/db.align", DB),
        ("pkg/db/sqlite.align", SQLITE),
        ("pkg/db/postgres.align", POSTGRES),
        ("pkg/db/internal.align", INTERNAL),
        ("pkg/db/internal/resource.align", RESOURCE),
        ("pkg/db/internal/descriptor.align", DESCRIPTOR),
        ("pkg/db/internal/sqlite.align", INTERNAL_SQLITE),
        ("pkg/db/internal/postgres.align", INTERNAL_POSTGRES),
        ("app/q4b_query.align", QUERY),
        ("main.align", main),
    ]
}

#[test]
fn public_streaming_surface_is_exact() {
    for required in [
        "pub resource rows<R> = pkg.db.internal.resource.drop_rows",
        "pub fn rows<P, R>(",
        "  target: exec,",
        "  statement: query<P, R>,",
        "  params: P,",
        "  options: slice<ExecuteOption>,",
        ") -> Result<rows<R>, Error>",
        "pub fn next<R>(borrow mut stream: rows<R>) -> Result<Option<R>, Error>",
        "pub fn rows_native<P, R>(",
        "control: pkg.db.internal.DescriptorHeaderControl,",
    ] {
        assert!(
            DB.contains(required) || SQLITE.contains(required) || POSTGRES.contains(required),
            "missing exact Q4b streaming surface `{required}`"
        );
    }
    for absent in ["pub fn cancel", "pub fn portal", "pub fn statement_cache"] {
        assert!(
            !DB.contains(absent),
            "unexpected deferred surface `{absent}`"
        );
    }
}

#[test]
fn direct_rows_and_next_typecheck_whole_and_per_unit() {
    let main = r#"module main
import pkg.db
import app.q4b_query

fn consume(borrow connection: pkg.db.conn) -> i32 {
  bytes := [1 as u8, 2 as u8]
  opened := pkg.db.rows(
    pkg.db.exec_conn(connection),
    app.q4b_query.selected(),
    app.q4b_query.Params { id: 7, label: "stream", payload: bytes[..] },
    [],
  )
  return match opened {
    Err(_) => 2
    Ok(stream_value) => {
      mut stream := stream_value
      first := pkg.db.next(stream)
      match first {
        Err(_) => 3
        Ok(value) => match value { None => 4, Some(row) => row.id as i32 }
      }
    }
  }
}

fn propagate(borrow connection: pkg.db.conn) -> Result<(), pkg.db.Error> {
  bytes := [1 as u8, 2 as u8]
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection),
    app.q4b_query.viewed(),
    app.q4b_query.Params { id: 7, label: "stream", payload: bytes[..] },
    [],
  )?
  current := pkg.db.next(stream)?
  match current { Some(row) => { ignored := row.label.len() } None => {} }
  return Ok(())
}

fn main() -> i32 = 0
"#;
    let checked = diff_check_multi(
        "pkg-db-q4b-direct-rows-surface",
        &package_files(main),
        "main.align",
    );
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "unexpected whole-program diagnostics:\n{}\nunexpected per-unit diagnostics:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
    let built = build_per_unit_multi(
        "pkg-db-q4b-direct-rows-mir",
        &package_files(main),
        "main.align",
    );
    assert!(
        built
            .unit("main")
            .mir
            .fns
            .iter()
            .any(|function| function.name.as_str() == "main"),
        "direct rows must survive the checked-HIR gate"
    );
}

#[test]
fn streamed_views_cannot_cross_generation_or_escape() {
    let cases = [
        (
            "use-after-next",
            r#"fn bad(borrow connection: pkg.db.conn) -> i32 {
  bytes := [1 as u8]
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.viewed(),
    app.q4b_query.Params { id: 1, label: "first", payload: bytes[..] }, [],
  ) else { return 0 }
  first := pkg.db.next(stream) else { return 0 }
  row := first else { return 0 }
  ignored := pkg.db.next(stream)
  return if row.label == "first" { 1 } else { 0 }
}"#,
        ),
        (
            "return",
            r#"fn bad(borrow connection: pkg.db.conn) -> str {
  bytes := [1 as u8]
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.viewed(),
    app.q4b_query.Params { id: 1, label: "first", payload: bytes[..] }, [],
  ) else { return "" }
  first := pkg.db.next(stream) else { return "" }
  row := first else { return "" }
  return row.label
}"#,
        ),
        (
            "builder-storage",
            r#"fn bad(borrow connection: pkg.db.conn, out: region) -> i32 {
  bytes := [1 as u8]
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.viewed(),
    app.q4b_query.Params { id: 1, label: "first", payload: bytes[..] }, [],
  ) else { return 0 }
  first := pkg.db.next(stream) else { return 0 }
  row := first else { return 0 }
  mut values := array_builder<str>(out)
  values.push(row.label)
  ignored := pkg.db.next(stream)
  return values.len() as i32
}"#,
        ),
        (
            "branch-generation",
            r#"fn bad(borrow connection: pkg.db.conn, advance: bool) -> i32 {
  bytes := [1 as u8]
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.viewed(),
    app.q4b_query.Params { id: 1, label: "first", payload: bytes[..] }, [],
  ) else { return 0 }
  first := pkg.db.next(stream) else { return 0 }
  row := first else { return 0 }
  if advance { ignored := pkg.db.next(stream) }
  return row.label.len() as i32
}"#,
        ),
        (
            "loop-generation",
            r#"fn bad(borrow connection: pkg.db.conn) -> i32 {
  bytes := [1 as u8]
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.viewed(),
    app.q4b_query.Params { id: 1, label: "first", payload: bytes[..] }, [],
  ) else { return 0 }
  first := pkg.db.next(stream) else { return 0 }
  row := first else { return 0 }
  loop { ignored := pkg.db.next(stream); break }
  return row.payload.len() as i32
}"#,
        ),
    ];
    for (name, body) in cases {
        let main = format!(
            "module main\nimport pkg.db\nimport app.q4b_query\n{body}\nfn main() -> i32 = 0\n"
        );
        let checked = diff_check_multi(
            &format!("pkg-db-q4b-stream-view-{name}"),
            &package_files(&main),
            "main.align",
        );
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{name} unexpectedly accepted in whole/per-unit checking"
        );
    }
}

#[test]
fn sqlite_direct_stream_retains_binds_and_releases_each_native_phase_once() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4b_query

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_prepare_calls() -> i32
  fn align_sqlite_q4a_bind_i64_calls() -> i32
  fn align_sqlite_q4a_bind_text_calls() -> i32
  fn align_sqlite_q4a_bind_blob_calls() -> i32
  fn align_sqlite_q4a_reset_calls() -> i32
  fn align_sqlite_q4a_clear_calls() -> i32
  fn align_sqlite_q4a_finalize_calls() -> i32
  fn align_sqlite_q4a_protocol_ok() -> i32
}

fn execute_once(borrow connection: pkg.db.conn, id: i64, initial_label: str) -> i32 {
  mut label := initial_label.clone()
  label_view: str := label
  mut bytes := [1 as u8, 2 as u8, 3 as u8]
  opened := pkg.db.rows(
    pkg.db.exec_conn(connection),
    app.q4b_query.selected(),
    app.q4b_query.Params { id: id, label: label_view, payload: bytes[..] },
    [],
  )
  return match opened {
    Err(_) => 2
    Ok(rows_value) => {
      mut rows := rows_value
      label = "source storage replaced".clone()
      bytes[0] = 9
      if label.len() == 0 || bytes[0] != 9 { return 3 }
      first := pkg.db.next(rows)
      match first {
        Err(_) => { return 7 }
        Ok(value) => match value {
          None => { return 8 }
          Some(row) => if row.id != id { return 9 }
        }
      }
      second := pkg.db.next(rows)
      match second {
        Err(_) => 10
        Ok(value) => match value { None => 0, Some(_) => 11 }
      }
    }
  }
}

fn run(borrow connection: pkg.db.conn) -> i32 {
  first := execute_once(connection, 7, "first")
  if first != 0 { return first }
  return execute_once(connection, 8, "second")
}

fn main() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  opened := pkg.db.sqlite.connect(":memory:", [])
  result := match opened { Err(_) => 5, Ok(connection) => run(connection) }
  if result != 0 { return result }
  return unsafe {
    if align_sqlite_q4a_protocol_ok() == 1
      && align_sqlite_q4a_prepare_calls() == 2
      && align_sqlite_q4a_bind_i64_calls() == 2
      && align_sqlite_q4a_bind_text_calls() == 2
      && align_sqlite_q4a_bind_blob_calls() == 2
      && align_sqlite_q4a_reset_calls() == 0
      && align_sqlite_q4a_clear_calls() == 0
      && align_sqlite_q4a_finalize_calls() == 2 { 42 } else { 6 }
  }
}
"#;
    let fixture = format!("{POSTGRES_STUB}\n{SQLITE_STUB}");
    let output = build_and_run_multi_with_c(
        "pkg-db-q4b-sqlite-direct-stream",
        &package_files(main),
        "main.align",
        &fixture,
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
fn complete_sqlite_parameter_and_row_matrix_is_exact() {
    if !backend_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4b_query

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
    app.q4b_query.full(),
    app.q4b_query.FullParams {
      b: true,
      nb: nb,
      i16v: -12 as i16,
      ni16: ni16,
      i32v: 3456 as i32,
      ni32: ni32,
      i64v: 7890123,
      ni64: ni64,
      f32v: 1.5 as f32,
      nf32: nf32,
      f64v: -9.75,
      nf64: nf64,
      textv: "hello",
      ntext: ntext,
      bytesv: bytes[..],
      nbytes: nbytes,
    },
    [],
  )
  mut stream := opened else { return 2 }
  first := pkg.db.next(stream) else { return 3 }
  row := first else { return 4 }
  if !row.b || row.i16v != (-12 as i16) || row.i32v != (3456 as i32)
    || row.i64v != 7890123 || row.f32v != (1.5 as f32) || row.f64v != -9.75
    || row.textv != "hello" || row.bytesv.len() != 3 || row.bytesv[0] != 0
    || row.bytesv[1] != 127 || row.bytesv[2] != 255 { return 5 }
  if present {
    vb := row.nb else { return 6 }
    vi16 := row.ni16 else { return 7 }
    vi32 := row.ni32 else { return 8 }
    vi64 := row.ni64 else { return 9 }
    vf32 := row.nf32 else { return 10 }
    vf64 := row.nf64 else { return 11 }
    vt := row.ntext else { return 12 }
    vx := row.nbytes else { return 13 }
    if !vb || vi16 != (-1234 as i16) || vi32 != (-123456 as i32)
      || vi64 != -123456789 || vf32 != (2.5 as f32) || vf64 != -3.25
      || vt != "nullable" || vx.len() != 3 || vx[2] != 255 { return 14 }
  } else {
    match row.nb { Some(_) => { return 15 } None => {} }
    match row.ni16 { Some(_) => { return 16 } None => {} }
    match row.ni32 { Some(_) => { return 17 } None => {} }
    match row.ni64 { Some(_) => { return 18 } None => {} }
    match row.nf32 { Some(_) => { return 19 } None => {} }
    match row.nf64 { Some(_) => { return 20 } None => {} }
    match row.ntext { Some(_) => { return 21 } None => {} }
    match row.nbytes { Some(_) => { return 22 } None => {} }
  }
  exhausted := pkg.db.next(stream) else { return 23 }
  return match exhausted { Some(_) => 24, None => 0 }
}

fn main() -> i32 {
  opened := pkg.db.sqlite.connect(":memory:", [])
  connection := opened else { return 30 }
  first := execute_once(connection, true)
  if first != 0 { return first }
  second := execute_once(connection, false)
  if second != 0 { return second }
  arena out {
    bytes := [4 as u8, 0 as u8, 255 as u8]
    selected := pkg.db.one(
      pkg.db.exec_conn(connection),
      app.q4b_query.full(),
      app.q4b_query.FullParams {
        b: true,
        nb: Some(false),
        i16v: -2 as i16,
        ni16: Some(3 as i16),
        i32v: -4 as i32,
        ni32: Some(5 as i32),
        i64v: -6,
        ni64: Some(7),
        f32v: -1.25 as f32,
        nf32: Some(2.75 as f32),
        f64v: -3.5,
        nf64: Some(4.5),
        textv: "retained",
        ntext: Some("optional"),
        bytesv: bytes[..],
        nbytes: Some(bytes[..]),
      },
      out,
      [],
    )
    row := selected else { return 31 }
    retained_text := row.ntext else { return 32 }
    retained_bytes := row.nbytes else { return 33 }
    if row.textv != "retained" || retained_text != "optional"
      || row.bytesv.len() != 3 || row.bytesv[1] != 0 || row.bytesv[2] != 255
      || retained_bytes.len() != 3 || retained_bytes[0] != 4 { return 34 }
  }
  return 42
}
"#;
    let output = build_and_run_multi_with_static_descriptors(
        "pkg-db-q4b-sqlite-complete-matrix",
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
fn complete_postgres_parameter_and_row_matrix_is_exact() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.q4b_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
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
    app.q4b_query.full_postgres(),
    app.q4b_query.FullParams {
      b: true,
      nb: nb,
      i16v: -12 as i16,
      ni16: ni16,
      i32v: 3456 as i32,
      ni32: ni32,
      i64v: 7890123,
      ni64: ni64,
      f32v: 1.5 as f32,
      nf32: nf32,
      f64v: -9.75,
      nf64: nf64,
      textv: "hello",
      ntext: ntext,
      bytesv: bytes[..],
      nbytes: nbytes,
    },
    [],
  )
  mut stream := opened else { return 2 }
  first := pkg.db.next(stream) else { return 3 }
  row := first else { return 4 }
  if !row.b || row.i16v != (-12 as i16) || row.i32v != (3456 as i32)
    || row.i64v != 7890123 || row.f32v != (1.5 as f32) || row.f64v != -9.75
    || row.textv != "hello" || row.bytesv.len() != 3 || row.bytesv[0] != 0
    || row.bytesv[1] != 127 || row.bytesv[2] != 255 { return 5 }
  if present {
    vb := row.nb else { return 6 }
    vi16 := row.ni16 else { return 7 }
    vi32 := row.ni32 else { return 8 }
    vi64 := row.ni64 else { return 9 }
    vf32 := row.nf32 else { return 10 }
    vf64 := row.nf64 else { return 11 }
    vt := row.ntext else { return 12 }
    vx := row.nbytes else { return 13 }
    if !vb || vi16 != (-1234 as i16) || vi32 != (-123456 as i32)
      || vi64 != -123456789 || vf32 != (2.5 as f32) || vf64 != -3.25
      || vt != "nullable" || vx.len() != 3 || vx[2] != 255 { return 14 }
  } else {
    match row.nb { Some(_) => { return 15 } None => {} }
    match row.ni16 { Some(_) => { return 16 } None => {} }
    match row.ni32 { Some(_) => { return 17 } None => {} }
    match row.ni64 { Some(_) => { return 18 } None => {} }
    match row.nf32 { Some(_) => { return 19 } None => {} }
    match row.nf64 { Some(_) => { return 20 } None => {} }
    match row.ntext { Some(_) => { return 21 } None => {} }
    match row.nbytes { Some(_) => { return 22 } None => {} }
  }
  exhausted := pkg.db.next(stream) else { return 23 }
  return match exhausted { Some(_) => 24, None => 0 }
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  opened := pkg.db.postgres.connect("postgresql://stub/db", [])
  connection := opened else { return 30 }
  first := execute_once(connection, true)
  if first != 0 { return first }
  second := execute_once(connection, false)
  if second != 0 { return second }
  arena out {
    bytes := [4 as u8, 0 as u8, 255 as u8]
    selected := pkg.db.one(
      pkg.db.exec_conn(connection),
      app.q4b_query.full_postgres(),
      app.q4b_query.FullParams {
        b: true, nb: Some(false), i16v: -2 as i16, ni16: Some(3 as i16),
        i32v: -4 as i32, ni32: Some(5 as i32), i64v: -6, ni64: Some(7),
        f32v: -1.25 as f32, nf32: Some(2.75 as f32),
        f64v: -3.5, nf64: Some(4.5), textv: "retained", ntext: Some("optional"),
        bytesv: bytes[..], nbytes: Some(bytes[..]),
      },
      out,
      [],
    )
    row := selected else { return 31 }
    retained_text := row.ntext else { return 32 }
    retained_bytes := row.nbytes else { return 33 }
    if row.textv != "retained" || retained_text != "optional"
      || row.bytesv.len() != 3 || row.bytesv[1] != 0 || row.bytesv[2] != 255
      || retained_bytes.len() != 3 || retained_bytes[0] != 4 { return 34 }
  }
  return unsafe {
    if align_pg_protocol_ok() != 1 { return 40 + align_pg_protocol_error() }
    if align_pg_execute_calls() != 3 { return 36 }
    return 42
  }
}
"#;
    let output = build_and_run_multi_with_c(
        "pkg-db-q4b-postgres-complete-matrix",
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
fn postgres_deadline_cancel_drain_and_poisoning_are_exact() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.q4b_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_nonblocking_calls() -> i32
  fn align_pg_cancel_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn consume(
  borrow connection: pkg.db.conn,
  statement: pkg.db.query<app.q4b_query.DeadlineParams, app.q4b_query.DeadlineRow>,
  options: slice<pkg.db.ExecuteOption>,
) -> Result<i64, pkg.db.Error> {
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), statement,
    app.q4b_query.DeadlineParams { value: 42 }, options,
  )?
  first := pkg.db.next(stream)?
  row := first else {
    return Err(pkg.db.Error.Cardinality(pkg.db.CardinalityError {
      expected_min: 1, expected_max: 1, observed_at_least: 0,
    }))
  }
  exhausted := pkg.db.next(stream)?
  match exhausted {
    Some(_) => {
      return Err(pkg.db.Error.Cardinality(pkg.db.CardinalityError {
        expected_min: 1, expected_max: 1, observed_at_least: 2,
      }))
    }
    None => { return Ok(row.value) }
  }
}

fn timed_out(result: Result<i64, pkg.db.Error>) -> bool = match result {
  Err(error) => match error { Timeout(_) => true, _ => false }
  Ok(_) => false
}

fn poisoned(borrow connection: pkg.db.conn) -> bool {
  result := consume(connection, app.q4b_query.deadline_success(), [])
  return match result {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "db.connection.state"
      _ => false
    }
    Ok(_) => false
  }
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  reusable := pkg.db.postgres.connect("postgresql://stub/reusable", []) else { return 1 }
  immediate := consume(
    reusable, app.q4b_query.deadline_success(),
    [pkg.db.ExecuteOption.TimeoutNs(100000000)],
  ) else { return 2 }
  if immediate != 42 { return 3 }
  expired := consume(
    reusable, app.q4b_query.deadline_wait(),
    [pkg.db.ExecuteOption.TimeoutNs(1000000)],
  )
  if !timed_out(expired) { return 4 }
  reused := consume(reusable, app.q4b_query.deadline_success(), []) else { return 5 }
  if reused != 42 { return 6 }

  duplicate := consume(
    reusable, app.q4b_query.deadline_success(),
    [pkg.db.ExecuteOption.TimeoutNs(1), pkg.db.ExecuteOption.TimeoutNs(2)],
  )
  duplicate_ok := match duplicate {
    Err(error) => match error {
      Unsupported(contract) => contract.message == "duplicate database execution timeout"
      _ => false
    }
    Ok(_) => false
  }
  if !duplicate_ok { return 7 }

  cancel_connection := pkg.db.postgres.connect("postgresql://stub/cancel", []) else { return 8 }
  cancel_failed := consume(
    cancel_connection, app.q4b_query.deadline_cancel_fail(),
    [pkg.db.ExecuteOption.TimeoutNs(1000000)],
  )
  if !timed_out(cancel_failed) || !poisoned(cancel_connection) { return 9 }

  drain_connection := pkg.db.postgres.connect("postgresql://stub/drain", []) else { return 10 }
  drain_failed := consume(
    drain_connection, app.q4b_query.deadline_drain_fail(),
    [pkg.db.ExecuteOption.TimeoutNs(1000000)],
  )
  if !timed_out(drain_failed) || !poisoned(drain_connection) { return 11 }

  return unsafe {
    if align_pg_protocol_ok() != 1 { return 12 }
    if align_pg_execute_calls() != 5 { return 13 }
    if align_pg_cancel_calls() != 3 { return 14 }
    if align_pg_nonblocking_calls() != 8 { return 15 }
    if align_pg_clear_calls() != 5 { return 16 }
    if align_pg_finish_calls() != 2 { return 17 }
    return 42
  }
}
"#;
    let output = build_and_run_multi_with_c(
        "pkg-db-q4b-postgres-deadline-cancel",
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
fn owned_text_and_bytes_params_bind_before_their_sources_drop() {
    if !backend_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4b_query

fn main() -> i32 {
  connection := pkg.db.sqlite.connect(":memory:", []) else { return 1 }
  mut bytes_builder: array_builder<u8> := array_builder()
  bytes_builder.push(0 as u8)
  bytes_builder.push(127 as u8)
  bytes_builder.push(255 as u8)
  bytes := bytes_builder.build()
  mut nullable_builder: array_builder<u8> := array_builder()
  nullable_builder.push(4 as u8)
  nullable_builder.push(0 as u8)
  nullable_builder.push(9 as u8)
  nullable := nullable_builder.build()
  opened := pkg.db.rows(
    pkg.db.exec_conn(connection),
    app.q4b_query.owned(),
    app.q4b_query.OwnedParams {
      textv: "owned text".clone(),
      ntext: Some("optional text".clone()),
      bytesv: bytes,
      nbytes: Some(nullable),
    },
    [],
  )
  mut stream := opened else { return 2 }
  first := pkg.db.next(stream) else { return 3 }
  row := first else { return 4 }
  nullable_text := row.ntext else { return 5 }
  nullable_bytes := row.nbytes else { return 6 }
  if row.textv != "owned text" || nullable_text != "optional text" { return 7 }
  if row.bytesv.len() != 3 || row.bytesv[0] != 0
    || row.bytesv[1] != 127 || row.bytesv[2] != 255 { return 8 }
  if nullable_bytes.len() != 3 || nullable_bytes[0] != 4
    || nullable_bytes[1] != 0 || nullable_bytes[2] != 9 { return 9 }
  exhausted := pkg.db.next(stream) else { return 10 }
  return match exhausted { Some(_) => 11, None => 42 }
}
"#;
    let output = build_and_run_multi_with_static_descriptors(
        "pkg-db-q4b-owned-params",
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
fn sqlite_stream_lifecycle_and_overlap_are_exact() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4b_query

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_prepare_calls() -> i32
  fn align_sqlite_q4a_reset_calls() -> i32
  fn align_sqlite_q4a_clear_calls() -> i32
  fn align_sqlite_q4a_finalize_calls() -> i32
  fn align_sqlite_q4a_busy_timeout_calls() -> i32
  fn align_sqlite_q4a_last_busy_timeout() -> i32
  fn align_sqlite_q4a_fail_next_busy_timeout()
  fn align_sqlite_q4a_protocol_ok() -> i32
}

fn params(id: i64, label: str, payload: slice<u8>) -> app.q4b_query.Params {
  return app.q4b_query.Params { id: id, label: label, payload: payload }
}

fn overlap_error<R>(result: Result<pkg.db.rows<R>, pkg.db.Error>) -> bool = match result {
  Err(error) => match error {
    Unsupported(contract) => contract.item == "sqlite.connection.active_execution"
    _ => false
  }
  Ok(_) => false
}

fn ordinary_then_timeout(borrow connection: pkg.db.conn) -> i32 {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut first := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.selected(), params(7, "first", bytes[..]), [],
  ) else { return 1 }
  second := pkg.db.sqlite.rows_native(
    pkg.db.exec_conn(connection), app.q4b_query.selected(), params(8, "second", bytes[..]), [],
    [pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(7000000)],
  )
  if !overlap_error(second) { return 2 }
  // `first` drops on return and owns the direct statement finalization.
  return 0
}

fn timeout_then_ordinary(borrow connection: pkg.db.conn) -> i32 {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut first := pkg.db.sqlite.rows_native(
    pkg.db.exec_conn(connection), app.q4b_query.selected(), params(7, "first", bytes[..]), [],
    [pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(7000000)],
  ) else { return 3 }
  second := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.selected(), params(8, "second", bytes[..]), [],
  )
  if !overlap_error(second) { return 4 }
  return 0
}

fn prepared_early(
  borrow mut statement: pkg.db.stmt<app.q4b_query.Params, app.q4b_query.Row>,
) -> i32 {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut stream := pkg.db.rows_stmt(statement, params(7, "first", bytes[..]), []) else { return 5 }
  return 0
}

fn prepared_exhaust(
  borrow mut statement: pkg.db.stmt<app.q4b_query.Params, app.q4b_query.Row>,
) -> i32 {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut stream := pkg.db.rows_stmt(statement, params(8, "second", bytes[..]), []) else { return 6 }
  first := pkg.db.next(stream) else { return 7 }
  row := first else { return 8 }
  if row.id != 8 { return 9 }
  exhausted := pkg.db.next(stream) else { return 10 }
  return match exhausted { Some(_) => 11, None => 0 }
}

fn prepared_phase(borrow connection: pkg.db.conn) -> i32 {
  mut statement := pkg.db.sqlite.prepare_native(
    pkg.db.exec_conn(connection), app.q4b_query.selected(), [],
    [pkg.db.sqlite.PrepareOption.Persistent, pkg.db.sqlite.PrepareOption.Normalize],
  ) else { return 12 }
  early := prepared_early(statement)
  if early != 0 { return early }
  return prepared_exhaust(statement)
}

fn poison_restore(borrow connection: pkg.db.conn) -> i32 {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut stream := pkg.db.sqlite.rows_native(
    pkg.db.exec_conn(connection), app.q4b_query.selected(), params(7, "first", bytes[..]), [],
    [pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(9000000)],
  ) else { return 13 }
  unsafe { align_sqlite_q4a_fail_next_busy_timeout() }
  return 0
}

fn closed_after_restore_failure(borrow connection: pkg.db.conn) -> bool {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  result := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.selected(), params(7, "first", bytes[..]), [],
  )
  return match result {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "db.connection.state"
        || contract.item == "db.exec.state"
      _ => false
    }
    Ok(_) => false
  }
}

fn run(borrow connection: pkg.db.conn) -> i32 {
  first := ordinary_then_timeout(connection)
  if first != 0 { return first }
  second := timeout_then_ordinary(connection)
  if second != 0 { return second }
  prepared := prepared_phase(connection)
  if prepared != 0 { return prepared }
  poisoned := poison_restore(connection)
  if poisoned != 0 { return poisoned }
  if !closed_after_restore_failure(connection) { return 14 }
  return 0
}

fn main() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  connection := pkg.db.sqlite.connect(":memory:", []) else { return 15 }
  result := run(connection)
  if result != 0 { return result }
  return unsafe {
    if align_sqlite_q4a_protocol_ok() != 1 { return 16 }
    if align_sqlite_q4a_prepare_calls() != 4 { return 17 }
    if align_sqlite_q4a_finalize_calls() != 4 { return 18 }
    if align_sqlite_q4a_reset_calls() != 2 { return 19 }
    if align_sqlite_q4a_clear_calls() != 2 { return 20 }
    if align_sqlite_q4a_busy_timeout_calls() != 4 { return 21 }
    if align_sqlite_q4a_last_busy_timeout() != 0 { return 22 }
    return 42
  }
}
"#;
    let fixture = format!("{POSTGRES_STUB}\n{SQLITE_STUB}");
    let output = build_and_run_multi_with_c(
        "pkg-db-q4b-sqlite-stream-lifecycle",
        &package_files(main),
        "main.align",
        &fixture,
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
fn postgres_buffered_stream_lifecycle_is_exact() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.q4b_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_prepare_calls() -> i32
  fn align_pg_execute_prepared_calls() -> i32
  fn align_pg_control_calls() -> i32
  fn align_pg_deallocate_calls() -> i32
  fn align_pg_protocol_ok() -> i32
  fn align_pg_protocol_error() -> i32
}

fn deadline_params() -> app.q4b_query.DeadlineParams {
  return app.q4b_query.DeadlineParams { value: 42 }
}

fn prepared_params(id: i64, label: str, payload: slice<u8>) -> app.q4b_query.Params {
  return app.q4b_query.Params { id: id, label: label, payload: payload }
}

fn overlap_error<R>(result: Result<pkg.db.rows<R>, pkg.db.Error>) -> bool = match result {
  Err(error) => match error {
    Unsupported(contract) => contract.item == "postgres.connection.active_execution"
    _ => false
  }
  Ok(_) => false
}

fn direct_early(borrow connection: pkg.db.conn) -> i32 {
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.deadline_success(), deadline_params(), [],
  ) else { return 1 }
  return 0
}

fn direct_exhaust(borrow connection: pkg.db.conn) -> i32 {
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.deadline_success(), deadline_params(), [],
  ) else { return 2 }
  first := pkg.db.next(stream) else { return 3 }
  row := first else { return 4 }
  if row.value != 42 { return 5 }
  exhausted := pkg.db.next(stream) else { return 6 }
  return match exhausted { Some(_) => 7, None => 0 }
}

fn direct_decode_error(borrow connection: pkg.db.conn) -> i32 {
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.malformed_postgres(), deadline_params(), [],
  ) else { return 8 }
  decoded := pkg.db.next(stream)
  return match decoded {
    Err(error) => match error { Decode(_) => 0, _ => 9 }
    Ok(_) => 10
  }
}

fn direct_overlap(borrow connection: pkg.db.conn) -> i32 {
  mut first := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.deadline_success(), deadline_params(), [],
  ) else { return 11 }
  second := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.deadline_success(), deadline_params(), [],
  )
  if !overlap_error(second) { return 12 }
  return 0
}

fn prepared_early_pg(
  borrow mut statement: pkg.db.stmt<app.q4b_query.Params, app.q4b_query.Row>,
) -> i32 {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut stream := pkg.db.rows_stmt(
    statement, prepared_params(7, "first", bytes[..]), [],
  ) else { return 13 }
  return 0
}

fn prepared_exhaust_pg(
  borrow mut statement: pkg.db.stmt<app.q4b_query.Params, app.q4b_query.Row>,
) -> i32 {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut stream := pkg.db.rows_stmt(
    statement, prepared_params(8, "second", bytes[..]), [],
  ) else { return 14 }
  first := pkg.db.next(stream) else { return 15 }
  row := first else { return 16 }
  if row.id != 0 { return 17 }
  exhausted := pkg.db.next(stream) else { return 18 }
  return match exhausted { Some(_) => 19, None => 0 }
}

fn prepared_phase_pg(borrow connection: pkg.db.conn) -> i32 {
  mut statement := pkg.db.prepare(
    pkg.db.exec_conn(connection), app.q4b_query.selected_postgres(), [],
  ) else { return 20 }
  early := prepared_early_pg(statement)
  if early != 0 { return early }
  return prepared_exhaust_pg(statement)
}

fn tx_rows(borrow transaction: pkg.db.tx) -> i32 {
  mut stream := pkg.db.rows(
    pkg.db.exec_tx(transaction), app.q4b_query.deadline_success(), deadline_params(), [],
  ) else { return 21 }
  first := pkg.db.next(stream) else { return 22 }
  row := first else { return 23 }
  if row.value != 42 { return 24 }
  exhausted := pkg.db.next(stream) else { return 25 }
  return match exhausted { Some(_) => 26, None => 0 }
}

fn run(connection: pkg.db.conn) -> i32 {
  early := direct_early(connection)
  if early != 0 { return early }
  exhausted := direct_exhaust(connection)
  if exhausted != 0 { return exhausted }
  malformed := direct_decode_error(connection)
  if malformed != 0 { return malformed }
  overlap := direct_overlap(connection)
  if overlap != 0 { return overlap }
  reused := direct_exhaust(connection)
  if reused != 0 { return reused }
  prepared := prepared_phase_pg(connection)
  if prepared != 0 { return prepared }
  transaction := pkg.db.begin(connection, []) else { return 27 }
  tx_result := tx_rows(transaction)
  if tx_result != 0 { return tx_result }
  returned := pkg.db.rollback(transaction) else { return 28 }
  return 0
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/lifecycle", []) else { return 29 }
  result := run(connection)
  if result != 0 { return result }
  return unsafe {
    if align_pg_protocol_ok() != 1 { return 30 + align_pg_protocol_error() }
    if align_pg_execute_calls() != 6 { return 50 }
    if align_pg_prepare_calls() != 1 { return 51 }
    if align_pg_execute_prepared_calls() != 2 { return 52 }
    if align_pg_control_calls() != 2 { return 53 }
    if align_pg_deallocate_calls() != 1 { return 54 }
    if align_pg_clear_calls() != 12 { return 55 }
    if align_pg_finish_calls() != 1 { return 56 }
    return 42
  }
}
"#;
    let output = build_and_run_multi_with_c(
        "pkg-db-q4b-postgres-buffered-lifecycle",
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
fn postgres_prepared_deadline_recovers_for_reuse() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.q4b_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_clear_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_prepare_calls() -> i32
  fn align_pg_execute_prepared_calls() -> i32
  fn align_pg_nonblocking_calls() -> i32
  fn align_pg_cancel_calls() -> i32
  fn align_pg_deallocate_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn params(id: i64, label: str, payload: slice<u8>) -> app.q4b_query.Params {
  return app.q4b_query.Params { id: id, label: label, payload: payload }
}

fn expires(
  borrow mut statement: pkg.db.stmt<app.q4b_query.Params, app.q4b_query.Row>,
) -> bool {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  result := pkg.db.rows_stmt(
    statement,
    params(7, "first", bytes[..]),
    [pkg.db.ExecuteOption.TimeoutNs(1000000)],
  )
  return match result {
    Err(error) => match error { Timeout(_) => true, _ => false }
    Ok(_) => false
  }
}

fn reuse(
  borrow mut statement: pkg.db.stmt<app.q4b_query.Params, app.q4b_query.Row>,
) -> i32 {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut stream := pkg.db.rows_stmt(
    statement, params(8, "second", bytes[..]), [],
  ) else { return 1 }
  first := pkg.db.next(stream) else { return 2 }
  row := first else { return 3 }
  if row.id != 0 { return 4 }
  exhausted := pkg.db.next(stream) else { return 5 }
  return match exhausted { Some(_) => 6, None => 0 }
}

fn run(connection: pkg.db.conn) -> i32 {
  mut statement := pkg.db.prepare(
    pkg.db.exec_conn(connection), app.q4b_query.selected_postgres_timeout(), [],
  ) else { return 7 }
  if !expires(statement) { return 8 }
  return reuse(statement)
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/prepared-deadline", []) else {
    return 9
  }
  result := run(connection)
  if result != 0 { return result }
  return unsafe {
    if align_pg_protocol_ok() != 1 { return 10 }
    if align_pg_prepare_calls() != 1 { return 11 }
    if align_pg_execute_prepared_calls() != 2 { return 12 }
    if align_pg_nonblocking_calls() != 2 { return 13 }
    if align_pg_cancel_calls() != 1 { return 14 }
    if align_pg_deallocate_calls() != 1 { return 15 }
    if align_pg_clear_calls() != 4 { return 16 }
    if align_pg_finish_calls() != 1 { return 17 }
    return 42
  }
}
"#;
    let output = build_and_run_multi_with_c(
        "pkg-db-q4b-postgres-prepared-deadline",
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
fn postgres_command_deadline_is_enforced_and_recovers_for_reuse() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.q4b_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_nonblocking_calls() -> i32
  fn align_pg_cancel_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn params() -> app.q4b_query.DeadlineParams {
  return app.q4b_query.DeadlineParams { value: 42 }
}

fn run(connection: pkg.db.conn) -> i32 {
  immediate := pkg.db.execute(
    pkg.db.exec_conn(connection), app.q4b_query.command_success(), params(),
    [pkg.db.ExecuteOption.TimeoutNs(100000000)],
  ) else { return 1 }
  affected := immediate.rows_affected else { return 2 }
  if affected != 2 { return 3 }
  expired := pkg.db.execute(
    pkg.db.exec_conn(connection), app.q4b_query.command_wait(), params(),
    [pkg.db.ExecuteOption.TimeoutNs(1000000)],
  )
  timeout := match expired {
    Err(error) => match error { Timeout(_) => true, _ => false }
    Ok(_) => false
  }
  if !timeout { return 4 }
  reused := pkg.db.execute(
    pkg.db.exec_conn(connection), app.q4b_query.command_success(), params(), [],
  ) else { return 5 }
  reused_affected := reused.rows_affected else { return 6 }
  return if reused_affected == 2 { 0 } else { 7 }
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/command-deadline", []) else {
    return 8
  }
  result := run(connection)
  if result != 0 { return result }
  return unsafe {
    if align_pg_protocol_ok() != 1 { return 9 }
    if align_pg_execute_calls() != 3 { return 10 }
    if align_pg_nonblocking_calls() != 4 { return 11 }
    if align_pg_cancel_calls() != 1 { return 12 }
    if align_pg_clear_calls() != 3 { return 13 }
    if align_pg_finish_calls() != 1 { return 14 }
    return 42
  }
}
"#;
    let output = build_and_run_multi_with_c(
        "pkg-db-q4b-postgres-command-deadline",
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
fn malformed_native_view_values_fail_before_safe_view_formation() {
    if !backend_available() || !cc_available() {
        return;
    }
    let sqlite_main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4b_query

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_set_row_fault(fault: i32)
  fn align_sqlite_q4a_prepare_calls() -> i32
  fn align_sqlite_q4a_finalize_calls() -> i32
  fn align_sqlite_q4a_protocol_ok() -> i32
}

fn rejects(borrow connection: pkg.db.conn, fault: i32) -> bool {
  unsafe { align_sqlite_q4a_set_row_fault(fault) }
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.viewed(),
    app.q4b_query.Params { id: 7, label: "first", payload: bytes[..] }, [],
  ) else { return false }
  result := pkg.db.next(stream)
  return match result {
    Err(error) => match error { Decode(_) => true, _ => false }
    Ok(_) => false
  }
}

fn clean(borrow connection: pkg.db.conn) -> bool {
  unsafe { align_sqlite_q4a_set_row_fault(0) }
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.viewed(),
    app.q4b_query.Params { id: 7, label: "first", payload: bytes[..] }, [],
  ) else { return false }
  first := pkg.db.next(stream) else { return false }
  row := first else { return false }
  if row.label != "view" || row.payload.len() != 3 || row.payload[2] != 3 { return false }
  exhausted := pkg.db.next(stream) else { return false }
  return match exhausted { Some(_) => false, None => true }
}

fn main() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  connection := pkg.db.sqlite.connect(":memory:", []) else { return 1 }
  mut fault: i32 := 1
  loop {
    if fault > 5 { break }
    if !rejects(connection, fault) { return 10 + fault }
    fault = fault + 1
  }
  if !clean(connection) { return 20 }
  return unsafe {
    if align_sqlite_q4a_protocol_ok() != 1 { return 21 }
    if align_sqlite_q4a_prepare_calls() != 6 { return 22 }
    if align_sqlite_q4a_finalize_calls() != 6 { return 23 }
    return 42
  }
}
"#;
    let sqlite_fixture = format!("{POSTGRES_STUB}\n{SQLITE_STUB}");
    let sqlite = build_and_run_multi_with_c(
        "pkg-db-q4b-sqlite-malformed-views",
        &package_files(sqlite_main),
        "main.align",
        &sqlite_fixture,
    );
    assert_eq!(
        sqlite.status.code(),
        Some(42),
        "SQLite status: {:?}; stdout: {}; stderr: {}",
        sqlite.status,
        String::from_utf8_lossy(&sqlite.stdout),
        String::from_utf8_lossy(&sqlite.stderr),
    );

    let postgres_main = r#"module main
import pkg.db
import pkg.db.postgres
import app.q4b_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn rejects(
  borrow connection: pkg.db.conn,
  statement: pkg.db.query<app.q4b_query.Params, app.q4b_query.ViewRow>,
) -> bool {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), statement,
    app.q4b_query.Params { id: 7, label: "first", payload: bytes[..] }, [],
  ) else {
    return false
  }
  result := pkg.db.next(stream)
  return match result {
    Err(error) => match error { Decode(_) => true, _ => false }
    Ok(_) => false
  }
}

fn clean(borrow connection: pkg.db.conn) -> bool {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_query.viewed_postgres_clean(),
    app.q4b_query.Params { id: 7, label: "first", payload: bytes[..] }, [],
  ) else { return false }
  first := pkg.db.next(stream) else { return false }
  row := first else { return false }
  if row.label != "view" || row.payload.len() != 3 || row.payload[2] != 3 { return false }
  exhausted := pkg.db.next(stream) else { return false }
  return match exhausted { Some(_) => false, None => true }
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/malformed-views", []) else {
    return 1
  }
  if !rejects(connection, app.q4b_query.viewed_postgres_text_null()) { return 2 }
  if !rejects(connection, app.q4b_query.viewed_postgres_text_length()) { return 3 }
  if !rejects(connection, app.q4b_query.viewed_postgres_text_utf8()) { return 4 }
  if !rejects(connection, app.q4b_query.viewed_postgres_bytes_null()) { return 5 }
  if !rejects(connection, app.q4b_query.viewed_postgres_bytes_length()) { return 6 }
  if !rejects(connection, app.q4b_query.viewed_postgres_bytes_hex()) { return 7 }
  if !clean(connection) { return 8 }
  return unsafe {
    if align_pg_protocol_ok() != 1 { return 9 }
    if align_pg_execute_calls() != 7 { return 10 }
    if align_pg_clear_calls() != 7 { return 11 }
    return 42
  }
}
"#;
    let postgres = build_and_run_multi_with_c(
        "pkg-db-q4b-postgres-malformed-views",
        &package_files(postgres_main),
        "main.align",
        POSTGRES_STUB,
    );
    assert_eq!(
        postgres.status.code(),
        Some(42),
        "PostgreSQL status: {:?}; stdout: {}; stderr: {}",
        postgres.status,
        String::from_utf8_lossy(&postgres.stdout),
        String::from_utf8_lossy(&postgres.stderr),
    );
}

#[test]
#[ignore = "local D8 scalar/text streaming measurement; not a correctness or CI gate"]
fn million_row_streaming_measurement_reports_delivery_counts() {
    if !backend_available() {
        return;
    }
    let queries = r#"module app.q4b_bench
import pkg.db
import pkg.db.sqlite

pub Params { limit: i64 }
pub ScalarRow { value: i64 }
pub TextRow { value: str }

pub fn scalar() -> pkg.db.query<Params, ScalarRow> = pkg.db.sqlite.query(
  "WITH RECURSIVE seq(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM seq WHERE value < :limit) SELECT value AS value FROM seq",
  [],
  [],
)

pub fn text() -> pkg.db.query<Params, TextRow> = pkg.db.sqlite.query(
  "WITH RECURSIVE seq(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM seq WHERE value < :limit) SELECT 'row' AS value FROM seq",
  [],
  [],
)
"#;
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4b_bench
import std.time

fn scalar(borrow connection: pkg.db.conn, limit: i64) -> Result<(), pkg.db.Error> {
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_bench.scalar(),
    app.q4b_bench.Params { limit: limit }, [],
  )?
  start := time.instant()
  mut count: i64 := 0
  mut checksum: i64 := 0
  loop {
    current := pkg.db.next(stream)?
    row := current else { break }
    count = count + 1
    checksum = checksum + row.value
  }
  print(count)
  print(checksum)
  print(time.instant() - start)
  return Ok(())
}

fn text_rows(borrow connection: pkg.db.conn, limit: i64) -> Result<(), pkg.db.Error> {
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.q4b_bench.text(),
    app.q4b_bench.Params { limit: limit }, [],
  )?
  start := time.instant()
  mut count: i64 := 0
  mut delivered_bytes: i64 := 0
  loop {
    current := pkg.db.next(stream)?
    row := current else { break }
    count = count + 1
    delivered_bytes = delivered_bytes + row.value.len()
  }
  print(count)
  print(delivered_bytes)
  print(time.instant() - start)
  return Ok(())
}

fn main() -> i32 {
  connection := pkg.db.sqlite.connect(":memory:", []) else { return 1 }
  scalar_result := scalar(connection, 1000000)
  match scalar_result { Err(_) => { return 2 } Ok(_) => {} }
  text_result := text_rows(connection, 1000000)
  match text_result { Err(_) => { return 3 } Ok(_) => {} }
  return 0
}
"#;
    let mut files = package_files(main);
    files.push(("app/q4b_bench.align", queries));
    let output = build_and_run_multi_with_static_descriptors(
        "pkg-db-q4b-million-row-measurement",
        &files,
        "main.align",
    );
    assert!(
        output.status.success(),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let values = String::from_utf8(output.stdout)
        .expect("measurement output UTF-8")
        .lines()
        .map(|line| line.parse::<u64>().expect("measurement integer"))
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 6);
    assert_eq!(values[0], 1_000_000);
    assert_eq!(values[1], 500_000_500_000);
    assert_eq!(values[3], 1_000_000);
    assert_eq!(values[4], 3_000_000);
    println!(
        "sqlite-stream-scalar\t{:.2}\tns/row\nsqlite-stream-text\t{:.2}\tns/row",
        values[2] as f64 / 1_000_000.0,
        values[5] as f64 / 1_000_000.0,
    );
}

#[test]
#[ignore = "local D9 deadline/cancellation measurement; not a correctness or CI gate"]
fn postgres_deadline_overhead_measurement_reports_native_counts() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.q4b_query
import std.time

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_nonblocking_calls() -> i32
  fn align_pg_cancel_calls() -> i32
}

fn params() -> app.q4b_query.DeadlineParams {
  return app.q4b_query.DeadlineParams { value: 42 }
}

fn run(
  borrow connection: pkg.db.conn,
  iterations: i64,
  timed: bool,
) -> Result<i64, pkg.db.Error> {
  start := time.instant()
  mut index: i64 := 0
  loop {
    if index >= iterations { break }
    if timed {
      result := pkg.db.execute(
        pkg.db.exec_conn(connection), app.q4b_query.command_success(), params(),
        [pkg.db.ExecuteOption.TimeoutNs(1000000000)],
      )?
    } else {
      result := pkg.db.execute(
        pkg.db.exec_conn(connection), app.q4b_query.command_success(), params(), [],
      )?
    }
    index = index + 1
  }
  return Ok(time.instant() - start)
}

fn cancelled(borrow connection: pkg.db.conn, iterations: i64) -> i64 {
  start := time.instant()
  mut index: i64 := 0
  loop {
    if index >= iterations { break }
    result := pkg.db.execute(
      pkg.db.exec_conn(connection), app.q4b_query.command_wait(), params(),
      [pkg.db.ExecuteOption.TimeoutNs(1000000)],
    )
    match result {
      Err(error) => match error { Timeout(_) => {}, _ => { return -1 } }
      Ok(_) => { return -1 }
    }
    index = index + 1
  }
  return time.instant() - start
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/deadline-bench", []) else {
    return 1
  }
  iterations: i64 := 10000
  ordinary := run(connection, iterations, false) else { return 2 }
  timed := run(connection, iterations, true) else { return 3 }
  cancellation := cancelled(connection, 16)
  if cancellation < 0 { return 4 }
  print(ordinary)
  print(timed)
  print(cancellation)
  return unsafe {
    if align_pg_execute_calls() != 20016 { return 5 }
    if align_pg_nonblocking_calls() != 20032 { return 6 }
    if align_pg_cancel_calls() != 16 { return 7 }
    return 0
  }
}
"#;
    let output = build_and_run_multi_with_c(
        "pkg-db-q4b-deadline-measurement",
        &package_files(main),
        "main.align",
        POSTGRES_STUB,
    );
    assert!(
        output.status.success(),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let values = String::from_utf8(output.stdout)
        .expect("measurement output UTF-8")
        .lines()
        .map(|line| line.parse::<u64>().expect("measurement nanoseconds"))
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 3);
    println!(
        "postgres-command-ordinary\t{:.2}\tns/op\npostgres-command-deadline\t{:.2}\tns/op\npostgres-command-cancel\t{:.2}\tns/op",
        values[0] as f64 / 10_000.0,
        values[1] as f64 / 10_000.0,
        values[2] as f64 / 16.0,
    );
}

#[test]
fn deadline_disposition_and_precedence_are_exact() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import pkg.db.postgres
import app.q4b_query

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_prepare_calls() -> i32
  fn align_sqlite_q4a_protocol_ok() -> i32
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_prepare_calls() -> i32
  fn align_pg_control_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn contract<T>(result: Result<T, pkg.db.Error>, item: str, message: str) -> bool = match result {
  Err(error) => match error {
    Unsupported(value) => value.item == item && value.message == message
    _ => false
  }
  Ok(_) => false
}

fn params() -> app.q4b_query.DeadlineParams {
  return app.q4b_query.DeadlineParams { value: 42 }
}

fn sqlite_params(payload: slice<u8>) -> app.q4b_query.Params {
  return app.q4b_query.Params { id: 7, label: "first", payload: payload }
}

fn sqlite_begin_duplicate(connection: pkg.db.conn) -> bool {
  result := pkg.db.begin(connection, [
    pkg.db.TxOption.BeginTimeoutNs(1),
    pkg.db.TxOption.BeginTimeoutNs(2),
  ])
  return contract(
    result,
    "db.transaction.begin_timeout_ns",
    "duplicate database transaction begin timeout",
  )
}

fn postgres_begin_unsupported(connection: pkg.db.conn) -> bool {
  result := pkg.db.postgres.begin_native(
    connection,
    [pkg.db.TxOption.BeginTimeoutNs(1)],
    [pkg.db.postgres.TxOption.Deferrable(true)],
  )
  return contract(
    result,
    "db.transaction.begin_timeout_ns",
    "PostgreSQL transaction begin deadlines are not supported in v1",
  )
}

fn inspect(
  borrow sqlite_connection: pkg.db.conn,
  borrow postgres_connection: pkg.db.conn,
) -> i32 {
  sqlite_target := pkg.db.exec_conn(sqlite_connection)
  postgres_target := pkg.db.exec_conn(postgres_connection)
  bytes := [1 as u8, 2 as u8, 3 as u8]
  sqlite_duplicate := pkg.db.rows(
    sqlite_target, app.q4b_query.selected(), sqlite_params(bytes[..]),
    [pkg.db.ExecuteOption.TimeoutNs(1), pkg.db.ExecuteOption.TimeoutNs(2)],
  )
  if !contract(
    sqlite_duplicate,
    "db.execute.timeout_ns",
    "duplicate database execution timeout",
  ) { return 1 }
  sqlite_native := pkg.db.sqlite.rows_native(
    sqlite_target, app.q4b_query.selected(), sqlite_params(bytes[..]),
    [pkg.db.ExecuteOption.TimeoutNs(1)],
    [pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(0)],
  )
  if !contract(
    sqlite_native,
    "db.execute.timeout_ns",
    "SQLite does not support common execution deadlines",
  ) { return 2 }
  sqlite_prepare := pkg.db.prepare(
    sqlite_target, app.q4b_query.selected(),
    [pkg.db.PrepareOption.TimeoutNs(1), pkg.db.PrepareOption.TimeoutNs(2)],
  )
  if !contract(
    sqlite_prepare,
    "db.prepare.timeout_ns",
    "duplicate database prepare timeout",
  ) { return 3 }
  sqlite_prepare_native := pkg.db.sqlite.prepare_native(
    sqlite_target, app.q4b_query.selected(), [pkg.db.PrepareOption.TimeoutNs(1)],
    [pkg.db.sqlite.PrepareOption.Persistent, pkg.db.sqlite.PrepareOption.Persistent],
  )
  if !contract(
    sqlite_prepare_native,
    "db.prepare.timeout_ns",
    "SQLite does not support common prepare deadlines",
  ) { return 4 }

  postgres_duplicate := pkg.db.postgres.rows_native(
    postgres_target, app.q4b_query.deadline_success(), params(),
    [pkg.db.ExecuteOption.TimeoutNs(1), pkg.db.ExecuteOption.TimeoutNs(2)],
    [pkg.db.postgres.ExecuteOption.ResultFormat(pkg.db.postgres.Format.Binary)],
  )
  if !contract(
    postgres_duplicate,
    "db.execute.timeout_ns",
    "duplicate database execution timeout",
  ) { return 5 }
  postgres_prepare := pkg.db.postgres.prepare_native(
    postgres_target, app.q4b_query.selected_postgres(),
    [pkg.db.PrepareOption.TimeoutNs(1)],
    [pkg.db.postgres.PrepareOption.ParameterOid("missing", 0 as u32)],
  )
  if !contract(
    postgres_prepare,
    "db.prepare.timeout_ns",
    "PostgreSQL prepare deadlines are not supported in v1",
  ) { return 6 }

  arena out {
    sqlite_meta := pkg.db.sqlite.meta_database_native(
      sqlite_target, pkg.db.MetaDetail.Names, out,
      [pkg.db.MetaOption.TimeoutNs(1)],
      [pkg.db.sqlite.MetaOption.IncludeInternalObjects],
    )
    if !contract(
      sqlite_meta, "db.meta.timeout_ns", "SQLite does not support common metadata deadlines",
    ) { return 7 }
    postgres_meta := pkg.db.postgres.meta_database_native(
      postgres_target, pkg.db.MetaDetail.Names, out,
      [pkg.db.MetaOption.TimeoutNs(1)],
      [
        pkg.db.postgres.MetaOption.SearchPathOnly,
        pkg.db.postgres.MetaOption.IncludeSystemCatalogs,
      ],
    )
    if !contract(
      postgres_meta,
      "db.meta.timeout_ns",
      "PostgreSQL metadata deadlines are not supported in v1",
    ) { return 8 }
    sqlite_explain := pkg.db.sqlite.explain_native(
      sqlite_target, app.q4b_query.selected(), sqlite_params(bytes[..]), out,
      [pkg.db.ExplainOption.TimeoutNs(1)],
      [pkg.db.sqlite.ExplainOption.QueryPlan, pkg.db.sqlite.ExplainOption.Bytecode],
    )
    if !contract(
      sqlite_explain,
      "db.explain.timeout_ns",
      "SQLite does not support common EXPLAIN deadlines",
    ) { return 9 }
    postgres_explain := pkg.db.postgres.explain_native(
      postgres_target, app.q4b_query.deadline_success(), params(), out,
      [pkg.db.ExplainOption.TimeoutNs(1)],
      [pkg.db.postgres.ExplainOption.Buffers(true)],
    )
    if !contract(
      postgres_explain,
      "db.explain.timeout_ns",
      "PostgreSQL EXPLAIN deadlines are not supported in v1",
    ) { return 10 }
  }
  return 0
}

fn main() -> i32 {
  unsafe { align_sqlite_q4a_reset(); align_pg_reset() }
  sqlite_connection := pkg.db.sqlite.connect(":memory:", []) else { return 11 }
  postgres_connection := pkg.db.postgres.connect("postgresql://stub/disposition", []) else {
    return 12
  }
  inspected := inspect(sqlite_connection, postgres_connection)
  if inspected != 0 { return inspected }
  sqlite_begin_connection := pkg.db.sqlite.connect(":memory:", []) else { return 13 }
  if !sqlite_begin_duplicate(sqlite_begin_connection) { return 14 }
  postgres_begin_connection := pkg.db.postgres.connect("postgresql://stub/begin", []) else {
    return 15
  }
  if !postgres_begin_unsupported(postgres_begin_connection) { return 16 }
  return unsafe {
    if align_sqlite_q4a_protocol_ok() != 1 || align_pg_protocol_ok() != 1 { return 17 }
    if align_sqlite_q4a_prepare_calls() != 0 { return 18 }
    if align_pg_execute_calls() != 0 || align_pg_prepare_calls() != 0
      || align_pg_control_calls() != 0 { return 19 }
    return 42
  }
}
"#;
    let fixture = format!("{POSTGRES_STUB}\n{SQLITE_STUB}");
    let output = build_and_run_multi_with_c(
        "pkg-db-q4b-deadline-disposition",
        &package_files(main),
        "main.align",
        &fixture,
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
fn postgres_deadline_fault_phases_are_exact() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.q4b_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_nonblocking_calls() -> i32
  fn align_pg_cancel_calls() -> i32
  fn align_pg_control_calls() -> i32
  fn align_pg_protocol_ok() -> i32
  fn align_pg_fail_next_nonblocking_enable()
  fn align_pg_fail_next_nonblocking_restore()
  fn align_pg_delay_next_nonblocking_enable()
}

fn params() -> app.q4b_query.DeadlineParams {
  return app.q4b_query.DeadlineParams { value: 42 }
}

fn consume(
  borrow connection: pkg.db.conn,
  query: pkg.db.query<app.q4b_query.DeadlineParams, app.q4b_query.DeadlineRow>,
  options: slice<pkg.db.ExecuteOption>,
) -> Result<i64, pkg.db.Error> {
  mut stream := pkg.db.rows(pkg.db.exec_conn(connection), query, params(), options)?
  first := pkg.db.next(stream)?
  row := first else {
    return Err(pkg.db.Error.Cardinality(pkg.db.CardinalityError {
      expected_min: 1, expected_max: 1, observed_at_least: 0,
    }))
  }
  exhausted := pkg.db.next(stream)?
  match exhausted {
    Some(_) => {
      return Err(pkg.db.Error.Cardinality(pkg.db.CardinalityError {
        expected_min: 1, expected_max: 1, observed_at_least: 2,
      }))
    }
    None => { return Ok(row.value) }
  }
}

fn consume_tx(
  borrow transaction: pkg.db.tx,
  query: pkg.db.query<app.q4b_query.DeadlineParams, app.q4b_query.DeadlineRow>,
  options: slice<pkg.db.ExecuteOption>,
) -> Result<i64, pkg.db.Error> {
  mut stream := pkg.db.rows(pkg.db.exec_tx(transaction), query, params(), options)?
  first := pkg.db.next(stream)?
  row := first else {
    return Err(pkg.db.Error.Cardinality(pkg.db.CardinalityError {
      expected_min: 1, expected_max: 1, observed_at_least: 0,
    }))
  }
  exhausted := pkg.db.next(stream)?
  match exhausted {
    Some(_) => {
      return Err(pkg.db.Error.Cardinality(pkg.db.CardinalityError {
        expected_min: 1, expected_max: 1, observed_at_least: 2,
      }))
    }
    None => { return Ok(row.value) }
  }
}

fn timeout<T>(result: Result<T, pkg.db.Error>) -> bool = match result {
  Err(error) => match error { Timeout(_) => true, _ => false }
  Ok(_) => false
}

fn connection_error<T>(result: Result<T, pkg.db.Error>) -> bool = match result {
  Err(error) => match error { Connection(_) => true, _ => false }
  Ok(_) => false
}

fn poisoned(borrow connection: pkg.db.conn) -> bool {
  result := consume(connection, app.q4b_query.deadline_success(), [])
  return match result {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "db.connection.state"
        || contract.item == "db.exec.state"
      _ => false
    }
    Ok(_) => false
  }
}

fn enable_failure(connection: pkg.db.conn) -> i32 {
  before := unsafe { align_pg_execute_calls() }
  unsafe { align_pg_fail_next_nonblocking_enable() }
  failed := consume(
    connection, app.q4b_query.deadline_success(),
    [pkg.db.ExecuteOption.TimeoutNs(100000000)],
  )
  if !connection_error(failed) || unsafe { align_pg_execute_calls() } != before { return 1 }
  reused := consume(connection, app.q4b_query.deadline_success(), []) else { return 2 }
  return if reused == 42 { 0 } else { 3 }
}

fn pre_send_expiry(connection: pkg.db.conn) -> i32 {
  before := unsafe { align_pg_execute_calls() }
  cancel_before := unsafe { align_pg_cancel_calls() }
  unsafe { align_pg_delay_next_nonblocking_enable() }
  expired := consume(
    connection, app.q4b_query.deadline_success(),
    [pkg.db.ExecuteOption.TimeoutNs(1000000)],
  )
  if !timeout(expired) || unsafe { align_pg_execute_calls() } != before
    || unsafe { align_pg_cancel_calls() } != cancel_before { return 4 }
  reused := consume(connection, app.q4b_query.deadline_success(), []) else { return 5 }
  return if reused == 42 { 0 } else { 6 }
}

fn pre_send_restore_failure(connection: pkg.db.conn) -> i32 {
  before := unsafe { align_pg_execute_calls() }
  unsafe {
    align_pg_delay_next_nonblocking_enable()
    align_pg_fail_next_nonblocking_restore()
  }
  expired := consume(
    connection, app.q4b_query.deadline_success(),
    [pkg.db.ExecuteOption.TimeoutNs(1000000)],
  )
  if !timeout(expired) || unsafe { align_pg_execute_calls() } != before { return 7 }
  return if poisoned(connection) { 0 } else { 8 }
}

fn send_failure(connection: pkg.db.conn) -> i32 {
  before := unsafe { align_pg_execute_calls() }
  failed := consume(
    connection, app.q4b_query.deadline_send_fail(),
    [pkg.db.ExecuteOption.TimeoutNs(100000000)],
  )
  if !connection_error(failed) || unsafe { align_pg_execute_calls() } != before { return 9 }
  reused := consume(connection, app.q4b_query.deadline_success(), []) else { return 10 }
  return if reused == 42 { 0 } else { 11 }
}

fn flush_failure(connection: pkg.db.conn) -> i32 {
  failed := consume(
    connection, app.q4b_query.deadline_flush_fail(),
    [pkg.db.ExecuteOption.TimeoutNs(100000000)],
  )
  if !connection_error(failed) { return 12 }
  return if poisoned(connection) { 0 } else { 13 }
}

fn cancel_resource_failure(connection: pkg.db.conn) -> i32 {
  cancel_before := unsafe { align_pg_cancel_calls() }
  expired := consume(
    connection, app.q4b_query.deadline_cancel_resource_fail(),
    [pkg.db.ExecuteOption.TimeoutNs(1000000)],
  )
  if !timeout(expired) || unsafe { align_pg_cancel_calls() } != cancel_before { return 14 }
  return if poisoned(connection) { 0 } else { 15 }
}

fn normal_restore_failure(connection: pkg.db.conn) -> i32 {
  unsafe { align_pg_fail_next_nonblocking_restore() }
  failed := consume(
    connection, app.q4b_query.deadline_success(),
    [pkg.db.ExecuteOption.TimeoutNs(100000000)],
  )
  if !connection_error(failed) { return 16 }
  return if poisoned(connection) { 0 } else { 17 }
}

fn transaction_status_failure(connection: pkg.db.conn) -> i32 {
  transaction := pkg.db.begin(connection, []) else { return 18 }
  expired := consume_tx(
    transaction, app.q4b_query.deadline_transaction_unknown(),
    [pkg.db.ExecuteOption.TimeoutNs(1000000)],
  )
  if !timeout(expired) { return 19 }
  after := consume_tx(transaction, app.q4b_query.deadline_success(), [])
  return match after {
    Err(error) => match error {
      Unsupported(_) => 0
      _ => 20
    }
    Ok(_) => 21
  }
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  a := pkg.db.postgres.connect("postgresql://stub/a", []) else { return 22 }
  if enable_failure(a) != 0 { return 23 }
  b := pkg.db.postgres.connect("postgresql://stub/b", []) else { return 24 }
  if pre_send_expiry(b) != 0 { return 25 }
  c := pkg.db.postgres.connect("postgresql://stub/c", []) else { return 26 }
  if pre_send_restore_failure(c) != 0 { return 27 }
  d := pkg.db.postgres.connect("postgresql://stub/d", []) else { return 28 }
  if send_failure(d) != 0 { return 29 }
  e := pkg.db.postgres.connect("postgresql://stub/e", []) else { return 30 }
  if flush_failure(e) != 0 { return 31 }
  f := pkg.db.postgres.connect("postgresql://stub/f", []) else { return 32 }
  if cancel_resource_failure(f) != 0 { return 33 }
  g := pkg.db.postgres.connect("postgresql://stub/g", []) else { return 34 }
  if normal_restore_failure(g) != 0 { return 35 }
  h := pkg.db.postgres.connect("postgresql://stub/h", []) else { return 36 }
  if transaction_status_failure(h) != 0 { return 37 }
  return unsafe {
    if align_pg_protocol_ok() != 1 { return 38 }
    if align_pg_execute_calls() != 7 { return 39 }
    if align_pg_clear_calls() != 8 { return 40 }
    if align_pg_finish_calls() != 8 { return 41 }
    if align_pg_nonblocking_calls() != 15 { return 42 }
    if align_pg_cancel_calls() != 1 { return 43 }
    if align_pg_control_calls() != 1 { return 44 }
    return 45
  }
}
"#;
    let output = build_and_run_multi_with_c(
        "pkg-db-q4b-postgres-deadline-fault-phases",
        &package_files(main),
        "main.align",
        POSTGRES_STUB,
    );
    assert_eq!(
        output.status.code(),
        Some(45),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
