//! pkg.db Q5a/D11 migration lifecycle owners.

mod common;

use align_driver::Hash128;
use align_driver::db_migrate::{
    HistoryRow, MigrationDriver, MigrationPolicy, MigrationStatus, StoredMigrationState, reconcile,
    resolve_migration_paths, screen_postgres_catalog,
};
use align_driver::db_migrate_native::{
    MigrationOperation, RepairAction, run_postgres_migration, run_sqlite_migration,
    screen_sqlite_catalog_native,
};
use align_driver::db_prepare::{MigrationEntry, encode_migration_catalog, read_migration_catalog};
use common::Proj;
use std::path::{Path, PathBuf};

fn project(tag: &str, migrations: &[(&str, &str)]) -> Proj {
    let project = Proj::new(
        tag,
        &[("main.align", "module main\nfn main() -> i32 = 0\n")],
        "main.align",
    );
    let directory = project.dir.join("db/migrations");
    std::fs::create_dir_all(&directory).expect("create migrations");
    for (name, sql) in migrations {
        std::fs::write(directory.join(name), sql).expect("write migration");
    }
    project
}

fn screened_sqlite(project: &Proj) -> align_driver::db_migrate::ScreenedCatalog {
    let catalog = read_migration_catalog(&project.dir.join("db/migrations")).expect("catalog");
    screen_sqlite_catalog_native(&catalog).expect("screen SQLite")
}

#[test]
fn migration_cli_rejects_invalid_forms_before_catalog_environment_or_native_work() {
    let project = project("pkg-db-q5a-cli", &[("0001_create.sql", "SELECT 1;")]);
    let entry = project.dir.join("main.align");
    let missing = project.dir.join("missing.align");
    for (arguments, expected) in [
        (
            vec!["--migrations", "missing", "--driver", "bogus"],
            "requires --entry",
        ),
        (
            vec![
                "--entry",
                missing.to_str().unwrap(),
                "--migrations",
                "missing",
            ],
            "requires --driver",
        ),
        (
            vec![
                "--entry",
                missing.to_str().unwrap(),
                "--migrations",
                "missing",
                "--driver",
                "postgres",
                "--postgres-url-env",
                "Q5A_MUST_NOT_BE_READ",
                "--sqlite-path",
                "db.sqlite",
            ],
            "requires exactly --postgres-url-env",
        ),
        (
            vec![
                "--entry",
                entry.to_str().unwrap(),
                "--migrations",
                "db/migrations",
                "--driver",
                "sqlite",
                "--sqlite-path",
                "db.sqlite",
                "--version",
                "bad",
            ],
            "valid only for db repair",
        ),
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
            .args(["db", "status"])
            .args(arguments)
            .output()
            .expect("run invalid migration CLI");
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected),
            "expected `{expected}` in `{stderr}`"
        );
    }
    assert!(!project.dir.join("db.sqlite").exists());
    assert!(!project.dir.join("db.sqlite.align-migrate.lock").exists());
}

#[test]
fn migration_path_and_policy_screening_are_exact() {
    let project = project(
        "pkg-db-q5a-screen",
        &[(
            "0001_create.sql",
            "-- align:migration transaction=forbidden\nSELECT ';' /* ; */;",
        )],
    );
    let resolved =
        resolve_migration_paths(&project.dir.join("main.align"), Path::new("db/migrations"))
            .expect("resolve contained catalog");
    let catalog = read_migration_catalog(&resolved.migrations).expect("catalog");
    let screened = screen_sqlite_catalog_native(&catalog).expect("screen");
    assert_eq!(screened.entries[0].policy, MigrationPolicy::Forbidden);
    assert_eq!(screened.entries[0].statement_count, 1);

    std::fs::write(
        resolved.migrations.join("0001_create.sql"),
        "-- align:migration transaction=forbidden\nSELECT 1; SELECT 2;",
    )
    .expect("replace migration");
    let catalog = read_migration_catalog(&resolved.migrations).expect("catalog");
    assert!(
        screen_sqlite_catalog_native(&catalog)
            .unwrap_err()
            .to_string()
            .contains("exactly one statement")
    );

    std::fs::write(
        resolved.migrations.join("0001_create.sql"),
        "SELECT 1;\n-- align:migration transaction=required",
    )
    .expect("replace migration");
    let catalog = read_migration_catalog(&resolved.migrations).expect("catalog");
    assert!(
        screen_postgres_catalog(&catalog)
            .unwrap_err()
            .to_string()
            .contains("after its first physical line")
    );
}

