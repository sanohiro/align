//! pkg.db Q5b2/D12 native catalog and EXPLAIN owners.

mod common;
use common::*;
mod db_harness;
use db_harness::package_source;


const SQLITE_SHAPE_STUB: &str = include_str!("fixtures/pkg_db_q5b2_sqlite_stub.c");

const SQLITE_SHAPE_MODULE: &str = r#"module pkg.db.shape_fixture
import pkg.db
import pkg.db.internal.resource

extern "C" {
  fn align_q5b2_fake_sqlite() -> raw
  fn align_q5b2_finalize_calls() -> i32
  fn align_q5b2_fake_postgres() -> raw
  fn align_q5b2_clear_calls() -> i32
}

fn sqlite_shape_status() -> i32 {
  unsafe {
    state := pkg.db.internal.resource.new_conn_state(
      1, align_q5b2_fake_sqlite(), 0,
    )
    connection: pkg.db.conn := resource.from_raw(state)
    target := pkg.db.exec_conn(connection)
    arena out {
      malformed := pkg.db.meta_schemas(target, pkg.db.MetaDetail.Names, out, [])
      malformed_ok := match malformed {
        Err(error) => match error {
          Decode(contract) => contract.item == "metadata.schemas"
          _ => false
        }
        Ok(_) => false
      }
      after := pkg.db.meta_schemas(target, pkg.db.MetaDetail.Names, out, [])
      after_ok := match after { Ok(rows) => rows.len() == 0 Err(_) => false }
      malformed_group := pkg.db.meta_keys(
        target,
        pkg.db.TableRef { schema: "main", name: "target" },
        pkg.db.MetaDetail.Names,
        out,
        [],
      )
      group_ok := match malformed_group {
        Err(error) => match error { Decode(_) => true _ => false }
        Ok(_) => false
      }
      if !malformed_ok { return 11 }
      if !after_ok { return 12 }
      if !group_ok { return 13 }
      if align_q5b2_finalize_calls() != 3 { return 14 }
      return 0
    }
  }
}

fn postgres_shape_ok() -> bool {
  unsafe {
    state := pkg.db.internal.resource.new_conn_state(
      2, align_q5b2_fake_postgres(), 0,
    )
    connection: pkg.db.conn := resource.from_raw(state)
    target := pkg.db.exec_conn(connection)
    arena out {
      malformed := pkg.db.meta_schemas(target, pkg.db.MetaDetail.Names, out, [])
      malformed_ok := match malformed {
        Err(error) => match error {
          Decode(contract) => contract.item == "metadata.schemas"
          _ => false
        }
        Ok(_) => false
      }
      after := pkg.db.meta_schemas(target, pkg.db.MetaDetail.Names, out, [])
      after_ok := match after { Ok(rows) => rows.len() == 0 Err(_) => false }
      malformed_group := pkg.db.meta_keys(
        target,
        pkg.db.TableRef { schema: "public", name: "target" },
        pkg.db.MetaDetail.Names,
        out,
        [],
      )
      group_ok := match malformed_group {
        Err(error) => match error { Decode(_) => true _ => false }
        Ok(_) => false
      }
      return malformed_ok && after_ok && group_ok && align_q5b2_clear_calls() == 3
    }
  }
}

pub fn run() -> i32 {
  sqlite_status := sqlite_shape_status()
  if sqlite_status != 0 { return sqlite_status }
  if !postgres_shape_ok() { return 2 }
  return 42
}
"#;

const SQLITE_SHAPE_MAIN: &str = r#"module main
import pkg.db.shape_fixture

fn main() -> i32 = pkg.db.shape_fixture.run()
"#;

const POSTGRES_LEASE_MODULE: &str = r#"module pkg.db.lease_fixture
import pkg.db
import pkg.db.postgres
import pkg.db.internal.resource

Params { id: i64 }
Row { id: i64, again: i64 }

fn query() -> pkg.db.query<Params, Row> = pkg.db.query(
  "SELECT :id AS id, :id AS again",
  [],
)

fn null_result_query() -> pkg.db.query<Params, Row> = pkg.db.query(
  "SELECT :id AS id, :id AS again /* NULL_RESULT */",
  [],
)

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
}

fn catalog_overlap<T>(result: Result<T, pkg.db.Error>) -> bool = match result {
  Err(error) => match error {
    Unsupported(contract) => contract.item == "postgres.connection.active_execution"
      && match contract.query_id { None => true Some(_) => false }
    _ => false
  }
  Ok(_) => false
}

fn explain_overlap<T>(result: Result<T, pkg.db.Error>) -> bool = match result {
  Err(error) => match error {
    Unsupported(contract) => contract.item == "postgres.connection.active_execution"
      && match contract.query_id { Some(_) => true None => false }
    _ => false
  }
  Ok(_) => false
}

fn lease_active(state: raw) -> bool {
  unsafe {
    active: u8 := raw.load(state, 20)
    return active == 1
  }
}

fn exercise(target: pkg.db.exec, state: raw) -> i32 {
  unsafe {
    align_pg_reset()
    raw.store(state, 20, 1 as u8)
    table := pkg.db.TableRef { schema: "public", name: "target" }
    schema: Option<pkg.db.SchemaRef> := None
    query := query()
    arena out {
      if !catalog_overlap(pkg.db.meta_database(target, pkg.db.MetaDetail.Names, out, [])) {
        return 1
      }
      if !catalog_overlap(pkg.db.meta_schemas(target, pkg.db.MetaDetail.Names, out, [])) {
        return 2
      }
      if !catalog_overlap(pkg.db.meta_tables(
        target, schema, pkg.db.MetaDetail.Names, out, [],
      )) { return 3 }
      if !catalog_overlap(pkg.db.meta_table(target, table, pkg.db.MetaDetail.Names, out, [])) {
        return 4
      }
      if !catalog_overlap(pkg.db.meta_columns(target, table, pkg.db.MetaDetail.Names, out, [])) {
        return 5
      }
      if !catalog_overlap(pkg.db.meta_keys(target, table, pkg.db.MetaDetail.Names, out, [])) {
        return 6
      }
      if !catalog_overlap(pkg.db.meta_indexes(target, table, pkg.db.MetaDetail.Names, out, [])) {
        return 7
      }

      if !catalog_overlap(pkg.db.postgres.meta_database_native(
        target, pkg.db.MetaDetail.Names, out, [], [],
      )) { return 8 }
      if !catalog_overlap(pkg.db.postgres.meta_schemas_native(
        target, pkg.db.MetaDetail.Names, out, [], [],
      )) { return 9 }
      if !catalog_overlap(pkg.db.postgres.meta_tables_native(
        target, schema, pkg.db.MetaDetail.Names, out, [], [],
      )) { return 10 }
      if !catalog_overlap(pkg.db.postgres.meta_table_native(
        target, table, pkg.db.MetaDetail.Names, out, [], [],
      )) { return 11 }
      if !catalog_overlap(pkg.db.postgres.meta_columns_native(
        target, table, pkg.db.MetaDetail.Names, out, [], [],
      )) { return 12 }
      if !catalog_overlap(pkg.db.postgres.meta_keys_native(
        target, table, pkg.db.MetaDetail.Names, out, [], [],
      )) { return 13 }
      if !catalog_overlap(pkg.db.postgres.meta_indexes_native(
        target, table, pkg.db.MetaDetail.Names, out, [], [],
      )) { return 14 }

      if !explain_overlap(pkg.db.explain(
        target, query, Params { id: 1 }, out, [],
      )) { return 15 }
      if !explain_overlap(pkg.db.postgres.explain_native(
        target, query, Params { id: 2 }, out, [], [],
      )) { return 16 }
      if !lease_active(state) { return 17 }
      if align_pg_execute_calls() != 0 { return 18 }
      raw.store(state, 20, 0 as u8)
      failed := pkg.db.explain(
        target, null_result_query(), Params { id: 3 }, out, [],
      )
      match failed { Ok(_) => { return 20 } Err(_) => {} }
      if lease_active(state) { return 21 }
      if align_pg_execute_calls() != 1 { return 22 }
      return 0
    }
  }
}

