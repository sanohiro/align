//! pkg.db A1/D13 explicit fixed-capacity pool owners.

mod common;
mod db_harness;

use db_harness::{
    CounterExpect, Layout, Needs, PG, SQLITE_Q4A, expect_checks_clean, expect_checks_rejected,
    gate, package_source, run_per_unit_c, run_whole_program,
};

const SQLITE_POOL_MAIN: &str = r#"module main
import pkg.db
import pkg.db.pool

fn is_capacity(error: pkg.db.Error) -> bool = match error {
  Unsupported(value) => value.item == "db.pool.capacity"
  _ => false
}

fn checkout_all(borrow owner: pkg.db.pool.Pool) -> i32 {
  first := pkg.db.pool.try_acquire(owner) else { return 6 }
  second := pkg.db.pool.try_acquire(owner) else { return 7 }
  exhausted := pkg.db.pool.try_acquire(owner)
  match exhausted {
    Ok(_) => { return 8 }
    Err(error) => match error { PoolExhausted => {} _ => { return 9 } }
  }
  during := pkg.db.pool.info(owner) else { return 10 }
  if during.idle != 0 || during.checked_out != 2 { return 11 }
  return 0
}

fn main() -> i32 {
  invalid := pkg.db.pool.open_sqlite("bad\u{0000}path", 0, [])
  match invalid {
    Ok(_) => { return 1 }
    Err(error) => if !is_capacity(error) { return 2 }
  }

  opened := pkg.db.pool.open_sqlite(":memory:", 2, [])
  return match opened {
    Err(_) => 3
    Ok(owner) => {
      before := pkg.db.pool.info(owner) else { return 4 }
      sqlite_driver := match before.driver { SQLite => true PostgreSQL => false }
      if !sqlite_driver || before.capacity != 2 || before.idle != 2 || before.checked_out != 0 {
        return 5
      }
      checkout := checkout_all(owner)
      if checkout != 0 { return checkout }
      after := pkg.db.pool.info(owner) else { return 12 }
      if after.idle != 2 || after.checked_out != 0 { return 13 }
      return 42
    }
  }
}
"#;

const POOL_QUERY: &str = r#"module app.pool_query
import pkg.db
import pkg.db.sqlite
import pkg.db.postgres

pub Empty { ignored: i64 }
pub SqliteParams { value: i64 }
pub PgParams { first_value: i64, last_value: i64 }
pub ValueRow { value: i64 }

pub fn sqlite_values() -> pkg.db.query<SqliteParams, ValueRow> = pkg.db.sqlite.query(
  "SELECT CAST(:value AS INTEGER) AS value UNION ALL SELECT CAST(:value + 1 AS INTEGER) UNION ALL SELECT CAST(:value + 2 AS INTEGER)",
  [],
  [],
)

pub fn sqlite_cache_size() -> pkg.db.query<Empty, ValueRow> = pkg.db.sqlite.query(
  "SELECT cache_size AS value FROM pragma_cache_size WHERE :ignored = :ignored",
  [],
  [],
)

