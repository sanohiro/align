//! pkg.db Q3/D3+D5 checked/offline metadata owners.

mod common;
use align_driver::db_prepare::{
    build_metadata_batch, encode_migration_catalog, postgres_schema_fingerprint,
    publish_metadata_batch, read_migration_catalog, sqlite_database_schema_fingerprint,
    sqlite_memory_schema_fingerprint, MetadataDescriber, MigrationEntry, NativeColumnDescription,
    NativeParameterDescription, NativeStatementDescription, PreparationEnvironment, PrepareError,
};
use align_driver::db_prepare_native::{PostgresDescriber, SqliteDescriber};
use align_driver::{
    build_per_unit, build_static_artifacts, check, lower_to_mir, resolve_static_descriptors,
    Driver, Hash128,
};
use align_interface::{DriverEntry, MetaNullability, StaticArtifact, VerificationState};
use align_span::SourceMap;
use common::Proj;

const DB: &str = include_str!("../../../apps/db/pkg/db.align");
const SQLITE: &str = include_str!("../../../apps/db/pkg/db/sqlite.align");
const POSTGRES: &str = include_str!("../../../apps/db/pkg/db/postgres.align");
const INTERNAL: &str = include_str!("../../../apps/db/pkg/db/internal.align");
const RESOURCE: &str = include_str!("../../../apps/db/pkg/db/internal/resource.align");
const DESCRIPTOR: &str = include_str!("../../../apps/db/pkg/db/internal/descriptor.align");
const INTERNAL_SQLITE: &str = include_str!("../../../apps/db/pkg/db/internal/sqlite.align");
const INTERNAL_POSTGRES: &str = include_str!("../../../apps/db/pkg/db/internal/postgres.align");

