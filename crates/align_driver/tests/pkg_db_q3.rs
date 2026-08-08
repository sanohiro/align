//! pkg.db Q3/D3+D5 checked/offline metadata owners.

mod common;
use align_driver::db_prepare::{
    MetadataDescriber, MigrationEntry, NativeColumnDescription, NativeParameterDescription,
    NativeStatementDescription, PreparationEnvironment, PrepareError, build_metadata_batch,
    encode_migration_catalog, postgres_schema_fingerprint, publish_metadata_batch,
    read_migration_catalog, sqlite_database_schema_fingerprint, sqlite_memory_schema_fingerprint,
};
use align_driver::{Driver, Hash128, check};
use align_driver::db_prepare_native::{PostgresDescriber, SqliteDescriber};
use align_interface::{DriverEntry, MetaNullability, StaticArtifact};
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
    let main = "module main\nimport app.read\nimport app.write\nimport app.sqlite_table\nimport app.pg_read\nfn main() -> i32 = 0\n";
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

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.sqlite.query(
  "SELECT CAST(:value AS BIGINT) AS value",
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [],
)
"#,
        ),
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
        Ok(NativeStatementDescription {
            parameters: vec![NativeParameterDescription {
                ordinal: 1,
                source_name: (self.driver == Driver::SQLite).then(|| "value".to_string()),
                native_type: Some(
                    match self.driver {
                        Driver::SQLite => "INTEGER",
                        Driver::PostgreSQL => "int8",
                    }
                    .to_string(),
                ),
                native_type_id: (self.driver == Driver::PostgreSQL).then_some(20),
            }],
            columns: if query {
                vec![NativeColumnDescription {
                    ordinal: 0,
                    source_alias: "value".to_string(),
                    native_type: Some(
                        match self.driver {
                            Driver::SQLite => "INTEGER",
                            Driver::PostgreSQL => "int8",
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
        ["app.read.query", "app.sqlite_table.query"]
    );
    assert_eq!(first.files.len(), 2);
    assert!(first.files.iter().all(|file| file.bytes.ends_with(b"}\n")));
    let nullable = b"\"nullable\":\"unknown\"";
    assert!(
        first.files[0]
            .bytes
            .windows(nullable.len())
            .any(|bytes| bytes == nullable)
    );

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
    assert!(
        postgres_schema_fingerprint(
            "schema-v1",
            &["app".to_string(), "public".to_string()],
            &[],
        )
        .is_ok()
    );

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
    assert_eq!(written.selected, 2);
    assert_eq!(written.changed, 2);
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
}

#[test]
fn selection_rejects_unknown_and_duplicate_ids_before_native_open() {
    let project = project("pkg-db-q3-selection");
    let entry = project.dir.join(&project.entry);
    for selected in [
        vec!["app.missing.query".to_string()],
        vec!["app.read.query".to_string(), "app.read.query".to_string()],
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
        &["app.read.query".to_string()],
        &mut describer,
    )
    .expect("native SQLite metadata");
    assert_eq!(batch.files.len(), 1);
    let bytes = &batch.files[0].bytes;
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
    bytes.extend_from_slice(&u32::try_from(entries.len()).expect("fixture count").to_le_bytes());
    for (version, filename, content) in entries {
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(filename.len()).expect("fixture filename").to_le_bytes());
        bytes.extend_from_slice(filename.as_bytes());
        let hash = Hash128::of(content);
        bytes.extend_from_slice(&hash.lo.to_le_bytes());
        bytes.extend_from_slice(&hash.hi.to_le_bytes());
    }
    bytes
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
    assert_eq!(catalog.entries.iter().map(|entry| entry.version).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(
        catalog.encoded,
        reference_migration_bytes(&[
            (1, "0001_create_items.sql", first),
            (2, "0002_seed.sql", second),
        ])
    );
    assert_eq!(catalog.fingerprint, Hash128::of(&catalog.encoded));

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
    assert!(batch.files[0].bytes.windows(b"\"origin_table\":\"items\"".len())
        .any(|window| window == b"\"origin_table\":\"items\""));

    std::fs::remove_file(migrations.join("0002_seed.sql")).expect("remove second migration");
    std::fs::write(migrations.join("0003_gap.sql"), b"SELECT 1;").expect("write gap migration");
    assert!(read_migration_catalog(&migrations).unwrap_err().to_string().contains("expected 0002"));

    let empty = encode_migration_catalog(Vec::<MigrationEntry>::new()).expect("encode empty catalog");
    assert_eq!(empty.encoded, reference_migration_bytes(&[]));
}

#[test]
fn prepare_cli_writes_then_checks_sqlite_metadata() {
    let project = project("pkg-db-q3-cli");
    let entry = project.dir.join(&project.entry);
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
    assert!(written.status.success(), "{}", String::from_utf8_lossy(&written.stderr));
    assert!(String::from_utf8_lossy(&written.stdout).contains("selected=1 changed=1"));
    let checked = command(true);
    assert!(checked.status.success(), "{}", String::from_utf8_lossy(&checked.stderr));
    assert!(String::from_utf8_lossy(&checked.stdout).contains("selected=1 changed=0"));

    let invalid = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
        .args([
            "db",
            "prepare",
            entry.to_str().expect("UTF-8 entry"),
            "--driver",
            "postgres",
            "--memory",
        ])
        .output()
        .expect("run invalid prepare");
    assert!(!invalid.status.success());
}

#[test]
fn postgres_native_prepare_describes_the_selected_query() {
    let required = std::env::var_os("ALIGN_DB_POSTGRES_REQUIRED").is_some();
    let Some(url) = std::env::var("ALIGN_DB_POSTGRES_URL").ok() else {
        assert!(!required, "ALIGN_DB_POSTGRES_URL is required by this test environment");
        eprintln!("skipping PostgreSQL Q3 owner: ALIGN_DB_POSTGRES_URL is not set");
        return;
    };
    let project = project("pkg-db-q3-postgres-native");
    let entry = project.dir.join(&project.entry);
    let (mut source_map, checked) = checked_project(&project);
    let mut describer = PostgresDescriber::new(url, "q3-test-v1".to_string());
    let batch = build_metadata_batch(
        &mut source_map,
        &entry,
        &checked,
        &["app.pg_read.query".to_string()],
        &mut describer,
    )
    .expect("native PostgreSQL metadata");
    assert_eq!(batch.files.len(), 1);
    let bytes = &batch.files[0].bytes;
    for needle in [
        b"\"driver\":\"postgres\"".as_slice(),
        b"\"native_type\":\"bigint\"".as_slice(),
        b"\"native_type_id\":20".as_slice(),
        b"\"nullable\":\"unknown\"".as_slice(),
    ] {
        assert!(bytes.windows(needle.len()).any(|window| window == needle));
    }
}