pub fn postgres_values() -> pkg.db.query<PgParams, ValueRow> = pkg.db.postgres.query(
  "SELECT value::bigint AS value FROM generate_series(:first_value, :last_value) AS value ORDER BY value",
  [],
  [
    pkg.db.postgres.QueryOption.ParameterType("first_value", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("last_value", "int8"),
  ],
)
"#;

const SQLITE_POOL_TESTKIT: &str = r#"module pkg.db.testkit.pool_sqlite
import pkg.db
import pkg.db.internal.resource

extern "C" link("sqlite3") {
  fn sqlite3_exec(database: raw, sql: raw, callback: raw, argument: raw, error_out: raw) -> i32
}

pub fn raw_begin(borrow connection: pkg.db.conn) -> i32 {
  unsafe {
    state := resource.raw(resource.borrow(connection))
    if !pkg.db.internal.resource.conn_live_for_driver(state, 1) { return -1 }
    database: raw := raw.load(state, 8)
    sql := raw.alloc(6)
    bytes := "BEGIN".bytes()
    mut i := 0
    loop {
      if i >= bytes.len() { break }
      raw.store(sql, i, bytes[i])
      i = i + 1
    }
    raw.store(sql, 5, 0 as u8)
    status := sqlite3_exec(database, sql, raw.null(), raw.null(), raw.null())
    raw.free(sql)
    return status
  }
}

pub fn set_cache_size(borrow connection: pkg.db.conn, value: i32) -> i32 {
  unsafe {
    state := resource.raw(resource.borrow(connection))
    if !pkg.db.internal.resource.conn_live_for_driver(state, 1) { return -1 }
    database: raw := raw.load(state, 8)
    text := if value == -111 { "PRAGMA cache_size = -111" } else {
      if value == -222 { "PRAGMA cache_size = -222" } else { return -2 }
    }
    bytes := text.bytes()
    sql := raw.alloc(bytes.len() + 1)
    mut i := 0
    loop {
      if i >= bytes.len() { break }
      raw.store(sql, i, bytes[i])
      i = i + 1
    }
    raw.store(sql, bytes.len(), 0 as u8)
    status := sqlite3_exec(database, sql, raw.null(), raw.null(), raw.null())
    raw.free(sql)
    return status
  }
}
"#;

const SQLITE_DEPENDENT_AND_LIFO_MAIN: &str = r#"module main
import pkg.db
import pkg.db.pool
import pkg.db.testkit.pool_sqlite
import app.pool_query

fn prepare_error_code(error: pkg.db.Error) -> i32 = match error {
  Connection(_) => 40
  Timeout(_) => 41
  Cancelled(_) => 42
  NotFound => 43
  PoolExhausted => 44
  Cardinality(_) => 45
  Constraint(_) => 46
  Serialization(_) => 47
  Deadlock(_) => 48
  SchemaMismatch(_) => 49
  DriverMismatch(_) => 50
  Decode(_) => 51
  Encode(_) => 52
  InvalidQuery(_) => 53
  Unsupported(_) => 54
  Native(_) => 55
}

fn prepared_batch(borrow connection: pkg.db.conn) -> i32 {
  prepared_result := pkg.db.prepare(
    pkg.db.exec_conn(connection), app.pool_query.sqlite_values(), [],
  )
  prepared := match prepared_result {
    Ok(value) => value
    Err(error) => { return prepare_error_code(error) }
  }
  mut statement := prepared
  mut stream := pkg.db.rows_stmt(
    statement, app.pool_query.SqliteParams { value: 7 }, [],
  ) else { return 2 }
  selected := pkg.db.next_batch(stream, 8) else { return 3 }
  values := selected else { return 4 }
  if (pkg.db.batch_len(values) else { return 5 }) != 3 { return 6 }
  first := pkg.db.batch_row(values, 0) else { return 7 }
  row := first else { return 8 }
  return if row.value == 7 { 0 } else { 9 }
}

fn inspect(borrow connection: pkg.db.conn) -> i32 {
  arena out {
    database := pkg.db.meta_database(
      pkg.db.exec_conn(connection), pkg.db.MetaDetail.Names, out, [],
    ) else { return 10 }
    if database.engine_version.len() == 0 { return 11 }
    plan := pkg.db.explain(
      pkg.db.exec_conn(connection),
      app.pool_query.sqlite_values(),
      app.pool_query.SqliteParams { value: 7 },
      out,
      [],
    ) else { return 12 }
    if plan.body.len() == 0 || plan.analyzed { return 13 }
    return 0
  }
}

fn use_and_release(borrow owner: pkg.db.pool.Pool) -> i32 {
  mut connection := pkg.db.pool.try_acquire(owner) else { return 14 }
  dependent := prepared_batch(connection)
  if dependent != 0 { return dependent }
  committing := pkg.db.begin(connection, []) else { return 15 }
  connection = pkg.db.commit(committing) else { return 16 }
  rolling_back := pkg.db.begin(connection, []) else { return 17 }
  connection = pkg.db.rollback(rolling_back) else { return 18 }
  return inspect(connection)
}

fn release_marked(connection: pkg.db.conn, value: i32) -> i32 {
  return pkg.db.testkit.pool_sqlite.set_cache_size(connection, value)
}

fn cache_size(borrow connection: pkg.db.conn) -> i64 {
  arena out {
    row := pkg.db.one(
      pkg.db.exec_conn(connection), app.pool_query.sqlite_cache_size(),
      app.pool_query.Empty { ignored: 1 }, out, [],
    ) else { return 0 }
    return row.value
  }
}

fn lifo(borrow owner: pkg.db.pool.Pool) -> i32 {
  first := pkg.db.pool.try_acquire(owner) else { return 20 }
  second := pkg.db.pool.try_acquire(owner) else { return 21 }
  if release_marked(first, -111) != 0 { return 22 }
  if release_marked(second, -222) != 0 { return 23 }
  top := pkg.db.pool.try_acquire(owner) else { return 24 }
  if cache_size(top) != -222 { return 25 }
  other := pkg.db.pool.try_acquire(owner) else { return 26 }
  if cache_size(other) != -111 { return 27 }
  state := pkg.db.pool.info(owner) else { return 28 }
  return if state.idle == 0 && state.checked_out == 2 { 0 } else { 29 }
}

fn main() -> i32 {
  owner := pkg.db.pool.open_sqlite(":memory:", 1, []) else { return 30 }
  used := use_and_release(owner)
  if used != 0 { return used }
  returned := pkg.db.pool.info(owner) else { return 31 }
  if returned.idle != 1 || returned.checked_out != 0 { return 32 }

  ordered := pkg.db.pool.open_sqlite(":memory:", 2, []) else { return 33 }
  order := lifo(ordered)
  if order != 0 { return order }
  return 42
}
"#;

const POSTGRES_LIVE_DEPENDENT_MAIN: &str = r#"module main
import pkg.db
import pkg.db.pool
import app.pool_query

fn prepared_batch(borrow connection: pkg.db.conn) -> i32 {
  prepared := pkg.db.prepare(
    pkg.db.exec_conn(connection), app.pool_query.postgres_values(), [],
  ) else { return 1 }
  mut statement := prepared
  mut stream := pkg.db.rows_stmt(
    statement, app.pool_query.PgParams { first_value: 7, last_value: 9 }, [],
  ) else { return 2 }
  selected := pkg.db.next_batch(stream, 8) else { return 3 }
  values := selected else { return 4 }
  if (pkg.db.batch_len(values) else { return 5 }) != 3 { return 6 }
  first := pkg.db.batch_row(values, 0) else { return 7 }
  row := first else { return 8 }
  return if row.value == 7 { 0 } else { 9 }
}

fn inspect(borrow connection: pkg.db.conn) -> i32 {
  arena out {
    database := pkg.db.meta_database(
      pkg.db.exec_conn(connection), pkg.db.MetaDetail.Names, out, [],
    ) else { return 10 }
    if database.engine_version.len() == 0 { return 11 }
    plan := pkg.db.explain(
      pkg.db.exec_conn(connection),
      app.pool_query.postgres_values(),
      app.pool_query.PgParams { first_value: 7, last_value: 9 },
      out,
      [],
    ) else { return 12 }
    if plan.body.len() == 0 || plan.analyzed { return 13 }
    return 0
  }
}

fn use_and_release(borrow owner: pkg.db.pool.Pool) -> i32 {
  mut connection := pkg.db.pool.try_acquire(owner) else { return 14 }
  committing := pkg.db.begin(connection, []) else { return 15 }
  connection = pkg.db.commit(committing) else { return 16 }
  rolling_back := pkg.db.begin(connection, []) else { return 17 }
  connection = pkg.db.rollback(rolling_back) else { return 18 }
  dependent := prepared_batch(connection)
  if dependent != 0 { return dependent }
  return inspect(connection)
}

fn run(url: str) -> i32 {
  owner := pkg.db.pool.open_postgres(url, 1, []) else { return 20 }
  used := use_and_release(owner)
  if used != 0 { return used }
  state := pkg.db.pool.info(owner) else { return 21 }
  return if state.idle == 1 && state.checked_out == 0 { 42 } else { 22 }
}

fn main(args: array<str>) -> Result<(), Error> {
  print(run(args[1]))
  return Ok(())
}
"#;

const SQLITE_RETIRE_AND_OUTLIVE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.pool
import pkg.db.testkit.pool_sqlite

fn leak_native_transaction(borrow owner: pkg.db.pool.Pool) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 1 }
  return if pkg.db.testkit.pool_sqlite.raw_begin(connection) == 0 { 0 } else { 2 }
}

