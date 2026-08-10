//! pkg.db Q4a/D6+D7 prepared-statement and transaction ownership owners.

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
const SQLITE_PREPARED_STUB: &str = include_str!("fixtures/pkg_db_q4a_sqlite_stub.c");
const PREPARED_BENCH: &str = include_str!("fixtures/pkg_db_q4a_bench.c");

const Q4A_QUERY: &str = r#"module app.q4a_query
import pkg.db
import pkg.db.sqlite

pub Params {
  id: i64,
  label: str,
  payload: slice<u8>,
}

pub Row { id: i64 }

pub TxParams { id: i64 }

pub fn selected() -> pkg.db.query<Params, Row> = pkg.db.sqlite.query(
  "SELECT :id AS id WHERE :label = :label AND :payload = :payload",
  [],
  [],
)

pub fn touched() -> pkg.db.command<TxParams> = pkg.db.sqlite.command(
  "UPDATE q4a_owner SET id = :id",
  [],
  [],
)

pub fn setup() -> pkg.db.command<TxParams> = pkg.db.sqlite.command(
  "CREATE TABLE q4a_owner AS SELECT :id AS id",
  [],
  [],
)
"#;

const Q4A_POSTGRES_QUERY: &str = r#"module app.q4a_postgres_query
import pkg.db
import pkg.db.postgres

pub Params {
  id: i64,
  label: str,
  payload: slice<u8>,
}

pub Row { id: i64 }

pub TxParams { value: i64 }

pub fn selected() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:id AS BIGINT) AS id WHERE :label = :label AND :payload = :payload",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("id", "int8")],
)

pub fn touched() -> pkg.db.command<TxParams> = pkg.db.postgres.command(
  "UPDATE q4a_owner SET value = :value /* COMMAND_OK */",
  [],
  [pkg.db.postgres.CommandOption.ParameterType("value", "int8")],
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
        ("app/q4a_query.align", Q4A_QUERY),
        ("app/q4a_postgres_query.align", Q4A_POSTGRES_QUERY),
        ("main.align", main),
    ]
}

const Q4A_SURFACE_PREFIX: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4a_query

fn execute_once(
  borrow mut statement: pkg.db.stmt<app.q4a_query.Params, app.q4a_query.Row>,
  params: app.q4a_query.Params,
) -> i32 {
  result := pkg.db.rows_stmt(statement, params, [])
  return match result { Ok(rows) => 0, Err(_) => 3 }
}
"#;

#[test]
fn q4a_public_surface_remains_exact_after_q4b_streaming_extension() {
    for required in [
        "pub resource stmt<P, R> = pkg.db.internal.resource.drop_stmt",
        "pub resource rows<R> = pkg.db.internal.resource.drop_rows",
        "pub fn prepare<P, R>(",
        "pub fn rows_stmt<P, R>(",
        "  borrow mut statement: stmt<P, R>,",
        "pub fn next<R>(borrow mut stream: rows<R>) -> Result<Option<R>, Error>",
        "pub fn begin(connection: conn, options: slice<TxOption>) -> Result<tx, Error>",
        "pub fn commit(transaction: tx) -> Result<conn, Error>",
        "pub fn rollback(transaction: tx) -> Result<conn, Error>",
    ] {
        assert!(
            DB.contains(required),
            "missing exact Q4a common surface `{required}`"
        );
    }
    assert!(DB.contains("pub PrepareOption {\n  TimeoutNs(i64)\n}"));
    assert!(DB.contains("pub TxOption {\n  BeginTimeoutNs(i64)\n}"));
    assert!(SQLITE.contains("pub PrepareOption {\n  Persistent\n  Normalize\n}"));
    assert!(SQLITE.contains("pub TxOption {\n  Deferred\n  Immediate\n  Exclusive\n}"));
    assert!(POSTGRES.contains("pub PrepareOption {\n  ParameterOid(str, u32)\n}"));
    assert!(POSTGRES.contains(
        "pub TxOption {\n  Isolation(Isolation)\n  Access(Access)\n  Deferrable(bool)\n}"
    ));
    for deferred in ["pub fn cancel", "pub fn portal", "pub fn statement_cache"] {
        assert!(
            !DB.contains(deferred),
            "Q4a must not publish deferred surface `{deferred}`"
        );
    }
    for (module, source, expected) in [
        ("pkg.db", DB, 2),
        ("pkg.db.internal.sqlite", INTERNAL_SQLITE, 0),
        ("pkg.db.internal.postgres", INTERNAL_POSTGRES, 0),
    ] {
        assert_eq!(
            source
                .matches("tail_reserved: u32 := raw.load(data, 116)")
                .count(),
            expected,
            "{module} must delegate to the two shared v5 header validators"
        );
        assert_eq!(
            source.matches("tail_reserved == 0").count(),
            expected,
            "{module} must not duplicate the v5 tail-reserved check"
        );
    }
    for source in [INTERNAL_SQLITE, INTERNAL_POSTGRES] {
        assert!(source.contains(
            "return pkg.db.command_header_valid(statement, pkg.db.internal.descriptor_header_control())"
        ));
        assert!(source.contains(
            "return pkg.db.query_header_valid(statement, pkg.db.internal.descriptor_header_control())"
        ));
    }
}

