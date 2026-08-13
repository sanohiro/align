//! pkg.db A2/D14 dynamic SQL owners.

mod common;
mod db_harness;

use db_harness::{
    Case, Layout, Needs, PG, RunnerKind, SQLITE_Q4A, expect_checks_clean, expect_checks_rejected,
    package_source,
};

const SQLITE_DYNAMIC_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

fn main() -> i32 {
  connection := pkg.db.sqlite.connect(":memory:", [
    pkg.db.sqlite.ConnectOption.BusyTimeoutNs(1000000),
  ]) else { return 1 }

  created := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.SQLite,
    "CREATE TABLE dynamic_values(id INTEGER PRIMARY KEY)",
    [],
    [],
  ) else { return 2 }
  match created.rows_affected { Some(_) => { return 3 } None => {} }

  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Null,
    pkg.db.value.Bool(true),
    pkg.db.value.I16(-2),
    pkg.db.value.I32(16909060),
    pkg.db.value.I64(-2),
    pkg.db.value.F32(1.5),
    pkg.db.value.F64(2.5),
    pkg.db.value.Text("a\0b"),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
  ]
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.SQLite,
    "SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9",
    params[..],
    [],
  ) else { return 4 }

  arena out {
    first_result := pkg.db.dynamic_next(stream, out)
    first := match first_result {
      Ok(value) => value
      Err(error) => {
        return match error {
          Connection(native) => match native.extended_code {
            Some(code) => code as i32
            None => 51
          }
          Decode(contract) => if contract.item == "db.dynamic.value" { 52 } else { 53 }
          Unsupported(contract) => if contract.item == "db.dynamic.rows.cleanup" { 54 } else { 55 }
          InvalidQuery(_) => 56
          _ => 57
        }
      }
    }
    result := first else { return 6 }
    if result.values.len() != 9 { return 7 }
    match result.values[0] { Null => {} _ => { return 8 } }
    match result.values[1] { I64(v) => if v != 1 { return 9 } _ => { return 10 } }
    match result.values[2] { I64(v) => if v != -2 { return 11 } _ => { return 12 } }
    match result.values[3] { I64(v) => if v != 16909060 { return 13 } _ => { return 14 } }
    match result.values[4] { I64(v) => if v != -2 { return 15 } _ => { return 16 } }
    match result.values[5] { F64(v) => if v != 1.5 { return 17 } _ => { return 18 } }
    match result.values[6] { F64(v) => if v != 2.5 { return 19 } _ => { return 20 } }
    match result.values[7] {
      Text(v) => if v.len() != 3 || v.bytes()[1] != 0 { return 21 }
      _ => { return 22 }
    }
    match result.values[8] {
      Bytes(v) => if v.bytes.len() != 2 || v.bytes[0] != 0 || v.bytes[1] != 255 { return 23 }
      _ => { return 24 }
    }
    exhausted := pkg.db.dynamic_next(stream, out) else { return 25 }
    match exhausted { Some(_) => { return 26 } None => {} }
  }

  empty_params := [
    pkg.db.value.Text(""),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[0..0] }),
    pkg.db.value.Null,
  ]
  mut empty_stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.SQLite,
    "SELECT ?1, ?2, ?3",
    empty_params[..],
    [],
  ) else { return 27 }
  arena empty_out {
    present := pkg.db.dynamic_next(empty_stream, empty_out) else { return 28 }
    row := present else { return 29 }
    match row.values[0] { Text(value) => if value.len() != 0 { return 30 }, _ => { return 31 } }
    match row.values[1] {
      Bytes(value) => if value.bytes.len() != 0 { return 32 }
      _ => { return 33 }
    }
    match row.values[2] { Null => {} _ => { return 34 } }
    exhausted := pkg.db.dynamic_next(empty_stream, empty_out) else { return 48 }
    match exhausted { Some(_) => { return 49 } None => {} }
  }

  repeated_params := [pkg.db.value.I64(7)]
  mut repeated := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT :value, :value", repeated_params[..], [],
  ) else { return 35 }
  arena repeated_out {
    present := pkg.db.dynamic_next(repeated, repeated_out) else { return 36 }
    row := present else { return 37 }
    match row.values[0] { I64(value) => if value != 7 { return 38 }, _ => { return 39 } }
    match row.values[1] { I64(value) => if value != 7 { return 40 }, _ => { return 41 } }
    exhausted := pkg.db.dynamic_next(repeated, repeated_out) else { return 50 }
    match exhausted { Some(_) => { return 51 } None => {} }
  }

  sparse_params := [pkg.db.value.I64(1), pkg.db.value.I64(2), pkg.db.value.I64(3)]
  mut sparse := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT ?3 /* one statement */ -- tail\n", sparse_params[..], [],
  ) else { return 43 }
  arena sparse_out {
    present := pkg.db.dynamic_next(sparse, sparse_out) else { return 44 }
    row := present else { return 45 }
    match row.values[0] { I64(value) => if value != 3 { return 46 }, _ => { return 47 } }
  }
  return 42
}
"#;

const POSTGRES_DYNAMIC_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres

extern "C" {
  fn align_pg_reset()
  fn align_pg_protocol_ok() -> i32
  fn align_pg_protocol_error() -> i32
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Null,
    pkg.db.value.Bool(true),
    pkg.db.value.I16(-2),
    pkg.db.value.I32(16909060),
    pkg.db.value.I64(-2),
    pkg.db.value.F32(1.5),
    pkg.db.value.F64(-0.0),
    pkg.db.value.Text("é"),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
  ]
  stream_result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_MATRIX",
    params[..],
    [],
  )
  mut stream := match stream_result {
    Ok(value) => value
    Err(error) => {
      protocol := unsafe { align_pg_protocol_error() }
      if protocol != 0 { return 100 + protocol }
      return match error {
        Connection(_) => 31
        Native(_) => 32
        Decode(_) => 33
        Unsupported(_) => 34
        InvalidQuery(contract) => {
          if contract.item == "db.dynamic.statement" { return 35 }
          if contract.item == "db.dynamic.rows" { return 37 }
          38
        }
        _ => 36
      }
    }
  }
  arena out {
    first := pkg.db.dynamic_next(stream, out) else { return 3 }
    result := first else { return 4 }
    if result.values.len() != 9 { return 5 }
    match result.values[0] { Null => {} _ => { return 6 } }
    match result.values[1] { Bool(v) => if !v { return 7 } _ => { return 8 } }
    match result.values[2] { I16(v) => if v != -2 { return 9 } _ => { return 10 } }
    match result.values[3] { I32(v) => if v != 16909060 { return 11 } _ => { return 12 } }
    match result.values[4] { I64(v) => if v != -2 { return 13 } _ => { return 14 } }
    match result.values[5] { F32(v) => if v != 1.5 { return 15 } _ => { return 16 } }
    match result.values[6] { F64(v) => if v != 0.0 { return 17 } _ => { return 18 } }
    match result.values[7] { Text(v) => if v != "é" { return 19 } _ => { return 20 } }
    match result.values[8] {
      Bytes(v) => if v.bytes.len() != 2 || v.bytes[0] != 0 || v.bytes[1] != 255 { return 21 }
      _ => { return 22 }
    }
    exhausted := pkg.db.dynamic_next(stream, out) else { return 23 }
    match exhausted { Some(_) => { return 24 } None => {} }
  }

  command := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.PostgreSQL,
    "UPDATE stub SET value = 1 /* DYNAMIC_COMMAND */",
    [],
    [],
  ) else { return 25 }
  match command.rows_affected {
    Some(value) => if value != 2 { return 26 }
    None => { return 27 }
  }

  mut many := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_MATRIX DYNAMIC_MANY_ROWS",
    params[..],
    [],
  ) else { return 28 }
  arena many_out {
    first_many := pkg.db.dynamic_next(many, many_out) else { return 29 }
    first_row := first_many else { return 30 }
    second_many := pkg.db.dynamic_next(many, many_out) else { return 31 }
    second_row := second_many else { return 32 }
    match first_row.values[7] { Text(value) => if value != "é" { return 33 }, _ => { return 34 } }
    match second_row.values[8] {
      Bytes(value) => if value.bytes.len() != 2 || value.bytes[1] != 255 { return 35 }
      _ => { return 36 }
    }
    exhausted := pkg.db.dynamic_next(many, many_out) else { return 37 }
    match exhausted { Some(_) => { return 38 } None => {} }
  }

  mut empty_rows := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_ZERO_ROWS",
    params[..],
    [],
  ) else { return 39 }
  arena empty_out {
    exhausted := pkg.db.dynamic_next(empty_rows, empty_out) else { return 40 }
    match exhausted { Some(_) => { return 41 } None => {} }
  }

  mut zero_columns := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE DYNAMIC_ZERO_COLUMNS",
    [],
    [],
  ) else { return 47 }
  arena zero_out {
    present := pkg.db.dynamic_next(zero_columns, zero_out) else { return 43 }
    row := present else { return 44 }
    if row.values.len() != 0 { return 45 }
  }
  empty_params := [
    pkg.db.value.Text(""),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[0..0] }),
    pkg.db.value.Null,
  ]
  mut empty_values := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_EMPTY_VALUES",
    empty_params[..],
    [],
  ) else { return 48 }
  arena empty_values_out {
    present := pkg.db.dynamic_next(empty_values, empty_values_out) else { return 49 }
    row := present else { return 50 }
    match row.values[0] { Text(value) => if value.len() != 0 { return 51 }, _ => { return 52 } }
    match row.values[1] {
      Bytes(value) => if value.bytes.len() != 0 { return 53 }
      _ => { return 54 }
    }
    match row.values[2] { Null => {} _ => { return 55 } }
  }
  if unsafe { align_pg_protocol_ok() } != 1 { return 46 }
  return 42
}
"#;

