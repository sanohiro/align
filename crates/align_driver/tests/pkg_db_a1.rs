//! pkg.db A1 owner tests: producer-owned common batch storage and borrowed SoA projection.

mod common;
use common::*;
mod db_harness;
use db_harness::*;
use std::sync::LazyLock;

static POSTGRES_PUBLIC: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/postgres.align"));

const TEST_HELPER: &str = r#"module pkg.db.a1_test
import pkg.db
import pkg.db.internal.descriptor
import pkg.db.internal.resource

pub fn set_rows_terminal<R>(borrow mut rows: pkg.db.rows<R>, state: u8) {
  unsafe { raw.store(resource.raw(resource.borrow(rows)), 5, state) }
}

pub fn set_rows_version<R>(borrow mut rows: pkg.db.rows<R>, version: u32) {
  unsafe { raw.store(resource.raw(resource.borrow(rows)), 0, version) }
}

pub fn set_rows_delivery<R>(borrow mut rows: pkg.db.rows<R>, delivery: u8) {
  unsafe { raw.store(resource.raw(resource.borrow(rows)), 96, delivery) }
}

pub fn set_rows_pending<R>(borrow mut rows: pkg.db.rows<R>, pending: u8) {
  unsafe { raw.store(resource.raw(resource.borrow(rows)), 97, pending) }
}

pub fn set_rows_prior_timeout<R>(borrow mut rows: pkg.db.rows<R>, prior: i32) {
  unsafe { raw.store(resource.raw(resource.borrow(rows)), 76, prior) }
}

pub fn set_rows_deadlines<R>(
  borrow mut rows: pkg.db.rows<R>, deadline: i64, duration: i64,
) {
  unsafe {
    wrapper := resource.raw(resource.borrow(rows))
    raw.store(wrapper, 104, deadline)
    raw.store(wrapper, 112, duration)
  }
}

pub fn clear_rows_context_native<R>(borrow mut rows: pkg.db.rows<R>) {
  unsafe {
    wrapper := resource.raw(resource.borrow(rows))
    context: raw := raw.load(wrapper, 24)
    raw.store(context, 8, raw.null())
  }
}

pub fn restore_rows_context_native<R>(borrow mut rows: pkg.db.rows<R>) {
  unsafe {
    wrapper := resource.raw(resource.borrow(rows))
    context: raw := raw.load(wrapper, 24)
    native: raw := raw.load(wrapper, 8)
    raw.store(context, 8, native)
  }
}

pub fn rows_shape<R>(
  borrow rows: pkg.db.rows<R>, driver: u8, terminal: u8, delivery: u8,
  pending: bool, portal: i32, deadline_present: bool,
) -> bool {
  unsafe {
    wrapper := resource.raw(resource.borrow(rows))
    version: u32 := raw.load(wrapper, 0)
    actual_driver: u8 := raw.load(wrapper, 4)
    actual_terminal: u8 := raw.load(wrapper, 5)
    actual_delivery: u8 := raw.load(wrapper, 96)
    actual_pending: u8 := raw.load(wrapper, 97)
    actual_portal: i32 := raw.load(wrapper, 100)
    deadline: i64 := raw.load(wrapper, 104)
    duration: i64 := raw.load(wrapper, 112)
    deadline_ok := if deadline_present {
      deadline >= 0 && duration > 0
    } else { deadline == -1 && duration == -1 }
    return pkg.db.internal.resource.rows_header_valid(wrapper) && version == 3
      && actual_driver == driver && actual_terminal == terminal
      && actual_delivery == delivery && actual_pending == (if pending { 1 } else { 0 })
      && actual_portal == portal && deadline_ok
  }
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

pub fn postgres_stream_fatal() -> pkg.db.query<PgParams, PgRow> = pkg.db.query(
  "SELECT u.id AS user_id, u.name AS user_name, g.id AS group_id, g.name AS group_name FROM users AS u LEFT JOIN user_groups AS ug ON ug.user_id = u.id LEFT JOIN groups AS g ON g.id = ug.group_id WHERE u.id >= :first_user_id AND u.id <= :last_user_id ORDER BY u.id, g.id /* Q6_USER_GROUPS STREAM_FATAL_AFTER_ONE */",
  [],
)

pub fn postgres_stream_missing_terminal() -> pkg.db.query<PgParams, PgRow> = pkg.db.query(
  "SELECT u.id AS user_id, u.name AS user_name, g.id AS group_id, g.name AS group_name FROM users AS u LEFT JOIN user_groups AS ug ON ug.user_id = u.id LEFT JOIN groups AS g ON g.id = ug.group_id WHERE u.id >= :first_user_id AND u.id <= :last_user_id ORDER BY u.id, g.id /* Q6_USER_GROUPS MISSING_TERMINAL */",
  [],
)

pub fn postgres_stream_post_terminal() -> pkg.db.query<PgParams, PgRow> = pkg.db.query(
  "SELECT u.id AS user_id, u.name AS user_name, g.id AS group_id, g.name AS group_name FROM users AS u LEFT JOIN user_groups AS ug ON ug.user_id = u.id LEFT JOIN groups AS g ON g.id = ug.group_id WHERE u.id >= :first_user_id AND u.id <= :last_user_id ORDER BY u.id, g.id /* Q6_USER_GROUPS POST_TERMINAL_FATAL */",
  [],
)

pub fn postgres_stream_copy() -> pkg.db.query<PgParams, PgRow> = pkg.db.query(
  "SELECT u.id AS user_id, u.name AS user_name, g.id AS group_id, g.name AS group_name FROM users AS u LEFT JOIN user_groups AS ug ON ug.user_id = u.id LEFT JOIN groups AS g ON g.id = ug.group_id WHERE u.id >= :first_user_id AND u.id <= :last_user_id ORDER BY u.id, g.id /* Q6_USER_GROUPS STREAM_COPY */",
  [],
)

pub fn postgres_stream_timeout() -> pkg.db.query<PgParams, PgRow> = pkg.db.query(
  "SELECT u.id AS user_id, u.name AS user_name, g.id AS group_id, g.name AS group_name FROM users AS u LEFT JOIN user_groups AS ug ON ug.user_id = u.id LEFT JOIN groups AS g ON g.id = ug.group_id WHERE u.id >= :first_user_id AND u.id <= :last_user_id ORDER BY u.id, g.id /* Q6_USER_GROUPS TIMEOUT_AFTER_ONE */",
  [],
)

pub fn postgres_command() -> pkg.db.command<PgParams> = pkg.db.command(
  "UPDATE users SET id = id WHERE id >= :first_user_id AND id <= :last_user_id /* COMMAND_OK */",
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

const LIVE_POSTGRES_QUERY: &str = r#"module app.live_stream
import pkg.db
import pkg.db.postgres

pub Params { first_value: i64, last_value: i64 }
pub DelayParams { seconds: f64 }
pub ValueParams { value: i64 }
pub Row { value: i64, label: str }

pub fn values() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT value::bigint AS value, ('row-' || value::text)::text AS label FROM generate_series(:first_value, :last_value) AS value ORDER BY value",
  [],
  [
    pkg.db.postgres.QueryOption.ParameterType("first_value", "int8"),
    pkg.db.postgres.QueryOption.ParameterType("last_value", "int8"),
  ],
)

pub fn delayed() -> pkg.db.query<DelayParams, Row> = pkg.db.postgres.query(
  "SELECT 1::bigint AS value, 'delayed'::text AS label FROM (SELECT pg_sleep(:seconds)) AS waited",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("seconds", "float8")],
)