fn detached() -> Result<pkg.db.conn, pkg.db.Error> {
  owner := pkg.db.pool.open_sqlite(":memory:", 2, [])?
  return pkg.db.pool.try_acquire(owner)
}

fn main() -> i32 {
  opened := pkg.db.pool.open_sqlite(":memory:", 1, [])
  retired := match opened {
    Err(_) => { return 3 }
    Ok(owner) => {
      result := leak_native_transaction(owner)
      if result != 0 { return result }
      state := pkg.db.pool.info(owner) else { return 4 }
      if state.capacity != 1 || state.idle != 0 || state.checked_out != 0 { return 5 }
      match pkg.db.pool.try_acquire(owner) {
        Ok(_) => { return 6 }
        Err(error) => match error { PoolExhausted => 0 _ => 7 }
      }
    }
  }
  if retired != 0 { return retired }

  mut connection := detached() else { return 8 }
  transaction := pkg.db.begin(connection, []) else { return 9 }
  connection = pkg.db.rollback(transaction) else { return 10 }
  return 42
}
"#;

const SQLITE_POOL_STUB_TESTKIT: &str = r#"module pkg.db.testkit.pool_stub
import pkg.db
import pkg.db.internal.resource

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_pool_fail_connect_at(ordinal: i32)
  fn align_sqlite_pool_rollback_fault(fault: i32)
  fn align_sqlite_pool_control_fault(fault: i32)
  fn align_sqlite_pool_connect_calls() -> i32
  fn align_sqlite_pool_close_calls() -> i32
  fn align_sqlite_pool_close_ordinal(index: i32) -> i32
  fn align_sqlite_pool_connection_ordinal(database: raw) -> i32
}

pub fn reset() { unsafe { align_sqlite_q4a_reset() } }
pub fn fail_connect_at(ordinal: i32) { unsafe { align_sqlite_pool_fail_connect_at(ordinal) } }
pub fn rollback_fault(fault: i32) { unsafe { align_sqlite_pool_rollback_fault(fault) } }
pub fn control_fault(fault: i32) { unsafe { align_sqlite_pool_control_fault(fault) } }
pub fn connect_calls() -> i32 { unsafe { return align_sqlite_pool_connect_calls() } }
pub fn close_calls() -> i32 { unsafe { return align_sqlite_pool_close_calls() } }
pub fn close_ordinal(index: i32) -> i32 {
  unsafe { return align_sqlite_pool_close_ordinal(index) }
}
pub fn ordinal(borrow connection: pkg.db.conn) -> i32 {
  unsafe {
    state := resource.raw(resource.borrow(connection))
    if !pkg.db.internal.resource.conn_live_for_driver(state, 1) { return -1 }
    database: raw := raw.load(state, 8)
    return align_sqlite_pool_connection_ordinal(database)
  }
}
"#;

const SQLITE_PARTIAL_FORMATION_MAIN: &str = r#"module main
import pkg.db
import pkg.db.pool
import pkg.db.testkit.pool_stub

fn is_open_error(error: pkg.db.Error) -> bool = match error {
  Unsupported(value) => value.item == "sqlite.connect.open"
  _ => false
}

fn run_ordinal(ordinal: i32) -> i32 {
  pkg.db.testkit.pool_stub.reset()
  pkg.db.testkit.pool_stub.fail_connect_at(ordinal)
  opened := pkg.db.pool.open_sqlite(":memory:", 3, [])
  match opened {
    Ok(_) => { return 1 }
    Err(error) => if !is_open_error(error) { return 2 }
  }
  if pkg.db.testkit.pool_stub.connect_calls() != ordinal { return 3 }
  if pkg.db.testkit.pool_stub.close_calls() != ordinal - 1 { return 4 }
  mut close_index: i32 := 0
  loop {
    if close_index >= ordinal - 1 { break }
    if pkg.db.testkit.pool_stub.close_ordinal(close_index) != ordinal - 1 - close_index {
      return 20 + close_index
    }
    close_index = close_index + 1
  }
  return 0
}

fn invalid_capacities() -> i32 {
  values := [
    0 as i64,
    -1 as i64,
    1025 as i64,
    9223372036854775807 as i64,
    (-9223372036854775807 as i64) - 1,
  ]
  mut i := 0
  loop {
    if i >= values.len() { break }
    pkg.db.testkit.pool_stub.reset()
    opened := pkg.db.pool.open_sqlite("bad\u{0000}path", values[i], [])
    match opened {
      Ok(_) => { return 5 }
      Err(error) => match error {
        Unsupported(value) => if value.item != "db.pool.capacity" { return 6 }
        _ => { return 7 }
      }
    }
    if pkg.db.testkit.pool_stub.connect_calls() != 0 { return 8 }
    i = i + 1
  }
  return 0
}

