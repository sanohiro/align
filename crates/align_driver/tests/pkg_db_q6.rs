//! pkg.db Q6/D10 compound Query shaping and exact mutable-retention owners.

mod common;
use common::*;
mod db_harness;
use db_harness::*;
use std::sync::LazyLock;

static USER_GROUPS: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/app/user_groups.align"));
static TRANSACTION_MASTER: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/app/transaction_master.align"));


// ================================================================================================
// Layer-1 migrated cases.
//
// Native call counts moved out of the Align epilogues into `expect_counters`, so a mismatch now
// names the counter instead of returning a hand-numbered sentinel.
// ================================================================================================
const SQLITE_USER_GROUPS_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import app.user_groups

extern "C" {
  fn align_sqlite_q6_reset()
  fn align_sqlite_q6_prepare_calls() -> i32
  fn align_sqlite_q6_step_calls() -> i32
  fn align_sqlite_q6_delivered_rows() -> i32
  fn align_sqlite_q6_finalize_calls() -> i32
  fn align_sqlite_q6_protocol_ok() -> i32
}

fn counts(prepare: i32, steps: i32, delivered: i32, finalized: i32) -> bool = unsafe {
  align_sqlite_q6_protocol_ok() == 1
    && align_sqlite_q6_prepare_calls() == prepare
    && align_sqlite_q6_step_calls() == steps
    && align_sqlite_q6_delivered_rows() == delivered
    && align_sqlite_q6_finalize_calls() == finalized
}

fn error_code(error: pkg.db.Error) -> i32 {
  return match error {
    Connection(_) => 101
    Timeout(_) => 102
    Cancelled(_) => 103
    NotFound => 104
    Cardinality(_) => 105
    Constraint(_) => 106
    Serialization(_) => 107
    Deadlock(_) => 108
    SchemaMismatch(_) => 109
    DriverMismatch(_) => 110
    Decode(_) => 111
    Encode(_) => 112
    InvalidQuery(_) => 113
    Unsupported(_) => 114
    Native(_) => 115
  }
}

fn is_contract_error(error: pkg.db.Error, item: str) -> bool {
  return match error {
    Connection(_) => false
    Timeout(_) => false
    Cancelled(_) => false
    NotFound => false
    Cardinality(_) => false
    Constraint(_) => false
    Serialization(_) => false
    Deadlock(_) => false
    SchemaMismatch(_) => false
    DriverMismatch(_) => false
    Decode(contract) => contract.item == item
    Encode(_) => false
    InvalidQuery(_) => false
    Unsupported(_) => false
    Native(_) => false
  }
}

fn one_parent(borrow connection: pkg.db.conn) -> i32 {
  unsafe { align_sqlite_q6_reset() }
  arena out {
    attempted := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 0, last_user_id: 0 }, out,
    )
    empty := match attempted {
      Err(error) => { return error_code(error) }
      Ok(value) => value
    }
    if match empty { Some(_) => true, None => false } { return 2 }
    if !counts(1, 1, 0, 1) { return 3 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    shaped := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 1, last_user_id: 1 }, out,
    ) else { return 4 }
    value := shaped else { return 5 }
    if value.user.id != 1 || value.user.name != "Alice" { return 6 }
    if value.groups.len() != 2 { return 7 }
    if value.groups[0].id != 10 || value.groups[0].name != "Admin" { return 8 }
    if value.groups[1].id != 20 || value.groups[1].name != "Dev" { return 9 }
    if !counts(1, 3, 2, 1) { return 10 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    shaped := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 2, last_user_id: 2 }, out,
    ) else { return 11 }
    value := shaped else { return 12 }
    if value.user.id != 2 || value.user.name != "Bob" || value.groups.len() != 0 {
      return 13
    }
    if !counts(1, 2, 1, 1) { return 14 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    partial_id := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 3, last_user_id: 3 }, out,
    )
    error := match partial_id { Ok(_) => { return 15 } Err(cause) => cause }
    if !is_contract_error(error, "app.user_groups.group") { return 23 }
    if !counts(1, 1, 1, 1) { return 16 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    partial_name := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 4, last_user_id: 4 }, out,
    )
    error := match partial_name { Ok(_) => { return 17 } Err(cause) => cause }
    if !is_contract_error(error, "app.user_groups.group") { return 24 }
    if !counts(1, 1, 1, 1) { return 18 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    inconsistent := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 5, last_user_id: 5 }, out,
    )
    error := match inconsistent { Ok(_) => { return 19 } Err(cause) => cause }
    if !is_contract_error(error, "app.user_groups.user_name") { return 25 }
    if !counts(1, 2, 2, 1) { return 20 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    inconsistent_id := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 6, last_user_id: 6 }, out,
    )
    error := match inconsistent_id { Ok(_) => { return 21 } Err(cause) => cause }
    if !is_contract_error(error, "app.user_groups.user_id") { return 26 }
    if !counts(1, 2, 2, 1) { return 22 }
  }
  return 0
}