pub fn create_effects() -> pkg.db.command<ValueParams> = pkg.db.postgres.command(
  "CREATE TEMP TABLE align_a1_stream_effect AS SELECT :value::bigint AS value", [],
  [pkg.db.postgres.CommandOption.ParameterType("value", "int8")],
)

pub fn change_effect() -> pkg.db.query<ValueParams, Row> = pkg.db.postgres.query(
  "UPDATE align_a1_stream_effect SET value = value + :value RETURNING value, 'effect'::text AS label",
  [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)

pub fn current_effect() -> pkg.db.query<ValueParams, Row> = pkg.db.postgres.query(
  "SELECT value, 'effect'::text AS label FROM align_a1_stream_effect WHERE :value = :value", [],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int8")],
)
"#;

const LIVE_POSTGRES_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.live_stream

fn params(first_value: i64, last_value: i64) -> app.live_stream.Params {
  return app.live_stream.Params { first_value: first_value, last_value: last_value }
}

fn transaction_effect(borrow transaction: pkg.db.tx) -> bool {
  mut tx_changed := pkg.db.postgres.rows_native(
    pkg.db.exec_tx(transaction), app.live_stream.change_effect(),
    app.live_stream.ValueParams { value: 5 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(1))],
  ) else { return false }
  tx_row := pkg.db.next(tx_changed) else { return false }
  tx_value := tx_row else { return false }
  if tx_value.value != 47 { return false }
  match pkg.db.next(tx_changed) else { return false } { Some(_) => { return false } None => {} }
  mut in_tx := pkg.db.rows(
    pkg.db.exec_tx(transaction), app.live_stream.current_effect(),
    app.live_stream.ValueParams { value: 0 }, [],
  ) else { return false }
  in_tx_row := pkg.db.next(in_tx) else { return false }
  in_tx_value := in_tx_row else { return false }
  return in_tx_value.value == 47
}

fn run(url: str) -> i32 {
  connection := pkg.db.postgres.connect(url, []) else { return 1 }

  setup := pkg.db.execute(
    pkg.db.exec_conn(connection), app.live_stream.create_effects(),
    app.live_stream.ValueParams { value: 40 }, [],
  ) else { return 40 }
  mut changed := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.live_stream.change_effect(),
    app.live_stream.ValueParams { value: 2 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  ) else { return 41 }
  changed_row := pkg.db.next(changed) else { return 43 }
  changed_value := changed_row else { return 44 }
  if changed_value.value != 42 { return 45 }
  match pkg.db.next(changed) else { return 46 } { Some(_) => { return 47 } None => {} }

  mut singles := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.live_stream.values(), params(1, 5), [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  ) else { return 2 }
  first := pkg.db.next_batch(singles, 3) else { return 3 }
  first_batch := first else { return 4 }
  second := pkg.db.next_batch(singles, 3) else { return 5 }
  second_batch := second else { return 6 }
  if (pkg.db.batch_len(first_batch) else { return 7 }) != 3
    || (pkg.db.batch_len(second_batch) else { return 8 }) != 2 { return 9 }
  first_row := pkg.db.batch_row(first_batch, 0) else { return 10 }
  first_value := first_row else { return 11 }
  last_row := pkg.db.batch_row(second_batch, 1) else { return 12 }
  last_value := last_row else { return 13 }
  if first_value.value != 1 || first_value.label != "row-1"
    || last_value.value != 5 || last_value.label != "row-5" { return 14 }
  match pkg.db.next_batch(singles, 3) else { return 15 } {
    Some(_) => { return 16 }
    None => {}
  }

  mut chunks := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.live_stream.values(), params(1, 5), [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(2))],
  ) else { return 17 }
  head := pkg.db.next(chunks) else { return 18 }
  head_value := head else { return 19 }
  if head_value.value != 1 { return 20 }
  middle := pkg.db.next_batch(chunks, 3) else { return 21 }
  middle_batch := middle else { return 22 }
  tail := pkg.db.next_batch(chunks, 3) else { return 23 }
  tail_batch := tail else { return 24 }
  if (pkg.db.batch_len(middle_batch) else { return 25 }) != 3
    || (pkg.db.batch_len(tail_batch) else { return 26 }) != 1 { return 27 }
  match pkg.db.next(chunks) else { return 28 } {
    Some(_) => { return 29 }
    None => {}
  }

  arena out {
    singleton := pkg.db.postgres.one_native(
      pkg.db.exec_conn(connection), app.live_stream.values(), params(9, 9), out, [],
      [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
    ) else { return 30 }
    if singleton.value != 9 || singleton.label != "row-9" { return 31 }
  }
  arena out {
    many := pkg.db.postgres.one_native(
      pkg.db.exec_conn(connection), app.live_stream.values(), params(1, 2), out, [],
      [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(1))],
    )
    many_is_cardinality := match many {
      Err(error) => match error { Cardinality(value) => value.observed_at_least == 2, _ => false }
      Ok(_) => false
    }
    if !many_is_cardinality { return 32 }
  }

  mut delayed := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.live_stream.delayed(),
    app.live_stream.DelayParams { seconds: 0.5 },
    [pkg.db.ExecuteOption.TimeoutNs(100000000)],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  ) else { return 37 }
  delayed_result := pkg.db.next(delayed)
  delayed_timed_out := match delayed_result {
    Err(error) => match error { Timeout(_) => true, _ => false }
    Ok(_) => false
  }
  if !delayed_timed_out { return 38 }

  mut buffered := pkg.db.rows(
    pkg.db.exec_conn(connection), app.live_stream.values(), params(42, 42), [],
  ) else { return 33 }
  reused := pkg.db.next(buffered) else { return 34 }
  reused_value := reused else { return 35 }
  if reused_value.value != 42 { return 36 }

  tx_connection := pkg.db.postgres.connect(url, []) else { return 48 }
  tx_setup := pkg.db.execute(
    pkg.db.exec_conn(tx_connection), app.live_stream.create_effects(),
    app.live_stream.ValueParams { value: 42 }, [],
  ) else { return 49 }
  transaction := pkg.db.begin(tx_connection, []) else { return 50 }
  if !transaction_effect(transaction) { return 51 }
  returned := pkg.db.rollback(transaction) else { return 59 }
  mut after_rollback := pkg.db.rows(
    pkg.db.exec_conn(returned), app.live_stream.current_effect(),
    app.live_stream.ValueParams { value: 0 }, [],
  ) else { return 60 }
  restored_row := pkg.db.next(after_rollback) else { return 61 }
  restored_value := restored_row else { return 62 }
  if restored_value.value != 42 { return 63 }
  return 42
}

