//! pkg.db Q4b/D8+D9 typed streaming, deadline, cancellation, and cleanup owners.

mod common;
use common::*;
mod db_harness;
use db_harness::{counters, *};
use std::sync::LazyLock;

// The pkg.db package sources are read at RUNTIME (common::fixture), not `include_str!`d, so editing
// a `.align` file no longer rebuilds+relinks this test crate. See common/mod.rs for the rationale.
static DB: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db.align"));
static SQLITE: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db/sqlite.align"));
static POSTGRES: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db/postgres.align"));
static INTERNAL: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db/internal.align"));
static RESOURCE: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db/internal/resource.align"));
static DESCRIPTOR: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/internal/descriptor.align"));
static INTERNAL_SQLITE: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/internal/sqlite.align"));
static INTERNAL_POSTGRES: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/internal/postgres.align"));
// C stubs change rarely; keeping them baked costs nothing on `.align` edits.
const POSTGRES_STUB: &str = include_str!("fixtures/pkg_db_q2_postgres_stub.c");

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


// ================================================================================================
// Layer-1 migrated cases.
//
// Each is ONE record. The `#[test]` executes it and `layer1_case_fingerprints_match_the_golden`
// hashes it, both through `Case`, so the golden cannot drift from what actually runs: change the
// runner, the stubs, the gate, or the expected code and the digest moves.
// ================================================================================================
const SQLITE_DIRECT_STREAM_MAIN: &str = r#"module main
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

const SQLITE_COMPLETE_MATRIX_MAIN: &str = r#"module main
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

const POSTGRES_DEADLINE_CANCEL_MAIN: &str = r#"module main
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

const OWNED_PARAMS_MAIN: &str = r#"module main
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

const SQLITE_STREAM_LIFECYCLE_MAIN: &str = r#"module main
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

const POSTGRES_BUFFERED_LIFECYCLE_MAIN: &str = r#"module main
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

const POSTGRES_PREPARED_DEADLINE_MAIN: &str = r#"module main
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

const POSTGRES_COMMAND_DEADLINE_MAIN: &str = r#"module main
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

const POSTGRES_MALFORMED_VIEWS_MAIN: &str = r#"module main
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

const SQLITE_MALFORMED_VIEWS_MAIN: &str = r#"module main
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

const DEADLINE_DISPOSITION_MAIN: &str = r#"module main
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

const POSTGRES_DEADLINE_FAULT_PHASES_MAIN: &str = r#"module main
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

const CASE_SQLITE_DIRECT_STREAM: Case = Case {
    label: "pkg-db-q4b-sqlite-direct-stream",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q4A],
    counters: &[],
    modules: q4b_modules,
    main: SQLITE_DIRECT_STREAM_MAIN,
    expected_exit: 42,
    envs: &[],
    expect_counters: &[],
};

const CASE_SQLITE_COMPLETE_MATRIX: Case = Case {
    label: "pkg-db-q4b-sqlite-complete-matrix",
    runner: RunnerKind::StaticDescriptors,
    needs: Needs::Backend,
    links: &[],
    counters: &[],
    modules: q4b_modules,
    main: SQLITE_COMPLETE_MATRIX_MAIN,
    expected_exit: 42,
    envs: &[],
    expect_counters: &[],
};

const CASE_POSTGRES_DEADLINE_CANCEL: Case = Case {
    label: "pkg-db-q4b-postgres-deadline-cancel",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: q4b_modules,
    main: POSTGRES_DEADLINE_CANCEL_MAIN,
    expected_exit: 42,
    envs: &[],
    expect_counters: &[],
};

const CASE_OWNED_PARAMS: Case = Case {
    label: "pkg-db-q4b-owned-params",
    runner: RunnerKind::StaticDescriptors,
    needs: Needs::Backend,
    links: &[],
    counters: &[],
    modules: q4b_modules,
    main: OWNED_PARAMS_MAIN,
    expected_exit: 42,
    envs: &[],
    expect_counters: &[],
};

const CASE_SQLITE_STREAM_LIFECYCLE: Case = Case {
    label: "pkg-db-q4b-sqlite-stream-lifecycle",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q4A],
    counters: &[],
    modules: q4b_modules,
    main: SQLITE_STREAM_LIFECYCLE_MAIN,
    expected_exit: 42,
    envs: &[],
    expect_counters: &[],
};

const CASE_POSTGRES_BUFFERED_LIFECYCLE: Case = Case {
    label: "pkg-db-q4b-postgres-buffered-lifecycle",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: q4b_modules,
    main: POSTGRES_BUFFERED_LIFECYCLE_MAIN,
    expected_exit: 42,
    envs: &[],
    expect_counters: &[],
};

const CASE_POSTGRES_PREPARED_DEADLINE: Case = Case {
    label: "pkg-db-q4b-postgres-prepared-deadline",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: q4b_modules,
    main: POSTGRES_PREPARED_DEADLINE_MAIN,
    expected_exit: 42,
    envs: &[],
    expect_counters: &[],
};

const CASE_POSTGRES_COMMAND_DEADLINE: Case = Case {
    label: "pkg-db-q4b-postgres-command-deadline",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: q4b_modules,
    main: POSTGRES_COMMAND_DEADLINE_MAIN,
    expected_exit: 42,
    envs: &[],
    expect_counters: &[],
};

const CASE_POSTGRES_MALFORMED_VIEWS: Case = Case {
    label: "pkg-db-q4b-postgres-malformed-views",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: q4b_modules,
    main: POSTGRES_MALFORMED_VIEWS_MAIN,
    expected_exit: 42,
    envs: &[],
    expect_counters: &[],
};

const CASE_SQLITE_MALFORMED_VIEWS: Case = Case {
    label: "pkg-db-q4b-sqlite-malformed-views",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q4A],
    counters: &[],
    modules: q4b_modules,
    main: SQLITE_MALFORMED_VIEWS_MAIN,
    expected_exit: 42,
    envs: &[],
    expect_counters: &[],
};

