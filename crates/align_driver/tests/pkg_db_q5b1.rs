//! pkg.db Q5b1/D12 static Query metadata materializer owners.

mod common;
use align_driver::{generated_query_meta_rows, GeneratedMetaDetail, GeneratedQueryMetaEntry};
use align_interface::{
    CheckPolicy, CheckedColumnMeta, CheckedParameterMeta, CheckedQueryEvidence, Driver, Hash128,
    MetaNullability, StaticArtifact, StaticOptionValue, VerificationState,
};
use common::*;

const DB: &str = include_str!("../../../apps/db/pkg/db.align");
const SQLITE: &str = include_str!("../../../apps/db/pkg/db/sqlite.align");
const POSTGRES: &str = include_str!("../../../apps/db/pkg/db/postgres.align");
const INTERNAL: &str = include_str!("../../../apps/db/pkg/db/internal.align");
const RESOURCE: &str = include_str!("../../../apps/db/pkg/db/internal/resource.align");
const DESCRIPTOR: &str = include_str!("../../../apps/db/pkg/db/internal/descriptor.align");
const INTERNAL_SQLITE: &str = include_str!("../../../apps/db/pkg/db/internal/sqlite.align");
const INTERNAL_POSTGRES: &str = include_str!("../../../apps/db/pkg/db/internal/postgres.align");

const LOOKUP: &str = r#"module app.lookup
import pkg.db

pub Params { id: i64 }
pub Row { id: i64, again: i64 }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.query(
  "SELECT :id AS id, :id AS again",
  [],
)
"#;

const TEST_STATE: &str = r#"module pkg.db.q5b1_test
import pkg.db

fn state(target: pkg.db.exec) -> raw {
  unsafe {
    return match target {
      Conn(reference) => resource.raw(reference)
      Tx(reference) => resource.raw(reference)
    }
  }
}

pub fn set_version(target: pkg.db.exec, value: u32) {
  unsafe { raw.store(state(target), 0, value) }
}

pub fn set_closed(target: pkg.db.exec, value: u8) {
  unsafe { raw.store(state(target), 5, value) }
}

pub fn set_reserved(target: pkg.db.exec, value: u16) {
  unsafe { raw.store(state(target), 6, value) }
}

pub fn take_native(target: pkg.db.exec) -> raw {
  unsafe {
    connection_state := state(target)
    native: raw := raw.load(connection_state, 8)
    raw.store(connection_state, 8, raw.null())
    return native
  }
}

pub fn restore_native(target: pkg.db.exec, native: raw) {
  unsafe { raw.store(state(target), 8, native) }
}
"#;

const MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import app.lookup

PrivateParams { id: i64 }
PrivateRow { id: i64 }

fn private_query() -> pkg.db.query<PrivateParams, PrivateRow> = pkg.db.query(
  "SELECT :id AS id",
  [],
)

fn summary(entry: pkg.db.MetaQueryEntry) -> bool = match entry {
  Summary => true
  _ => false
}

fn parameter(entry: pkg.db.MetaQueryEntry) -> bool = match entry {
  Parameter => true
  _ => false
}

fn column(entry: pkg.db.MetaQueryEntry) -> bool = match entry {
  Column => true
  _ => false
}

fn declared(state: pkg.db.MetaQueryState) -> bool = match state {
  Declared => true
  _ => false
}

fn unknown(value: pkg.db.MetaNullability) -> bool = match value {
  Unknown => true
  _ => false
}

fn ordinal(value: Option<i64>, expected: i64) -> bool = match value {
  Some(actual) => actual == expected
  None => false
}

fn absent(value: Option<str>) -> bool = match value {
  Some(_) => false
  None => true
}