fn project(tag: &str) -> Proj {
    let main = "module main\nimport app.read\nimport app.write\nimport app.sqlite_table\nimport app.sqlite_command\nimport app.pg_read\nimport app.pg_command\nfn main() -> i32 = 0\n";
    let project = Proj::new(tag, &[("main.align", main)], "main.align");
    for directory in ["pkg/db/internal", "app"] {
        std::fs::create_dir_all(project.dir.join(directory)).expect("create package directory");
    }
    for (name, source) in [
        ("pkg/db.align", DB),
        ("pkg/db/sqlite.align", SQLITE),
        ("pkg/db/postgres.align", POSTGRES),
        ("pkg/db/internal.align", INTERNAL),
        ("pkg/db/internal/resource.align", RESOURCE),
        ("pkg/db/internal/descriptor.align", DESCRIPTOR),
        ("pkg/db/internal/sqlite.align", INTERNAL_SQLITE),
        ("pkg/db/internal/postgres.align", INTERNAL_POSTGRES),
        (
            "app/read.align",
            r#"module app.read
import pkg.db
import pkg.db.sqlite

pub Params { value: i64 }
pub Row { value: i64 }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.sqlite.query_file(
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [],
)
"#,
        ),
        ("app/read.sql", "SELECT CAST(:value AS BIGINT) AS value"),
        (
            "app/write.align",
            r#"module app.write
import pkg.db
import pkg.db.postgres

pub Params { value: i64 }

pub fn command() -> pkg.db.command<Params> = pkg.db.postgres.command(
  "DELETE FROM items WHERE value = :value",
  [pkg.db.CommandOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [],
)
"#,
        ),
        (
            "app/sqlite_table.align",
            r#"module app.sqlite_table
import pkg.db
import pkg.db.sqlite

pub Params { value: i64 }
pub Row { value: i64 }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.sqlite.query(
  "SELECT value AS value FROM items WHERE value = :value",
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [],
)
"#,
        ),
        (
            "app/pg_read.align",
            r#"module app.pg_read
import pkg.db
import pkg.db.postgres

pub Params { value: i64 }
pub Row { value: i64 }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:value AS BIGINT) AS value",
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [],
)
"#,
        ),
        (
            "app/sqlite_command.align",
            r#"module app.sqlite_command
import pkg.db
import pkg.db.sqlite

pub Params {}

pub fn command() -> pkg.db.command<Params> = pkg.db.sqlite.command(
  "CREATE TEMP TABLE align_q3_sqlite_probe(value INTEGER)",
  [pkg.db.CommandOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [],
)
"#,
        ),
        (
            "app/pg_command.align",
            r#"module app.pg_command
import pkg.db
import pkg.db.postgres

pub Params {}

pub fn command() -> pkg.db.command<Params> = pkg.db.postgres.command(
  "CREATE TEMP TABLE align_q3_postgres_probe(value BIGINT)",
  [pkg.db.CommandOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [],
)
"#,
        ),
    ] {
        std::fs::write(project.dir.join(name), source).expect("write package source");
    }
    project
}

struct FakeDescriber {
    driver: Driver,
    environment_calls: usize,
    describe_calls: Vec<String>,
}

impl FakeDescriber {
    fn new(driver: Driver) -> Self {
        Self {
            driver,
            environment_calls: 0,
            describe_calls: Vec::new(),
        }
    }
}

impl MetadataDescriber for FakeDescriber {
    fn driver(&self) -> Driver {
        self.driver
    }

    fn environment(&mut self) -> Result<PreparationEnvironment, PrepareError> {
        self.environment_calls += 1;
        Ok(PreparationEnvironment {
            engine_version: match self.driver {
                Driver::SQLite => "3.53.3",
                Driver::PostgreSQL => "16.4",
            }
            .to_string(),
            driver_version: match self.driver {
                Driver::SQLite => "libsqlite3 3.53.3",
                Driver::PostgreSQL => "libpq 18.4",
            }
            .to_string(),
            schema_fingerprint: Hash128 { lo: 7, hi: 11 },
            search_path: match self.driver {
                Driver::SQLite => Vec::new(),
                Driver::PostgreSQL => vec!["app".to_string(), "public".to_string()],
            },
            extensions: Vec::new(),
        })
    }

    fn describe(
        &mut self,
        artifact: &StaticArtifact,
        _entry: &DriverEntry,
    ) -> Result<NativeStatementDescription, PrepareError> {
        let (id, query) = match artifact {
            StaticArtifact::Query(query) => (query.query_id.as_str(), true),
            StaticArtifact::Command(command) => (command.command_id.as_str(), false),
        };
        self.describe_calls.push(id.to_string());
        let empty_params = matches!(id, "app.sqlite_command.command" | "app.pg_command.command");
        Ok(NativeStatementDescription {
            parameters: if empty_params {
                Vec::new()
            } else {
                vec![NativeParameterDescription {
                    ordinal: 1,
                    source_name: (self.driver == Driver::SQLite).then(|| "value".to_string()),
                    native_type: Some(
                        match self.driver {
                            Driver::SQLite => "INTEGER",
                            Driver::PostgreSQL => "bigint",
                        }
                        .to_string(),
                    ),
                    native_type_id: (self.driver == Driver::PostgreSQL).then_some(20),
                }]
            },
            columns: if query {
                vec![NativeColumnDescription {
                    ordinal: 0,
                    source_alias: "value".to_string(),
                    native_type: Some(
                        match self.driver {
                            Driver::SQLite => "INTEGER",
                            Driver::PostgreSQL => "bigint",
                        }
                        .to_string(),
                    ),
                    native_type_id: (self.driver == Driver::PostgreSQL).then_some(20),
                    origin_schema: None,
                    origin_table: None,
                    origin_column: None,
                    nullable: MetaNullability::Unknown,
                }]
            } else {
                Vec::new()
            },
        })
    }
}

fn checked_project(project: &Proj) -> (SourceMap, align_driver::Checked) {
    let entry = project.dir.join(&project.entry);
    let source = std::fs::read_to_string(&entry).expect("read entry");
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, &entry.display().to_string(), &source);
    assert!(
        !checked.diags.has_errors(),
        "unexpected diagnostics:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags)
    );
    (source_map, checked)
}

#[test]
fn regeneration_ignores_missing_required_metadata_and_is_deterministic() {
    let project = project("pkg-db-q3-regeneration");
    let entry = project.dir.join(&project.entry);
    let (mut source_map, checked) = checked_project(&project);
    let mut describer = FakeDescriber::new(Driver::SQLite);
    let first = build_metadata_batch(&mut source_map, &entry, &checked, &[], &mut describer)
        .expect("regeneration batch");
    assert_eq!(describer.environment_calls, 1);
    assert_eq!(
        describer.describe_calls,
        [
            "app.read.query",
            "app.sqlite_command.command",
            "app.sqlite_table.query",
        ]
    );
    assert_eq!(first.files.len(), 3);
    assert!(first.files.iter().all(|file| file.bytes.ends_with(b"}\n")));
    let nullable = b"\"nullable\":\"unknown\"";
    assert!(first.files[0]
        .bytes
        .windows(nullable.len())
        .any(|bytes| bytes == nullable));

    let (mut source_map, checked) = checked_project(&project);
    let mut second_describer = FakeDescriber::new(Driver::SQLite);
    let second = build_metadata_batch(
        &mut source_map,
        &entry,
        &checked,
        &[],
        &mut second_describer,
    )
    .expect("second regeneration batch");
    assert_eq!(first, second);
}