const CASE_DEADLINE_DISPOSITION: Case = Case {
    label: "pkg-db-q4b-deadline-disposition",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q4A],
    counters: &[],
    modules: q4b_modules,
    main: DEADLINE_DISPOSITION_MAIN,
    expected_exit: 42,
    envs: &[],
    expect_counters: &[],
};

const CASE_POSTGRES_DEADLINE_FAULT_PHASES: Case = Case {
    label: "pkg-db-q4b-postgres-deadline-fault-phases",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: q4b_modules,
    main: POSTGRES_DEADLINE_FAULT_PHASES_MAIN,
    expected_exit: 45,
    envs: &[],
    expect_counters: &[],
};

/// Every Layer-1 case, for the fingerprint golden.
const LAYER1_CASES: &[&Case] = &[
    &CASE_SQLITE_DIRECT_STREAM,
    &CASE_SQLITE_COMPLETE_MATRIX,
    &CASE_POSTGRES_DEADLINE_CANCEL,
    &CASE_OWNED_PARAMS,
    &CASE_SQLITE_STREAM_LIFECYCLE,
    &CASE_POSTGRES_BUFFERED_LIFECYCLE,
    &CASE_POSTGRES_PREPARED_DEADLINE,
    &CASE_POSTGRES_COMMAND_DEADLINE,
    &CASE_POSTGRES_MALFORMED_VIEWS,
    &CASE_SQLITE_MALFORMED_VIEWS,
    &CASE_DEADLINE_DISPOSITION,
    &CASE_POSTGRES_DEADLINE_FAULT_PHASES,
];

/// The `pkg.db` package plus this suite's query module and one `main`.
fn q4b(main: &str) -> Layout {
    Layout::new()
        .module("app/q4b_query.align", QUERY)
        .main(main)
}

/// The suite modules every q4b case adds on top of the `pkg.db` package.
fn q4b_modules() -> Vec<(&'static str, &'static str)> {
    vec![("app/q4b_query.align", QUERY)]
}