const SQLITE_STUB_DYNAMIC_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_fail_next_busy_timeout()
  fn align_sqlite_q4a_prepare_calls() -> i32
  fn align_sqlite_q4a_finalize_calls() -> i32
  fn align_sqlite_q4a_busy_timeout_calls() -> i32
  fn align_sqlite_q4a_last_busy_timeout() -> i32
  fn align_sqlite_q4a_protocol_ok() -> i32
}

fn open_and_drop(
  borrow connection: pkg.db.conn,
  params: slice<pkg.db.value>,
) -> i32 {
  made := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE",
    params,
    [],
  )
  return match made {
    Err(_) => 1
    Ok(stream) => 0
  }
}

fn main() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  connection := pkg.db.sqlite.connect(":stub:", [
    pkg.db.sqlite.ConnectOption.BusyTimeoutNs(3000000),
  ]) else { return 1 }
  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Null,
    pkg.db.value.Bool(true),
    pkg.db.value.I16(-2),
    pkg.db.value.I32(16909060),
    pkg.db.value.I64(-2),
    pkg.db.value.F32(1.5),
    pkg.db.value.F64(2.5),
    pkg.db.value.Text("a\0b"),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
  ]

  if open_and_drop(connection, params[..]) != 0 { return 2 }
  if unsafe { align_sqlite_q4a_prepare_calls() } != 1 { return 3 }
  if unsafe { align_sqlite_q4a_finalize_calls() } != 1 { return 4 }
  if unsafe { align_sqlite_q4a_busy_timeout_calls() } != 3 { return 5 }
  if unsafe { align_sqlite_q4a_last_busy_timeout() } != 3 { return 6 }

  command := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.SQLite,
    "UPDATE stub SET value = 1 /* DYNAMIC_SQLITE_COMMAND */",
    [],
    [],
  ) else { return 7 }
  match command.rows_affected { Some(_) => { return 8 } None => {} }
  if unsafe { align_sqlite_q4a_prepare_calls() } != 2 { return 9 }
  if unsafe { align_sqlite_q4a_finalize_calls() } != 2 { return 10 }
  if unsafe { align_sqlite_q4a_busy_timeout_calls() } != 4 { return 11 }

  unsafe { align_sqlite_q4a_fail_next_busy_timeout() }
  failed := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection),
    pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE",
    params[..],
    [],
  )
  cleanup_failure := match failed {
    Ok(_) => false
    Err(error) => match error {
      Unsupported(contract) => contract.item == "db.dynamic.rows.cleanup"
      _ => false
    }
  }
  if !cleanup_failure { return 12 }
  if unsafe { align_sqlite_q4a_prepare_calls() } != 3 { return 13 }
  if unsafe { align_sqlite_q4a_finalize_calls() } != 3 { return 14 }
  if unsafe { align_sqlite_q4a_busy_timeout_calls() } != 5 { return 15 }
  if unsafe { align_sqlite_q4a_protocol_ok() } != 1 { return 16 }
  return 42
}
"#;

const DYNAMIC_VALIDATION_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import pkg.db.sqlite

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_protocol_ok() -> i32
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_prepare_calls() -> i32
  fn align_sqlite_q4a_protocol_ok() -> i32
}

fn sqlite_preflight(borrow connection: pkg.db.conn) -> i32 {
  mismatch := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "bad\0sql", [pkg.db.value.F64(0.0 / 0.0)][..],
    [pkg.db.ExecuteOption.TimeoutNs(0)],
  )
  mismatch_ok := match mismatch {
    Err(error) => match error {
      DriverMismatch(contract) => contract.item == "db.dynamic.driver"
        && match contract.query_id { None => true, Some(_) => false }
      _ => false
    }
    Ok(_) => false
  }
  if !mismatch_ok { return 1 }

  nul_sql := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "bad\0sql", [], [pkg.db.ExecuteOption.TimeoutNs(0)],
  )
  nul_ok := match nul_sql {
    Err(error) => match error {
      Encode(contract) => contract.item == "db.dynamic.sql"
        && contract.message == "dynamic SQL contains U+0000"
      _ => false
    }
    Ok(_) => false
  }
  if !nul_ok { return 2 }

  invalid_timeout := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "UPDATE DYNAMIC_SQLITE_COMMAND", [], [pkg.db.ExecuteOption.TimeoutNs(0)],
  )
  invalid_ok := match invalid_timeout {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "db.execute.timeout_ns"
        && contract.message == "database execution timeout must be positive"
      _ => false
    }
    Ok(_) => false
  }
  if !invalid_ok { return 3 }

  duplicate := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "UPDATE DYNAMIC_SQLITE_COMMAND", [], [
      pkg.db.ExecuteOption.TimeoutNs(1),
      pkg.db.ExecuteOption.TimeoutNs(0),
    ],
  )
  duplicate_payload_ok := match duplicate {
    Err(error) => match error {
      Unsupported(contract) => contract.message == "database execution timeout must be positive"
      _ => false
    }
    Ok(_) => false
  }
  if !duplicate_payload_ok { return 4 }

  unavailable := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "UPDATE DYNAMIC_SQLITE_COMMAND", [], [pkg.db.ExecuteOption.TimeoutNs(1)],
  )
  unavailable_ok := match unavailable {
    Err(error) => match error {
      Unsupported(contract) => contract.message
        == "SQLite does not support common execution deadlines"
      _ => false
    }
    Ok(_) => false
  }
  if !unavailable_ok || unsafe { align_sqlite_q4a_prepare_calls() } != 0 { return 5 }

  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Null,
    pkg.db.value.Bool(true),
    pkg.db.value.I16(-2),
    pkg.db.value.I32(16909060),
    pkg.db.value.I64(-2),
    pkg.db.value.F32(1.5),
    pkg.db.value.F64(2.5),
    pkg.db.value.Text("a\0b"),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
  ]
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE", params[..], [],
  ) else { return 6 }
  overlap := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "UPDATE DYNAMIC_SQLITE_COMMAND", [pkg.db.value.F64(0.0 / 0.0)][..], [],
  )
  overlap_ok := match overlap {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "sqlite.connection.active_execution"
      _ => false
    }
    Ok(_) => false
  }
  if !overlap_ok || unsafe { align_sqlite_q4a_prepare_calls() } != 1 { return 7 }
  return 0
}

fn postgres_overlap(borrow connection: pkg.db.conn) -> i32 {
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE", [], [],
  ) else { return 1 }
  overlap := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "UPDATE DYNAMIC_COMMAND", [pkg.db.value.Text("a\0b")][..], [],
  )
  overlap_ok := match overlap {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "postgres.connection.active_execution"
      _ => false
    }
    Ok(_) => false
  }
  if !overlap_ok || unsafe { align_pg_execute_calls() } != 1 { return 2 }
  return 0
}

