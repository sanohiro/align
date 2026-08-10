//! pkg.db Q6/D10 compound Query shaping and exact mutable-retention owners.

mod common;
use common::*;
use std::sync::LazyLock;

static DB: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db.align"));
static SQLITE: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db/sqlite.align"));
static POSTGRES: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db/postgres.align"));
static INTERNAL: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/pkg/db/internal.align"));
static RESOURCE: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/internal/resource.align"));
static DESCRIPTOR: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/internal/descriptor.align"));
static INTERNAL_SQLITE: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/internal/sqlite.align"));
static INTERNAL_POSTGRES: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/pkg/db/internal/postgres.align"));
static USER_GROUPS: LazyLock<&str> = LazyLock::new(|| fixture("apps/db/app/user_groups.align"));
static TRANSACTION_MASTER: LazyLock<&str> =
    LazyLock::new(|| fixture("apps/db/app/transaction_master.align"));
static POSTGRES_STUB: &str = include_str!("fixtures/pkg_db_q2_postgres_stub.c");
static SQLITE_Q6_STUB: &str = include_str!("fixtures/pkg_db_q6_sqlite_stub.c");

fn package_files(main: &str) -> Vec<(&'static str, &str)> {
    vec![
        ("pkg/db.align", *DB),
        ("pkg/db/sqlite.align", *SQLITE),
        ("pkg/db/postgres.align", *POSTGRES),
        ("pkg/db/internal.align", *INTERNAL),
        ("pkg/db/internal/resource.align", *RESOURCE),
        ("pkg/db/internal/descriptor.align", *DESCRIPTOR),
        ("pkg/db/internal/sqlite.align", *INTERNAL_SQLITE),
        ("pkg/db/internal/postgres.align", *INTERNAL_POSTGRES),
        ("app/user_groups.align", *USER_GROUPS),
        ("app/transaction_master.align", *TRANSACTION_MASTER),
        ("main.align", main),
    ]
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
    let checked = diff_check_multi("pkg-db-q6-modules", &package_files(main), "main.align");
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
    let whole = whole_mir_multi("pkg-db-q6-compound-mir-whole", &files, "main.align");
    assert_compound_mir_shape(&whole);

    let per_unit = build_per_unit_multi("pkg-db-q6-compound-mir-unit", &files, "main.align");
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
        &package_files(main),
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

#[test]
fn borrow_mut_shaper_retention_is_exact_and_fail_closed() {
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
fn sqlite_user_groups_one_parent_and_segmented_matrices_are_exact() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
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
    Connection(_) => 31
    Timeout(_) => 32
    Cancelled(_) => 33
    NotFound => 34
    Cardinality(_) => 35
    Constraint(_) => 36
    Serialization(_) => 37
    Deadlock(_) => 38
    SchemaMismatch(_) => 39
    DriverMismatch(_) => 40
    Decode(_) => 41
    Encode(_) => 42
    InvalidQuery(_) => 43
    Unsupported(_) => 44
    Native(_) => 45
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
    if match partial_id { Ok(_) => true, Err(_) => false } { return 15 }
    if !counts(1, 1, 1, 1) { return 16 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    partial_name := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 4, last_user_id: 4 }, out,
    )
    if match partial_name { Ok(_) => true, Err(_) => false } { return 17 }
    if !counts(1, 1, 1, 1) { return 18 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    inconsistent := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 5, last_user_id: 5 }, out,
    )
    if match inconsistent { Ok(_) => true, Err(_) => false } { return 19 }
    if !counts(1, 2, 2, 1) { return 20 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    inconsistent_id := app.user_groups.run(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 6, last_user_id: 6 }, out,
    )
    if match inconsistent_id { Ok(_) => true, Err(_) => false } { return 21 }
    if !counts(1, 2, 2, 1) { return 22 }
  }
  return 0
}

