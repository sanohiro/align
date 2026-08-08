//! pkg.db Q2/D2+D4 native scalar execution owners.

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
        ("main.align", main),
    ]
}

#[test]
fn sqlite_connect_mir_retains_private_native_helpers() {
    let main = r#"module main
import pkg.db.sqlite

fn main() -> i32 {
  result := pkg.db.sqlite.connect(":memory:", [])
  return match result { Ok(_) => 42, Err(_) => 1 }
}
"#;
    let files = package_files(main);
    let whole = whole_mir_multi("pkg-db-q2-sqlite-helper-whole", &files, "main.align");
    assert!(whole.contains("fn pkg.db.sqlite$c_string("), "{whole}");
    assert!(
        whole.contains("call program pkg.db.sqlite$c_string("),
        "{whole}"
    );

    let per_unit = build_per_unit_multi("pkg-db-q2-sqlite-helper-unit", &files, "main.align");
    let sqlite = align_mir::print::program_to_string(&per_unit.unit("pkg.db.sqlite").mir);
    assert!(sqlite.contains("fn pkg.db.sqlite$c_string("), "{sqlite}");
    assert!(
        sqlite.contains("call program pkg.db.sqlite$c_string("),
        "{sqlite}"
    );
}

#[test]
fn sqlite_connect_configures_and_drops_one_native_connection() {
    if !backend_available() {
        return;
    }
    let main = r#"module main
import pkg.db.sqlite

fn main() -> i32 {
  result := pkg.db.sqlite.connect(":memory:", [
    pkg.db.sqlite.ConnectOption.OpenReadWrite,
    pkg.db.sqlite.ConnectOption.Create,
    pkg.db.sqlite.ConnectOption.BusyTimeoutNs(1),
    pkg.db.sqlite.ConnectOption.Pragma("foreign_keys", "ON"),
    pkg.db.sqlite.ConnectOption.Pragma("application_id", "42"),
  ])
  return match result {
    Ok(connection) => 42
    Err(_) => 1
  }
}
"#;
    let files = package_files(main);
    let output = build_and_run_multi("pkg-db-q2-sqlite-connect", &files, "main.align");
    assert_eq!(
        output.status.code(),
        Some(42),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sqlite_connect_rejects_invalid_input_before_open() {
    if !backend_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite

fn is_unsupported(result: Result<pkg.db.conn, pkg.db.Error>) -> bool = match result {
  Ok(connection) => false
  Err(error) => match error { Unsupported(_) => true, _ => false }
}

fn main() -> i32 {
  bad_timeout := is_unsupported(pkg.db.sqlite.connect(":memory:", [
    pkg.db.sqlite.ConnectOption.BusyTimeoutNs(0),
  ]))
  conflicting_mode := is_unsupported(pkg.db.sqlite.connect(":memory:", [
    pkg.db.sqlite.ConnectOption.OpenReadOnly,
    pkg.db.sqlite.ConnectOption.Create,
  ]))
  duplicate_pragma := is_unsupported(pkg.db.sqlite.connect(":memory:", [
    pkg.db.sqlite.ConnectOption.Pragma("foreign_keys", "ON"),
    pkg.db.sqlite.ConnectOption.Pragma("FOREIGN_KEYS", "OFF"),
  ]))
  invalid_pragma := is_unsupported(pkg.db.sqlite.connect(":memory:", [
    pkg.db.sqlite.ConnectOption.Pragma("bad-name", "ON"),
  ]))
  if bad_timeout && conflicting_mode && duplicate_pragma && invalid_pragma { return 42 }
  return 1
}
"#;
    let files = package_files(main);
    let output = build_and_run_multi("pkg-db-q2-sqlite-invalid-connect", &files, "main.align");
    assert_eq!(
        output.status.code(),
        Some(42),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn postgres_connect_rejects_embedded_nul_before_libpq() {
    if !backend_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres

fn main() -> i32 {
  result := pkg.db.postgres.connect("postgresql://local\0host/db", [])
  return match result {
    Err(error) => match error {
      Encode(contract) => if contract.item == "postgres.connect.url" { 42 } else { 2 }
      _ => 3
    }
    Ok(_) => 1
  }
}
"#;
    let files = package_files(main);
    let output = build_and_run_multi("pkg-db-q2-postgres-nul", &files, "main.align");
    assert_eq!(
        output.status.code(),
        Some(42),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sqlite_native_command_and_one_execute_generated_i64_thunks() {
    if !backend_available() {
        return;
    }
    const SETUP: &str = r#"module app.setup
import pkg.db
import pkg.db.sqlite

pub Params { marker: i64 }

pub fn command() -> pkg.db.command<Params> =
  pkg.db.sqlite.command("CREATE TABLE items AS SELECT :marker AS value WHERE 0", [], [])
"#;
    const READ: &str = r#"module app.read
import pkg.db
import pkg.db.sqlite

pub Params { value: i64 }
pub Row { value: i64 }

pub fn query() -> pkg.db.query<Params, Row> =
  pkg.db.sqlite.query("SELECT CAST(:value AS INTEGER) AS value", [], [])
"#;
    const ITEMS: &str = r#"module app.items
import pkg.db
import pkg.db.sqlite

pub Params { value: i64 }
pub Row { value: i64 }

pub fn insert() -> pkg.db.command<Params> =
  pkg.db.sqlite.command("INSERT INTO items(value) VALUES (:value), (:value)", [], [])

pub fn query() -> pkg.db.query<Params, Row> =
  pkg.db.sqlite.query("SELECT value AS value FROM items WHERE value = :value", [], [])
"#;
    const MALFORMED: &str = r#"module app.malformed
import pkg.db
import pkg.db.sqlite

pub Params { marker: i64 }
pub Row { value: i64 }

pub fn setup() -> pkg.db.command<Params> = pkg.db.sqlite.command(
  "CREATE TABLE malformed_items AS SELECT :marker AS marker, 'bad' AS value WHERE 0",
  [],
  [],
)

pub fn insert() -> pkg.db.command<Params> = pkg.db.sqlite.command(
  "INSERT INTO malformed_items(marker, value) VALUES (:marker, 'bad'), (:marker, :marker)",
  [],
  [],
)

pub fn bad_first() -> pkg.db.query<Params, Row> = pkg.db.sqlite.query(
  "SELECT value AS value FROM malformed_items WHERE marker = :marker ORDER BY rowid",
  [],
  [],
)

pub fn valid_first() -> pkg.db.query<Params, Row> = pkg.db.sqlite.query(
  "SELECT value AS value FROM malformed_items WHERE marker = :marker ORDER BY rowid DESC",
  [],
  [],
)
"#;
    const TIMEOUT: &str = r#"module app.timeout
import pkg.db
import pkg.db.sqlite

pub Params { marker: i64 }
pub Row { timeout: i64 }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.sqlite.query(
  "SELECT timeout AS timeout FROM pragma_busy_timeout WHERE :marker = :marker",
  [],
  [],
)
"#;
    const INVALID: &str = r#"module app.invalid
import pkg.db
import pkg.db.sqlite

pub Params { value: i64 }
pub Row { value: i64 }

pub fn row_command() -> pkg.db.command<Params> = pkg.db.sqlite.command(
  "SELECT :value AS value",
  [],
  [],
)
"#;
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.setup
import app.read
import app.items
import app.malformed
import app.timeout
import app.invalid

fn main() -> i32 {
  connected := pkg.db.sqlite.connect(":memory:", [
    pkg.db.sqlite.ConnectOption.BusyTimeoutNs(3000000),
  ])
  return match connected {
    Err(_) => 1
    Ok(connection) => {
      target := pkg.db.exec_conn(connection)
      executed := pkg.db.sqlite.execute_native(
        target,
        app.setup.command(),
        app.setup.Params { marker: 1 },
        [],
        [],
      )
      match executed {
        Err(error) => match error {
          Unsupported(_) => 2
          InvalidQuery(_) => 4
          Connection(native) => {
            print(native.message)
            match native.extended_code { Some(code) => code as i32, None => 5 }
          }
          Decode(_) => 6
          DriverMismatch(_) => 7
          _ => 8
        }
        Ok(_) => arena out {
          during_timeout := pkg.db.sqlite.one_native(
            target,
            app.timeout.query(),
            app.timeout.Params { marker: 1 },
            out,
            [],
            [pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(7000000)],
          )
          after_timeout := pkg.db.sqlite.one_native(
            target, app.timeout.query(), app.timeout.Params { marker: 1 }, out, [], [],
          )
          during_timeout_ok := match during_timeout {
            Ok(row) => row.timeout == 7
            Err(_) => false
          }
          after_timeout_ok := match after_timeout {
            Ok(row) => row.timeout == 3
            Err(_) => false
          }
          common_winner := pkg.db.sqlite.one_native(
            target,
            app.read.query(),
            app.read.Params { value: 1 },
            out,
            [pkg.db.ExecuteOption.TimeoutNs(0)],
            [pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(0)],
          )
          common_winner_ok := match common_winner {
            Err(error) => match error {
              Unsupported(contract) => contract.item == "db.execute.timeout_ns"
              _ => false
            }
            Ok(_) => false
          }
          bad_native := pkg.db.sqlite.one_native(
            target,
            app.read.query(),
            app.read.Params { value: 1 },
            out,
            [],
            [pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(0)],
          )
          duplicate_native := pkg.db.sqlite.one_native(
            target,
            app.read.query(),
            app.read.Params { value: 1 },
            out,
            [],
            [
              pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(1000000),
              pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(2000000),
            ],
          )
          overflow_native := pkg.db.sqlite.one_native(
            target,
            app.read.query(),
            app.read.Params { value: 1 },
            out,
            [],
            [pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(2147483647000001)],
          )
          bad_native_ok := match bad_native {
            Err(error) => match error {
              Unsupported(contract) => contract.item == "sqlite.execute.busy_timeout_ns"
              _ => false
            }
            Ok(_) => false
          }
          duplicate_native_ok := match duplicate_native {
            Err(error) => match error {
              Unsupported(contract) => contract.item == "sqlite.execute.busy_timeout_ns"
              _ => false
            }
            Ok(_) => false
          }
          overflow_native_ok := match overflow_native {
            Err(error) => match error {
              Unsupported(contract) => contract.item == "sqlite.execute.busy_timeout_ns"
              _ => false
            }
            Ok(_) => false
          }
          if !during_timeout_ok || !after_timeout_ok || !common_winner_ok
            || !bad_native_ok || !duplicate_native_ok || !overflow_native_ok {
            return 11
          }
          zero := pkg.db.sqlite.one_native(
            target,
            app.items.query(),
            app.items.Params { value: 7 },
            out,
            [],
            [pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(9000000)],
          )
          zero_ok := match zero {
            Err(error) => match error {
              Cardinality(detail) => detail.observed_at_least == 0
              _ => false
            }
            Ok(_) => false
          }
          after_zero := pkg.db.sqlite.one_native(
            target, app.timeout.query(), app.timeout.Params { marker: 1 }, out, [], [],
          )
          after_zero_ok := match after_zero { Ok(row) => row.timeout == 3, Err(_) => false }
          inserted := pkg.db.sqlite.execute_native(
            target, app.items.insert(), app.items.Params { value: 7 }, [], [],
          )
          inserted_ok := match inserted {
            Ok(detail) => match detail.rows_affected { Some(count) => count == 2, None => false }
            Err(_) => false
          }
          many := pkg.db.sqlite.one_native(
            target, app.items.query(), app.items.Params { value: 7 }, out, [], [],
          )
          many_ok := match many {
            Err(error) => match error {
              Cardinality(detail) => detail.observed_at_least == 2
              _ => false
            }
            Ok(_) => false
          }
          if !zero_ok || !after_zero_ok || !inserted_ok || !many_ok { return 9 }
          malformed_setup := pkg.db.sqlite.execute_native(
            target, app.malformed.setup(), app.malformed.Params { marker: 11 }, [], [],
          )
          malformed_insert := pkg.db.sqlite.execute_native(
            target, app.malformed.insert(), app.malformed.Params { marker: 11 }, [], [],
          )
          setup_ok := match malformed_setup { Ok(_) => true, Err(_) => false }
          insert_ok := match malformed_insert { Ok(_) => true, Err(_) => false }
          bad_first := pkg.db.sqlite.one_native(
            target,
            app.malformed.bad_first(),
            app.malformed.Params { marker: 11 },
            out,
            [],
            [pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(11000000)],
          )
          bad_first_ok := match bad_first {
            Err(error) => match error { Decode(_) => true, _ => false }
            Ok(_) => false
          }
          valid_first := pkg.db.sqlite.one_native(
            target, app.malformed.valid_first(), app.malformed.Params { marker: 11 }, out, [], [],
          )
          valid_first_ok := match valid_first {
            Err(error) => match error {
              Cardinality(detail) => detail.observed_at_least == 2
              _ => false
            }
            Ok(_) => false
          }
          after_bad_first := pkg.db.sqlite.one_native(
            target, app.timeout.query(), app.timeout.Params { marker: 1 }, out, [], [],
          )
          after_bad_first_ok := match after_bad_first {
            Ok(row) => row.timeout == 3
            Err(_) => false
          }
          if !setup_ok || !insert_ok || !bad_first_ok || !after_bad_first_ok
            || !valid_first_ok { return 10 }
          row_command := pkg.db.sqlite.execute_native(
            target,
            app.invalid.row_command(),
            app.invalid.Params { value: 13 },
            [],
            [pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(13000000)],
          )
          row_command_ok := match row_command {
            Err(error) => match error {
              InvalidQuery(contract) => contract.item == "db.command.row"
              _ => false
            }
            Ok(_) => false
          }
          after_invalid := pkg.db.sqlite.one_native(
            target, app.timeout.query(), app.timeout.Params { marker: 1 }, out, [], [],
          )
          after_invalid_ok := match after_invalid {
            Ok(row) => row.timeout == 3
            Err(_) => false
          }
          if !row_command_ok || !after_invalid_ok {
            return 12
          }
          selected := pkg.db.sqlite.one_native(
            target, app.read.query(), app.read.Params { value: 42 }, out, [], [],
          )
          match selected { Ok(row) => row.value as i32, Err(_) => 3 }
        }
      }
    }
  }
}
"#;
    let mut files = package_files(main);
    files.push(("app/setup.align", SETUP));
    files.push(("app/read.align", READ));
    files.push(("app/items.align", ITEMS));
    files.push(("app/malformed.align", MALFORMED));
    files.push(("app/timeout.align", TIMEOUT));
    files.push(("app/invalid.align", INVALID));
    let built = build_per_unit_multi("pkg-db-q2-sqlite-native-scalar", &files, "main.align");
    let main_mir = align_mir::print::program_to_string(&built.unit("main").mir);
    assert!(
        main_mir.contains("fn main("),
        "entry MIR lost main:\n{main_mir}"
    );
    let callbacks_mir = align_mir::print::program_to_string(&built.unit("pkg.db.internal").mir);
    assert!(
        callbacks_mir.contains("fn pkg.db.internal$bind_i64_v1("),
        "summary: {:?}\n{callbacks_mir}",
        built.unit("pkg.db.internal").summary.fns,
    );
    assert!(
        callbacks_mir.contains("fn pkg.db.internal$validate_row_count_v1("),
        "{callbacks_mir}"
    );
    assert!(
        callbacks_mir.contains("fn pkg.db.internal$validate_i64_v1("),
        "{callbacks_mir}"
    );
    assert!(
        callbacks_mir.contains("fn pkg.db.internal$read_i64_v1("),
        "{callbacks_mir}"
    );
    let output = built.link_and_run();
    assert_eq!(
        output.status.code(),
        Some(42),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn postgres_native_command_and_one_own_buffered_results() {
    if !backend_available() || !cc_available() {
        return;
    }
    const COMMAND: &str = r#"module app.pg_command
import pkg.db
import pkg.db.postgres

pub Params { value: i64 }

pub fn command() -> pkg.db.command<Params> = pkg.db.postgres.command(
  "UPDATE stub SET value = :value /* COMMAND_OK */",
  [],
  [pkg.db.postgres.CommandOption.ParameterType("value", "int8")],
)

pub fn row_command() -> pkg.db.command<Params> = pkg.db.postgres.command(
  "SELECT :value AS value /* ROW_COMMAND */",
  [],
  [pkg.db.postgres.CommandOption.ParameterType("value", "int8")],
)

pub fn malformed_affected() -> pkg.db.command<Params> = pkg.db.postgres.command(
  "UPDATE stub SET value = :value /* COMMAND_OK AFFECTED_MALFORMED */",
  [],
  [pkg.db.postgres.CommandOption.ParameterType("value", "int8")],
)
"#;
    const QUERY: &str = r#"module app.pg_query
import pkg.db
import pkg.db.postgres

pub Params { value: i64 }
pub Row { value: i64 }

pub fn one() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn zero() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* ZERO_ROWS */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn bad_first() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* BAD_FIRST */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn valid_first() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* VALID_FIRST */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn constraint() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* NATIVE_CONSTRAINT */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn serialization() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* NATIVE_SERIALIZATION */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn deadlock() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* NATIVE_DEADLOCK */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn cancelled() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* NATIVE_CANCELLED */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn null_result() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value /* NULL_RESULT */",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)
"#;
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.pg_command
import app.pg_query

extern "C" {
  fn align_pg_reset()
  fn align_pg_connect_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_protocol_ok() -> i32
  fn align_pg_last_timeout() -> i32
}

fn exercise() -> i32 {
  unsafe { align_pg_reset() }
  connected := pkg.db.postgres.connect("postgresql://stub/db", [
    pkg.db.postgres.ConnectOption.ApplicationName("align-q2"),
    pkg.db.postgres.ConnectOption.ConnectTimeoutNs(1),
    pkg.db.postgres.ConnectOption.Parameter("options", "-c statement_timeout=20"),
  ])
  return match connected {
    Err(_) => 1
    Ok(connection) => {
      target := pkg.db.exec_conn(connection)
      statement := app.pg_command.command()
      command := pkg.db.postgres.execute_native(
        target,
        statement,
        app.pg_command.Params { value: 7 },
        [],
        [],
      )
      command_ok := match command {
        Ok(detail) => match detail.rows_affected { Some(value) => value == 2, None => false }
        Err(_) => false
      }
      arena out {
        selected := pkg.db.postgres.one_native(
          target, app.pg_query.one(), app.pg_query.Params { value: 42 }, out, [], [],
        )
        selected_ok := match selected { Ok(row) => row.value == 42, Err(_) => false }
        zero := pkg.db.postgres.one_native(
          target, app.pg_query.zero(), app.pg_query.Params { value: 1 }, out, [], [],
        )
        zero_ok := match zero {
          Err(error) => match error {
            Cardinality(detail) => detail.observed_at_least == 0
            _ => false
          }
          Ok(_) => false
        }
        bad_first := pkg.db.postgres.one_native(
          target, app.pg_query.bad_first(), app.pg_query.Params { value: 5 }, out, [], [],
        )
        bad_first_ok := match bad_first {
          Err(error) => match error { Decode(_) => true, _ => false }
          Ok(_) => false
        }
        valid_first := pkg.db.postgres.one_native(
          target, app.pg_query.valid_first(), app.pg_query.Params { value: 5 }, out, [], [],
        )
        valid_first_ok := match valid_first {
          Err(error) => match error {
            Cardinality(detail) => detail.observed_at_least == 2
            _ => false
          }
          Ok(_) => false
        }
        row_command := pkg.db.postgres.execute_native(
          target,
          app.pg_command.row_command(),
          app.pg_command.Params { value: 9 },
          [],
          [],
        )
        row_command_ok := match row_command {
          Err(error) => match error {
            InvalidQuery(contract) => contract.item == "db.command.row"
            _ => false
          }
          Ok(_) => false
        }
        malformed_affected := pkg.db.postgres.execute_native(
          target,
          app.pg_command.malformed_affected(),
          app.pg_command.Params { value: 9 },
          [],
          [],
        )
        malformed_affected_ok := match malformed_affected {
          Err(error) => match error {
            Native(native) => native.message == "stub execution failure"
              && match native.sqlstate { Some(state) => state == "XX000", None => false }
              && match native.detail { Some(detail) => detail == "stub detail", None => false }
            _ => false
          }
          Ok(_) => false
        }
        constraint := pkg.db.postgres.one_native(
          target, app.pg_query.constraint(), app.pg_query.Params { value: 1 }, out, [], [],
        )
        constraint_ok := match constraint {
          Err(error) => match error {
            Constraint(native) => match native.constraint {
              Some(name) => name == "stub_constraint"
              None => false
            }
            _ => false
          }
          Ok(_) => false
        }
        serialization := pkg.db.postgres.one_native(
          target, app.pg_query.serialization(), app.pg_query.Params { value: 1 }, out, [], [],
        )
        serialization_ok := match serialization {
          Err(error) => match error { Serialization(_) => true, _ => false }
          Ok(_) => false
        }
        deadlock := pkg.db.postgres.one_native(
          target, app.pg_query.deadlock(), app.pg_query.Params { value: 1 }, out, [], [],
        )
        deadlock_ok := match deadlock {
          Err(error) => match error { Deadlock(_) => true, _ => false }
          Ok(_) => false
        }
        cancelled := pkg.db.postgres.one_native(
          target, app.pg_query.cancelled(), app.pg_query.Params { value: 1 }, out, [], [],
        )
        cancelled_ok := match cancelled {
          Err(error) => match error { Cancelled(_) => true, _ => false }
          Ok(_) => false
        }
        null_result := pkg.db.postgres.one_native(
          target, app.pg_query.null_result(), app.pg_query.Params { value: 1 }, out, [], [],
        )
        null_result_ok := match null_result {
          Err(error) => match error {
            Connection(native) => native.message == "stub connection failure"
            _ => false
          }
          Ok(_) => false
        }
        native_calls_ok := unsafe {
          align_pg_connect_calls() == 1 && align_pg_execute_calls() == 12
            && align_pg_clear_calls() == 11 && align_pg_protocol_ok() == 1
            && align_pg_last_timeout() == 2 && align_pg_finish_calls() == 0
        }
        if command_ok && selected_ok && zero_ok && bad_first_ok && valid_first_ok
          && row_command_ok && malformed_affected_ok && constraint_ok && serialization_ok
          && deadlock_ok && cancelled_ok && null_result_ok && native_calls_ok { 42 } else { 2 }
      }
    }
  }
}

fn main() -> i32 {
  result := exercise()
  if result != 42 { return result }
  return unsafe { if align_pg_finish_calls() == 1 { 42 } else { 3 } }
}
"#;
    let mut files = package_files(main);
    files.push(("app/pg_command.align", COMMAND));
    files.push(("app/pg_query.align", QUERY));
    let output = build_and_run_multi_with_c(
        "pkg-db-q2-postgres-native-scalar",
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
fn postgres_connect_options_validate_before_open_and_close_once() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres

extern "C" {
  fn align_pg_reset()
  fn align_pg_connect_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_encoding_calls() -> i32
  fn align_pg_last_timeout() -> i32
}

fn rejected(url: str, options: slice<pkg.db.postgres.ConnectOption>, item: str) -> bool {
  unsafe { align_pg_reset() }
  result := pkg.db.postgres.connect(url, options)
  error_ok := match result {
    Err(error) => match error {
      Unsupported(contract) => contract.item == item
      Encode(contract) => contract.item == item
      _ => false
    }
    Ok(_) => false
  }
  return unsafe { error_ok && align_pg_connect_calls() == 0 && align_pg_finish_calls() == 0 }
}

fn opens_with_timeout(ns: i64) -> bool {
  result := pkg.db.postgres.connect(
    "postgresql://stub/db",
    [pkg.db.postgres.ConnectOption.ConnectTimeoutNs(ns)],
  )
  return match result { Ok(_) => true, Err(_) => false }
}

fn timeout_case(ns: i64, expected_seconds: i32) -> bool {
  unsafe { align_pg_reset() }
  connected := opens_with_timeout(ns)
  return unsafe {
    connected && align_pg_connect_calls() == 1 && align_pg_finish_calls() == 1
      && align_pg_encoding_calls() == 1 && align_pg_last_timeout() == expected_seconds
  }
}

fn connection_failures_close_in_status_order() -> bool {
  unsafe { align_pg_reset() }
  bad := pkg.db.postgres.connect("postgresql://stub/bad-connection", [])
  bad_ok := match bad {
    Err(error) => match error {
      Connection(native) => native.message == "stub connection failure"
      _ => false
    }
    Ok(_) => false
  }
  bad_counts := unsafe {
    align_pg_connect_calls() == 1 && align_pg_finish_calls() == 1
      && align_pg_encoding_calls() == 0
  }
  unsafe { align_pg_reset() }
  encoding := pkg.db.postgres.connect("postgresql://stub/bad-encoding", [])
  encoding_ok := match encoding {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "postgres.connection.client_encoding"
      _ => false
    }
    Ok(_) => false
  }
  encoding_counts := unsafe {
    align_pg_connect_calls() == 1 && align_pg_finish_calls() == 1
      && align_pg_encoding_calls() == 1
  }
  return bad_ok && bad_counts && encoding_ok && encoding_counts
}

fn main() -> i32 {
  invalid_timeout := rejected(
    "postgresql://stub/db",
    [pkg.db.postgres.ConnectOption.ConnectTimeoutNs(0)],
    "postgres.connect.connect_timeout_ns",
  )
  overflow_timeout := rejected(
    "postgresql://stub/db",
    [pkg.db.postgres.ConnectOption.ConnectTimeoutNs(2147483647000000001)],
    "postgres.connect.connect_timeout_ns",
  )
  duplicate := rejected(
    "postgresql://stub/db",
    [
      pkg.db.postgres.ConnectOption.ApplicationName("one"),
      pkg.db.postgres.ConnectOption.ApplicationName("two"),
    ],
    "postgres.connect.parameter",
  )
  url_collision := rejected(
    "postgresql://stub/db?application_name=url-app",
    [pkg.db.postgres.ConnectOption.ApplicationName("option-app")],
    "postgres.connect.parameter",
  )
  direct_encoding := rejected(
    "postgresql://stub/db?client_encoding=LATIN1",
    [],
    "postgres.connection.client_encoding",
  )
  option_encoding_a := rejected(
    "postgresql://stub/db",
    [pkg.db.postgres.ConnectOption.Parameter("options", "-c client_encoding=LATIN1")],
    "postgres.connection.client_encoding",
  )
  option_encoding_b := rejected(
    "postgresql://stub/db",
    [pkg.db.postgres.ConnectOption.Parameter("options", "-cCLIENT-ENCODING=LATIN1")],
    "postgres.connection.client_encoding",
  )
  option_encoding_c := rejected(
    "postgresql://stub/db",
    [pkg.db.postgres.ConnectOption.Parameter("options", "--CLIENT_ENCODING=LATIN1")],
    "postgres.connection.client_encoding",
  )
  malformed_c := rejected(
    "postgresql://stub/db",
    [pkg.db.postgres.ConnectOption.Parameter("options", "-c")],
    "postgres.connect.options",
  )
  malformed_escape := rejected(
    "postgresql://stub/db",
    [pkg.db.postgres.ConnectOption.Parameter("options", "statement_timeout=20\\")],
    "postgres.connect.options",
  )
  timeout_one := timeout_case(1, 2)
  timeout_second := timeout_case(1000000000, 2)
  timeout_two := timeout_case(2000000000, 2)
  timeout_three := timeout_case(2000000001, 3)
  if !invalid_timeout { return 10 }
  if !overflow_timeout { return 11 }
  if !duplicate { return 12 }
  if !url_collision { return 13 }
  if !direct_encoding { return 14 }
  if !option_encoding_a { return 15 }
  if !option_encoding_b { return 16 }
  if !option_encoding_c { return 17 }
  if !malformed_c { return 18 }
  if !malformed_escape { return 19 }
  if !timeout_one { return 20 }
  if !timeout_second { return 22 }
  if !timeout_two { return 23 }
  if !timeout_three { return 24 }
  if !connection_failures_close_in_status_order() { return 21 }
  return 42
}
"#;
    let files = package_files(main);
    let output = build_and_run_multi_with_c(
        "pkg-db-q2-postgres-connect-options",
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
fn common_surface_dispatches_to_sqlite_without_driver_cycle() {
    if !backend_available() {
        return;
    }
    const COMMAND: &str = r#"module app.common_command
import pkg.db

pub Params { marker: i64 }

pub fn create() -> pkg.db.command<Params> = pkg.db.command(
  "CREATE TABLE common_items AS SELECT :marker AS value WHERE 0",
  [],
)
"#;
    const QUERY: &str = r#"module app.common_query
import pkg.db

pub Params { value: i64 }
pub Row { value: i64 }

pub fn one() -> pkg.db.query<Params, Row> = pkg.db.query(
  "SELECT CAST(:value AS INTEGER) AS value",
  [],
)
"#;
    let main = r#"module main
import std.crypto
import pkg.db
import pkg.db.sqlite
import app.common_command
import app.common_query

fn main() -> i32 {
  connected := pkg.db.sqlite.connect(":memory:", [])
  return match connected {
    Err(_) => 1
    Ok(connection) => {
      target := pkg.db.exec_conn(connection)
      created := pkg.db.execute(
        target,
        app.common_command.create(),
        app.common_command.Params { marker: 1 },
        [],
      )
      created_ok := match created {
        Ok(result) => match result.rows_affected { Some(value) => value == 0, None => false }
        Err(_) => false
      }
      arena out {
        selected := pkg.db.one(
          target,
          app.common_query.one(),
          app.common_query.Params { value: 42 },
          out,
          [],
        )
        selected_ok := match selected { Ok(row) => row.value == 42, Err(_) => false }
        timeout := pkg.db.one(
          target,
          app.common_query.one(),
          app.common_query.Params { value: 42 },
          out,
          [pkg.db.ExecuteOption.TimeoutNs(0)],
        )
        timeout_ok := match timeout {
          Err(error) => match error {
            Unsupported(contract) => contract.item == "db.execute.timeout_ns"
            _ => false
          }
          Ok(_) => false
        }
        digest := crypto.sha256("libpq link order")
        crypto_ok := digest.len() == 32
        if created_ok && selected_ok && timeout_ok && crypto_ok { 42 } else { 2 }
      }
    }
  }
}

"#;
    let mut files = package_files(main);
    files.push(("app/common_command.align", COMMAND));
    files.push(("app/common_query.align", QUERY));
    let whole = build_and_run_multi_with_static_descriptors(
        "pkg-db-q2-common-sqlite-whole",
        &files,
        "main.align",
    );
    assert_eq!(
        whole.status.code(),
        Some(42),
        "whole stdout: {}; stderr: {}",
        String::from_utf8_lossy(&whole.stdout),
        String::from_utf8_lossy(&whole.stderr),
    );
    let built = build_per_unit_multi("pkg-db-q2-common-sqlite", &files, "main.align");
    let link_libs = built.link_libs_union();
    assert!(
        link_libs.iter().any(|linked| linked == "pq"),
        "missing libpq: {link_libs:?}"
    );
    let ordered_link_libs = order_link_libs(&link_libs);
    let position = |library: &str| {
        ordered_link_libs
            .iter()
            .rposition(|linked| linked == library)
            .unwrap_or_else(|| {
                panic!("missing `{library}` in ordered link list: {ordered_link_libs:?}")
            })
    };
    assert!(
        position("pq") < position("ssl") && position("ssl") < position("crypto"),
        "libpq TLS closure must be ordered dependent-first: {ordered_link_libs:?}"
    );
    let output = built.link_and_run();
    assert_eq!(
        output.status.code(),
        Some(42),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let main_mir = align_mir::print::program_to_string(&built.unit("main").mir);
    assert!(
        main_mir.contains("call_with_cleanup program pkg.db.internal.sqlite$execute_prevalidated$"),
        "common execute did not lower to the SQLite engine:\n{main_mir}"
    );
    assert!(
        main_mir.contains("call_with_cleanup program pkg.db.internal.sqlite$one_prevalidated$"),
        "common one did not lower to the SQLite engine:\n{main_mir}"
    );
}

#[test]
fn postgres_bytea_codecs_preserve_text_and_binary_boundaries() {
    if !backend_available() {
        return;
    }
    const CODEC: &str = r#"module pkg.db.codec_test
import pkg.db.internal.postgres

fn byte_at(pointer: raw, offset: i64) -> u8 {
  unsafe { return raw.load(pointer, offset) }
}

pub fn run() -> i32 {
  mut input := buffer(4)
  input.put_u8(0)
  input.put_u8(15)
  input.put_u8(16)
  input.put_u8(255)
  bytes := input.bytes()
  text := pkg.db.internal.postgres.encode_bytea_text(bytes)
  binary := pkg.db.internal.postgres.encode_bytea_binary(bytes)
  unsafe {
    text_ok := pkg.db.internal.postgres.bytea_text_length(bytes) == 10
      && byte_at(text, 0) == 92
      && byte_at(text, 1) == 120
      && byte_at(text, 2) == 48
      && byte_at(text, 3) == 48
      && byte_at(text, 4) == 48
      && byte_at(text, 5) == 102
      && byte_at(text, 6) == 49
      && byte_at(text, 7) == 48
      && byte_at(text, 8) == 102
      && byte_at(text, 9) == 102
      && byte_at(text, 10) == 0
    binary_ok := pkg.db.internal.postgres.bytea_binary_length(bytes) == 4
      && byte_at(binary, 0) == 0
      && byte_at(binary, 1) == 15
      && byte_at(binary, 2) == 16
      && byte_at(binary, 3) == 255
      && byte_at(binary, 4) == 0
    pkg.db.internal.postgres.free_bytea_buffer(text)
    pkg.db.internal.postgres.free_bytea_buffer(binary)
    if text_ok && binary_ok { return 42 }
    return 1
  }
}

"#;
    let main = r#"module main
import pkg.db.codec_test

fn main() -> i32 = pkg.db.codec_test.run()
"#;
    let mut files = package_files(main);
    files.push(("pkg/db/codec_test.align", CODEC));
    let output = build_and_run_multi("pkg-db-q2-postgres-bytea", &files, "main.align");
    assert_eq!(
        output.status.code(),
        Some(42),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn common_surface_dispatches_to_postgres_engine() {
    if !backend_available() || !cc_available() {
        return;
    }
    const QUERY: &str = r#"module app.common_pg
import pkg.db
import pkg.db.postgres

pub Params { value: i64 }
pub Row { value: i64 }

pub fn command() -> pkg.db.command<Params> = pkg.db.postgres.command(
  "UPDATE stub SET value = :value /* COMMAND_OK */",
  [],
  [pkg.db.postgres.CommandOption.ParameterType("value", "int8")],
)

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)
"#;
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.common_pg

extern "C" {
  fn align_pg_reset()
  fn align_pg_connect_calls() -> i32
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
}

fn main() -> i32 {
  unsafe { align_pg_reset() }
  connected := pkg.db.postgres.connect("postgresql://stub/db", [])
  return match connected {
    Err(_) => 1
    Ok(connection) => {
      target := pkg.db.exec_conn(connection)
      command := pkg.db.execute(
        target,
        app.common_pg.command(),
        app.common_pg.Params { value: 7 },
        [],
      )
      command_ok := match command {
        Ok(result) => match result.rows_affected { Some(value) => value == 2, None => false }
        Err(_) => false
      }
      arena out {
        selected := pkg.db.one(
          target,
          app.common_pg.query(),
          app.common_pg.Params { value: 42 },
          out,
          [],
        )
        selected_ok := match selected { Ok(row) => row.value == 42, Err(_) => false }
        native_ok := unsafe {
          align_pg_connect_calls() == 1 && align_pg_execute_calls() == 2
            && align_pg_clear_calls() == 2
        }
        if command_ok && selected_ok && native_ok { 42 } else { 2 }
      }
    }
  }
}
"#;
    let mut files = package_files(main);
    files.push(("app/common_pg.align", QUERY));
    let output = build_and_run_multi_with_c(
        "pkg-db-q2-common-postgres",
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
fn postgres_required_mode_requires_configuration() {
    let required = std::env::var("ALIGN_DB_POSTGRES_REQUIRED").ok().as_deref() == Some("1");
    let configured = std::env::var("ALIGN_DB_POSTGRES_URL")
        .ok()
        .filter(|url| !url.is_empty())
        .is_some();
    assert!(
        !required || configured,
        "ALIGN_DB_POSTGRES_REQUIRED=1 requires ALIGN_DB_POSTGRES_URL"
    );
}

#[test]
fn inherited_environment_survives_pkg_db_link_closure() {
    if !backend_available() {
        return;
    }
    let main = r#"module main
import std.env
import pkg.db
import pkg.db.sqlite
import pkg.db.postgres

fn main() -> i32 {
  return match env.get("ALIGN_DB_Q2_INHERITED") {
    Some(value) => if value == "inherited-value" { 42 } else { 2 }
    None => 1
  }
}
"#;
    let output = build_and_run_multi_with_env(
        "pkg-db-q2-inherited-environment",
        &package_files(main),
        "main.align",
        &[("ALIGN_DB_Q2_INHERITED", "inherited-value")],
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
fn postgres_required_portable_query_runs_against_both_drivers() {
    if !backend_available() {
        return;
    }
    let required = std::env::var("ALIGN_DB_POSTGRES_REQUIRED").ok().as_deref() == Some("1");
    let configured = std::env::var("ALIGN_DB_POSTGRES_URL")
        .ok()
        .filter(|url| !url.is_empty())
        .is_some();
    if !configured {
        assert!(
            !required,
            "ALIGN_DB_POSTGRES_REQUIRED=1 requires ALIGN_DB_POSTGRES_URL"
        );
        eprintln!("postgres portable-query integration skipped: ALIGN_DB_POSTGRES_URL is not set");
        return;
    }
    let postgres_url = std::env::var("ALIGN_DB_POSTGRES_URL").expect("configured URL");
    const QUERY: &str = r#"module app.portable_query
import pkg.db

pub Params { value: i64 }
pub Row { value: i64 }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.query(
  "SELECT CAST(:value AS BIGINT) AS value",
  [],
)
"#;
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import pkg.db.postgres
import app.portable_query

fn main(args: array<str>) -> Result<(), Error> {
  url := args[1]
  sqlite := pkg.db.sqlite.connect(":memory:", [])
  postgres := pkg.db.postgres.connect(url, [])
  match sqlite {
    Err(_) => { print(2); return Ok(()) }
    Ok(sqlite_connection) => match postgres {
      Err(_) => { print(3); return Ok(()) }
      Ok(postgres_connection) => {
        sqlite_target := pkg.db.exec_conn(sqlite_connection)
        postgres_target := pkg.db.exec_conn(postgres_connection)
        arena sqlite_out {
          sqlite_row := pkg.db.one(
            sqlite_target,
            app.portable_query.query(),
            app.portable_query.Params { value: 42 },
            sqlite_out,
            [],
          )
          sqlite_ok := match sqlite_row { Ok(row) => row.value == 42, Err(_) => false }
          arena postgres_out {
            postgres_row := pkg.db.one(
              postgres_target,
              app.portable_query.query(),
              app.portable_query.Params { value: 42 },
              postgres_out,
              [],
            )
            postgres_ok := match postgres_row { Ok(row) => row.value == 42, Err(_) => false }
            if sqlite_ok && postgres_ok { print(42) } else { print(4) }
            return Ok(())
          }
        }
      }
    }
  }
}
"#;
    let files = [
        ("pkg/db.align", DB),
        ("pkg/db/sqlite.align", SQLITE),
        ("pkg/db/postgres.align", POSTGRES),
        ("pkg/db/internal.align", INTERNAL),
        ("pkg/db/internal/resource.align", RESOURCE),
        ("pkg/db/internal/descriptor.align", DESCRIPTOR),
        ("pkg/db/internal/sqlite.align", INTERNAL_SQLITE),
        ("pkg/db/internal/postgres.align", INTERNAL_POSTGRES),
        ("app/portable_query.align", QUERY),
        ("main.align", main),
    ];
    let output = build_and_run_multi_args_with_env(
        "pkg-db-q2-required-postgres-portable",
        &files,
        "main.align",
        &[postgres_url.as_str()],
        &[],
    );
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "42\n",
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}