fn main() -> i32 {
  unsafe {
    align_pg_reset()
    align_sqlite_q4a_reset()
  }
  sqlite := pkg.db.sqlite.connect(":stub:", []) else { return 1 }
  sqlite_status := sqlite_preflight(sqlite)
  if sqlite_status != 0 { return 10 + sqlite_status }

  nan := pkg.db.dynamic_execute(
    pkg.db.exec_conn(sqlite), pkg.db.Driver.SQLite,
    "UPDATE DYNAMIC_SQLITE_COMMAND", [pkg.db.value.F64(0.0 / 0.0)][..], [],
  )
  nan_ok := match nan {
    Err(error) => match error {
      Encode(contract) => contract.item == "db.dynamic.parameter"
        && contract.message == "SQLite dynamic floating parameter must not be NaN"
      _ => false
    }
    Ok(_) => false
  }
  if !nan_ok || unsafe { align_sqlite_q4a_prepare_calls() } != 1 { return 20 }

  empty := pkg.db.dynamic_execute(
    pkg.db.exec_conn(sqlite), pkg.db.Driver.SQLite,
    "DYNAMIC_SQLITE_EMPTY", [], [],
  )
  empty_ok := match empty {
    Err(error) => match error {
      InvalidQuery(contract) => contract.item == "db.dynamic.statement"
      _ => false
    }
    Ok(_) => false
  }
  if !empty_ok { return 21 }

  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Null,
    pkg.db.value.Bool(true),
    pkg.db.value.I16(-2),
    pkg.db.value.I32(16909060),
    pkg.db.value.I64(-2),
    pkg.db.value.F32(1.5),
    pkg.db.value.F64(2.5),
    pkg.db.value.Text("a\0b"),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
  ]
  multi := pkg.db.dynamic_rows(
    pkg.db.exec_conn(sqlite), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE_MULTI SECOND", params[..], [],
  )
  multi_ok := match multi {
    Err(error) => match error {
      InvalidQuery(contract) => contract.item == "db.dynamic.statement"
      _ => false
    }
    Ok(_) => false
  }
  if !multi_ok { return 22 }

  mismatch_count := pkg.db.dynamic_rows(
    pkg.db.exec_conn(sqlite), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE", [], [],
  )
  count_ok := match mismatch_count {
    Err(error) => match error {
      InvalidQuery(contract) => contract.item == "db.dynamic.parameters"
      _ => false
    }
    Ok(_) => false
  }
  if !count_ok { return 23 }

  rows_command := pkg.db.dynamic_rows(
    pkg.db.exec_conn(sqlite), pkg.db.Driver.SQLite,
    "UPDATE DYNAMIC_SQLITE_COMMAND", [], [],
  )
  rows_kind_ok := match rows_command {
    Err(error) => match error {
      InvalidQuery(contract) => contract.item == "db.dynamic.rows"
      _ => false
    }
    Ok(_) => false
  }
  if !rows_kind_ok { return 24 }

  execute_rows := pkg.db.dynamic_execute(
    pkg.db.exec_conn(sqlite), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE", params[..], [],
  )
  execute_kind_ok := match execute_rows {
    Err(error) => match error {
      InvalidQuery(contract) => contract.item == "db.dynamic.execute"
      _ => false
    }
    Ok(_) => false
  }
  if !execute_kind_ok || unsafe { align_sqlite_q4a_prepare_calls() } != 6 { return 25 }

  postgres := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 30 }
  pg_mismatch := pkg.db.dynamic_execute(
    pkg.db.exec_conn(postgres), pkg.db.Driver.SQLite,
    "bad\0sql", [pkg.db.value.Text("a\0b")][..], [],
  )
  pg_mismatch_ok := match pg_mismatch {
    Err(error) => match error { DriverMismatch(_) => true, _ => false }
    Ok(_) => false
  }
  if !pg_mismatch_ok { return 31 }
  pg_nul := pkg.db.dynamic_execute(
    pkg.db.exec_conn(postgres), pkg.db.Driver.PostgreSQL,
    "UPDATE DYNAMIC_COMMAND", [pkg.db.value.Text("a\0b")][..], [],
  )
  pg_nul_ok := match pg_nul {
    Err(error) => match error {
      Encode(contract) => contract.message
        == "PostgreSQL dynamic text parameter contains U+0000"
      _ => false
    }
    Ok(_) => false
  }
  if !pg_nul_ok || unsafe { align_pg_execute_calls() } != 0 { return 32 }
  pg_overlap_status := postgres_overlap(postgres)
  if pg_overlap_status != 0 { return 32 + pg_overlap_status }
  reused := pkg.db.dynamic_execute(
    pkg.db.exec_conn(postgres), pkg.db.Driver.PostgreSQL,
    "UPDATE DYNAMIC_COMMAND", [], [],
  ) else { return 35 }
  if unsafe { align_pg_execute_calls() } != 2 { return 36 }
  if unsafe { align_pg_protocol_ok() } != 1
    || unsafe { align_sqlite_q4a_protocol_ok() } != 1 { return 37 }
  return 42
}
"#;

const POSTGRES_STATUS_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres

extern "C" {
  fn align_pg_reset()
  fn align_pg_force_result_status(status: i32)
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_nonblocking_calls() -> i32
  fn align_pg_forbidden_after_status_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn rejects(status: i32) -> i32 {
  unsafe {
    align_pg_reset()
    align_pg_force_result_status(status)
  }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE", [], [],
  )
  rejected := match result { Err(_) => true, Ok(_) => false }
  unsafe { align_pg_force_result_status(-1) }
  if !rejected { return 2 }
  if unsafe { align_pg_execute_calls() } != 1 { return 3 }
  if unsafe { align_pg_clear_calls() } != 1 { return 4 }
  if unsafe { align_pg_finish_calls() } != 1 { return 5 }
  if unsafe { align_pg_forbidden_after_status_calls() } != 0 { return 6 }
  if unsafe { align_pg_protocol_ok() } != 1 { return 7 }
  return 0
}

fn rejects_async(status: i32) -> i32 {
  unsafe {
    align_pg_reset()
    align_pg_force_result_status(status)
  }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE", [], [pkg.db.ExecuteOption.TimeoutNs(1000000000)],
  )
  rejected := match result { Err(_) => true, Ok(_) => false }
  unsafe { align_pg_force_result_status(-1) }
  if !rejected { return 2 }
  if unsafe { align_pg_execute_calls() } != 1
    || unsafe { align_pg_clear_calls() } != 1
    || unsafe { align_pg_finish_calls() } != 1 { return 3 }
  // Once the unsafe result status is observed there is no second status read, result fetch,
  // transaction probe, or blocking-mode restore.
  if unsafe { align_pg_forbidden_after_status_calls() } != 0
    || unsafe { align_pg_nonblocking_calls() } != 1 { return 4 }
  return 0
}

fn rejects_ordinary(status: i32) -> i32 {
  unsafe {
    align_pg_reset()
    align_pg_force_result_status(status)
  }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE", [], [],
  )
  expected := match result {
    Err(error) => if status == 0 {
      match error { InvalidQuery(contract) => contract.item == "db.dynamic.statement", _ => false }
    } else {
      if status == 1 {
        match error { InvalidQuery(contract) => contract.item == "db.dynamic.rows", _ => false }
      } else { match error { Native(_) => true, _ => false } }
    }
    Ok(_) => false
  }
  unsafe { align_pg_force_result_status(-1) }
  if !expected || unsafe { align_pg_clear_calls() } != 1
    || unsafe { align_pg_finish_calls() } != 0 { return 2 }
  reused := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "UPDATE DYNAMIC_COMMAND", [], [],
  ) else { return 3 }
  if unsafe { align_pg_execute_calls() } != 2
    || unsafe { align_pg_clear_calls() } != 2 { return 4 }
  return 0
}

fn main() -> i32 {
  statuses := [3 as i32, 4 as i32, 8 as i32, 10 as i32, 11 as i32, 13 as i32]
  mut i := 0
  loop {
    if i >= statuses.len() { break }
    result := rejects(statuses[i])
    if result != 0 { return 10 + (i as i32) * 10 + result }
    i = i + 1
  }
  copy_async := rejects_async(3)
  if copy_async != 0 { return 80 + copy_async }
  unknown_async := rejects_async(13)
  if unknown_async != 0 { return 90 + unknown_async }
  ordinary := [0 as i32, 1 as i32, 5 as i32, 6 as i32, 7 as i32, 9 as i32, 12 as i32]
  i = 0
  loop {
    if i >= ordinary.len() { break }
    result := rejects_ordinary(ordinary[i])
    if result != 0 { return 100 + (i as i32) * 10 + result }
    i = i + 1
  }
  return 42
}
"#;