#[test]
fn checked_metadata_sqlite_query_and_postgres_command_goldens() {
    let project = project("pkg-db-q3-metadata-goldens");
    let entry = project.dir.join(&project.entry);
    let (mut source_map, checked) = checked_project(&project);
    let mut sqlite = FakeDescriber::new(Driver::SQLite);
    let sqlite = build_metadata_batch(
        &mut source_map,
        &entry,
        &checked,
        &["app.read.query".to_string()],
        &mut sqlite,
    )
    .expect("SQLite golden batch");
    let sqlite_bytes = include_bytes!("golden/checked_metadata_sqlite_query_v1.json");
    let sqlite_digest = include_str!("golden/checked_metadata_sqlite_query_v1.digest").trim();
    assert_eq!(sqlite.files[0].bytes, sqlite_bytes);
    assert_eq!(Hash128::of(sqlite_bytes).to_hex(), sqlite_digest);

    let (mut source_map, checked) = checked_project(&project);
    let mut postgres = FakeDescriber::new(Driver::PostgreSQL);
    let postgres = build_metadata_batch(
        &mut source_map,
        &entry,
        &checked,
        &["app.write.command".to_string()],
        &mut postgres,
    )
    .expect("PostgreSQL golden batch");
    let postgres_bytes = include_bytes!("golden/checked_metadata_postgres_command_v1.json");
    let postgres_digest = include_str!("golden/checked_metadata_postgres_command_v1.digest").trim();
    assert_eq!(postgres.files[0].bytes, postgres_bytes);
    assert_eq!(Hash128::of(postgres_bytes).to_hex(), postgres_digest);
}

#[test]
fn schema_identities_and_publication_are_exact_and_check_is_read_only() {
    assert_ne!(
        sqlite_memory_schema_fingerprint(None),
        sqlite_memory_schema_fingerprint(Some(Hash128 { lo: 1, hi: 2 }))
    );
    assert_eq!(
        sqlite_database_schema_fingerprint("schema-v1").expect("SQLite schema identity"),
        sqlite_database_schema_fingerprint("schema-v1").expect("stable SQLite schema identity")
    );
    assert!(sqlite_database_schema_fingerprint("bad\0id").is_err());
    assert!(postgres_schema_fingerprint(
        "schema-v1",
        &["app".to_string(), "public".to_string()],
        &[],
    )
    .is_ok());

    let project = project("pkg-db-q3-publication");
    let entry = project.dir.join(&project.entry);
    let (mut source_map, checked) = checked_project(&project);
    let mut describer = FakeDescriber::new(Driver::SQLite);
    let batch = build_metadata_batch(&mut source_map, &entry, &checked, &[], &mut describer)
        .expect("metadata batch");
    assert!(publish_metadata_batch(&batch, true).is_err());
    assert!(
        !project.dir.join(".align-db").exists(),
        "--check must not create the output directory"
    );

    let written = publish_metadata_batch(&batch, false).expect("publish metadata");
    assert_eq!(written.selected, 3);
    assert_eq!(written.changed, 3);
    let publication_lock = project.dir.join(".align-db/.publication.lock");
    assert!(std::fs::symlink_metadata(&publication_lock)
        .expect("publication lock")
        .is_file());
    let clean = publish_metadata_batch(&batch, true).expect("metadata is current");
    assert_eq!(clean.changed, 0);
    let path = &batch.files[0].path;
    std::fs::write(path, b"stale\n").expect("corrupt metadata");
    let stale = std::fs::read(path).expect("read stale metadata");
    assert!(publish_metadata_batch(&batch, true).is_err());
    assert_eq!(std::fs::read(path).expect("check leaves bytes"), stale);
    let repaired = publish_metadata_batch(&batch, false).expect("repair metadata");
    assert_eq!(repaired.changed, 1);
    assert_eq!(
        std::fs::read(path).expect("read repaired metadata"),
        batch.files[0].bytes
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let outside = project.dir.join("outside-metadata.json");
        std::fs::write(&outside, b"outside\n").expect("write symlink target");
        std::fs::remove_file(path).expect("remove metadata destination");
        symlink(&outside, path).expect("create metadata symlink");
        let error = publish_metadata_batch(&batch, false).expect_err("reject metadata symlink");
        assert!(error.to_string().contains("is a symlink"));
        assert_eq!(
            std::fs::read(&outside).expect("read symlink target"),
            b"outside\n"
        );
    }
}

