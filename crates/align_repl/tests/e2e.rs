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
fn saved_file_object_matches_the_real_alignc_binary() {
    // The build-parity owner from `docs/impl/22-repl-plan.md` §12: the object the REPL emitted for
    // a session is byte-identical to the object the SHIPPED `alignc` binary emits for the file
    // `:save` actually wrote.
    //
    // Distinct from `build::repl_object_matches_the_alignc_build_path`, which reproduces the driver
    // calls in-process against a hardcoded source string. That proves the library path is
    // deterministic; it cannot see a change to `render`/`save`, and it never crosses into the
    // compiler users run.
    //
    // Scope, stated rather than implied: this pins the OBJECT only. `alignc build` stages its
    // objects in an `ArtifactStage` that `Drop` removes, so `emit-obj` is the only externally
    // observable object the shipped binary produces — and it is a different call site from the one
    // §4.1 mirrors (`build_per_unit` + `emit_object_cached` rather than `build_package` +
    // `codegen_package_parallel`). The two agree because both bottom out in the same
    // `emit_object_file(mir, obj, target, profile, &[], rt_lto)`. The link line, the
    // capability-library union, and the atomic publish are covered by the neighbouring
    // `saved_file_builds_with_the_real_alignc_binary`, which runs the linked program.
    //
    // The object rather than the executable, deliberately: a Mach-O image carries an `LC_UUID` and
    // page hashes derived from link-time inputs, so two links of identical objects differ by
    // construction.
    let stage = align_driver::ArtifactStage::temp("align-repl-object-parity")
        .unwrap_or_else(|error| panic!("create object-parity stage: {error}"));
    let source_path = stage.path().join("saved.align");
    let mut repl = Session::new(Config {
        jobs: 1,
        ..Config::default()
    })
    .unwrap_or_else(|error| panic!("start align-repl session: {error}"));
    for entry in [
        "P { a: i64, b: i64 }",
        "fn total(p: P) -> i64 = p.a + p.b",
        "p := P{a: 2, b: 40}",
        "print(total(p))",
    ] {
        match repl.submit(entry) {
            Outcome::Applied { .. } => {}
            Outcome::CompileFailed { rendered, .. } => panic!("`{entry}` did not compile:\n{rendered}"),
            other => panic!("`{entry}` was not applied: {other:?}"),
        }
    }
    repl.save(&source_path, false)
        .unwrap_or_else(|error| panic!("save session source: {error:?}"));

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.join("../..");
    let cargo = workspace.join("scripts/cargo.sh");
    let emitted = Command::new(&cargo)
        .current_dir(stage.path())
        // Only `CARGO_BUILD_JOBS` matters here: `alignc` resolves its own job count for build
        // verbs only, and `emit-obj` rejects `-j` outright.
        .env("CARGO_BUILD_JOBS", "1")
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
            "emit-obj",
            "saved.align",
            "alignc.o",
        ])
        .output()
        .unwrap_or_else(|error| panic!("run real alignc emit-obj: {error}"));
    assert!(
        emitted.status.success(),
        "alignc stdout:\n{}\nalignc stderr:\n{}",
        String::from_utf8_lossy(&emitted.stdout),
        String::from_utf8_lossy(&emitted.stderr)
    );

    // `object_path` hardcodes unit0 on the one-unit assumption; enforce it here rather than trust
    // it, so a future multi-unit session cannot silently reduce this owner to a fraction.
    let session_stage = repl
        .object_path()
        .parent()
        .unwrap_or_else(|| panic!("the session object has a parent directory"))
        .to_path_buf();
    let units: Vec<std::path::PathBuf> = std::fs::read_dir(&session_stage)
        .unwrap_or_else(|error| panic!("read the session stage: {error}"))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "o")
                && path
                    .file_stem()
                    .is_some_and(|stem| stem.to_string_lossy().starts_with("unit"))
        })
        .collect();
    assert_eq!(units.len(), 1, "a v1 session must be a one-unit package, found {units:?}");

    let repl_object = std::fs::read(repl.object_path())
        .unwrap_or_else(|error| panic!("read the repl object: {error}"));
    let alignc_object = std::fs::read(stage.path().join("alignc.o"))
        .unwrap_or_else(|error| panic!("read the alignc object: {error}"));
    assert!(!repl_object.is_empty(), "the repl object must not be empty");
    // Report the divergence by size and first differing offset: these are multi-kilobyte buffers,
    // and `assert_eq!` on them prints both as decimal byte lists, twice.
    assert_eq!(
        repl_object.len(),
        alignc_object.len(),
        "object sizes differ: repl {} bytes, alignc {} bytes",
        repl_object.len(),
        alignc_object.len()
    );
    if let Some(offset) = repl_object
        .iter()
        .zip(&alignc_object)
        .position(|(left, right)| left != right)
    {
        panic!(
            "the REPL's object diverged from the shipped compiler's object for the saved program \
             at byte {offset}: repl {:#04x} vs alignc {:#04x}",
            repl_object[offset], alignc_object[offset]
        );
    }
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