fn main(args: array<str>) -> Result<(), Error> {
  print(run(args[1]))
  return Ok(())
}
"#;


// ================================================================================================
// Layer-1 migrated cases.
//
// Native call counts moved out of the Align epilogues into `expect_counters`, so a mismatch names
// the counter instead of returning a hand-numbered sentinel.
// ================================================================================================
const ZERO_COLUMN_PLAN_MAIN: &str = r#"module main
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

const SQLITE_BATCHES_MAIN: &str = r#"module main
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

  if !pkg.db.a1_test.rows_shape(stream, 1 as u8, 0 as u8, 3 as u8, false, 0, false) {
    return 45
  }
  pkg.db.a1_test.set_rows_version(stream, 2 as u32)
  malformed_version := pkg.db.next_batch(stream, 1)
  pkg.db.a1_test.set_rows_version(stream, 3 as u32)
  match malformed_version {
    Err(error) => match error { InvalidQuery(_) => {} _ => { return 46 } }
    Ok(_) => { return 46 }
  }
  pkg.db.a1_test.set_rows_delivery(stream, 1 as u8)
  malformed_delivery := pkg.db.next_batch(stream, 1)
  pkg.db.a1_test.set_rows_delivery(stream, 3 as u8)
  match malformed_delivery {
    Err(error) => match error { InvalidQuery(_) => {} _ => { return 47 } }
    Ok(_) => { return 47 }
  }
  pkg.db.a1_test.set_rows_pending(stream, 1 as u8)
  malformed_pending := pkg.db.next_batch(stream, 1)
  pkg.db.a1_test.set_rows_pending(stream, 0 as u8)
  match malformed_pending {
    Err(error) => match error { InvalidQuery(_) => {} _ => { return 48 } }
    Ok(_) => { return 48 }
  }
  pkg.db.a1_test.set_rows_prior_timeout(stream, -1 as i32)
  malformed_prior := pkg.db.next_batch(stream, 1)
  pkg.db.a1_test.set_rows_prior_timeout(stream, 0 as i32)
  match malformed_prior {
    Err(error) => match error { InvalidQuery(_) => {} _ => { return 52 } }
    Ok(_) => { return 52 }
  }
  pkg.db.a1_test.set_rows_deadlines(stream, 0, -1)
  malformed_deadline := pkg.db.next_batch(stream, 1)
  pkg.db.a1_test.set_rows_deadlines(stream, -1, -1)
  match malformed_deadline {
    Err(error) => match error { InvalidQuery(_) => {} _ => { return 49 } }
    Ok(_) => { return 49 }
  }
  pkg.db.a1_test.clear_rows_context_native(stream)
  malformed_context := pkg.db.next_batch(stream, 1)
  pkg.db.a1_test.restore_rows_context_native(stream)
  match malformed_context {
    Err(error) => match error { InvalidQuery(_) => {} _ => { return 50 } }
    Ok(_) => { return 50 }
  }

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
  if !pkg.db.a1_test.rows_shape(stream, 1 as u8, 1 as u8, 0 as u8, false, 0, false) {
    return 51
  }

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

const POSTGRES_BUFFERED_BATCHES_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query
import pkg.db.a1_test
import pkg.db.testkit.pg