#[test]
fn selection_rejects_unknown_and_duplicate_ids_before_native_open() {
    let project = project("pkg-db-q3-selection");
    let entry = project.dir.join(&project.entry);
    for selected in [
        vec!["app.missing.query".to_string()],
        vec!["app.read.query".to_string(), "app.read.query".to_string()],
        vec!["app.read.query".to_string()],
    ] {
        let (mut source_map, checked) = checked_project(&project);
        let mut describer = FakeDescriber::new(Driver::PostgreSQL);
        let error =
            build_metadata_batch(&mut source_map, &entry, &checked, &selected, &mut describer)
                .expect_err("invalid selection");
        assert_eq!(describer.environment_calls, 0, "{error}");
        assert!(describer.describe_calls.is_empty(), "{error}");
    }
}

#[test]
fn generated_metadata_is_consumed_offline_and_stale_required_evidence_fails() {
    let project = project("pkg-db-q3-offline");
    let entry = project.dir.join(&project.entry);
    for driver in [Driver::SQLite, Driver::PostgreSQL] {
        let (mut source_map, checked) = checked_project(&project);
        let mut describer = FakeDescriber::new(driver);
        let batch = build_metadata_batch(&mut source_map, &entry, &checked, &[], &mut describer)
            .expect("metadata regeneration");
        publish_metadata_batch(&batch, false).expect("publish regenerated metadata");
    }

    let build_offline = || {
        let (mut source_map, checked) = checked_project(&project);
        let resolution_digest = align_interface::codegen_impl_hash(&lower_to_mir(&checked.hir));
        let resolved = resolve_static_descriptors(
            &project.dir,
            &mut source_map,
            &checked.static_descriptors,
            resolution_digest,
        )
        .expect("resolve offline static inputs");
        build_static_artifacts(&checked.static_descriptors, &resolved)
    };
    let artifacts = build_offline().expect("consume current metadata offline");
    assert_eq!(artifacts.len(), 6);
    for artifact in &artifacts {
        let entries = match &artifact.artifact {
            StaticArtifact::Query(query) => &query.driver_entries,
            StaticArtifact::Command(command) => &command.driver_entries,
        };
        assert!(entries
            .iter()
            .all(|entry| { entry.checked_metadata.state == VerificationState::DatabaseChecked }));
    }

    let entry_source = std::fs::read_to_string(&entry).expect("read entry for per-unit build");
    let mut per_unit_sources = SourceMap::new();
    let per_unit = build_per_unit(
        &mut per_unit_sources,
        entry.to_str().expect("UTF-8 entry"),
        &entry_source,
    );
    assert!(
        !per_unit.diags.has_errors(),
        "per-unit checked metadata diagnostics: {}",
        align_driver::format_diagnostics(&per_unit_sources, &per_unit.diags)
    );
    let per_unit_artifacts = per_unit
        .units
        .iter()
        .flat_map(|unit| unit.static_artifacts.iter())
        .collect::<Vec<_>>();
    assert_eq!(per_unit_artifacts.len(), 6);
    for artifact in per_unit_artifacts {
        let entries = match &artifact.artifact {
            StaticArtifact::Query(query) => &query.driver_entries,
            StaticArtifact::Command(command) => &command.driver_entries,
        };
        assert!(entries
            .iter()
            .all(|entry| { entry.checked_metadata.state == VerificationState::DatabaseChecked }));
    }

    let read_path = project.dir.join("app/read.sql");
    let source = std::fs::read_to_string(&read_path).expect("read Query source");
    std::fs::write(
        &read_path,
        source.replace("CAST(:value AS BIGINT)", "CAST(:value + 1 AS BIGINT)"),
    )
    .expect("mutate Query source");
    let stale = build_offline().expect_err("required stale metadata must fail");
    assert!(stale.reason.contains("artifact inputs changed"), "{stale}");
}

#[test]
fn sqlite_native_prepare_describes_the_selected_query() {
    let project = project("pkg-db-q3-sqlite-native");
    let entry = project.dir.join(&project.entry);
    let (mut source_map, checked) = checked_project(&project);
    let schema = sqlite_memory_schema_fingerprint(None);
    let mut describer = SqliteDescriber::memory(schema);
    let batch = build_metadata_batch(
        &mut source_map,
        &entry,
        &checked,
        &[
            "app.read.query".to_string(),
            "app.sqlite_command.command".to_string(),
        ],
        &mut describer,
    )
    .expect("native SQLite metadata");
    assert_eq!(batch.files.len(), 2);
    let bytes = &batch
        .files
        .iter()
        .find(|file| file.descriptor_id == "app.read.query")
        .expect("SQLite Query metadata")
        .bytes;
    for needle in [
        b"\"driver\":\"sqlite\"".as_slice(),
        b"\"source_name\":\"value\"".as_slice(),
        b"\"source_alias\":\"value\"".as_slice(),
        b"\"nullable\":\"unknown\"".as_slice(),
    ] {
        assert!(bytes.windows(needle.len()).any(|window| window == needle));
    }
}