fn segmented(borrow connection: pkg.db.conn) -> i32 {
  unsafe { align_sqlite_q6_reset() }
  arena out {
    empty := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 0, last_user_id: 0 }, out,
    ) else { return 50 }
    if empty.users.len() != 0 || empty.groups.len() != 0
      || empty.group_offsets.len() != 1 || empty.group_offsets[0] != 0 { return 51 }
    if !counts(1, 1, 0, 1) { return 52 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    result := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 1, last_user_id: 3 }, out,
    ) else { return 53 }
    if result.users.len() != 3 || result.groups.len() != 3
      || result.group_offsets.len() != 4 { return 54 }
    if result.users[0].id != 1 || result.users[0].name != "Alice"
      || result.users[1].id != 2 || result.users[1].name != "Bob"
      || result.users[2].id != 3 || result.users[2].name != "Cara" { return 55 }
    if result.group_offsets[0] != 0 || result.group_offsets[1] != 2
      || result.group_offsets[2] != 2 || result.group_offsets[3] != 3 { return 56 }
    if result.groups[0].id != 10 || result.groups[1].id != 20
      || result.groups[2].id != 40 { return 57 }
    if !counts(1, 5, 4, 1) { return 58 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    partial_id := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 3, last_user_id: 3 }, out,
    )
    error := match partial_id { Ok(_) => { return 59 } Err(cause) => cause }
    if !is_contract_error(error, "app.user_groups.group") { return 60 }
    if !counts(1, 1, 1, 1) { return 61 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    partial_name := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 4, last_user_id: 4 }, out,
    )
    error := match partial_name { Ok(_) => { return 62 } Err(cause) => cause }
    if !is_contract_error(error, "app.user_groups.group") { return 63 }
    if !counts(1, 1, 1, 1) { return 64 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    disagreement := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 5, last_user_id: 5 }, out,
    )
    error := match disagreement { Ok(_) => { return 65 } Err(cause) => cause }
    if !is_contract_error(error, "app.user_groups.user_name") { return 66 }
    if !counts(1, 2, 2, 1) { return 67 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    non_adjacent := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 7, last_user_id: 7 }, out,
    )
    error := match non_adjacent { Ok(_) => { return 68 } Err(cause) => cause }
    if !is_contract_error(error, "app.user_groups.order") { return 69 }
    if !counts(1, 3, 3, 1) { return 70 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    partial_precedence := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 9, last_user_id: 9 }, out,
    )
    error := match partial_precedence { Ok(_) => { return 71 } Err(cause) => cause }
    if !is_contract_error(error, "app.user_groups.group") { return 72 }
    if !counts(1, 1, 1, 1) { return 73 }
  }
  return 0
}

fn main() -> i32 {
  connection := pkg.db.sqlite.connect(":memory:", []) else { return 29 }
  one := one_parent(connection)
  if one != 0 { return one }
  many := segmented(connection)
  if many != 0 { return many }
  return 42
}
"#;

const SQLITE_TRANSACTION_MASTER_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import app.transaction_master
import pkg.db.testkit.sqlite6


fn project(target: pkg.db.exec, out: region) -> i32 {
  rows := app.transaction_master.run(
    target, app.transaction_master.Params { account_id: 1 }, out,
  ) else { return 1 }
  if rows.len() != 2 { return 2 }
  if rows[0].transaction.id != 100 || rows[0].transaction.posted_at != "2026-08-10"
    || rows[0].transaction.amount_cents != 5000 { return 3 }
  if rows[0].status.id != 7 || rows[0].status.code != "posted"
    || rows[0].status.name != "Posted" { return 4 }
  if rows[0].customer.id != 900 || rows[0].customer.name != "Ada" { return 5 }
  if rows[1].transaction.id != 101 || rows[1].customer.id != 901
    || rows[1].transaction.posted_at != "2026-08-09"
    || rows[1].transaction.amount_cents != 5001
    || rows[1].status.id != 7 || rows[1].status.code != "posted"
    || rows[1].status.name != "Posted"
    || rows[1].customer.name != "Linus" { return 6 }
  return 0
}

fn main() -> i32 {
  pkg.db.testkit.sqlite6.reset()
  connection := pkg.db.sqlite.connect(":memory:", []) else { return 7 }
  arena conn_out {
    direct := project(pkg.db.exec_conn(connection), conn_out)
    if direct != 0 { return direct }
  }
  transaction := pkg.db.begin(connection, []) else { return 8 }
  arena tx_out {
    nested := project(pkg.db.exec_tx(transaction), tx_out)
    if nested != 0 { return nested }
  }
  returned := pkg.db.rollback(transaction) else { return 9 }
  pkg.db.testkit.sqlite6.dump()
  return 42
}
"#;

const POSTGRES_COMPOUND_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.user_groups
import app.transaction_master
import pkg.db.testkit.pg


fn one_parent(borrow connection: pkg.db.conn) -> i32 {
  arena out {
    shaped := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 1, last_user_id: 1 }, out,
    ) else { return 1 }
    value := shaped else { return 2 }
    if value.user.id != 1 || value.user.name != "Alice" || value.groups.len() != 2 {
      return 3
    }
    if value.groups[0].id != 10 || value.groups[0].name != "Admin"
      || value.groups[1].id != 20 || value.groups[1].name != "Dev" { return 4 }
  }
  arena out {
    segmented := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 1, last_user_id: 3 }, out,
    ) else { return 5 }
    if segmented.users.len() != 3 || segmented.groups.len() != 3
      || segmented.group_offsets.len() != 4 { return 6 }
    if segmented.group_offsets[0] != 0 || segmented.group_offsets[1] != 2
      || segmented.group_offsets[2] != 2 || segmented.group_offsets[3] != 3 { return 7 }
  }
  return 0
}

fn project(target: pkg.db.exec, out: region) -> i32 {
  rows := app.transaction_master.run(
    target, app.transaction_master.Params { account_id: 1 }, out,
  ) else { return 8 }
  if rows.len() != 2 { return 9 }
  if rows[0].transaction.id != 100 || rows[0].status.code != "posted"
    || rows[0].customer.name != "Ada" { return 10 }
  if rows[1].transaction.id != 101 || rows[1].customer.name != "Linus" { return 11 }
  return 0
}

fn main() -> i32 {
  pkg.db.testkit.pg.reset()
  connection := pkg.db.postgres.connect("postgresql://stub/q6", []) else { return 12 }
  grouped := one_parent(connection)
  if grouped != 0 { return grouped }
  arena direct_out {
    direct := project(pkg.db.exec_conn(connection), direct_out)
    if direct != 0 { return direct }
  }
  transaction := pkg.db.begin(connection, []) else { return 13 }
  arena tx_out {
    nested := project(pkg.db.exec_tx(transaction), tx_out)
    if nested != 0 { return nested }
  }
  returned := pkg.db.rollback(transaction) else { return 14 }
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const SQLITE_PRE_SEND_FAILURE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import app.user_groups
import pkg.db.testkit.sqlite6

extern "C" {
  fn align_sqlite_q6_reset()
  fn align_sqlite_q6_fail_next_bind()
}

fn main() -> i32 {
  unsafe {
    align_sqlite_q6_reset()
    align_sqlite_q6_fail_next_bind()
  }
  connection := pkg.db.sqlite.connect(":memory:", []) else { return 1 }
  arena out {
    failed := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 1, last_user_id: 1 }, out,
    )
    if match failed { Ok(_) => true, Err(_) => false } { return 2 }
  }
  pkg.db.testkit.sqlite6.dump()
  return 42
}
"#;

