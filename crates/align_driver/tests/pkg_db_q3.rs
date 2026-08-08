//! pkg.db Q3/D3+D5 checked/offline metadata owners.

mod common;
use align_driver::db_prepare::{
    MetadataDescriber, NativeColumnDescription, NativeParameterDescription,
    NativeStatementDescription, PreparationEnvironment, PrepareError, build_metadata_batch,
    postgres_schema_fingerprint, publish_metadata_batch, sqlite_database_schema_fingerprint,
    sqlite_memory_schema_fingerprint,
};
use align_driver::{Driver, Hash128, check};
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
    let main = "module main\nimport app.read\nimport app.write\nfn main() -> i32 = 0\n";
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
    assert_eq!(describer.describe_calls, ["app.read.query"]);
    assert_eq!(first.files.len(), 1);
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
    assert_eq!(written.selected, 1);
    assert_eq!(written.changed, 1);
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