pub fn run() -> i32 {
  unsafe {
    opened := pkg.db.postgres.connect("fake", [])
    connection := opened else { return 23 }
    connection_state := resource.raw(resource.borrow(connection))
    connection_status := exercise(pkg.db.exec_conn(connection), connection_state)
    if connection_status != 0 { return connection_status }

    tx_opened := pkg.db.postgres.connect("fake", [])
    tx_connection := tx_opened else { return 24 }
    transaction_state := resource.into_raw(tx_connection)
    pkg.db.internal.resource.set_transaction_active(transaction_state, true)
    transaction: pkg.db.tx := resource.from_raw(transaction_state)
    transaction_status := exercise(pkg.db.exec_tx(transaction), transaction_state)
    if transaction_status != 0 { return 30 + transaction_status }
    return 73
  }
}
"#;

const POSTGRES_LEASE_MAIN: &str = r#"module main
import pkg.db.lease_fixture

fn main() -> i32 = pkg.db.lease_fixture.run()
"#;

const POSTGRES_STATUS_MODULE: &str = r#"module pkg.db.status_fixture
import pkg.db
import pkg.db.postgres
import pkg.db.internal.resource

Params { value: i64 }
PreparedParams { id: i64, label: str, payload: slice<u8> }
Row { value: i64 }

fn query() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value", [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

fn wait_query() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* TIMEOUT_WAIT */", [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

fn prepared_query() -> pkg.db.query<PreparedParams, Row> = pkg.db.postgres.query(
  "SELECT CAST(:id AS BIGINT) AS value WHERE :label = :label AND :payload = :payload", [],
  [pkg.db.postgres.QueryOption.ParameterType("id", "int8")],
)

fn prepared_wait_query() -> pkg.db.query<PreparedParams, Row> = pkg.db.postgres.query(
  "SELECT CAST(:id AS BIGINT) AS value WHERE :label = :label AND :payload = :payload /* TIMEOUT_WAIT */", [],
  [pkg.db.postgres.QueryOption.ParameterType("id", "int8")],
)

fn command() -> pkg.db.command<Params> = pkg.db.postgres.command(
  "UPDATE stub SET value = :value /* COMMAND_OK */", [],
  [pkg.db.postgres.CommandOption.ParameterType("value", "int8")],
)

extern "C" {
  fn align_pg_reset()
  fn align_pg_force_result_status(status: i32)
  fn align_pg_clear_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_forbidden_after_status_calls() -> i32
  fn align_pg_make_result() -> raw
}

fn force(status: i32) { unsafe { align_pg_force_result_status(status) } }

fn exact_native<T>(result: Result<T, pkg.db.Error>) -> bool = match result {
  Err(error) => match error {
    Native(native) => native.message == "stub execution failure"
      && match native.sqlstate { Some(state) => state == "XX000" None => false }
    _ => false
  }
  Ok(_) => false
}

fn exact_timeout<T>(result: Result<T, pkg.db.Error>) -> bool = match result {
  Err(error) => match error {
    Timeout(native) => native.message == "cancelled by deadline"
    _ => false
  }
  Ok(_) => false
}

fn exact_command<T>(status: i32, result: Result<T, pkg.db.Error>) -> bool {
  if status == 3 || status == 4 || status == 8 {
    return match result {
      Err(error) => match error {
        InvalidQuery(contract) => contract.item == "db.command.row"
        _ => false
      }
      Ok(_) => false
    }
  }
  return exact_native(result)
}

fn closed(state: raw) -> bool {
  unsafe {
    closed_tag: u8 := raw.load(state, 5)
    native: raw := raw.load(state, 8)
    lease: u8 := raw.load(state, 20)
    transaction: u8 := raw.load(state, 21)
    return closed_tag == 1 && native.is_null() && lease == 0 && transaction == 0
  }
}

fn counters(expected_clear: i32) -> bool {
  unsafe {
    return align_pg_clear_calls() == expected_clear && align_pg_finish_calls() == 1
      && align_pg_forbidden_after_status_calls() == 0
  }
}

fn direct_rows(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  target := pkg.db.exec_conn(connection)
  force(status)
  failed := exact_native(pkg.db.rows(target, query(), Params { value: 7 }, []))
  return failed && closed(state) && counters(1)
}

fn direct_one(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  target := pkg.db.exec_conn(connection)
  force(status)
  arena out {
    failed := exact_native(pkg.db.one(target, query(), Params { value: 7 }, out, []))
    return failed && closed(state) && counters(1)
  }
}

fn direct_command(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  target := pkg.db.exec_conn(connection)
  force(status)
  failed := exact_command(status, pkg.db.execute(target, command(), Params { value: 7 }, []))
  return failed && closed(state) && counters(1)
}

fn prepared_rows(status: i32, timeout: bool, wait: bool) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  target := pkg.db.exec_conn(connection)
  mut statement := pkg.db.prepare(
    target, if wait { prepared_wait_query() } else { prepared_query() }, [],
  ) else { return false }
  force(status)
  failed := if timeout {
    if wait {
      exact_timeout(pkg.db.rows_stmt(
        statement, PreparedParams {
          id: 7, label: "first", payload: [1 as u8, 2 as u8, 3 as u8],
        }, [pkg.db.ExecuteOption.TimeoutNs(1000000)],
      ))
    } else {
      exact_native(pkg.db.rows_stmt(
        statement, PreparedParams {
          id: 7, label: "first", payload: [1 as u8, 2 as u8, 3 as u8],
        }, [pkg.db.ExecuteOption.TimeoutNs(100000000)],
      ))
    }
  } else {
    exact_native(pkg.db.rows_stmt(statement, PreparedParams {
      id: 7, label: "first", payload: [1 as u8, 2 as u8, 3 as u8],
    }, []))
  }
  return failed && closed(state) && counters(2)
}

fn prepare_result(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  force(status)
  failed := exact_native(pkg.db.prepare(
    pkg.db.exec_conn(connection), prepared_query(), [],
  ))
  return failed && closed(state) && counters(1)
}

fn timed_rows(status: i32, wait: bool) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  target := pkg.db.exec_conn(connection)
  force(status)
  failed := if wait {
    exact_timeout(pkg.db.rows(
      target, wait_query(), Params { value: 7 }, [pkg.db.ExecuteOption.TimeoutNs(1000000)],
    ))
  } else {
    exact_native(pkg.db.rows(
      target, query(), Params { value: 7 }, [pkg.db.ExecuteOption.TimeoutNs(100000000)],
    ))
  }
  return failed && closed(state) && counters(1)
}

fn timed_one(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  target := pkg.db.exec_conn(connection)
  force(status)
  arena out {
    failed := exact_native(pkg.db.one(
      target, query(), Params { value: 7 }, out,
      [pkg.db.ExecuteOption.TimeoutNs(100000000)],
    ))
    return failed && closed(state) && counters(1)
  }
}

fn timed_command(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  target := pkg.db.exec_conn(connection)
  force(status)
  failed := exact_command(status, pkg.db.execute(
    target, command(), Params { value: 7 },
    [pkg.db.ExecuteOption.TimeoutNs(100000000)],
  ))
  return failed && closed(state) && counters(1)
}