const POSTGRES_SEND_FAILURE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.user_groups
import pkg.db.testkit.pg

extern "C" {
  fn align_pg_reset()
  fn align_pg_q6_fail_next_execute()
}

fn main() -> i32 {
  unsafe {
    align_pg_reset()
    align_pg_q6_fail_next_execute()
  }
  connection := pkg.db.postgres.connect("postgresql://stub/q6-fail", []) else { return 1 }
  arena out {
    failed := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 1, last_user_id: 1 }, out,
    )
    if match failed { Ok(_) => true, Err(_) => false } { return 2 }
  }
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const CASE_SQLITE_USER_GROUPS: Case = Case {
    label: "pkg-db-q6-sqlite-user-groups",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q6],
    counters: &[],
    modules: q6_modules,
    main: SQLITE_USER_GROUPS_MAIN,
    expected_exit: 42,
    expect_counters: &[],
};

const CASE_SQLITE_TRANSACTION_MASTER: Case = Case {
    label: "pkg-db-q6-sqlite-transaction-master",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q6],
    counters: &[&SQLITE_Q6],
    modules: q6_modules,
    main: SQLITE_TRANSACTION_MASTER_MAIN,
    expected_exit: 42,
    expect_counters: &[
        ("sqlite6.protocol_ok", 1),
        ("sqlite6.prepare_calls", 2),
        ("sqlite6.step_calls", 6),
        ("sqlite6.delivered_rows", 4),
        ("sqlite6.finalize_calls", 2),
    ],
};

const CASE_POSTGRES_COMPOUND: Case = Case {
    label: "pkg-db-q6-postgres-compound",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[&PG],
    modules: q6_modules,
    main: POSTGRES_COMPOUND_MAIN,
    expected_exit: 42,
    expect_counters: &[
        ("pg.prepare_calls", 0),
        ("pg.execute_prepared_calls", 0),
        ("pg.protocol_ok", 1),
        ("pg.execute_calls", 4),
        ("pg.delivered_rows", 10),
        ("pg.control_calls", 2),
        ("pg.clear_calls", 6),
    ],
};

const CASE_SQLITE_PRE_SEND_FAILURE: Case = Case {
    label: "pkg-db-q6-sqlite-pre-send-failure",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG, &SQLITE_Q6],
    counters: &[&SQLITE_Q6],
    modules: q6_modules,
    main: SQLITE_PRE_SEND_FAILURE_MAIN,
    expected_exit: 42,
    expect_counters: &[
        ("sqlite6.protocol_ok", 1),
        ("sqlite6.prepare_calls", 1),
        ("sqlite6.step_calls", 0),
        ("sqlite6.delivered_rows", 0),
        ("sqlite6.finalize_calls", 1),
    ],
};

const CASE_POSTGRES_SEND_FAILURE: Case = Case {
    label: "pkg-db-q6-postgres-send-failure",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[&PG],
    modules: q6_modules,
    main: POSTGRES_SEND_FAILURE_MAIN,
    expected_exit: 42,
    expect_counters: &[
        ("pg.protocol_ok", 1),
        ("pg.execute_calls", 1),
        ("pg.clear_calls", 0),
    ],
};

/// Every Layer-1 case in this suite, for the fingerprint golden.
const LAYER1_CASES: &[&Case] = &[
    &CASE_SQLITE_USER_GROUPS,
    &CASE_SQLITE_TRANSACTION_MASTER,
    &CASE_POSTGRES_COMPOUND,
    &CASE_SQLITE_PRE_SEND_FAILURE,
    &CASE_POSTGRES_SEND_FAILURE,
];

/// The suite modules every q6 case adds on top of the `pkg.db` package. Both are shipped demo app
/// modules read from `apps/db/app/`, not test fixtures.
fn q6_modules() -> Vec<(&'static str, &'static str)> {
    vec![
        ("app/user_groups.align", *USER_GROUPS),
        ("app/transaction_master.align", *TRANSACTION_MASTER),
    ]
}

fn package_files(main: &str) -> Layout {
    Layout::new()
        .module("app/user_groups.align", *USER_GROUPS)
        .module("app/transaction_master.align", *TRANSACTION_MASTER)
        .main(main)
}

fn mir_function<'a>(mir: &'a str, name: &str) -> &'a str {
    let marker = format!("fn {name}(");
    let start = mir
        .find(&marker)
        .unwrap_or_else(|| panic!("missing MIR function `{name}`"));
    let rest = &mir[start..];
    let end = rest[marker.len()..]
        .find("\nfn ")
        .map_or(rest.len(), |offset| marker.len() + offset);
    &rest[..end]
}

fn assert_compound_mir_shape(mir: &str) {
    let one_step = mir_function(mir, "app.user_groups$step");
    assert_eq!(one_step.matches("clone_in(").count(), 2);
    assert_eq!(one_step.matches("array_builder_push(").count(), 1);

    let segmented_step = mir_function(mir, "app.user_groups$segmented_step");
    assert_eq!(segmented_step.matches("clone_in(").count(), 3);
    assert_eq!(segmented_step.matches("array_builder_push(").count(), 1);

    let one_run = mir_function(mir, "app.user_groups$run");
    assert_eq!(one_run.matches("array_builder_new(").count(), 1);
    assert_eq!(one_run.matches("array_builder_build(").count(), 1);

    let segmented_run = mir_function(mir, "app.user_groups$run_segmented");
    assert_eq!(segmented_run.matches("array_builder_new(").count(), 3);
    assert_eq!(segmented_run.matches("array_builder_build(").count(), 3);

    let transaction_step = mir_function(mir, "app.transaction_master$step");
    assert_eq!(transaction_step.matches("clone_in(").count(), 4);
    assert_eq!(transaction_step.matches("array_builder_push(").count(), 1);

    let transaction_run = mir_function(mir, "app.transaction_master$run");
    assert_eq!(transaction_run.matches("array_builder_new(").count(), 1);
    assert_eq!(transaction_run.matches("array_builder_build(").count(), 1);
}

