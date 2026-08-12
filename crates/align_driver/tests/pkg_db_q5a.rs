//! pkg.db Q5a/D11 migration lifecycle owners.

mod common;
mod db_harness;

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

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
            .args(["db", "status", "--unknown"])
            .arg(std::ffi::OsString::from_vec(vec![0xff]))
            .output()
            .expect("run source-ordered non-UTF-8 CLI case");
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unknown `db` migration option"));
    }
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
    assert!(
        resolve_migration_paths(&project.dir.join("main.align"), Path::new("../outside"),).is_err()
    );
    assert!(
        resolve_migration_paths(
            &project.dir.join("main.align"),
            &project.dir.join("db/migrations"),
        )
        .is_err()
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let linked_entry = project.dir.join("linked.align");
        symlink(project.dir.join("main.align"), &linked_entry).expect("entry symlink");
        assert!(resolve_migration_paths(&linked_entry, Path::new("db/migrations")).is_err());
        let linked_dir = project.dir.join("linked-migrations");
        symlink(project.dir.join("db/migrations"), &linked_dir).expect("directory symlink");
        assert!(
            resolve_migration_paths(
                &project.dir.join("main.align"),
                Path::new("linked-migrations"),
            )
            .is_err()
        );
    }

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
fn postgres_migration_copy_is_rejected_before_native_work() {
    for (index, (sql, expected)) in [
        (
            "COPY target FROM STDIN;",
            "contains a PostgreSQL COPY statement",
        ),
        (
            "-- align:migration transaction=forbidden\n/* lead */ copy target FROM STDIN;",
            "contains a PostgreSQL COPY statement",
        ),
        (
            "SELECT 1; COPY target FROM STDIN;",
            "contains a PostgreSQL COPY statement",
        ),
        (
            "BEGIN; COPY target FROM STDIN;",
            "contains a transaction-control statement",
        ),
        (
            "COPY target FROM STDIN; BEGIN;",
            "contains a PostgreSQL COPY statement",
        ),
        (
            "-- align:migration transaction=forbidden\nCOPY target FROM STDIN; SELECT 1;",
            "contains a PostgreSQL COPY statement",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let raw = encode_migration_catalog(vec![MigrationEntry {
            version: 1,
            filename: format!("0001_copy_{index}.sql"),
            path: PathBuf::from(format!("0001_copy_{index}.sql")),
            bytes: sql.as_bytes().to_vec(),
        }])
        .expect("COPY catalog");
        let error = screen_postgres_catalog(&raw).expect_err("COPY must fail during screening");
        assert!(error.to_string().contains(expected), "{sql}: {error}");
    }

    for sql in [
        "SELECT 'COPY target FROM STDIN';",
        "SELECT $$ COPY target FROM STDIN $$;",
        "SELECT 1 /* COPY target FROM STDIN */;",
        "SELECT copy FROM target;",
    ] {
        let raw = encode_migration_catalog(vec![MigrationEntry {
            version: 1,
            filename: "0001_near_miss.sql".to_string(),
            path: PathBuf::from("0001_near_miss.sql"),
            bytes: sql.as_bytes().to_vec(),
        }])
        .expect("near-miss catalog");
        screen_postgres_catalog(&raw).expect("quoted/comment/COLUMN COPY text is data");
    }

    let sqlite = project(
        "pkg-db-q5a-sqlite-copy",
        &[("0001_copy.sql", "COPY target FROM STDIN;")],
    );
    screened_sqlite(&sqlite);
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
    let cleared = run_sqlite_migration(
        &dirty_database,
        MigrationOperation::Repair {
            version: 1,
            action: RepairAction::ClearDirty,
            expected_checksum: &checksum,
        },
        &dirty_catalog,
    )
    .expect("clear dirty migration");
    assert_eq!(cleared.rows[0].status, MigrationStatus::Pending);
    assert!(
        run_sqlite_migration(&dirty_database, MigrationOperation::Migrate, &dirty_catalog,)
            .is_err()
    );
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

    let forged_project = project(
        "pkg-db-q5a-sqlite-forged-history",
        &[
            ("0001_create.sql", "CREATE TABLE stable(id INTEGER);"),
            (
                "0002_erase_history.sql",
                "-- align:migration transaction=forbidden\nDELETE FROM main.__align_migrations_v1;",
            ),
        ],
    );
    let forged_database = forged_project.dir.join("forged.sqlite");
    let forged_catalog = screened_sqlite(&forged_project);
    assert!(
        run_sqlite_migration(
            &forged_database,
            MigrationOperation::Migrate,
            &forged_catalog,
        )
        .unwrap_err()
        .to_string()
        .contains("exact Applying snapshot was restored")
    );
    let forged_status = run_sqlite_migration(
        &forged_database,
        MigrationOperation::Status,
        &forged_catalog,
    )
    .expect("status after history forgery");
    assert_eq!(forged_status.rows[0].status, MigrationStatus::Applied);
    assert_eq!(forged_status.rows[1].status, MigrationStatus::DirtyApplying);
    assert!(
        run_sqlite_migration(
            &forged_database,
            MigrationOperation::Migrate,
            &forged_catalog,
        )
        .is_err()
    );

    let malformed_project = project(
        "pkg-db-q5a-sqlite-malformed-history",
        &[
            ("0001_create.sql", "CREATE TABLE stable(id INTEGER);"),
            (
                "0002_malformed_history.sql",
                "-- align:migration transaction=forbidden\nUPDATE main.__align_migrations_v1 SET completed_statements=1 WHERE version=2;",
            ),
        ],
    );
    let malformed_database = malformed_project.dir.join("malformed.sqlite");
    let malformed_catalog = screened_sqlite(&malformed_project);
    assert!(
        run_sqlite_migration(
            &malformed_database,
            MigrationOperation::Migrate,
            &malformed_catalog,
        )
        .is_err()
    );
    assert!(
        run_sqlite_migration(
            &malformed_database,
            MigrationOperation::Status,
            &malformed_catalog,
        )
        .unwrap_err()
        .to_string()
        .contains("invalid state")
    );
}

#[test]
fn status_missing_sqlite_target_does_not_create_the_database() {
    let project = project(
        "pkg-db-q5a-sqlite-missing",
        &[("0001_create.sql", "CREATE TABLE item(id INTEGER);")],
    );
    let database = project.dir.join("missing.sqlite");
    let catalog = screened_sqlite(&project);
    assert!(run_sqlite_migration(&database, MigrationOperation::Status, &catalog).is_err());
    assert!(!database.exists());
    let lock = PathBuf::from(format!("{}.align-migrate.lock", database.display()));
    assert!(!lock.exists());
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
fn migration_catalog_history_scaling_measurement() {
    for count in [10u32, 100, 1000] {
        let entries = (1..=count)
            .map(|version| MigrationEntry {
                version,
                filename: format!("{version:04}_item.sql"),
                path: PathBuf::from(format!("{version:04}_item.sql")),
                bytes: format!("SELECT {version};").into_bytes(),
            })
            .collect();
        let started = std::time::Instant::now();
        let raw = encode_migration_catalog(entries).expect("catalog");
        let catalog = screen_postgres_catalog(&raw).expect("screen");
        let history = catalog
            .entries
            .iter()
            .map(|entry| HistoryRow {
                format_version: 1,
                version: entry.version,
                filename: entry.filename.clone(),
                checksum: entry.checksum.clone(),
                policy: entry.policy,
                state: StoredMigrationState::Applied,
                completed_statements: entry.statement_count,
            })
            .collect();
        assert!(
            reconcile(MigrationDriver::Postgres, &catalog, history)
                .expect("reconcile")
                .is_current()
        );
        eprintln!(
            "Q5a catalog/history measurement entries={count} elapsed_us={}",
            started.elapsed().as_micros()
        );
    }
}

#[test]
fn postgres_required_migration_lifecycle() {
    let Some(url) = db_harness::live_postgres_url("PostgreSQL Q5a owner") else {
        return;
    };
    let project = project(
        "pkg-db-q5a-postgres",
        &[
            (
                "0001_create.sql",
                "CREATE TABLE align_q5a_probe(id bigint PRIMARY KEY); INSERT INTO align_q5a_probe VALUES (1);",
            ),
            (
                "0002_index.sql",
                "-- align:migration transaction=forbidden\nCREATE INDEX CONCURRENTLY align_q5a_probe_id_index ON align_q5a_probe(id);",
            ),
        ],
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

    std::fs::write(
        project.dir.join("db/migrations/0003_erase_history.sql"),
        "-- align:migration transaction=forbidden\nDELETE FROM align_internal.migrations_v1 WHERE version=3;",
    )
    .expect("write history-forgery migration");
    let raw = read_migration_catalog(&project.dir.join("db/migrations")).expect("forgery catalog");
    let forged = screen_postgres_catalog(&raw).expect("screen forgery catalog");
    assert!(
        run_postgres_migration(&url, MigrationOperation::Migrate, &forged)
            .unwrap_err()
            .to_string()
            .contains("exact Applying snapshot was restored")
    );
    let after_forgery = run_postgres_migration(&url, MigrationOperation::Status, &forged)
        .expect("status after history forgery");
    assert_eq!(after_forgery.rows[0].status, MigrationStatus::Applied);
    assert_eq!(after_forgery.rows[1].status, MigrationStatus::Applied);
    assert_eq!(after_forgery.rows[2].status, MigrationStatus::DirtyApplying);
    run_postgres_migration(
        &url,
        MigrationOperation::Repair {
            version: 3,
            action: RepairAction::AcceptApplied,
            expected_checksum: &forged.entries[2].checksum,
        },
        &forged,
    )
    .expect("accept restored forbidden migration");

    std::fs::write(
        project.dir.join("db/migrations/0004_history_trigger.sql"),
        "CREATE TABLE align_q5a_history_ref(version integer REFERENCES align_internal.migrations_v1(version));",
    )
    .expect("write inbound-history-FK migration");
    let raw =
        read_migration_catalog(&project.dir.join("db/migrations")).expect("inbound FK catalog");
    let inbound_fk = screen_postgres_catalog(&raw).expect("screen inbound FK catalog");
    assert!(run_postgres_migration(&url, MigrationOperation::Migrate, &inbound_fk).is_err());
    let after_inbound_fk = run_postgres_migration(&url, MigrationOperation::Status, &inbound_fk)
        .expect("status after rejected inbound history FK");
    assert_eq!(after_inbound_fk.rows[3].status, MigrationStatus::Pending);

    std::fs::write(
        project.dir.join("db/migrations/0004_history_trigger.sql"),
        "GRANT SELECT (checksum) ON align_internal.migrations_v1 TO PUBLIC;",
    )
    .expect("write history-column-ACL migration");
    let raw =
        read_migration_catalog(&project.dir.join("db/migrations")).expect("column ACL catalog");
    let column_acl = screen_postgres_catalog(&raw).expect("screen column ACL catalog");
    assert!(run_postgres_migration(&url, MigrationOperation::Migrate, &column_acl).is_err());
    let after_column_acl = run_postgres_migration(&url, MigrationOperation::Status, &column_acl)
        .expect("status after rejected history column ACL");
    assert_eq!(after_column_acl.rows[3].status, MigrationStatus::Pending);

    std::fs::write(
        project.dir.join("db/migrations/0004_history_trigger.sql"),
        "CREATE FUNCTION align_q5a_history_trigger() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$; CREATE TRIGGER align_q5a_bad BEFORE UPDATE ON align_internal.migrations_v1 FOR EACH ROW EXECUTE FUNCTION align_q5a_history_trigger();",
    )
    .expect("write history-invariant migration");
    let raw = read_migration_catalog(&project.dir.join("db/migrations")).expect("expanded catalog");
    let expanded = screen_postgres_catalog(&raw).expect("screen expanded catalog");
    assert!(run_postgres_migration(&url, MigrationOperation::Migrate, &expanded).is_err());
    let after = run_postgres_migration(&url, MigrationOperation::Status, &expanded)
        .expect("status after rejected history mutation");
    assert_eq!(after.rows[3].status, MigrationStatus::Pending);

    std::fs::write(
        project.dir.join("db/migrations/0004_history_trigger.sql"),
        "-- align:migration transaction=forbidden\nUPDATE align_internal.migrations_v1 SET completed_statements=1 WHERE version=4;",
    )
    .expect("replace with malformed-history migration");
    let raw =
        read_migration_catalog(&project.dir.join("db/migrations")).expect("malformed catalog");
    let malformed = screen_postgres_catalog(&raw).expect("screen malformed catalog");
    assert!(run_postgres_migration(&url, MigrationOperation::Migrate, &malformed).is_err());
    assert!(
        run_postgres_migration(&url, MigrationOperation::Status, &malformed)
            .unwrap_err()
            .to_string()
            .contains("invalid state")
    );
}