/// The pre-harness layout builder, kept ONLY as the oracle for
/// `layout_reproduces_the_pre_harness_package_files_exactly`. It is the backward-looking half of
/// the migration proof: the forward-looking half is the case-fingerprint golden. Do not call it
/// from a test.
fn legacy_package_files(main: &str) -> Vec<(&'static str, &str)> {
    vec![
        ("pkg/db.align", *DB),
        ("pkg/db/sqlite.align", *SQLITE),
        ("pkg/db/postgres.align", *POSTGRES),
        ("pkg/db/internal.align", *INTERNAL),
        ("pkg/db/internal/resource.align", *RESOURCE),
        ("pkg/db/internal/descriptor.align", *DESCRIPTOR),
        ("pkg/db/internal/sqlite.align", *INTERNAL_SQLITE),
        ("pkg/db/internal/postgres.align", *INTERNAL_POSTGRES),
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
    expect_checks_clean("pkg-db-q4b-direct-rows-surface", &q4b(main));
    let built = build_per_unit_multi(
        "pkg-db-q4b-direct-rows-mir",
        &q4b(main).files(),
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
fn postgres_parameter_type_must_match_the_params_field_shape() {
    let mismatched_query = QUERY.replacen(
        "pkg.db.postgres.QueryOption.ParameterType(\"id\", \"int8\")",
        "pkg.db.postgres.QueryOption.ParameterType(\"id\", \"text\")",
        1,
    );
    let main = "module main\nimport app.q4b_query\nfn main() -> i32 = 0\n";
    let files = [
        ("pkg/db.align", *DB),
        ("pkg/db/sqlite.align", *SQLITE),
        ("pkg/db/postgres.align", *POSTGRES),
        ("pkg/db/internal.align", *INTERNAL),
        ("pkg/db/internal/resource.align", *RESOURCE),
        ("pkg/db/internal/descriptor.align", *DESCRIPTOR),
        ("pkg/db/internal/sqlite.align", *INTERNAL_SQLITE),
        ("pkg/db/internal/postgres.align", *INTERNAL_POSTGRES),
        ("app/q4b_query.align", mismatched_query.as_str()),
        ("main.align", main),
    ];
    let diagnostics = build_per_unit_multi_diagnostics(
        "pkg-db-q4b-postgres-parameter-type-mismatch",
        &files,
        "main.align",
    );
    assert!(
        diagnostics
            .contains("maps PostgreSQL parameter `id` to incompatible native type `text`"),
        "the generated runtime boundary must reject an incompatible ParameterType:\n{diagnostics}",
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
        expect_checks_rejected(&format!("pkg-db-q4b-stream-view-{name}"), &q4b(&main));
    }
}

#[test]
fn sqlite_direct_stream_retains_binds_and_releases_each_native_phase_once() {
    CASE_SQLITE_DIRECT_STREAM.run();
}

/// The 16-field parameter/row round trip on the WHOLE-PROGRAM static-descriptor pipeline.
///
/// `full_matrix_parity_is_exact_on_both_drivers` now covers the same round trip on both drivers,
/// but it must use the per-unit + C-fixture pipeline because that is the only one that can link the
/// libpq stub. That leaves this pipeline — whole-program `check` plus
/// `lower_to_mir_with_static_descriptors` — uncovered for this matrix unless it is retained here.
/// `FINDINGS.md` records `owner-test-topology` ("Retain the whole-program execution owner") for
/// exactly this mistake, so this owner is deliberately kept rather than folded into the table.
#[test]
fn sqlite_full_matrix_retains_the_whole_program_static_descriptor_path() {
    CASE_SQLITE_COMPLETE_MATRIX.run();
}


// ================================================================================================
// The dual-driver parity owner for the 16-field parameter/row matrix.
//
// This replaces `complete_postgres_parameter_and_row_matrix_is_exact`, which was a hand-maintained
// copy of the SQLite member: 134 and 136 lines differing in 52, part of that difference being pure
// formatting drift of the same struct literal. One program now carries the body and selects its
// driver at run time, so the two members cannot drift apart; the table declares, per case, whether
// the drivers must agree.
// ================================================================================================

/// One program, both drivers. `statement` and `open` are the ONLY driver-dependent points; the body
/// under test below them is shared by construction.
const PARITY_MAIN: &str = r#"module main
import std.env
import pkg.db
import pkg.db.postgres
import pkg.db.sqlite
import pkg.db.testkit.pg
import app.q4b_query

fn statement(driver: str) -> pkg.db.query<app.q4b_query.FullParams, app.q4b_query.FullRow> =
  if driver == "postgres" { app.q4b_query.full_postgres() } else { app.q4b_query.full() }

fn open(driver: str) -> Result<pkg.db.conn, pkg.db.Error> =
  if driver == "postgres" {
    pkg.db.postgres.connect("postgresql://stub/db", [])
  } else {
    pkg.db.sqlite.connect(":memory:", [])
  }

fn execute_once(borrow connection: pkg.db.conn, driver: str, present: bool) -> i32 {
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
    statement(driver),
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

fn retained(borrow connection: pkg.db.conn, driver: str) -> i32 {
  return arena out {
    bytes := [4 as u8, 0 as u8, 255 as u8]
    selected := pkg.db.one(
      pkg.db.exec_conn(connection),
      statement(driver),
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
    0
  }
}

fn main() -> i32 {
  driver := env.get("ALIGN_DB_DRIVER") else { return 90 }
  case := env.get("ALIGN_DB_CASE") else { return 91 }
  if case == "__list__" {
    print("present_values")
    print("absent_values")
    print("one_retained_bytes")
    print("counters")
    return 42
  }
  pkg.db.testkit.pg.reset()
  opened := open(driver)
  connection := opened else { return 30 }
  if case == "present_values" {
    return execute_once(connection, driver, true)
  } else {
    if case == "absent_values" {
      return execute_once(connection, driver, false)
    } else {
      if case == "one_retained_bytes" {
        return retained(connection, driver)
      } else {
        if case == "counters" {
          present := execute_once(connection, driver, true)
          if present != 0 { return present }
          absent := execute_once(connection, driver, false)
          if absent != 0 { return absent }
          one := retained(connection, driver)
          if one != 0 { return one }
          pkg.db.testkit.pg.dump()
          return 42
        } else {
          return 99
        }
      }
    }
  }
}
"#;

/// How many rows of [`PARITY_CASES`] may declare a cross-driver divergence.
///
/// A ratchet: raising it must happen in the same diff that adds the divergence, so the increase is
/// visible in review. Lowering it is always allowed. The 16-field matrix is fully portable today,
/// so the budget is zero.
const PARITY_DIFFERS_BUDGET: usize = 0;

const PARITY_CASES: &[ParityCase] = &[
    // 0 is `execute_once`/`retained` reporting success; any other value is one of their numbered
    // failure points, and the table requires BOTH drivers to reach the same one.
    ParityCase::same("present_values", 0),
    ParityCase::same("absent_values", 0),
    ParityCase::same("one_retained_bytes", 0),
    // Native call counts are a libpq-stub property; SQLite has no counterpart to compare against,
    // so this row runs on PostgreSQL only and its counter table is asserted below.
    ParityCase::driver_only(
        "counters",
        Driver::Postgres,
        42,
        "libpq call counts have no SQLite counterpart to compare against",
    ),
];

/// The parity owner's case-fingerprint golden.
///
/// Each row is observed from the built program (`ParityProgram::fingerprint`): the pipeline from
/// the engine, the child environment from the engine, the modules from the ones actually compiled.
/// Only the expected class comes from the table above, which owns it. Switching the runner,
/// changing the compile profile, or adding a variable to a child run therefore shows up here as a
/// changed digest rather than passing quietly.
///
/// The eight `pkg.db` package sources are deliberately NOT part of a digest: they are product code
/// with their own owners, and folding them in would break this golden on every `apps/db` edit
/// without saying anything about the parity owner.
///
/// Regenerate ONLY with a reviewed reason, from the panic message this emits.
const PARITY_FINGERPRINT_GOLDEN: &str = "\
pkg-db-q4b-full-matrix-parity/absent_values/postgres ec61f97163fb426e
pkg-db-q4b-full-matrix-parity/absent_values/sqlite a0c3f1256f797be4
pkg-db-q4b-full-matrix-parity/counters/postgres 17833e8112c666ee
pkg-db-q4b-full-matrix-parity/one_retained_bytes/postgres 00029e1d32fbb498
pkg-db-q4b-full-matrix-parity/one_retained_bytes/sqlite ebd3c5440d20030c
pkg-db-q4b-full-matrix-parity/present_values/postgres fafe721a49cf9464
pkg-db-q4b-full-matrix-parity/present_values/sqlite b3cb905db2f0ef98
";

#[test]
fn full_matrix_parity_is_exact_on_both_drivers() {
    let Some(_gate) = gate(Needs::BackendAndCc) else { return };
    let program = ParityProgram::build(
        "pkg-db-q4b-full-matrix-parity",
        &q4b(PARITY_MAIN).with_counters(&PG),
    );
    let runs = run_parity(
        &program,
        PARITY_CASES,
        &[Driver::Sqlite, Driver::Postgres],
        PARITY_DIFFERS_BUDGET,
    );
    // The same three operations the parity rows ran, in one process: two `rows` executions and one
    // `one`. This is what the retired PostgreSQL twin asserted as `execute_calls != 3`.
    counters::pg()
        .pg_execute(3)
        .assert(run_of(&runs, "counters", Driver::Postgres));

    // H6: one process per case, so stub state cannot accumulate across cases. `counters` runs after
    // three other cases have each already executed statements against the same program; if state
    // leaked between cases its counts would exceed its own three operations. Running it a second
    // time must produce byte-identical counters for the same reason.
    let second = program.run_case(Driver::Postgres, "counters");
    second.expect_exit(42);
    counters::pg().pg_execute(3).assert(&second);

    // The golden is built from the ENGINE's own view of each run, not from inputs rebuilt here.
    let mut log = FingerprintLog::new();
    for case in PARITY_CASES {
        for driver in [Driver::Sqlite, Driver::Postgres] {
            let (expected, applicable) = match case.expect {
                Expect::Same(code) => (code, true),
                Expect::Differs {
                    sqlite, postgres, ..
                } => (
                    if driver == Driver::Sqlite {
                        sqlite
                    } else {
                        postgres
                    },
                    true,
                ),
                Expect::DriverOnly {
                    driver: only, code, ..
                } => (code, only == driver),
            };
            if applicable {
                log.record(&program.fingerprint(case.name, driver, expected));
            }
        }
    }
    log.assert_matches(PARITY_FINGERPRINT_GOLDEN);
}

#[test]
fn postgres_deadline_cancel_drain_and_poisoning_are_exact() {
    CASE_POSTGRES_DEADLINE_CANCEL.run();
}

#[test]
fn owned_text_and_bytes_params_bind_before_their_sources_drop() {
    CASE_OWNED_PARAMS.run();
}

#[test]
fn sqlite_stream_lifecycle_and_overlap_are_exact() {
    CASE_SQLITE_STREAM_LIFECYCLE.run();
}

#[test]
fn postgres_buffered_stream_lifecycle_is_exact() {
    CASE_POSTGRES_BUFFERED_LIFECYCLE.run();
}

#[test]
fn postgres_prepared_deadline_recovers_for_reuse() {
    CASE_POSTGRES_PREPARED_DEADLINE.run();
}

#[test]
fn postgres_command_deadline_is_enforced_and_recovers_for_reuse() {
    CASE_POSTGRES_COMMAND_DEADLINE.run();
}

#[test]
fn malformed_native_view_values_fail_before_safe_view_formation() {
    CASE_SQLITE_MALFORMED_VIEWS.run();

    CASE_POSTGRES_MALFORMED_VIEWS.run();
}

#[test]
#[ignore = "local D8 scalar/text streaming measurement; not a correctness or CI gate"]
fn million_row_streaming_measurement_reports_delivery_counts() {
    let Some(_gate) = gate(Needs::Backend) else { return };
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
    let layout = q4b(main).module("app/q4b_bench.align", queries);
    let output = build_and_run_multi_with_static_descriptors(
        "pkg-db-q4b-million-row-measurement",
        &layout.files(),
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
    let Some(_gate) = gate(Needs::BackendAndCc) else { return };
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
        &q4b(main).files(),
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
    CASE_DEADLINE_DISPOSITION.run();
}

#[test]
fn postgres_deadline_fault_phases_are_exact() {
    CASE_POSTGRES_DEADLINE_FAULT_PHASES.run();
}

// ================================================================================================
// Harness self-tests and fail-open probes.
//
// A migration is only as trustworthy as the machinery it migrates onto, so every harness invariant
// has an owner here, and every converted assertion class has a probe that deliberately breaks it
// and requires the harness to report the break precisely. These live in `pkg_db_q4b.rs` rather than
// a new test binary because q4b is the harness's first consumer and a new binary would cost a
// ~25 MB link and ~25 s of startup for no additional coverage.
// ================================================================================================
mod harness_selftest {
    use super::*;
    use db_harness::counters::Counters;
    use db_harness::run::{LiveDecision, live_postgres_decision, should_clear_env};
    use db_harness::{CaseFingerprint, FingerprintLog, Mismatch, assert_no_mismatches};

    /// Synthesising an `ExitStatus` needs `ExitStatusExt`, which is unix-only; these helpers and
    /// every owner built on them are gated as one group so the module still compiles elsewhere.
    #[cfg(unix)]
    fn canned(label: &str, stdout: &str, code: i32) -> Run {
        Run::new(
            label,
            std::process::Output {
                status: exit_status(code),
                stdout: stdout.as_bytes().to_vec(),
                stderr: Vec::new(),
            },
        )
    }

    #[cfg(unix)]
    fn exit_status(code: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code << 8)
    }

    #[cfg(unix)]
    fn signalled_status(signal: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(signal)
    }

    const DUMP: &str = "\
#db-counters-begin
pg.execute_calls
3
pg.clear_calls
3
pg.protocol_ok
1
pg.protocol_error
0
#db-counters-end
";

    // ---- H1: layout composition ---------------------------------------------------------------

    /// The backward-looking half of the migration proof: the new builder reproduces the retired
    /// `package_files` list EXACTLY — same paths, same sources, same order.
    #[test]
    fn layout_reproduces_the_pre_harness_package_files_exactly() {
        let main = "module main\nfn main() -> i32 = 0\n";
        let legacy = legacy_package_files(main);
        let built = q4b(main);
        let built = built.files();
        assert_eq!(
            legacy.len(),
            built.len(),
            "module count changed: {:?} vs {:?}",
            legacy.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
            built.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        );
        for (index, ((lp, ls), (bp, bs))) in legacy.iter().zip(built.iter()).enumerate() {
            assert_eq!(lp, bp, "module {index} path");
            assert_eq!(ls, bs, "module {index} source ({lp})");
        }
    }

    #[test]
    fn layout_module_replaces_rather_than_appends() {
        let layout = Layout::new().module("app/x.align", "one").module("app/x.align", "two");
        let files = layout.files();
        assert_eq!(
            files.iter().filter(|(p, _)| *p == "app/x.align").count(),
            1,
            "replacing a path must not leave a duplicate entry"
        );
        assert_eq!(
            files.iter().find(|(p, _)| *p == "app/x.align").unwrap().1,
            "two"
        );
    }

    /// `common::Proj::write` does not create parent directories; `Layout::materialize` must.
    #[test]
    fn layout_materializes_nested_module_paths() {
        let proj = q4b("module main\nfn main() -> i32 = 0\n")
            .materialize("pkg-db-q4b-selftest-materialize");
        for relative in [
            "pkg/db.align",
            "pkg/db/internal/resource.align",
            "app/q4b_query.align",
            "main.align",
        ] {
            assert!(
                proj.dir.join(relative).is_file(),
                "{relative} was not written"
            );
        }
    }

    /// H9: the counters module can only arrive together with the C source that defines its symbols.
    #[test]
    fn pg_counters_always_arrive_with_their_stub() {
        let layout = Layout::new().with_counters(&PG);
        assert!(
            layout.has_c_fixture(),
            "with_counters must also link the stub that defines the counters"
        );
        assert!(
            layout.paths().contains(&PG.counters_path),
            "with_counters must add the Align counters module"
        );
        assert!(
            !Layout::new().linking(&PG).paths().contains(&PG.counters_path),
            "linking the stub alone must NOT inject the counters module: a pure-refactor migration \
             depends on the compiled module set being unchanged"
        );
    }

    // ---- H2 / P5: exit reporting ---------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn exit_check_accepts_the_expected_code() {
        assert!(canned("ok", "", 42).check_exit(42).is_ok());
    }

    /// P5: a signal-killed child has `code() == None`; the report must still be legible.
    #[cfg(unix)]
    #[test]
    fn exit_report_survives_signal_termination() {
        let run = Run::new(
            "killed",
            std::process::Output {
                status: signalled_status(9),
                stdout: b"partial".to_vec(),
                stderr: Vec::new(),
            },
        );
        let mismatch = run.check_exit(42).expect_err("a killed child cannot be exit 42");
        assert!(mismatch.actual.contains("None"), "{mismatch}");
        let described = run.describe();
        assert!(described.contains("status:"), "{described}");
        assert!(described.contains("partial"), "{described}");
    }

    // ---- H3 / P1 / P2: counter table ------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn counter_table_accepts_matching_counters() {
        let run = canned("pg", DUMP, 42);
        assert!(counters::pg().pg_execute(3).pg_clear(3).check(&run).is_empty());
    }

    /// P1: one counter off by one is reported BY NAME, with expected and actual.
    #[cfg(unix)]
    #[test]
    fn counter_table_names_an_off_by_one_counter() {
        let run = canned("pg", DUMP, 42);
        let found = counters::pg().pg_execute(4).check(&run);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].what.contains("pg.execute_calls"), "{:?}", found[0]);
        assert_eq!(found[0].expected, "4");
        assert_eq!(found[0].actual, "3");
    }

    /// P2: a counter the stub never reported is "absent", not silently zero.
    #[cfg(unix)]
    #[test]
    fn counter_table_reports_an_absent_counter_as_absent() {
        let run = canned("pg", DUMP, 42);
        let found = counters::pg().pg_finish(1).check(&run);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].actual.starts_with("absent"), "{:?}", found[0]);
    }

    /// H3: EVERY mismatch is reported, not just the first.
    #[cfg(unix)]
    #[test]
    fn counter_table_reports_every_mismatch() {
        let run = canned("pg", DUMP, 42);
        let found = counters::pg().pg_execute(9).pg_clear(9).check(&run);
        assert_eq!(found.len(), 2, "both wrong counters must be reported: {found:?}");
    }

    // ---- H5: dump format ------------------------------------------------------------------------

    #[test]
    fn counter_dump_parses_name_value_pairs() {
        let parsed = Counters::parse(DUMP).expect("well-formed dump");
        assert_eq!(parsed.get("pg.execute_calls"), Some(3));
        assert_eq!(parsed.get("pg.protocol_error"), Some(0));
    }

    #[test]
    fn counter_dump_rejects_malformed_input() {
        // No dump at all: a program that forgot to call dump() must not read as "zero counters".
        assert!(Counters::parse("42\n").is_err());
        // Truncated: the end sentinel makes this detectable.
        assert!(Counters::parse("#db-counters-begin\npg.execute_calls\n3\n").is_err());
        // A name with no value.
        assert!(Counters::parse("#db-counters-begin\npg.execute_calls\n#db-counters-end\n").is_err());
        // Non-numeric value.
        assert!(Counters::parse("#db-counters-begin\npg.x\nnope\n#db-counters-end\n").is_err());
        // Same name twice with different values.
        assert!(
            Counters::parse("#db-counters-begin\npg.x\n1\npg.x\n2\n#db-counters-end\n").is_err()
        );
    }

    /// A program's own `print` output must survive alongside a counter dump.
    #[cfg(unix)]
    #[test]
    fn payload_lines_exclude_the_counter_dump() {
        let run = canned("pg", &format!("hello\n{DUMP}world\n"), 42);
        assert_eq!(run.payload_lines(), vec!["hello", "world"]);
    }

    // ---- H10 / P6: accumulation -----------------------------------------------------------------

    /// P6: one broken row does not hide the others.
    #[test]
    fn accumulated_failures_all_appear_in_the_message() {
        let mismatches: Vec<Mismatch> = (0..3)
            .map(|i| Mismatch {
                what: format!("row{i}"),
                expected: "x".into(),
                actual: "y".into(),
            })
            .collect();
        let panicked = std::panic::catch_unwind(|| assert_no_mismatches("table", &mismatches))
            .expect_err("must panic");
        let message = panic_message(&panicked);
        for i in 0..3 {
            assert!(message.contains(&format!("row{i}")), "{message}");
        }
        assert!(message.contains("3 expectation(s) failed"), "{message}");
    }

    fn panic_message(panicked: &Box<dyn std::any::Any + Send>) -> String {
        panicked
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panicked.downcast_ref::<&str>().map(|s| (*s).to_string()))
            .unwrap_or_else(|| "<non-string panic>".to_string())
    }

    // ---- H7 / P8: required-mode gating ----------------------------------------------------------

    /// P8: required mode with no URL is a FAILURE, never a skip. Tested through the pure decision
    /// so no test mutates process-global environment.
    #[test]
    fn required_live_postgres_without_a_url_fails_rather_than_skipping() {
        assert_eq!(live_postgres_decision(true, None), LiveDecision::Fail);
        assert_eq!(live_postgres_decision(true, Some("")), LiveDecision::Fail);
        assert_eq!(live_postgres_decision(false, None), LiveDecision::Skip);
        assert_eq!(
            live_postgres_decision(true, Some("postgresql://x")),
            LiveDecision::Run("postgresql://x".to_string())
        );
    }

    // ---- H12: selector hygiene -------------------------------------------------------------------

    /// H12: the engine clears foreign `ALIGN_DB_*` variables, and derives the exception list from
    /// the keys it actually set rather than from a hardcoded copy of them.
    #[test]
    fn engine_clears_foreign_align_db_variables() {
        let set = ["ALIGN_DB_DRIVER", "ALIGN_DB_CASE"];
        assert!(should_clear_env("ALIGN_DB_STRAY", &set));
        assert!(should_clear_env("ALIGN_DB_POSTGRES_URL", &set));
        assert!(!should_clear_env("ALIGN_DB_DRIVER", &set));
        assert!(!should_clear_env("ALIGN_DB_CASE", &set));
        assert!(!should_clear_env("PATH", &set));
        // Adding a key to the child automatically stops it being cleared: no second list to update.
        let with_url = ["ALIGN_DB_DRIVER", "ALIGN_DB_CASE", "ALIGN_DB_POSTGRES_URL"];
        assert!(!should_clear_env("ALIGN_DB_POSTGRES_URL", &with_url));
    }

    /// P3-5/P3-6, now parameterized over every stub: each registry and the Align module that
    /// prints its counters must agree, in the same order. Pure string comparison — no C compiler,
    /// no LLVM, milliseconds — so a counter added on one side and forgotten on the other fails
    /// immediately rather than surviving until someone happens to assert on it.
    #[test]
    fn counter_registries_match_their_align_modules() {
        let mut mismatches = Vec::new();
        for stub in db_harness::stubs::ALL {
            let printed = stub.names_in_module();
            let registry: Vec<String> = stub.names.iter().map(|n| (*n).to_string()).collect();
            if printed != registry {
                mismatches.push(Mismatch {
                    what: format!("counter registry for stub `{}`", stub.id),
                    expected: format!("{registry:?}"),
                    actual: format!("{printed:?}"),
                });
            }
        }
        assert_no_mismatches("counter registries", &mismatches);
    }

    /// An expectation naming a counter outside the registry is a test bug, and must be reported as
    /// one rather than as an absent counter.
    #[test]
    fn counter_expectation_rejects_an_unknown_name() {
        let panicked = std::panic::catch_unwind(|| counters::CounterExpect::new().eq("pg.nope", 1))
            .expect_err("an unknown counter name must be rejected");
        assert!(panic_message(&panicked).contains("not a known counter"));
    }

    /// P3-6: a dump carrying a name outside the registry is a schema break, not an absent counter.
    #[test]
    fn counter_dump_rejects_an_unknown_name() {
        let error = Counters::parse("#db-counters-begin\npg.mystery\n1\n#db-counters-end\n")
            .expect_err("unknown counter names must be rejected");
        assert!(error.contains("not in the known registry"), "{error}");
    }

    // ---- fingerprint ------------------------------------------------------------------------------

    /// Every axis of the fingerprint must change the digest. A file-only hash would miss the last
    /// four, and each of them changes what the case proves.
    #[test]
    fn fingerprint_covers_every_axis_of_a_case() {
        let base = || {
            CaseFingerprint::new("case", RUNNER_PER_UNIT_C)
                .files(&[("main.align", "a")])
                .env(&[("K", "V")])
                .argv(&["one"])
                .expected_exit(42)
        };
        let reference = base().digest();
        assert_eq!(base().digest(), reference, "the digest must be stable");
        assert_ne!(
            CaseFingerprint::new("case", RUNNER_STATIC_DESCRIPTORS)
                .files(&[("main.align", "a")])
                .env(&[("K", "V")])
                .argv(&["one"])
                .expected_exit(42)
                .digest(),
            reference,
            "a changed RUNNER must change the digest"
        );
        assert_ne!(
            base().files(&[("main.align", "b")]).digest(),
            reference,
            "changed sources must change the digest"
        );
        assert_ne!(
            base().env(&[("K", "W")]).digest(),
            reference,
            "a changed ENVIRONMENT must change the digest"
        );
        assert_ne!(
            base().argv(&["two"]).digest(),
            reference,
            "changed ARGV must change the digest"
        );
        assert_ne!(
            base().expected_exit(7).digest(),
            reference,
            "a changed EXPECTED EXIT must change the digest"
        );
    }

    /// Environment order must not perturb the digest; file order must.
    #[test]
    fn fingerprint_is_stable_under_environment_order() {
        let a = CaseFingerprint::new("c", RUNNER_PER_UNIT_C).env(&[("A", "1"), ("B", "2")]);
        let b = CaseFingerprint::new("c", RUNNER_PER_UNIT_C).env(&[("B", "2"), ("A", "1")]);
        assert_eq!(a.digest(), b.digest());
        let f1 = CaseFingerprint::new("c", RUNNER_PER_UNIT_C)
            .files(&[("a", "1"), ("b", "2")])
            .digest();
        let f2 = CaseFingerprint::new("c", RUNNER_PER_UNIT_C)
            .files(&[("b", "2"), ("a", "1")])
            .digest();
        assert_ne!(f1, f2, "module ORDER is part of what the driver sees");
    }

    #[test]
    fn fingerprint_log_reports_added_removed_and_changed_rows() {
        let mut log = FingerprintLog::new();
        log.record(&CaseFingerprint::new("kept", RUNNER_PER_UNIT_C));
        log.record(&CaseFingerprint::new("added", RUNNER_PER_UNIT_C));
        let golden = {
            let mut other = FingerprintLog::new();
            other.record(&CaseFingerprint::new("kept", RUNNER_PER_UNIT_C));
            other.record(&CaseFingerprint::new("removed", RUNNER_PER_UNIT_C));
            other.render()
        };
        let panicked =
            std::panic::catch_unwind(move || log.assert_matches(&golden)).expect_err("must panic");
        let message = panic_message(&panicked);
        assert!(message.contains("added:   added"), "{message}");
        assert!(message.contains("removed: removed"), "{message}");
    }
}