fn reference_migration_bytes(entries: &[(u32, &str, &[u8])]) -> Vec<u8> {
    let mut bytes = b"ALIGNMIG".to_vec();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(entries.len())
            .expect("fixture count")
            .to_le_bytes(),
    );
    for (version, filename, content) in entries {
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.extend_from_slice(
            &u32::try_from(filename.len())
                .expect("fixture filename")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(filename.as_bytes());
        let hash = Hash128::of(content);
        bytes.extend_from_slice(&hash.lo.to_le_bytes());
        bytes.extend_from_slice(&hash.hi.to_le_bytes());
    }
    bytes
}

fn reference_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(
        &u32::try_from(value.len())
            .expect("fixture string length")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(value.as_bytes());
}

fn reference_schema_identity(
    driver: Driver,
    source_tag: u8,
    schema_id: Option<&str>,
    catalog: Option<Option<Hash128>>,
    search_path: &[&str],
    extensions: &[(&str, &str, Option<&str>)],
) -> Vec<u8> {
    let mut bytes = b"ALIGNSID".to_vec();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(driver as u8);
    bytes.push(source_tag);
    if let Some(catalog) = catalog {
        match catalog {
            Some(hash) => {
                bytes.push(1);
                bytes.extend_from_slice(&hash.lo.to_le_bytes());
                bytes.extend_from_slice(&hash.hi.to_le_bytes());
            }
            None => bytes.push(0),
        }
    }
    if let Some(schema_id) = schema_id {
        reference_string(&mut bytes, schema_id);
    }
    if driver == Driver::PostgreSQL {
        bytes.extend_from_slice(
            &u32::try_from(search_path.len())
                .expect("fixture path count")
                .to_le_bytes(),
        );
        for path in search_path {
            reference_string(&mut bytes, path);
        }
        bytes.extend_from_slice(
            &u32::try_from(extensions.len())
                .expect("fixture extension count")
                .to_le_bytes(),
        );
        for (schema, name, version) in extensions {
            reference_string(&mut bytes, schema);
            reference_string(&mut bytes, name);
            match version {
                Some(version) => {
                    bytes.push(1);
                    reference_string(&mut bytes, version);
                }
                None => bytes.push(0),
            }
        }
    }
    bytes
}

#[test]
fn schema_identity_goldens_match_an_independent_encoder() {
    let migration_digest = Hash128 {
        lo: u64::from_str_radix("ffbfab28572cd6b2", 16).expect("fixture low hash"),
        hi: u64::from_str_radix("488eae1dcb753582", 16).expect("fixture high hash"),
    };
    let extensions = vec![
        align_driver::db_prepare::PreparationExtension {
            schema: "public".to_string(),
            name: "pg_trgm".to_string(),
            version: Some("1.6".to_string()),
        },
        align_driver::db_prepare::PreparationExtension {
            schema: "公".to_string(),
            name: "hstore".to_string(),
            version: None,
        },
    ];
    let fixtures = [
        (
            "schema_identity_sqlite_empty_v1",
            reference_schema_identity(Driver::SQLite, 0, None, Some(None), &[], &[]),
            sqlite_memory_schema_fingerprint(None),
            include_str!("golden/schema_identity_sqlite_empty_v1.hex"),
            include_str!("golden/schema_identity_sqlite_empty_v1.digest"),
        ),
        (
            "schema_identity_sqlite_migrations_v1",
            reference_schema_identity(
                Driver::SQLite,
                0,
                None,
                Some(Some(migration_digest)),
                &[],
                &[],
            ),
            sqlite_memory_schema_fingerprint(Some(migration_digest)),
            include_str!("golden/schema_identity_sqlite_migrations_v1.hex"),
            include_str!("golden/schema_identity_sqlite_migrations_v1.digest"),
        ),
        (
            "schema_identity_sqlite_database_v1",
            reference_schema_identity(Driver::SQLite, 1, Some("スキーマ-v1"), None, &[], &[]),
            sqlite_database_schema_fingerprint("スキーマ-v1").expect("SQLite database identity"),
            include_str!("golden/schema_identity_sqlite_database_v1.hex"),
            include_str!("golden/schema_identity_sqlite_database_v1.digest"),
        ),
        (
            "schema_identity_postgres_v1",
            reference_schema_identity(
                Driver::PostgreSQL,
                2,
                Some("スキーマ-v1"),
                None,
                &["app", "公"],
                &[("public", "pg_trgm", Some("1.6")), ("公", "hstore", None)],
            ),
            postgres_schema_fingerprint(
                "スキーマ-v1",
                &["app".to_string(), "公".to_string()],
                &extensions,
            )
            .expect("PostgreSQL identity"),
            include_str!("golden/schema_identity_postgres_v1.hex"),
            include_str!("golden/schema_identity_postgres_v1.digest"),
        ),
    ];
    for (name, bytes, production, expected_hex, expected_digest) in fixtures {
        assert_eq!(Hash128::of(&bytes), production, "{name}");
        let hex = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(hex, expected_hex.trim(), "{name}");
        assert_eq!(production.to_hex(), expected_digest.trim(), "{name}");
    }
}