fn begin_common(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  force(status)
  failed := exact_native(pkg.db.begin(connection, []))
  return failed && counters(1)
}

fn begin_native(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  force(status)
  failed := exact_native(pkg.db.postgres.begin_native(connection, [], []))
  return failed && counters(1)
}

fn finish_transaction(status: i32, commit: bool) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  transaction := pkg.db.begin(connection, []) else { return false }
  force(status)
  failed := if commit {
    exact_native(pkg.db.commit(transaction))
  } else { exact_native(pkg.db.rollback(transaction)) }
  return failed && counters(2)
}

fn catalog(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  target := pkg.db.exec_conn(connection)
  force(status)
  arena out {
    failed := exact_native(pkg.db.meta_schemas(target, pkg.db.MetaDetail.Names, out, []))
    return failed && closed(state) && counters(1)
  }
}

fn explain(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  target := pkg.db.exec_conn(connection)
  force(status)
  arena out {
    failed := exact_native(pkg.db.explain(target, query(), Params { value: 7 }, out, []))
    return failed && closed(state) && counters(1)
  }
}

fn force_drop_rows<R>(rows: pkg.db.rows<R>, status: i32) { force(status) }
fn force_drop_stmt<P, R>(statement: pkg.db.stmt<P, R>, status: i32) { force(status) }
fn force_drop_tx(transaction: pkg.db.tx, status: i32) { force(status) }
fn force_drop_cursor(cursor: pkg.db.catalog_cursor, status: i32) { force(status) }

fn rows_drop(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  rows := pkg.db.rows(pkg.db.exec_conn(connection), query(), Params { value: 7 }, []) else {
    return false
  }
  force_drop_rows(rows, status)
  return closed(state) && counters(1)
}

fn stmt_drop(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  statement := pkg.db.prepare(pkg.db.exec_conn(connection), prepared_query(), []) else {
    return false
  }
  force_drop_stmt(statement, status)
  return closed(state) && counters(2)
}

fn tx_drop(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  transaction := pkg.db.begin(connection, []) else { return false }
  force_drop_tx(transaction, status)
  return counters(2)
}

fn cursor_drop(status: i32) -> bool {
  unsafe { align_pg_reset() }
  connection := pkg.db.postgres.connect("fake", []) else { return false }
  state := unsafe { resource.raw(resource.borrow(connection)) }
  result := unsafe { align_pg_make_result() }
  if result.is_null() { return false }
  wrapper := unsafe { pkg.db.internal.resource.new_catalog_cursor(
    2, result, state, raw.null(), raw.null(),
  ) }
  cursor: pkg.db.catalog_cursor := unsafe { resource.from_raw_borrowed(
    wrapper, resource.borrow(connection),
  ) }
  force_drop_cursor(cursor, status)
  return closed(state) && counters(1)
}

fn exercise(status: i32) -> i32 {
  if !direct_rows(status) { return 1 }
  if !direct_one(status) { return 2 }
  if !direct_command(status) { return 3 }
  if !prepared_rows(status, false, false) { return 4 }
  if !prepare_result(status) { return 5 }
  if !timed_rows(status, false) { return 6 }
  if !timed_one(status) { return 7 }
  if !timed_command(status) { return 8 }
  if !prepared_rows(status, true, false) { return 9 }
  if !timed_rows(status, true) { return 10 }
  if !prepared_rows(status, true, true) { return 11 }
  if !begin_common(status) { return 12 }
  if !begin_native(status) { return 13 }
  if !finish_transaction(status, true) { return 14 }
  if !finish_transaction(status, false) { return 15 }
  if !catalog(status) { return 16 }
  if !explain(status) { return 17 }
  if !rows_drop(status) { return 18 }
  if !stmt_drop(status) { return 19 }
  if !tx_drop(status) { return 20 }
  if !cursor_drop(status) { return 21 }
  return 0
}

pub fn run() -> i32 {
  statuses := [3 as i32, 4 as i32, 8 as i32, 10 as i32, 11 as i32, 99 as i32]
  mut i: i64 := 0
  loop {
    if i >= statuses.len() { break }
    failure := exercise(statuses[i])
    if failure != 0 { return failure }
    i = i + 1
  }
  return 200
}
"#;

const POSTGRES_STATUS_MAIN: &str = r#"module main
import pkg.db.status_fixture

fn main() -> i32 = pkg.db.status_fixture.run()
"#;

const LOOKUP: &str = r#"module app.lookup
import pkg.db

pub Params { id: i64 }
pub Row { id: i64, again: i64 }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.query(
  "SELECT :id AS id, :id AS again",
  [],
)
"#;

const BAD_LOOKUP: &str = r#"module app.bad_lookup
import pkg.db

pub Params { id: i64 }
pub Row { id: i64 }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.query(
  "SELECT FROM broken WHERE value = :id",
  [],
)
"#;

const SETUP: &str = r#"module pkg.db.q5b2_setup
import pkg.db

extern "C" link("sqlite3") {
  fn sqlite3_exec(database: raw, sql: raw, callback: raw, argument: raw, error_out: raw) -> i32
}

extern "C" link("pq") {
  fn PQexec(connection: raw, command: raw) -> raw
  fn PQresultStatus(result: raw) -> i32
  fn PQclear(result: raw)
}

fn c_string(value: str) -> raw {
  unsafe {
    bytes := value.bytes()
    output := raw.alloc(bytes.len() + 1)
    mut i := 0
    loop {
      if i >= bytes.len() { break }
      raw.store(output, i, bytes[i])
      i = i + 1
    }
    terminal: u8 := 0
    raw.store(output, bytes.len(), terminal)
    return output
  }
}

pub fn sqlite(target: pkg.db.exec) -> bool {
  unsafe {
    state := match target {
      Conn(reference) => resource.raw(reference)
      Tx(_) => { return false }
    }
    database: raw := raw.load(state, 8)
    sql := c_string("CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL DEFAULT '', score REAL GENERATED ALWAYS AS (length(name)) STORED); CREATE VIEW user_names AS SELECT name FROM users; CREATE TABLE parent (id INTEGER PRIMARY KEY); CREATE TABLE child (a INTEGER, b TEXT, PRIMARY KEY (a, b), UNIQUE (b), FOREIGN KEY (a) REFERENCES parent(id) ON DELETE CASCADE); CREATE INDEX child_b_desc ON child (b DESC)")
    status := sqlite3_exec(database, sql, raw.null(), raw.null(), raw.null())
    raw.free(sql)
    return status == 0
  }
}

pub fn set_closed(target: pkg.db.exec, closed: bool) {
  unsafe {
    state := match target {
      Conn(reference) => resource.raw(reference)
      Tx(_) => { return }
    }
    tag: u8 := if closed { 1 } else { 0 }
    raw.store(state, 5, tag)
  }
}

pub fn postgres(target: pkg.db.exec) -> bool {
  unsafe {
    state := match target {
      Conn(reference) => resource.raw(reference)
      Tx(_) => { return false }
    }
    connection: raw := raw.load(state, 8)
    sql := c_string("DROP SCHEMA IF EXISTS align_q5b2 CASCADE; DROP SCHEMA IF EXISTS pguser_q5b2 CASCADE; CREATE SCHEMA align_q5b2; CREATE SCHEMA pguser_q5b2; SET search_path = align_q5b2; CREATE TABLE align_q5b2.counter (value BIGINT NOT NULL); INSERT INTO align_q5b2.counter VALUES (0); CREATE TABLE align_q5b2.parent (id BIGINT PRIMARY KEY); CREATE TABLE align_q5b2.child (a BIGINT, b TEXT, generated BIGINT GENERATED ALWAYS AS (a + 1) STORED, PRIMARY KEY (a, b), UNIQUE (b), FOREIGN KEY (a) REFERENCES align_q5b2.parent(id) ON DELETE CASCADE); CREATE INDEX align_q5b2_child_b ON align_q5b2.child (b DESC) INCLUDE (generated); CREATE INDEX align_q5b2_child_b_hash ON align_q5b2.child USING hash (b)")
    result := PQexec(connection, sql)
    raw.free(sql)
    if result.is_null() { return false }
    status := PQresultStatus(result)
    PQclear(result)
    return status == 1
  }
}
"#;