// ================================================================================================
// Parity-engine fail-open probes (P3, P4, P9).
//
// These need a real compiled program, so they share ONE build and deliberately feed it wrong
// tables. Each probe proves the engine reports the specific break rather than passing quietly —
// which is the whole reason the parity table replaces two hand-maintained twins.
// ================================================================================================
#[test]
fn parity_engine_detects_table_and_program_disagreement() {
    let Some(_gate) = gate(Needs::BackendAndCc) else { return };
    let program = ParityProgram::build(
        "pkg-db-q4b-parity-probes",
        &q4b(PARITY_MAIN).with_counters(&PG),
    );
    let drivers = [Driver::Sqlite, Driver::Postgres];

    let message = |cases: &'static [ParityCase], budget: usize| -> String {
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_parity(&program, cases, &drivers, budget);
        }))
        .expect_err("the probe table must be rejected");
        panicked
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panicked.downcast_ref::<&str>().map(|s| (*s).to_string()))
            .unwrap_or_else(|| "<non-string panic>".to_string())
    };

    // P3: a case whose declared class is wrong must be reported on BOTH drivers, by name.
    const WRONG_CLASS: &[ParityCase] = &[
        ParityCase::same("present_values", 7),
        ParityCase::same("absent_values", 0),
        ParityCase::same("one_retained_bytes", 0),
        ParityCase::driver_only("counters", Driver::Postgres, 42, "libpq only"),
    ];
    let report = message(WRONG_CLASS, 0);
    assert!(report.contains("present_values"), "{report}");
    assert!(report.contains("sqlite"), "{report}");
    assert!(report.contains("postgres"), "{report}");
    assert!(report.contains("Some(7)"), "{report}");

    // P4a: a divergence declared where the drivers actually agree is still checked per driver, so
    // the wrong half is reported with its justification attached.
    const FALSE_DIVERGENCE: &[ParityCase] = &[
        ParityCase::differs("present_values", 0, 7, "probe: not a real divergence"),
        ParityCase::same("absent_values", 0),
        ParityCase::same("one_retained_bytes", 0),
        ParityCase::driver_only("counters", Driver::Postgres, 42, "libpq only"),
    ];
    let report = message(FALSE_DIVERGENCE, 1);
    assert!(report.contains("declared divergence"), "{report}");
    assert!(report.contains("probe: not a real divergence"), "{report}");

    // P4b: the divergence budget is a ratchet — the same table over budget fails before it runs.
    let report = message(FALSE_DIVERGENCE, 0);
    assert!(report.contains("budget"), "{report}");

    // The resource ceiling refuses an oversized table BEFORE spawning anything.
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_parity_with_limits(
            &program,
            PARITY_CASES,
            &drivers,
            PARITY_DIFFERS_BUDGET,
            Limits {
                max_cases: 1,
                max_child_runs: 1024,
            },
        );
    }))
    .expect_err("a table over its case ceiling must be refused");
    let report = panicked
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(report.contains("over the declared ceiling"), "{report}");

    // P9: a case name changed by ONE character is caught twice over — the bidirectional `__list__`
    // diff sees a table row the program does not have AND a program case the table does not have,
    // and the run itself hits the unknown-case sentinel.
    const TYPO: &[ParityCase] = &[
        ParityCase::same("present_value", 0),
        ParityCase::same("absent_values", 0),
        ParityCase::same("one_retained_bytes", 0),
        ParityCase::driver_only("counters", Driver::Postgres, 42, "libpq only"),
    ];
    let report = message(TYPO, 0);
    assert!(report.contains("present_value`"), "{report}");
    assert!(
        report.contains("present_values`"),
        "the program's own case must be reported as absent from the table: {report}"
    );
    assert!(
        report.contains(&format!("sentinel {}", db_harness::parity::UNKNOWN_CASE)),
        "the typo must also hit the unknown-case sentinel: {report}"
    );
}