fn main() -> i32 {
  pkg.db.testkit.pg.reset()
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
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const POSTGRES_VALUE_MATRIX_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query
import pkg.db.testkit.pg


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
  pkg.db.testkit.pg.reset()
  connection := pkg.db.postgres.connect("postgresql://stub/a1-matrix", []) else { return 30 }
  first := execute_once(connection, true)
  if first != 0 { return first }
  second := execute_once(connection, false)
  if second != 0 { return second }
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const POSTGRES_BATCH_DECODE_FAILURE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query
import pkg.db.testkit.pg


fn main() -> i32 {
  pkg.db.testkit.pg.reset()
  connection := pkg.db.postgres.connect("postgresql://stub/a1-error", []) else { return 1 }
  bytes := [1 as u8, 2 as u8, 3 as u8]
  rows_result := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection),
    app.batch_query.postgres_bad_view(),
    app.batch_query.PgViewParams { id: 7, label: "first", payload: bytes[..] },
    [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
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
  mut reused := pkg.db.rows(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
  ) else { return 5 }
  if match pkg.db.next(reused) else { return 6 } { Some(_) => false, None => true } { return 7 }
  match pkg.db.next(reused) else { return 8 } { Some(_) => { return 9 } None => {} }
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const POSTGRES_STREAMED_MODES_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query
import pkg.db.a1_test
import pkg.db.testkit.pg

fn conn_buffered(borrow connection: pkg.db.conn) -> bool {
  mut buffered := pkg.db.rows(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }, [],
  ) else { return false }
  if !pkg.db.a1_test.rows_shape(buffered, 2 as u8, 0 as u8, 0 as u8, false, 0, false) {
    return false
  }
  all := pkg.db.next_batch(buffered, 8) else { return false }
  all_batch := all else { return false }
  if (pkg.db.batch_len(all_batch) else { return false }) != 4 { return false }
  return pkg.db.a1_test.rows_shape(
    buffered, 2 as u8, 1 as u8, 0 as u8, false, 0, false,
  )
}

fn conn_single(borrow connection: pkg.db.conn) -> i32 {
  mut singles := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  ) else { return 2 }
  if !pkg.db.a1_test.rows_shape(singles, 2 as u8, 0 as u8, 1 as u8, true, 0, false) {
    return 28
  }
  pkg.db.a1_test.set_rows_pending(singles, 0 as u8)
  malformed_stream := pkg.db.next(singles)
  match malformed_stream {
    Err(error) => match error { InvalidQuery(_) => {} _ => { return 34 } }
    Ok(_) => { return 34 }
  }
  pkg.db.a1_test.set_rows_pending(singles, 1 as u8)
  pkg.db.a1_test.set_rows_prior_timeout(singles, 1 as i32)
  malformed_prior := pkg.db.next(singles)
  match malformed_prior {
    Err(error) => match error { InvalidQuery(_) => {} _ => { return 35 } }
    Ok(_) => { return 35 }
  }
  pkg.db.a1_test.set_rows_prior_timeout(singles, 0 as i32)
  first := pkg.db.next_batch(singles, 3) else { return 3 }
  first_batch := first else { return 4 }
  second := pkg.db.next_batch(singles, 3) else { return 5 }
  second_batch := second else { return 6 }
  if (pkg.db.batch_len(first_batch) else { return 7 }) != 3
    || (pkg.db.batch_len(second_batch) else { return 8 }) != 1 { return 9 }
  if !pkg.db.a1_test.rows_shape(singles, 2 as u8, 1 as u8, 0 as u8, false, 0, false) {
    return 29
  }
  if match pkg.db.next_batch(singles, 3) else { return 10 } {
    Some(_) => true
    None => false
  } { return 11 }
  return 0
}

fn conn_chunk(borrow connection: pkg.db.conn) -> i32 {
  mut chunks := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(2))],
  ) else { return 12 }
  if !pkg.db.a1_test.rows_shape(chunks, 2 as u8, 0 as u8, 2 as u8, true, 2, false) {
    return 30
  }
  first_row := pkg.db.next(chunks) else { return 13 }
  row := first_row else { return 14 }
  if row.user_id != 1 || row.user_name != "Alice" { return 15 }
  middle := pkg.db.next_batch(chunks, 2) else { return 16 }
  middle_batch := middle else { return 17 }
  tail := pkg.db.next_batch(chunks, 2) else { return 18 }
  tail_batch := tail else { return 19 }
  if (pkg.db.batch_len(middle_batch) else { return 20 }) != 2
    || (pkg.db.batch_len(tail_batch) else { return 21 }) != 1 { return 22 }
  if !pkg.db.a1_test.rows_shape(chunks, 2 as u8, 1 as u8, 0 as u8, false, 0, false) {
    return 31
  }
  return 0
}

fn tx_single(borrow transaction: pkg.db.tx) -> bool {
  mut stream := pkg.db.postgres.rows_native(
    pkg.db.exec_tx(transaction), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  ) else { return false }
  batch := pkg.db.next_batch(stream, 8) else { return false }
  values := batch else { return false }
  return (pkg.db.batch_len(values) else { return false }) == 4
}

fn tx_chunk(borrow transaction: pkg.db.tx) -> bool {
  mut stream := pkg.db.postgres.rows_native(
    pkg.db.exec_tx(transaction), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(2))],
  ) else { return false }
  batch := pkg.db.next_batch(stream, 8) else { return false }
  values := batch else { return false }
  return (pkg.db.batch_len(values) else { return false }) == 4
}

fn tx_buffered(borrow transaction: pkg.db.tx) -> bool {
  mut stream := pkg.db.rows(
    pkg.db.exec_tx(transaction), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }, [],
  ) else { return false }
  batch := pkg.db.next_batch(stream, 8) else { return false }
  values := batch else { return false }
  return (pkg.db.batch_len(values) else { return false }) == 4
}

fn main() -> i32 {
  pkg.db.testkit.pg.reset()
  connection := pkg.db.postgres.connect("postgresql://stub/a1-stream", []) else { return 1 }
  single_status := conn_single(connection)
  if single_status != 0 { return single_status }
  chunk_status := conn_chunk(connection)
  if chunk_status != 0 { return chunk_status }
  if !conn_buffered(connection) { return 23 }

  transaction := pkg.db.begin(connection, []) else { return 35 }
  if !tx_single(transaction) { return 36 }
  if !tx_chunk(transaction) { return 41 }
  if !tx_buffered(transaction) { return 46 }
  returned := pkg.db.rollback(transaction) else { return 51 }
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const POSTGRES_STREAMED_ONE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query
import pkg.db.testkit.pg

fn cardinality_two(result: Result<app.batch_query.PgRow, pkg.db.Error>) -> bool {
  return match result {
    Err(error) => match error {
      Cardinality(value) => value.observed_at_least == 2
      _ => false
    }
    Ok(_) => false
  }
}

fn serialization(result: Result<app.batch_query.PgRow, pkg.db.Error>) -> bool {
  return match result {
    Err(error) => match error {
      Serialization(_) => true
      _ => false
    }
    Ok(_) => false
  }
}

fn reuse_conn(borrow connection: pkg.db.conn) -> bool {
  mut stream := pkg.db.rows(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
  ) else { return false }
  first := pkg.db.next(stream) else { return false }
  match first { None => { return false } Some(_) => {} }
  exhausted := pkg.db.next(stream) else { return false }
  return match exhausted { Some(_) => false, None => true }
}

fn tx_single_one(borrow transaction: pkg.db.tx) -> bool {
  arena values {
    result := pkg.db.postgres.one_native(
      pkg.db.exec_tx(transaction), app.batch_query.postgres_rows(),
      app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, values, [],
      [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
    )
    return match result { Ok(row) => row.user_id == 1, Err(_) => false }
  }
}

fn tx_portal_one(borrow transaction: pkg.db.tx) -> bool {
  arena values {
    result := pkg.db.postgres.one_native(
      pkg.db.exec_tx(transaction), app.batch_query.postgres_rows(),
      app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }, values, [],
      [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(2))],
    )
    return cardinality_two(result)
  }
}

