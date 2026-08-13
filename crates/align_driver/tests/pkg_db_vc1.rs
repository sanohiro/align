//! pkg.db VC1 vector-search compatibility checkpoint.

mod common;
use common::*;
mod db_harness;
use db_harness::Layout;

use align_driver::db_prepare::{PreparationExtension, postgres_schema_fingerprint};
use align_driver::static_inputs::metadata_path;

const VECTOR_QUERY: &str = r#"module app.vector
import pkg.db
import pkg.db.postgres

pub Params {
  embedding: str,
  category: str,
}

pub Row {
  id: i64,
  label: str,
  distance: f64,
}

pub fn search() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT item.id, item.label, item.embedding <-> CAST(:embedding AS text)::vector(3) AS distance FROM (VALUES (1::bigint, 'first'::text, '[1,2,3]'::vector(3), 'keep'::text), (2::bigint, 'second'::text, '[2,2,3]'::vector(3), 'keep'::text), (3::bigint, 'third'::text, '[4,2,3]'::vector(3), 'keep'::text), (4::bigint, 'filtered'::text, '[1,2,3]'::vector(3), 'other'::text)) AS item(id, label, embedding, category) WHERE item.category = CAST(:category AS text) ORDER BY distance, item.id LIMIT 2",
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [],
)
"#;

const VECTOR_MAIN: &str = r#"module main
import pkg.db
import pkg.db.postgres
import app.vector

fn params() -> app.vector.Params = app.vector.Params {
  embedding: "[1,2,3]",
  category: "keep",
}

fn direct(target: pkg.db.exec) -> i32 {
  opened := pkg.db.rows(target, app.vector.search(), params(), []) else { return 10 }
  mut rows := opened
  first_value := pkg.db.next(rows) else { return 11 }
  first := first_value else { return 12 }
  if first.id != 1 || first.label != "first" || first.distance != 0.0 { return 13 }
  second_value := pkg.db.next(rows) else { return 14 }
  second := second_value else { return 15 }
  if second.id != 2 || second.label != "second" || second.distance != 1.0 { return 16 }
  end := pkg.db.next(rows) else { return 17 }
  return match end { None => 0 Some(_) => 18 }
}

fn prepared(target: pkg.db.exec) -> i32 {
  created := pkg.db.prepare(target, app.vector.search(), []) else { return 20 }
  mut statement := created
  opened := pkg.db.rows_stmt(statement, params(), []) else { return 21 }
  mut rows := opened
  first_value := pkg.db.next(rows) else { return 22 }
  first := first_value else { return 23 }
  if first.id != 1 || first.label != "first" || first.distance != 0.0 { return 24 }
  second_value := pkg.db.next(rows) else { return 25 }
  second := second_value else { return 26 }
  if second.id != 2 || second.label != "second" || second.distance != 1.0 { return 27 }
  end := pkg.db.next(rows) else { return 28 }
  return match end { None => 0 Some(_) => 29 }
}

fn run(url: str) -> i32 {
  opened := pkg.db.postgres.connect(url, [])
  connection := opened else { return 1 }
  target := pkg.db.exec_conn(connection)
  arena out {
    metadata := pkg.db.meta_query(
      target, app.vector.search(), pkg.db.MetaDetail.Full, out, [],
    ) else { return 2 }
    if metadata.len() != 6 { return 3 }
    checked := match metadata[0].state {
      DatabaseChecked => true
      _ => false
    }
    if !checked { return 4 }
    plan := pkg.db.explain(target, app.vector.search(), params(), out, []) else { return 5 }
    if plan.analyzed || plan.body.len() == 0 { return 6 }
    direct_status := direct(target)
    if direct_status != 0 { return direct_status }
    return prepared(target)
  }
}

fn main(args: array<str>) -> Result<(), Error> {
  print(run(args[1]))
  return Ok(())
}
"#;

fn required(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| value == "1")
}

fn configured_url(name: &str, context: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Some(value),
        _ if required("ALIGN_DB_PGVECTOR_REQUIRED") => {
            panic!("ALIGN_DB_PGVECTOR_REQUIRED=1 requires {name} for {context}")
        }
        _ => {
            eprintln!("skipping {context}: {name} is not configured");
            None
        }
    }
}