const POSTGRES_RESULT_FAULT_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres

extern "C" {
  fn align_pg_reset()
  fn align_pg_set_binary_fault(fault: i32)
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn constructor_fault(fault: i32) -> i32 {
  unsafe {
    align_pg_reset()
    align_pg_set_binary_fault(fault)
  }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  params := [pkg.db.value.Bool(true), pkg.db.value.Text("valid")]
  result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT BINARY_FAULT", params[..], [],
  )
  rejected := match result {
    Err(error) => match error {
      Decode(contract) => contract.item == "db.dynamic.value"
        && contract.message == "database dynamic value has an unsupported native type"
      _ => false
    }
    Ok(_) => false
  }
  if !rejected { return 2 }
  if unsafe { align_pg_execute_calls() } != 1
    || unsafe { align_pg_clear_calls() } != 1
    || unsafe { align_pg_protocol_ok() } != 1 { return 3 }
  return 0
}

fn value_fault(fault: i32) -> i32 {
  unsafe {
    align_pg_reset()
    align_pg_set_binary_fault(fault)
  }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  params := [pkg.db.value.Bool(true), pkg.db.value.Text("valid")]
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT BINARY_FAULT", params[..], [],
  ) else { return 2 }
  arena out {
    first := pkg.db.dynamic_next(stream, out)
    rejected := match first {
      Err(error) => match error {
        Decode(contract) => contract.item == "db.dynamic.value"
          && contract.message == "database dynamic value has an invalid native representation"
        _ => false
      }
      Ok(_) => false
    }
    if !rejected { return 3 }
    repeated := pkg.db.dynamic_next(stream, out)
    repeated_ok := match repeated {
      Err(error) => match error {
        InvalidQuery(contract) => contract.item == "db.dynamic.rows.state"
        _ => false
      }
      Ok(_) => false
    }
    if !repeated_ok { return 4 }
  }
  if unsafe { align_pg_execute_calls() } != 1
    || unsafe { align_pg_clear_calls() } != 1
    || unsafe { align_pg_protocol_ok() } != 1 { return 5 }
  reused := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "UPDATE DYNAMIC_COMMAND", [], [],
  ) else { return 6 }
  return 0
}

fn unsupported_shape(sql: str) -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL, sql, [], [],
  )
  return match result {
    Err(error) => match error {
      Decode(contract) => if contract.message
        == "database dynamic value has an unsupported native type" { 0 } else { 2 }
      _ => 3
    }
    Ok(_) => 4
  }
}

fn main() -> i32 {
  constructor_format := constructor_fault(1)
  if constructor_format != 0 { return 10 + constructor_format }
  metadata_before_zero := constructor_fault(7)
  if metadata_before_zero != 0 { return 20 + metadata_before_zero }
  mut fault := 2
  loop {
    if fault > 6 { break }
    value_result := value_fault(fault)
    if value_result != 0 { return 30 + fault * 10 + value_result }
    fault = fault + 1
  }
  oid := unsupported_shape("SELECT DYNAMIC_SIMPLE DYNAMIC_UNSUPPORTED_OID")
  if oid != 0 { return 90 + oid }
  format := unsupported_shape("SELECT DYNAMIC_SIMPLE DYNAMIC_TEXT_FORMAT")
  if format != 0 { return 100 + format }
  return 42
}
"#;

const POSTGRES_INVARIANT_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres

extern "C" {
  fn align_pg_reset()
  fn align_pg_force_result_status(status: i32)
  fn align_pg_encoding_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_forbidden_after_status_calls() -> i32
}

fn encoding_cleanup() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE DYNAMIC_ENCODING_BAD", [], [],
  )
  cleanup := match result {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "db.dynamic.rows.cleanup"
      _ => false
    }
    Ok(_) => false
  }
  if !cleanup { return 2 }
  if unsafe { align_pg_encoding_calls() } != 2
    || unsafe { align_pg_clear_calls() } != 1
    || unsafe { align_pg_finish_calls() } != 1 { return 3 }
  return 0
}

fn first_error_wins() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE DYNAMIC_NATIVE_ERROR DYNAMIC_ENCODING_BAD", [], [],
  )
  native := match result { Err(error) => match error { Native(_) => true, _ => false }, Ok(_) => false }
  if !native { return 2 }
  if unsafe { align_pg_encoding_calls() } != 2
    || unsafe { align_pg_clear_calls() } != 1
    || unsafe { align_pg_finish_calls() } != 1 { return 3 }
  return 0
}

fn unsafe_status_skips_encoding() -> i32 {
  unsafe {
    align_pg_reset()
    align_pg_force_result_status(3)
  }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE DYNAMIC_ENCODING_BAD", [], [],
  )
  unsafe { align_pg_force_result_status(-1) }
  rejected := match result { Err(_) => true, Ok(_) => false }
  if !rejected { return 2 }
  if unsafe { align_pg_encoding_calls() } != 1
    || unsafe { align_pg_forbidden_after_status_calls() } != 0 { return 3 }
  return 0
}

fn connection_transaction_drift() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE DYNAMIC_TX_DRIFT", [], [],
  )
  return match result {
    Err(error) => match error {
      Unsupported(contract) => if contract.item == "db.dynamic.rows.cleanup" { 0 } else { 2 }
      _ => 3
    }
    Ok(_) => 4
  }
}

fn transaction_idle_drift() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  transaction := pkg.db.begin(connection, []) else { return 2 }
  result := pkg.db.dynamic_rows(
    pkg.db.exec_tx(transaction), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE DYNAMIC_TX_IDLE", [], [],
  )
  cleanup := match result {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "db.dynamic.rows.cleanup"
      _ => false
    }
    Ok(_) => false
  }
  if !cleanup || unsafe { align_pg_finish_calls() } != 1 { return 3 }
  return 0
}

fn main() -> i32 {
  first := encoding_cleanup()
  if first != 0 { return 10 + first }
  second := first_error_wins()
  if second != 0 { return 20 + second }
  third := unsafe_status_skips_encoding()
  if third != 0 { return 30 + third }
  fourth := connection_transaction_drift()
  if fourth != 0 { return 40 + fourth }
  fifth := transaction_idle_drift()
  if fifth != 0 { return 50 + fifth }
  return 42
}
"#;

const POSTGRES_TIMEOUT_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_nonblocking_calls() -> i32
  fn align_pg_cancel_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn timeout_connection() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  timed := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE TIMEOUT_WAIT", [],
    [pkg.db.ExecuteOption.TimeoutNs(2000000)],
  )
  timeout_ok := match timed { Err(error) => match error { Timeout(_) => true, _ => false }, Ok(_) => false }
  if !timeout_ok { return 2 }
  if unsafe { align_pg_cancel_calls() } != 1
    || unsafe { align_pg_nonblocking_calls() } != 2 { return 3 }
  reused := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "UPDATE DYNAMIC_COMMAND", [], [],
  ) else { return 4 }
  if unsafe { align_pg_execute_calls() } != 2 { return 5 }
  return 0
}

fn timeout_transaction() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  transaction := pkg.db.begin(connection, []) else { return 2 }
  timed := pkg.db.dynamic_execute(
    pkg.db.exec_tx(transaction), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE TIMEOUT_WAIT", [],
    [pkg.db.ExecuteOption.TimeoutNs(2000000)],
  )
  timeout_ok := match timed { Err(error) => match error { Timeout(_) => true, _ => false }, Ok(_) => false }
  if !timeout_ok { return 3 }
  returned := pkg.db.rollback(transaction) else { return 4 }
  command := pkg.db.dynamic_execute(
    pkg.db.exec_conn(returned), pkg.db.Driver.PostgreSQL,
    "UPDATE DYNAMIC_COMMAND", [], [],
  ) else { return 5 }
  return 0
}

fn timeout_success() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE", [], [pkg.db.ExecuteOption.TimeoutNs(1000000000)],
  ) else { return 2 }
  arena out {
    first := pkg.db.dynamic_next(stream, out) else { return 3 }
    row := first else { return 4 }
    match row.values[0] { Bool(value) => if !value { return 5 }, _ => { return 6 } }
  }
  if unsafe { align_pg_nonblocking_calls() } != 2
    || unsafe { align_pg_cancel_calls() } != 0 { return 7 }
  return 0
}

fn main() -> i32 {
  first := timeout_connection()
  if first != 0 { return 10 + first }
  second := timeout_transaction()
  if second != 0 { return 20 + second }
  third := timeout_success()
  if third != 0 { return 30 + third }
  if unsafe { align_pg_protocol_ok() } != 1 { return 40 }
  return 42
}
"#;