#[test]
fn compound_query_modules_are_exact_and_typecheck_whole_and_per_unit() {
    for required in [
        "pub fn query() -> pkg.db.query<Params, Row>",
        "mut stream := pkg.db.rows(target, query(), params, [])?",
        "row := pkg.db.next(stream)? else { break }",
        "fn step(",
        "borrow mut state: State",
        "borrow mut groups: array_builder<Group>",
        "row.user_name.clone_in(out)",
        "pub fn run_segmented(",
        "group_offsets: array<i64>",
    ] {
        assert!(
            USER_GROUPS.contains(required),
            "missing User + Groups source shape: {required}"
        );
    }
    for required in [
        "pub fn query() -> pkg.db.query<Params, Row>",
        "fn step(",
        "borrow mut output: array_builder<OutputRow>",
        "mut stream := pkg.db.rows(target, query(), params, [])?",
        "row := pkg.db.next(stream)? else { break }",
    ] {
        assert!(
            TRANSACTION_MASTER.contains(required),
            "missing transaction/master source shape: {required}"
        );
    }
    for absent in ["db.fold", "group_adjacent_by", "HashMap", "lazy"] {
        assert!(
            !USER_GROUPS.contains(absent),
            "unexpected hidden compound surface: {absent}"
        );
        assert!(
            !TRANSACTION_MASTER.contains(absent),
            "unexpected hidden compound surface: {absent}"
        );
    }
    assert!(!USER_GROUPS.contains("pub fn step("));
    assert!(!TRANSACTION_MASTER.contains("pub fn step("));

    let main = r#"module main
import pkg.db
import app.user_groups
import app.transaction_master

fn one(target: pkg.db.exec, out: region) -> Result<Option<app.user_groups.Output>, pkg.db.Error> =
  app.user_groups.run(target, app.user_groups.Params { first_user_id: 1, last_user_id: 1 }, out)

fn segmented(target: pkg.db.exec, out: region) -> Result<app.user_groups.SegmentedOutput, pkg.db.Error> =
  app.user_groups.run_segmented(target, app.user_groups.Params { first_user_id: 1, last_user_id: 3 }, out)

fn projected(target: pkg.db.exec, out: region) -> Result<array<app.transaction_master.OutputRow>, pkg.db.Error> =
  app.transaction_master.run(target, app.transaction_master.Params { account_id: 1 }, out)

fn main() -> i32 = 0
"#;
    let checked = diff_check_multi("pkg-db-q6-modules", &package_files(main).files(), "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole-program diagnostics:\n{}\nper-unit diagnostics:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
}

#[test]
fn compound_shaping_uses_region_builders_and_exact_visible_copies() {
    assert_eq!(USER_GROUPS.matches(".clone_in(out)").count(), 5);
    assert_eq!(USER_GROUPS.matches("array_builder(out)").count(), 4);
    assert_eq!(USER_GROUPS.matches(".build()").count(), 4);
    assert_eq!(TRANSACTION_MASTER.matches(".clone_in(out)").count(), 4);
    assert_eq!(TRANSACTION_MASTER.matches("array_builder(out)").count(), 1);
    assert_eq!(TRANSACTION_MASTER.matches(".build()").count(), 1);
    for forbidden in ["heap", "reflect", "materialize", "pkg.db.all("] {
        assert!(!USER_GROUPS.contains(forbidden));
        assert!(!TRANSACTION_MASTER.contains(forbidden));
    }

    let main = r#"module main
import app.user_groups
import app.transaction_master
fn main() -> i32 = 0
"#;
    let files = package_files(main);
    let whole = whole_mir_multi("pkg-db-q6-compound-mir-whole", &files.files(), "main.align");
    assert_compound_mir_shape(&whole);

    let per_unit = build_per_unit_multi("pkg-db-q6-compound-mir-unit", &files.files(), "main.align");
    let user_groups = align_mir::print::program_to_string(&per_unit.unit("app.user_groups").mir);
    let transaction =
        align_mir::print::program_to_string(&per_unit.unit("app.transaction_master").mir);
    assert_compound_mir_shape(&format!("{user_groups}\n{transaction}"));
}

#[test]
fn shapers_are_pure_and_cannot_reach_database_io() {
    let main = r#"module main
import app.user_groups
import app.transaction_master
fn main() -> i32 = 0
"#;
    let mir = whole_mir_multi(
        "pkg-db-q6-pure-shaper-boundary",
        &package_files(main).files(),
        "main.align",
    );
    for function in [
        "app.user_groups$step",
        "app.user_groups$segmented_step",
        "app.transaction_master$step",
    ] {
        let body = mir_function(&mir, function);
        for impure in [
            "pkg.db$rows",
            "pkg.db$next",
            "sqlite3_",
            "PQexec",
            "resource_from_raw",
            "resource_drop",
        ] {
            assert!(
                !body.contains(impure),
                "Pure shaper `{function}` unexpectedly reaches `{impure}`",
            );
        }
    }
    for run in [
        "app.user_groups$run",
        "app.user_groups$run_segmented",
        "app.transaction_master$run",
    ] {
        let body = mir_function(&mir, run);
        assert!(body.contains("pkg.db$rows"));
        assert!(body.contains("pkg.db$next"));
    }
}

/// Exact same-program `borrow mut` retention owners. Each test closes one matrix
/// cell independently so a failure names its scenario instead of hiding the rest.
mod borrow_mut_shaper_retention {
    use super::*;

    #[test]
    fn direct_and_wrapped_store() {
        let exact = r#"Item { text: str }
State { text: str }

fn leaf(
  borrow mut state: State,
  borrow mut items: array_builder<Item>,
  source: str,
  out: region,
) {
  state.text = source.clone_in(out)
  items.push(Item { text: state.text })
}

fn wrapped(
  borrow mut state: State,
  borrow mut items: array_builder<Item>,
  source: str,
  out: region,
) {
  if source.len() > 0 { leaf(state, items, source, out) } else { state.text = "" }
}

fn main() -> i32 {
  arena out {
    mut owner := "source".clone()
    source: str := owner
    mut state := State { text: "" }
    mut items: array_builder<Item> := array_builder(out)
    wrapped(state, items, source, out)
    owner = "replaced".clone()
    built := items.build()
    if owner.len() == 8 && state.text == "source" && built[0].text == "source" {
      return 42
    }
    return 0
  }
}
"#;
        let exact_diagnostics = check_diagnostics("pkg-db-q6-exact-retention", exact);
        assert!(
            exact_diagnostics.is_empty(),
            "exact same-program retention unexpectedly failed:\n{exact_diagnostics}",
        );
        let raw = exact.replace("state.text = source.clone_in(out)", "state.text = source");
        assert!(
            check_errs("pkg-db-q6-raw-retention", &raw),
            "retaining the raw source must keep its owner live",
        );
    }

    #[test]
    fn imported_interface_fallback() {
        let imported = [
            (
                "shaper.align",
                "module shaper\npub State { text: str }\npub fn copy(borrow mut state: State, source: str, out: region) { state.text = source.clone_in(out) }\n",
            ),
            (
                "main.align",
                "module main\nimport shaper\nfn main() -> i32 { arena out { mut owner := \"source\".clone(); source: str := owner; mut state := shaper.State { text: \"\" }; shaper.copy(state, source, out); owner = \"replaced\".clone(); return state.text.len() as i32 } }\n",
            ),
        ];
        let checked = diff_check_multi(
            "pkg-db-q6-imported-retention-fallback",
            &imported,
            "main.align",
        );
        assert!(
            !checked.whole_errors && checked.per_unit_errors,
            "an available same-program body is exact, while its interface-only call must fail closed:\nwhole={}\nper-unit={} ",
            checked.whole_diags,
            checked.per_unit_diags,
        );
    }

    #[test]
    fn recursive_store() {
        let recursive = r#"State { text: str }
fn copy_recursive(
  borrow mut state: State,
  source: str,
  out: region,
  remaining: i64,
) {
  if remaining == 0 {
    state.text = source.clone_in(out)
    return
  }
  copy_recursive(state, source, out, remaining - 1)
}
fn main() -> i32 {
  arena out {
    mut owner := "source".clone()
    source: str := owner
    mut state := State { text: "" }
    copy_recursive(state, source, out, 2)
    owner = "replaced".clone()
    if owner.len() == 8 && state.text == "source" { return 42 }
    return 0
  }
}
"#;
        assert!(
            check_diagnostics("pkg-db-q6-recursive-retention", recursive).is_empty(),
            "recursive exact retention must converge to the cloned destination region",
        );
        assert!(
            check_errs(
                "pkg-db-q6-recursive-raw-retention",
                &recursive.replace("source.clone_in(out)", "source"),
            ),
            "recursive raw retention must preserve the source owner",
        );
    }

    #[test]
    fn whole_replacement() {
        let replacements = r#"State { text: str }
fn replace_whole(borrow mut state: State, source: str, out: region) {
  state = State { text: source.clone_in(out) }
}
fn main() -> i32 {
  arena out {
    mut owner := "source".clone()
    source: str := owner
    mut state := State { text: "" }
    replace_whole(state, source, out)
    owner = "replaced".clone()
    if state.text == "source" { return 42 }
    return 0
  }
}
"#;
        let replacement_diagnostics =
            check_diagnostics("pkg-db-q6-replacement-retention", replacements);
        assert!(
            replacement_diagnostics.is_empty(),
            "whole replacement must retain only cloned destination storage:\n{replacement_diagnostics}",
        );
        assert!(
            check_errs(
                "pkg-db-q6-raw-replacement-retention",
                &replacements.replace("source.clone_in(out)", "source"),
            ),
            "whole raw replacement must retain its source owner",
        );
    }

    #[test]
    fn unary_exact_clear() {
        let unary = r#"State { text: str }
fn clear(borrow mut state: State) { state.text = "" }
fn main() -> i32 {
  mut owner := "source".clone()
  view: str := owner
  mut state := State { text: view }
  clear(state)
  owner = "replaced".clone()
  if owner.len() == 8 && state.text == "" { return 42 }
  return 0
}
"#;
        let unary_diagnostics = check_diagnostics("pkg-db-q6-unary-exact-retention", unary);
        assert!(
            unary_diagnostics.is_empty(),
            "a unary direct replacement must clear the obsolete owner fact:\n{unary_diagnostics}",
        );
    }

    #[test]
    fn eager_control_argument() {
        let eager_argument = r#"State { text: str }
fn retain(borrow mut state: State, source: str) { state.text = source }
fn main() -> i32 {
  mut owner := "source".clone()
  view: str := owner
  mut state := State { text: "" }
  retain(state, if true { view } else { "" })
  owner = "replaced".clone()
  return state.text.len() as i32
}
"#;
        let eager_diagnostics =
            check_diagnostics("pkg-db-q6-eager-argument-retention", eager_argument);
        assert!(
            eager_diagnostics.contains("use of invalidated borrow 'state'"),
            "a retained control-expression argument must preserve its completion-time owner:\n{eager_diagnostics}",
        );
    }

    #[test]
    fn transparent_try_exit() {
        let try_exit = r#"State { text: str }
fn retain_then_try(
  borrow mut state: State,
  source: str,
  pass: bool,
) -> Result<(), i64> {
  state.text = source
  result: Result<(), i64> := if pass { Ok(()) } else { Err(1) }
  _ := result?
  state.text = ""
  return Ok(())
}
fn main() -> i32 {
  mut owner := "source".clone()
  view: str := owner
  mut state := State { text: "" }
  attempted := retain_then_try(state, view, false)
  _ := match attempted { Ok(_) => 0 Err(_) => 1 }
  owner = "replaced".clone()
  return state.text.len() as i32
}
"#;
        let try_diagnostics = check_diagnostics("pkg-db-q6-try-exit-retention", try_exit);
        assert!(
            try_diagnostics.contains("use of invalidated borrow 'state'"),
            "the transparent `?` error edge must publish its mutable destination state:\n{try_diagnostics}",
        );
    }

    #[test]
    fn transparent_return_forward() {
        let return_forward = r#"State { text: str }
fn retain(borrow mut state: State, source: str) -> Result<(), i64> {
  state.text = source
  return Ok(())
}
fn forward(borrow mut state: State, source: str) -> Result<(), i64> {
  return retain(state, source)
}
fn main() -> i32 {
  mut owner := "source".clone()
  view: str := owner
  mut state := State { text: "" }
  attempted := forward(state, view)
  _ := match attempted { Ok(_) => 0 Err(_) => 1 }
  owner = "replaced".clone()
  return state.text.len() as i32
}
"#;
        let return_diagnostics =
            check_diagnostics("pkg-db-q6-return-forward-retention", return_forward);
        assert!(
            return_diagnostics.contains("use of invalidated borrow 'state'"),
            "a transparent return wrapper must publish transitive mutable retention:\n{return_diagnostics}",
        );
    }

    #[test]
    fn two_destination_region_snapshot() {
        let simultaneous_regions = r#"State { text: str }
fn swap(borrow mut first: State, borrow mut second: State) {
  saved := first
  first = second
  second = saved
}
fn main() -> i32 {
  arena outer {
    outer_text := "outer".clone_in(outer)
    mut outer_state := State { text: outer_text }
    arena inner {
      inner_text := "inner".clone_in(inner)
      mut inner_state := State { text: inner_text }
      swap(inner_state, outer_state)
    }
    return outer_state.text.len() as i32
  }
}
"#;
        let simultaneous_diagnostics = check_diagnostics(
            "pkg-db-q6-simultaneous-region-snapshot",
            simultaneous_regions,
        );
        assert!(
            simultaneous_diagnostics
                .contains("cannot retain a shorter-lived view through this mutable borrow",),
            "a two-destination swap must validate every source against the pre-call regions:\n{simultaneous_diagnostics}",
        );
    }

    #[test]
    fn borrowed_owned_storage() {
        let owned_storage = r#"State { values: slice<i64> }
fn retain_slice(borrow mut state: State, borrow source: array<i64>) {
  state.values = source[..]
}
fn main() -> i32 {
  mut source := [1, 2, 3].to_array()
  mut state := State { values: [] }
  retain_slice(state, source)
  source = [4, 5, 6].to_array()
  values := state.values
  return values.sum() as i32
}
"#;
        let owned_storage_diagnostics =
            check_diagnostics("pkg-db-q6-retained-owned-storage", owned_storage);
        assert!(
            owned_storage_diagnostics.contains("use of invalidated borrow 'state'"),
            "retaining a slice of a borrowed owned array must preserve its storage root:\n{owned_storage_diagnostics}",
        );
    }

    #[test]
    fn forwarded_owned_storage() {
        let forwarding_storage = r#"State { values: slice<i64> }
fn retain_slice(borrow mut state: State, borrow source: array<i64>) {
  state.values = source[..]
}
fn forward(borrow mut state: State) {
  source := [1, 2, 3].to_array()
  retain_slice(state, source)
}
fn main() -> i32 {
  mut state := State { values: [] }
  forward(state)
  values := state.values
  return values.sum() as i32
}
"#;
        let forwarding_storage_diagnostics =
            check_diagnostics("pkg-db-q6-forwarded-owned-storage", forwarding_storage);
        assert!(
            forwarding_storage_diagnostics
                .contains("cannot retain a shorter-lived view through this mutable borrow"),
            "a forwarding helper must not retain its local owned-array storage in a caller destination:\n{forwarding_storage_diagnostics}",
        );
    }

    #[test]
    fn forwarded_fixed_storage() {
        let forwarding_fixed_storage = r#"State { values: slice<i64> }
fn retain_slice(borrow mut state: State, source: slice<i64>) {
  state.values = source
}
fn forward(borrow mut state: State) {
  source := [1, 2, 3]
  retain_slice(state, source[..])
}
fn main() -> i32 {
  mut state := State { values: [] }
  forward(state)
  values := state.values
  return values.sum() as i32
}
"#;
        let forwarding_fixed_diagnostics = check_diagnostics(
            "pkg-db-q6-forwarded-fixed-storage",
            forwarding_fixed_storage,
        );
        assert!(
            forwarding_fixed_diagnostics
                .contains("cannot retain a shorter-lived view through this mutable borrow"),
            "the contained-edge control must keep fixed-array lexical storage frame-bounded:\n{forwarding_fixed_diagnostics}",
        );
    }

    #[test]
    fn imported_storage_fallback() {
        let imported_storage_fallback = [
            (
                "shaper.align",
                "module shaper\npub State { values: slice<i64> }\npub fn ignore(borrow mut state: State, borrow source: array<i64>) {}\n",
            ),
            (
                "main.align",
                "module main\nimport shaper\nfn forward(borrow mut state: shaper.State) { source := [1, 2, 3].to_array(); shaper.ignore(state, source) }\nfn main() -> i32 { mut state := shaper.State { values: [] }; forward(state); values := state.values; return values.sum() as i32 }\n",
            ),
        ];
        let imported_storage_checked = diff_check_multi(
            "pkg-db-q6-imported-storage-fallback",
            &imported_storage_fallback,
            "main.align",
        );
        assert!(
            !imported_storage_checked.whole_errors
                && imported_storage_checked
                    .per_unit_diags
                    .contains("cannot retain a shorter-lived view through this mutable borrow"),
            "an interface-only call must conservatively retain owned argument storage:\nwhole={}\nper-unit={}",
            imported_storage_checked.whole_diags,
            imported_storage_checked.per_unit_diags,
        );
    }

    #[test]
    fn indirect_storage_fallback() {
        let indirect_storage_fallback = r#"State { values: slice<i64> }
fn ignore(borrow mut state: State, borrow source: array<i64>) {
}
fn forward(borrow mut state: State) {
  source := [1, 2, 3].to_array()
  apply := ignore
  apply(state, source)
}
fn main() -> i32 {
  mut state := State { values: [] }
  forward(state)
  values := state.values
  return values.sum() as i32
}
"#;
        let indirect_storage_diagnostics = check_diagnostics(
            "pkg-db-q6-indirect-storage-fallback",
            indirect_storage_fallback,
        );
        assert!(
            indirect_storage_diagnostics
                .contains("cannot retain a shorter-lived view through this mutable borrow"),
            "an indirect call must conservatively retain owned argument storage:\n{indirect_storage_diagnostics}",
        );
    }

    #[test]
    fn contained_field_projection() {
        let contained_field = r#"Holder { text: str }
State { text: str }
fn retain_field(borrow mut state: State, borrow source: Holder) {
  state.text = source.text
}
fn main() -> i32 {
  owner := "source".clone()
  view: str := owner
  mut source := Holder { text: view }
  mut state := State { text: "" }
  retain_field(state, source)
  source = Holder { text: "other" }
  if source.text == "other" && state.text == "source" { return 42 }
  return 0
}
"#;
        let contained_field_diagnostics =
            check_diagnostics("pkg-db-q6-contained-field-retention", contained_field);
        assert!(
            contained_field_diagnostics.is_empty(),
            "contained-field retention must not pin the source aggregate header:\n{contained_field_diagnostics}",
        );
    }

    #[test]
    fn indirect_all_argument_fallback() {
        let indirect = r#"State { text: str }
fn copy(borrow mut state: State, source: str, out: region) {
  state.text = source.clone_in(out)
}
fn main() -> i32 {
  arena out {
    mut owner := "source".clone()
    source: str := owner
    mut state := State { text: "" }
    apply := copy
    apply(state, source, out)
    owner = "replaced".clone()
    return state.text.len() as i32
  }
}
"#;
        assert!(
            check_errs("pkg-db-q6-indirect-retention-fallback", indirect),
            "an indirect call must retain the conservative all-argument fallback",
        );
    }

    #[test]
    fn same_region_distinct_builders() {
        let distinct_builders = r#"Item { text: str }
fn push_both(
  borrow mut left: array_builder<Item>,
  borrow mut right: array_builder<Item>,
  source: str,
  out: region,
) {
  left.push(Item { text: source.clone_in(out) })
  right.push(Item { text: source.clone_in(out) })
}
fn main() -> i32 {
  arena out {
    mut left: array_builder<Item> := array_builder(out)
    mut right: array_builder<Item> := array_builder(out)
    push_both(left, right, "value", out)
    left_out := left.build()
    right_out := right.build()
    return (left_out.len() + right_out.len()) as i32
  }
}
"#;
        assert!(
            !check_errs("pkg-db-q6-same-region-distinct-builders", distinct_builders),
            "distinct builder places in one region must not be treated as storage aliases",
        );
    }

    #[test]
    fn nested_slice_alias() {
        let nested_alias = r#"fn touch(
  borrow mut first: slice<i64>,
  borrow mut second: slice<i64>,
) {}
fn main() -> i32 {
  mut values := [1, 2, 3]
  touch(values[..], values[..])
  return 0
}
"#;
        assert!(
            check_errs("pkg-db-q6-nested-slice-alias", nested_alias),
            "two mutable slices reaching the same storage must still reject",
        );
    }

    #[test]
    fn resource_owner_reference_alias() {
        let resource_alias = [
            (
                "pkg/resource/internal.align",
                "module pkg.resource.internal\npub fn drop_handle(handle: raw) { unsafe { raw.free(handle) } }\n",
            ),
            (
                "pkg/resource.align",
                "module pkg.resource\nimport pkg.resource.internal\npub resource Handle = pkg.resource.internal.drop_handle\npub fn open() -> Handle { unsafe { return resource.from_raw(raw.alloc(8)) } }\npub fn touch(borrow mut owner: Handle, reference: resource_ref<Handle>) {}\n",
            ),
            (
                "main.align",
                "module main\nimport pkg.resource\nfn main() -> i32 { mut owner := pkg.resource.open(); pkg.resource.touch(owner, resource.borrow(owner)); return 0 }\n",
            ),
        ];
        let resource_checked = diff_check_multi(
            "pkg-db-q6-nested-resource-alias",
            &resource_alias,
            "main.align",
        );
        assert!(
            resource_checked.whole_errors && resource_checked.per_unit_errors,
            "a mutable resource and its dependent reference must reject in both modes:\nwhole={}\nper-unit={}",
            resource_checked.whole_diags,
            resource_checked.per_unit_diags,
        );
    }

    #[test]
    fn resource_reference_pair_alias() {
        let resource_reference_alias = [
            (
                "pkg/resource/internal.align",
                "module pkg.resource.internal\npub fn drop_handle(handle: raw) { unsafe { raw.free(handle) } }\n",
            ),
            (
                "pkg/resource.align",
                "module pkg.resource\nimport pkg.resource.internal\npub resource Handle = pkg.resource.internal.drop_handle\npub fn open() -> Handle { unsafe { return resource.from_raw(raw.alloc(8)) } }\npub fn touch_refs(borrow mut first: resource_ref<Handle>, borrow mut second: resource_ref<Handle>) {}\n",
            ),
            (
                "main.align",
                "module main\nimport pkg.resource\nfn main() -> i32 { owner := pkg.resource.open(); mut first := resource.borrow(owner); mut second := resource.borrow(owner); pkg.resource.touch_refs(first, second); return 0 }\n",
            ),
        ];
        let resource_reference_checked = diff_check_multi(
            "pkg-db-q6-resource-reference-alias",
            &resource_reference_alias,
            "main.align",
        );
        assert!(
            resource_reference_checked.whole_errors && resource_reference_checked.per_unit_errors,
            "two mutable reference slots with one resource owner must reject in both modes:\nwhole={}\nper-unit={}",
            resource_reference_checked.whole_diags,
            resource_reference_checked.per_unit_diags,
        );
    }

    #[test]
    fn direct_local_dyn_storage() {
        let direct = r#"State { values: slice<i64> }
fn retain_local(borrow mut state: State) {
  source := [1, 2, 3].to_array()
  state.values = source[..]
}
fn main() -> i32 {
  mut state := State { values: [] }
  retain_local(state)
  values := state.values
  return values.sum() as i32
}
"#;
        let diagnostics = check_diagnostics("pkg-db-q6-direct-local-dyn-storage", direct);
        assert!(
            diagnostics.contains("cannot be stored into a longer-lived struct field"),
            "a borrow-mut callee must not retain a slice of its frame-owned dynamic array:\n{diagnostics}",
        );
    }

    #[test]
    fn direct_local_fixed_storage() {
        let direct = r#"State { values: slice<i64> }
fn retain_local(borrow mut state: State) {
  source := [1, 2, 3]
  state.values = source[..]
}
fn main() -> i32 {
  mut state := State { values: [] }
  retain_local(state)
  values := state.values
  return values.sum() as i32
}
"#;
        let diagnostics = check_diagnostics("pkg-db-q6-direct-local-fixed-storage", direct);
        assert!(
            diagnostics.contains("cannot be stored into a longer-lived struct field"),
            "a borrow-mut callee must not retain a slice of its fixed stack array:\n{diagnostics}",
        );
    }

    #[test]
    fn whole_assign_frame_view() {
        let whole = r#"State { text: str }
fn retain_whole(borrow mut state: State) {
  owner := "frame".clone()
  view: str := owner
  state = State { text: view }
}
fn main() -> i32 {
  mut state := State { text: "" }
  retain_whole(state)
  return state.text.len() as i32
}
"#;
        let diagnostics = check_diagnostics("pkg-db-q6-whole-assign-frame-view", whole);
        assert!(
            diagnostics.contains("cannot be stored into a longer-lived mutable destination"),
            "a whole-parameter replacement must not retain a frame-local view:\n{diagnostics}",
        );
    }

    #[test]
    fn whole_assign_frame_storage() {
        let whole = r#"State { values: slice<i64> }
fn retain_whole(borrow mut state: State) {
  source := [1, 2, 3].to_array()
  state = State { values: source[..] }
}
fn main() -> i32 {
  mut state := State { values: [] }
  retain_whole(state)
  values := state.values
  return values.sum() as i32
}
"#;
        let diagnostics = check_diagnostics("pkg-db-q6-whole-assign-frame-storage", whole);
        assert!(
            diagnostics.contains("cannot be stored into a longer-lived mutable destination"),
            "a whole-parameter replacement must not retain frame-owned dynamic-array storage:\n{diagnostics}",
        );
    }

    #[test]
    fn forwarded_dyn_storage() {
        let forwarded = r#"State { values: slice<i64> }
fn retain_slice(borrow mut state: State, source: slice<i64>) {
  state.values = source
}
fn forward(borrow mut state: State) {
  source := [1, 2, 3].to_array()
  retain_slice(state, source[..])
}
fn main() -> i32 {
  mut state := State { values: [] }
  forward(state)
  values := state.values
  return values.sum() as i32
}
"#;
        let diagnostics = check_diagnostics("pkg-db-q6-forwarded-dyn-storage", forwarded);
        assert!(
            diagnostics.contains("cannot retain a shorter-lived view through this mutable borrow"),
            "a forwarding helper must not retain a slice of its local dynamic array through the contained edge:\n{diagnostics}",
        );
    }
}