#[test]
fn migration_catalog_validates_before_sqlite_open_and_applies_atomically() {
    let project = project("pkg-db-q3-migrations");
    let migrations = project.dir.join("migrations");
    std::fs::create_dir(&migrations).expect("create migrations");
    let first = b"CREATE TABLE items(value INTEGER NOT NULL);\n";
    let second = "INSERT INTO items(value) VALUES (1); -- 日本語\n".as_bytes();
    std::fs::write(migrations.join("0002_seed.sql"), second).expect("write second migration");
    std::fs::write(migrations.join("0001_create_items.sql"), first).expect("write first migration");
    let catalog = read_migration_catalog(&migrations).expect("read migration catalog");
    assert_eq!(
        catalog
            .entries
            .iter()
            .map(|entry| entry.version)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(
        catalog.encoded,
        reference_migration_bytes(&[
            (1, "0001_create_items.sql", first),
            (2, "0002_seed.sql", second),
        ])
    );
    assert_eq!(catalog.fingerprint, Hash128::of(&catalog.encoded));
    let hex = |bytes: &[u8]| {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        hex(&catalog.encoded),
        include_str!("golden/migration_catalog_nonempty_v1.hex").trim()
    );
    assert_eq!(
        catalog.fingerprint.to_hex(),
        include_str!("golden/migration_catalog_nonempty_v1.digest").trim()
    );

    let entry = project.dir.join(&project.entry);
    let (mut source_map, checked) = checked_project(&project);
    let mut describer = SqliteDescriber::memory_with_migrations(catalog);
    let batch = build_metadata_batch(
        &mut source_map,
        &entry,
        &checked,
        &["app.sqlite_table.query".to_string()],
        &mut describer,
    )
    .expect("describe migrated SQLite schema");
    assert_eq!(batch.files.len(), 1);
    assert!(batch.files[0]
        .bytes
        .windows(b"\"origin_table\":\"items\"".len())
        .any(|window| window == b"\"origin_table\":\"items\""));

    std::fs::remove_file(migrations.join("0002_seed.sql")).expect("remove second migration");
    std::fs::write(migrations.join("0003_gap.sql"), b"SELECT 1;").expect("write gap migration");
    assert!(read_migration_catalog(&migrations)
        .unwrap_err()
        .to_string()
        .contains("expected 0002"));

    let empty =
        encode_migration_catalog(Vec::<MigrationEntry>::new()).expect("encode empty catalog");
    assert_eq!(empty.encoded, reference_migration_bytes(&[]));
    assert_eq!(
        hex(&empty.encoded),
        include_str!("golden/migration_catalog_empty_v1.hex").trim()
    );
    assert_eq!(
        empty.fingerprint.to_hex(),
        include_str!("golden/migration_catalog_empty_v1.digest").trim()
    );
}

#[test]
fn migration_catalog_rejects_invalid_names_and_symlinks() {
    let project = project("pkg-db-q3-migration-invalid");
    let migrations = project.dir.join("migrations");
    std::fs::create_dir(&migrations).expect("create migrations");
    let invalid = migrations.join("0001_Create.sql");
    std::fs::write(&invalid, b"SELECT 1;").expect("write invalid migration");
    assert!(read_migration_catalog(&migrations)
        .unwrap_err()
        .to_string()
        .contains("filename"));
    std::fs::remove_file(&invalid).expect("remove invalid migration");

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::symlink;

        let target = migrations.join("target.txt");
        std::fs::write(&target, b"SELECT 1;").expect("write symlink target");
        let link = migrations.join("0001_link.sql");
        symlink(&target, &link).expect("create migration symlink");
        assert!(read_migration_catalog(&migrations)
            .unwrap_err()
            .to_string()
            .contains("symlink"));
        std::fs::remove_file(&link).expect("remove symlink");
        let non_utf8 = migrations.join(std::ffi::OsString::from_vec(vec![
            0xff, b'.', b's', b'q', b'l',
        ]));
        if std::fs::write(&non_utf8, b"SELECT 1;").is_ok() {
            assert!(read_migration_catalog(&migrations)
                .unwrap_err()
                .to_string()
                .contains("non-UTF-8"));
        }
    }
}