const DYNAMIC_TEST_HELPER: &str = r#"module pkg.db.a2_test
import pkg.db
import pkg.db.internal.resource

pub fn corrupt(borrow mut stream: pkg.db.dynamic_rows, mutation: i32) {
  unsafe {
    wrapper := resource.raw(resource.borrow(stream))
    if mutation == 0 { raw.store(wrapper, 0, 2 as u32) }
    if mutation == 1 { raw.store(wrapper, 4, 3 as u8) }
    if mutation == 2 { raw.store(wrapper, 5, 3 as u8) }
    if mutation == 3 { raw.store(wrapper, 6, 1 as u16) }
    if mutation == 4 { raw.store(wrapper, 8, raw.null()) }
    if mutation == 5 { raw.store(wrapper, 16, raw.null()) }
    if mutation == 6 { raw.store(wrapper, 24, -1 as i64) }
    if mutation == 7 { raw.store(wrapper, 32, -1 as i64) }
    if mutation == 8 { raw.store(wrapper, 24, 2 as i64) }
    if mutation == 9 { raw.store(wrapper, 40, -1 as i64) }
    if mutation == 10 { raw.store(wrapper, 5, 1 as u8) }
  }
}

pub fn live_shape(borrow stream: pkg.db.dynamic_rows) -> bool {
  unsafe {
    wrapper := resource.raw(resource.borrow(stream))
    version: u32 := raw.load(wrapper, 0)
    driver: u8 := raw.load(wrapper, 4)
    terminal: u8 := raw.load(wrapper, 5)
    reserved: u16 := raw.load(wrapper, 6)
    native: raw := raw.load(wrapper, 8)
    state: raw := raw.load(wrapper, 16)
    next: i64 := raw.load(wrapper, 24)
    rows: i64 := raw.load(wrapper, 32)
    columns: i64 := raw.load(wrapper, 40)
    return version == 1 && driver == 2 && terminal == 0 && reserved == 0
      && !native.is_null() && !state.is_null() && next == 0 && rows == 1 && columns == 1
      && pkg.db.internal.resource.dynamic_rows_header(wrapper) == 0
  }
}
"#;

const POSTGRES_MALFORMED_RESOURCE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import pkg.db.a2_test

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_forbidden_after_status_calls() -> i32
}

fn one(mutation: i32) -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE", [], [],
  ) else { return 2 }
  if !pkg.db.a2_test.live_shape(stream) { return 3 }
  pkg.db.a2_test.corrupt(stream, mutation)
  arena out {
    result := pkg.db.dynamic_next(stream, out)
    rejected := match result {
      Err(error) => match error {
        InvalidQuery(contract) => contract.item == "db.dynamic.rows.state"
      _ => false
      }
      Ok(_) => false
    }
    if !rejected { return 4 }
  }
  if unsafe { align_pg_execute_calls() } != 1
    || unsafe { align_pg_clear_calls() } != 0
    || unsafe { align_pg_finish_calls() } != 0
    || unsafe { align_pg_forbidden_after_status_calls() } != 0 { return 5 }
  return 0
}

fn main() -> i32 {
  mut mutation := 0
  loop {
    if mutation > 10 { break }
    result := one(mutation)
    if result != 0 { return 10 + mutation * 10 + result }
    if unsafe { align_pg_finish_calls() } != 1
      || unsafe { align_pg_clear_calls() } != 0 { return 120 + mutation }
    mutation = mutation + 1
  }
  return 42
}
"#;

const DYNAMIC_PUBLIC_USER: &str = r#"module app.dynamic_user
import pkg.db

pub fn classify(value: pkg.db.value) -> i32 = match value {
  Null => 0
  Bool(_) => 1
  I16(_) => 2
  I32(_) => 3
  I64(_) => 4
  F32(_) => 5
  F64(_) => 6
  Text(_) => 7
  Bytes(_) => 8
}

pub fn make(bytes: slice<u8>) -> pkg.db.value =
  pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes })

pub fn advance(
  borrow mut stream: pkg.db.dynamic_rows,
  out: region,
) -> Result<Option<pkg.db.row>, pkg.db.Error> = pkg.db.dynamic_next(stream, out)
"#;

const DYNAMIC_ROW_BORROW_MAIN: &str = r#"module main
import pkg.db
import app.dynamic_user

fn inspect(borrow row: pkg.db.row) -> i64 = row.values.len()

fn use_row(
  borrow mut stream: pkg.db.dynamic_rows,
  out: region,
) -> Result<i64, pkg.db.Error> {
  advanced := app.dynamic_user.advance(stream, out)
  return match advanced {
    Err(error) => Err(error)
    Ok(selected) => match selected {
      None => Ok(0)
      Some(row) => Ok(inspect(row))
    }
  }
}

fn main() {}
"#;

const DYNAMIC_ROW_VALUE_MAIN: &str = r#"module main
import pkg.db
import app.dynamic_user

fn consume(row: pkg.db.row) -> i64 = row.values.len()

fn use_row(
  borrow mut stream: pkg.db.dynamic_rows,
  out: region,
) -> Result<i64, pkg.db.Error> {
  advanced := app.dynamic_user.advance(stream, out)
  return match advanced {
    Err(error) => Err(error)
    Ok(selected) => match selected {
      None => Ok(0)
      Some(row) => Ok(consume(row))
    }
  }
}

fn main() {}
"#;

const DYNAMIC_PUBLIC_MAIN: &str = r#"module main
import pkg.db
import app.dynamic_user

fn main() -> i32 {
  bytes := [0 as u8, 255 as u8]
  value := app.dynamic_user.make(bytes[..])
  copied := value
  first := app.dynamic_user.classify(value)
  second := app.dynamic_user.classify(copied)
  return if first == 8 && second == 8 { 42 } else { 1 }
}
"#;

const BARE_SLICE_SUM_MAIN: &str = r#"module main

bad_value {
  Bytes(slice<u8>)
}

fn main() {}
"#;

const REGION_ESCAPE_MAIN: &str = r#"module main
import pkg.db

fn escape(
  borrow mut stream: pkg.db.dynamic_rows,
) -> Result<Option<pkg.db.row>, pkg.db.Error> {
  arena out {
    return pkg.db.dynamic_next(stream, out)
  }
}

fn main() {}
"#;

const PRIVATE_RESOURCE_IMPORT_MAIN: &str = r#"module main
import pkg.db.internal.resource

fn main() {}
"#;

const DYNAMIC_POOL_MAIN: &str = r#"module main
import pkg.db
import pkg.db.pool

extern "C" {
  fn align_pg_reset()
  fn align_sqlite_q4a_reset()
  fn align_pg_protocol_ok() -> i32
  fn align_sqlite_q4a_protocol_ok() -> i32
}

fn sqlite_direct(borrow owner: pkg.db.pool.Pool) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 1 }
  command := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "UPDATE DYNAMIC_SQLITE_COMMAND", [], [],
  ) else { return 2 }
  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Null, pkg.db.value.Bool(true), pkg.db.value.I16(-2),
    pkg.db.value.I32(16909060), pkg.db.value.I64(-2), pkg.db.value.F32(1.5),
    pkg.db.value.F64(2.5), pkg.db.value.Text("a\0b"),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
  ]
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE", params[..], [],
  ) else { return 3 }
  arena out {
    first := pkg.db.dynamic_next(stream, out) else { return 4 }
    row := first else { return 5 }
    if row.values.len() != 9 { return 6 }
  }
  return 0
}

fn sqlite_transaction(borrow owner: pkg.db.pool.Pool) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 1 }
  transaction := pkg.db.begin(connection, []) else { return 2 }
  command := pkg.db.dynamic_execute(
    pkg.db.exec_tx(transaction), pkg.db.Driver.SQLite,
    "UPDATE DYNAMIC_SQLITE_COMMAND", [], [],
  ) else { return 3 }
  returned := pkg.db.rollback(transaction) else { return 4 }
  return 0
}

fn postgres_direct(borrow owner: pkg.db.pool.Pool) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 1 }
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_SIMPLE", [], [],
  ) else { return 2 }
  arena out {
    first := pkg.db.dynamic_next(stream, out) else { return 3 }
    row := first else { return 4 }
    match row.values[0] { Bool(value) => if !value { return 5 }, _ => { return 6 } }
  }
  return 0
}

