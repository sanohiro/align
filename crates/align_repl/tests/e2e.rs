use std::io::Write;
use std::process::{Command, Stdio};

use align_repl::{Config, Outcome, Session};

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read fixture {}: {error}", path.display()))
}

#[test]
fn cli_rejects_extra_arguments_and_malformed_jobs() {
    let extra = Command::new(env!("CARGO_BIN_EXE_align-repl"))
        .args(["--help", "extra"])
        .output()
        .unwrap_or_else(|error| panic!("run align-repl with extra argument: {error}"));
    assert!(!extra.status.success());
    assert!(String::from_utf8_lossy(&extra.stderr).contains("unexpected argument"));

    let invalid_jobs = Command::new(env!("CARGO_BIN_EXE_align-repl"))
        .env("ALIGNC_JOBS", "many")
        .output()
        .unwrap_or_else(|error| panic!("run align-repl with invalid jobs: {error}"));
    assert!(!invalid_jobs.status.success());
    assert!(String::from_utf8_lossy(&invalid_jobs.stderr).contains("invalid ALIGNC_JOBS 'many'"));
}

#[test]
fn piped_session_runs_without_prompts() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_align-repl"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn align-repl: {error}"));
    let Some(stdin) = child.stdin.as_mut() else {
        panic!("align-repl child has no piped stdin");
    };
    stdin
        .write_all(b"1 + 2\nx := 5\nprint(x * 2)\n:quit\n")
        .unwrap_or_else(|error| panic!("write script: {error}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("wait for align-repl: {error}"));
    assert!(output.status.success(), "status: {:?}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("align> "));
    assert!(stdout.contains("3\n"));
    assert!(stdout.contains("10\n"));
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn twenty_five_entry_transcript_matches_runtime_loaded_golden() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_align-repl"))
        .env("ALIGNC_JOBS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn align-repl: {error}"));
    let Some(stdin) = child.stdin.as_mut() else {
        panic!("align-repl child has no piped stdin");
    };
    stdin
        .write_all(fixture("session-25.in").as_bytes())
        .unwrap_or_else(|error| panic!("write transcript: {error}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("wait for align-repl: {error}"));
    assert!(output.status.success(), "status: {:?}", output.status);
    assert_eq!(String::from_utf8_lossy(&output.stdout), fixture("session-25.stdout"));
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn saved_file_builds_with_the_real_alignc_binary() {
    let stage = align_driver::ArtifactStage::temp("align-repl-saved-owner")
        .unwrap_or_else(|error| panic!("create saved-file stage: {error}"));
    let source_path = stage.path().join("saved.align");
    let mut repl = Session::new(Config {
        jobs: 1,
        ..Config::default()
    })
    .unwrap_or_else(|error| panic!("start align-repl session: {error}"));
    assert!(matches!(repl.submit("40 + 2"), Outcome::Applied { .. }));
    repl.save(&source_path, false)
        .unwrap_or_else(|error| panic!("save session source: {error:?}"));

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.join("../..");
    let cargo = workspace.join("scripts/cargo.sh");
    let built = Command::new(&cargo)
        .current_dir(stage.path())
        .env("CARGO_BUILD_JOBS", "1")
        .env("ALIGNC_JOBS", "1")
        .args([
            "run",
            "--manifest-path",
            workspace.join("Cargo.toml").to_string_lossy().as_ref(),
            "-q",
            "-p",
            "align_driver",
            "--bin",
            "alignc",
            "--",
            "build",
            "saved.align",
            "-j",
            "1",
        ])
        .output()
        .unwrap_or_else(|error| panic!("run real alignc build: {error}"));
    assert!(
        built.status.success(),
        "alignc stdout:\n{}\nalignc stderr:\n{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(stage.path().join("saved"))
        .output()
        .unwrap_or_else(|error| panic!("run saved alignc program: {error}"));
    assert!(run.status.success(), "saved program status: {:?}", run.status);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    assert!(run.stderr.is_empty(), "{}", String::from_utf8_lossy(&run.stderr));
}

#[test]
fn cache_off_still_builds_and_runs() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_align-repl"))
        .env("ALIGNC_CACHE", "off")
        .env("ALIGNC_JOBS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn cache-off align-repl: {error}"));
    let Some(stdin) = child.stdin.as_mut() else {
        panic!("align-repl child has no piped stdin");
    };
    stdin
        .write_all(b"6 * 7\n:quit\n")
        .unwrap_or_else(|error| panic!("write cache-off script: {error}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("wait for cache-off align-repl: {error}"));
    assert!(output.status.success(), "status: {:?}", output.status);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
}