fn form_maximum() -> i32 {
  return match pkg.db.pool.open_sqlite(":memory:", 1024, []) {
    Err(_) => { return 9 }
    Ok(owner) => {
      state := pkg.db.pool.info(owner) else { return 10 }
      if state.capacity == 1024 && state.idle == 1024 && state.checked_out == 0 { 0 } else { 11 }
    }
  }
}

fn maximum_capacity() -> i32 {
  pkg.db.testkit.pool_stub.reset()
  formed := form_maximum()
  if formed != 0 { return formed }
  if pkg.db.testkit.pool_stub.connect_calls() != 1024
    || pkg.db.testkit.pool_stub.close_calls() != 1024 {
    return 12
  }
  return 0
}

fn release(connection: pkg.db.conn) {}

fn initial_and_returned_lifo() -> i32 {
  pkg.db.testkit.pool_stub.reset()
  owner := pkg.db.pool.open_sqlite(":memory:", 3, []) else { return 13 }
  first := pkg.db.pool.try_acquire(owner) else { return 14 }
  second := pkg.db.pool.try_acquire(owner) else { return 15 }
  if pkg.db.testkit.pool_stub.ordinal(first) != 3
    || pkg.db.testkit.pool_stub.ordinal(second) != 2 {
    return 16
  }
  release(second)
  release(first)
  again := pkg.db.pool.try_acquire(owner) else { return 18 }
  return if pkg.db.testkit.pool_stub.ordinal(again) == 3 { 0 } else { 19 }
}

fn main() -> i32 {
  invalid := invalid_capacities()
  if invalid != 0 { return invalid }
  maximum := maximum_capacity()
  if maximum != 0 { return maximum }
  lifo := initial_and_returned_lifo()
  if lifo != 0 { return lifo }
  mut ordinal: i32 := 1
  loop {
    if ordinal > 3 { break }
    result := run_ordinal(ordinal)
    if result != 0 { return (ordinal * 10) + result }
    ordinal = ordinal + 1
  }
  return 42
}
"#;

const SQLITE_ROLLBACK_FAILURE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.pool
import pkg.db.testkit.pool_stub

fn fail_implicit_drop(borrow owner: pkg.db.pool.Pool, fault: i32) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 1 }
  transaction := pkg.db.begin(connection, []) else { return 2 }
  pkg.db.testkit.pool_stub.rollback_fault(fault)
  return 0
}

fn failed_commit(borrow owner: pkg.db.pool.Pool) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 3 }
  transaction := pkg.db.begin(connection, []) else { return 4 }
  pkg.db.testkit.pool_stub.control_fault(1)
  match pkg.db.commit(transaction) { Ok(_) => { return 5 } Err(_) => {} }
  state := pkg.db.pool.info(owner) else { return 6 }
  return if state.idle == 1 && state.checked_out == 0 { 0 } else { 7 }
}

fn failed_rollback(borrow owner: pkg.db.pool.Pool) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 8 }
  transaction := pkg.db.begin(connection, []) else { return 9 }
  pkg.db.testkit.pool_stub.control_fault(2)
  match pkg.db.rollback(transaction) { Ok(_) => { return 10 } Err(_) => {} }
  state := pkg.db.pool.info(owner) else { return 11 }
  return if state.idle == 1 && state.checked_out == 0 { 0 } else { 12 }
}

fn main() -> i32 {
  pkg.db.testkit.pool_stub.reset()
  mut fault: i32 := 1
  loop {
    if fault > 2 { break }
    owner := pkg.db.pool.open_sqlite(":memory:", 1, []) else { return 10 + fault }
    failed := fail_implicit_drop(owner, fault)
    if failed != 0 { return 20 + failed }
    state := pkg.db.pool.info(owner) else { return 30 + fault }
    if state.capacity != 1 || state.idle != 0 || state.checked_out != 0 {
      return 40 + fault
    }
    match pkg.db.pool.try_acquire(owner) {
      Ok(_) => { return 50 + fault }
      Err(error) => match error { PoolExhausted => {} _ => { return 60 + fault } }
    }
    if pkg.db.testkit.pool_stub.connect_calls() != fault
      || pkg.db.testkit.pool_stub.close_calls() != fault {
      return 70 + fault
    }
    fault = fault + 1
  }
  owner := pkg.db.pool.open_sqlite(":memory:", 1, []) else { return 90 }
  commit_failure := failed_commit(owner)
  if commit_failure != 0 { return 90 + commit_failure }
  rollback_failure := failed_rollback(owner)
  if rollback_failure != 0 { return 110 + rollback_failure }
  return 42
}
"#;

const POSTGRES_POOL_TESTKIT: &str = r#"module pkg.db.testkit.pool_pg

extern "C" {
  fn align_pg_connect_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_finish_ordinal(index: i32) -> i32
  fn align_pg_pool_rollback_fault(fault: i32)
  fn align_pg_fail_next_control()
  fn align_pg_rollback_next_commit()
}

pub fn connect_calls() -> i32 { unsafe { return align_pg_connect_calls() } }
pub fn finish_calls() -> i32 { unsafe { return align_pg_finish_calls() } }
pub fn finish_ordinal(index: i32) -> i32 { unsafe { return align_pg_finish_ordinal(index) } }
pub fn rollback_fault(fault: i32) { unsafe { align_pg_pool_rollback_fault(fault) } }
pub fn fail_next_control() { unsafe { align_pg_fail_next_control() } }
pub fn rollback_next_commit() { unsafe { align_pg_rollback_next_commit() } }
"#;