/// P7: a layout missing a package module must produce a compile diagnostic, never a silent pass.
#[test]
fn a_layout_missing_a_package_module_is_rejected() {
    let main = "module main\nimport pkg.db\nfn main() -> i32 = 0\n";
    let complete = diff_check_multi(
        "pkg-db-q4b-probe-layout-complete",
        &q4b(main).files(),
        "main.align",
    );
    assert!(
        !complete.whole_errors && !complete.per_unit_errors,
        "the control layout must check cleanly:\n{}",
        complete.whole_diags
    );
    let missing = diff_check_multi(
        "pkg-db-q4b-probe-layout-missing",
        &q4b(main).without("pkg/db/internal.align").files(),
        "main.align",
    );
    assert!(
        missing.whole_errors && missing.per_unit_errors,
        "a layout missing pkg/db/internal.align must be REJECTED, not silently checked as a \
         smaller program (whole={}, per_unit={})",
        missing.whole_errors,
        missing.per_unit_errors,
    );
}


/// P2-3: a table row that never executes must FAIL, not pass quietly.
///
/// A `DriverOnly` row whose driver is absent from the run set matches nothing in the execution
/// loop. Reporting it as green would mean the parity table's own coverage can silently shrink to
/// nothing — the exact failure mode it exists to prevent.
#[test]
fn parity_engine_fails_a_row_that_never_runs() {
    let Some(_gate) = gate(Needs::BackendAndCc) else { return };
    let program = ParityProgram::build(
        "pkg-db-q4b-parity-unexecuted",
        &q4b(PARITY_MAIN).with_counters(&PG),
    );
    // Only SQLite runs, so the PostgreSQL-only `counters` row cannot execute.
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_parity(&program, PARITY_CASES, &[Driver::Sqlite], PARITY_DIFFERS_BUDGET);
    }))
    .expect_err("a row that never runs must be reported");
    let report = panicked
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panicked.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_default();
    assert!(report.contains("counters"), "{report}");
    assert!(report.contains("never ran"), "{report}");
    assert!(report.contains("at least one execution"), "{report}");
}