fn prepare(project: &Proj, url_env: &str, url: &str, check_only: bool) -> std::process::Output {
    let entry = project.dir.join(&project.entry);
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"));
    command.current_dir(&project.dir).args([
        "db",
        "prepare",
        entry.to_str().expect("UTF-8 VC1 entry"),
        "--driver",
        "postgres",
        "--url-env",
        url_env,
        "--schema-id",
        "vc1-pgvector-0.8.6",
        "--query",
        "app.vector.search",
    ]);
    command.env(url_env, url);
    if check_only {
        command.arg("--check");
    }
    command.output().expect("run VC1 db prepare")
}

#[test]
fn pgvector_required_mode_requires_configuration() {
    if required("ALIGN_DB_PGVECTOR_REQUIRED") {
        assert!(
            std::env::var("ALIGN_DB_PGVECTOR_URL").is_ok_and(|value| !value.is_empty()),
            "ALIGN_DB_PGVECTOR_REQUIRED=1 requires ALIGN_DB_PGVECTOR_URL"
        );
    }
}

#[test]
fn pgvector_existing_surface_covers_checked_direct_prepared_metadata_and_explain() {
    if !backend_available() {
        return;
    }
    let Some(vector_url) = configured_url("ALIGN_DB_PGVECTOR_URL", "pgvector VC1 owner") else {
        return;
    };
    let Some(standard_url) = configured_url("ALIGN_DB_POSTGRES_URL", "no-extension VC1 control")
    else {
        return;
    };

    let project = Layout::new()
        .module("app/vector.align", VECTOR_QUERY)
        .main(VECTOR_MAIN)
        .materialize("pkg-db-vc1-pgvector");

    let published = prepare(&project, "ALIGN_DB_PGVECTOR_URL", &vector_url, false);
    assert!(
        published.status.success(),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&published.stdout),
        String::from_utf8_lossy(&published.stderr),
    );
    let evidence = String::from_utf8_lossy(&published.stdout);
    assert!(
        evidence.contains("extension=public.vector@0.8.6"),
        "pinned extension identity missing from preparation evidence:\n{evidence}"
    );

    let metadata = metadata_path(
        &project.dir,
        "app.vector.search",
        align_driver::Driver::PostgreSQL,
    )
    .expect("VC1 metadata path");
    let bytes = std::fs::read(&metadata).expect("read VC1 metadata");
    assert!(
        bytes
            .windows(b"{\"schema\":\"public\",\"name\":\"vector\",\"version\":\"0.8.6\"}".len())
            .any(|window| window
                == b"{\"schema\":\"public\",\"name\":\"vector\",\"version\":\"0.8.6\"}"),
        "checked metadata omitted the canonical pgvector extension record: {}",
        String::from_utf8_lossy(&bytes),
    );

    let checked = prepare(&project, "ALIGN_DB_PGVECTOR_URL", &vector_url, true);
    assert!(
        checked.status.success(),
        "unchanged pgvector metadata did not pass --check: {}",
        String::from_utf8_lossy(&checked.stderr),
    );

    let absent = prepare(&project, "ALIGN_DB_POSTGRES_URL", &standard_url, true);
    assert!(
        !absent.status.success(),
        "the ordinary no-extension PostgreSQL service accepted a pgvector Query"
    );
    assert!(
        String::from_utf8_lossy(&absent.stderr).contains("vector"),
        "the no-extension control failed for an unrelated reason: {}",
        String::from_utf8_lossy(&absent.stderr),
    );

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
        .current_dir(&project.dir)
        .args([
            "run",
            project
                .dir
                .join(&project.entry)
                .to_str()
                .expect("UTF-8 VC1 entry"),
            vector_url.as_str(),
        ])
        .env_remove("ALIGN_DB_PGVECTOR_URL")
        .output()
        .expect("build and run the offline-checked VC1 program");
    assert!(
        output.status.success(),
        "status: {:?}; stdout: {}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "0\n");
}

#[test]
fn pgvector_extension_version_changes_schema_identity() {
    let extension = |version: &str| PreparationExtension {
        schema: "public".to_string(),
        name: "vector".to_string(),
        version: Some(version.to_string()),
    };
    let search_path = vec!["pg_catalog".to_string(), "public".to_string()];
    let pinned =
        postgres_schema_fingerprint("vc1-pgvector-0.8.6", &search_path, &[extension("0.8.6")])
            .expect("pinned pgvector identity");
    let changed =
        postgres_schema_fingerprint("vc1-pgvector-0.8.6", &search_path, &[extension("0.8.5")])
            .expect("changed pgvector identity");
    assert_ne!(pinned, changed);
}