#[test]
fn prepare_cli_input_and_precedence_matrix() {
    let project = project("pkg-db-q3-cli");
    let entry = project.dir.join(&project.entry);
    let missing_entry = project.dir.join("missing.align");
    for (arguments, expected) in [
        (
            vec!["--driver", "sqlite", "--database", "db.sqlite"],
            "SQLite requires exactly",
        ),
        (
            vec![
                "--driver",
                "postgres",
                "--url-env",
                "Q3_MUST_NOT_BE_READ",
                "--schema-id",
                "v1",
                "--memory",
            ],
            "valid only for SQLite",
        ),
        (
            vec![
                "--driver",
                "sqlite",
                "--memory",
                "--query",
                "app.read.query",
                "--query",
                "app.read.query",
            ],
            "duplicate --query",
        ),
        (
            vec!["--driver", "sqlite", "--memory", "--unknown", "value"],
            "unknown `db prepare` option",
        ),
        (
            vec![
                "--driver",
                "postgres",
                "--url-env",
                "PGHOSTADDR",
                "--schema-id",
                "v1",
            ],
            "must not begin with `PG`",
        ),
        (vec!["--memory"], "requires --driver"),
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
            .args([
                "db",
                "prepare",
                missing_entry.to_str().expect("UTF-8 missing entry"),
            ])
            .args(arguments)
            .output()
            .expect("run invalid db prepare");
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected),
            "expected `{expected}` in `{stderr}`"
        );
    }
    assert!(!project.dir.join(".align-db").exists());

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;

        let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
            .args([
                std::ffi::OsString::from("db"),
                std::ffi::OsString::from("prepare"),
                missing_entry.as_os_str().to_owned(),
                std::ffi::OsString::from("--query"),
                std::ffi::OsString::from_vec(vec![0xff]),
            ])
            .output()
            .expect("run non-UTF-8 db prepare");
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("must be UTF-8"));
    }

    let command = |check_only: bool| {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"));
        command.args([
            "db",
            "prepare",
            entry.to_str().expect("UTF-8 entry"),
            "--driver",
            "sqlite",
            "--memory",
            "--query",
            "app.read.query",
        ]);
        if check_only {
            command.arg("--check");
        }
        command.output().expect("run alignc db prepare")
    };
    let written = command(false);
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    assert!(String::from_utf8_lossy(&written.stdout).contains("selected=1 changed=1"));
    let checked = command(true);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(String::from_utf8_lossy(&checked.stdout).contains("selected=1 changed=0"));
}