#[test]
fn q4b_prepared_next_typechecks_whole_and_per_unit() {
    let main = format!(
        "{Q4A_SURFACE_PREFIX}\n{}",
        r#"
fn consume(
  borrow mut statement: pkg.db.stmt<app.q4a_query.Params, app.q4a_query.Row>,
) -> i32 {
  bytes := [1 as u8, 2 as u8]
  opened := pkg.db.rows_stmt(statement, app.q4a_query.Params {
    id: 7,
    label: "stream",
    payload: bytes[..],
  }, [])
  return match opened {
    Err(_) => 2
    Ok(stream_value) => {
      mut stream := stream_value
      first := pkg.db.next(stream)
      match first {
        Err(_) => 3
        Ok(value) => match value {
          None => 4
          Some(row) => row.id as i32
        }
      }
    }
  }
}

fn main() -> i32 = 0
"#
    );
    let checked = diff_check_multi(
        "pkg-db-q4b-prepared-next-surface",
        &package_files(&main),
        "main.align",
    );
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "unexpected whole-program diagnostics:\n{}\nunexpected per-unit diagnostics:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
}

#[test]
fn q4a_wrong_scope_arity_and_compiler_private_bridges_fail_closed() {
    let wrong_arity = r#"module main
import pkg.db
import app.q4a_query

fn bad(target: pkg.db.exec) -> i32 {
  value := pkg.db.prepare(target, app.q4a_query.selected())
  return 0
}

fn main() -> i32 = 0
"#;
    let diagnostics = check_multi_diagnostics(
        "pkg-db-q4a-prepare-arity",
        &package_files(wrong_arity),
        "main.align",
    );
    assert!(
        diagnostics.contains("expects 3 argument(s), got 2"),
        "unexpected diagnostics:\n{diagnostics}"
    );

    let wrong_scope = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4a_query

fn bad(target: pkg.db.exec) -> i32 {
  value := pkg.db.prepare(
    target,
    app.q4a_query.selected(),
    [pkg.db.sqlite.PrepareOption.Persistent],
  )
  return 0
}

fn main() -> i32 = 0
"#;
    let diagnostics = check_multi_diagnostics(
        "pkg-db-q4a-prepare-option-scope",
        &package_files(wrong_scope),
        "main.align",
    );
    assert!(
        diagnostics.contains("pkg.db.sqlite$PrepareOption")
            && diagnostics.contains("pkg.db$PrepareOption"),
        "unexpected diagnostics:\n{diagnostics}"
    );

    let sealed = r#"module main
import pkg.db
import pkg.db.internal.descriptor
import app.q4a_query

fn bad(
  borrow mut statement: pkg.db.stmt<app.q4a_query.Params, app.q4a_query.Row>,
  params: app.q4a_query.Params,
  context: raw,
) -> i32 {
  unsafe {
    return pkg.db.internal.descriptor.bind_prepared(
      resource.borrow(statement), context, params,
    )
  }
}

fn main() -> i32 = 0
"#;
    let diagnostics = check_multi_diagnostics(
        "pkg-db-q4a-sealed-prepared-binder",
        &package_files(sealed),
        "main.align",
    );
    assert!(
        diagnostics.contains("static descriptor operations are compiler-private to `pkg.db`"),
        "unexpected diagnostics:\n{diagnostics}"
    );

    let header_bypass = r#"module main
import pkg.db
import app.q4a_query

fn bad(statement: pkg.db.query<app.q4a_query.Params, app.q4a_query.Row>) -> bool {
  return pkg.db.query_header_valid(statement, true)
}

fn main() -> i32 = 0
"#;
    let diagnostics = check_multi_diagnostics(
        "pkg-db-q4b-sealed-descriptor-header",
        &package_files(header_bypass),
        "main.align",
    );
    assert!(
        diagnostics.contains("type mismatch: bool vs pkg.db.internal$DescriptorHeaderControl"),
        "application source reached the shared descriptor validator:\n{diagnostics}"
    );
}