const POSTGRES_QUERIES: &str = r#"module app.pg_inspect
import pkg.db
import pkg.db.postgres

pub Params { probe: i64 }
pub Row { value: i64 }

pub fn mutate() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "UPDATE align_q5b2.counter SET value = value + 1 WHERE :probe = 1 RETURNING value",
  [],
  [],
)

pub fn counter() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT value FROM align_q5b2.counter WHERE :probe = 1",
  [],
  [],
)
"#;

const BAD_NORMALIZED_EXPLAIN: &str = r#"module pkg.db.bad_normalized_explain
import pkg.db
import pkg.db.postgres
import pkg.db.internal.descriptor

Params { probe: i64 }
Row { probe: i64 }

fn query() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT :probe AS probe", [], [],
)

pub fn run(
  target: pkg.db.exec,
  out: region,
) -> Result<pkg.db.QueryPlan, pkg.db.Error> {
  unsafe {
    statement := query()
    return pkg.db.internal.descriptor.explain_postgres_native(
      statement, target, Params { probe: 1 }, out, "bad-normalized", 0, 1, 0,
    )
  }
}
"#;

const MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import pkg.db.q5b2_setup

fn table_kind(value: pkg.db.MetaTableKind) -> bool = match value {
  Table => true
  _ => false
}

fn view_kind(value: pkg.db.MetaTableKind) -> bool = match value {
  View => true
  _ => false
}

fn unknown(value: pkg.db.MetaNullability) -> bool = match value {
  Unknown => true
  _ => false
}

fn absent(value: Option<str>) -> bool = match value {
  Some(_) => false
  None => true
}

fn is_not_found(result: Result<pkg.db.TableMeta, pkg.db.Error>) -> bool = match result {
  Err(error) => match error { NotFound => true _ => false }
  Ok(_) => false
}

fn encode_item<T>(result: Result<T, pkg.db.Error>, expected: str) -> bool = match result {
  Err(error) => match error {
    Encode(contract) => contract.item == expected && match contract.query_id {
      None => true
      Some(_) => false
    }
    _ => false
  }
  Ok(_) => false
}

fn main() -> i32 {
  opened := pkg.db.sqlite.connect(":memory:", [])
  connection := opened else { return 1 }
  target := pkg.db.exec_conn(connection)
  if !pkg.db.q5b2_setup.sqlite(target) { return 2 }
  arena out {
    database_result := pkg.db.meta_database(target, pkg.db.MetaDetail.Full, out, [])
    database := database_result else { return 3 }
    if database.engine_version.len() == 0 { return 4 }
    default_schema := database.default_schema else { return 5 }
    encoding := database.encoding else { return 6 }
    read_only := database.read_only else { return 7 }
    transactional := database.transactional_ddl else { return 8 }
    if default_schema != "main" || encoding != "UTF-8" || read_only || !transactional { return 9 }

    schemas_result := pkg.db.meta_schemas(target, pkg.db.MetaDetail.Names, out, [])
    schemas := schemas_result else { return 10 }
    if schemas.len() != 1 || schemas[0].name != "main" || schemas[0].system { return 11 }

    tables_result := pkg.db.meta_tables(target, None, pkg.db.MetaDetail.Summary, out, [])
    tables := tables_result else { return 12 }
    if tables.len() != 4 { return 13 }
    if tables[0].name != "child" || !table_kind(tables[0].kind) { return 14 }
    if tables[1].name != "parent" || !table_kind(tables[1].kind) { return 15 }
    if tables[2].name != "user_names" || !view_kind(tables[2].kind) { return 16 }
    if tables[3].name != "users" || !table_kind(tables[3].kind) { return 17 }

    reference := pkg.db.TableRef { schema: "main", name: "users" }
    table_result := pkg.db.meta_table(target, reference, pkg.db.MetaDetail.Names, out, [])
    table := table_result else { return 18 }
    if !table_kind(table.kind) || !absent(table.native_kind) { return 19 }
    missing := pkg.db.meta_table(
      target,
      pkg.db.TableRef { schema: "main", name: "missing" },
      pkg.db.MetaDetail.Names,
      out,
      [],
    )
    if !is_not_found(missing) { return 20 }

    names_result := pkg.db.meta_columns(target, reference, pkg.db.MetaDetail.Names, out, [])
    names := names_result else { return 21 }
    if names.len() != 2 || names[0].ordinal != 0 || names[1].ordinal != 1 { return 22 }
    if names[0].name != "id" || names[1].name != "name" { return 23 }
    if !unknown(names[0].nullable) || !absent(names[0].native_type) { return 24 }
    full_result := pkg.db.meta_columns(target, reference, pkg.db.MetaDetail.Full, out, [])
    full := full_result else { return 25 }
    id_type := full[0].logical_type else { return 26 }
    name_type := full[1].logical_type else { return 27 }
    if id_type != "i64" || name_type != "str" { return 28 }

    child := pkg.db.TableRef { schema: "main", name: "child" }
    keys_result := pkg.db.meta_keys(target, child, pkg.db.MetaDetail.Full, out, [])
    keys := keys_result else { return 29 }
    if keys.len() != 4 { return 30 }
    if keys[0].key_ordinal != 0 || keys[0].term_ordinal != 0 { return 31 }
    if keys[1].key_ordinal != 0 || keys[1].term_ordinal != 1 { return 32 }
    if keys[2].key_ordinal != 1 || keys[2].term_ordinal != 0 { return 33 }
    if keys[3].key_ordinal != 2 || keys[3].term_ordinal != 0 { return 34 }
    foreign_table := keys[3].referenced_table else { return 35 }
    if foreign_table != "parent" { return 36 }

    indexes_result := pkg.db.meta_indexes(target, child, pkg.db.MetaDetail.Summary, out, [])
    indexes := indexes_result else { return 37 }
    if indexes.len() != 4 { return 38 }
    if indexes[0].name != "child_b_desc" || indexes[0].term_ordinal != 0 { return 39 }
    if indexes[1].name != "sqlite_autoindex_child_1" || indexes[1].term_ordinal != 0 { return 40 }
    if indexes[2].name != "sqlite_autoindex_child_1" || indexes[2].term_ordinal != 1 { return 41 }
    if indexes[3].name != "sqlite_autoindex_child_2" || indexes[3].term_ordinal != 0 { return 42 }

    bad_schema := pkg.db.meta_tables(
      target,
      Some(pkg.db.SchemaRef { name: "bad\0schema" }),
      pkg.db.MetaDetail.Names,
      out,
      [],
    )
    if !encode_item(bad_schema, "metadata.schema") { return 43 }
    bad_table := pkg.db.meta_table(
      target,
      pkg.db.TableRef { schema: "bad\0schema", name: "bad\0name" },
      pkg.db.MetaDetail.Names,
      out,
      [],
    )
    if !encode_item(bad_table, "metadata.table.schema") { return 44 }

    return 45
  }
}
"#;

const EXPLAIN_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import app.lookup
import app.bad_lookup

fn sqlite_driver(value: pkg.db.Driver) -> bool = match value {
  SQLite => true
  _ => false
}