/// H8: the temp project — sources, objects, executables — is removed when the layout's `Proj` drops.
#[test]
fn materialized_projects_are_removed_on_drop() {
    let path = {
        let proj = q4b("module main\nfn main() -> i32 = 0\n")
            .materialize("pkg-db-q4b-selftest-cleanup");
        let path = proj.dir.clone();
        assert!(path.join("pkg/db.align").is_file());
        path
    };
    assert!(
        !path.exists(),
        "the temp project must be removed on drop, even after a panicking assertion: {}",
        path.display()
    );
}

/// The Layer-1 migration's forward guard.
///
/// `Case::run` and `Case::fingerprint` read the same record, so this golden moves whenever a case's
/// pipeline, stubs, host requirement, module set, program text, or expected exit code changes. That
/// is the axis set a source-only hash misses: the file list can be identical while the runner or
/// the expected code has silently changed, and each of those changes what the case proves.
///
/// Regenerate ONLY with a reviewed reason, from the panic message this emits.
const LAYER1_FINGERPRINT_GOLDEN: &str = "\
pkg-db-q4b-deadline-disposition 6f38d49e84b8bcb6
pkg-db-q4b-owned-params fb2498e6f5ad6909
pkg-db-q4b-postgres-buffered-lifecycle a3f0fc348ca1e0d1
pkg-db-q4b-postgres-command-deadline 7fd12c3f5881702a
pkg-db-q4b-postgres-deadline-cancel 42dc3901b32bfd87
pkg-db-q4b-postgres-deadline-fault-phases 6e35e28e51d6bf72
pkg-db-q4b-postgres-malformed-views 45929779913f034f
pkg-db-q4b-postgres-prepared-deadline c116ce709ac486ee
pkg-db-q4b-sqlite-complete-matrix dcde0b8a6ef9d397
pkg-db-q4b-sqlite-direct-stream 9e23a2b6b843f93f
pkg-db-q4b-sqlite-malformed-views a50d4daae76ae810
pkg-db-q4b-sqlite-stream-lifecycle a4746af8bec88901
";