const POSTGRES_POOL_MAIN: &str = r#"module main
import pkg.db
import pkg.db.pool
import pkg.db.postgres
import pkg.db.testkit.pg
import pkg.db.testkit.pool_pg

DetachedPair {
  first: pkg.db.conn,
  second: pkg.db.conn,
}

fn is_contract_item(error: pkg.db.Error, expected: str) -> bool = match error {
  Unsupported(value) => value.item == expected
  _ => false
}

fn invalid_capacities() -> i32 {
  bad_option := pkg.db.postgres.ConnectOption.ApplicationName("bad\u{0000}name")
  bad_options := [bad_option]
  values := [
    0 as i64,
    -1 as i64,
    1025 as i64,
    9223372036854775807 as i64,
    (-9223372036854775807 as i64) - 1,
  ]
  mut i := 0
  loop {
    if i >= values.len() { break }
    opened := pkg.db.pool.open_postgres(
      "bad\u{0000}url",
      values[i],
      bad_options[..],
    )
    match opened {
      Ok(_) => { return 20 }
      Err(error) => if !is_contract_item(error, "db.pool.capacity") { return 21 }
    }
    i = i + 1
  }
  return 0
}

fn partial_formation_cleanup() -> i32 {
  mut ordinal: i32 := 1
  loop {
    if ordinal > 3 { break }
    before_connect := pkg.db.testkit.pool_pg.connect_calls()
    before_finish := pkg.db.testkit.pool_pg.finish_calls()
    pkg.db.testkit.pg.fail_connect_at(before_connect + ordinal)
    opened := pkg.db.pool.open_postgres("postgresql://stub/pool", 3, [])
    pkg.db.testkit.pg.fail_connect_at(0)
    match opened {
      Ok(_) => { return 22 }
      Err(error) => if !is_contract_item(error, "postgres.connect.open") { return 23 }
    }
    if pkg.db.testkit.pool_pg.connect_calls() != before_connect + ordinal
      || pkg.db.testkit.pool_pg.finish_calls() != before_finish + ordinal - 1 {
      return 24
    }
    mut close_index: i32 := 0
    loop {
      if close_index >= ordinal - 1 { break }
      expected := before_connect + ordinal - 1 - close_index
      if pkg.db.testkit.pool_pg.finish_ordinal(before_finish + close_index) != expected {
        return 25
      }
      close_index = close_index + 1
    }
    ordinal = ordinal + 1
  }
  pkg.db.testkit.pg.fail_connect_at(0)
  return 0
}

fn maximum_capacity() -> i32 {
  opened := pkg.db.pool.open_postgres("postgresql://stub/pool", 1024, [])
  return match opened {
    Err(_) => 24
    Ok(owner) => {
      state := pkg.db.pool.info(owner) else { return 25 }
      if state.capacity == 1024 && state.idle == 1024 && state.checked_out == 0 { 0 } else { 26 }
    }
  }
}

fn implicit_rollback(borrow owner: pkg.db.pool.Pool) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 1 }
  transaction := pkg.db.begin(connection, []) else { return 2 }
  return 0
}

fn fail_implicit_drop(borrow owner: pkg.db.pool.Pool, fault: i32) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 9 }
  transaction := pkg.db.begin(connection, []) else { return 10 }
  pkg.db.testkit.pool_pg.rollback_fault(fault)
  return 0
}

fn rollback_failure_matrix() -> i32 {
  mut fault: i32 := 1
  loop {
    if fault > 3 { break }
    owner := pkg.db.pool.open_postgres("postgresql://stub/pool", 1, []) else { return 11 }
    failed := fail_implicit_drop(owner, fault)
    if failed != 0 { return failed }
    state := pkg.db.pool.info(owner) else { return 12 }
    if state.capacity != 1 || state.idle != 0 || state.checked_out != 0 { return 13 }
    match pkg.db.pool.try_acquire(owner) {
      Ok(_) => { return 14 }
      Err(error) => match error { PoolExhausted => {} _ => { return 15 } }
    }
    fault = fault + 1
  }
  return 0
}

fn failed_commit(borrow owner: pkg.db.pool.Pool, tag_mismatch: bool) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 16 }
  transaction := pkg.db.begin(connection, []) else { return 17 }
  if tag_mismatch {
    pkg.db.testkit.pool_pg.rollback_next_commit()
  } else {
    pkg.db.testkit.pool_pg.fail_next_control()
  }
  match pkg.db.commit(transaction) { Ok(_) => { return 18 } Err(_) => {} }
  state := pkg.db.pool.info(owner) else { return 19 }
  return if state.idle == 1 && state.checked_out == 0 { 0 } else { 20 }
}

fn failed_rollback(borrow owner: pkg.db.pool.Pool) -> i32 {
  connection := pkg.db.pool.try_acquire(owner) else { return 21 }
  transaction := pkg.db.begin(connection, []) else { return 22 }
  pkg.db.testkit.pool_pg.fail_next_control()
  match pkg.db.rollback(transaction) { Ok(_) => { return 23 } Err(_) => {} }
  state := pkg.db.pool.info(owner) else { return 24 }
  return if state.idle == 1 && state.checked_out == 0 { 0 } else { 25 }
}

fn explicit_end_failures() -> i32 {
  owner := pkg.db.pool.open_postgres("postgresql://stub/pool", 1, []) else { return 26 }
  mut status := failed_commit(owner, false)
  if status != 0 { return status }
  status = failed_commit(owner, true)
  if status != 0 { return status }
  return failed_rollback(owner)
}