fn main() -> i32 {
  opened := pkg.db.sqlite.connect(":memory:", [])
  connection := opened else { return 1 }
  target := pkg.db.exec_conn(connection)
  query := app.lookup.query()
  arena out {
    names_result := pkg.db.meta_query(target, query, pkg.db.MetaDetail.Names, out, [])
    names := names_result else { return 2 }
    if names.len() != 1 || !summary(names[0].entry) { return 3 }
    if names[0].query_id != "app.lookup.query" || names[0].artifact_digest.len() != 32 {
      return 4
    }
    if names[0].source_sql_hash.len() != 32 || names[0].driver_wire_sql_hash.len() != 32 {
      return 5
    }
    if !declared(names[0].state) || !unknown(names[0].nullable) { return 6 }
    if !absent(names[0].metadata_fingerprint) { return 7 }

    summary_result := pkg.db.meta_query(target, query, pkg.db.MetaDetail.Summary, out, [])
    summary_rows := summary_result else { return 8 }
    if summary_rows.len() != 4 { return 9 }
    if !summary(summary_rows[0].entry) || !parameter(summary_rows[1].entry) {
      return 10
    }
    if !column(summary_rows[2].entry) || !column(summary_rows[3].entry) { return 11 }
    parameter_name := summary_rows[1].source_name else { return 12 }
    if parameter_name != "id" { return 13 }
    if !ordinal(summary_rows[1].ordinal, 1) { return 14 }
    first_alias := summary_rows[2].source_alias else { return 15 }
    second_alias := summary_rows[3].source_alias else { return 16 }
    if first_alias != "id" || second_alias != "again" { return 17 }
    if !ordinal(summary_rows[2].ordinal, 0) { return 18 }
    if !ordinal(summary_rows[3].ordinal, 1) { return 19 }
    if !unknown(summary_rows[2].nullable) || !unknown(summary_rows[3].nullable) { return 20 }

    full_result := pkg.db.meta_query(target, query, pkg.db.MetaDetail.Full, out, [])
    full := full_result else { return 21 }
    if full.len() != 4 { return 22 }
    if !absent(full[0].prepare_identity) { return 23 }
    if !absent(full[1].native_type) { return 24 }
    if !absent(full[2].origin_table) { return 25 }

    bad_timeout := pkg.db.meta_query(target, query, pkg.db.MetaDetail.Names, out, [
      pkg.db.MetaOption.TimeoutNs(0),
    ])
    mut invalid_timeout_ok := false
    match bad_timeout {
      Err(error) => match error {
        Unsupported(contract) => {
          item_ok := contract.item == "db.meta.timeout_ns"
          mut query_id_ok := false
          match contract.query_id {
            Some(actual) => { query_id_ok = actual == "app.lookup.query" }
            None => {}
          }
          invalid_timeout_ok = item_ok && query_id_ok
        }
        _ => {}
      }
      Ok(_) => {}
    }
    if !invalid_timeout_ok { return 26 }
    duplicate := pkg.db.meta_query(target, query, pkg.db.MetaDetail.Names, out, [
      pkg.db.MetaOption.TimeoutNs(1),
      pkg.db.MetaOption.TimeoutNs(2),
    ])
    mut duplicate_ok := false
    match duplicate {
      Err(error) => match error {
        Unsupported(contract) => {
          duplicate_ok = contract.message == "duplicate database metadata timeout"
        }
        _ => {}
      }
      Ok(_) => {}
    }
    if !duplicate_ok { return 27 }

    private := private_query()
    private_result := pkg.db.meta_query(target, private, pkg.db.MetaDetail.Names, out, [])
    private_rows := private_result else { return 28 }
    if private_rows.len() != 1 || private_rows[0].query_id != "main.private_query" {
      return 29
    }
    return 42
  }
}
"#;

const NON_LIVE_MAIN: &str = r#"module main
import pkg.db
import pkg.db.sqlite
import pkg.db.q5b1_test
import app.lookup

fn rejected(result: Result<array<pkg.db.QueryMeta>, pkg.db.Error>) -> bool = match result {
  Ok(_) => false
  Err(error) => match error {
    Unsupported(contract) => contract.item == "db.meta.exec" && match contract.query_id {
      Some(query_id) => query_id == "app.lookup.query"
      None => false
    }
    _ => false
  }
}