#[test]
fn q4a_parent_child_and_transaction_aliases_are_compile_time_errors() {
    let cases = [
        (
            "exec-view-across-begin",
            r#"module main
import pkg.db

fn bad(connection: pkg.db.conn) -> i32 {
  view := pkg.db.exec_conn(connection)
  begun := pkg.db.begin(connection, [])
  return pkg.db.driver_tag(view) as i32
}

fn main() -> i32 = 0
"#,
        ),
        (
            "stmt-parent-move",
            r#"module main
import pkg.db
import app.q4a_query

fn bad(connection: pkg.db.conn) -> i32 {
  made := pkg.db.prepare(pkg.db.exec_conn(connection), app.q4a_query.selected(), [])
  return match made {
    Err(_) => 0
    Ok(statement_value) => {
      mut statement := statement_value
      begun := pkg.db.begin(connection, [])
      bytes := [1 as u8]
      rows := pkg.db.rows_stmt(statement, app.q4a_query.Params {
        id: 1, label: "x", payload: bytes[..],
      }, [])
      0
    }
  }
}

fn main() -> i32 = 0
"#,
        ),
        (
            "second-rows-generation",
            r#"module main
import pkg.db
import app.q4a_query

fn bad(borrow mut statement: pkg.db.stmt<app.q4a_query.Params, app.q4a_query.Row>) -> i32 {
  first_bytes := [1 as u8]
  first := pkg.db.rows_stmt(statement, app.q4a_query.Params {
    id: 1, label: "first", payload: first_bytes[..],
  }, [])
  second_bytes := [2 as u8]
  second := pkg.db.rows_stmt(statement, app.q4a_query.Params {
    id: 2, label: "second", payload: second_bytes[..],
  }, [])
  return match first { Err(_) => 0, Ok(_) => 1 }
}

fn main() -> i32 = 0
"#,
        ),
        (
            "tx-use-after-commit",
            r#"module main
import pkg.db

fn bad(transaction: pkg.db.tx) -> i32 {
  ended := pkg.db.commit(transaction)
  view := pkg.db.exec_tx(transaction)
  return pkg.db.driver_tag(view) as i32
}

fn main() -> i32 = 0
"#,
        ),
        (
            "tx-stmt-across-commit",
            r#"module main
import pkg.db
import app.q4a_query

fn bad(transaction: pkg.db.tx) -> i32 {
  made := pkg.db.prepare(pkg.db.exec_tx(transaction), app.q4a_query.selected(), [])
  return match made {
    Err(_) => 0
    Ok(statement_value) => {
      mut statement := statement_value
      ended := pkg.db.commit(transaction)
      bytes := [1 as u8]
      rows := pkg.db.rows_stmt(statement, app.q4a_query.Params {
        id: 1, label: "x", payload: bytes[..],
      }, [])
      0
    }
  }
}

fn main() -> i32 = 0
"#,
        ),
        (
            "tx-rows-across-rollback",
            r#"module main
import pkg.db
import app.q4a_query

fn bad(transaction: pkg.db.tx) -> i32 {
  made := pkg.db.prepare(pkg.db.exec_tx(transaction), app.q4a_query.selected(), [])
  return match made {
    Err(_) => 0
    Ok(statement_value) => {
      mut statement := statement_value
      bytes := [1 as u8]
      rows := pkg.db.rows_stmt(statement, app.q4a_query.Params {
        id: 1, label: "x", payload: bytes[..],
      }, [])
      ended := pkg.db.rollback(transaction)
      match rows { Err(_) => 0, Ok(_) => 1 }
    }
  }
}

fn main() -> i32 = 0
"#,
        ),
    ];
    for (name, main) in cases {
        assert!(
            check_multi_errs(
                &format!("pkg-db-q4a-{name}"),
                &package_files(main),
                "main.align",
            ),
            "ownership case `{name}` must fail"
        );
    }
}

#[test]
fn prepared_and_transaction_surface_typechecks_whole_and_per_unit() {
    let suffix = r#"
fn prepared_phase(borrow connection: pkg.db.conn) -> i32 {
  prepared := pkg.db.sqlite.prepare_native(
    pkg.db.exec_conn(connection),
    app.q4a_query.selected(),
    [],
    [pkg.db.sqlite.PrepareOption.Persistent, pkg.db.sqlite.PrepareOption.Normalize],
  )
  return match prepared {
    Err(_) => 2
    Ok(statement_value) => {
      mut statement := statement_value
      bytes := [1 as u8, 2 as u8, 3 as u8]
      first := execute_once(statement, app.q4a_query.Params {
        id: 7,
        label: "first",
        payload: bytes[..],
      })
      if first != 0 { return first }
      execute_once(statement, app.q4a_query.Params {
        id: 8,
        label: "second",
        payload: bytes[..],
      })
    }
  }
}

fn transaction_phase(connection: pkg.db.conn) -> i32 {
  begun := pkg.db.sqlite.begin_native(
    connection,
    [],
    [pkg.db.sqlite.TxOption.Immediate],
  )
  return match begun {
    Err(_) => 5
    Ok(transaction) => finish_transaction(transaction)
  }
}

fn finish_transaction(transaction: pkg.db.tx) -> i32 {
  executed := pkg.db.execute(
    pkg.db.exec_tx(transaction),
    app.q4a_query.touched(),
    app.q4a_query.TxParams { id: 1 },
    [],
  )
  match executed {
    Err(_) => { return 6 }
    Ok(_) => {}
  }
  returned := pkg.db.rollback(transaction)
  return match returned { Ok(connection) => 0, Err(_) => 7 }
}

fn run(connection: pkg.db.conn) -> i32 {
  prepared := prepared_phase(connection)
  if prepared != 0 { return prepared }
  return transaction_phase(connection)
}

fn main() -> i32 {
  opened := pkg.db.sqlite.connect(":memory:", [])
  return match opened { Ok(connection) => run(connection), Err(_) => 1 }
}
"#;
    let main = format!("{Q4A_SURFACE_PREFIX}{suffix}");
    let files = package_files(&main);
    let _whole = whole_mir_multi("pkg-db-q4a-surface-whole", &files, "main.align");

    let per_unit = build_per_unit_multi("pkg-db-q4a-surface-unit", &files, "main.align");
    assert!(
        per_unit
            .walk
            .units
            .iter()
            .any(|unit| unit.unit == "pkg.db.internal.sqlite"),
        "prepared SQLite engine must remain in the per-unit closure"
    );
}

#[test]
fn sqlite_prepared_reuse_copies_views_and_closes_each_native_phase_once() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4a_query

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