fn tx_buffered_one(borrow transaction: pkg.db.tx) -> bool {
  arena values {
    result := pkg.db.postgres.one_native(
      pkg.db.exec_tx(transaction), app.batch_query.postgres_rows(),
      app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, values, [], [],
    )
    return match result { Ok(row) => row.user_id == 1, Err(_) => false }
  }
}

fn main() -> i32 {
  pkg.db.testkit.pg.reset()
  connection := pkg.db.postgres.connect("postgresql://stub/a1-one", []) else { return 1 }
  arena values {
    singleton := pkg.db.postgres.one_native(
      pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
      app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, values, [],
      [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
    ) else { return 2 }
    if singleton.user_id != 1 || singleton.user_name != "Alice" { return 3 }
  }
  arena values {
    many := pkg.db.postgres.one_native(
      pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
      app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }, values, [],
      [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(2))],
    )
    if !cardinality_two(many) { return 4 }
  }
  arena values {
    late := pkg.db.postgres.one_native(
      pkg.db.exec_conn(connection), app.batch_query.postgres_stream_fatal(),
      app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }, values, [],
      [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
    )
    if !serialization(late) { return 5 }
  }
  if !reuse_conn(connection) { return 6 }
  transaction := pkg.db.begin(connection, []) else { return 7 }
  if !tx_single_one(transaction) { return 8 }
  if !tx_portal_one(transaction) { return 9 }
  if !tx_buffered_one(transaction) { return 10 }
  returned := pkg.db.rollback(transaction) else { return 11 }
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const POSTGRES_DELIVERY_VALIDATION_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query
import pkg.db.testkit.pg

fn rows_contract(
  result: Result<pkg.db.rows<app.batch_query.PgRow>, pkg.db.Error>,
  message: str,
) -> bool {
  return match result {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "postgres.execute.delivery"
        && contract.message == message
      _ => false
    }
    Ok(_) => false
  }
}

fn command_contract(result: Result<pkg.db.exec_result, pkg.db.Error>) -> bool {
  return match result {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "postgres.execute.delivery"
        && contract.message == "PostgreSQL row delivery requires a Query result"
      _ => false
    }
    Ok(_) => false
  }
}

fn main() -> i32 {
  pkg.db.testkit.pg.reset()
  connection := pkg.db.postgres.connect("postgresql://stub/a1-options", []) else { return 1 }
  target := pkg.db.exec_conn(connection)
  params := app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }
  invalid_after := pkg.db.postgres.rows_native(
    target, app.batch_query.postgres_rows(), params, [],
    [
      pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow),
      pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(0)),
    ],
  )
  if !rows_contract(
    invalid_after,
    "PostgreSQL portal batch size must be between 1 and 2147483647 rows",
  ) { return 2 }
  invalid_before := pkg.db.postgres.rows_native(
    target, app.batch_query.postgres_rows(), params, [],
    [
      pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(0)),
      pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow),
    ],
  )
  if !rows_contract(
    invalid_before,
    "PostgreSQL portal batch size must be between 1 and 2147483647 rows",
  ) { return 3 }
  duplicate := pkg.db.postgres.rows_native(
    target, app.batch_query.postgres_rows(), params, [],
    [
      pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow),
      pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(1)),
    ],
  )
  if !rows_contract(duplicate, "duplicate PostgreSQL row delivery option") { return 4 }
  rejected_command := pkg.db.postgres.execute_native(
    target, app.batch_query.postgres_command(), params, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(0))],
  )
  if !command_contract(rejected_command) { return 5 }
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const POSTGRES_STREAMED_FAILURES_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query
import pkg.db.testkit.pg

extern "C" {
  fn align_pg_fail_next_row_mode()
  fn align_pg_fail_next_nonblocking_restore()
  fn align_pg_forbidden_after_status_calls() -> i32
}

fn sequence<T>(result: Result<T, pkg.db.Error>) -> bool {
  return match result {
    Err(error) => match error {
      InvalidQuery(contract) => contract.item == "postgres.rows.delivery"
        && contract.message == "PostgreSQL streamed result sequence is invalid"
      _ => false
    }
    Ok(_) => false
  }
}

fn cleanup<T>(result: Result<T, pkg.db.Error>) -> bool {
  return match result {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "db.rows.cleanup"
        && contract.message
          == "PostgreSQL streamed rows cleanup failed and the connection was closed"
      _ => false
    }
    Ok(_) => false
  }
}

