//! pkg.db Q5b2/D12 native catalog and EXPLAIN owners.

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

const SQLITE_SHAPE_STUB: &str = r#"
#include <stdint.h>
#include <string.h>

static int fake_database;
static int fake_statement;
static int prepare_calls;
static int step_calls;
static int finalize_calls;
static int sqlite_key_query;
static int fake_postgres;
static int fake_result;
static int pq_exec_calls;
static int pq_clear_calls;
static int postgres_key_query;

void *align_q5b2_fake_sqlite(void) { return &fake_database; }
int32_t align_q5b2_finalize_calls(void) { return finalize_calls; }
void *align_q5b2_fake_postgres(void) { return &fake_postgres; }
int32_t align_q5b2_clear_calls(void) { return pq_clear_calls; }

int32_t sqlite3_prepare_v2(
    void *database,
    const char *sql,
    int32_t bytes,
    void **statement_out,
    const char **tail_out) {
  (void)database;
  (void)sql;
  (void)bytes;
  (void)tail_out;
  prepare_calls += 1;
  step_calls = 0;
  sqlite_key_query = strstr(sql, "WITH pk_terms") != 0;
  *statement_out = &fake_statement;
  return 0;
}

int32_t sqlite3_step(void *statement) {
  (void)statement;
  if (prepare_calls == 1 && step_calls == 0) {
    step_calls += 1;
    return 100;
  }
  if (sqlite_key_query && step_calls < 2) {
    step_calls += 1;
    return 100;
  }
  return 101;
}

int32_t sqlite3_column_count(void *statement) {
  (void)statement;
  return sqlite_key_query ? 13 : 2;
}

int32_t sqlite3_bind_text(
    void *statement,
    int32_t index,
    const char *value,
    int32_t bytes,
    void *destructor) {
  (void)statement;
  (void)index;
  (void)value;
  (void)bytes;
  (void)destructor;
  return 0;
}

int32_t sqlite3_column_type(void *statement, int32_t column) {
  (void)statement;
  return column == 0 || column == 2 || column == 3 || column == 12 ? 1 : 5;
}

int64_t sqlite3_column_int64(void *statement, int32_t column) {
  (void)statement;
  if (column == 3) return step_calls == 1 ? 0 : 2;
  if (column == 12) return 1;
  return 0;
}

int32_t sqlite3_finalize(void *statement) {
  (void)statement;
  finalize_calls += 1;
  return 0;
}

int32_t sqlite3_close_v2(void *database) {
  (void)database;
  return 0;
}

void *PQexecParams(
    void *connection,
    const char *command,
    int32_t parameter_count,
    const uint32_t *parameter_types,
    const char *const *parameter_values,
    const int32_t *parameter_lengths,
    const int32_t *parameter_formats,
    int32_t result_format) {
  (void)connection;
  (void)command;
  (void)parameter_count;
  (void)parameter_types;
  (void)parameter_values;
  (void)parameter_lengths;
  (void)parameter_formats;
  (void)result_format;
  pq_exec_calls += 1;
  postgres_key_query = strstr(command, "WITH constraints") != 0;
  return &fake_result;
}
int32_t PQresultStatus(void *result) { (void)result; return 2; }
int32_t PQntuples(void *result) {
  (void)result;
  if (pq_exec_calls == 1) return 1;
  if (postgres_key_query) return 2;
  return 0;
}
int32_t PQnfields(void *result) {
  (void)result;
  if (pq_exec_calls == 1) return 3;
  if (postgres_key_query) return 15;
  return 4;
}
int32_t PQgetisnull(void *result, int32_t row, int32_t column) {
  (void)result; (void)row;
  if (postgres_key_query && (column == 0 || column == 2 || column == 3)) return 0;
  return 1;
}
char *PQgetvalue(void *result, int32_t row, int32_t column) {
  (void)result;
  static char zero[] = "0";
  static char two[] = "2";
  if (postgres_key_query && column == 3 && row == 1) return two;
  return zero;
}
int32_t PQgetlength(void *result, int32_t row, int32_t column) {
  (void)result; (void)row; (void)column;
  return postgres_key_query ? 1 : 0;
}
char *PQcmdTuples(void *result) { (void)result; return 0; }
char *PQerrorMessage(void *connection) { (void)connection; return 0; }
char *PQresultErrorField(void *result, int32_t field) {
  (void)result; (void)field; return 0;
}
void PQclear(void *result) { (void)result; pq_clear_calls += 1; }
void PQfinish(void *connection) { (void)connection; }
"#;

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

pub fn postgres(target: pkg.db.exec) -> bool {
  unsafe {
    state := match target {
      Conn(reference) => resource.raw(reference)
      Tx(_) => { return false }
    }
    connection: raw := raw.load(state, 8)
    sql := c_string("DROP SCHEMA IF EXISTS align_q5b2 CASCADE; DROP SCHEMA IF EXISTS pguser_q5b2 CASCADE; CREATE SCHEMA align_q5b2; CREATE SCHEMA pguser_q5b2; SET search_path = align_q5b2; CREATE TABLE align_q5b2.counter (value BIGINT NOT NULL); INSERT INTO align_q5b2.counter VALUES (0); CREATE TABLE align_q5b2.parent (id BIGINT PRIMARY KEY); CREATE TABLE align_q5b2.child (a BIGINT, b TEXT, generated BIGINT GENERATED ALWAYS AS (a + 1) STORED, PRIMARY KEY (a, b), UNIQUE (b), FOREIGN KEY (a) REFERENCES align_q5b2.parent(id) ON DELETE CASCADE); CREATE INDEX align_q5b2_child_b ON align_q5b2.child (b DESC) INCLUDE (generated)")
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
import pkg.db.postgres
import pkg.db.q5b2_setup
import app.pg_inspect

fn json_plan(value: pkg.db.PlanFormat) -> bool = match value {
  Json => true
  _ => false
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
    if indexes.len() != 5 || indexes[0].name != "align_q5b2_child_b"
      || indexes[0].term_ordinal != 0 || indexes[1].term_ordinal != 1 {
      return 16
    }

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
        ("pkg/db.align", DB),
        ("pkg/db/sqlite.align", SQLITE),
        ("pkg/db/postgres.align", POSTGRES),
        ("pkg/db/internal.align", INTERNAL),
        ("pkg/db/internal/resource.align", RESOURCE),
        ("pkg/db/internal/descriptor.align", DESCRIPTOR),
        ("pkg/db/internal/sqlite.align", INTERNAL_SQLITE),
        ("pkg/db/internal/postgres.align", INTERNAL_POSTGRES),
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
            DB.contains(required),
            "missing Q5b2 common operation `{required}`"
        );
    }
    for native in [SQLITE, POSTGRES] {
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
fn postgres_required_catalog_and_explain_contract_is_exact() {
    if !backend_available() {
        return;
    }
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
        &postgres_package_files(),
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