fn text_plan(value: pkg.db.PlanFormat) -> bool = match value {
  Text => true
  _ => false
}

fn native_plan(value: pkg.db.PlanFormat) -> bool = match value {
  Native => true
  _ => false
}

fn contract_item<T>(result: Result<T, pkg.db.Error>, expected: str) -> bool = match result {
  Err(error) => match error {
    Unsupported(contract) => contract.item == expected && match contract.query_id {
      Some(_) => true
      None => false
    }
    _ => false
  }
  Ok(_) => false
}

fn main() -> i32 {
  opened := pkg.db.sqlite.connect(":memory:", [])
  connection := opened else { return 1 }
  target := pkg.db.exec_conn(connection)
  query := app.lookup.query()
  arena out {
    plan_result := pkg.db.explain(
      target,
      query,
      app.lookup.Params { id: 1 },
      out,
      [],
    )
    plan := plan_result else { return 2 }
    if !sqlite_driver(plan.driver) || !text_plan(plan.format) || plan.analyzed { return 3 }
    if plan.body.len() == 0 { return 4 }
    bytecode_result := pkg.db.sqlite.explain_native(
      target,
      query,
      app.lookup.Params { id: 2 },
      out,
      [],
      [pkg.db.sqlite.ExplainOption.Bytecode],
    )
    bytecode := bytecode_result else { return 5 }
    if !native_plan(bytecode.format) || bytecode.analyzed || bytecode.body.len() == 0 { return 6 }

    duplicate_mode := pkg.db.sqlite.explain_native(
      target,
      query,
      app.lookup.Params { id: 2 },
      out,
      [],
      [pkg.db.sqlite.ExplainOption.QueryPlan, pkg.db.sqlite.ExplainOption.Bytecode],
    )
    if !contract_item(duplicate_mode, "sqlite.explain.mode") { return 10 }
    unavailable_timeout := pkg.db.explain(
      target,
      query,
      app.lookup.Params { id: 2 },
      out,
      [pkg.db.ExplainOption.TimeoutNs(1)],
    )
    if !contract_item(unavailable_timeout, "db.explain.timeout_ns") { return 11 }

    broken := pkg.db.explain(
      target,
      app.bad_lookup.query(),
      app.bad_lookup.Params { id: 3 },
      out,
      [],
    )
    match broken { Ok(_) => { return 7 } Err(_) => {} }
    after_error := pkg.db.explain(
      target,
      query,
      app.lookup.Params { id: 4 },
      out,
      [],
    ) else { return 8 }
    if after_error.body.len() == 0 { return 9 }
    return 42
  }
}
"#;

const SQLITE_NATIVE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import pkg.db.q5b2_setup

fn contract_item<T>(result: Result<T, pkg.db.Error>, expected: str) -> bool = match result {
  Err(error) => match error {
    Unsupported(contract) => contract.item == expected && match contract.query_id {
      None => true
      Some(_) => false
    }
    _ => false
  }
  Ok(_) => false
}

fn encode_item<T>(result: Result<T, pkg.db.Error>, expected: str) -> bool = match result {
  Err(error) => match error {
    Encode(contract) => contract.item == expected && match contract.query_id {
      None => true
      Some(_) => false
    }
    _ => false
  }
  Ok(_) => false
}

fn main() -> i32 {
  opened := pkg.db.sqlite.connect(":memory:", [])
  connection := opened else { return 1 }
  target := pkg.db.exec_conn(connection)
  if !pkg.db.q5b2_setup.sqlite(target) { return 2 }
  arena out {
    database := pkg.db.sqlite.meta_database_native(
      target, pkg.db.MetaDetail.Full, out, [], [],
    ) else { return 3 }
    if database.engine_version.len() == 0 { return 4 }

    schemas := pkg.db.sqlite.meta_schemas_native(
      target, pkg.db.MetaDetail.Names, out, [], [],
    ) else { return 5 }
    if schemas.len() != 1 || schemas[0].name != "main" { return 6 }

    tables := pkg.db.sqlite.meta_tables_native(
      target,
      None,
      pkg.db.MetaDetail.Names,
      out,
      [],
      [pkg.db.sqlite.MetaOption.IncludeInternalObjects],
    ) else { return 7 }
    if tables.len() != 5 || tables[2].name != "sqlite_sequence" { return 8 }

    users := pkg.db.TableRef { schema: "main", name: "users" }
    table := pkg.db.sqlite.meta_table_native(
      target, users, pkg.db.MetaDetail.Names, out, [], [],
    ) else { return 9 }
    if table.name != "users" { return 10 }

    columns := pkg.db.sqlite.meta_columns_native(
      target,
      users,
      pkg.db.MetaDetail.Full,
      out,
      [],
      [pkg.db.sqlite.MetaOption.IncludeHiddenColumns],
    ) else { return 11 }
    if columns.len() != 3 || columns[2].name != "score" || columns[2].ordinal != 2 {
      return 12
    }

    child := pkg.db.TableRef { schema: "main", name: "child" }
    keys := pkg.db.sqlite.meta_keys_native(
      target, child, pkg.db.MetaDetail.Summary, out, [], [],
    ) else { return 13 }
    indexes := pkg.db.sqlite.meta_indexes_native(
      target, child, pkg.db.MetaDetail.Summary, out, [], [],
    ) else { return 14 }
    if keys.len() != 4 || indexes.len() != 4 { return 15 }

    common_first := pkg.db.sqlite.meta_table_native(
      target,
      pkg.db.TableRef { schema: "bad\0schema", name: "bad\0name" },
      pkg.db.MetaDetail.Names,
      out,
      [pkg.db.MetaOption.TimeoutNs(0)],
      [
        pkg.db.sqlite.MetaOption.IncludeInternalObjects,
        pkg.db.sqlite.MetaOption.IncludeInternalObjects,
      ],
    )
    if !contract_item(common_first, "db.meta.timeout_ns") { return 16 }

    native_second := pkg.db.sqlite.meta_table_native(
      target,
      pkg.db.TableRef { schema: "bad\0schema", name: "bad\0name" },
      pkg.db.MetaDetail.Names,
      out,
      [],
      [
        pkg.db.sqlite.MetaOption.IncludeHiddenColumns,
        pkg.db.sqlite.MetaOption.IncludeHiddenColumns,
      ],
    )
    if !contract_item(native_second, "sqlite.meta.include_hidden_columns") { return 17 }

    identifier_third := pkg.db.sqlite.meta_table_native(
      target,
      pkg.db.TableRef { schema: "bad\0schema", name: "bad\0name" },
      pkg.db.MetaDetail.Names,
      out,
      [],
      [],
    )
    if !encode_item(identifier_third, "metadata.table.schema") { return 18 }
    duplicate_common := pkg.db.sqlite.meta_schemas_native(
      target,
      pkg.db.MetaDetail.Names,
      out,
      [pkg.db.MetaOption.IncludeSystem, pkg.db.MetaOption.IncludeSystem],
      [],
    )
    if !contract_item(duplicate_common, "db.meta.include_system") { return 19 }
    return 52
  }
}
"#;

const POSTGRES_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import pkg.db.postgres
import pkg.db.q5b2_setup
import app.pg_inspect

fn json_plan(value: pkg.db.PlanFormat) -> bool = match value {
  Json => true
  _ => false
}

fn mismatch<T>(result: Result<T, pkg.db.Error>) -> bool = match result {
  Err(error) => match error { DriverMismatch(_) => true _ => false }
  Ok(_) => false
}

fn descending(value: Option<pkg.db.MetaSortOrder>) -> bool = match value {
  Some(Desc) => true
  _ => false
}

fn nulls_first(value: Option<pkg.db.MetaNullOrder>) -> bool = match value {
  Some(First) => true
  _ => false
}