fn detached() -> Result<pkg.db.conn, pkg.db.Error> {
  owner := pkg.db.pool.open_postgres("postgresql://stub/pool", 1, [])?
  return pkg.db.pool.try_acquire(owner)
}

fn detached_pair() -> Result<DetachedPair, pkg.db.Error> {
  owner := pkg.db.pool.open_postgres("postgresql://stub/pool", 2, [])?
  first := pkg.db.pool.try_acquire(owner)?
  second := pkg.db.pool.try_acquire(owner)?
  return Ok(DetachedPair { first: first, second: second })
}

fn detached_mixed() -> Result<pkg.db.conn, pkg.db.Error> {
  owner := pkg.db.pool.open_postgres("postgresql://stub/pool", 2, [])?
  return pkg.db.pool.try_acquire(owner)
}

fn drop_detached_pair() -> i32 {
  owners := detached_pair() else { return 16 }
  first_driver := pkg.db.driver_tag(pkg.db.exec_conn(owners.first))
  second_driver := pkg.db.driver_tag(pkg.db.exec_conn(owners.second))
  return if first_driver == 2 && second_driver == 2 { 0 } else { 17 }
}

fn drop_detached_mixed() -> i32 {
  connection := detached_mixed() else { return 27 }
  return if pkg.db.driver_tag(pkg.db.exec_conn(connection)) == 2 { 0 } else { 28 }
}

fn run() -> i32 {
  invalid := invalid_capacities()
  if invalid != 0 { return invalid }
  partial := partial_formation_cleanup()
  if partial != 0 { return partial }
  maximum := maximum_capacity()
  if maximum != 0 { return maximum }
  failures := rollback_failure_matrix()
  if failures != 0 { return failures }
  explicit_failures := explicit_end_failures()
  if explicit_failures != 0 { return explicit_failures }
  opened := pkg.db.pool.open_postgres("postgresql://stub/pool", 1, [])
  result := match opened {
    Err(_) => 3
    Ok(owner) => {
      rolled_back := implicit_rollback(owner)
      if rolled_back != 0 { return rolled_back }
      state := pkg.db.pool.info(owner) else { return 4 }
      if state.idle != 1 || state.checked_out != 0 { return 5 }
      0
    }
  }
  if result != 0 { return result }
  pair := drop_detached_pair()
  if pair != 0 { return pair }
  mixed := drop_detached_mixed()
  if mixed != 0 { return mixed }
  mut connection := detached() else { return 6 }
  transaction := pkg.db.begin(connection, []) else { return 7 }
  connection = pkg.db.rollback(transaction) else { return 8 }
  return 0
}