fn main() -> i32 {
  opened := pkg.db.sqlite.connect(":memory:", [])
  connection := opened else { return 1 }
  target := pkg.db.exec_conn(connection)
  query := app.lookup.query()
  arena out {
    pkg.db.q5b1_test.set_closed(target, 1)
    closed := pkg.db.meta_query(target, query, pkg.db.MetaDetail.Names, out, [])
    pkg.db.q5b1_test.set_closed(target, 0)
    if !rejected(closed) { return 2 }

    pkg.db.q5b1_test.set_version(target, 0)
    wrong_version := pkg.db.meta_query(target, query, pkg.db.MetaDetail.Names, out, [])
    pkg.db.q5b1_test.set_version(target, 1)
    if !rejected(wrong_version) { return 3 }

    pkg.db.q5b1_test.set_reserved(target, 1)
    reserved := pkg.db.meta_query(target, query, pkg.db.MetaDetail.Names, out, [])
    pkg.db.q5b1_test.set_reserved(target, 0)
    if !rejected(reserved) { return 4 }

    native := pkg.db.q5b1_test.take_native(target)
    missing_native := pkg.db.meta_query(target, query, pkg.db.MetaDetail.Names, out, [])
    pkg.db.q5b1_test.restore_native(target, native)
    if !rejected(missing_native) { return 5 }

    valid := pkg.db.meta_query(target, query, pkg.db.MetaDetail.Names, out, [])
    rows := valid else { return 6 }
    if rows.len() != 1 { return 7 }
    return 42
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
        ("app/lookup.align", LOOKUP),
        ("main.align", MAIN),
    ]
}

fn non_live_package_files() -> Vec<(&'static str, &'static str)> {
    let mut files = package_files();
    files.retain(|(path, _)| *path != "main.align");
    files.push(("pkg/db/q5b1_test.align", TEST_STATE));
    files.push(("main.align", NON_LIVE_MAIN));
    files
}

#[test]
fn q5b1_publishes_exact_query_surface_without_catalog_stubs() {
    for required in [
        "pub DriverRestriction {",
        "pub MetaDetail {",
        "pub DatabaseMeta {",
        "pub SchemaMeta {",
        "pub TableMeta {",
        "pub ColumnMeta {",
        "pub KeyMeta {",
        "pub IndexMeta {",
        "pub QueryMeta {",
        "pub QueryPlan {",
        "pub MetaOption {",
        "pub ExplainOption {",
        "pub fn meta_query<P, R>(",
    ] {
        assert!(
            DB.contains(required),
            "missing public D12 surface `{required}`"
        );
    }
    for deferred in [
        "pub fn meta_database(",
        "pub fn meta_schemas(",
        "pub fn meta_tables(",
        "pub fn meta_table(",
        "pub fn meta_columns(",
        "pub fn meta_keys(",
        "pub fn meta_indexes(",
        "pub fn explain<",
    ] {
        assert!(
            !DB.contains(deferred),
            "Q5b2 operation acquired a placeholder `{deferred}`"
        );
    }
    assert!(
        SQLITE.contains("pub MetaOption {\n  IncludeInternalObjects\n  IncludeHiddenColumns\n}")
    );
    assert!(SQLITE.contains("pub ExplainOption {\n  QueryPlan\n  Bytecode\n}"));
    assert!(POSTGRES.contains("pub MetaOption {\n  SearchPathOnly\n  IncludeSystemCatalogs\n}"));
    assert!(POSTGRES.contains("  Analyze\n  Format(PlanFormat)"));
}