fn no_sort(value: Option<pkg.db.MetaSortOrder>) -> bool = match value {
  None => true
  Some(_) => false
}

fn no_null_order(value: Option<pkg.db.MetaNullOrder>) -> bool = match value {
  None => true
  Some(_) => false
}

fn contract_item<T>(result: Result<T, pkg.db.Error>, expected: str) -> bool = match result {
  Err(error) => match error {
    Unsupported(contract) => contract.item == expected
    _ => false
  }
  Ok(_) => false
}

fn run(url: str) -> i32 {
  opened := pkg.db.postgres.connect(url, [])
  connection := opened else { return 1 }
  target := pkg.db.exec_conn(connection)
  if !pkg.db.q5b2_setup.postgres(target) { return 2 }
  arena out {
    database := pkg.db.postgres.meta_database_native(
      target, pkg.db.MetaDetail.Full, out, [], [],
    ) else { return 3 }
    if database.engine_version.len() == 0 { return 4 }

    schemas := pkg.db.postgres.meta_schemas_native(
      target,
      pkg.db.MetaDetail.Summary,
      out,
      [],
      [pkg.db.postgres.MetaOption.SearchPathOnly],
    ) else { return 5 }
    mut saw_schema := false
    mut schema_index := 0
    loop {
      if schema_index >= schemas.len() { break }
      if schemas[schema_index].name == "align_q5b2" && schemas[schema_index].visible {
        saw_schema = true
      }
      schema_index = schema_index + 1
    }
    if !saw_schema { return 6 }

    all_user_schemas := pkg.db.postgres.meta_schemas_native(
      target, pkg.db.MetaDetail.Names, out, [], [],
    ) else { return 34 }
    mut saw_pguser := false
    mut all_schema_index := 0
    loop {
      if all_schema_index >= all_user_schemas.len() { break }
      if all_user_schemas[all_schema_index].name == "pguser_q5b2"
        && !all_user_schemas[all_schema_index].system {
        saw_pguser = true
      }
      all_schema_index = all_schema_index + 1
    }
    if !saw_pguser { return 35 }

    visible_system_schemas := pkg.db.postgres.meta_schemas_native(
      target,
      pkg.db.MetaDetail.Names,
      out,
      [pkg.db.MetaOption.IncludeSystem],
      [pkg.db.postgres.MetaOption.SearchPathOnly],
    ) else { return 36 }
    mut saw_pg_catalog := false
    mut visible_schema_index := 0
    loop {
      if visible_schema_index >= visible_system_schemas.len() { break }
      current := visible_system_schemas[visible_schema_index]
      if current.name == "pg_catalog" && current.visible && current.system {
        saw_pg_catalog = true
      }
      visible_schema_index = visible_schema_index + 1
    }
    if !saw_pg_catalog { return 37 }

    schema_filter: Option<pkg.db.SchemaRef> := Some(pkg.db.SchemaRef { name: "align_q5b2" })
    tables := pkg.db.postgres.meta_tables_native(
      target,
      schema_filter,
      pkg.db.MetaDetail.Summary,
      out,
      [],
      [pkg.db.postgres.MetaOption.SearchPathOnly],
    ) else { return 7 }
    if tables.len() != 3 || tables[0].name != "child" || tables[1].name != "counter"
      || tables[2].name != "parent" {
      return 8
    }

    child := pkg.db.TableRef { schema: "align_q5b2", name: "child" }
    table := pkg.db.postgres.meta_table_native(
      target, child, pkg.db.MetaDetail.Full, out, [], [],
    ) else { return 9 }
    if table.name != "child" { return 10 }
    columns := pkg.db.postgres.meta_columns_native(
      target, child, pkg.db.MetaDetail.Full, out, [], [],
    ) else { return 11 }
    if columns.len() != 3 || columns[0].ordinal != 0 || columns[2].ordinal != 2
      || columns[2].name != "generated" {
      return 12
    }
    generated_sql := columns[2].generated_sql else { return 31 }
    if generated_sql.len() == 0 { return 32 }
    match columns[2].default_sql { Some(_) => { return 33 } None => {} }
    keys := pkg.db.postgres.meta_keys_native(
      target, child, pkg.db.MetaDetail.Full, out, [], [],
    ) else { return 13 }
    if keys.len() != 4 { return 14 }
    indexes := pkg.db.postgres.meta_indexes_native(
      target, child, pkg.db.MetaDetail.Full, out, [], [],
    ) else { return 15 }
    if indexes.len() != 6 || indexes[0].name != "align_q5b2_child_b"
      || indexes[0].term_ordinal != 0 || indexes[1].term_ordinal != 1 {
      return 16
    }
    if !descending(indexes[0].sort_order) || !nulls_first(indexes[0].null_order) {
      return 38
    }
    hash := indexes[2]
    hash_method := hash.native_method else { return 39 }
    if hash.name != "align_q5b2_child_b_hash" || hash_method != "hash"
      || !no_sort(hash.sort_order) || !no_null_order(hash.null_order) {
      return 40
    }

    pkg.db.q5b2_setup.set_closed(target, true)
    wrong_driver := pkg.db.sqlite.meta_schemas_native(
      target, pkg.db.MetaDetail.Names, out, [], [],
    )
    pkg.db.q5b2_setup.set_closed(target, false)
    if !mismatch(wrong_driver) { return 41 }

    conflict := pkg.db.postgres.meta_table_native(
      target,
      pkg.db.TableRef { schema: "bad\0schema", name: "bad\0name" },
      pkg.db.MetaDetail.Names,
      out,
      [],
      [
        pkg.db.postgres.MetaOption.SearchPathOnly,
        pkg.db.postgres.MetaOption.IncludeSystemCatalogs,
      ],
    )
    if !contract_item(conflict, "postgres.meta.options") { return 17 }

    query := app.pg_inspect.mutate()
    common_plan := pkg.db.explain(
      target, query, app.pg_inspect.Params { probe: 1 }, out, [],
    ) else { return 18 }
    if common_plan.analyzed || common_plan.body.len() == 0 { return 19 }
    before := pkg.db.one(
      target, app.pg_inspect.counter(), app.pg_inspect.Params { probe: 1 }, out, [],
    ) else { return 20 }
    if before.value != 0 { return 21 }

    text_plan := pkg.db.postgres.explain_native(
      target,
      query,
      app.pg_inspect.Params { probe: 1 },
      out,
      [],
      [pkg.db.postgres.ExplainOption.Format(pkg.db.postgres.PlanFormat.Text)],
    ) else { return 29 }
    if text_plan.analyzed || text_plan.body.len() == 0 { return 30 }

    invalid_plan := pkg.db.postgres.explain_native(
      target,
      query,
      app.pg_inspect.Params { probe: 1 },
      out,
      [],
      [pkg.db.postgres.ExplainOption.Buffers(true)],
    )
    if !contract_item(invalid_plan, "postgres.explain.analyze") { return 22 }
    still_before := pkg.db.one(
      target, app.pg_inspect.counter(), app.pg_inspect.Params { probe: 1 }, out, [],
    ) else { return 23 }
    if still_before.value != 0 { return 24 }

    analyzed_plan := pkg.db.postgres.explain_native(
      target,
      query,
      app.pg_inspect.Params { probe: 1 },
      out,
      [],
      [
        pkg.db.postgres.ExplainOption.Analyze,
        pkg.db.postgres.ExplainOption.Format(pkg.db.postgres.PlanFormat.Json),
      ],
    ) else { return 25 }
    if !analyzed_plan.analyzed || !json_plan(analyzed_plan.format)
      || analyzed_plan.body.len() == 0 {
      return 26
    }
    after := pkg.db.one(
      target, app.pg_inspect.counter(), app.pg_inspect.Params { probe: 1 }, out, [],
    ) else { return 27 }
    if after.value != 1 { return 28 }
    return 62
  }
}