#[test]
fn layer1_case_fingerprints_match_the_golden() {
    let mut log = FingerprintLog::new();
    for case in LAYER1_CASES {
        log.record(&case.fingerprint());
    }
    log.assert_matches(LAYER1_FINGERPRINT_GOLDEN);
}

/// A program whose `hang` case never finishes inside the cap.
///
/// The body is a serial LCG whose addend comes from a run-time string length, so `-O2` can neither
/// close-form it nor delete it — a plain `acc = acc + i` sum was recognised and folded, and the
/// case returned instantly. The bound is finite, so a runaway child still cannot outlive the suite
/// if the kill path itself ever regresses.
const TIMEOUT_MAIN: &str = r#"module main
import std.env
import pkg.db
import pkg.db.postgres
import pkg.db.sqlite

fn main() -> i32 {
  driver := env.get("ALIGN_DB_DRIVER") else { return 90 }
  case := env.get("ALIGN_DB_CASE") else { return 91 }
  if driver == "" { return 92 }
  if case == "__list__" {
    print("hang")
    print("after_hang")
    return 42
  }
  if case == "hang" {
    mut i := 0
    mut acc := 1
    step := case.len() as i64
    loop {
      if i >= 8000000000 { break }
      acc = (acc * 1103515245 + 12345 + step) % 2147483647
      i = i + 1
    }
    return acc as i32
  } else {
    if case == "after_hang" {
      return 8
    } else {
      return 99
    }
  }
}
"#;