fn main() -> i32 {
  pkg.db.testkit.pg.reset()
  connection := pkg.db.postgres.connect("postgresql://stub/a1-failures", []) else { return 1 }
  mut missing := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.batch_query.postgres_stream_missing_terminal(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  ) else { return 2 }
  first := pkg.db.next(missing) else { return 3 }
  match first { None => { return 4 } Some(_) => {} }
  if !sequence(pkg.db.next(missing)) { return 5 }

  mut post_terminal := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.batch_query.postgres_stream_post_terminal(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(2))],
  ) else { return 6 }
  if match pkg.db.next(post_terminal) else { return 7 } { Some(_) => false, None => true } {
    return 8
  }
  if !sequence(pkg.db.next(post_terminal)) { return 9 }

  unsafe { align_pg_fail_next_row_mode() }
  rejected := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  )
  rejected_ok := match rejected {
    Err(error) => match error {
      InvalidQuery(contract) => contract.message
        == "PostgreSQL client rejected the selected row delivery mode"
      _ => false
    }
    Ok(_) => false
  }
  if !rejected_ok { return 10 }

  mut reused := pkg.db.rows(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
  ) else { return 11 }
  if match pkg.db.next(reused) else { return 12 } { Some(_) => false, None => true } { return 13 }

  restore_closed := pkg.db.postgres.connect("postgresql://stub/a1-restore", []) else { return 19 }
  mut restore_rows := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(restore_closed), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(2))],
  ) else { return 20 }
  prefix := pkg.db.next_batch(restore_rows, 3) else { return 21 }
  prefix_batch := prefix else { return 22 }
  if (pkg.db.batch_len(prefix_batch) else { return 23 }) != 3 { return 24 }
  unsafe { align_pg_fail_next_nonblocking_restore() }
  if !cleanup(pkg.db.next_batch(restore_rows, 3)) { return 25 }
  repeated_restore := pkg.db.next(restore_rows)
  repeated_restore_failed := match repeated_restore {
    Err(error) => match error {
      InvalidQuery(contract) => contract.item == "db.rows.state"
      _ => false
    }
    Ok(_) => false
  }
  if !repeated_restore_failed { return 27 }
  restore_poisoned := pkg.db.rows(
    pkg.db.exec_conn(restore_closed), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
  )
  match restore_poisoned {
    Err(_) => {}
    Ok(_) => { return 26 }
  }

  closed := pkg.db.postgres.connect("postgresql://stub/a1-copy", []) else { return 14 }
  mut copy_rows := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(closed), app.batch_query.postgres_stream_copy(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  ) else { return 15 }
  if !sequence(pkg.db.next(copy_rows)) { return 16 }
  if unsafe { align_pg_forbidden_after_status_calls() } != 0 { return 17 }
  poisoned := pkg.db.rows(
    pkg.db.exec_conn(closed), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
  )
  poisoned_ok := match poisoned {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "db.connection.state"
        || contract.item == "db.exec.state"
      _ => false
    }
    Ok(_) => false
  }
  if !poisoned_ok { return 18 }
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const POSTGRES_STREAMED_TIMEOUT_DROP_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query
import pkg.db.testkit.pg

extern "C" {
  fn align_pg_execute_calls() -> i32
  fn align_pg_cancel_calls() -> i32
  fn align_pg_single_row_mode_calls() -> i32
  fn align_pg_delay_next_nonblocking_enable()
  fn align_pg_fail_next_nonblocking_restore()
}

fn timeout<T>(result: Result<T, pkg.db.Error>) -> bool = match result {
  Err(error) => match error { Timeout(_) => true, _ => false }
  Ok(_) => false
}

fn drop_early(borrow connection: pkg.db.conn) -> i32 {
  opened := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(2))],
  )
  stream := opened else { return 1 }
  return 0
}

fn consume_one(borrow connection: pkg.db.conn) -> bool {
  opened := pkg.db.rows(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
  )
  mut stream := opened else { return false }
  return match pkg.db.next(stream) {
    Err(_) => false
    Ok(value) => match value { Some(_) => true, None => false }
  }
}

fn pre_send_expiry(borrow connection: pkg.db.conn) -> bool {
  execute_before := unsafe { align_pg_execute_calls() }
  cancel_before := unsafe { align_pg_cancel_calls() }
  selector_before := unsafe { align_pg_single_row_mode_calls() }
  unsafe { align_pg_delay_next_nonblocking_enable() }
  expired := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 },
    [pkg.db.ExecuteOption.TimeoutNs(1000000)],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  )
  return timeout(expired)
    && unsafe { align_pg_execute_calls() } == execute_before
    && unsafe { align_pg_cancel_calls() } == cancel_before
    && unsafe { align_pg_single_row_mode_calls() } == selector_before
}

fn pre_send_restore_failure(borrow connection: pkg.db.conn) -> bool {
  execute_before := unsafe { align_pg_execute_calls() }
  unsafe {
    align_pg_delay_next_nonblocking_enable()
    align_pg_fail_next_nonblocking_restore()
  }
  expired := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 },
    [pkg.db.ExecuteOption.TimeoutNs(1000000)],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  )
  if !timeout(expired) || unsafe { align_pg_execute_calls() } != execute_before { return false }
  poisoned := pkg.db.rows(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
  )
  return match poisoned {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "db.connection.state"
        || contract.item == "db.exec.state"
      _ => false
    }
    Ok(_) => false
  }
}

fn main() -> i32 {
  pkg.db.testkit.pg.reset()
  connection := pkg.db.postgres.connect("postgresql://stub/a1-timeout", []) else { return 1 }
  mut timed := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.batch_query.postgres_stream_timeout(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 3 },
    [pkg.db.ExecuteOption.TimeoutNs(10000000)],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  ) else { return 2 }
  match pkg.db.next(timed) else { return 3 } { None => { return 4 } Some(_) => {} }
  timed_out := pkg.db.next(timed)
  timeout_ok := match timed_out {
    Err(error) => match error { Timeout(_) => true, _ => false }
    Ok(_) => false
  }
  if !timeout_ok || !consume_one(connection) { return 5 }
  if drop_early(connection) != 0 { return 6 }
  if !consume_one(connection) { return 7 }
  before_send := pkg.db.postgres.connect("postgresql://stub/a1-pre-send", []) else { return 8 }
  if !pre_send_expiry(before_send) || !consume_one(before_send) { return 9 }
  restore_fail := pkg.db.postgres.connect("postgresql://stub/a1-restore", []) else { return 10 }
  if !pre_send_restore_failure(restore_fail) { return 11 }
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const POSTGRES_STREAMED_UNSAFE_STATUSES_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.batch_query
import pkg.db.testkit.pg

extern "C" {
  fn align_pg_force_result_status(status: i32)
  fn align_pg_forbidden_after_status_calls() -> i32
}

fn invalid_sequence<T>(result: Result<T, pkg.db.Error>) -> bool = match result {
  Err(error) => match error {
    InvalidQuery(contract) => contract.item == "postgres.rows.delivery"
      && contract.message == "PostgreSQL streamed result sequence is invalid"
    _ => false
  }
  Ok(_) => false
}

fn exercise(status: i32) -> bool {
  pkg.db.testkit.pg.reset()
  connection := pkg.db.postgres.connect("postgresql://stub/a1-unsafe", []) else { return false }
  unsafe { align_pg_force_result_status(status) }
  mut stream := pkg.db.postgres.rows_native(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
    [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.SingleRow)],
  ) else { return false }
  failed := invalid_sequence(pkg.db.next(stream))
  unsafe { align_pg_force_result_status(-1) }
  if !failed || unsafe { align_pg_forbidden_after_status_calls() } != 0 { return false }
  reuse := pkg.db.rows(
    pkg.db.exec_conn(connection), app.batch_query.postgres_rows(),
    app.batch_query.PgParams { first_user_id: 1, last_user_id: 1 }, [],
  )
  return match reuse { Err(_) => true, Ok(_) => false }
}