fn main(args: array<str>) -> Result<(), Error> {
  print(run(args[1]))
  return Ok(())
}
"#;

const POSTGRES_BRIDGE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import pkg.db.postgres
import pkg.db.q5b2_setup
import pkg.db.bad_normalized_explain
import app.pg_inspect

fn mismatch<T>(result: Result<T, pkg.db.Error>) -> bool = match result {
  Err(error) => match error { DriverMismatch(_) => true _ => false }
  Ok(_) => false
}

fn contract_item<T>(result: Result<T, pkg.db.Error>, expected: str) -> bool = match result {
  Err(error) => match error {
    Unsupported(contract) => contract.item == expected
    _ => false
  }
  Ok(_) => false
}

fn main() -> i32 {
  opened := pkg.db.sqlite.connect(":memory:", [])
  connection := opened else { return 1 }
  target := pkg.db.exec_conn(connection)
  arena out {
    pkg.db.q5b2_setup.set_closed(target, true)
    poisoned_wrong_driver := pkg.db.postgres.meta_schemas_native(
      target, pkg.db.MetaDetail.Names, out, [], [],
    )
    pkg.db.q5b2_setup.set_closed(target, false)
    if !mismatch(poisoned_wrong_driver) { return 7 }
    settings := pkg.db.postgres.explain_native(
      target,
      app.pg_inspect.mutate(),
      app.pg_inspect.Params { probe: 1 },
      out,
      [],
      [pkg.db.postgres.ExplainOption.Settings(true)],
    )
    if !mismatch(settings) { return 2 }
    timing := pkg.db.postgres.explain_native(
      target,
      app.pg_inspect.mutate(),
      app.pg_inspect.Params { probe: 1 },
      out,
      [],
      [pkg.db.postgres.ExplainOption.Timing(false)],
    )
    if !contract_item(timing, "postgres.explain.analyze") { return 3 }
    duplicate_format := pkg.db.postgres.explain_native(
      target,
      app.pg_inspect.mutate(),
      app.pg_inspect.Params { probe: 1 },
      out,
      [],
      [
        pkg.db.postgres.ExplainOption.Format(pkg.db.postgres.PlanFormat.Text),
        pkg.db.postgres.ExplainOption.Format(pkg.db.postgres.PlanFormat.Json),
      ],
    )
    if !contract_item(duplicate_format, "postgres.explain.format") { return 4 }
    duplicate_settings := pkg.db.postgres.explain_native(
      target,
      app.pg_inspect.mutate(),
      app.pg_inspect.Params { probe: 1 },
      out,
      [],
      [
        pkg.db.postgres.ExplainOption.Settings(true),
        pkg.db.postgres.ExplainOption.Settings(false),
      ],
    )
    if !contract_item(duplicate_settings, "postgres.explain.options") { return 5 }
    malformed_normalized := pkg.db.bad_normalized_explain.run(
      target, out,
    )
    if !contract_item(malformed_normalized, "postgres.explain.analyze") { return 6 }
    return 53
  }
}
"#;

fn package_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("pkg/db.align", package_source("pkg/db.align")),
        ("pkg/db/sqlite.align", package_source("pkg/db/sqlite.align")),
        ("pkg/db/postgres.align", package_source("pkg/db/postgres.align")),
        ("pkg/db/internal.align", package_source("pkg/db/internal.align")),
        ("pkg/db/internal/resource.align", package_source("pkg/db/internal/resource.align")),
        ("pkg/db/internal/descriptor.align", package_source("pkg/db/internal/descriptor.align")),
        ("pkg/db/internal/sqlite.align", package_source("pkg/db/internal/sqlite.align")),
        ("pkg/db/internal/postgres.align", package_source("pkg/db/internal/postgres.align")),
        (
            "pkg/db/internal/postgres_status.align",
            package_source("pkg/db/internal/postgres_status.align"),
        ),
        ("pkg/db/q5b2_setup.align", SETUP),
        ("main.align", MAIN),
    ]
}

fn explain_package_files() -> Vec<(&'static str, &'static str)> {
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("app/lookup.align", LOOKUP));
    files.push(("app/bad_lookup.align", BAD_LOOKUP));
    files.push(("main.align", EXPLAIN_MAIN));
    files
}

fn sqlite_native_package_files() -> Vec<(&'static str, &'static str)> {
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("main.align", SQLITE_NATIVE_MAIN));
    files
}

fn sqlite_shape_package_files() -> Vec<(&'static str, &'static str)> {
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("pkg/db/shape_fixture.align", SQLITE_SHAPE_MODULE));
    files.push(("main.align", SQLITE_SHAPE_MAIN));
    files
}

fn postgres_lease_package_files() -> Vec<(&'static str, &'static str)> {
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("pkg/db/lease_fixture.align", POSTGRES_LEASE_MODULE));
    files.push(("main.align", POSTGRES_LEASE_MAIN));
    files
}

fn postgres_status_package_files() -> Vec<(&'static str, &'static str)> {
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("pkg/db/status_fixture.align", POSTGRES_STATUS_MODULE));
    files.push(("main.align", POSTGRES_STATUS_MAIN));
    files
}

fn postgres_package_files() -> Vec<(&'static str, &'static str)> {
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("app/pg_inspect.align", POSTGRES_QUERIES));
    files.push(("main.align", POSTGRES_MAIN));
    files
}

fn postgres_bridge_package_files() -> Vec<(&'static str, &'static str)> {
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("app/pg_inspect.align", POSTGRES_QUERIES));
    files.push((
        "pkg/db/bad_normalized_explain.align",
        BAD_NORMALIZED_EXPLAIN,
    ));
    files.push(("main.align", POSTGRES_BRIDGE_MAIN));
    files
}

#[test]
fn q5b2_publishes_exact_common_and_native_surface() {
    for required in [
        "pub fn meta_database(",
        "pub fn meta_schemas(",
        "pub fn meta_tables(",
        "pub fn meta_table(",
        "pub fn meta_columns(",
        "pub fn meta_keys(",
        "pub fn meta_indexes(",
        "pub fn explain<P, R>(",
    ] {
        assert!(
            package_source("pkg/db.align").contains(required),
            "missing Q5b2 common operation `{required}`"
        );
    }
    for native in [package_source("pkg/db/sqlite.align"), package_source("pkg/db/postgres.align")] {
        for required in [
            "pub fn meta_database_native(",
            "pub fn meta_schemas_native(",
            "pub fn meta_tables_native(",
            "pub fn meta_table_native(",
            "pub fn meta_columns_native(",
            "pub fn meta_keys_native(",
            "pub fn meta_indexes_native(",
            "pub fn explain_native<P, R>(",
        ] {
            assert!(
                native.contains(required),
                "missing native operation `{required}`"
            );
        }
    }
}

#[test]
fn catalog_adapters_require_an_internal_sealed_control() {
    for sealed in [
        "controls: pkg.db.internal.SqliteCatalogControls",
        "controls: pkg.db.internal.PostgresCatalogControls",
    ] {
        assert!(
            package_source("pkg/db.align").contains(sealed),
            "catalog adapter lacks sealed control `{sealed}`"
        );
    }

    let bypass = r#"module main
import pkg.db

fn bypass(
  target: pkg.db.exec,
  out: region,
) -> Result<array<pkg.db.SchemaMeta>, pkg.db.Error> {
  return pkg.db.catalog_sqlite_schemas(target, pkg.db.MetaDetail.Names, out, true)
}

fn main() -> i32 = 0
"#;
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("main.align", bypass));
    let diagnostics =
        check_multi_diagnostics("pkg-db-q5b2-sealed-catalog-adapter", &files, "main.align");
    assert!(
        diagnostics.contains("type mismatch: bool vs pkg.db.internal$SqliteCatalogControls"),
        "a raw boolean must not reach the internal catalog adapter:\n{diagnostics}"
    );
}