#[test]
fn materializer_call_is_query_only_and_exactly_typed() {
    let bad_command = r#"module pkg.db.bad
import pkg.db
import pkg.db.internal.descriptor

Params { id: i64 }

pub fn bad(statement: pkg.db.command<Params>) -> Option<pkg.db.QueryMeta> {
  unsafe { return pkg.db.internal.descriptor.materialize_meta(statement, 0, 0, 0) }
}
"#;
    let mut files = package_files();
    files.push(("pkg/db/bad.align", bad_command));
    let main = "module badmain\nimport pkg.db.bad\nfn main() -> i32 = 0\n";
    files.push(("badmain.align", main));
    assert!(check_multi_errs(
        "pkg-db-q5b1-materializer-command",
        &files,
        "badmain.align"
    ));

    let bad_driver = r#"module pkg.db.badtype
import pkg.db
import pkg.db.internal.descriptor

Params { id: i64 }
Row { id: i64 }

pub fn bad(statement: pkg.db.query<Params, Row>) -> Option<pkg.db.QueryMeta> {
  unsafe { return pkg.db.internal.descriptor.materialize_meta(statement, false, 0, 0) }
}
"#;
    let mut files = package_files();
    files.push(("pkg/db/badtype.align", bad_driver));
    let main = "module badtypemain\nimport pkg.db.badtype\nfn main() -> i32 = 0\n";
    files.push(("badtypemain.align", main));
    assert!(check_multi_errs(
        "pkg-db-q5b1-materializer-type",
        &files,
        "badtypemain.align"
    ));
}

