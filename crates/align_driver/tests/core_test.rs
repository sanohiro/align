//! `core.test` owner: real CLI, compiler-produced harness, cache, bounds, and terminal cleanup.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NONCE: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "align-core-test-owner-{}-{label}-{nonce}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("create owner scratch");
        Self(path)
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create Align source parent");
        }
        std::fs::write(&path, source).expect("write Align source");
        path
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_alignc"))
            .args(args)
            .current_dir(&self.0)
            .env("ALIGNC_CACHE", "off")
            .output()
            .expect("run alignc")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn available() -> bool {
    align_driver::backend_available()
        && Command::new("cc")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
}

fn install_pkg_db(scratch: &Scratch) {
    let package_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/db");
    for path in [
        "pkg/db.align",
        "pkg/db/sqlite.align",
        "pkg/db/postgres.align",
        "pkg/db/internal.align",
        "pkg/db/internal/resource.align",
        "pkg/db/internal/descriptor.align",
        "pkg/db/internal/sqlite.align",
        "pkg/db/internal/postgres.align",
        "pkg/db/internal/postgres_status.align",
    ] {
        let source = std::fs::read_to_string(package_root.join(path)).expect("read pkg.db fixture");
        scratch.write(path, &source);
    }
}

#[test]
fn catalog_harness_assertion_and_source_main_boundaries_execute_end_to_end() {
    if !available() {
        return;
    }
    let scratch = Scratch::new("catalog");
    scratch.write(
        "dep.align",
        "module dep\nimport core.test\ntest \"dependency\" { test.expect(true) }\n",
    );
    scratch.write("bridge.align", "module bridge\nimport dep\n");
    scratch.write(
        "main.align",
        concat!(
            "module app\n",
            "import bridge\n",
            "import core.test\n",
            "fn main() { print(999) }\n",
            "test \"passes\" { test.expect_eq(2 + 2, 4) }\n",
            "test \"fails\" { test.expect(false) }\n",
            "test \"multiline\" {\n",
            "  test.expect(true\n",
            "    && false)\n",
            "}\n",
        ),
    );
    let output = scratch.run(&["test", "main.align"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected test status: {output:?}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "FAIL app::fails\n",
            "reason: returned Error.Invalid\n",
            "--- stderr ---\n",
            "assertion failed: app::fails:6:16: expected true\n",
            "FAIL app::multiline\n",
            "reason: returned Error.Invalid\n",
            "--- stderr ---\n",
            "assertion failed: app::multiline:8:3: expected true\n",
            "test result: FAILED. 2 passed; 2 failed\n",
        ),
        "dependency-first catalog, assertion location, or source-main suppression drifted: {output:?}"
    );
    assert!(
        output.stderr.is_empty(),
        "ordinary test failure wrote driver stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn every_source_main_abi_is_encoded_and_only_runs_when_a_test_calls_it() {
    if !available() {
        return;
    }
    let cases = [
        (
            "i32",
            "import core.test\nfn main() -> i32 = 7\ntest \"calls\" { test.expect_eq(main(), 7) }\n",
        ),
        ("unit", "fn main() {}\ntest \"calls\" { main() }\n"),
        (
            "result",
            "fn main() -> Result<(), Error> = Ok(())\ntest \"calls\" { main()? }\n",
        ),
        (
            "args-result",
            "fn main(args: array<str>) -> Result<(), Error> = Ok(())\ntest \"calls\" { main([\"arg\"].to_array())? }\n",
        ),
    ];
    for (label, source) in cases {
        let scratch = Scratch::new(label);
        scratch.write("main.align", source);
        let output = scratch.run(&["test", "main.align"]);
        assert_eq!(output.status.code(), Some(0), "{label}: {output:?}");
        assert_eq!(output.stdout, b"test result: ok. 1 passed; 0 failed\n");
        assert!(
            output.stderr.is_empty(),
            "{label}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let scratch = Scratch::new("not-automatic");
    scratch.write(
        "main.align",
        concat!(
            "import std.process\n",
            "fn main() { process.abort() }\n",
            "test \"does not call main\" {}\n",
        ),
    );
    let output = scratch.run(&["test", "main.align"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "source main ran automatically: {output:?}"
    );
    assert_eq!(output.stdout, b"test result: ok. 1 passed; 0 failed\n");
}

#[test]
fn production_artifact_and_interface_ignore_the_test_overlay() {
    if !available() {
        return;
    }
    let plain = Scratch::new("production-plain");
    let tested = Scratch::new("production-tested");
    let production = "fn main() { print(7) }\n";
    plain.write("main.align", production);
    tested.write(
        "main.align",
        &format!("import core.test\n{production}test \"extra\" {{ test.expect(false) }}\n"),
    );
    let plain_build = plain.run(&["build", "main.align", "--profile", "dev"]);
    let tested_build = tested.run(&["build", "main.align", "--profile", "dev"]);
    assert!(plain_build.status.success(), "{plain_build:?}");
    assert!(tested_build.status.success(), "{tested_build:?}");
    assert_eq!(
        std::fs::read(plain.0.join("main")).unwrap(),
        std::fs::read(tested.0.join("main")).unwrap(),
        "test-only source changed production executable bytes"
    );
    let plain_interface = plain.run(&["emit-interface", "main.align"]);
    let tested_interface = tested.run(&["emit-interface", "main.align"]);
    assert_eq!(plain_interface.status, tested_interface.status);
    assert_eq!(plain_interface.stdout, tested_interface.stdout);
}

#[test]
fn zero_tests_and_reachable_process_command_fail_before_artifact_formation() {
    if !available() {
        return;
    }
    let scratch = Scratch::new("pre-artifact");
    scratch.write("none.align", "module none\nfn helper() -> i64 = 1\n");
    let none = scratch.run(&["test", "none.align"]);
    assert_eq!(none.status.code(), Some(1));
    assert!(none.stdout.is_empty());
    assert_eq!(none.stderr, b"alignc: no tests found\n");

    scratch.write(
        "command.align",
        concat!(
            "module command_test\n",
            "import std.process\n",
            "test \"closed boundary\" {\n",
            "  command := process.command(\"/bin/echo\", [\"/bin/echo\", \"no\"])\n",
            "}\n",
        ),
    );
    let command = scratch.run(&["test", "command.align"]);
    assert_eq!(
        command.status.code(),
        Some(1),
        "unexpected command status: {command:?}"
    );
    assert!(command.stdout.is_empty());
    assert_eq!(
        command.stderr,
        b"process.command is not available from test code; run the external process in an owner test\n"
    );

    scratch.write(
        "command_dep.align",
        concat!(
            "module command_dep\n",
            "import std.process\n",
            "pub fn closed() {\n",
            "  command := process.command(\"/bin/echo\", [\"/bin/echo\", \"no\"])\n",
            "}\n",
        ),
    );
    scratch.write(
        "command_import.align",
        concat!(
            "module command_import\n",
            "import command_dep\n",
            "test \"closed imported boundary\" { command_dep.closed() }\n",
        ),
    );
    let imported = scratch.run(&["test", "command_import.align"]);
    assert_eq!(imported.status.code(), Some(1));
    assert!(imported.stdout.is_empty());
    assert_eq!(
        imported.stderr,
        b"process.command is not available from test code; run the external process in an owner test\n"
    );

    let descriptor = Scratch::new("pre-artifact-static-descriptor");
    install_pkg_db(&descriptor);
    descriptor.write(
        "app/query.align",
        concat!(
            "module app.query\n",
            "import pkg.db\n",
            "import pkg.db.sqlite\n",
            "pub Params { id: i64 }\n",
            "pub Row { id: i64 }\n",
            "pub fn query() -> pkg.db.query<Params, Row> = ",
            "pkg.db.sqlite.query_file(\n",
            "  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],\n",
            "  [],\n",
            ")\n",
        ),
    );
    for (entry, source, expected) in [
        (
            "no_tests.align",
            "module no_tests\nimport app.query\nfn keep() -> i64 = 1\n",
            "alignc: no tests found",
        ),
        (
            "process.align",
            concat!(
                "module process_test\n",
                "import app.query\n",
                "import std.process\n",
                "test \"closed before static input\" {\n",
                "  command := process.command(\"/bin/echo\", [\"/bin/echo\", \"no\"])\n",
                "}\n",
            ),
            "process.command is not available from test code",
        ),
    ] {
        descriptor.write(entry, source);
        let output = descriptor.run(&["test", entry]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{entry}: {output:?}");
        assert!(output.stdout.is_empty(), "{entry}: {output:?}");
        assert!(stderr.contains(expected), "{entry}: {stderr}");
        assert!(
            !stderr.contains("query.sql"),
            "{entry}: static input resolution masked the test boundary: {stderr}"
        );
    }

    for (label, binding) in [
        ("direct-resource-drop", "owner := test.resource.open()"),
        ("nested-resource-drop", "owner := test.resource.boxed()"),
    ] {
        let scratch = Scratch::new(label);
        scratch.write(
            "test/resource/internal.align",
            concat!(
                "module test.resource.internal\n",
                "import std.process\n",
                "pub fn drop_handle(handle: raw) {\n",
                "  unsafe {\n",
                "    command := process.command(\"/bin/echo\", [\"/bin/echo\", \"no\"])\n",
                "    raw.free(handle)\n",
                "  }\n",
                "}\n",
            ),
        );
        scratch.write(
            "test/resource.align",
            concat!(
                "module test.resource\n",
                "import test.resource.internal\n",
                "pub resource Handle<T> = test.resource.internal.drop_handle\n",
                "pub Owner { handle: Handle<i32> }\n",
                "pub fn open() -> Handle<i32> { unsafe { return resource.from_raw(raw.alloc(8)) } }\n",
                "pub fn boxed() -> Owner = Owner { handle: open() }\n",
            ),
        );
        let entry = scratch.write(
            "main.align",
            &format!(
                "module app\nimport core.test\nimport test.resource\ntest \"drop hook boundary\" {{ {binding} }}\n"
            ),
        );
        let source = std::fs::read_to_string(&entry).expect("read resource Drop entry");
        let mut source_map = align_span::SourceMap::new();
        let checked = align_driver::check(
            &mut source_map,
            entry.to_str().expect("UTF-8 scratch path"),
            &source,
        );
        assert!(!checked.diags.has_errors(), "{label}: frontend rejected fixture");
        let whole_error = align_driver::lower_test_to_mir_with_static_descriptors(
            &checked,
            &mut source_map,
            &scratch.0,
        )
        .expect_err("whole-program lowering permitted an implicit process edge");
        assert_eq!(
            whole_error,
            "process.command is not available from test code; run the external process in an owner test",
            "{label}: whole-program boundary"
        );
        let mut per_unit_source_map = align_span::SourceMap::new();
        let per_unit = align_driver::build_test_per_unit_at(
            &mut per_unit_source_map,
            entry.to_str().expect("UTF-8 scratch path"),
            &source,
            &entry,
        );
        assert!(!per_unit.diags.has_errors(), "{label}: per-unit fixture rejected");
        assert_eq!(
            per_unit.boundary_error.as_deref(),
            Some(
                "process.command is not available from test code; run the external process in an owner test"
            ),
            "{label}: per-unit implicit resource Drop bypassed the process boundary"
        );
        let output = scratch.run(&["test", "main.align"]);
        assert_eq!(output.status.code(), Some(1), "{label}: {output:?}");
        assert!(output.stdout.is_empty(), "{label}: {output:?}");
        assert_eq!(
            output.stderr,
            b"process.command is not available from test code; run the external process in an owner test\n",
            "{label}: implicit resource Drop bypassed the process boundary"
        );
    }
}

#[test]
fn options_bounds_and_test_cache_reach_only_their_terminal_consumers() {
    if !available() {
        return;
    }
    let scratch = Scratch::new("options");
    scratch.write(
        "main.align",
        concat!("module options\n", "test \"writes\" { print(1) }\n",),
    );
    let repeated = scratch.run(&["test", "missing.align", "--timeout-ns=1", "--timeout-ns=2"]);
    assert_eq!(repeated.status.code(), Some(1));
    assert_eq!(
        repeated.stderr, b"alignc: --timeout-ns may be specified at most once\n",
        "test-option validation did not precede source I/O"
    );

    let bounded = scratch.run(&["test", "main.align", "--max-output-bytes=0"]);
    assert_eq!(
        bounded.status.code(),
        Some(1),
        "unexpected output-bound status: {bounded:?}"
    );
    assert_eq!(
        bounded.stdout,
        concat!(
            "FAIL options::writes\n",
            "reason: stdout exceeded 0 bytes\n",
            "test result: FAILED. 0 passed; 1 failed\n",
        )
        .as_bytes()
    );

    let cache = scratch.0.join("cache");
    let first = Command::new(env!("CARGO_BIN_EXE_alignc"))
        .args(["test", "main.align", "--profile", "dev", "--cache-stats"])
        .current_dir(&scratch.0)
        .env("ALIGNC_CACHE", &cache)
        .output()
        .expect("first cached test");
    let second = Command::new(env!("CARGO_BIN_EXE_alignc"))
        .args(["test", "main.align", "--cache-stats"])
        .current_dir(&scratch.0)
        .env("ALIGNC_CACHE", &cache)
        .output()
        .expect("second cached test");
    assert_eq!(first.status.code(), Some(0));
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(
        first.stdout, second.stdout,
        "cache state changed test output"
    );
    let second_stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        second_stderr.contains("@test/options hit"),
        "{second_stderr}"
    );
    assert!(
        second_stderr.contains("@test/harness hit"),
        "{second_stderr}"
    );
}

#[test]
fn timeout_kills_the_row_and_reports_only_bounded_evidence() {
    if !available() {
        return;
    }
    let scratch = Scratch::new("timeout");
    scratch.write(
        "main.align",
        concat!(
            "module timeout_test\n",
            "test \"slow\" {\n",
            "  mut i := 0\n",
            "  loop {\n",
            "    i = i + 1\n",
            "    if i == 100000000000 { break }\n",
            "  }\n",
            "}\n",
        ),
    );
    let output = scratch.run(&["test", "main.align", "--timeout-ns=50000000"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected timeout status: {output:?}"
    );
    assert_eq!(
        output.stdout,
        concat!(
            "FAIL timeout_test::slow\n",
            "reason: timed out after 50000000 ns\n",
            "test result: FAILED. 0 passed; 1 failed\n",
        )
        .as_bytes(),
        "{output:?}"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn timeout_removes_group_descendants_before_reporting() {
    if !available() {
        return;
    }
    let scratch = Scratch::new("descendant-timeout");
    scratch.write(
        "main.align",
        concat!(
            "module descendant_timeout\n",
            "import std.process\n",
            "test \"tree\" {\n",
            "  child := process.spawn(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"echo $$ > descendant.pid; exec /bin/sleep 30\"][0..3])?\n",
            "  mut i := 0\n",
            "  loop {\n",
            "    i = i + 1\n",
            "    if i == 100000000000 { break }\n",
            "  }\n",
            "}\n",
        ),
    );
    let output = scratch.run(&["test", "main.align", "--timeout-ns=500000000"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        output.stdout,
        concat!(
            "FAIL descendant_timeout::tree\n",
            "reason: timed out after 500000000 ns\n",
            "test result: FAILED. 0 passed; 1 failed\n",
        )
        .as_bytes()
    );
    assert!(output.stderr.is_empty());
    let pid = std::fs::read_to_string(scratch.0.join("descendant.pid"))
        .expect("descendant published its pid")
        .trim()
        .parse::<i32>()
        .expect("decimal descendant pid");
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "descendant remained alive after the row report"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[test]
fn exit_exec_abort_and_returned_error_are_row_failures_not_suite_exits() {
    if !available() {
        return;
    }
    let scratch = Scratch::new("terminal-product");
    scratch.write(
        "main.align",
        concat!(
            "module terminal_product\n",
            "import std.process\n",
            "test \"exit\" { process.exit(7) }\n",
            "test \"exec\" { process.exec(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"exit 8\"][0..3])? }\n",
            "test \"abort\" { process.abort() }\n",
            "test \"error\" { return Err(Error.Code(-9)) }\n",
            "test \"after\" {}\n",
        ),
    );
    let output = scratch.run(&["test", "main.align"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        output.stdout,
        concat!(
            "FAIL terminal_product::exit\n",
            "reason: exited with status 7; completion record: length\n",
            "FAIL terminal_product::exec\n",
            "reason: exited with status 8; completion record: length\n",
            "FAIL terminal_product::abort\n",
            "reason: exited with status 1; completion record: length\n",
            "FAIL terminal_product::error\n",
            "reason: returned Error.Code(-9)\n",
            "test result: FAILED. 1 passed; 4 failed\n",
        )
        .as_bytes(),
        "{output:?}"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn graceful_signal_cleans_child_and_stage_then_exits_numerically() {
    if !available() {
        return;
    }
    let scratch = Scratch::new("signal");
    scratch.write(
        "main.align",
        concat!(
            "module signal_test\n",
            "import std.fs\n",
            "test \"slow\" {\n",
            "  fs.write_file(\"started\", \"yes\")?\n",
            "  mut i := 0\n",
            "  loop {\n",
            "    i = i + 1\n",
            "    if i == 100000000000 { break }\n",
            "  }\n",
            "}\n",
        ),
    );
    for (name, signal, expected_status) in [
        ("SIGHUP", libc::SIGHUP, 129),
        ("SIGINT", libc::SIGINT, 130),
        ("SIGQUIT", libc::SIGQUIT, 131),
        ("SIGTERM", libc::SIGTERM, 143),
    ] {
        let _ = std::fs::remove_file(scratch.0.join("started"));
        let mut child = Command::new(env!("CARGO_BIN_EXE_alignc"))
            .args(["test", "main.align", "--timeout-ns=900000000000"])
            .current_dir(&scratch.0)
            .env("ALIGNC_CACHE", scratch.0.join("cache"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn signal owner");
        let pid = child.id();
        let prefix = format!(".align-test-exe-{pid}-");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stage = None;
        loop {
            let found = std::fs::read_dir(std::env::temp_dir())
                .expect("read temp dir")
                .filter_map(Result::ok)
                .find(|entry| {
                    entry.file_name().to_string_lossy().starts_with(&prefix)
                        && entry.path().join("tests").is_file()
                })
                .map(|entry| entry.path());
            if found.is_some() {
                stage = found;
            }
            if scratch.0.join("started").is_file() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                panic!("test executable stage was not materialized for {name}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let stage = stage.expect("stage existed when child wrote its marker");
        let object_prefix = format!(".align-test-obj-{pid}-");
        assert!(
            std::fs::read_dir(std::env::temp_dir())
                .expect("read temp dir for object-stage discharge")
                .filter_map(Result::ok)
                .all(|entry| !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&object_prefix)),
            "a compiler object stage survived into the first test row"
        );
        let signaled = Instant::now();
        assert_eq!(unsafe { libc::kill(pid as i32, signal) }, 0);
        let output = child.wait_with_output().expect("wait signal owner");
        let graceful = signaled.elapsed();
        assert!(
            graceful >= Duration::from_millis(240) && graceful < Duration::from_secs(5),
            "{name} graceful forwarding window drifted: {graceful:?}"
        );
        assert_eq!(
            output.status.code(),
            Some(expected_status),
            "{name} was not a numeric exit: {output:?}"
        );
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        assert!(
            !stage.exists(),
            "test executable stage survived terminal cleanup"
        );
        assert!(!Path::new(&stage).exists());
    }
}