#[test]
fn postgres_rejects_ambient_connection_defaults_before_native_load() {
    for url in [
        "postgresql:///align",
        "postgresql://align:align@127.0.0.1/align",
        "postgresql://align:align@127.0.0.1:5432/align?host=elsewhere",
        "postgresql://align:align@127.0.0.1:5432/align?target_session_attrs=primary",
        "postgresql://align:align@host%2Chost:5432/align",
        "host=127.0.0.1 dbname=align user=align password=align",
    ] {
        let mut describer = PostgresDescriber::new(url.to_string(), "q3-test-v1".to_string());
        let error = describer
            .environment()
            .expect_err("reject incomplete or overriding PostgreSQL URL");
        assert!(
            error.to_string().contains("PostgreSQL preparation")
                && error.to_string().contains("URL"),
            "{url}: {error}"
        );
    }

    let project = project("pkg-db-q3-postgres-ambient");
    let entry = project.dir.join(&project.entry);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
        .args([
            "db",
            "prepare",
            entry.to_str().expect("UTF-8 entry"),
            "--driver",
            "postgres",
            "--url-env",
            "ALIGN_DB_Q3_AMBIENT_URL",
            "--schema-id",
            "v1",
            "--query",
            "app.pg_read.query",
        ])
        .env(
            "ALIGN_DB_Q3_AMBIENT_URL",
            "postgresql://align:align@127.0.0.1:5432/align",
        )
        .env("PGHOSTADDR", "127.0.0.2")
        .output()
        .expect("run ambient PostgreSQL rejection");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("rejects ambient PG* environment variables"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn prepare_rejects_unsupported_static_options_before_native_work() {
    let project = project("pkg-db-q3-postgres-option-validation");
    std::fs::write(
        project.dir.join("app/pg_read.align"),
        r#"module app.pg_read
import pkg.db
import pkg.db.postgres

pub Params { value: i64 }
pub Row { value: i64 }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT :value AS value",
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [pkg.db.postgres.QueryOption.ParameterType("value", "int4")],
)
"#,
    )
    .expect("write unsupported PostgreSQL option fixture");
    let entry = project.dir.join(&project.entry);
    let (mut source_map, checked) = checked_project(&project);
    let mut describer = FakeDescriber::new(Driver::PostgreSQL);
    let error = build_metadata_batch(
        &mut source_map,
        &entry,
        &checked,
        &["app.pg_read.query".to_string()],
        &mut describer,
    )
    .expect_err("unsupported parameter type must fail before native work");
    assert_eq!(describer.environment_calls, 0, "{error}");
    assert!(
        error
            .to_string()
            .contains("unsupported PostgreSQL parameter type `int4`"),
        "{error}"
    );
}

#[test]
fn sqlite_prepare_enforces_static_version_options() {
    let project = project("pkg-db-q3-sqlite-version-option");
    std::fs::write(
        project.dir.join("app/read.align"),
        r#"module app.read
import pkg.db
import pkg.db.sqlite

pub Params { value: i64 }
pub Row { value: i64 }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.sqlite.query_file(
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [pkg.db.sqlite.QueryOption.RequireVersionAtLeast(4294967295, 0, 0)],
)
"#,
    )
    .expect("write SQLite version option fixture");
    let entry = project.dir.join(&project.entry);
    let (mut source_map, checked) = checked_project(&project);
    let mut describer = SqliteDescriber::memory(sqlite_memory_schema_fingerprint(None));
    let error = build_metadata_batch(
        &mut source_map,
        &entry,
        &checked,
        &["app.read.query".to_string()],
        &mut describer,
    )
    .expect_err("newer required SQLite version must fail before statement preparation");
    assert!(
        error.to_string().contains("older than required version"),
        "{error}"
    );
}

#[test]
fn postgres_native_prepare_describes_the_selected_query() {
    let required = std::env::var_os("ALIGN_DB_POSTGRES_REQUIRED").is_some();
    let Some(url) = std::env::var("ALIGN_DB_POSTGRES_URL").ok() else {
        assert!(
            !required,
            "ALIGN_DB_POSTGRES_URL is required by this test environment"
        );
        eprintln!("skipping PostgreSQL Q3 owner: ALIGN_DB_POSTGRES_URL is not set");
        return;
    };
    let project = project("pkg-db-q3-postgres-native");
    std::fs::write(
        project.dir.join("app/pg_read.align"),
        r#"module app.pg_read
import pkg.db
import pkg.db.postgres

pub Params { typed: i64, inferred: i64 }
pub Row { inferred: i64, typed: i64 }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT CAST(:inferred AS BIGINT) AS inferred, :typed AS typed",
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [pkg.db.postgres.QueryOption.ParameterType("typed", "int8")],
)
"#,
    )
    .expect("write PostgreSQL parameter type fixture");
    let entry = project.dir.join(&project.entry);
    let (mut source_map, checked) = checked_project(&project);
    let mut describer = PostgresDescriber::new(url.clone(), "q3-test-v1".to_string());
    let batch = build_metadata_batch(
        &mut source_map,
        &entry,
        &checked,
        &[
            "app.pg_read.query".to_string(),
            "app.pg_command.command".to_string(),
        ],
        &mut describer,
    )
    .expect("native PostgreSQL metadata");
    assert_eq!(batch.files.len(), 2);
    let bytes = &batch
        .files
        .iter()
        .find(|file| file.descriptor_id == "app.pg_read.query")
        .expect("PostgreSQL Query metadata")
        .bytes;
    for needle in [
        b"\"driver\":\"postgres\"".as_slice(),
        b"\"native_type\":\"bigint\"".as_slice(),
        b"\"native_type_id\":20".as_slice(),
        b"\"nullable\":\"unknown\"".as_slice(),
    ] {
        assert!(bytes.windows(needle.len()).any(|window| window == needle));
    }

    let run_cli = |check_only: bool| {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"));
        command.args([
            "db",
            "prepare",
            entry.to_str().expect("UTF-8 entry"),
            "--driver",
            "postgres",
            "--url-env",
            "ALIGN_DB_POSTGRES_URL",
            "--schema-id",
            "q3-test-v1",
            "--query",
            "app.pg_read.query",
            "--query",
            "app.pg_command.command",
        ]);
        if check_only {
            command.arg("--check");
        }
        command.output().expect("run PostgreSQL db prepare CLI")
    };
    let published = run_cli(false);
    assert!(
        published.status.success(),
        "{}",
        String::from_utf8_lossy(&published.stderr)
    );
    let output = [published.stdout.as_slice(), published.stderr.as_slice()].concat();
    assert!(!output
        .windows(url.len())
        .any(|window| window == url.as_bytes()));
    let checked = run_cli(true);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
}