#[test]
fn static_query_metadata_materializes_exact_declared_projections() {
    if !backend_available() {
        return;
    }
    let files = package_files();
    let output = build_and_run_multi_with_static_descriptors(
        "pkg-db-q5b1-query-meta-whole",
        &files,
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
fn meta_query_rejects_non_live_execution_targets_before_materialization() {
    if !backend_available() {
        return;
    }
    let files = non_live_package_files();
    let output = build_and_run_multi_with_static_descriptors(
        "pkg-db-q5b1-query-meta-live-exec",
        &files,
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
fn static_query_metadata_thunk_links_from_its_producer_unit() {
    if !backend_available() {
        return;
    }
    let files = package_files();
    let built = build_per_unit_multi("pkg-db-q5b1-query-meta-unit", &files, "main.align");
    let producer = built.unit("app.lookup");
    let mir = align_mir::print::program_to_string(&producer.mir);
    assert!(
        mir.contains("fn app.lookup$query$query_meta_v1("),
        "producer QueryMeta thunk is absent:\n{mir}"
    );
    let thunk = producer
        .mir
        .fns
        .iter()
        .find(|function| function.name.as_str() == "app.lookup$query$query_meta_v1")
        .expect("producer QueryMeta thunk");
    for statement in thunk.blocks.iter().flat_map(|block| &block.stmts) {
        match statement {
            align_mir::Stmt::Let(
                _,
                align_mir::Rvalue::Bin(..)
                | align_mir::Rvalue::StrLit(..)
                | align_mir::Rvalue::MakeEnum { .. }
                | align_mir::Rvalue::OptionSome(..)
                | align_mir::Rvalue::OptionNone
                | align_mir::Rvalue::Load(..),
            )
            | align_mir::Stmt::StoreField(..) => {}
            other => panic!("QueryMeta thunk performs non-materialization work: {other:?}"),
        }
    }
    assert!(thunk.blocks.iter().all(|block| matches!(
        block.term,
        align_mir::Term::Branch(..) | align_mir::Term::Return(..)
    )));
    let output = built.link_and_run();
    assert_eq!(
        output.status.code(),
        Some(42),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn checked_query_metadata_projection_uses_only_selected_driver_evidence() {
    let files = package_files();
    let built = build_per_unit_multi("pkg-db-q5b1-query-meta-checked", &files, "main.align");
    let producer = built.unit("app.lookup");
    let artifact = producer
        .static_artifacts
        .iter()
        .find(|artifact| artifact.descriptor_id == "app.lookup.query")
        .expect("Query artifact");
    let StaticArtifact::Query(mut query) = artifact.artifact.clone() else {
        panic!("selected artifact is not a Query")
    };
    for option in &mut query.static_options {
        if let StaticOptionValue::Check { policy } = &mut option.value {
            *policy = CheckPolicy::CheckedOptional;
        }
    }
    for entry in &mut query.driver_entries {
        entry.checked_metadata.policy = CheckPolicy::CheckedOptional;
    }
    let sqlite = query
        .driver_entries
        .iter_mut()
        .find(|entry| entry.driver == Driver::SQLite)
        .expect("SQLite entry");
    sqlite.checked_metadata.state = VerificationState::DatabaseChecked;
    sqlite.checked_metadata.metadata_format_version = Some(1);
    sqlite.checked_metadata.metadata_digest = Some(Hash128::of(b"checked metadata"));
    sqlite.checked_metadata.query_evidence = Some(CheckedQueryEvidence {
        prepare_identity: "prepare-v1".to_string(),
        schema_identity: "schema-v1".to_string(),
        server_identity: "server-v1".to_string(),
        parameters: vec![CheckedParameterMeta {
            ordinal: 1,
            native_type: Some("INTEGER".to_string()),
            native_type_id: None,
        }],
        columns: vec![
            CheckedColumnMeta {
                ordinal: 0,
                native_type: Some("INTEGER".to_string()),
                native_type_id: Some(1),
                origin_schema: Some("main".to_string()),
                origin_table: Some("items".to_string()),
                origin_column: Some("id".to_string()),
                nullable: MetaNullability::No,
            },
            CheckedColumnMeta {
                ordinal: 1,
                native_type: Some("INTEGER".to_string()),
                native_type_id: Some(1),
                origin_schema: None,
                origin_table: None,
                origin_column: None,
                nullable: MetaNullability::Unknown,
            },
        ],
    });
    let digest = Hash128::of(b"checked Query artifact");
    let artifact = StaticArtifact::Query(query.clone());

    let names = generated_query_meta_rows(
        &artifact,
        digest,
        Driver::SQLite,
        GeneratedMetaDetail::Names,
    )
    .expect("Names rows");
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].entry, GeneratedQueryMetaEntry::Summary);
    assert_eq!(names[0].metadata_fingerprint, None);

    let summary = generated_query_meta_rows(
        &artifact,
        digest,
        Driver::SQLite,
        GeneratedMetaDetail::Summary,
    )
    .expect("Summary rows");
    assert_eq!(summary.len(), 4);
    let fingerprint = Hash128::of(b"checked metadata").to_hex();
    assert_eq!(
        summary[0].metadata_fingerprint.as_deref(),
        Some(fingerprint.as_str())
    );
    assert_eq!(summary[0].prepare_identity, None);
    assert_eq!(summary[1].ordinal, Some(1));
    assert_eq!(summary[1].native_type, None);
    assert_eq!(summary[2].nullable, MetaNullability::Unknown);

    let full =
        generated_query_meta_rows(&artifact, digest, Driver::SQLite, GeneratedMetaDetail::Full)
            .expect("Full rows");
    assert_eq!(full[0].artifact_digest, digest.to_hex());
    assert_eq!(full[0].prepare_identity.as_deref(), Some("prepare-v1"));
    assert_eq!(full[1].native_type.as_deref(), Some("INTEGER"));
    assert_eq!(full[2].origin_table.as_deref(), Some("items"));
    assert_eq!(full[2].nullable, MetaNullability::No);
    assert_eq!(full[3].nullable, MetaNullability::Unknown);

    let mut missing_fingerprint = query.clone();
    missing_fingerprint
        .driver_entries
        .iter_mut()
        .find(|entry| entry.driver == Driver::SQLite)
        .expect("SQLite entry")
        .checked_metadata
        .metadata_digest = None;
    assert_eq!(
        generated_query_meta_rows(
            &StaticArtifact::Query(missing_fingerprint),
            digest,
            Driver::SQLite,
            GeneratedMetaDetail::Summary,
        ),
        Err("DatabaseChecked Query has no metadata fingerprint")
    );

    let postgres = generated_query_meta_rows(
        &artifact,
        digest,
        Driver::PostgreSQL,
        GeneratedMetaDetail::Full,
    )
    .expect("PostgreSQL declared rows");
    assert_eq!(postgres[0].state, VerificationState::Declared);
    assert_eq!(postgres[0].metadata_fingerprint, None);
    assert!(postgres.iter().all(|row| row.native_type.is_none()));
}