fn postgres_transaction(borrow owner: pkg.db.pool.Pool) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 1 }
  transaction := pkg.db.begin(connection, []) else { return 2 }
  command := pkg.db.dynamic_execute(
    pkg.db.exec_tx(transaction), pkg.db.Driver.PostgreSQL,
    "UPDATE DYNAMIC_COMMAND", [], [],
  ) else { return 3 }
  returned := pkg.db.rollback(transaction) else { return 4 }
  return 0
}

fn idle(borrow owner: pkg.db.pool.Pool) -> bool {
  state := pkg.db.pool.info(owner) else { return false }
  return state.capacity == 1 && state.idle == 1 && state.checked_out == 0
}

fn main() -> i32 {
  unsafe {
    align_pg_reset()
    align_sqlite_q4a_reset()
  }
  sqlite := pkg.db.pool.open_sqlite(":stub:", 1, []) else { return 1 }
  first := sqlite_direct(sqlite)
  if first != 0 || !idle(sqlite) { return 10 + first }
  second := sqlite_transaction(sqlite)
  if second != 0 || !idle(sqlite) { return 20 + second }

  postgres := pkg.db.pool.open_postgres("postgresql://stub/pool", 1, []) else { return 30 }
  third := postgres_direct(postgres)
  if third != 0 || !idle(postgres) { return 40 + third }
  fourth := postgres_transaction(postgres)
  if fourth != 0 || !idle(postgres) { return 50 + fourth }
  if unsafe { align_pg_protocol_ok() } != 1
    || unsafe { align_sqlite_q4a_protocol_ok() } != 1 { return 60 }
  return 42
}
"#;

const LIVE_POSTGRES_DYNAMIC_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres

fn run(url: str) -> i32 {
  connection := pkg.db.postgres.connect(url, []) else { return 1 }
  created := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "CREATE TEMP TABLE align_dynamic_values(b bool, s int2, i int4, l int8, f4 real, f8 double precision, t text, data bytea, absent text)",
    [], [],
  ) else { return 2 }
  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Bool(true), pkg.db.value.I16(-2), pkg.db.value.I32(16909060),
    pkg.db.value.I64(-2), pkg.db.value.F32(1.5), pkg.db.value.F64(-0.0),
    pkg.db.value.Text("é"), pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
    pkg.db.value.Null,
  ]
  inserted := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "INSERT INTO align_dynamic_values VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::text)",
    params[..], [],
  ) else { return 3 }
  match inserted.rows_affected { Some(value) => if value != 1 { return 4 }, None => { return 5 } }

  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT b, s, i, l, f4, f8, t, data, absent FROM align_dynamic_values", [], [],
  ) else { return 6 }
  arena out {
    first := pkg.db.dynamic_next(stream, out) else { return 7 }
    row := first else { return 8 }
    if row.values.len() != 9 { return 9 }
    match row.values[0] { Bool(value) => if !value { return 10 }, _ => { return 11 } }
    match row.values[1] { I16(value) => if value != -2 { return 12 }, _ => { return 13 } }
    match row.values[2] { I32(value) => if value != 16909060 { return 14 }, _ => { return 15 } }
    match row.values[3] { I64(value) => if value != -2 { return 16 }, _ => { return 17 } }
    match row.values[4] { F32(value) => if value != 1.5 { return 18 }, _ => { return 19 } }
    match row.values[5] { F64(value) => if value != 0.0 { return 20 }, _ => { return 21 } }
    match row.values[6] { Text(value) => if value != "é" { return 22 }, _ => { return 23 } }
    match row.values[7] {
      Bytes(value) => if value.bytes.len() != 2 || value.bytes[0] != 0
        || value.bytes[1] != 255 { return 24 }
      _ => { return 25 }
    }
    match row.values[8] { Null => {} _ => { return 26 } }
    exhausted := pkg.db.dynamic_next(stream, out) else { return 27 }
    match exhausted { Some(_) => { return 28 } None => {} }
  }
  dropped := pkg.db.dynamic_execute(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "DROP TABLE align_dynamic_values", [], [],
  ) else { return 29 }
  return 42
}

fn main(args: array<str>) -> Result<(), Error> {
  print(run(args[1]))
  return Ok(())
}
"#;

const DYNAMIC_COUNT_BOUNDARY_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_prepare_calls() -> i32
}

fn main() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  connection := pkg.db.sqlite.connect(":stub:", []) else { return 1 }
  arena values_out {
    mut builder: array_builder<pkg.db.value> := array_builder(values_out)
    mut i := 0
    loop {
      if i >= 65536 { break }
      builder.push(pkg.db.value.Null)
      i = i + 1
    }
    values := builder.build()
    rejected := pkg.db.dynamic_execute(
      pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
      "UPDATE DYNAMIC_SQLITE_COMMAND", values[..], [],
    )
    rejected_ok := match rejected {
      Err(error) => match error {
        Unsupported(contract) => contract.item == "db.dynamic.parameters"
          && contract.message == "dynamic SQL supports at most 65535 parameters"
        _ => false
      }
      Ok(_) => false
    }
    if !rejected_ok || unsafe { align_sqlite_q4a_prepare_calls() } != 0 { return 2 }

    accepted_phase := pkg.db.dynamic_execute(
      pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
      "UPDATE DYNAMIC_SQLITE_COMMAND", values[0..65535], [],
    )
    reached_native_count := match accepted_phase {
      Err(error) => match error {
        InvalidQuery(contract) => contract.item == "db.dynamic.parameters"
          && contract.message == "dynamic SQL parameter count does not match the statement"
        _ => false
      }
      Ok(_) => false
    }
    if !reached_native_count || unsafe { align_sqlite_q4a_prepare_calls() } != 1 { return 3 }
  }
  return 42
}
"#;

const SQLITE_FAILURE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_set_row_fault(fault: i32)
  fn align_sqlite_q4a_fail_next_text()
  fn align_sqlite_q4a_fail_next_finalize()
  fn align_sqlite_q4a_finalize_calls() -> i32
  fn align_sqlite_q4a_busy_timeout_calls() -> i32
  fn align_sqlite_pool_close_calls() -> i32
  fn align_sqlite_q4a_protocol_ok() -> i32
}

fn values(borrow connection: pkg.db.conn, fault: i32) -> i32 {
  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Null, pkg.db.value.Bool(true), pkg.db.value.I16(-2),
    pkg.db.value.I32(16909060), pkg.db.value.I64(-2), pkg.db.value.F32(1.5),
    pkg.db.value.F64(2.5), pkg.db.value.Text("a\0b"),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
  ]
  unsafe { align_sqlite_q4a_set_row_fault(fault) }
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE", params[..], [],
  ) else { return 1 }
  arena out {
    result := pkg.db.dynamic_next(stream, out)
    rejected := match result {
      Err(error) => match error {
        Decode(contract) => contract.item == "db.dynamic.value"
      _ => false
      }
      Ok(_) => false
    }
    if !rejected { return 2 }
    repeated := pkg.db.dynamic_next(stream, out)
    repeated_ok := match repeated {
      Err(error) => match error {
        InvalidQuery(contract) => contract.item == "db.dynamic.rows.state"
        _ => false
      }
      Ok(_) => false
    }
    if !repeated_ok { return 3 }
  }
  if unsafe { align_sqlite_q4a_finalize_calls() } != 1 { return 4 }
  // Constructor + the failed advance: decode cleanup must not restore twice.
  return if unsafe { align_sqlite_q4a_busy_timeout_calls() } == 2 { 0 } else { 5 }
}

fn one_value_fault(fault: i32) -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  connection := pkg.db.sqlite.connect(":stub:", []) else { return 1 }
  return values(connection, fault)
}

fn bind_failure() -> i32 {
  unsafe {
    align_sqlite_q4a_reset()
    align_sqlite_q4a_fail_next_text()
  }
  connection := pkg.db.sqlite.connect(":stub:", []) else { return 1 }
  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Null, pkg.db.value.Bool(true), pkg.db.value.I16(-2),
    pkg.db.value.I32(16909060), pkg.db.value.I64(-2), pkg.db.value.F32(1.5),
    pkg.db.value.F64(2.5), pkg.db.value.Text("a\0b"),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
  ]
  result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE", params[..], [],
  )
  rejected := match result { Err(error) => match error { Connection(_) => true, _ => false }, Ok(_) => false }
  if !rejected || unsafe { align_sqlite_q4a_finalize_calls() } != 1 { return 2 }
  return 0
}