#[test]
fn sqlite_user_groups_one_parent_and_segmented_matrices_are_exact() {
    CASE_SQLITE_USER_GROUPS.run();
}

#[test]
fn sqlite_transaction_master_runs_on_connection_and_transaction() {
    CASE_SQLITE_TRANSACTION_MASTER.run();
}

#[test]
fn postgres_compound_shaping_dispatches_once_on_connection_and_transaction() {
    CASE_POSTGRES_COMPOUND.run();
}

#[test]
fn compound_failures_do_not_retry_or_construct_rows() {
    CASE_SQLITE_PRE_SEND_FAILURE.run();
    CASE_POSTGRES_SEND_FAILURE.run();
}

#[test]
#[ignore = "local Q6 high-fanout shaping measurement; timing is not a correctness or CI gate"]
fn high_fanout_shaping_measurement_reports_one_execution() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.user_groups
import std.time

extern "C" {
  fn align_sqlite_q6_reset()
  fn align_sqlite_q6_prepare_calls() -> i32
  fn align_sqlite_q6_step_calls() -> i32
  fn align_sqlite_q6_delivered_rows() -> i32
  fn align_sqlite_q6_finalize_calls() -> i32
  fn align_sqlite_q6_protocol_ok() -> i32
}

fn main() -> i32 {
  connection := pkg.db.sqlite.connect(":memory:", []) else { return 1 }
  unsafe { align_sqlite_q6_reset() }
  arena out {
    started := time.instant()
    shaped := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 8, last_user_id: 8 }, out,
    ) else { return 2 }
    value := shaped else { return 3 }
    elapsed := time.instant() - started
    print(value.groups.len())
    print(elapsed)
    if value.groups.len() != 10000 { return 4 }
  }
  return unsafe {
    if align_sqlite_q6_protocol_ok() != 1 { return 5 }
    if align_sqlite_q6_prepare_calls() != 1 { return 6 }
    if align_sqlite_q6_step_calls() != 10001 { return 7 }
    if align_sqlite_q6_delivered_rows() != 10000 { return 8 }
    if align_sqlite_q6_finalize_calls() != 1 { return 9 }
    42
  }
}
"#;
    let fixture = format!("{}\n{}", PG.c_source, SQLITE_Q6.c_source);
    let output = build_and_run_multi_with_c(
        "pkg-db-q6-high-fanout",
        &package_files(main).files(),
        "main.align",
        &fixture,
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(
        output.status.code(),
        Some(42),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// The Layer-1 migration's forward guard for this suite.
///
/// Regenerate ONLY with a reviewed reason, from the panic message this emits.
const LAYER1_FINGERPRINT_GOLDEN: &str = "\
pkg-db-q6-postgres-compound 106d86859e970e19
pkg-db-q6-postgres-send-failure ae08532600fc7010
pkg-db-q6-sqlite-pre-send-failure 5ec434a74adbd7ae
pkg-db-q6-sqlite-transaction-master 744ac2d59a00d4f8
pkg-db-q6-sqlite-user-groups 77007739f0587979
";

#[test]
fn layer1_case_fingerprints_match_the_golden() {
    let mut log = FingerprintLog::new();
    for case in LAYER1_CASES {
        log.record(&case.fingerprint());
    }
    log.assert_matches(LAYER1_FINGERPRINT_GOLDEN);
}
