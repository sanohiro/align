//! pkg.db D14/A2 SQLite scalar-callback owners.

mod common;
mod db_harness;

use db_harness::{Case, Layout, Needs, PG, RunnerKind, SQLITE_Q4A, expect_checks_clean, expect_checks_rejected, package_source};

const CALLBACK_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

fn echo(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> {
  if args.values.len() == 0 { return Ok(pkg.db.value.Null) }
  return Ok(args.values[0])
}

fn main() -> i32 {
  callback := pkg.db.sqlite.function(echo)
  return 42
}
"#;

const PARALLEL_CALLBACK_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

fn score(value: pkg.db.value) -> i64 = match value { Null => 0, _ => 1 }

fn direct(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> {
  values := args.values
  count := values.par_map(score).sum()
  return Ok(pkg.db.value.I64(count))
}

fn helper(values: slice<pkg.db.value>) -> i64 = values.par_map(score).sum()

fn through_helper(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Ok(pkg.db.value.I64(helper(args.values)))

fn through_concrete_function_value(text: str) -> i64 {
  f := helper_text
  return f(text)
}

fn helper_text(text: str) -> i64 = task_group {
  task := spawn(fn { text.len() })
  wait()
  task.get()
}

fn through_function_value(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> {
  if args.values.len() == 0 { return Ok(pkg.db.value.Null) }
  return match args.values[0] {
    Text(text) => Ok(pkg.db.value.I64(through_concrete_function_value(text)))
    _ => Ok(pkg.db.value.Null)
  }
}

fn main() -> i32 {
  first := pkg.db.sqlite.function(direct)
  second := pkg.db.sqlite.function(through_helper)
  third := pkg.db.sqlite.function(through_function_value)
  return 42
}
"#;

const SAFE_PARALLEL_CALLBACK_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

fn twice(value: i64) -> i64 = value * 2

fn safe(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> {
  count := [1, 2, 3].par_map(twice).sum()
  if args.values.len() == 0 { return Ok(pkg.db.value.I64(count)) }
  return Ok(args.values[0])
}

fn main() -> i32 {
  callback := pkg.db.sqlite.function(safe)
  return 42
}
"#;

const IMPORTED_PARALLEL_CALLBACK_MODULE: &str = r#"module callback_parallel_dep
import pkg.db
import pkg.db.sqlite

fn score(value: pkg.db.value) -> i64 = match value { Null => 0, _ => 1 }

pub fn callback(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> {
  values := args.values
  count := values.par_map(score).sum()
  return Ok(pkg.db.value.I64(count))
}
"#;

const IMPORTED_PARALLEL_CALLBACK_MAIN: &str = r#"module main
import callback_parallel_dep
import pkg.db.sqlite

fn main() -> i32 {
  callback := pkg.db.sqlite.function(callback_parallel_dep.callback)
  return 42
}
"#;

const WRONG_SIGNATURE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

fn wrong(value: i64) -> i64 = value

fn main() -> i32 {
  callback := pkg.db.sqlite.function(wrong)
  return 42
}
"#;

const DYNAMIC_TARGET_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

fn first(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> = Ok(pkg.db.value.Null)
fn second(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> = Ok(pkg.db.value.Null)

fn main() -> i32 {
  target := if true { first } else { second }
  callback := pkg.db.sqlite.function(target)
  return 42
}
"#;

const LAMBDA_TARGET_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

fn main() -> i32 {
  callback := pkg.db.sqlite.function(fn args {
    return Ok(pkg.db.value.Null)
  })
  return 42
}
"#;

const CAPTURED_TARGET_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

fn main() -> i32 {
  captured := 1
  callback := pkg.db.sqlite.function(fn args {
    if captured == args.values.len() { return Ok(pkg.db.value.I64(captured)) }
    return Ok(pkg.db.value.Null)
  })
  return 42
}
"#;

const EXTERN_TARGET_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

extern "C" {
  fn native_callback(value: raw) -> i32
}

fn main() -> i32 {
  callback := pkg.db.sqlite.function(native_callback)
  return 42
}
"#;

const CONSTRUCTED_DESCRIPTOR_MAIN: &str = r#"module main
import pkg.db.sqlite

fn main() -> i32 {
  callback := pkg.db.sqlite.scalar_function {}
  return 42
}
"#;

const CONSTRUCTED_ARGS_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite

fn main() -> i32 {
  values := [pkg.db.value.Null]
  args := pkg.db.sqlite.function_args { values: values }
  return args.values.len() as i32
}
"#;

const INTERNAL_DESCRIPTOR_ACCESS_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import pkg.db.internal.descriptor

fn echo(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Ok(pkg.db.value.Null)

fn main() -> i32 {
  callback := pkg.db.sqlite.function(echo)
  unsafe {
    pointer := pkg.db.internal.descriptor.sqlite_callback_data(callback)
    if pointer.is_null() { return 1 }
  }
  return 42
}
"#;

const IMPORTED_CALLBACK_MODULE: &str = r#"module callback_dep
import pkg.db
import pkg.db.sqlite

pub fn echo(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> {
  if args.values.len() == 0 { return Ok(pkg.db.value.Null) }
  return Ok(args.values[0])
}
"#;

const IMPORTED_CALLBACK_MAIN: &str = r#"module main
import callback_dep
import pkg.db.sqlite

fn main() -> i32 {
  callback := pkg.db.sqlite.function(callback_dep.echo)
  return 42
}
"#;

const CALLBACK_TESTKIT_MODULE: &str = r#"module pkg.db.testkit.callback
import pkg.db.sqlite
import pkg.db.internal.descriptor

extern "C" {
  fn test_sqlite_callback_invoke(callback: raw, scenario: i32) -> i32
}

pub fn invoke(callback: pkg.db.sqlite.scalar_function, scenario: i32) -> i32 {
  unsafe {
    descriptor := pkg.db.internal.descriptor.sqlite_callback_data(callback)
    trampoline: raw := raw.load(descriptor, 8)
    return test_sqlite_callback_invoke(trampoline, scenario)
  }
}
"#;

const CALLBACK_INJECTED_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import pkg.db.testkit.callback

fn inspect(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> {
  if args.values.len() == 2 {
    first := match args.values[0] { Text(value) => value.len(), _ => -1 }
    second := match args.values[1] { Text(value) => value.len(), _ => -1 }
    return Ok(pkg.db.value.I64(first + second))
  }
  if args.values.len() != 1 { return Err("unexpected arity") }
  return match args.values[0] {
    Text(value) => Ok(pkg.db.value.I64(value.len()))
    Bytes(value) => Ok(pkg.db.value.I64(value.bytes.len()))
    _ => Ok(pkg.db.value.I64(-1))
  }
}

fn main() -> i32 {
  callback := pkg.db.sqlite.function(inspect)
  mut scenario := 0
  loop {
    if scenario >= 16 { break }
    result := pkg.db.testkit.callback.invoke(callback, scenario)
    if result != 42 { return result }
    scenario = scenario + 1
  }
  return 42
}
"#;

const CALLBACK_REGISTRATION_TESTKIT_MODULE: &str = r#"module pkg.db.testkit.callback_registration

extern "C" {
  fn test_sqlite_callback_registration_reset(mode: i32, version: i32)
  fn test_sqlite_callback_registration_calls() -> i32
  fn test_sqlite_callback_registration_protocol_ok() -> i32
}

pub fn reset(mode: i32, version: i32) {
  unsafe { test_sqlite_callback_registration_reset(mode, version) }
}

pub fn calls() -> i32 {
  unsafe { return test_sqlite_callback_registration_calls() }
}

pub fn protocol_ok() -> bool {
  unsafe { return test_sqlite_callback_registration_protocol_ok() == 1 }
}
"#;

const CALLBACK_REGISTRATION_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import pkg.db.testkit.callback_registration

fn value(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Ok(pkg.db.value.Null)

fn is_connection_error(result: Result<(), pkg.db.Error>) -> bool = match result {
  Err(error) => match error { Connection(_) => true _ => false }
  Ok(_) => false
}

fn is_snapshot_error(result: Result<(), pkg.db.Error>) -> bool = match result {
  Err(error) => match error {
    Connection(native) => {
      primary := match native.code { Some(code) => code == "1", None => false }
      extended := match native.extended_code { Some(code) => code == 257, None => false }
      primary && extended && native.message == "x"
    }
    _ => false
  }
  Ok(_) => false
}

fn is_callback_connection_error(result: Result<(), pkg.db.Error>) -> bool = match result {
  Err(error) => match error {
    Unsupported(contract) => contract.item == "sqlite.callback.connection"
    _ => false
  }
  Ok(_) => false
}

fn main() -> i32 {
  pkg.db.testkit.callback_registration.reset(0, 3029999)
  mut old_version := pkg.db.sqlite.connect(":memory:", []) else { return 1 }
  old_result := pkg.db.sqlite.register_function(
    old_version, "align_old", 0, pkg.db.sqlite.function(value), [],
  )
  old_rejected := match old_result {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "sqlite.callback.version"
      _ => false
    }
    Ok(_) => false
  }
  if !old_rejected || pkg.db.testkit.callback_registration.calls() != 0 { return 2 }

  pkg.db.testkit.callback_registration.reset(1, 3030000)
  mut native_failure := pkg.db.sqlite.connect(":memory:", []) else { return 3 }
  failed := pkg.db.sqlite.register_function(
    native_failure, "align_fail", 0, pkg.db.sqlite.function(value), [],
  )
  if !is_snapshot_error(failed) || pkg.db.testkit.callback_registration.calls() != 1 { return 4 }
  after_failure := pkg.db.sqlite.register_function(
    native_failure, "align_again", 0, pkg.db.sqlite.function(value), [],
  )
  if !is_callback_connection_error(after_failure)
    || pkg.db.testkit.callback_registration.calls() != 1 { return 5 }

  pkg.db.testkit.callback_registration.reset(2, 3030000)
  mut post_failure := pkg.db.sqlite.connect(":memory:", []) else { return 6 }
  contradicted := pkg.db.sqlite.register_function(
    post_failure, "align_post", 0, pkg.db.sqlite.function(value), [],
  )
  post_rejected := match contradicted {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "sqlite.callback.cleanup"
      _ => false
    }
    Ok(_) => false
  }
  if !post_rejected || pkg.db.testkit.callback_registration.calls() != 1 { return 7 }
  after_post := pkg.db.sqlite.register_function(
    post_failure, "align_again", 0, pkg.db.sqlite.function(value), [],
  )
  if !is_callback_connection_error(after_post)
    || pkg.db.testkit.callback_registration.calls() != 1 { return 8 }
  if !pkg.db.testkit.callback_registration.protocol_ok() { return 9 }

  pkg.db.testkit.callback_registration.reset(1, 3030000)
  mut removal_failure := pkg.db.sqlite.connect(":memory:", []) else { return 10 }
  removed := pkg.db.sqlite.remove_function(removal_failure, "align_remove", 0)
  if !is_snapshot_error(removed)
    || pkg.db.testkit.callback_registration.calls() != 1 { return 11 }
  after_removal := pkg.db.sqlite.remove_function(removal_failure, "align_remove", 0)
  if !is_callback_connection_error(after_removal)
    || pkg.db.testkit.callback_registration.calls() != 1 { return 12 }
  return 42
}
"#;

const CALLBACK_RUNTIME_MAIN: &str = r#"module main
import pkg.db
import pkg.db.pool
import pkg.db.sqlite

fn echo(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> {
  if args.values.len() != 1 { return Err("wrong arity") }
  return Ok(args.values[0])
}

fn bool_value(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Ok(pkg.db.value.Bool(args.values.len() == 0))

fn i16_value(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Ok(pkg.db.value.I16(-2))

fn i32_value(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Ok(pkg.db.value.I32(16909060))

fn f32_value(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Ok(pkg.db.value.F32(1.5))

fn positive_infinity(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Ok(pkg.db.value.F64(1.0 / 0.0))

fn negative_infinity(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Ok(pkg.db.value.F64(-1.0 / 0.0))

fn distinct_connection(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> {
  if args.values.len() != 0 { return Err("wrong arity") }
  nested := pkg.db.sqlite.connect(":memory:", []) else { return Err("nested connect failed") }
  mut rows := pkg.db.dynamic_rows(
    pkg.db.exec_conn(nested), pkg.db.Driver.SQLite, "SELECT 42", [], [],
  ) else { return Err("nested query failed") }
  mut answer: i64 := -1
  arena out {
    selected := pkg.db.dynamic_next(rows, out) else { return Err("nested next failed") }
    row := selected else { return Err("nested row absent") }
    if row.values.len() != 1 { return Err("nested row shape") }
    match row.values[0] {
      I64(value) => { answer = value }
      _ => { return Err("nested value shape") }
    }
  }
  return Ok(pkg.db.value.I64(answer))
}

fn impure_value(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> {
  print(args.values.len())
  return Ok(pkg.db.value.Null)
}

fn callback_error(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Err("callback failed")

fn invalid_value(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Ok(pkg.db.value.Text("a\0b"))

fn invalid_error(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> = Err("")

fn invalid_nul_error(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> = Err("a\0b")

fn nan_value(args: pkg.db.sqlite.function_args) -> Result<pkg.db.value, str> =
  Ok(pkg.db.value.F64(0.0 / 0.0))

fn verify_values(borrow connection: pkg.db.conn) -> i32 {
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT align_echo(NULL), align_echo(7), align_echo(-0.0), align_echo('é'), align_echo(x'00ff'), align_echo(''), align_echo(x''), align_bool(), align_i16(), align_i32(), align_f32(), align_pos_inf(), align_neg_inf(), align_nested(), align_slice()",
    [], [],
  ) else { return 7 }
  arena out {
    present := pkg.db.dynamic_next(stream, out) else { return 8 }
    row := present else { return 9 }
    if row.values.len() != 15 { return 10 }
    match row.values[0] { Null => {} _ => { return 11 } }
    match row.values[1] { I64(value) => if value != 7 { return 12 }, _ => { return 13 } }
    match row.values[2] { F64(value) => if value != 0.0 { return 14 }, _ => { return 15 } }
    match row.values[3] { Text(value) => if value != "é" { return 16 }, _ => { return 17 } }
    match row.values[4] {
      Bytes(value) => if value.bytes.len() != 2 || value.bytes[0] != 0 || value.bytes[1] != 255 { return 18 }
      _ => { return 19 }
    }
    match row.values[5] { Text(value) => if value.len() != 0 { return 20 }, _ => { return 21 } }
    match row.values[6] { Bytes(value) => if value.bytes.len() != 0 { return 22 }, _ => { return 23 } }
    match row.values[7] { I64(value) => if value != 1 { return 24 }, _ => { return 25 } }
    match row.values[8] { I64(value) => if value != -2 { return 26 }, _ => { return 27 } }
    match row.values[9] { I64(value) => if value != 16909060 { return 28 }, _ => { return 29 } }
    match row.values[10] { F64(value) => if value != 1.5 { return 30 }, _ => { return 31 } }
    match row.values[11] { F64(value) => if value <= 1.0e300 { return 59 }, _ => { return 60 } }
    match row.values[12] { F64(value) => if value >= -1.0e300 { return 61 }, _ => { return 62 } }
    match row.values[13] { I64(value) => if value != 42 { return 65 }, _ => { return 66 } }
    match row.values[14] { I64(value) => if value != 1 { return 67 }, _ => { return 68 } }
  }
  return 0
}

fn query_fails_with(borrow connection: pkg.db.conn, sql: str, expected: str) -> bool {
  stream_result := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite, sql, [], [],
  )
  mut stream := match stream_result {
    Ok(value) => value
    Err(_) => { return false }
  }
  arena out {
    result := pkg.db.dynamic_next(stream, out)
    return match result {
      Err(error) => match error {
        Connection(native) => native.message == expected
        _ => false
      }
      Ok(_) => false
    }
  }
}

fn main() -> i32 {
  mut connection := pkg.db.sqlite.connect(":memory:", []) else { return 1 }
  bad_name := pkg.db.sqlite.register_function(
    connection, "", 1, pkg.db.sqlite.function(echo), [],
  )
  bad_name_ok := match bad_name {
    Err(error) => match error { Unsupported(contract) => contract.item == "sqlite.callback.name", _ => false }
    Ok(_) => false
  }
  if !bad_name_ok { return 41 }
  nul_name := pkg.db.sqlite.register_function(
    connection, "bad\0name", 1, pkg.db.sqlite.function(echo), [],
  )
  nul_name_ok := match nul_name {
    Err(error) => match error { Encode(contract) => contract.item == "sqlite.callback.name", _ => false }
    Ok(_) => false
  }
  if !nul_name_ok { return 45 }
  long_name := pkg.db.sqlite.register_function(
    connection,
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    0, pkg.db.sqlite.function(bool_value), [],
  )
  long_name_ok := match long_name {
    Err(error) => match error { Unsupported(contract) => contract.item == "sqlite.callback.name", _ => false }
    Ok(_) => false
  }
  if !long_name_ok { return 46 }
  negative_arity := pkg.db.sqlite.register_function(
    connection, "align_echo", -1, pkg.db.sqlite.function(echo), [],
  )
  negative_arity_ok := match negative_arity {
    Err(error) => match error { Unsupported(contract) => contract.item == "sqlite.function.arity", _ => false }
    Ok(_) => false
  }
  if !negative_arity_ok { return 47 }
  bad_arity := pkg.db.sqlite.register_function(
    connection, "align_echo", 128, pkg.db.sqlite.function(echo), [],
  )
  bad_arity_ok := match bad_arity {
    Err(error) => match error { Unsupported(contract) => contract.item == "sqlite.function.arity", _ => false }
    Ok(_) => false
  }
  if !bad_arity_ok { return 43 }
  duplicate_option := pkg.db.sqlite.register_function(
    connection, "align_echo", 1, pkg.db.sqlite.function(echo),
    [pkg.db.sqlite.FunctionOption.Deterministic, pkg.db.sqlite.FunctionOption.Deterministic],
  )
  duplicate_ok := match duplicate_option {
    Err(error) => match error { Unsupported(contract) => contract.item == "sqlite.function.option", _ => false }
    Ok(_) => false
  }
  if !duplicate_ok { return 44 }
  pkg.db.sqlite.register_function(
    connection, "align_arity_127", 127, pkg.db.sqlite.function(echo), [],
  ) else { return 48 }
  pkg.db.sqlite.remove_function(connection, "align_arity_127", 127) else { return 49 }
  pkg.db.sqlite.register_function(
    connection,
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    0, pkg.db.sqlite.function(bool_value), [],
  ) else { return 53 }
  pkg.db.sqlite.remove_function(
    connection,
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    0,
  ) else { return 54 }
  pkg.db.sqlite.register_function(
    connection, "align_echo", 1, pkg.db.sqlite.function(echo),
    [pkg.db.sqlite.FunctionOption.Deterministic],
  ) else { return 2 }
  pkg.db.sqlite.register_function(connection, "align_bool", 0, pkg.db.sqlite.function(bool_value), []) else { return 3 }
  pkg.db.sqlite.register_function(connection, "align_i16", 0, pkg.db.sqlite.function(i16_value), []) else { return 4 }
  pkg.db.sqlite.register_function(connection, "align_i32", 0, pkg.db.sqlite.function(i32_value), []) else { return 5 }
  pkg.db.sqlite.register_function(connection, "align_f32", 0, pkg.db.sqlite.function(f32_value), []) else { return 6 }
  pkg.db.sqlite.register_function(connection, "align_pos_inf", 0, pkg.db.sqlite.function(positive_infinity), []) else { return 63 }
  pkg.db.sqlite.register_function(connection, "align_neg_inf", 0, pkg.db.sqlite.function(negative_infinity), []) else { return 64 }
  pkg.db.sqlite.register_function(connection, "align_nested", 0, pkg.db.sqlite.function(distinct_connection), []) else { return 69 }
  padded_name := "xalign_slicey"
  sliced_name := padded_name[1..12]
  pkg.db.sqlite.register_function(connection, sliced_name, 0, pkg.db.sqlite.function(bool_value), []) else { return 70 }
  deterministic_impure := pkg.db.sqlite.register_function(
    connection, "align_impure", 0, pkg.db.sqlite.function(impure_value),
    [pkg.db.sqlite.FunctionOption.Deterministic],
  )
  deterministic_rejected := match deterministic_impure {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "sqlite.function.deterministic"
      _ => false
    }
    Ok(_) => false
  }
  if !deterministic_rejected { return 32 }
  pkg.db.sqlite.register_function(connection, "align_impure", 0, pkg.db.sqlite.function(impure_value), []) else { return 33 }
  pkg.db.sqlite.remove_function(connection, "align_impure", 0) else { return 34 }
  pkg.db.sqlite.register_function(connection, "align_callback_error", 0, pkg.db.sqlite.function(callback_error), []) else { return 35 }
  pkg.db.sqlite.register_function(connection, "align_invalid_value", 0, pkg.db.sqlite.function(invalid_value), []) else { return 36 }
  pkg.db.sqlite.register_function(connection, "align_invalid_error", 0, pkg.db.sqlite.function(invalid_error), []) else { return 37 }
  pkg.db.sqlite.register_function(connection, "align_invalid_nul_error", 0, pkg.db.sqlite.function(invalid_nul_error), []) else { return 55 }
  pkg.db.sqlite.register_function(connection, "align_nan", 0, pkg.db.sqlite.function(nan_value), []) else { return 56 }

  verified := verify_values(connection)
  if verified != 0 { return verified }
  pkg.db.sqlite.remove_function(connection, sliced_name, 0) else { return 71 }
  if !query_fails_with(connection, "SELECT align_callback_error()", "callback failed") { return 38 }
  if !query_fails_with(
    connection,
    "SELECT align_invalid_value()",
    "pkg.db SQLite function callback returned an invalid value",
  ) { return 39 }
  if !query_fails_with(
    connection,
    "SELECT align_invalid_error()",
    "pkg.db SQLite function callback returned an invalid error message",
  ) { return 40 }
  if !query_fails_with(
    connection,
    "SELECT align_invalid_nul_error()",
    "pkg.db SQLite function callback returned an invalid error message",
  ) { return 57 }
  if !query_fails_with(
    connection,
    "SELECT align_nan()",
    "pkg.db SQLite function callback returned an invalid value",
  ) { return 58 }

  pool := pkg.db.pool.open_sqlite(":memory:", 1, []) else { return 50 }
  mut pooled := pkg.db.pool.try_acquire(pool) else { return 51 }
  pooled_registration := pkg.db.sqlite.register_function(
    pooled, "align_pool", 0, pkg.db.sqlite.function(bool_value), [],
  )
  pooled_rejected := match pooled_registration {
    Err(error) => match error {
      Unsupported(contract) => contract.item == "sqlite.callback.connection"
      _ => false
    }
    Ok(_) => false
  }
  if !pooled_rejected { return 52 }

  pkg.db.sqlite.remove_function(connection, "align_echo", 1) else { return 28 }
  removed := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT align_echo(1)", [], [],
  )
  return match removed { Err(_) => 42, Ok(_) => 29 }
}
"#;

const FORMATION_STUBS: &[&db_harness::Stub] = &[&SQLITE_Q4A];
const REGISTRATION_STUBS: &[&db_harness::Stub] = &[&PG, &SQLITE_Q4A];
const NO_COUNTERS: &[(&str, i64)] = &[];
const NO_ENVS: &[(&str, &str)] = &[];

fn no_modules() -> Vec<(&'static str, &'static str)> {
    Vec::new()
}

const CALLBACK_FORMATION: Case = Case {
    label: "pkg-db-callback-formation",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: FORMATION_STUBS,
    counters: &[],
    modules: no_modules,
    main: CALLBACK_MAIN,
    envs: NO_ENVS,
    expected_exit: 42,
    expect_counters: NO_COUNTERS,
};

const CALLBACK_RUNTIME: Case = Case {
    label: "pkg-db-callback-runtime",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: &[&PG],
    counters: &[],
    modules: no_modules,
    main: CALLBACK_RUNTIME_MAIN,
    envs: NO_ENVS,
    expected_exit: 42,
    expect_counters: NO_COUNTERS,
};

const CALLBACK_INJECTED: Case = Case {
    label: "pkg-db-callback-injected-native-values",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: FORMATION_STUBS,
    counters: &[],
    modules: callback_testkit_modules,
    main: CALLBACK_INJECTED_MAIN,
    envs: NO_ENVS,
    expected_exit: 42,
    expect_counters: NO_COUNTERS,
};

const CALLBACK_REGISTRATION: Case = Case {
    label: "pkg-db-callback-registration-failures",
    runner: RunnerKind::PerUnitC,
    needs: Needs::BackendAndCc,
    links: REGISTRATION_STUBS,
    counters: &[],
    modules: callback_registration_modules,
    main: CALLBACK_REGISTRATION_MAIN,
    envs: NO_ENVS,
    expected_exit: 42,
    expect_counters: NO_COUNTERS,
};

fn callback_testkit_modules() -> Vec<(&'static str, &'static str)> {
    vec![("pkg/db/testkit/callback.align", CALLBACK_TESTKIT_MODULE)]
}

fn callback_registration_modules() -> Vec<(&'static str, &'static str)> {
    vec![(
        "pkg/db/testkit/callback_registration.align",
        CALLBACK_REGISTRATION_TESTKIT_MODULE,
    )]
}

#[test]
fn sqlite_callback_public_producer_accepts_only_one_exact_static_target() {
    let source = package_source("pkg/db/sqlite.align");
    for required in [
        "pub function_args {\n  values: slice<pkg.db.value>,\n}",
        "pub scalar_function {}",
        "pub FunctionOption {\n  Deterministic\n}",
        "pub fn function(",
    ] {
        assert!(source.contains(required), "missing callback surface `{required}`");
    }
    for forbidden in [
        "pub fn register_collation",
        "pub fn aggregate_function",
        "pub fn window_function",
        "pub fn raw_function",
    ] {
        assert!(!source.contains(forbidden), "forbidden callback surface `{forbidden}`");
    }
    expect_checks_clean(
        "pkg-db-callback-static-target",
        &Layout::new().main(CALLBACK_MAIN),
    );
    expect_checks_clean(
        "pkg-db-callback-lambda-target",
        &Layout::new().main(LAMBDA_TARGET_MAIN),
    );
    expect_checks_clean(
        "pkg-db-callback-imported-target",
        &Layout::new()
            .module("callback_dep.align", IMPORTED_CALLBACK_MODULE)
            .main(IMPORTED_CALLBACK_MAIN),
    );
    expect_checks_rejected(
        "pkg-db-callback-wrong-signature",
        &Layout::new().main(WRONG_SIGNATURE_MAIN),
    );
    expect_checks_rejected(
        "pkg-db-callback-dynamic-target",
        &Layout::new().main(DYNAMIC_TARGET_MAIN),
    );
    expect_checks_rejected(
        "pkg-db-callback-captured-target",
        &Layout::new().main(CAPTURED_TARGET_MAIN),
    );
    expect_checks_rejected(
        "pkg-db-callback-extern-target",
        &Layout::new().main(EXTERN_TARGET_MAIN),
    );
    expect_checks_rejected(
        "pkg-db-callback-constructed-descriptor",
        &Layout::new().main(CONSTRUCTED_DESCRIPTOR_MAIN),
    );
    expect_checks_rejected(
        "pkg-db-callback-constructed-args",
        &Layout::new().main(CONSTRUCTED_ARGS_MAIN),
    );
    expect_checks_rejected(
        "pkg-db-callback-private-access",
        &Layout::new().main(INTERNAL_DESCRIPTOR_ACCESS_MAIN),
    );
}

#[test]
fn sqlite_callback_invocation_views_never_reach_parallel_workers() {
    const DIAGNOSTIC: &str =
        "SQLite callback invocation views cannot be transferred to parallel workers";
    let direct = Layout::new().main(PARALLEL_CALLBACK_MAIN);
    let direct_checked = common::diff_check_multi(
        "pkg-db-callback-parallel-transfer",
        &direct.files(),
        "main.align",
    );
    assert!(direct_checked.whole_errors && direct_checked.per_unit_errors);
    for diagnostics in [&direct_checked.whole_diags, &direct_checked.per_unit_diags] {
        assert_eq!(
            diagnostics.matches(DIAGNOSTIC).count(),
            3,
            "direct/helper/function-value descriptors must each receive the exact diagnostic:\n{diagnostics}",
        );
    }

    let imported = Layout::new()
        .module(
            "callback_parallel_dep.align",
            IMPORTED_PARALLEL_CALLBACK_MODULE,
        )
        .main(IMPORTED_PARALLEL_CALLBACK_MAIN);
    let imported_checked = common::diff_check_multi(
        "pkg-db-callback-imported-parallel-transfer",
        &imported.files(),
        "main.align",
    );
    assert!(imported_checked.whole_errors && imported_checked.per_unit_errors);
    for diagnostics in [
        &imported_checked.whole_diags,
        &imported_checked.per_unit_diags,
    ] {
        assert_eq!(diagnostics.matches(DIAGNOSTIC).count(), 1, "{diagnostics}");
    }
    expect_checks_clean(
        "pkg-db-callback-static-parallel",
        &Layout::new().main(SAFE_PARALLEL_CALLBACK_MAIN),
    );
}

#[test]
fn sqlite_callback_descriptor_and_trampoline_link_in_per_unit_build() {
    CALLBACK_FORMATION.run();
}

#[test]
fn sqlite_callback_descriptor_and_trampoline_have_the_exact_generated_llvm_shape() {
    if db_harness::gate(Needs::Backend).is_none() {
        return;
    }
    let layout = CALLBACK_FORMATION.layout();
    let files = layout.files();
    let whole_llvm = common::emit_llvm_multi(
        "pkg-db-callback-whole-llvm-shape",
        &files,
        "main.align",
    );
    let built = common::build_per_unit_multi(
        "pkg-db-callback-llvm-shape",
        &files,
        "main.align",
    );
    let per_unit_llvm = common::emit_llvm_ir(
        &built.unit("main").mir,
        common::BuildTarget::Baseline,
        false,
        &[],
        false,
    )
    .expect("emit callback LLVM");
    let sqlite_unit = built
        .walk
        .units
        .iter()
        .find(|unit| {
            unit.mir
                .externs
                .iter()
                .any(|external| external.name.as_str() == "align_pkg_db_sqlite_register_v2")
        })
        .expect("per-unit build contains the SQLite package unit");
    let per_unit_sqlite_llvm = common::emit_llvm_ir(
        &sqlite_unit.mir,
        common::BuildTarget::Baseline,
        false,
        &[],
        false,
    )
    .expect("emit SQLite package LLVM");
    let mut stale_effect = built.unit("main").mir.clone();
    let callback_target = stale_effect
        .fns
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.stmts)
        .find_map(|statement| match statement {
            align_mir::Stmt::Let(
                _,
                align_mir::Rvalue::SqliteCallbackDescriptor(descriptor),
            ) => Some(descriptor.target.clone()),
            _ => None,
        })
        .expect("fixture contains a SQLite callback descriptor");
    *stale_effect
        .sqlite_callback_effects
        .get_mut(&callback_target)
        .expect("fixture retains the target-owned callback effect") = align_sema::FnEffect::Impure;
    let stale_error = common::emit_llvm_ir(
        &stale_effect,
        common::BuildTarget::Baseline,
        false,
        &[],
        false,
    )
    .expect_err("a descriptor effect that disagrees with its target must fail closed");
    assert!(
        stale_error.to_string().contains("callable target invalid"),
        "unexpected stale callback effect error: {stale_error}",
    );
    for (pipeline, llvm) in [("whole", &whole_llvm), ("per-unit", &per_unit_llvm)] {
        assert!(
            llvm.contains(
                "@sqlite_callback_descriptor = private unnamed_addr constant { i32, i8, i8, i16, ptr, ptr, i64 }",
            ),
            "{pipeline}: missing exact 32-byte descriptor record:\n{llvm}",
        );
        assert!(
            llvm.lines().any(|line| {
                line.contains("@sqlite_callback_descriptor") && line.ends_with(", align 8")
            }),
            "{pipeline}: descriptor record must be explicitly 8-aligned:\n{llvm}",
        );
        assert!(
            llvm.contains("@sqlite_callback_identity = private unnamed_addr constant"),
            "{pipeline}: missing immutable NUL-terminated callback identity:\n{llvm}",
        );
        let definition = llvm
            .lines()
            .find(|line| line.starts_with("define private void @\"align_gen$sqlite_scalar$"))
            .unwrap_or_else(|| panic!("{pipeline}: missing private SQLite callback definition:\n{llvm}"));
        assert!(definition.contains("(ptr ") && definition.contains(", i32 ") && definition.contains(", ptr "));
        let attribute = definition
            .split_whitespace()
            .find(|part| part.starts_with('#'))
            .expect("generated callback definition has an attribute group");
        assert!(
            llvm.lines().any(|line| {
                line.starts_with(&format!("attributes {attribute} =")) && line.contains("nounwind")
            }),
            "{pipeline}: generated callback must be nounwind:\n{llvm}",
        );
        assert_eq!(
            llvm.matches("call ptr @sqlite3_context_db_handle").count(),
            1,
            "{pipeline}: callback must save its database handle exactly once",
        );
    }
    for (pipeline, llvm) in [
        ("whole", &whole_llvm),
        ("per-unit SQLite", &per_unit_sqlite_llvm),
    ] {
        assert!(
            llvm.contains("define private ptr @align_pkg_db_sqlite_register_v2"),
            "{pipeline}: missing the exact v2 registration helper definition:\n{llvm}",
        );
        for call in [
            "call i32 @sqlite3_create_function_v2",
            "call i32 @sqlite3_errcode",
            "call i32 @sqlite3_extended_errcode",
            "call ptr @sqlite3_errmsg",
            "call ptr @align_rt_alloc",
            "call void @llvm.trap",
        ] {
            assert!(
                llvm.contains(call),
                "{pipeline}: registration helper is missing `{call}`:\n{llvm}",
            );
        }
        assert!(
            llvm.lines().any(|line| line.contains("and i32") && line.trim_end().ends_with(", 255")),
            "{pipeline}: primary SQLite result code must be masked to eight bits:\n{llvm}",
        );
    }
    for artifact in ["@sqlite_callback_descriptor =", "@sqlite_callback_identity ="] {
        let whole = whole_llvm
            .lines()
            .find(|line| line.contains(artifact))
            .unwrap_or_else(|| panic!("whole: missing generated artifact `{artifact}`"));
        let per_unit = per_unit_llvm
            .lines()
            .find(|line| line.contains(artifact))
            .unwrap_or_else(|| panic!("per-unit: missing generated artifact `{artifact}`"));
        assert_eq!(
            whole, per_unit,
            "whole and per-unit callback artifacts must be byte-identical",
        );
    }
}

#[test]
fn sqlite_callback_native_input_failpoints_preserve_order_and_exact_results() {
    CALLBACK_INJECTED.run();
}

#[test]
fn sqlite_callback_registration_failures_poison_without_a_second_native_call() {
    CALLBACK_REGISTRATION.run();
}

#[test]
fn sqlite_callback_round_trips_native_values_and_removes_the_exact_identity() {
    CALLBACK_RUNTIME.run();
}