#[test]
fn sqlite_required_forbidden_status_check_and_repair_close_the_state_matrix() {
    let required = project(
        "pkg-db-q5a-sqlite-required",
        &[
            (
                "0001_create.sql",
                "CREATE TABLE users(id INTEGER PRIMARY KEY); INSERT INTO users VALUES (1);",
            ),
            (
                "0002_index.sql",
                "CREATE INDEX users_id_index ON users(id);",
            ),
        ],
    );
    let database = required.dir.join("state.sqlite");
    let catalog = screened_sqlite(&required);
    let migrated = run_sqlite_migration(&database, MigrationOperation::Migrate, &catalog)
        .expect("migrate required catalog");
    assert!(migrated.is_current());
    assert_eq!(
        migrated.rows[0]
            .history
            .as_ref()
            .unwrap()
            .completed_statements,
        2
    );
    assert!(
        run_sqlite_migration(&database, MigrationOperation::Check, &catalog)
            .expect("check current")
            .is_current()
    );

    let dirty_project = project(
        "pkg-db-q5a-sqlite-dirty",
        &[(
            "0001_bad.sql",
            "-- align:migration transaction=forbidden\nSELECT no_such_function();",
        )],
    );
    let dirty_database = dirty_project.dir.join("dirty.sqlite");
    let dirty_catalog = screened_sqlite(&dirty_project);
    assert!(
        run_sqlite_migration(&dirty_database, MigrationOperation::Migrate, &dirty_catalog,)
            .is_err()
    );
    let status = run_sqlite_migration(&dirty_database, MigrationOperation::Status, &dirty_catalog)
        .expect("dirty status");
    assert_eq!(status.rows[0].status, MigrationStatus::DirtyFailed);
    let checksum = dirty_catalog.entries[0].checksum.clone();
    let repaired = run_sqlite_migration(
        &dirty_database,
        MigrationOperation::Repair {
            version: 1,
            action: RepairAction::AcceptApplied,
            expected_checksum: &checksum,
        },
        &dirty_catalog,
    )
    .expect("accept dirty migration");
    assert!(repaired.is_current());
}

#[test]
fn migration_history_state_matrix_is_fail_closed_and_ordered() {
    let entries = (1..=2)
        .map(|version| MigrationEntry {
            version,
            filename: format!("{version:04}_item.sql"),
            path: PathBuf::from(format!("{version:04}_item.sql")),
            bytes: b"SELECT 1;".to_vec(),
        })
        .collect();
    let raw = encode_migration_catalog(entries).expect("catalog");
    let catalog = screen_postgres_catalog(&raw).expect("screen");
    let checksum = Hash128::of(b"SELECT 1;").to_hex();
    let report = reconcile(
        MigrationDriver::Postgres,
        &catalog,
        vec![
            HistoryRow {
                format_version: 1,
                version: 1,
                filename: "0001_other.sql".to_string(),
                checksum: checksum.clone(),
                policy: MigrationPolicy::Required,
                state: StoredMigrationState::Applied,
                completed_statements: 1,
            },
            HistoryRow {
                format_version: 1,
                version: 3,
                filename: "0003_old.sql".to_string(),
                checksum,
                policy: MigrationPolicy::Forbidden,
                state: StoredMigrationState::Applied,
                completed_statements: 1,
            },
        ],
    )
    .expect("reconcile");
    assert_eq!(
        report.rows.iter().map(|row| row.status).collect::<Vec<_>>(),
        vec![
            MigrationStatus::NameMismatch,
            MigrationStatus::Pending,
            MigrationStatus::HistoryOnly,
        ]
    );
    assert!(!report.can_migrate());
    assert!(report.render().contains("mismatched=1 history_only=1"));
}

#[test]
fn postgres_required_migration_lifecycle() {
    let required = std::env::var_os("ALIGN_DB_POSTGRES_REQUIRED").is_some();
    let Some(url) = std::env::var("ALIGN_DB_POSTGRES_URL").ok() else {
        assert!(
            !required,
            "ALIGN_DB_POSTGRES_URL is required by this test environment"
        );
        eprintln!("skipping PostgreSQL Q5a owner: ALIGN_DB_POSTGRES_URL is not set");
        return;
    };
    let project = project(
        "pkg-db-q5a-postgres",
        &[(
            "0001_create.sql",
            "CREATE TABLE align_q5a_probe(id bigint PRIMARY KEY); INSERT INTO align_q5a_probe VALUES (1);",
        )],
    );
    let raw = read_migration_catalog(&project.dir.join("db/migrations")).expect("catalog");
    let catalog = screen_postgres_catalog(&raw).expect("screen");
    let report = run_postgres_migration(&url, MigrationOperation::Migrate, &catalog)
        .expect("PostgreSQL migrate");
    assert!(report.is_current());
    assert!(
        run_postgres_migration(&url, MigrationOperation::Status, &catalog)
            .expect("PostgreSQL status")
            .is_current()
    );
}