fn step_failure() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  connection := pkg.db.sqlite.connect(":stub:", []) else { return 1 }
  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Null, pkg.db.value.Bool(true), pkg.db.value.I16(-2),
    pkg.db.value.I32(16909060), pkg.db.value.I64(-2), pkg.db.value.F32(1.5),
    pkg.db.value.F64(2.5), pkg.db.value.Text("a\0b"),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
  ]
  unsafe { align_sqlite_q4a_set_row_fault(20) }
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE", params[..], [],
  ) else { return 2 }
  arena out {
    result := pkg.db.dynamic_next(stream, out)
    rejected := match result { Err(error) => match error { Connection(_) => true, _ => false }, Ok(_) => false }
    if !rejected { return 3 }
  }
  return 0
}

fn finalize_failure() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  connection := pkg.db.sqlite.connect(":stub:", []) else { return 1 }
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE_ZERO_ROWS", [], [],
  ) else { return 2 }
  unsafe { align_sqlite_q4a_fail_next_finalize() }
  arena out {
    result := pkg.db.dynamic_next(stream, out)
    cleanup := match result {
      Err(error) => match error {
        Unsupported(contract) => contract.item == "db.dynamic.rows.cleanup"
        _ => false
      }
      Ok(_) => false
    }
    if !cleanup { return 3 }
  }
  return if unsafe { align_sqlite_pool_close_calls() } == 1 { 0 } else { 4 }
}

fn early_drop_failure() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  connection := pkg.db.sqlite.connect(":stub:", []) else { return 1 }
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE_ZERO_ROWS", [], [],
  ) else { return 2 }
  unsafe { align_sqlite_q4a_fail_next_finalize() }
  return 0
}

fn mixed_rows() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  connection := pkg.db.sqlite.connect(":stub:", []) else { return 1 }
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE_MIXED", [], [],
  ) else { return 2 }
  arena out {
    first := pkg.db.dynamic_next(stream, out) else { return 3 }
    first_row := first else { return 4 }
    second := pkg.db.dynamic_next(stream, out) else { return 5 }
    second_row := second else { return 6 }
    match first_row.values[0] { I64(value) => if value != 7 { return 7 }, _ => { return 8 } }
    match second_row.values[0] { Text(value) => if value != "two" { return 9 }, _ => { return 10 } }
    match first_row.values[0] { I64(value) => if value != 7 { return 11 }, _ => { return 12 } }
    exhausted := pkg.db.dynamic_next(stream, out) else { return 13 }
    match exhausted { Some(_) => { return 14 } None => {} }
  }
  return 0
}

fn null_empty_blob() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  connection := pkg.db.sqlite.connect(":stub:", []) else { return 1 }
  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Null, pkg.db.value.Bool(true), pkg.db.value.I16(-2),
    pkg.db.value.I32(16909060), pkg.db.value.I64(-2), pkg.db.value.F32(1.5),
    pkg.db.value.F64(2.5), pkg.db.value.Text("a\0b"),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
  ]
  unsafe { align_sqlite_q4a_set_row_fault(17) }
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE", params[..], [],
  ) else { return 2 }
  arena out {
    first := pkg.db.dynamic_next(stream, out) else { return 3 }
    row := first else { return 4 }
    match row.values[8] {
      Bytes(value) => if value.bytes.len() != 0 { return 5 }
      _ => { return 6 }
    }
  }
  return 0
}

fn decode_beats_finalize_failure() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  connection := pkg.db.sqlite.connect(":stub:", []) else { return 1 }
  bytes := [0 as u8, 255 as u8]
  params := [
    pkg.db.value.Null, pkg.db.value.Bool(true), pkg.db.value.I16(-2),
    pkg.db.value.I32(16909060), pkg.db.value.I64(-2), pkg.db.value.F32(1.5),
    pkg.db.value.F64(2.5), pkg.db.value.Text("a\0b"),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes[..] }),
  ]
  unsafe { align_sqlite_q4a_set_row_fault(10) }
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT DYNAMIC_SQLITE", params[..], [],
  ) else { return 2 }
  unsafe { align_sqlite_q4a_fail_next_finalize() }
  arena out {
    result := pkg.db.dynamic_next(stream, out)
    first_error := match result {
      Err(error) => match error {
        Decode(contract) => contract.item == "db.dynamic.value"
        _ => false
      }
      Ok(_) => false
    }
    if !first_error { return 3 }
  }
  return if unsafe { align_sqlite_pool_close_calls() } == 1 { 0 } else { 4 }
}

fn main() -> i32 {
  faults := [10 as i32, 11 as i32, 12 as i32, 13 as i32, 14 as i32, 15 as i32, 16 as i32]
  mut i := 0
  loop {
    if i >= faults.len() { break }
    result := one_value_fault(faults[i])
    if result != 0 { return 10 + (i as i32) * 10 + result }
    i = i + 1
  }
  bind := bind_failure()
  if bind != 0 { return 90 + bind }
  step := step_failure()
  if step != 0 { return 100 + step }
  finalized := finalize_failure()
  if finalized != 0 { return 110 + finalized }
  early := early_drop_failure()
  if early != 0 { return 120 + early }
  if unsafe { align_sqlite_pool_close_calls() } != 1 { return 125 }
  mixed := mixed_rows()
  if mixed != 0 { return 130 + mixed }
  null_blob := null_empty_blob()
  if null_blob != 0 { return 150 + null_blob }
  first_error := decode_beats_finalize_failure()
  if first_error != 0 { return 160 + first_error }
  if unsafe { align_sqlite_q4a_protocol_ok() } != 1 { return 150 }
  return 42
}
"#;

const DYNAMIC_INPUT_LIFETIME_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import pkg.db.sqlite

extern "C" {
  fn align_pg_reset()
  fn align_pg_protocol_ok() -> i32
}

fn sqlite_input_lifetime() -> i32 {
  connection := pkg.db.sqlite.connect(":memory:", []) else { return 1 }
  mut text_owner := "before".clone()
  text_view: str := text_owner
  mut bytes_owner := [1 as u8, 2 as u8, 3 as u8]
  params := [
    pkg.db.value.Text(text_view),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes_owner[..] }),
  ]
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT ?1, ?2", params[..], [],
  ) else { return 2 }
  text_owner = "after".clone()
  bytes_owner[0] = 9
  arena out {
    first := pkg.db.dynamic_next(stream, out) else { return 3 }
    row := first else { return 4 }
    match row.values[0] { Text(value) => if value != "before" { return 5 }, _ => { return 6 } }
    match row.values[1] {
      Bytes(value) => if value.bytes.len() != 3 || value.bytes[0] != 1 { return 7 }
      _ => { return 8 }
    }
  }
  return if text_owner == "after" && bytes_owner[0] == 9 { 0 } else { 9 }
}

fn postgres_input_lifetime() -> i32 {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("postgresql://stub/dynamic", []) else { return 1 }
  mut text_owner := "before".clone()
  text_view: str := text_owner
  mut bytes_owner := [1 as u8, 2 as u8, 3 as u8]
  params := [
    pkg.db.value.Text(text_view),
    pkg.db.value.Bytes(pkg.db.byte_view { bytes: bytes_owner[..] }),
  ]
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.PostgreSQL,
    "SELECT DYNAMIC_ECHO", params[..], [],
  ) else { return 2 }
  text_owner = "after".clone()
  bytes_owner[0] = 9
  arena out {
    first := pkg.db.dynamic_next(stream, out) else { return 3 }
    row := first else { return 4 }
    match row.values[0] { Text(value) => if value != "before" { return 5 }, _ => { return 6 } }
    match row.values[1] {
      Bytes(value) => if value.bytes.len() != 3 || value.bytes[0] != 1 { return 7 }
      _ => { return 8 }
    }
  }
  if unsafe { align_pg_protocol_ok() } != 1 { return 9 }
  return if text_owner == "after" && bytes_owner[0] == 9 { 0 } else { 10 }
}

fn main() -> i32 {
  sqlite := sqlite_input_lifetime()
  if sqlite != 0 { return 10 + sqlite }
  postgres := postgres_input_lifetime()
  if postgres != 0 { return 30 + postgres }
  return 42
}
"#;

fn no_modules() -> Vec<(&'static str, &'static str)> {
    Vec::new()
}

fn malformed_modules() -> Vec<(&'static str, &'static str)> {
    vec![("pkg/db/a2_test.align", DYNAMIC_TEST_HELPER)]
}

