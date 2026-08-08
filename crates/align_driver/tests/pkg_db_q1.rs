//! pkg.db Q1/D1 owner tests: checked public descriptors through exact static artifacts.

use align_driver::{
    build_per_unit, build_static_artifacts, check, emit_llvm_ir, execute_fake_static,
    metadata_path, resolve_static_descriptors, BuildTarget, Driver, FakeCardinality, FakeExecutionError,
    FakeStatementKind, FakeValue, GeneratedStaticRuntime, StaticDescriptorInputErrorCause,
    StaticInputError,
};
use align_interface::{
    static_options_hash, BindRetention, CheckPolicy, StaticArtifact, StaticOption,
    StaticOptionOwner, StaticOptionValue, VerificationState,
};
use align_sema::StaticDescriptorConsumer;
use align_span::SourceMap;
use std::fs::{create_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DB: &str = include_str!("../../../apps/db/pkg/db.align");
const SQLITE: &str = include_str!("../../../apps/db/pkg/db/sqlite.align");
const POSTGRES: &str = include_str!("../../../apps/db/pkg/db/postgres.align");
const INTERNAL: &str = include_str!("../../../apps/db/pkg/db/internal.align");
const RESOURCE: &str = include_str!("../../../apps/db/pkg/db/internal/resource.align");
const DESCRIPTOR: &str = include_str!("../../../apps/db/pkg/db/internal/descriptor.align");
const INTERNAL_SQLITE: &str = include_str!("../../../apps/db/pkg/db/internal/sqlite.align");
const INTERNAL_POSTGRES: &str = include_str!("../../../apps/db/pkg/db/internal/postgres.align");

struct TempProject(PathBuf);

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(label: &str) -> TempProject {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "align-pkg-db-q1-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(root.join("pkg/db")).expect("package directories");
    create_dir_all(root.join("pkg/db/internal")).expect("internal package directories");
    write(root.join("pkg/db.align"), DB).expect("pkg.db");
    write(root.join("pkg/db/sqlite.align"), SQLITE).expect("pkg.db.sqlite");
    write(root.join("pkg/db/postgres.align"), POSTGRES).expect("pkg.db.postgres");
    write(root.join("pkg/db/internal.align"), INTERNAL).expect("pkg.db internal");
    write(root.join("pkg/db/internal/resource.align"), RESOURCE).expect("pkg.db resource");
    write(root.join("pkg/db/internal/descriptor.align"), DESCRIPTOR)
        .expect("pkg.db descriptor bridge");
    write(root.join("pkg/db/internal/sqlite.align"), INTERNAL_SQLITE)
        .expect("pkg.db SQLite execution internals");
    write(root.join("pkg/db/internal/postgres.align"), INTERNAL_POSTGRES)
        .expect("pkg.db PostgreSQL execution internals");
    TempProject(root)
}

fn write_sources(root: &Path) -> (PathBuf, String) {
    create_dir_all(root.join("app")).expect("application module directory");
    write(
        root.join("app/users.align"),
        concat!(
            "module app.users\n",
            "import pkg.db\n",
            "pub Params { id: i64 }\n",
            "pub Row { id: i64, name: str }\n",
            "pub fn query() -> pkg.db.query<Params, Row> = ",
            "pkg.db.query(\"SELECT :id AS id, 'name' AS name WHERE :id = :id\", [])\n",
        ),
    )
    .expect("query module");
    write(
        root.join("app/prune.align"),
        concat!(
            "module app.prune\n",
            "import pkg.db\n",
            "pub Params { id: i64 }\n",
            "pub fn command() -> pkg.db.command<Params> = pkg.db.command_file([])\n",
        ),
    )
    .expect("command module");
    write(
        root.join("app/prune.sql"),
        "DELETE FROM sessions WHERE user_id = :id\n",
    )
    .expect("command SQL");
    write(
        root.join("app/find.align"),
        concat!(
            "module app.find\n",
            "import pkg.db\n",
            "pub Params { id: i64 }\n",
            "pub Row { id: i64 }\n",
            "pub fn query() -> pkg.db.query<Params, Row> = pkg.db.query_file([])\n",
        ),
    )
    .expect("file Query module");
    write(root.join("app/find.sql"), "SELECT :id AS id\n").expect("Query SQL");
    write(
        root.join("app/touch.align"),
        concat!(
            "module app.touch\n",
            "import pkg.db\n",
            "pub Params { id: i64 }\n",
            "pub fn command() -> pkg.db.command<Params> = ",
            "pkg.db.command(\"UPDATE users SET touched = 1 WHERE id = :id\", [])\n",
        ),
    )
    .expect("inline command module");
    let entry = concat!(
        "module main\n",
        "import app.users\n",
        "import app.prune\n",
        "import app.find\n",
        "import app.touch\n",
        "fn main() -> i32 = 0\n",
    )
    .to_string();
    let entry_path = root.join("main.align");
    write(&entry_path, &entry).expect("entry");
    (entry_path, entry)
}

fn checked_query_metadata_json(
    query: &align_interface::StaticQueryArtifact,
    driver: Driver,
    static_options: &[StaticOption],
) -> Vec<u8> {
    let entry = query
        .driver_entries
        .iter()
        .find(|entry| entry.driver == driver)
        .expect("driver entry");
    let driver_name = match driver {
        Driver::SQLite => "sqlite",
        Driver::PostgreSQL => "postgres",
    };
    let restriction = match query.driver_restriction {
        align_interface::DriverRestriction::AnySupportedDriver => "any_supported_driver",
        align_interface::DriverRestriction::SQLiteOnly => "sqlite_only",
        align_interface::DriverRestriction::PostgreSQLOnly => "postgres_only",
    };
    let statement_class = match query.query_meta_plan.statement_class {
        align_interface::MetaStatementClass::Select => "select",
        align_interface::MetaStatementClass::Dml => "dml",
        align_interface::MetaStatementClass::Ddl => "ddl",
        align_interface::MetaStatementClass::Native => "native",
        align_interface::MetaStatementClass::Unknown => "unknown",
    };
    let source_identity = match &query.source_identity {
        align_interface::SqlSourceIdentity::Inline { query_or_command_id } => {
            format!("{{\"kind\":\"inline\",\"descriptor_id\":\"{query_or_command_id}\"}}")
        }
        align_interface::SqlSourceIdentity::File { logical_path } => {
            format!("{{\"kind\":\"file\",\"logical_path\":\"{logical_path}\"}}")
        }
    };
    let parameters = query
        .query_meta_plan
        .parameters
        .iter()
        .map(|parameter| {
            format!(
                "{{\"source_name\":\"{}\",\"protocol_ordinal\":{},\"logical_type\":\"{}\",\"native_type\":null,\"native_type_id\":null}}",
                parameter.source_name, parameter.ordinal, parameter.logical_type
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let columns = query
        .query_meta_plan
        .columns
        .iter()
        .map(|column| {
            format!(
                "{{\"ordinal\":{},\"source_alias\":\"{}\",\"logical_type\":\"{}\",\"native_type\":null,\"native_type_id\":null,\"nullable\":\"unknown\",\"origin_schema\":null,\"origin_table\":null,\"origin_column\":null}}",
                column.ordinal, column.source_alias, column.logical_type
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"format_version\":1,\"descriptor_id\":\"{}\",\"module\":\"{}\",\"item\":\"{}\",\"driver\":\"{driver_name}\",\"driver_restriction\":\"{restriction}\",\"statement_kind\":\"query\",\"statement_class\":\"{statement_class}\",\"source_identity\":{source_identity},\"source_sql_hash\":\"{}\",\"wire_sql_hash\":\"{}\",\"rewrite_format_version\":{},\"static_options_hash\":\"{}\",\"params_fingerprint\":\"{}\",\"row_fingerprint\":\"{}\",\"schema_fingerprint\":\"01000000000000000000000000000000\",\"engine_version\":\"test-engine\",\"driver_version\":\"test-driver\",\"search_path\":[],\"extensions\":[],\"parameters\":[{parameters}],\"columns\":[{columns}]}}\n",
        query.query_id,
        query.unit,
        query.item,
        query.source_sql_hash.to_hex(),
        entry.wire_sql_hash.to_hex(),
        entry.rewrite_format_version,
        static_options_hash(static_options).expect("static options hash").to_hex(),
        query.params_fingerprint.to_hex(),
        query.row_fingerprint.to_hex(),
    )
    .into_bytes()
}

fn checked_command_metadata_json(
    command: &align_interface::StaticCommandArtifact,
    driver: Driver,
    static_options: &[StaticOption],
) -> Vec<u8> {
    let entry = command
        .driver_entries
        .iter()
        .find(|entry| entry.driver == driver)
        .expect("driver entry");
    let fields = match &command.params_type.root {
        align_interface::CanonicalType::Named { path, args } => command
            .params_type
            .definitions
            .iter()
            .find(|definition| definition.path == *path && definition.args == *args)
            .and_then(|definition| match &definition.kind {
                align_interface::CanonicalDefinitionBody::Struct { fields } => Some(fields),
                align_interface::CanonicalDefinitionBody::Sum { .. } => None,
            })
            .expect("Params fields"),
        _ => panic!("named Params contract"),
    };
    let mut bindings = entry.bindings.iter().collect::<Vec<_>>();
    bindings.sort_by_key(|binding| binding.protocol_ordinal);
    let parameters = bindings
        .iter()
        .map(|binding| {
            let field = &fields[binding.params_field_ordinal as usize];
            format!(
                "{{\"source_name\":\"{}\",\"protocol_ordinal\":{},\"logical_type\":\"{}\",\"native_type\":null,\"native_type_id\":null}}",
                binding.source_name,
                binding.protocol_ordinal,
                field.ty.spelling()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let source_identity = match &command.source_identity {
        align_interface::SqlSourceIdentity::Inline { query_or_command_id } => {
            format!("{{\"kind\":\"inline\",\"descriptor_id\":\"{query_or_command_id}\"}}")
        }
        align_interface::SqlSourceIdentity::File { logical_path } => {
            format!("{{\"kind\":\"file\",\"logical_path\":\"{logical_path}\"}}")
        }
    };
    format!(
        "{{\"format_version\":1,\"descriptor_id\":\"{}\",\"module\":\"{}\",\"item\":\"{}\",\"driver\":\"sqlite\",\"driver_restriction\":\"sqlite_only\",\"statement_kind\":\"command\",\"statement_class\":\"dml\",\"source_identity\":{source_identity},\"source_sql_hash\":\"{}\",\"wire_sql_hash\":\"{}\",\"rewrite_format_version\":{},\"static_options_hash\":\"{}\",\"params_fingerprint\":\"{}\",\"row_fingerprint\":null,\"schema_fingerprint\":\"01000000000000000000000000000000\",\"engine_version\":\"test-engine\",\"driver_version\":\"test-driver\",\"search_path\":[],\"extensions\":[],\"parameters\":[{parameters}],\"columns\":[]}}\n",
        command.command_id,
        command.unit,
        command.item,
        command.source_sql_hash.to_hex(),
        entry.wire_sql_hash.to_hex(),
        entry.rewrite_format_version,
        static_options_hash(static_options)
            .expect("static options hash")
            .to_hex(),
        command.params_fingerprint.to_hex(),
    )
    .into_bytes()
}

#[test]
fn public_surface_whole_and_per_unit() {
    let project = project("surface");
    let (entry_path, entry) = write_sources(&project.0);
    let mut whole_sources = SourceMap::new();
    let whole = check(
        &mut whole_sources,
        entry_path.to_str().expect("UTF-8 entry path"),
        &entry,
    );
    assert!(
        !whole.diags.has_errors(),
        "whole-program diagnostics: {:?}",
        whole.diags.iter().collect::<Vec<_>>()
    );
    assert_eq!(whole.static_descriptors.len(), 4);
    let mut per_unit_sources = SourceMap::new();
    let per_unit = build_per_unit(
        &mut per_unit_sources,
        entry_path.to_str().expect("UTF-8 entry path"),
        &entry,
    );
    assert!(
        !per_unit.diags.has_errors(),
        "per-unit diagnostics: {:?}",
        per_unit.diags.iter().collect::<Vec<_>>()
    );
    let mut per_unit_descriptors = per_unit
        .units
        .iter()
        .flat_map(|unit| unit.static_descriptors.iter())
        .collect::<Vec<_>>();
    per_unit_descriptors.sort_by_key(|descriptor| descriptor.descriptor_id.as_str());
    assert_eq!(
        per_unit_descriptors
            .iter()
            .map(|descriptor| descriptor.descriptor_id.as_str())
            .collect::<Vec<_>>(),
        [
            "app.find.query",
            "app.prune.command",
            "app.touch.command",
            "app.users.query",
        ]
    );
    for descriptor in per_unit_descriptors {
        let whole_descriptor = whole
            .static_descriptors
            .iter()
            .find(|candidate| candidate.descriptor_id == descriptor.descriptor_id)
            .expect("whole descriptor");
        assert_eq!(descriptor.params_contract, whole_descriptor.params_contract);
        assert_eq!(descriptor.row_contract, whole_descriptor.row_contract);
        assert_eq!(descriptor.static_options, whole_descriptor.static_options);
    }
}

#[test]
fn file_constructors_accept_explicit_paths_on_the_shipped_surface() {
    let project = project("explicit-paths");
    create_dir_all(project.0.join("app/sql")).expect("SQL directory");
    write(project.0.join("app/sql/query.sql"), "SELECT :id AS id\n").expect("query SQL");
    write(project.0.join("app/sql/command.sql"), "UPDATE users SET seen = 1 WHERE id = :id\n")
        .expect("command SQL");
    write(
        project.0.join("app/paths.align"),
        concat!(
            "module app.paths\n",
            "import pkg.db\n",
            "import pkg.db.sqlite\n",
            "import pkg.db.postgres\n",
            "pub Params { id: i64 }\n",
            "pub Row { id: i64 }\n",
            "pub fn common_query() -> pkg.db.query<Params, Row> = ",
            "pkg.db.query_file(\"sql/query.sql\", [])\n",
            "pub fn common_command() -> pkg.db.command<Params> = ",
            "pkg.db.command_file(\"sql/command.sql\", [])\n",
            "pub fn sqlite_query() -> pkg.db.query<Params, Row> = ",
            "pkg.db.sqlite.query_file(\"sql/query.sql\", [], [])\n",
            "pub fn postgres_command() -> pkg.db.command<Params> = ",
            "pkg.db.postgres.command_file(\"sql/command.sql\", [], [])\n",
        ),
    )
    .expect("path module");
    let entry = "module main\nimport app.paths\nfn main() -> i32 = 0\n";
    let entry_path = project.0.join("main.align");
    write(&entry_path, entry).expect("entry");
    let mut source_map = SourceMap::new();
    let checked = check(
        &mut source_map,
        entry_path.to_str().expect("UTF-8 entry path"),
        entry,
    );
    assert!(
        !checked.diags.has_errors(),
        "explicit-path diagnostics: {:?}",
        checked.diags.iter().collect::<Vec<_>>()
    );
    assert_eq!(checked.static_descriptors.len(), 4);
    let resolved = resolve_static_descriptors(
        &project.0,
        &mut source_map,
        &checked.static_descriptors,
        align_interface::Hash128::of(b"pkg-db-q1-explicit-paths"),
    )
    .expect("explicit-path descriptors");
    assert!(resolved.inputs.iter().all(|input| matches!(
        &input.input.source,
        align_interface::SqlSourceIdentity::File { logical_path }
            if matches!(logical_path.as_str(), "app/sql/query.sql" | "app/sql/command.sql")
    )));
}

#[test]
fn checked_metadata_promotes_current_snapshots_and_obeys_policy_on_stale_data() {
    let project = project("checked-metadata");
    create_dir_all(project.0.join("app")).expect("application directory");
    let module_path = project.0.join("app/checked.align");
    let optional_source = concat!(
        "module app.checked\n",
        "import pkg.db\n",
        "import pkg.db.sqlite\n",
        "pub Params { id: i64 }\n",
        "pub Row { id: i64 }\n",
        "pub fn query() -> pkg.db.query<Params, Row> = ",
        "pkg.db.sqlite.query(\"SELECT :id AS id\", ",
        "[pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedOptional)], [])\n",
    );
    write(&module_path, optional_source).expect("checked module");
    let entry = "module main\nimport app.checked\nfn main() -> i32 = 0\n";
    let entry_path = project.0.join("main.align");
    write(&entry_path, entry).expect("entry");

    let build = |seed: &[u8]| {
        let mut source_map = SourceMap::new();
        let checked = check(
            &mut source_map,
            entry_path.to_str().expect("UTF-8 entry path"),
            entry,
        );
        assert!(
            !checked.diags.has_errors(),
            "checked-metadata diagnostics: {:?}",
            checked.diags.iter().collect::<Vec<_>>()
        );
        let resolved = resolve_static_descriptors(
            &project.0,
            &mut source_map,
            &checked.static_descriptors,
            align_interface::Hash128::of(seed),
        )
        .expect("checked-metadata inputs");
        build_static_artifacts(&checked.static_descriptors, &resolved)
    };

    let declared = build(b"checked-metadata-declared").expect("optional missing metadata");
    let StaticArtifact::Query(template) = &declared[0].artifact else {
        panic!("query artifact")
    };
    assert_eq!(
        template.driver_entries[0].checked_metadata.state,
        VerificationState::Declared
    );
    let optional_metadata = checked_query_metadata_json(template, Driver::SQLite, &template.static_options);
    let metadata_path = metadata_path(&project.0, "app.checked.query", Driver::SQLite)
        .expect("metadata path");
    create_dir_all(metadata_path.parent().expect("metadata directory"))
        .expect("metadata directory");
    write(&metadata_path, optional_metadata).expect("optional metadata");

    let checked_optional = build(b"checked-metadata-optional").expect("current optional metadata");
    let StaticArtifact::Query(query) = &checked_optional[0].artifact else {
        panic!("query artifact")
    };
    let metadata = &query.driver_entries[0].checked_metadata;
    assert_eq!(metadata.policy, CheckPolicy::CheckedOptional);
    assert_eq!(metadata.state, VerificationState::DatabaseChecked);
    assert!(metadata.metadata_digest.is_some());
    assert!(metadata.query_evidence.is_some());

    let required_source = optional_source.replace("CheckedOptional", "CheckedRequired");
    let required_options = [StaticOption {
        owner: StaticOptionOwner::Common,
        value: StaticOptionValue::Check {
            policy: CheckPolicy::CheckedRequired,
        },
    }];
    write(
        &metadata_path,
        checked_query_metadata_json(template, Driver::SQLite, &required_options),
    )
    .expect("required metadata");
    write(&module_path, &required_source).expect("required source");
    let checked_required = build(b"checked-metadata-required").expect("current required metadata");
    let StaticArtifact::Query(query) = &checked_required[0].artifact else {
        panic!("query artifact")
    };
    assert_eq!(
        query.driver_entries[0].checked_metadata.state,
        VerificationState::DatabaseChecked
    );

    write(
        &module_path,
        required_source.replace("SELECT :id AS id", "SELECT :id + 1 AS id"),
    )
    .expect("stale required source");
    let required_error = build(b"checked-metadata-stale-required")
        .expect_err("stale required metadata must fail");
    assert!(required_error.reason.contains("artifact inputs changed"));

    write(
        &module_path,
        optional_source.replace("SELECT :id AS id", "SELECT :id + 1 AS id"),
    )
    .expect("stale optional source");
    let stale_optional = build(b"checked-metadata-stale-optional")
        .expect("stale optional metadata falls back to declared");
    let StaticArtifact::Query(query) = &stale_optional[0].artifact else {
        panic!("query artifact")
    };
    assert_eq!(
        query.driver_entries[0].checked_metadata.state,
        VerificationState::Declared
    );
}

#[test]
fn checked_command_metadata_promotes_without_query_evidence() {
    let project = project("checked-command-metadata");
    create_dir_all(project.0.join("app")).expect("application directory");
    let module_path = project.0.join("app/checked_command.align");
    let optional_source = concat!(
        "module app.checked_command\n",
        "import pkg.db\n",
        "import pkg.db.sqlite\n",
        "pub Params { id: i64 }\n",
        "pub fn command() -> pkg.db.command<Params> = ",
        "pkg.db.sqlite.command(\"UPDATE users SET seen = 1 WHERE id = :id\", ",
        "[pkg.db.CommandOption.Check(pkg.db.CheckPolicy.CheckedOptional)], [])\n",
    );
    write(&module_path, optional_source).expect("command module");
    let entry = "module main\nimport app.checked_command\nfn main() -> i32 = 0\n";
    let entry_path = project.0.join("main.align");
    write(&entry_path, entry).expect("entry");
    let build = |seed: &[u8]| {
        let mut source_map = SourceMap::new();
        let checked = check(
            &mut source_map,
            entry_path.to_str().expect("UTF-8 entry path"),
            entry,
        );
        assert!(!checked.diags.has_errors());
        let resolved = resolve_static_descriptors(
            &project.0,
            &mut source_map,
            &checked.static_descriptors,
            align_interface::Hash128::of(seed),
        )
        .expect("command metadata inputs");
        build_static_artifacts(&checked.static_descriptors, &resolved)
    };
    let declared = build(b"checked-command-declared").expect("optional missing metadata");
    let StaticArtifact::Command(template) = &declared[0].artifact else {
        panic!("command artifact")
    };
    let metadata_path = metadata_path(
        &project.0,
        "app.checked_command.command",
        Driver::SQLite,
    )
    .expect("metadata path");
    create_dir_all(metadata_path.parent().expect("metadata directory"))
        .expect("metadata directory");
    write(
        &metadata_path,
        checked_command_metadata_json(template, Driver::SQLite, &template.static_options),
    )
    .expect("optional command metadata");
    let checked_optional = build(b"checked-command-optional").expect("current optional metadata");
    let StaticArtifact::Command(command) = &checked_optional[0].artifact else {
        panic!("command artifact")
    };
    assert_eq!(
        command.driver_entries[0].checked_metadata.state,
        VerificationState::DatabaseChecked
    );
    assert!(command.driver_entries[0]
        .checked_metadata
        .query_evidence
        .is_none());

    let required_options = [StaticOption {
        owner: StaticOptionOwner::Common,
        value: StaticOptionValue::Check {
            policy: CheckPolicy::CheckedRequired,
        },
    }];
    write(
        &metadata_path,
        checked_command_metadata_json(template, Driver::SQLite, &required_options),
    )
    .expect("required command metadata");
    write(
        &module_path,
        optional_source.replace("CheckedOptional", "CheckedRequired"),
    )
    .expect("required command source");
    let checked_required = build(b"checked-command-required").expect("current required metadata");
    let StaticArtifact::Command(command) = &checked_required[0].artifact else {
        panic!("command artifact")
    };
    assert_eq!(
        command.driver_entries[0].checked_metadata.state,
        VerificationState::DatabaseChecked
    );
}

#[test]
fn artifact_semantics_and_checked_in_goldens() {
    let project = project("artifact");
    let (entry_path, entry) = write_sources(&project.0);
    let mut source_map = SourceMap::new();
    let checked = check(
        &mut source_map,
        entry_path.to_str().expect("UTF-8 entry path"),
        &entry,
    );
    assert!(
        !checked.diags.has_errors(),
        "diagnostics: {:?}",
        checked.diags.iter().collect::<Vec<_>>()
    );
    let resolved = resolve_static_descriptors(
        &project.0,
        &mut source_map,
        &checked.static_descriptors,
        align_interface::Hash128::of(b"pkg-db-q1-resolution"),
    )
    .expect("resolved static inputs");
    let built = build_static_artifacts(&checked.static_descriptors, &resolved)
        .expect("validated static artifacts");
    assert_eq!(built.len(), 4);

    let query_artifact = built
        .iter()
        .find(|artifact| artifact.descriptor_id == "app.users.query")
        .expect("query artifact");
    let StaticArtifact::Query(query) = &query_artifact.artifact else {
        panic!("query descriptor built as command")
    };
    assert_eq!(query.occurrences.len(), 3);
    assert!(query
        .occurrences
        .iter()
        .all(|occurrence| occurrence.source_name == "id" && occurrence.protocol_ordinal == 1));
    assert_eq!(query.driver_entries.len(), 2);
    let sqlite = query
        .driver_entries
        .iter()
        .find(|entry| entry.driver == Driver::SQLite)
        .expect("SQLite entry");
    assert_eq!(sqlite.wire_sql, query.source_sql);
    assert_eq!(sqlite.bindings[0].retention, BindRetention::BindValue);
    let postgres = query
        .driver_entries
        .iter()
        .find(|entry| entry.driver == Driver::PostgreSQL)
        .expect("PostgreSQL entry");
    assert_eq!(
        std::str::from_utf8(&postgres.wire_sql).expect("wire UTF-8"),
        "SELECT $1 AS id, 'name' AS name WHERE $1 = $1"
    );
    assert_eq!(query.query_meta_plan.parameters.len(), 1);
    assert_eq!(query.query_meta_plan.columns.len(), 2);
    assert!(!query.decoded_span_map.is_empty());
    let GeneratedStaticRuntime::Query(runtime) = &query_artifact.runtime else {
        panic!("query artifact has command runtime data")
    };
    assert!(runtime.bytes.starts_with(b"ALIGNQST"));
    assert_eq!(
        query_artifact.digest.to_hex(),
        "11a2b7081c610a9c1745774360b1b6e7"
    );
    assert_eq!(
        align_interface::Hash128::of(&runtime.bytes).to_hex(),
        "c750d9b771de5bf8e9e0661a51ba5fb6"
    );
    assert_eq!(runtime.artifact_digest, query_artifact.digest);
    assert_eq!(runtime.static_options, query.static_options);
    assert_eq!(runtime.decoder.fields.len(), 2);
    assert_eq!(runtime.decoder.fields[0].row_field_ordinal, 0);
    assert_eq!(runtime.decoder.fields[1].row_field_ordinal, 1);
    assert!(runtime.drivers.iter().all(|driver| {
        driver.binder.fields.len() == 1
            && driver.binder.fields[0].params_field_ordinal == 0
            && driver.binder.fields[0].protocol_ordinal == 1
    }));

    let command_artifact = built
        .iter()
        .find(|artifact| artifact.descriptor_id == "app.prune.command")
        .expect("command artifact");
    let StaticArtifact::Command(command) = &command_artifact.artifact else {
        panic!("command descriptor built as Query")
    };
    assert_eq!(command.occurrences.len(), 1);
    assert!(command.decoded_span_map.is_empty());
    assert_eq!(command.driver_entries.len(), 2);
    let GeneratedStaticRuntime::Command(runtime) = &command_artifact.runtime else {
        panic!("command artifact has Query runtime data")
    };
    assert!(runtime.bytes.starts_with(b"ALIGNCST"));
    assert_eq!(
        command_artifact.digest.to_hex(),
        "569127377c471dc7ba327efc91170a66"
    );
    assert_eq!(
        align_interface::Hash128::of(&runtime.bytes).to_hex(),
        "108cb4f217617910decb55349129e800"
    );
    assert_eq!(runtime.artifact_digest, command_artifact.digest);
    assert_eq!(runtime.static_options, command.static_options);

    // Pin independently checked format bytes/digests at the owner boundary. The values are filled
    // from the first clean implementation run and thereafter change only with an intentional
    // artifact-format or semantic-contract update.
    for artifact in &built {
        assert_eq!(
            artifact.digest,
            align_interface::static_artifact_digest(&artifact.bytes).unwrap()
        );
    }
}

#[test]
fn scalar_bind_and_decode_shape_matrix() {
    let project = project("scalar-shapes");
    create_dir_all(project.0.join("app")).expect("application directory");
    let source = concat!(
        "module app.shapes\n",
        "import pkg.db\n",
        "pub Params { b: bool, i16v: i16, i32v: i32, i64v: i64, f32v: f32, ",
        "f64v: f64, text_view: str, text_owned: string, bytes_view: slice<u8>, ",
        "bytes_owned: array<u8>, maybe: Option<i64> }\n",
        "pub Row { b: bool, i16v: i16, i32v: i32, i64v: i64, f32v: f32, ",
        "f64v: f64, text_view: str, bytes_view: slice<u8>, maybe: Option<i64> }\n",
        "pub fn query() -> pkg.db.query<Params, Row> = pkg.db.query(\"SELECT ",
        ":b, :i16v, :i32v, :i64v, :f32v, :f64v, :text_view, :text_owned, ",
        ":bytes_view, :bytes_owned, :maybe\", [])\n",
    );
    write(project.0.join("app/shapes.align"), source).expect("shape module");
    let entry = "module main\nimport app.shapes\nfn main() -> i32 = 0\n";
    let entry_path = project.0.join("main.align");
    write(&entry_path, entry).expect("entry");
    let mut source_map = SourceMap::new();
    let checked = check(
        &mut source_map,
        entry_path.to_str().expect("UTF-8 entry path"),
        entry,
    );
    assert!(
        !checked.diags.has_errors(),
        "shape diagnostics: {:?}",
        checked.diags.iter().collect::<Vec<_>>()
    );
    let resolved = resolve_static_descriptors(
        &project.0,
        &mut source_map,
        &checked.static_descriptors,
        align_interface::Hash128::of(b"pkg-db-q1-shapes"),
    )
    .expect("resolved shape descriptor");
    let artifacts =
        build_static_artifacts(&checked.static_descriptors, &resolved).expect("shape artifact");
    let runtime = &artifacts[0].runtime;
    let params = vec![
        FakeValue::Bool(true),
        FakeValue::Integer(16),
        FakeValue::Integer(32),
        FakeValue::Integer(64),
        FakeValue::Float(32.0),
        FakeValue::Float(64.0),
        FakeValue::Text(b"view".to_vec()),
        FakeValue::Text(b"owned".to_vec()),
        FakeValue::Bytes(vec![1]),
        FakeValue::Bytes(vec![2]),
        FakeValue::Null,
    ];
    let row = vec![
        FakeValue::Bool(true),
        FakeValue::Integer(16),
        FakeValue::Integer(32),
        FakeValue::Integer(64),
        FakeValue::Float(32.0),
        FakeValue::Float(64.0),
        FakeValue::Text(b"view".to_vec()),
        FakeValue::Bytes(vec![1]),
        FakeValue::Null,
    ];
    execute_fake_static(
        runtime,
        Driver::SQLite,
        &params,
        std::slice::from_ref(&row),
        FakeCardinality::ExactlyOne,
    )
    .expect("every admitted scalar shape executes");

    let wrong_params = [
        FakeValue::Integer(1),
        FakeValue::Integer(i64::from(i16::MAX) + 1),
        FakeValue::Integer(i64::from(i32::MAX) + 1),
        FakeValue::Bool(false),
        FakeValue::Integer(1),
        FakeValue::Integer(1),
        FakeValue::Bool(false),
        FakeValue::Bool(false),
        FakeValue::Text(Vec::new()),
        FakeValue::Text(Vec::new()),
        FakeValue::Text(Vec::new()),
    ];
    for (ordinal, wrong) in wrong_params.into_iter().enumerate() {
        let mut malformed = params.clone();
        malformed[ordinal] = wrong;
        assert!(matches!(
            execute_fake_static(
                runtime,
                Driver::SQLite,
                &malformed,
                &[],
                FakeCardinality::All,
            ),
            Err(FakeExecutionError::ParameterType { ordinal: actual, .. })
                if actual as usize == ordinal
        ));
    }

    let wrong_row = [
        FakeValue::Integer(1),
        FakeValue::Integer(i64::from(i16::MAX) + 1),
        FakeValue::Integer(i64::from(i32::MAX) + 1),
        FakeValue::Bool(false),
        FakeValue::Integer(1),
        FakeValue::Integer(1),
        FakeValue::Bool(false),
        FakeValue::Text(Vec::new()),
        FakeValue::Text(Vec::new()),
    ];
    for (ordinal, wrong) in wrong_row.into_iter().enumerate() {
        let mut malformed = row.clone();
        malformed[ordinal] = wrong;
        assert!(matches!(
            execute_fake_static(
                runtime,
                Driver::SQLite,
                &params,
                &[malformed],
                FakeCardinality::All,
            ),
            Err(FakeExecutionError::RowType {
                row: 0,
                ordinal: actual,
                ..
            }) if actual as usize == ordinal
        ));
    }
}

#[test]
fn inline_decoded_span_map_tracks_escape_expansions_exactly() {
    let project = project("inline-spans");
    create_dir_all(project.0.join("app")).expect("application directory");
    let query_source = concat!(
        "module app.escaped\n",
        "import pkg.db\n",
        "pub Params { id: i64 }\n",
        "pub Row { id: i64, icon: str }\n",
        "pub fn query() -> pkg.db.query<Params, Row> = ",
        "pkg.db.query(\"SELECT :id AS id, '\\u{1f600}' AS icon\", [])\n",
    );
    write(project.0.join("app/escaped.align"), query_source).expect("escaped Query");
    let entry = "module main\nimport app.escaped\nfn main() -> i32 = 0\n";
    let entry_path = project.0.join("main.align");
    write(&entry_path, entry).expect("entry");
    let mut source_map = SourceMap::new();
    let checked = check(
        &mut source_map,
        entry_path.to_str().expect("UTF-8 entry path"),
        entry,
    );
    assert!(!checked.diags.has_errors());
    let resolved = resolve_static_descriptors(
        &project.0,
        &mut source_map,
        &checked.static_descriptors,
        align_interface::Hash128::of(b"pkg-db-q1-inline-span"),
    )
    .expect("resolved inline SQL");
    let built =
        build_static_artifacts(&checked.static_descriptors, &resolved).expect("inline artifact");
    let StaticArtifact::Query(query) = &built[0].artifact else {
        panic!("escaped descriptor must be a Query")
    };
    assert_eq!(
        std::str::from_utf8(&query.source_sql).unwrap(),
        "SELECT :id AS id, '😀' AS icon"
    );
    let escape = query
        .decoded_span_map
        .iter()
        .find(|entry| {
            &query.source_sql[entry.decoded_span.start as usize..entry.decoded_span.end as usize]
                == "😀".as_bytes()
        })
        .expect("unicode escape run");
    assert_eq!(
        &query_source.as_bytes()
            [escape.defining_file_span.start as usize..escape.defining_file_span.end as usize],
        b"\\u{1f600}"
    );
    let mut cursor = 0u32;
    for entry in &query.decoded_span_map {
        assert_eq!(entry.decoded_span.start, cursor);
        cursor = entry.decoded_span.end;
    }
    assert_eq!(cursor as usize, query.source_sql.len());
}

#[test]
fn inline_nul_diagnostic_points_at_the_exact_source_bytes() {
    fn assert_span(label: &str, nul_source: &str, expected_source: &[u8]) {
        let project = project(label);
        create_dir_all(project.0.join("app")).expect("application directory");
        let query_source = format!(
            concat!(
                "module app.nul\n",
                "import pkg.db\n",
                "pub Params {{ id: i64 }}\n",
                "pub Row {{ id: i64 }}\n",
                "pub fn query() -> pkg.db.query<Params, Row> = ",
                "pkg.db.query(\"SELECT :id AS id{}\", [])\n",
            ),
            nul_source
        );
        write(project.0.join("app/nul.align"), &query_source).expect("NUL Query");
        let entry = "module main\nimport app.nul\nfn main() -> i32 = 0\n";
        let entry_path = project.0.join("main.align");
        write(&entry_path, entry).expect("entry");
        let mut source_map = SourceMap::new();
        let checked = check(
            &mut source_map,
            entry_path.to_str().expect("UTF-8 entry path"),
            entry,
        );
        assert!(!checked.diags.has_errors());
        let error = resolve_static_descriptors(
            &project.0,
            &mut source_map,
            &checked.static_descriptors,
            align_interface::Hash128::of(label.as_bytes()),
        )
        .expect_err("decoded inline NUL must be rejected");
        assert!(matches!(
            error.cause,
            StaticDescriptorInputErrorCause::Input(StaticInputError::EmbeddedNul {
                offset: 16,
                ..
            })
        ));
        let defining_source = &source_map.files()[error.span.file as usize].src;
        assert_eq!(
            &defining_source.as_bytes()[error.span.lo as usize..error.span.hi as usize],
            expected_source
        );
    }

    assert_span("inline-nul-escape", "\\0", b"\\0");
    assert_span("inline-nul-raw", "\0", b"\0");
}

#[test]
fn generated_runtime_data_is_producer_owned() {
    let project = project("generated-data");
    let (entry_path, entry) = write_sources(&project.0);
    let mut source_map = SourceMap::new();
    let walk = build_per_unit(
        &mut source_map,
        entry_path.to_str().expect("UTF-8 entry path"),
        &entry,
    );
    assert!(!walk.diags.has_errors());
    for unit in walk
        .units
        .iter()
        .filter(|unit| !unit.static_descriptors.is_empty())
    {
        assert!(
            !unit.mir.fns.iter().any(|function| [
                "pkg.db$query_file$",
                "pkg.db$query$",
                "pkg.db$command_file$",
                "pkg.db$command$",
                "pkg.db.sqlite$query_file$",
                "pkg.db.sqlite$query$",
                "pkg.db.sqlite$command_file$",
                "pkg.db.sqlite$command$",
                "pkg.db.postgres$query_file$",
                "pkg.db.postgres$query$",
                "pkg.db.postgres$command_file$",
                "pkg.db.postgres$command$",
            ]
            .iter()
            .any(|prefix| function.name.as_str().starts_with(prefix))),
            "consumer-side static constructor monomorph survived in {}",
            unit.unit
        );
        let llvm = emit_llvm_ir(&unit.mir, BuildTarget::Baseline, false, &[], false)
            .expect("descriptor LLVM");
        for descriptor in &unit.static_descriptors {
            let symbol = format!("{}${}", descriptor.unit, descriptor.item);
            let function = unit
                .mir
                .fns
                .iter()
                .find(|function| function.name.as_str() == symbol)
                .expect("generated descriptor function");
            assert!(function.params.is_empty());
            assert_eq!(function.blocks.len(), 1);
            let statements = &function.blocks[0].stmts;
            let artifact = unit
                .static_artifacts
                .iter()
                .find(|artifact| artifact.descriptor_id == descriptor.descriptor_id)
                .expect("producer artifact");
            let Some(align_mir::Stmt::Let(0, align_mir::Rvalue::StaticData(data))) =
                statements.first()
            else {
                panic!("descriptor constructor must start with relocation-bearing static data");
            };
            assert_eq!(data.bytes.len(), 104);
            assert_eq!(data.align, 8);
            assert_eq!(&data.bytes[0..4], &2u32.to_le_bytes());
            assert_eq!(data.bytes[4], u8::from(descriptor.consumer == StaticDescriptorConsumer::Command));
            assert_eq!(&data.bytes[6..8], &[0, 0]);
            let q1 = data
                .relocations
                .iter()
                .find(|relocation| relocation.offset == 8)
                .expect("Q1 plan relocation");
            assert!(matches!(
                &q1.target,
                align_mir::StaticDataTarget::Bytes {
                    bytes,
                    nul_terminated: false,
                } if bytes == artifact.runtime.bytes()
            ));
            let thunk_offsets = data
                .relocations
                .iter()
                .filter_map(|relocation| matches!(
                    relocation.target,
                    align_mir::StaticDataTarget::Function(_)
                ).then_some(relocation.offset))
                .collect::<Vec<_>>();
            if descriptor.consumer == StaticDescriptorConsumer::Query {
                assert_eq!(thunk_offsets, vec![64, 72, 80, 88, 96]);
                let materializer = data
                    .relocations
                    .iter()
                    .find(|relocation| relocation.offset == 96)
                    .expect("D12 QueryMeta materializer relocation");
                assert!(matches!(
                    &materializer.target,
                    align_mir::StaticDataTarget::Function(name)
                        if name.as_str().ends_with("$query_meta_v1")
                ));
            } else {
                assert_eq!(thunk_offsets, vec![64, 72]);
                assert_eq!(&data.bytes[96..104], &[0; 8]);
            }
            assert!(matches!(
                statements.get(1),
                Some(align_mir::Stmt::StoreField(
                    0,
                    path,
                    align_mir::Operand::Value(0)
                )) if path == &[0]
            ));
            assert!(matches!(
                statements.get(2),
                Some(align_mir::Stmt::Let(1, align_mir::Rvalue::Load(0)))
            ));
            assert!(!statements.iter().any(|statement| matches!(
                statement,
                align_mir::Stmt::Let(_, align_mir::Rvalue::Call(..))
            )));
            let binder = unit
                .mir
                .fns
                .iter()
                .find(|function| function.name.as_str() == format!("{symbol}$static_bind_v1"))
                .expect("generated binder");
            assert_eq!(
                binder.param_modes,
                vec![align_ast::ParamMode::ByValue, align_ast::ParamMode::Borrow]
            );
            let static_validator = unit
                .mir
                .fns
                .iter()
                .find(|function| function.name.as_str() == format!("{symbol}$static_validate_v1"))
                .expect("generated static validator");
            if descriptor.descriptor_id == "app.users.query" {
                assert!(matches!(
                    static_validator.blocks.as_slice(),
                    [align_mir::Block {
                        term: align_mir::Term::Return(Some(align_mir::Operand::Const(
                            align_mir::Const::Int(-1, _)
                        ))),
                        ..
                    }]
                ));
            }
            let magic = match &artifact.runtime {
                GeneratedStaticRuntime::Query(_) => "ALIGNQST",
                GeneratedStaticRuntime::Command(_) => "ALIGNCST",
            };
            assert!(
                llvm.contains(&format!(
                    "[{} x i8] c\"{magic}",
                    artifact.runtime.bytes().len()
                )),
                "generated descriptor bytes are absent from LLVM for {}:\n{llvm}",
                descriptor.descriptor_id
            );
        }
    }
}

#[test]
fn typed_descriptor_contract_matrix() {
    let valid_project = project("typed-contract-valid");
    let (entry_path, entry) = write_sources(&valid_project.0);
    let mut source_map = SourceMap::new();
    let checked = check(
        &mut source_map,
        entry_path.to_str().expect("UTF-8 entry path"),
        &entry,
    );
    assert!(!checked.diags.has_errors());
    let query = checked
        .static_descriptors
        .iter()
        .find(|descriptor| descriptor.descriptor_id == "app.users.query")
        .expect("typed Query descriptor");
    assert!(query.row_ty.is_some());
    assert!(query.row_contract.is_some());
    assert_ne!(query.params_ty, query.row_ty.expect("Query Row type"));
    let command = checked
        .static_descriptors
        .iter()
        .find(|descriptor| descriptor.descriptor_id == "app.prune.command")
        .expect("typed command descriptor");
    assert!(command.row_ty.is_none());
    assert!(command.row_contract.is_none());

    let cases = [
        (
            "wrong-kind",
            concat!(
                "pub Params { id: i64 }\n",
                "pub Row { id: i64 }\n",
                "pub fn bad() -> pkg.db.command<Params> = ",
                "pkg.db.query(\"SELECT :id AS id\", [])\n",
            ),
            "type mismatch",
        ),
        (
            "command-row",
            concat!(
                "pub Params { id: i64 }\n",
                "pub Row { id: i64 }\n",
                "pub fn bad() -> pkg.db.command<Params, Row> = ",
                "pkg.db.command(\"DELETE FROM t WHERE id = :id\", [])\n",
            ),
            "takes 1 type argument(s), got 2",
        ),
        (
            "unresolved",
            concat!(
                "pub Row { id: i64 }\n",
                "pub fn bad() -> pkg.db.query<Missing, Row> = ",
                "pkg.db.query(\"SELECT 1 AS id\", [])\n",
            ),
            "unknown type: 'Missing'",
        ),
        (
            "runtime-options",
            concat!(
                "pub Params { id: i64 }\n",
                "pub Row { id: i64 }\n",
                "fn options() -> slice<pkg.db.QueryOption> = []\n",
                "pub fn bad() -> pkg.db.query<Params, Row> = ",
                "pkg.db.query(\"SELECT :id AS id\", options())\n",
            ),
            "explicit option-list literal",
        ),
        (
            "constructor-arity",
            concat!(
                "pub Params { id: i64 }\n",
                "pub Row { id: i64 }\n",
                "pub fn bad() -> pkg.db.query<Params, Row> = ",
                "pkg.db.query(\"SELECT :id AS id\")\n",
            ),
            "expects 2 argument(s), got 1",
        ),
    ];
    for (label, body, expected) in cases {
        let project = project(&format!("typed-contract-{label}"));
        create_dir_all(project.0.join("app")).expect("application directory");
        let source = format!("module app.invalid\nimport pkg.db\n{body}");
        write(project.0.join("app/invalid.align"), source).expect("invalid descriptor module");
        let entry = "module main\nimport app.invalid\nfn main() -> i32 = 0\n";
        let entry_path = project.0.join("main.align");
        write(&entry_path, entry).expect("entry");
        let mut source_map = SourceMap::new();
        let checked = check(
            &mut source_map,
            entry_path.to_str().expect("UTF-8 entry path"),
            entry,
        );
        assert!(checked.diags.has_errors(), "invalid case {label} must fail");
        assert!(
            checked
                .diags
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "missing `{expected}` in case {label}: {:?}",
            checked.diags.iter().collect::<Vec<_>>()
        );
        assert!(
            !checked
                .static_descriptors
                .iter()
                .any(|descriptor| descriptor.descriptor_id == "app.invalid.bad"),
            "invalid case {label} published a descriptor"
        );
    }
}

#[test]
fn interface_impl_cache_invalidation_matrix() {
    fn build(root: &Path, entry_path: &Path, entry: &str) -> align_driver::PerUnitWalk {
        let mut source_map = SourceMap::new();
        let walk = build_per_unit(
            &mut source_map,
            entry_path.to_str().expect("UTF-8 entry path"),
            entry,
        );
        assert!(
            !walk.diags.has_errors(),
            "{} diagnostics: {:?}",
            root.display(),
            walk.diags.iter().collect::<Vec<_>>()
        );
        walk
    }

    fn identities(
        walk: &align_driver::PerUnitWalk,
        unit: &str,
        descriptor: &str,
    ) -> (
        align_interface::Hash128,
        align_interface::Hash128,
        align_interface::Hash128,
    ) {
        let unit = walk
            .units
            .iter()
            .find(|candidate| candidate.unit == unit)
            .expect("unit artifact");
        let artifact = unit
            .static_artifacts
            .iter()
            .find(|artifact| artifact.descriptor_id == descriptor)
            .expect("static artifact");
        (
            unit.summary.interface_hash,
            unit.summary.impl_hash,
            artifact.digest,
        )
    }

    let project = project("identity");
    let (entry_path, entry) = write_sources(&project.0);
    let baseline = build(&project.0, &entry_path, &entry);
    let baseline_users = identities(&baseline, "app.users", "app.users.query");
    let baseline_main_deps = baseline
        .units
        .iter()
        .find(|unit| unit.unit == "main")
        .expect("main unit")
        .dep_interface_hashes
        .clone();

    let users_path = project.0.join("app/users.align");
    let users = std::fs::read_to_string(&users_path).expect("users source");
    write(
        &users_path,
        users.replace("'name' AS name", "'renamed' AS name"),
    )
    .expect("SQL-only edit");
    let sql_edit = build(&project.0, &entry_path, &entry);
    let sql_edit_users = identities(&sql_edit, "app.users", "app.users.query");
    assert_eq!(baseline_users.0, sql_edit_users.0);
    assert_ne!(baseline_users.1, sql_edit_users.1);
    assert_ne!(baseline_users.2, sql_edit_users.2);
    assert_eq!(
        baseline_main_deps,
        sql_edit
            .units
            .iter()
            .find(|unit| unit.unit == "main")
            .expect("main unit after SQL edit")
            .dep_interface_hashes
    );

    let users = std::fs::read_to_string(&users_path).expect("SQL-edited users source");
    write(
        &users_path,
        users.replace("Params { id: i64 }", "Params { id: i32 }"),
    )
    .expect("public Params edit");
    let contract_edit = build(&project.0, &entry_path, &entry);
    let contract_edit_users = identities(&contract_edit, "app.users", "app.users.query");
    assert_ne!(sql_edit_users.0, contract_edit_users.0);
    assert_ne!(sql_edit_users.2, contract_edit_users.2);

    let users = std::fs::read_to_string(&users_path).expect("Params-edited users source");
    write(
        &users_path,
        users.replace(
            "WHERE :id = :id\", [])",
            "WHERE :id = :id\", [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedOptional)])",
        ),
    )
    .expect("static option edit");
    let option_edit = build(&project.0, &entry_path, &entry);
    let option_edit_users = identities(&option_edit, "app.users", "app.users.query");
    assert_ne!(contract_edit_users.0, option_edit_users.0);
    assert_ne!(contract_edit_users.1, option_edit_users.1);
    assert_ne!(contract_edit_users.2, option_edit_users.2);

    let users = std::fs::read_to_string(&users_path).expect("option-edited users source");
    write(
        &users_path,
        users.replace("Row { id: i64, name: str }", "Row { name: str, id: i64 }"),
    )
    .expect("public Row order edit");
    let row_edit = build(&project.0, &entry_path, &entry);
    let row_edit_users = identities(&row_edit, "app.users", "app.users.query");
    assert_ne!(option_edit_users.0, row_edit_users.0);
    assert_ne!(option_edit_users.2, row_edit_users.2);

    let users = std::fs::read_to_string(&users_path).expect("Row-edited users source");
    write(
        &users_path,
        users
            .replace("import pkg.db\n", "import pkg.db\nimport pkg.db.sqlite\n")
            .replace("pkg.db.query(\"SELECT", "pkg.db.sqlite.query(\"SELECT")
            .replace(
                "pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedOptional)])",
                "pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedOptional)], [])",
            ),
    )
    .expect("driver restriction edit");
    let restriction_edit = build(&project.0, &entry_path, &entry);
    let restriction_edit_users = identities(&restriction_edit, "app.users", "app.users.query");
    assert_ne!(row_edit_users.0, restriction_edit_users.0);
    assert_ne!(row_edit_users.2, restriction_edit_users.2);
}

#[test]
fn static_option_rejection_matrix() {
    let project = project("options");
    create_dir_all(project.0.join("app")).expect("application directory");
    let options_path = project.0.join("app/options.align");
    let valid = concat!(
        "module app.options\n",
        "import pkg.db\n",
        "import pkg.db.sqlite\n",
        "import pkg.db.postgres\n",
        "pub Params { id: i64 }\n",
        "pub Row { id: i64 }\n",
        "pub fn lookup() -> pkg.db.query<Params, Row> = ",
        "pkg.db.sqlite.query(\"SELECT :id AS id\", ",
        "[pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedOptional)], ",
        "[pkg.db.sqlite.QueryOption.RequireVersionAtLeast(3, 40, 0)])\n",
        "pub fn touch() -> pkg.db.command<Params> = ",
        "pkg.db.postgres.command(\"UPDATE t SET seen = 1 WHERE id = :id\", [], ",
        "[pkg.db.postgres.CommandOption.ParameterType(\"id\", \"int8\")])\n",
    );
    write(&options_path, valid).expect("valid options module");
    let entry = "module main\nimport app.options\nfn main() -> i32 = 0\n";
    let entry_path = project.0.join("main.align");
    write(&entry_path, entry).expect("entry");
    let mut source_map = SourceMap::new();
    let checked = check(
        &mut source_map,
        entry_path.to_str().expect("UTF-8 entry path"),
        entry,
    );
    assert!(
        !checked.diags.has_errors(),
        "valid option diagnostics: {:?}",
        checked.diags.iter().collect::<Vec<_>>()
    );
    assert_eq!(checked.static_descriptors.len(), 2);
    assert!(checked.static_descriptors.iter().any(|descriptor| {
        descriptor.static_options
            == [
                align_sema::StaticDescriptorOption::Check(
                    align_sema::StaticCheckPolicy::CheckedOptional,
                ),
                align_sema::StaticDescriptorOption::SQLiteRequireVersionAtLeast {
                    major: 3,
                    minor: 40,
                    patch: 0,
                },
            ]
    }));
    let resolved = resolve_static_descriptors(
        &project.0,
        &mut source_map,
        &checked.static_descriptors,
        align_interface::Hash128::of(b"pkg-db-q1-options"),
    )
    .expect("resolved option descriptors");
    let artifacts =
        build_static_artifacts(&checked.static_descriptors, &resolved).expect("option artifacts");
    for artifact in &artifacts {
        match (&artifact.artifact, &artifact.runtime) {
            (StaticArtifact::Query(query), GeneratedStaticRuntime::Query(runtime)) => {
                assert_eq!(runtime.static_options, query.static_options);
                assert!(runtime.static_options.iter().any(|option| matches!(
                    option.value,
                    align_interface::StaticOptionValue::SQLiteRequireVersionAtLeast { .. }
                )));
            }
            (StaticArtifact::Command(command), GeneratedStaticRuntime::Command(runtime)) => {
                assert_eq!(runtime.static_options, command.static_options);
                assert!(runtime.static_options.iter().any(|option| matches!(
                    option.value,
                    align_interface::StaticOptionValue::PostgreSQLParameterType { .. }
                )));
            }
            _ => panic!("artifact/runtime kind mismatch"),
        }
    }

    let invalid = valid.replace(
        "[pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedOptional)]",
        concat!(
            "[pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedOptional), ",
            "pkg.db.QueryOption.Check(pkg.db.CheckPolicy.DeclaredOnly)]"
        ),
    );
    write(&options_path, invalid).expect("duplicate Check option");
    let mut source_map = SourceMap::new();
    let checked = check(
        &mut source_map,
        entry_path.to_str().expect("UTF-8 entry path"),
        entry,
    );
    assert!(checked.diags.has_errors());
    assert!(checked
        .diags
        .iter()
        .any(|diagnostic| diagnostic.message.contains("common Check option only once")));
    assert!(!checked
        .static_descriptors
        .iter()
        .any(|descriptor| descriptor.descriptor_id == "app.options.lookup"));

    let duplicate_native_cases = [
        (
            valid.replace(
                "[pkg.db.sqlite.QueryOption.RequireVersionAtLeast(3, 40, 0)]",
                concat!(
                    "[pkg.db.sqlite.QueryOption.RequireVersionAtLeast(3, 40, 0), ",
                    "pkg.db.sqlite.QueryOption.RequireVersionAtLeast(3, 41, 0)]"
                ),
            ),
            "SQLite RequireVersionAtLeast may be specified only once",
            "app.options.lookup",
        ),
        (
            valid.replace(
                "[pkg.db.postgres.CommandOption.ParameterType(\"id\", \"int8\")]",
                concat!(
                    "[pkg.db.postgres.CommandOption.ParameterType(\"id\", \"int8\"), ",
                    "pkg.db.postgres.CommandOption.ParameterType(\"id\", \"int4\")]"
                ),
            ),
            "PostgreSQL ParameterType for `id` may be specified only once",
            "app.options.touch",
        ),
    ];
    for (invalid, expected, descriptor_id) in duplicate_native_cases {
        write(&options_path, invalid).expect("duplicate native option");
        let mut source_map = SourceMap::new();
        let checked = check(
            &mut source_map,
            entry_path.to_str().expect("UTF-8 entry path"),
            entry,
        );
        assert!(checked.diags.has_errors());
        assert!(checked
            .diags
            .iter()
            .any(|diagnostic| diagnostic.message.contains(expected)));
        assert!(!checked
            .static_descriptors
            .iter()
            .any(|descriptor| descriptor.descriptor_id == descriptor_id));
    }
}

#[test]
fn malformed_static_query_matrix() {
    let cases = [
        (":id AS id", "? AS id", "cannot use `?` placeholders"),
        (
            "WHERE :id = :id",
            "WHERE :id = :id; SELECT 2",
            "exactly one statement",
        ),
        (
            ":id AS id, 'name' AS name WHERE :id = :id",
            ":other AS id, 'name' AS name WHERE :other = :other",
            "placeholders and Params fields must match exactly",
        ),
        (
            "Row { id: i64, name: str }",
            "Row { id: i64, name: string }",
            "unsupported static Row field type `string`",
        ),
    ];
    for (index, (from, to, expected)) in cases.into_iter().enumerate() {
        let project = project(&format!("malformed-{index}"));
        let (entry_path, entry) = write_sources(&project.0);
        let users_path = project.0.join("app/users.align");
        let users = std::fs::read_to_string(&users_path).expect("users source");
        assert!(users.contains(from), "fixture replacement source: {from}");
        write(&users_path, users.replacen(from, to, 1)).expect("malformed users source");
        let mut source_map = SourceMap::new();
        let walk = build_per_unit(
            &mut source_map,
            entry_path.to_str().expect("UTF-8 entry path"),
            &entry,
        );
        assert!(walk.diags.has_errors(), "malformed case {index} must fail");
        assert!(
            walk.diags
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "missing `{expected}` in case {index}: {:?}",
            walk.diags.iter().collect::<Vec<_>>()
        );
        assert!(!walk.units.iter().any(|unit| unit.unit == "app.users"));
    }
}

#[test]
fn fake_driver_query_and_command_end_to_end() {
    let project = project("fake-execution");
    let (entry_path, entry) = write_sources(&project.0);
    let mut source_map = SourceMap::new();
    let checked = check(
        &mut source_map,
        entry_path.to_str().expect("UTF-8 entry path"),
        &entry,
    );
    assert!(!checked.diags.has_errors());
    let resolved = resolve_static_descriptors(
        &project.0,
        &mut source_map,
        &checked.static_descriptors,
        align_interface::Hash128::of(b"pkg-db-q1-fake-whole"),
    )
    .expect("whole-program static inputs");
    let whole = build_static_artifacts(&checked.static_descriptors, &resolved)
        .expect("whole-program artifacts");

    let mut per_unit_sources = SourceMap::new();
    let per_unit = build_per_unit(
        &mut per_unit_sources,
        entry_path.to_str().expect("UTF-8 entry path"),
        &entry,
    );
    assert!(!per_unit.diags.has_errors());
    let per_unit = per_unit
        .units
        .iter()
        .flat_map(|unit| unit.static_artifacts.iter())
        .collect::<Vec<_>>();

    for artifacts in [whole.iter().collect::<Vec<_>>(), per_unit] {
        let query = artifacts
            .iter()
            .find(|artifact| artifact.descriptor_id == "app.users.query")
            .expect("query artifact");
        let executed = execute_fake_static(
            &query.runtime,
            Driver::PostgreSQL,
            &[FakeValue::Integer(7)],
            &[vec![
                FakeValue::Integer(7),
                FakeValue::Text(b"name".to_vec()),
            ]],
            FakeCardinality::ExactlyOne,
        )
        .expect("fake Query execution");
        assert_eq!(executed.kind, FakeStatementKind::Query);
        assert_eq!(executed.execution_count, 1);
        assert_eq!(executed.bound[0].params_field_ordinal, 0);
        assert_eq!(executed.bound[0].protocol_ordinal, 1);
        assert_eq!(executed.rows[0][1].row_field_ordinal, 1);
        assert_eq!(
            std::str::from_utf8(&executed.wire_sql).expect("wire SQL"),
            "SELECT $1 AS id, 'name' AS name WHERE $1 = $1"
        );
        assert_eq!(
            executed
                .query_meta
                .as_ref()
                .expect("QueryMeta")
                .columns
                .len(),
            2
        );

        let command = artifacts
            .iter()
            .find(|artifact| artifact.descriptor_id == "app.prune.command")
            .expect("command artifact");
        let executed = execute_fake_static(
            &command.runtime,
            Driver::SQLite,
            &[FakeValue::Integer(7)],
            &[],
            FakeCardinality::All,
        )
        .expect("fake command execution");
        assert_eq!(executed.kind, FakeStatementKind::Command);
        assert_eq!(executed.execution_count, 1);
        assert!(executed.rows.is_empty());
        assert!(executed.query_meta.is_none());

        assert!(matches!(
            execute_fake_static(
                &query.runtime,
                Driver::SQLite,
                &[FakeValue::Text(b"wrong".to_vec())],
                &[],
                FakeCardinality::ExactlyOne,
            ),
            Err(FakeExecutionError::ParameterType { ordinal: 0, .. })
        ));
        let mut malformed_runtime = query.runtime.clone();
        let GeneratedStaticRuntime::Query(runtime) = &mut malformed_runtime else {
            unreachable!("selected Query runtime")
        };
        runtime.drivers[0].binder.fields[0].params_field_ordinal = 1;
        assert_eq!(
            execute_fake_static(
                &malformed_runtime,
                Driver::SQLite,
                &[FakeValue::Integer(7)],
                &[],
                FakeCardinality::All,
            ),
            Err(FakeExecutionError::InvalidArtifact(
                "generated binder fields are not dense or have an invalid protocol ordinal"
            ))
        );

        let mut zero_protocol = query.runtime.clone();
        let GeneratedStaticRuntime::Query(runtime) = &mut zero_protocol else {
            unreachable!("selected Query runtime")
        };
        runtime.drivers[0].binder.fields[0].protocol_ordinal = 0;
        assert!(matches!(
            execute_fake_static(
                &zero_protocol,
                Driver::SQLite,
                &[FakeValue::Integer(7)],
                &[],
                FakeCardinality::All,
            ),
            Err(FakeExecutionError::InvalidArtifact(_))
        ));

        let mut restricted = query.runtime.clone();
        let GeneratedStaticRuntime::Query(runtime) = &mut restricted else {
            unreachable!("selected Query runtime")
        };
        runtime
            .drivers
            .retain(|driver| driver.driver == Driver::SQLite);
        assert_eq!(
            execute_fake_static(
                &restricted,
                Driver::PostgreSQL,
                &[FakeValue::Integer(7)],
                &[],
                FakeCardinality::All,
            ),
            Err(FakeExecutionError::DriverNotPermitted)
        );

        let mut malformed_decoder = query.runtime.clone();
        let GeneratedStaticRuntime::Query(runtime) = &mut malformed_decoder else {
            unreachable!("selected Query runtime")
        };
        runtime.decoder.fields[0].row_field_ordinal = 1;
        assert!(matches!(
            execute_fake_static(
                &malformed_decoder,
                Driver::SQLite,
                &[FakeValue::Integer(7)],
                &[vec![
                    FakeValue::Integer(7),
                    FakeValue::Text(b"name".to_vec())
                ]],
                FakeCardinality::All,
            ),
            Err(FakeExecutionError::InvalidArtifact(
                "generated decoder fields are not dense"
            ))
        ));
        assert!(matches!(
            execute_fake_static(
                &query.runtime,
                Driver::SQLite,
                &[FakeValue::Integer(7)],
                &[vec![FakeValue::Integer(7)]],
                FakeCardinality::All,
            ),
            Err(FakeExecutionError::RowWidth {
                row: 0,
                expected: 2,
                actual: 1
            })
        ));
        assert!(matches!(
            execute_fake_static(
                &query.runtime,
                Driver::SQLite,
                &[FakeValue::Integer(7)],
                &[vec![FakeValue::Integer(7), FakeValue::Integer(8)]],
                FakeCardinality::All,
            ),
            Err(FakeExecutionError::RowType {
                row: 0,
                ordinal: 1,
                ..
            })
        ));
        assert!(matches!(
            execute_fake_static(
                &query.runtime,
                Driver::SQLite,
                &[FakeValue::Integer(7)],
                &[],
                FakeCardinality::ExactlyOne,
            ),
            Err(FakeExecutionError::RowCount { actual: 0, .. })
        ));
        assert_eq!(
            execute_fake_static(
                &command.runtime,
                Driver::SQLite,
                &[FakeValue::Integer(7)],
                &[vec![FakeValue::Integer(1)]],
                FakeCardinality::All,
            ),
            Err(FakeExecutionError::CommandReturnedRows)
        );

        let file_query = artifacts
            .iter()
            .find(|artifact| artifact.descriptor_id == "app.find.query")
            .expect("file Query artifact");
        let file_query = execute_fake_static(
            &file_query.runtime,
            Driver::SQLite,
            &[FakeValue::Integer(9)],
            &[vec![FakeValue::Integer(9)]],
            FakeCardinality::ExactlyOne,
        )
        .expect("file Query execution");
        assert_eq!(file_query.execution_count, 1);
        assert_eq!(file_query.wire_sql, b"SELECT :id AS id\n");

        let inline_command = artifacts
            .iter()
            .find(|artifact| artifact.descriptor_id == "app.touch.command")
            .expect("inline command artifact");
        let inline_command = execute_fake_static(
            &inline_command.runtime,
            Driver::PostgreSQL,
            &[FakeValue::Integer(9)],
            &[],
            FakeCardinality::All,
        )
        .expect("inline command execution");
        assert_eq!(inline_command.execution_count, 1);
        assert_eq!(
            inline_command.wire_sql,
            b"UPDATE users SET touched = 1 WHERE id = $1"
        );
    }
}