fn main() -> i32 {
  pkg.db.testkit.pg.reset()
  result := run()
  if result != 0 { return result }
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const EXHAUSTIVE_ERROR_MAIN: &str = r#"module main
import pkg.db
import pkg.db.pool

fn classify(error: pkg.db.Error) -> i32 = match error {
  Connection(_) => 1
  Timeout(_) => 2
  Cancelled(_) => 3
  NotFound => 4
  PoolExhausted => 5
  Cardinality(_) => 6
  Constraint(_) => 7
  Serialization(_) => 8
  Deadlock(_) => 9
  SchemaMismatch(_) => 10
  DriverMismatch(_) => 11
  Decode(_) => 12
  Encode(_) => 13
  InvalidQuery(_) => 14
  Unsupported(_) => 15
  Native(_) => 16
}

fn main() -> i32 = classify(pkg.db.Error.PoolExhausted)
"#;

const POOL_STATE_TESTKIT: &str = r#"module pkg.db.pool.testkit.state
import pkg.db
import pkg.db.pool
import pkg.db.internal.resource

pub fn malformed(field: i32) -> pkg.db.pool.Pool {
  unsafe {
    state := pkg.db.internal.resource.new_pool_state(1, 2)
    if field == 0 { raw.store(state, 0, 0 as u32) }
    if field == 1 { raw.store(state, 4, 3 as u8) }
    if field == 2 { raw.store(state, 5, 2 as u8) }
    if field == 3 { raw.store(state, 6, 1 as u16) }
    if field == 4 { raw.store(state, 8, 0 as i64) }
    if field == 5 { raw.store(state, 16, -1 as i64) }
    if field == 6 { raw.store(state, 24, 3 as i64) }
    if field == 7 {
      slots: raw := raw.load(state, 32)
      raw.free(slots)
      raw.store(state, 32, raw.null())
    }
    if field == 8 { raw.store(state, 40, 1 as u64) }
    owner: pkg.db.pool.Pool := resource.from_raw(state)
    return owner
  }
}

pub fn legacy_conn_rejected() -> bool {
  unsafe {
    state := raw.alloc(40)
    raw.store(state, 0, 1 as u32)
    raw.store(state, 4, 1 as u8)
    raw.store(state, 5, 1 as u8)
    raw.store(state, 6, 0 as u16)
    raw.store(state, 8, raw.null())
    raw.store(state, 16, 0 as i32)
    raw.store(state, 20, 0 as u8)
    raw.store(state, 21, 0 as u8)
    raw.store(state, 22, 0 as u16)
    raw.store(state, 24, 0 as i64)
    raw.store(state, 32, raw.null())
    accepted := pkg.db.internal.resource.conn_state_valid(state)
    raw.free(state)
    return !accepted
  }
}

pub fn conn_v2_layout_exact() -> bool {
  unsafe {
    native := raw.alloc(8)
    state := pkg.db.internal.resource.new_conn_state(1, native, 17)
    version: u32 := raw.load(state, 0)
    driver: u8 := raw.load(state, 4)
    closed: u8 := raw.load(state, 5)
    reserved: u16 := raw.load(state, 6)
    stored_native: raw := raw.load(state, 8)
    busy_ms: i32 := raw.load(state, 16)
    lease: u8 := raw.load(state, 20)
    transaction: u8 := raw.load(state, 21)
    tail_reserved: u16 := raw.load(state, 22)
    next_statement: i64 := raw.load(state, 24)
    origin: raw := raw.load(state, 32)
    exact := version == 2
      && driver == 1
      && closed == 0
      && reserved == 0
      && !stored_native.is_null()
      && busy_ms == 17
      && lease == 0
      && transaction == 0
      && tail_reserved == 0
      && next_statement == 0
      && origin.is_null()
      && pkg.db.internal.resource.conn_state_valid(state)
    raw.free(state)
    raw.free(native)
    return exact
  }
}

pub fn conn_field_rejected(field: i32) -> bool {
  unsafe {
    native := raw.alloc(8)
    state := pkg.db.internal.resource.new_conn_state(1, native, 0)
    if field == 0 { raw.store(state, 0, 1 as u32) }
    if field == 1 { raw.store(state, 4, 3 as u8) }
    if field == 2 { raw.store(state, 5, 2 as u8) }
    if field == 3 { raw.store(state, 6, 1 as u16) }
    if field == 4 { raw.store(state, 8, raw.null()) }
    if field == 5 { raw.store(state, 16, -1 as i32) }
    if field == 6 {
      raw.store(state, 4, 2 as u8)
      raw.store(state, 16, 1 as i32)
    }
    if field == 7 { raw.store(state, 20, 2 as u8) }
    if field == 8 { raw.store(state, 21, 2 as u8) }
    if field == 9 { raw.store(state, 22, 1 as u16) }
    if field == 10 { raw.store(state, 24, -1 as i64) }
    if field == 11 { raw.store(state, 5, 1 as u8) }
    accepted := pkg.db.internal.resource.conn_state_valid(state)
    raw.free(state)
    raw.free(native)
    return !accepted
  }
}

pub fn pool_v1_layout_exact() -> bool {
  unsafe {
    state := pkg.db.internal.resource.new_pool_state(2, 2)
    version: u32 := raw.load(state, 0)
    driver: u8 := raw.load(state, 4)
    lifecycle: u8 := raw.load(state, 5)
    reserved: u16 := raw.load(state, 6)
    capacity: i64 := raw.load(state, 8)
    idle: i64 := raw.load(state, 16)
    checked_out: i64 := raw.load(state, 24)
    slots: raw := raw.load(state, 32)
    tail_reserved: u64 := raw.load(state, 40)
    first: raw := raw.load(slots, 0)
    second: raw := raw.load(slots, 8)
    exact := version == 1
      && driver == 2
      && lifecycle == 0
      && reserved == 0
      && capacity == 2
      && idle == 0
      && checked_out == 0
      && !slots.is_null()
      && first.is_null()
      && second.is_null()
      && tail_reserved == 0
      && pkg.db.internal.resource.pool_open_valid(state)
    pkg.db.internal.resource.drop_pool_state(state)
    return exact
  }
}

pub fn origin_attached(
  borrow owner: pkg.db.pool.Pool,
  borrow connection: pkg.db.conn,
) -> bool {
  unsafe {
    pool_state := resource.raw(resource.borrow(owner))
    connection_state := resource.raw(resource.borrow(connection))
    origin: raw := raw.load(connection_state, 32)
    return !origin.is_null()
      && pkg.db.internal.resource.pool_idle(pool_state) == 0
      && pkg.db.internal.resource.pool_checked_out(pool_state) == 1
  }
}
"#;

const MALFORMED_POOL_MAIN: &str = r#"module main
import pkg.db
import pkg.db.pool
import pkg.db.pool.testkit.state

fn is_state_error(error: pkg.db.Error) -> bool = match error {
  Unsupported(value) => value.item == "db.pool.state"
  _ => false
}

fn check_field(field: i32) -> bool {
  owner := pkg.db.pool.testkit.state.malformed(field)
  return match pkg.db.pool.info(owner) {
    Ok(_) => false
    Err(error) => is_state_error(error)
  }
}

fn check_origin() -> i32 {
  owner := pkg.db.pool.open_sqlite(":memory:", 1, []) else { return 1 }
  connection := pkg.db.pool.try_acquire(owner) else { return 2 }
  if !pkg.db.pool.testkit.state.origin_attached(owner, connection) { return 3 }
  return 0
}

fn main() -> i32 {
  mut field: i32 := 0
  loop {
    if field > 8 { break }
    if !check_field(field) { return 10 + field }
    field = field + 1
  }
  if !pkg.db.pool.testkit.state.legacy_conn_rejected() { return 30 }
  if !pkg.db.pool.testkit.state.conn_v2_layout_exact() { return 31 }
  mut conn_field: i32 := 0
  loop {
    if conn_field > 11 { break }
    if !pkg.db.pool.testkit.state.conn_field_rejected(conn_field) {
      return 50 + conn_field
    }
    conn_field = conn_field + 1
  }
  if !pkg.db.pool.testkit.state.pool_v1_layout_exact() { return 70 }
  origin := check_origin()
  if origin != 0 { return 80 + origin }
  return 42
}
"#;

#[test]
fn public_surface_is_exact() {
    let pool = package_source("pkg/db/pool.align");
    for required in [
        "pub MAX_CAPACITY: i64 := 1024",
        "pub resource Pool = pkg.db.pool.internal.resource.drop_pool",
        "pub Info {",
        "pub fn open_sqlite(",
        "pub fn open_postgres(",
        "pub fn try_acquire(",
        "pub fn info(",
    ] {
        assert!(pool.contains(required), "missing pool surface `{required}`");
    }
    for absent in [
        "pub fn acquire(",
        "pub fn close(",
        "pub fn release(",
        "pub fn adopt(",
    ] {
        assert!(!pool.contains(absent), "unexpected pool surface `{absent}`");
    }
    assert!(package_source("pkg/db.align").contains("  PoolExhausted\n"));
    let resource = package_source("pkg/db/internal/resource.align");
    for required in [
        "state := raw.alloc(40)",
        "raw.store(state, 32, raw.null())",
        "slots := raw.alloc(capacity * 8)",
        "pool := raw.alloc(48)",
        "raw.store(pool, 40, 0 as u64)",
    ] {
        assert!(
            resource.contains(required),
            "missing private ABI construction `{required}`"
        );
    }
}

#[test]
fn pool_error_exhaustive_match_typechecks_whole_and_per_unit() {
    let layout = Layout::new().main(EXHAUSTIVE_ERROR_MAIN);
    expect_checks_clean("pkg_db_pool_error_exhaustive", &layout);
}

#[test]
fn application_cannot_import_pool_state_transition_helpers() {
    let layout = Layout::new().main(
        r#"module main
import pkg.db.internal.resource
fn main() -> i32 {
  unsafe {
    return if pkg.db.internal.resource.pool_open_valid(raw.null()) { 1 } else { 0 }
  }
}
"#,
    );
    expect_checks_rejected("pkg_db_pool_internal_bypass", &layout);
}

#[test]
fn malformed_pool_fields_fail_before_access_and_conn_v1_is_rejected() {
    if gate(Needs::Backend).is_none() {
        return;
    }
    let layout = Layout::new()
        .module("pkg/db/pool/testkit/state.align", POOL_STATE_TESTKIT)
        .main(MALFORMED_POOL_MAIN);
    run_whole_program("pkg_db_pool_malformed", &layout, &[]).expect_exit(42);
}

#[test]
fn sqlite_pool_capacity_exhaustion_and_return_are_end_to_end() {
    if gate(Needs::Backend).is_none() {
        return;
    }
    let layout = Layout::new().main(SQLITE_POOL_MAIN);
    run_whole_program("pkg_db_pool_sqlite", &layout, &[]).expect_exit(42);
}

#[test]
fn sqlite_native_transaction_leak_retires_and_checkout_outlives_pool() {
    if gate(Needs::Backend).is_none() {
        return;
    }
    let layout = Layout::new()
        .module("pkg/db/testkit/pool_sqlite.align", SQLITE_POOL_TESTKIT)
        .main(SQLITE_RETIRE_AND_OUTLIVE_MAIN);
    run_whole_program("pkg_db_pool_sqlite_retire", &layout, &[]).expect_exit(42);
}

#[test]
fn sqlite_pooled_dependents_transactions_metadata_explain_and_lifo_are_end_to_end() {
    if gate(Needs::Backend).is_none() {
        return;
    }
    let layout = Layout::new()
        .module("pkg/db/testkit/pool_sqlite.align", SQLITE_POOL_TESTKIT)
        .module("app/pool_query.align", POOL_QUERY)
        .main(SQLITE_DEPENDENT_AND_LIFO_MAIN);
    db_harness::run_static_descriptors("pkg_db_pool_sqlite_dependent", &layout).expect_exit(42);
}

#[test]
fn sqlite_partial_formation_closes_every_prior_ordinal_once() {
    if gate(Needs::BackendAndCc).is_none() {
        return;
    }
    let layout = Layout::new()
        .linking(&PG)
        .linking(&SQLITE_Q4A)
        .module("pkg/db/testkit/pool_stub.align", SQLITE_POOL_STUB_TESTKIT)
        .main(SQLITE_PARTIAL_FORMATION_MAIN);
    run_per_unit_c("pkg_db_pool_sqlite_partial", &layout).expect_exit(42);
}

#[test]
fn sqlite_failed_rollback_or_idle_proof_retires_the_slot() {
    if gate(Needs::BackendAndCc).is_none() {
        return;
    }
    let layout = Layout::new()
        .linking(&PG)
        .linking(&SQLITE_Q4A)
        .module("pkg/db/testkit/pool_stub.align", SQLITE_POOL_STUB_TESTKIT)
        .main(SQLITE_ROLLBACK_FAILURE_MAIN);
    run_per_unit_c("pkg_db_pool_sqlite_rollback_failure", &layout).expect_exit(42);
}

#[test]
fn postgres_tx_drop_returns_only_after_rollback_and_checkout_outlives_pool() {
    if gate(Needs::BackendAndCc).is_none() {
        return;
    }
    let layout = Layout::new()
        .with_counters(&PG)
        .module("pkg/db/testkit/pool_pg.align", POSTGRES_POOL_TESTKIT)
        .main(POSTGRES_POOL_MAIN);
    let run = run_per_unit_c("pkg_db_pool_postgres", &layout);
    run.expect_exit(42);
    CounterExpect::new()
        .eq("pg.protocol_ok", 1)
        .eq("pg.connect_calls", 1040)
        .eq("pg.control_calls", 19)
        .eq("pg.finish_calls", 1037)
        .assert(&run);
}

#[test]
fn postgres_required_pool_runs_dependents_transactions_metadata_and_explain() {
    if gate(Needs::Backend).is_none() {
        return;
    }
    let layout = Layout::new()
        .module("app/pool_query.align", POOL_QUERY)
        .main(POSTGRES_LIVE_DEPENDENT_MAIN);
    let Some(url) = db_harness::live_postgres_url("PostgreSQL explicit-pool owner") else {
        return;
    };
    let output = common::build_and_run_multi_with_static_descriptors_args_with_env(
        "pkg-db-pool-live-postgres",
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
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
}