fn execute_once(
  borrow mut statement: pkg.db.stmt<app.q4a_query.Params, app.q4a_query.Row>,
  id: i64,
  initial_label: str,
) -> i32 {
  mut label := initial_label.clone()
  label_view: str := label
  mut bytes := [1 as u8, 2 as u8, 3 as u8]
  params := app.q4a_query.Params {
    id: id,
    label: label_view,
    payload: bytes[..],
  }
  result := pkg.db.rows_stmt(statement, params, [])
  return match result {
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

fn is_unsupported_item(error: pkg.db.Error, expected: str) -> bool = match error {
  Unsupported(contract) => contract.item == expected
  _ => false
}

fn prepared_phase(borrow connection: pkg.db.conn) -> i32 {
  common_invalid := pkg.db.sqlite.prepare_native(
    pkg.db.exec_conn(connection),
    app.q4a_query.selected(),
    [pkg.db.PrepareOption.TimeoutNs(0)],
    [pkg.db.sqlite.PrepareOption.Persistent, pkg.db.sqlite.PrepareOption.Persistent],
  )
  match common_invalid {
    Ok(_) => { return 10 }
    Err(error) => if !is_unsupported_item(error, "db.prepare.timeout_ns") { return 11 }
  }
  native_duplicate := pkg.db.sqlite.prepare_native(
    pkg.db.exec_conn(connection),
    app.q4a_query.selected(),
    [],
    [pkg.db.sqlite.PrepareOption.Normalize, pkg.db.sqlite.PrepareOption.Normalize],
  )
  match native_duplicate {
    Ok(_) => { return 13 }
    Err(error) => if !is_unsupported_item(error, "sqlite.prepare.option") { return 14 }
  }
  prepared := pkg.db.sqlite.prepare_native(
    pkg.db.exec_conn(connection),
    app.q4a_query.selected(),
    [],
    [pkg.db.sqlite.PrepareOption.Persistent, pkg.db.sqlite.PrepareOption.Normalize],
  )
  return match prepared {
    Err(_) => 4
    Ok(statement_value) => {
      mut statement := statement_value
      first := execute_once(statement, 7, "first")
      if first != 0 { return first }
      execute_once(statement, 8, "second")
    }
  }
}

fn main() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  opened := pkg.db.sqlite.connect(":memory:", [])
  result := match opened {
    Err(_) => 5
    Ok(connection) => prepared_phase(connection)
  }
  if result != 0 { return result }
  return unsafe {
    if align_sqlite_q4a_protocol_ok() == 1
      && align_sqlite_q4a_prepare_calls() == 1
      && align_sqlite_q4a_bind_i64_calls() == 2
      && align_sqlite_q4a_bind_text_calls() == 2
      && align_sqlite_q4a_bind_blob_calls() == 2
      && align_sqlite_q4a_reset_calls() == 2
      && align_sqlite_q4a_clear_calls() == 2
      && align_sqlite_q4a_finalize_calls() == 1 { 42 } else { 6 }
    }
}
"#;
    let files = package_files(main);
    let fixture = format!("{POSTGRES_STUB}\n{SQLITE_PREPARED_STUB}");
    let output = build_and_run_multi_with_c(
        "pkg-db-q4a-sqlite-prepared-reuse",
        &files,
        "main.align",
        &fixture,
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
fn sqlite_newly_supported_prepared_shape_reaches_native_prepare() {
    if !backend_available() || !cc_available() {
        return;
    }
    let unsupported = r#"module app.q4a_unsupported
import pkg.db
import pkg.db.sqlite

pub Params { enabled: bool }
pub Row { value: i64 }

pub fn selected() -> pkg.db.query<Params, Row> = pkg.db.sqlite.query(
  "SELECT :enabled AS value",
  [],
  [],
)
"#;
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4a_unsupported

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_prepare_calls() -> i32
  fn align_sqlite_q4a_protocol_ok() -> i32
}

fn main() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  opened := pkg.db.sqlite.connect(":memory:", [])
  prepared_ok := match opened {
    Err(_) => false
    Ok(connection) => {
      prepared := pkg.db.sqlite.prepare_native(
        pkg.db.exec_conn(connection),
        app.q4a_unsupported.selected(),
        [],
        [pkg.db.sqlite.PrepareOption.Persistent, pkg.db.sqlite.PrepareOption.Normalize],
      )
      match prepared { Ok(_) => true, Err(_) => false }
    }
  }
  return unsafe {
    if prepared_ok && align_sqlite_q4a_protocol_ok() == 1
      && align_sqlite_q4a_prepare_calls() == 1 { 42 } else { 1 }
  }
}
"#;
    let mut files = package_files(main);
    files.push(("app/q4a_unsupported.align", unsupported));
    let fixture = format!("{POSTGRES_STUB}\n{SQLITE_PREPARED_STUB}");
    let output = build_and_run_multi_with_c(
        "pkg-db-q4a-sqlite-unsupported-shape",
        &files,
        "main.align",
        &fixture,
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
fn sqlite_partial_bind_failure_cleans_native_state_and_statement_reuses() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4a_query

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_fail_next_text()
  fn align_sqlite_q4a_bind_i64_calls() -> i32
  fn align_sqlite_q4a_bind_text_calls() -> i32
  fn align_sqlite_q4a_bind_blob_calls() -> i32
  fn align_sqlite_q4a_reset_calls() -> i32
  fn align_sqlite_q4a_clear_calls() -> i32
  fn align_sqlite_q4a_finalize_calls() -> i32
  fn align_sqlite_q4a_protocol_ok() -> i32
}

fn run(borrow connection: pkg.db.conn) -> i32 {
  prepared := pkg.db.sqlite.prepare_native(
    pkg.db.exec_conn(connection),
    app.q4a_query.selected(),
    [],
    [pkg.db.sqlite.PrepareOption.Persistent, pkg.db.sqlite.PrepareOption.Normalize],
  )
  return match prepared {
    Err(_) => 1
    Ok(statement_value) => {
      mut statement := statement_value
      first_bytes := [1 as u8, 2 as u8, 3 as u8]
      unsafe { align_sqlite_q4a_fail_next_text() }
      failed := pkg.db.rows_stmt(statement, app.q4a_query.Params {
        id: 7,
        label: "first",
        payload: first_bytes[..],
      }, [])
      match failed { Ok(_) => { return 2 }, Err(_) => {} }
      second_bytes := [1 as u8, 2 as u8, 3 as u8]
      succeeded := pkg.db.rows_stmt(statement, app.q4a_query.Params {
        id: 8,
        label: "second",
        payload: second_bytes[..],
      }, [])
      match succeeded { Err(_) => 3, Ok(_) => 0 }
    }
  }
}

fn main() -> i32 {
  unsafe { align_sqlite_q4a_reset() }
  opened := pkg.db.sqlite.connect(":memory:", [])
  result := match opened { Err(_) => 4, Ok(connection) => run(connection) }
  if result != 0 { return result }
  return unsafe {
    if align_sqlite_q4a_protocol_ok() == 1
      && align_sqlite_q4a_bind_i64_calls() == 2
      && align_sqlite_q4a_bind_text_calls() == 2
      && align_sqlite_q4a_bind_blob_calls() == 1
      && align_sqlite_q4a_reset_calls() == 2
      && align_sqlite_q4a_clear_calls() == 2
      && align_sqlite_q4a_finalize_calls() == 1 { 42 } else { 5 }
  }
}
"#;
    let files = package_files(main);
    let fixture = format!("{POSTGRES_STUB}\n{SQLITE_PREPARED_STUB}");
    let output = build_and_run_multi_with_c(
        "pkg-db-q4a-sqlite-bind-recovery",
        &files,
        "main.align",
        &fixture,
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
fn sqlite_rows_cleanup_failure_poisons_the_parent_before_statement_reuse() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4a_query

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_fail_next_reset()
  fn align_sqlite_q4a_prepare_calls() -> i32
  fn align_sqlite_q4a_bind_i64_calls() -> i32
  fn align_sqlite_q4a_reset_calls() -> i32
  fn align_sqlite_q4a_clear_calls() -> i32
  fn align_sqlite_q4a_finalize_calls() -> i32
  fn align_sqlite_q4a_protocol_ok() -> i32
}

fn execute_once(
  borrow mut statement: pkg.db.stmt<app.q4a_query.Params, app.q4a_query.Row>,
  id: i64,
  initial_label: str,
) -> i32 {
  mut label := initial_label.clone()
  label_view: str := label
  mut bytes := [1 as u8, 2 as u8, 3 as u8]
  result := pkg.db.rows_stmt(statement, app.q4a_query.Params {
    id: id, label: label_view, payload: bytes[..],
  }, [])
  return match result {
    Err(_) => 2
    Ok(rows) => {
      label = "source storage replaced".clone()
      bytes[0] = 9
      if label.len() == 0 || bytes[0] != 9 { return 6 }
      0
    }
  }
}

fn run(borrow connection: pkg.db.conn) -> i32 {
  made := pkg.db.sqlite.prepare_native(
    pkg.db.exec_conn(connection),
    app.q4a_query.selected(),
    [],
    [pkg.db.sqlite.PrepareOption.Persistent, pkg.db.sqlite.PrepareOption.Normalize],
  )
  return match made {
    Err(_) => 1
    Ok(statement_value) => {
      mut statement := statement_value
      first := execute_once(statement, 7, "first")
      if first != 0 { return first }
      second := execute_once(statement, 8, "second")
      if second == 2 { 0 } else { 3 }
    }
  }
}

fn main() -> i32 {
  unsafe {
    align_sqlite_q4a_reset()
    align_sqlite_q4a_fail_next_reset()
  }
  opened := pkg.db.sqlite.connect(":memory:", [])
  result := match opened { Err(_) => 4, Ok(connection) => run(connection) }
  if result != 0 { return result }
  return unsafe {
    if align_sqlite_q4a_protocol_ok() == 1
      && align_sqlite_q4a_prepare_calls() == 1
      && align_sqlite_q4a_bind_i64_calls() == 1
      && align_sqlite_q4a_reset_calls() == 1
      && align_sqlite_q4a_clear_calls() == 1
      && align_sqlite_q4a_finalize_calls() == 1 { 42 } else { 5 }
  }
}
"#;
    let files = package_files(main);
    let fixture = format!("{POSTGRES_STUB}\n{SQLITE_PREPARED_STUB}");
    let output = build_and_run_multi_with_c(
        "pkg-db-q4a-sqlite-cleanup-poison",
        &files,
        "main.align",
        &fixture,
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
fn sqlite_transactions_execute_through_common_views_and_cover_all_begin_modes() {
    if !backend_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4a_query

fn exclusive(connection: pkg.db.conn) -> i32 {
  begun_exclusive := pkg.db.sqlite.begin_native(
    connection, [], [pkg.db.sqlite.TxOption.Exclusive],
  )
  return match begun_exclusive {
    Err(_) => 8
    Ok(transaction) => match pkg.db.rollback(transaction) {
      Err(_) => 9
      Ok(connection_value) => 42
    }
  }
}

fn immediate(connection: pkg.db.conn) -> i32 {
  begun_immediate := pkg.db.sqlite.begin_native(
    connection, [], [pkg.db.sqlite.TxOption.Immediate],
  )
  return match begun_immediate {
    Err(_) => 6
    Ok(transaction) => match pkg.db.rollback(transaction) {
      Err(_) => 7
      Ok(connection_value) => exclusive(connection_value)
    }
  }
}

fn use_transaction(transaction: pkg.db.tx) -> i32 {
  updated := pkg.db.execute(
    pkg.db.exec_tx(transaction),
    app.q4a_query.touched(),
    app.q4a_query.TxParams { id: 9 },
    [],
  )
  match updated { Err(_) => { return 3 }, Ok(_) => {} }
  arena metadata_out {
    metadata := pkg.db.meta_database(
      pkg.db.exec_tx(transaction), pkg.db.MetaDetail.Names, metadata_out, [],
    )
    match metadata { Err(_) => { return 12 }, Ok(_) => {} }
  }
  arena plan_out {
    plan_bytes := [1 as u8, 2 as u8, 3 as u8]
    explained := pkg.db.explain(
      pkg.db.exec_tx(transaction),
      app.q4a_query.selected(),
      app.q4a_query.Params { id: 9, label: "tx", payload: plan_bytes[..] },
      plan_out,
      [],
    )
    match explained { Err(_) => { return 13 }, Ok(_) => {} }
  }
  arena out {
    bytes := [1 as u8, 2 as u8, 3 as u8]
    selected := pkg.db.one(
      pkg.db.exec_tx(transaction),
      app.q4a_query.selected(),
      app.q4a_query.Params { id: 9, label: "tx", payload: bytes[..] },
      out,
      [],
    )
    match selected {
      Err(_) => { return 4 }
      Ok(row) => if row.id != 9 { return 5 }
    }
  }
  ended := pkg.db.commit(transaction)
  return match ended { Err(_) => 10, Ok(connection) => immediate(connection) }
}

fn run(connection: pkg.db.conn) -> i32 {
  setup := pkg.db.execute(
    pkg.db.exec_conn(connection),
    app.q4a_query.setup(),
    app.q4a_query.TxParams { id: 0 },
    [],
  )
  match setup { Err(_) => { return 1 }, Ok(_) => {} }
  begun_common := pkg.db.begin(connection, [])
  return match begun_common { Err(_) => 2, Ok(transaction) => use_transaction(transaction) }
}

fn main() -> i32 {
  opened := pkg.db.sqlite.connect(":memory:", [])
  return match opened { Err(_) => 11, Ok(connection) => run(connection) }
}
"#;
    let files = package_files(main);
    let output = build_and_run_multi_with_c(
        "pkg-db-q4a-sqlite-transactions",
        &files,
        "main.align",
        POSTGRES_STUB,
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
fn postgres_prepared_reuse_and_transactions_share_the_connection_lifecycle() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.q4a_postgres_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_prepare_calls() -> i32
  fn align_pg_execute_prepared_calls() -> i32
  fn align_pg_execute_calls() -> i32
  fn align_pg_control_calls() -> i32
  fn align_pg_deallocate_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn execute_once(
  borrow mut statement: pkg.db.stmt<
    app.q4a_postgres_query.Params,
    app.q4a_postgres_query.Row,
  >,
  id: i64,
  label: str,
) -> i32 {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  result := pkg.db.rows_stmt(statement, app.q4a_postgres_query.Params {
    id: id,
    label: label,
    payload: bytes[..],
  }, [])
  return match result { Err(_) => 1, Ok(_) => 0 }
}

fn common_prepared_phase(borrow connection: pkg.db.conn) -> i32 {
  prepared := pkg.db.prepare(
    pkg.db.exec_conn(connection),
    app.q4a_postgres_query.selected(),
    [],
  )
  return match prepared {
    Err(_) => 10
    Ok(statement_value) => {
      mut statement := statement_value
      execute_once(statement, 7, "first")
    }
  }
}

fn prepared_phase(borrow connection: pkg.db.conn) -> i32 {
  common := common_prepared_phase(connection)
  if common != 0 { return common }
  prepared := pkg.db.postgres.prepare_native(
    pkg.db.exec_conn(connection),
    app.q4a_postgres_query.selected(),
    [],
    [
      pkg.db.postgres.PrepareOption.ParameterOid("id", 20 as u32),
      pkg.db.postgres.PrepareOption.ParameterOid("label", 25 as u32),
      pkg.db.postgres.PrepareOption.ParameterOid("payload", 17 as u32),
    ],
  )
  return match prepared {
    Err(_) => 2
    Ok(statement_value) => {
      mut statement := statement_value
      first := execute_once(statement, 7, "first")
      if first != 0 { return first }
      execute_once(statement, 8, "second")
    }
  }
}

fn common_end(connection: pkg.db.conn) -> i32 {
  begun := pkg.db.begin(connection, [])
  return match begun {
    Err(_) => 7
    Ok(transaction) => match pkg.db.rollback(transaction) {
      Err(_) => 8
      Ok(connection_value) => 0
    }
  }
}

fn transaction_phase(connection: pkg.db.conn) -> i32 {
  begun := pkg.db.postgres.begin_native(
    connection,
    [],
    [
      pkg.db.postgres.TxOption.Isolation(pkg.db.postgres.Isolation.Serializable),
      pkg.db.postgres.TxOption.Access(pkg.db.postgres.Access.ReadOnly),
      pkg.db.postgres.TxOption.Deferrable(true),
    ],
  )
  return match begun {
    Err(_) => 3
    Ok(transaction) => {
      executed := pkg.db.execute(
        pkg.db.exec_tx(transaction),
        app.q4a_postgres_query.touched(),
        app.q4a_postgres_query.TxParams { value: 9 },
        [],
      )
      match executed { Err(_) => { return 4 }, Ok(_) => {} }
      ended := pkg.db.commit(transaction)
      match ended { Err(_) => 5, Ok(connection_value) => common_end(connection_value) }
    }
  }
}

fn run(connection: pkg.db.conn) -> i32 {
  prepared := prepared_phase(connection)
  if prepared != 0 { return prepared }
  return transaction_phase(connection)
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  opened := pkg.db.postgres.connect("postgresql://stub/q4a", [])
  result := match opened { Err(_) => 6, Ok(connection) => run(connection) }
  if result != 0 { return result }
  return unsafe {
    if align_pg_protocol_ok() == 1
      && align_pg_prepare_calls() == 2
      && align_pg_execute_prepared_calls() == 3
      && align_pg_execute_calls() == 1
      && align_pg_control_calls() == 4
      && align_pg_deallocate_calls() == 2
      && align_pg_finish_calls() == 1 { 42 } else { 9 }
  }
}
"#;
    let files = package_files(main);
    let output = build_and_run_multi_with_c(
        "pkg-db-q4a-postgres-prepared-transactions",
        &files,
        "main.align",
        POSTGRES_STUB,
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
fn postgres_prepare_and_transaction_options_fail_before_native_work() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.q4a_postgres_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_prepare_calls() -> i32
  fn align_pg_control_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn prepare_errors(borrow connection: pkg.db.conn) -> bool {
  zero := pkg.db.postgres.prepare_native(
    pkg.db.exec_conn(connection),
    app.q4a_postgres_query.selected(),
    [],
    [pkg.db.postgres.PrepareOption.ParameterOid("id", 0 as u32)],
  )
  match zero { Ok(_) => { return false }, Err(_) => {} }
  unknown := pkg.db.postgres.prepare_native(
    pkg.db.exec_conn(connection),
    app.q4a_postgres_query.selected(),
    [],
    [pkg.db.postgres.PrepareOption.ParameterOid("missing", 20 as u32)],
  )
  match unknown { Ok(_) => { return false }, Err(_) => {} }
  duplicate := pkg.db.postgres.prepare_native(
    pkg.db.exec_conn(connection),
    app.q4a_postgres_query.selected(),
    [],
    [
      pkg.db.postgres.PrepareOption.ParameterOid("id", 20 as u32),
      pkg.db.postgres.PrepareOption.ParameterOid("id", 21 as u32),
    ],
  )
  return match duplicate { Ok(_) => false, Err(_) => true }
}

fn common_precedes_native(error: pkg.db.Error) -> bool = match error {
  Unsupported(contract) => contract.item == "db.transaction.begin_timeout_ns"
  _ => false
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  opened := pkg.db.postgres.connect("postgresql://stub/q4a", [])
  native_checked := match opened {
    Err(_) => false
    Ok(connection) => {
      if !prepare_errors(connection) { return 1 }
      begun := pkg.db.postgres.begin_native(
        connection,
        [],
        [pkg.db.postgres.TxOption.Deferrable(true)],
      )
      match begun { Ok(_) => false, Err(_) => true }
    }
  }
  if !native_checked { return 2 }
  second_opened := pkg.db.postgres.connect("postgresql://stub/q4a", [])
  common_checked := match second_opened {
    Err(_) => false
    Ok(second_connection) => {
      second_begun := pkg.db.postgres.begin_native(
        second_connection,
        [pkg.db.TxOption.BeginTimeoutNs(0)],
        [pkg.db.postgres.TxOption.Deferrable(true)],
      )
      match second_begun { Ok(_) => false, Err(error) => common_precedes_native(error) }
    }
  }
  if !common_checked { return 3 }
  return unsafe {
    if align_pg_protocol_ok() == 1
      && align_pg_prepare_calls() == 0
      && align_pg_control_calls() == 0
      && align_pg_finish_calls() == 2 { 42 } else { 4 }
  }
}
"#;
    let files = package_files(main);
    let output = build_and_run_multi_with_c(
        "pkg-db-q4a-postgres-option-preflight",
        &files,
        "main.align",
        POSTGRES_STUB,
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
fn failed_postgres_commit_drops_through_rollback_then_closes_once() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres

extern "C" {
  fn align_pg_reset()
  fn align_pg_fail_next_control()
  fn align_pg_control_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn fail_commit(transaction: pkg.db.tx) -> bool {
  unsafe { align_pg_fail_next_control() }
  ended := pkg.db.commit(transaction)
  return match ended { Ok(_) => false, Err(_) => true }
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  opened := pkg.db.postgres.connect("postgresql://stub/q4a", [])
  failed := match opened {
    Err(_) => false
    Ok(connection) => match pkg.db.begin(connection, []) {
      Err(_) => false
      Ok(transaction) => fail_commit(transaction)
    }
  }
  if !failed { return 1 }
  return unsafe {
    if align_pg_protocol_ok() == 1
      && align_pg_control_calls() == 3
      && align_pg_finish_calls() == 1 { 42 } else { 2 }
  }
}
"#;
    let files = package_files(main);
    let output = build_and_run_multi_with_c(
        "pkg-db-q4a-postgres-failed-commit",
        &files,
        "main.align",
        POSTGRES_STUB,
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
fn postgres_implicit_rollback_command_tag_never_returns_a_connection() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres

extern "C" {
  fn align_pg_reset()
  fn align_pg_rollback_next_commit()
  fn align_pg_control_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn commit_aborted(transaction: pkg.db.tx) -> bool {
  unsafe { align_pg_rollback_next_commit() }
  ended := pkg.db.commit(transaction)
  return match ended {
    Ok(_) => false
    Err(error) => match error { Native(_) => true, _ => false }
  }
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  opened := pkg.db.postgres.connect("postgresql://stub/q4a", [])
  rejected := match opened {
    Err(_) => false
    Ok(connection) => match pkg.db.begin(connection, []) {
      Err(_) => false
      Ok(transaction) => commit_aborted(transaction)
    }
  }
  if !rejected { return 1 }
  return unsafe {
    if align_pg_protocol_ok() == 1
      && align_pg_control_calls() == 3
      && align_pg_finish_calls() == 1 { 42 } else { 2 }
  }
}
"#;
    let files = package_files(main);
    let output = build_and_run_multi_with_c(
        "pkg-db-q4a-postgres-implicit-rollback",
        &files,
        "main.align",
        POSTGRES_STUB,
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
fn failed_postgres_rollback_retries_fail_safe_rollback_then_closes_once() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres

extern "C" {
  fn align_pg_reset()
  fn align_pg_fail_next_control()
  fn align_pg_control_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_protocol_ok() -> i32
}

fn fail_rollback(transaction: pkg.db.tx) -> bool {
  unsafe { align_pg_fail_next_control() }
  ended := pkg.db.rollback(transaction)
  return match ended { Ok(_) => false, Err(_) => true }
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  opened := pkg.db.postgres.connect("postgresql://stub/q4a", [])
  failed := match opened {
    Err(_) => false
    Ok(connection) => match pkg.db.begin(connection, []) {
      Err(_) => false
      Ok(transaction) => fail_rollback(transaction)
    }
  }
  if !failed { return 1 }
  return unsafe {
    if align_pg_protocol_ok() == 1
      && align_pg_control_calls() == 3
      && align_pg_finish_calls() == 1 { 42 } else { 2 }
  }
}
"#;
    let files = package_files(main);
    let output = build_and_run_multi_with_c(
        "pkg-db-q4a-postgres-failed-rollback",
        &files,
        "main.align",
        POSTGRES_STUB,
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
#[ignore = "local comparative measurement; not a correctness or CI gate"]
fn prepared_execution_comparison_benchmark_reports_three_independent_rows() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.q4a_query
import std.time

extern "C" {
  fn align_sqlite_q4a_direct_prepared_bench(iterations: i32) -> i64
}

fn execute_prepared_once(
  borrow mut statement: pkg.db.stmt<app.q4a_query.Params, app.q4a_query.Row>,
  id: i64,
) -> Result<(), pkg.db.Error> {
  bytes := [1 as u8, 2 as u8, 3 as u8]
  rows := pkg.db.rows_stmt(statement, app.q4a_query.Params {
    id: id, label: "bench", payload: bytes[..],
  }, [])?
  return Ok(())
}

fn common_prepared_bench(borrow connection: pkg.db.conn, iterations: i64) -> Result<i64, pkg.db.Error> {
  made := pkg.db.prepare(pkg.db.exec_conn(connection), app.q4a_query.selected(), [])?
  mut statement := made
  start := time.instant()
  mut index: i64 := 0
  loop {
    if index >= iterations { break }
    execute_prepared_once(statement, index)?
    index = index + 1
  }
  return Ok(time.instant() - start)
}

fn reprepare_once(borrow connection: pkg.db.conn) -> Result<(), pkg.db.Error> {
  statement := pkg.db.prepare(pkg.db.exec_conn(connection), app.q4a_query.selected(), [])?
  return Ok(())
}

fn reprepare_bench(borrow connection: pkg.db.conn, iterations: i64) -> Result<i64, pkg.db.Error> {
  start := time.instant()
  mut index: i64 := 0
  loop {
    if index >= iterations { break }
    reprepare_once(connection)?
    index = index + 1
  }
  return Ok(time.instant() - start)
}

fn main() -> i32 {
  iterations: i64 := 1000
  direct := unsafe { align_sqlite_q4a_direct_prepared_bench(iterations as i32) }
  if direct < 0 { return 1 }
  opened := pkg.db.sqlite.connect(":memory:", [])
  return match opened {
    Err(_) => 2
    Ok(connection) => {
      common := common_prepared_bench(connection, iterations)
      match common {
        Err(_) => 3
        Ok(common_ns) => {
          reprepared := reprepare_bench(connection, iterations)
          match reprepared {
            Err(_) => 4
            Ok(reprepare_ns) => {
              print(direct)
              print(common_ns)
              print(reprepare_ns)
              0
            }
          }
        }
      }
    }
  }
}
"#;
    let files = package_files(main);
    let fixture = format!("{POSTGRES_STUB}\n{PREPARED_BENCH}");
    let output = build_and_run_multi_with_c(
        "pkg-db-q4a-prepared-benchmark",
        &files,
        "main.align",
        &fixture,
    );
    assert!(
        output.status.success(),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let totals = String::from_utf8(output.stdout)
        .expect("benchmark output UTF-8")
        .lines()
        .map(|line| line.parse::<u64>().expect("benchmark nanoseconds"))
        .collect::<Vec<_>>();
    assert_eq!(totals.len(), 3, "one total per benchmark row");
    const ITERATIONS: f64 = 1000.0;
    println!(
        "sqlite-direct-prepared\t{:.2}\tns/op",
        totals[0] as f64 / ITERATIONS
    );
    println!(
        "pkg-db-prepared-common\t{:.2}\tns/op",
        totals[1] as f64 / ITERATIONS
    );
    println!(
        "pkg-db-reprepare\t{:.2}\tns/op",
        totals[2] as f64 / ITERATIONS
    );
}