#[test]
fn explain_bridge_rejects_wrong_normalized_type_and_arity() {
    let wrong_type = r#"module pkg.db.bad_explain_type
import pkg.db
import pkg.db.internal.descriptor

Params { id: i64 }
Row { id: i64 }

pub fn bad(
  statement: pkg.db.query<Params, Row>,
  target: pkg.db.exec,
  out: region,
) -> Result<pkg.db.QueryPlan, pkg.db.Error> {
  unsafe {
    return pkg.db.internal.descriptor.explain_sqlite_native(
      statement, target, Params { id: 1 }, out, "bad", false,
    )
  }
}
"#;
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("pkg/db/bad_explain_type.align", wrong_type));
    files.push((
        "main.align",
        "module main\nimport pkg.db.bad_explain_type\nfn main() -> i32 = 0\n",
    ));
    let diagnostics =
        check_multi_diagnostics("pkg-db-q5b2-explain-bridge-type", &files, "main.align");
    assert!(
        diagnostics.contains("database EXPLAIN normalized option must be `u8`, got bool"),
        "unexpected diagnostics:\n{diagnostics}"
    );

    let wrong_arity = r#"module pkg.db.bad_explain_arity
import pkg.db
import pkg.db.internal.descriptor

Params { id: i64 }
Row { id: i64 }

pub fn bad(
  statement: pkg.db.query<Params, Row>,
  target: pkg.db.exec,
  out: region,
) -> Result<pkg.db.QueryPlan, pkg.db.Error> {
  unsafe {
    return pkg.db.internal.descriptor.explain_postgres_native(
      statement, target, Params { id: 1 }, out, "bad", 0, 0,
    )
  }
}
"#;
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("pkg/db/bad_explain_arity.align", wrong_arity));
    files.push((
        "main.align",
        "module main\nimport pkg.db.bad_explain_arity\nfn main() -> i32 = 0\n",
    ));
    let diagnostics =
        check_multi_diagnostics("pkg-db-q5b2-explain-bridge-arity", &files, "main.align");
    assert!(
        diagnostics.contains(
            "static descriptor operation 'explain_postgres_native' expects 8 argument(s), got 7"
        ),
        "unexpected diagnostics:\n{diagnostics}"
    );
}

#[test]
fn sqlite_database_schema_table_and_column_projection_is_exact() {
    if !backend_available() {
        return;
    }
    let output = build_and_run_multi("pkg-db-q5b2-sqlite-catalog", &package_files(), "main.align");
    assert_eq!(
        output.status.code(),
        Some(45),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn common_sqlite_explain_is_bound_and_inspection_only() {
    if !backend_available() {
        return;
    }
    let output = build_and_run_multi_with_static_descriptors(
        "pkg-db-q5b2-sqlite-explain",
        &explain_package_files(),
        "main.align",
    );
    assert_eq!(
        output.status.code(),
        Some(42),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sqlite_explain_generic_bridge_links_per_unit() {
    if !backend_available() {
        return;
    }
    let built = build_per_unit_multi(
        "pkg-db-q5b2-sqlite-explain-unit",
        &explain_package_files(),
        "main.align",
    );
    let output = built.link_and_run();
    assert_eq!(
        output.status.code(),
        Some(42),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sqlite_native_option_matrix_and_validation_precedence_are_exact() {
    if !backend_available() {
        return;
    }
    let output = build_and_run_multi(
        "pkg-db-q5b2-sqlite-native",
        &sqlite_native_package_files(),
        "main.align",
    );
    assert_eq!(
        output.status.code(),
        Some(52),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn malformed_native_catalog_rows_close_once_and_sqlite_releases_the_lease() {
    if !backend_available() || !cc_available() {
        return;
    }
    let output = build_and_run_multi_with_c(
        "pkg-db-q5b2-sqlite-shape",
        &sqlite_shape_package_files(),
        "main.align",
        SQLITE_SHAPE_STUB,
    );
    assert_eq!(
        output.status.code(),
        Some(42),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn postgres_catalog_and_explain_share_the_execution_lease() {
    if !backend_available() || !cc_available() {
        return;
    }
    let output = build_and_run_multi_with_c(
        "pkg-db-q5b2-postgres-lease",
        &postgres_lease_package_files(),
        "main.align",
        db_harness::PG.c_source,
    );
    assert_eq!(
        output.status.code(),
        Some(73),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn postgres_package_results_fail_closed_before_followup_native_work() {
    if !backend_available() || !cc_available() {
        return;
    }
    let output = build_and_run_multi_with_c(
        "pkg-db-postgres-status-safety",
        &postgres_status_package_files(),
        "main.align",
        db_harness::PG.c_source,
    );
    assert_eq!(
        output.status.code(),
        Some(200),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn postgres_status_authority_is_package_sealed() {
    let bad = r#"module app.bad_status
import pkg.db.internal.postgres_status

pub fn classify(status: i32) -> bool = pkg.db.internal.postgres_status.must_close(status)
"#;
    let main = "module main\nimport app.bad_status\nfn main() -> i32 = 0\n";
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("app/bad_status.align", bad));
    files.push(("main.align", main));
    let diagnostics = check_multi_diagnostics(
        "pkg-db-postgres-status-authority-sealed",
        &files,
        "main.align",
    );
    assert!(
        diagnostics.contains("cannot import internal module `pkg.db.internal.postgres_status`")
            && diagnostics.contains("only from within `pkg.db`"),
        "the status authority must remain package-sealed:\n{diagnostics}"
    );
}

#[test]
fn postgres_required_catalog_and_explain_contract_is_exact() {
    if !backend_available() {
        return;
    }
    let files = postgres_package_files();
    let diagnostics =
        check_multi_diagnostics("pkg-db-q5b2-postgres-typecheck", &files, "main.align");
    assert!(
        !diagnostics.lines().any(|line| line.contains(": error:")),
        "PostgreSQL Q5b2 fixture must type-check before live execution:\n{diagnostics}"
    );
    let required = std::env::var_os("ALIGN_DB_POSTGRES_REQUIRED").is_some();
    let Some(url) = std::env::var("ALIGN_DB_POSTGRES_URL")
        .ok()
        .filter(|url| !url.is_empty())
    else {
        assert!(
            !required,
            "ALIGN_DB_POSTGRES_URL is required by this test environment"
        );
        eprintln!("skipping PostgreSQL Q5b2 owner: ALIGN_DB_POSTGRES_URL is not set");
        return;
    };
    let output = build_and_run_multi_with_static_descriptors_args_with_env(
        "pkg-db-q5b2-postgres",
        &files,
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
    assert_eq!(String::from_utf8_lossy(&output.stdout), "62\n");
}

#[test]
fn postgres_native_generic_bridge_compiles_and_rejects_wrong_driver() {
    if !backend_available() {
        return;
    }
    let files = postgres_bridge_package_files();
    let output = build_and_run_multi_with_static_descriptors(
        "pkg-db-q5b2-postgres-bridge",
        &files,
        "main.align",
    );
    assert_eq!(
        output.status.code(),
        Some(53),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let built = build_per_unit_multi("pkg-db-q5b2-postgres-bridge-unit", &files, "main.align");
    let output = built.link_and_run();
    assert_eq!(
        output.status.code(),
        Some(53),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