/// PR-1 deferred this cell: the timeout path was implemented but no owner drove a hanging case.
///
/// The 2-second cap is generous for a loop that needs minutes, but a sufficiently loaded machine
/// could in principle let the kill land late. That direction fails LOUDLY (the hang row would
/// report a value mismatch instead of a timeout) rather than passing a broken engine, so the
/// failure mode is a visible flake, never a false green.
///
/// Two things must hold together, and only a real hang can show them: the timed-out case is
/// reported as a timeout rather than as some arbitrary signal exit code, and the cases after it
/// still run and still report. If `run_parity` asserted eagerly, the second row would vanish behind
/// the first.
#[test]
fn parity_engine_reports_a_timed_out_case_and_keeps_going() {
    let Some(_gate) = gate(Needs::BackendAndCc) else { return };
    let program = ParityProgram::build(
        "pkg-db-q4b-parity-timeout",
        &q4b(TIMEOUT_MAIN).linking(&PG),
    )
    .with_timeout(std::time::Duration::from_secs(2));

    const CASES: &[ParityCase] = &[
        ParityCase::same("hang", 42),
        // Deliberately wrong: the program returns 8. This row exists to prove it is still reached
        // and still reported after the row before it timed out.
        ParityCase::same("after_hang", 7),
    ];
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_parity(&program, CASES, &[Driver::Sqlite], 0);
    }))
    .expect_err("a timed-out case must fail the table");
    let report = panicked
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();

    // The exact string the engine emits, not a substring that `after_hang` would also satisfy:
    // `contains("hang")` matches "after_hang" too, so it could pass with the hang row missing.
    assert!(
        report.contains("case `hang` on sqlite: expected completion within"),
        "the hang row must be reported as a TIMEOUT, naming the case and driver: {report}"
    );
    assert!(
        report.contains("timed out and was killed"),
        "{report}"
    );
    assert!(
        report.contains("case `after_hang` on sqlite: expected Some(7)"),
        "the case after the hang must still have run and reported its OWN mismatch, which only \
         holds if outcomes are collected before anything is asserted: {report}"
    );
}