const SQLITE_DYNAMIC: Case = Case {
    label: "pkg-db-a2-sqlite-dynamic",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: no_modules,
    main: SQLITE_DYNAMIC_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const SQLITE_DYNAMIC_WHOLE: Case = Case {
    label: "pkg-db-a2-sqlite-dynamic-whole",
    runner: RunnerKind::WholeProgram,
    // The whole-program link retains the root package's libpq dependency even though this case
    // executes SQLite.  The required PostgreSQL gate is the repository's existing proof that the
    // native closure is installed; ordinary hosts without libpq skip instead of failing at link.
    needs: Needs::LivePostgres,
    links: &[],
    counters: &[],
    modules: no_modules,
    main: SQLITE_DYNAMIC_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const POSTGRES_DYNAMIC: Case = Case {
    label: "pkg-db-a2-postgres-dynamic",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: no_modules,
    main: POSTGRES_DYNAMIC_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const SQLITE_STUB_DYNAMIC: Case = Case {
    label: "pkg-db-a2-sqlite-stub-dynamic",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q4A],
    counters: &[],
    modules: no_modules,
    main: SQLITE_STUB_DYNAMIC_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const DYNAMIC_VALIDATION: Case = Case {
    label: "pkg-db-a2-dynamic-validation-order",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q4A],
    counters: &[],
    modules: no_modules,
    main: DYNAMIC_VALIDATION_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const POSTGRES_STATUS: Case = Case {
    label: "pkg-db-a2-postgres-status-fail-close",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: no_modules,
    main: POSTGRES_STATUS_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const POSTGRES_RESULT_FAULT: Case = Case {
    label: "pkg-db-a2-postgres-result-faults",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: no_modules,
    main: POSTGRES_RESULT_FAULT_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const POSTGRES_INVARIANT: Case = Case {
    label: "pkg-db-a2-postgres-session-invariants",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: no_modules,
    main: POSTGRES_INVARIANT_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const POSTGRES_TIMEOUT: Case = Case {
    label: "pkg-db-a2-postgres-timeout",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: no_modules,
    main: POSTGRES_TIMEOUT_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const POSTGRES_MALFORMED_RESOURCE: Case = Case {
    label: "pkg-db-a2-postgres-malformed-resource",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: malformed_modules,
    main: POSTGRES_MALFORMED_RESOURCE_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const DYNAMIC_POOL: Case = Case {
    label: "pkg-db-a2-dynamic-pool-provenance",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q4A],
    counters: &[],
    modules: no_modules,
    main: DYNAMIC_POOL_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const DYNAMIC_COUNT_BOUNDARY: Case = Case {
    label: "pkg-db-a2-dynamic-count-boundary",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q4A],
    counters: &[],
    modules: no_modules,
    main: DYNAMIC_COUNT_BOUNDARY_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const SQLITE_FAILURE: Case = Case {
    label: "pkg-db-a2-sqlite-failure-matrix",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q4A],
    counters: &[],
    modules: no_modules,
    main: SQLITE_FAILURE_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const DYNAMIC_INPUT_LIFETIME: Case = Case {
    label: "pkg-db-a2-dynamic-input-lifetime",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: no_modules,
    main: DYNAMIC_INPUT_LIFETIME_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

#[test]
fn sqlite_dynamic_values_round_trip_one_row() {
    SQLITE_DYNAMIC.run();
}

#[test]
fn sqlite_dynamic_runtime_matches_the_whole_program_pipeline() {
    SQLITE_DYNAMIC_WHOLE.run();
}

#[test]
fn postgres_dynamic_binary_values_round_trip_and_release_before_final_row() {
    POSTGRES_DYNAMIC.run();
}

#[test]
fn sqlite_dynamic_transient_bind_drop_and_busy_timeout_cleanup_are_exact() {
    SQLITE_STUB_DYNAMIC.run();
}

#[test]
fn dynamic_validation_precedence_overlap_and_statement_shape_are_exact() {
    DYNAMIC_VALIDATION.run();
}

#[test]
fn postgres_copy_pipeline_and_unknown_statuses_close_without_later_native_calls() {
    POSTGRES_STATUS.run();
}

#[test]
fn postgres_dynamic_metadata_and_value_faults_fail_atomically() {
    POSTGRES_RESULT_FAULT.run();
}

#[test]
fn postgres_dynamic_session_and_transaction_invariants_fail_closed() {
    POSTGRES_INVARIANT.run();
}

#[test]
fn postgres_dynamic_timeout_recovers_connection_and_transaction_targets() {
    POSTGRES_TIMEOUT.run();
}

#[test]
fn dynamic_public_surface_is_exact_and_typechecks_across_frontends() {
    let source = package_source("pkg/db.align");
    for required in [
        "pub byte_view {\n  bytes: slice<u8>,\n}",
        "pub value {\n  Null\n  Bool(bool)\n  I16(i16)\n  I32(i32)\n  I64(i64)\n  F32(f32)\n  F64(f64)\n  Text(str)\n  Bytes(byte_view)\n}",
        "pub row {\n  values: array<value>,\n}",
        "pub resource dynamic_rows = pkg.db.internal.resource.drop_dynamic_rows",
        "pub fn dynamic_execute(",
        "pub fn dynamic_rows(",
        "pub fn dynamic_next(",
    ] {
        assert!(
            source.contains(required),
            "missing exact dynamic surface `{required}`"
        );
    }
    for forbidden in [
        "pub fn dynamic_prepare",
        "pub fn dynamic_close",
        "pub fn dynamic_materialize",
        "pub fn dynamic_column",
        "pub fn dynamic_name",
        "pub fn dynamic_rewind",
    ] {
        assert!(
            !source.contains(forbidden),
            "unexpected dynamic surface `{forbidden}`"
        );
    }

    let clean = Layout::new()
        .module("app/dynamic_user.align", DYNAMIC_PUBLIC_USER)
        .main(DYNAMIC_PUBLIC_MAIN);
    expect_checks_clean("pkg-db-a2-public-surface", &clean);
    expect_checks_clean(
        "pkg-db-a2-returned-row-borrow",
        &Layout::new()
            .module("app/dynamic_user.align", DYNAMIC_PUBLIC_USER)
            .main(DYNAMIC_ROW_BORROW_MAIN),
    );
    expect_checks_rejected(
        "pkg-db-a2-returned-row-value-transfer",
        &Layout::new()
            .module("app/dynamic_user.align", DYNAMIC_PUBLIC_USER)
            .main(DYNAMIC_ROW_VALUE_MAIN),
    );
    expect_checks_rejected(
        "pkg-db-a2-bare-slice-sum",
        &Layout::new().main(BARE_SLICE_SUM_MAIN),
    );
    expect_checks_rejected(
        "pkg-db-a2-region-escape",
        &Layout::new().main(REGION_ESCAPE_MAIN),
    );
    expect_checks_rejected(
        "pkg-db-a2-private-resource-import",
        &Layout::new().main(PRIVATE_RESOURCE_IMPORT_MAIN),
    );
}

#[test]
fn malformed_dynamic_resource_headers_never_follow_embedded_pointers() {
    POSTGRES_MALFORMED_RESOURCE.run();
}

#[test]
fn dynamic_streams_preserve_direct_transaction_and_pool_parent_provenance() {
    DYNAMIC_POOL.run();
}

#[test]
fn postgres_required_dynamic_values_use_real_libpq17() {
    if !common::backend_available() {
        return;
    }
    let layout = Layout::new().main(LIVE_POSTGRES_DYNAMIC_MAIN);
    expect_checks_clean("pkg-db-a2-live-postgres-typecheck", &layout);
    let Some(url) = db_harness::live_postgres_url("PostgreSQL A2 dynamic-SQL owner") else {
        return;
    };
    let output = common::build_and_run_multi_args_with_env(
        "pkg-db-a2-live-postgres",
        &layout.files(),
        "main.align",
        &[url.as_str()],
        &[],
    );
    assert!(
        output.status.success(),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "42\n",
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn dynamic_parameter_count_accepts_65535_and_rejects_65536_before_native_work() {
    DYNAMIC_COUNT_BOUNDARY.run();
}

#[test]
fn sqlite_dynamic_failpoints_and_per_row_storage_classes_are_exact() {
    SQLITE_FAILURE.run();
}

#[test]
fn dynamic_constructors_retain_no_text_or_bytes_input_provenance() {
    DYNAMIC_INPUT_LIFETIME.run();
}