fn segmented(borrow connection: pkg.db.conn) -> i32 {
  unsafe { align_sqlite_q6_reset() }
  arena out {
    empty := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 0, last_user_id: 0 }, out,
    ) else { return 21 }
    if empty.users.len() != 0 || empty.groups.len() != 0
      || empty.group_offsets.len() != 1 || empty.group_offsets[0] != 0 { return 22 }
    if !counts(1, 1, 0, 1) { return 23 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    result := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 1, last_user_id: 3 }, out,
    ) else { return 24 }
    if result.users.len() != 3 || result.groups.len() != 3
      || result.group_offsets.len() != 4 { return 25 }
    if result.users[0].id != 1 || result.users[0].name != "Alice"
      || result.users[1].id != 2 || result.users[1].name != "Bob"
      || result.users[2].id != 3 || result.users[2].name != "Cara" { return 26 }
    if result.group_offsets[0] != 0 || result.group_offsets[1] != 2
      || result.group_offsets[2] != 2 || result.group_offsets[3] != 3 { return 27 }
    if result.groups[0].id != 10 || result.groups[1].id != 20
      || result.groups[2].id != 40 { return 28 }
    if !counts(1, 5, 4, 1) { return 29 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    partial_id := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 3, last_user_id: 3 }, out,
    )
    if match partial_id { Ok(_) => true, Err(_) => false } { return 30 }
    if !counts(1, 1, 1, 1) { return 31 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    partial_name := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 4, last_user_id: 4 }, out,
    )
    if match partial_name { Ok(_) => true, Err(_) => false } { return 32 }
    if !counts(1, 1, 1, 1) { return 33 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    disagreement := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 5, last_user_id: 5 }, out,
    )
    if match disagreement { Ok(_) => true, Err(_) => false } { return 34 }
    if !counts(1, 2, 2, 1) { return 35 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    non_adjacent := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 7, last_user_id: 7 }, out,
    )
    if match non_adjacent { Ok(_) => true, Err(_) => false } { return 36 }
    if !counts(1, 3, 3, 1) { return 37 }
  }

  unsafe { align_sqlite_q6_reset() }
  arena out {
    partial_precedence := app.user_groups.run_segmented(
      pkg.db.exec_conn(connection), app.user_groups.Params { first_user_id: 9, last_user_id: 9 }, out,
    )
    if match partial_precedence { Ok(_) => true, Err(_) => false } { return 38 }
    if !counts(1, 1, 1, 1) { return 39 }
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
    let fixture = format!("{POSTGRES_STUB}\n{SQLITE_Q6_STUB}");
    let output = build_and_run_multi_with_c(
        "pkg-db-q6-sqlite-user-groups",
        &package_files(main),
        "main.align",
        &fixture,
    );
    assert_eq!(
        output.status.code(),
        Some(42),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn sqlite_transaction_master_runs_on_connection_and_transaction() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.transaction_master

extern "C" {
  fn align_sqlite_q6_reset()
  fn align_sqlite_q6_prepare_calls() -> i32
  fn align_sqlite_q6_step_calls() -> i32
  fn align_sqlite_q6_delivered_rows() -> i32
  fn align_sqlite_q6_finalize_calls() -> i32
  fn align_sqlite_q6_protocol_ok() -> i32
}

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
  unsafe { align_sqlite_q6_reset() }
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
  return unsafe {
    if align_sqlite_q6_protocol_ok() != 1 { return 10 }
    if align_sqlite_q6_prepare_calls() != 2 { return 11 }
    if align_sqlite_q6_step_calls() != 6 { return 12 }
    if align_sqlite_q6_delivered_rows() != 4 { return 13 }
    if align_sqlite_q6_finalize_calls() != 2 { return 14 }
    42
  }
}
"#;
    let fixture = format!("{POSTGRES_STUB}\n{SQLITE_Q6_STUB}");
    let output = build_and_run_multi_with_c(
        "pkg-db-q6-sqlite-transaction-master",
        &package_files(main),
        "main.align",
        &fixture,
    );
    assert_eq!(
        output.status.code(),
        Some(42),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn postgres_compound_shaping_dispatches_once_on_connection_and_transaction() {
    if !backend_available() || !cc_available() {
        return;
    }
    let main = r#"module main
import pkg.db
import pkg.db.postgres
import app.user_groups
import app.transaction_master

extern "C" {
  fn align_pg_reset()
  fn align_pg_execute_calls() -> i32
  fn align_pg_execute_prepared_calls() -> i32
  fn align_pg_prepare_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_control_calls() -> i32
  fn align_pg_q6_delivered_rows() -> i32
  fn align_pg_protocol_ok() -> i32
  fn align_pg_protocol_error() -> i32
}

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
  unsafe { align_pg_reset() }
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
  return unsafe {
    if align_pg_prepare_calls() != 0 { return 21 }
    if align_pg_execute_prepared_calls() != 0 { return 22 }
    if align_pg_protocol_ok() != 1 { return 20 + align_pg_protocol_error() }
    if align_pg_execute_calls() != 4 { return 16 }
    if align_pg_q6_delivered_rows() != 10 { return 17 }
    if align_pg_control_calls() != 2 { return 18 }
    if align_pg_clear_calls() != 6 { return 19 }
    42
  }
}
"#;
    let output = build_and_run_multi_with_c(
        "pkg-db-q6-postgres-compound",
        &package_files(main),
        "main.align",
        POSTGRES_STUB,
    );
    assert_eq!(
        output.status.code(),
        Some(42),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn compound_failures_do_not_retry_or_construct_rows() {
    if !backend_available() || !cc_available() {
        return;
    }
    let sqlite_main = r#"module main
import pkg.db
import pkg.db.sqlite
import app.user_groups

extern "C" {
  fn align_sqlite_q6_reset()
  fn align_sqlite_q6_fail_next_bind()
  fn align_sqlite_q6_prepare_calls() -> i32
  fn align_sqlite_q6_step_calls() -> i32
  fn align_sqlite_q6_delivered_rows() -> i32
  fn align_sqlite_q6_finalize_calls() -> i32
  fn align_sqlite_q6_protocol_ok() -> i32
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
  return unsafe {
    if align_sqlite_q6_protocol_ok() != 1 { return 3 }
    if align_sqlite_q6_prepare_calls() != 1 { return 4 }
    if align_sqlite_q6_step_calls() != 0 { return 5 }
    if align_sqlite_q6_delivered_rows() != 0 { return 6 }
    if align_sqlite_q6_finalize_calls() != 1 { return 7 }
    42
  }
}
"#;
    let sqlite_fixture = format!("{POSTGRES_STUB}\n{SQLITE_Q6_STUB}");
    let sqlite = build_and_run_multi_with_c(
        "pkg-db-q6-sqlite-pre-send-failure",
        &package_files(sqlite_main),
        "main.align",
        &sqlite_fixture,
    );
    assert_eq!(
        sqlite.status.code(),
        Some(42),
        "SQLite status: {:?}; stdout: {}; stderr: {}",
        sqlite.status,
        String::from_utf8_lossy(&sqlite.stdout),
        String::from_utf8_lossy(&sqlite.stderr),
    );

    let postgres_main = r#"module main
import pkg.db
import pkg.db.postgres
import app.user_groups

extern "C" {
  fn align_pg_reset()
  fn align_pg_q6_fail_next_execute()
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_protocol_ok() -> i32
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
  return unsafe {
    if align_pg_protocol_ok() != 1 { return 3 }
    if align_pg_execute_calls() != 1 { return 4 }
    if align_pg_clear_calls() != 0 { return 5 }
    42
  }
}
"#;
    let postgres = build_and_run_multi_with_c(
        "pkg-db-q6-postgres-send-failure",
        &package_files(postgres_main),
        "main.align",
        POSTGRES_STUB,
    );
    assert_eq!(
        postgres.status.code(),
        Some(42),
        "PostgreSQL status: {:?}; stdout: {}; stderr: {}",
        postgres.status,
        String::from_utf8_lossy(&postgres.stdout),
        String::from_utf8_lossy(&postgres.stderr),
    );
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
    let fixture = format!("{POSTGRES_STUB}\n{SQLITE_Q6_STUB}");
    let output = build_and_run_multi_with_c(
        "pkg-db-q6-high-fanout",
        &package_files(main),
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