fn main() -> i32 {
  statuses := [3 as i32, 4 as i32, 8 as i32, 10 as i32, 11 as i32, 99 as i32]
  mut i: i64 := 0
  loop {
    if i >= statuses.len() { break }
    if !exercise(statuses[i]) { return (i as i32) + 1 }
    i = i + 1
  }
  pkg.db.testkit.pg.dump()
  return 42
}
"#;

const CASE_ZERO_COLUMN_PLAN: Case = Case {
    label: "pkg-db-a1-zero-column-plan",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: a1_modules,
    main: ZERO_COLUMN_PLAN_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const CASE_SQLITE_BATCHES: Case = Case {
    label: "pkg-db-a1-sqlite-batches",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: a1_modules,
    main: SQLITE_BATCHES_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[],
};

const CASE_POSTGRES_BUFFERED_BATCHES: Case = Case {
    label: "pkg-db-a1-postgres-buffered-batches",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[&PG],
    modules: a1_modules,
    main: POSTGRES_BUFFERED_BATCHES_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[
        ("pg.protocol_ok", 1),
        ("pg.execute_calls", 1),
        ("pg.clear_calls", 1),
        ("pg.delivered_rows", 4),
    ],
};

const CASE_POSTGRES_VALUE_MATRIX: Case = Case {
    label: "pkg-db-a1-postgres-value-matrix",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[&PG],
    modules: a1_modules,
    main: POSTGRES_VALUE_MATRIX_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[
        ("pg.protocol_ok", 1),
        ("pg.execute_calls", 2),
        ("pg.clear_calls", 2),
    ],
};

const CASE_POSTGRES_BATCH_DECODE_FAILURE: Case = Case {
    label: "pkg-db-a1-postgres-batch-decode-failure",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[&PG],
    modules: a1_modules,
    main: POSTGRES_BATCH_DECODE_FAILURE_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[
        ("pg.protocol_ok", 1),
        ("pg.execute_calls", 2),
        ("pg.clear_calls", 3),
        ("pg.nonblocking_calls", 2),
        ("pg.single_row_mode_calls", 1),
    ],
};

const CASE_POSTGRES_STREAMED_MODES: Case = Case {
    label: "pkg-db-a1-postgres-streamed-modes",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[&PG],
    modules: a1_modules,
    main: POSTGRES_STREAMED_MODES_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[
        ("pg.protocol_ok", 1),
        ("pg.execute_calls", 6),
        ("pg.clear_calls", 20),
        ("pg.nonblocking_calls", 8),
        ("pg.cancel_calls", 0),
        ("pg.cancel_socket_wait_calls", 0),
        ("pg.single_row_mode_calls", 2),
        ("pg.chunked_row_mode_calls", 2),
        ("pg.last_chunk_size", 2),
        ("pg.control_calls", 2),
        ("pg.delivered_rows", 24),
    ],
};

const CASE_POSTGRES_DELIVERY_VALIDATION: Case = Case {
    label: "pkg-db-a1-postgres-delivery-validation",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[&PG],
    modules: a1_modules,
    main: POSTGRES_DELIVERY_VALIDATION_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[
        ("pg.protocol_ok", 1),
        ("pg.execute_calls", 0),
        ("pg.single_row_mode_calls", 0),
        ("pg.chunked_row_mode_calls", 0),
    ],
};

const CASE_POSTGRES_STREAMED_ONE: Case = Case {
    label: "pkg-db-a1-postgres-streamed-one",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[&PG],
    modules: a1_modules,
    main: POSTGRES_STREAMED_ONE_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[
        ("pg.protocol_ok", 1),
        ("pg.execute_calls", 7),
        ("pg.clear_calls", 16),
        ("pg.nonblocking_calls", 10),
        ("pg.cancel_calls", 0),
        ("pg.cancel_socket_wait_calls", 0),
        ("pg.single_row_mode_calls", 3),
        ("pg.chunked_row_mode_calls", 2),
        ("pg.control_calls", 2),
    ],
};

const CASE_POSTGRES_STREAMED_FAILURES: Case = Case {
    label: "pkg-db-a1-postgres-streamed-failures",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[&PG],
    modules: a1_modules,
    main: POSTGRES_STREAMED_FAILURES_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[
        ("pg.protocol_ok", 1),
        ("pg.connect_calls", 3),
        ("pg.finish_calls", 2),
        ("pg.execute_calls", 6),
        ("pg.clear_calls", 9),
        ("pg.nonblocking_calls", 9),
        ("pg.cancel_calls", 1),
        ("pg.cancel_socket_wait_calls", 1),
        ("pg.single_row_mode_calls", 3),
        ("pg.chunked_row_mode_calls", 2),
    ],
};

const CASE_POSTGRES_STREAMED_TIMEOUT_DROP: Case = Case {
    label: "pkg-db-a1-postgres-streamed-timeout-drop",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[&PG],
    modules: a1_modules,
    main: POSTGRES_STREAMED_TIMEOUT_DROP_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[
        ("pg.protocol_ok", 1),
        ("pg.connect_calls", 3),
        ("pg.finish_calls", 1),
        ("pg.execute_calls", 5),
        ("pg.clear_calls", 11),
        ("pg.nonblocking_calls", 8),
        ("pg.cancel_calls", 2),
        ("pg.cancel_socket_wait_calls", 2),
        ("pg.single_row_mode_calls", 1),
        ("pg.chunked_row_mode_calls", 1),
        ("pg.last_chunk_size", 2),
    ],
};

const CASE_POSTGRES_STREAMED_UNSAFE_STATUSES: Case = Case {
    label: "pkg-db-a1-postgres-streamed-unsafe-statuses",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[&PG],
    modules: a1_modules,
    main: POSTGRES_STREAMED_UNSAFE_STATUSES_MAIN,
    envs: &[],
    expected_exit: 42,
    expect_counters: &[
        ("pg.protocol_ok", 1),
        ("pg.connect_calls", 1),
        ("pg.finish_calls", 1),
        ("pg.execute_calls", 1),
        ("pg.clear_calls", 2),
        ("pg.nonblocking_calls", 1),
        ("pg.cancel_calls", 0),
        ("pg.cancel_socket_wait_calls", 0),
        ("pg.single_row_mode_calls", 1),
        ("pg.delivered_rows", 0),
    ],
};

/// Every Layer-1 case in this suite, for the fingerprint golden.
const LAYER1_CASES: &[&Case] = &[
    &CASE_ZERO_COLUMN_PLAN,
    &CASE_SQLITE_BATCHES,
    &CASE_POSTGRES_BUFFERED_BATCHES,
    &CASE_POSTGRES_VALUE_MATRIX,
    &CASE_POSTGRES_BATCH_DECODE_FAILURE,
    &CASE_POSTGRES_DELIVERY_VALIDATION,
    &CASE_POSTGRES_STREAMED_FAILURES,
    &CASE_POSTGRES_STREAMED_MODES,
    &CASE_POSTGRES_STREAMED_ONE,
    &CASE_POSTGRES_STREAMED_TIMEOUT_DROP,
    &CASE_POSTGRES_STREAMED_UNSAFE_STATUSES,
];

/// The suite modules every a1 case adds on top of the `pkg.db` package.
fn a1_modules() -> Vec<(&'static str, &'static str)> {
    vec![
        ("pkg/db/a1_test.align", TEST_HELPER),
        ("app/batch_query.align", QUERY),
    ]
}

/// The layout the non-`Case` owners use, derived from the same module list the cases use.
fn package_files(main: &str) -> Layout {
    let mut layout = Layout::new();
    for (path, source) in a1_modules() {
        layout = layout.module(path, source);
    }
    layout.main(main)
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
        &package_files(main).files(),
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
fn direct_delivery_surface_is_staged_without_prepared_parity() {
    assert!(POSTGRES_PUBLIC.contains(
        "pub Delivery {\n  SingleRow\n  PortalBatch(i64)\n}"
    ));
    assert!(POSTGRES_PUBLIC.contains(
        "pub ExecuteOption {\n  ParameterFormat(str, Format)\n  ResultFormat(Format)\n  Delivery(Delivery)\n}"
    ));
    assert!(POSTGRES_PUBLIC.contains("pub fn rows_native<P, R>("));
    assert!(POSTGRES_PUBLIC.contains("pub fn one_native<P, R: RegionPlain>("));
    assert!(!POSTGRES_PUBLIC.contains("pub fn rows_stmt_native"));
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
        &package_files(main).files(),
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
    CASE_ZERO_COLUMN_PLAN.run();
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
            &package_files(&main).files(),
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
    CASE_SQLITE_BATCHES.run();
}

#[test]
fn postgres_buffered_batches_split_without_resend_and_own_copied_rows() {
    CASE_POSTGRES_BUFFERED_BATCHES.run();
}

#[test]
fn postgres_batch_all_value_kinds_and_null_bitmaps_are_exact() {
    CASE_POSTGRES_VALUE_MATRIX.run();
}

#[test]
fn postgres_batch_decode_failure_drops_partial_payload_and_closes_once() {
    CASE_POSTGRES_BATCH_DECODE_FAILURE.run();
}

#[test]
fn postgres_streamed_modes_span_results_and_preserve_buffered_default() {
    CASE_POSTGRES_STREAMED_MODES.run();
}

#[test]
fn postgres_delivery_validation_preserves_source_order_and_command_disposition() {
    CASE_POSTGRES_DELIVERY_VALIDATION.run();
}

#[test]
fn postgres_streamed_one_drains_cardinality_and_late_errors() {
    CASE_POSTGRES_STREAMED_ONE.run();
}

#[test]
fn postgres_streamed_sequences_recover_or_fail_at_the_owned_boundary() {
    CASE_POSTGRES_STREAMED_FAILURES.run();
}

#[test]
fn postgres_streamed_timeout_and_early_drop_cancel_and_restore() {
    CASE_POSTGRES_STREAMED_TIMEOUT_DROP.run();
}

#[test]
fn postgres_streamed_unsafe_statuses_close_without_followup_native_calls() {
    CASE_POSTGRES_STREAMED_UNSAFE_STATUSES.run();
}

#[test]
fn postgres_required_direct_delivery_uses_real_libpq17() {
    if !backend_available() {
        return;
    }
    let files = package_files(LIVE_POSTGRES_MAIN)
        .module("app/live_stream.align", LIVE_POSTGRES_QUERY);
    let diagnostics = check_multi_diagnostics(
        "pkg-db-a1-live-postgres-typecheck",
        &files.files(),
        "main.align",
    );
    assert!(
        !diagnostics.lines().any(|line| line.contains(": error:")),
        "PostgreSQL A1 fixture must type-check before live execution:\n{diagnostics}"
    );
    let Some(url) = db_harness::live_postgres_url("PostgreSQL A1 direct-delivery owner") else {
        return;
    };
    let output = build_and_run_multi_with_static_descriptors_args_with_env(
        "pkg-db-a1-live-postgres",
        &files.files(),
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

/// The Layer-1 migration's forward guard for this suite.
///
/// Regenerate ONLY with a reviewed reason, from the panic message this emits.
const LAYER1_FINGERPRINT_GOLDEN: &str = "\
pkg-db-a1-postgres-batch-decode-failure bedd652eb580b5e0
pkg-db-a1-postgres-buffered-batches ece0f00d9dd9bcfd
pkg-db-a1-postgres-delivery-validation 1673fb23140db463
pkg-db-a1-postgres-streamed-failures 8f5a917b70b16712
pkg-db-a1-postgres-streamed-modes 5b8d2922055d555b
pkg-db-a1-postgres-streamed-one 1d7544638671d32f
pkg-db-a1-postgres-streamed-timeout-drop d469ca7e48d03ed8
pkg-db-a1-postgres-streamed-unsafe-statuses eeb2bfc2233947fa
pkg-db-a1-postgres-value-matrix 1851d5f5b5d1cc0b
pkg-db-a1-sqlite-batches 32dd069e4357546a
pkg-db-a1-zero-column-plan 87bf43dc890fb641
";

#[test]
fn layer1_case_fingerprints_match_the_golden() {
    let mut log = FingerprintLog::new();
    for case in LAYER1_CASES {
        log.record(&case.fingerprint());
    }
    log.assert_matches(LAYER1_FINGERPRINT_GOLDEN);
}
