//! Request 60: executable selection, validation order, and every linking route.
#![cfg(unix)]

use std::fs::{self, File};
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::path::Path;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

struct Process(Option<Child>, Instant);
impl Process {
    fn spawn(command: &mut Command) -> Self {
        let child = command.process_group(0).spawn().expect("spawn owner child");
        Self(Some(child), Instant::now() + Duration::from_secs(60))
    }

    fn wait(&mut self) -> ExitStatus {
        loop {
            assert!(Instant::now() < self.1, "child exceeded owner deadline");
            match self.0.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    self.0.take();
                    return status;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => panic!("poll owner child: {error}"),
            }
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let pid = i32::try_from(child.id()).expect("child pid");
            // SAFETY: this owned child was spawned as the leader of a fresh process group.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_alignc"));
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("LC_ALL", "C")
        .env("TMPDIR", root)
        .env("TZ", "UTC")
        .stdout(File::create(root.join("stdout")).unwrap())
        .stderr(File::create(root.join("stderr")).unwrap());
    command
}

fn run(root: &Path, args: &[&str]) -> (ExitStatus, String, String) {
    let status = Process::spawn(command(root).args(args)).wait();
    (
        status,
        fs::read_to_string(root.join("stdout")).unwrap(),
        fs::read_to_string(root.join("stderr")).unwrap(),
    )
}

fn driver(root: &Path) -> String {
    let path = root.join("selected driver");
    // /usr/bin/cc is an explicit fixture tool, not an ambient search. The wrapper joins its
    // only writer through exec, retaining the production trusted-tool lifecycle contract.
    script(
        &path,
        "printf 'selected\\n' >> selected.log\nexec /usr/bin/cc -B/usr/bin \"$@\"",
    );
    path.to_str().unwrap().to_owned()
}

#[test]
fn explicit_driver_links_every_verb_without_path_and_relinks_cache_hits() {
    let stage = align_driver::ArtifactStage::temp("cc-verbs").unwrap();
    let root = stage.path();
    let cc = driver(root);
    fs::write(
        root.join("main.align"),
        "import core.test\nfn main() { print(17) }\ntest \"ok\" { test.expect(true) }\n",
    )
    .unwrap();
    let cases = [
        vec!["--cc", &cc, "build", "main.align", "--profile", "dev"],
        vec!["run", "main.align", "--cc", &cc, "--profile", "dev"],
        vec!["size", "main.align", "--cc", &cc, "--profile", "dev"],
        vec!["test", "main.align", "--cc", &cc],
        vec!["build", "main.align", "--cc", &cc, "--thin-lto"],
        vec!["build", "main.align", "--cc", &cc, "--thin-lto"],
        vec!["build", "main.align", "--cc", &cc, "--pgo-instrument"],
    ];
    for (index, args) in cases.iter().enumerate() {
        let (status, stdout, stderr) = run(root, args);
        assert!(status.success(), "{args:?}: {stdout}\n{stderr}");
        assert_eq!(
            fs::read_to_string(root.join("selected.log"))
                .unwrap()
                .lines()
                .count(),
            index + 1
        );
    }
    let trap = root.join("trap");
    fs::create_dir(&trap).unwrap();
    script(&trap.join("cc"), "printf wrong > wrong.log\nexit 91");
    let mut child = command(root);
    child
        .env("PATH", &trap)
        .args(["build", "main.align", "--profile", "dev", "--cc", &cc]);
    assert!(Process::spawn(&mut child).wait().success());
    assert!(!root.join("wrong.log").exists());
    // With no explicit override the existing default still consults PATH.
    let mut child = command(root);
    child
        .env("PATH", &trap)
        .args(["build", "main.align", "--profile", "dev"]);
    assert!(!Process::spawn(&mut child).wait().success());
    assert!(root.join("wrong.log").exists());
}

#[test]
fn invalid_driver_rejects_before_source_cache_output_or_execution() {
    let stage = align_driver::ArtifactStage::temp("cc-invalid").unwrap();
    let root = stage.path();
    let cc = driver(root);
    let absent = root.join("absent");
    let plain = root.join("plain");
    fs::write(&plain, "not executable").unwrap();
    let cases = [
        (vec!["build", "missing.align", "--cc"], "requires"),
        (vec!["build", "missing.align", "--cc="], "requires"),
        (
            vec!["build", "missing.align", "--cc", "relative"],
            "absolute",
        ),
        (
            vec!["build", "missing.align", "--cc", absent.to_str().unwrap()],
            "cannot inspect",
        ),
        (
            vec!["build", "missing.align", "--cc", root.to_str().unwrap()],
            "regular file",
        ),
        (
            vec!["build", "missing.align", "--cc", plain.to_str().unwrap()],
            "not executable",
        ),
        (
            vec!["build", "missing.align", "--cc", &cc, "--cc", &cc],
            "only once",
        ),
        (
            vec!["build", "missing.align", "--cc", "--pgo-instrument", &cc],
            "requires",
        ),
        (
            vec!["build", "missing.align", "--pgo-use", "--cc", &cc],
            "--pgo-use requires",
        ),
        (vec!["build", "--help", "--cc", "relative"], "absolute"),
    ];
    for (args, expected) in cases {
        let (status, _, stderr) = run(root, &args);
        assert!(!status.success(), "{args:?}");
        assert!(
            stderr.contains(expected) && !stderr.contains("cannot read"),
            "{args:?}: {stderr}"
        );
    }
    for verb in [
        "check",
        "check-per-unit",
        "emit-interface",
        "emit-mir",
        "emit-llvm",
        "emit-obj",
        "explain-opt",
        "fmt",
        "cache",
        "db",
        "--version",
    ] {
        for args in [
            vec![verb, "missing.align", "--cc", "relative"],
            vec!["--cc", "relative", verb, "missing.align"],
        ] {
            let (status, _, stderr) = run(root, &args);
            assert!(
                !status.success() && stderr.contains("only valid"),
                "{args:?}: {stderr}"
            );
        }
    }
    assert!(!root.join("selected.log").exists());
    assert!(!root.join(".cache").exists());
    assert!(!root.join("missing").exists());
    use std::os::unix::ffi::OsStringExt;
    let mut child = command(root);
    child
        .args(["build", "missing.align", "--cc"])
        .arg(std::ffi::OsString::from_vec(b"/bad\xff".to_vec()));
    assert!(!Process::spawn(&mut child).wait().success());
    assert!(fs::read_to_string(root.join("stderr"))
        .unwrap()
        .contains("UTF-8"));
    assert!(align_driver::CDriver::explicit("/bad\0path".into()).is_err());
}

#[test]
fn program_suffix_is_not_parsed_as_compiler_configuration() {
    let stage = align_driver::ArtifactStage::temp("cc-suffix").unwrap();
    let root = stage.path();
    let cc = driver(root);
    fs::write(root.join("main.align"), "fn main(args: array<str>) -> Result<(), Error> { print(args[1]); print(args[2]); return Ok(()) }\n").unwrap();
    let equal = format!("--cc={cc}");
    let (status, stdout, stderr) = run(
        root,
        &[
            "run",
            "main.align",
            &equal,
            "--profile",
            "dev",
            "--",
            "--pgo-use",
            "--cc=/not/a/compiler",
        ],
    );
    assert!(status.success(), "{stderr}");
    assert_eq!(stdout, "--pgo-use\n--cc=/not/a/compiler\n");
}

#[test]
fn watch_reuses_the_selected_driver_for_each_revision() {
    for options in [
        vec!["--profile", "dev"],
        vec!["--thin-lto"],
        vec!["--pgo-instrument"],
    ] {
        let stage = align_driver::ArtifactStage::temp("cc-watch").unwrap();
        let root = stage.path();
        let cc = driver(root);
        fs::write(root.join("main.align"), "fn main() { print(1) }\n").unwrap();
        let mut child = Process::spawn(
            command(root)
                .args(["build", "main.align", "--watch", "--cc", &cc])
                .args(&options),
        );
        for (count, marker) in [(1, "revision 1 ready"), (2, "revision 2 ready")] {
            loop {
                assert!(
                    Instant::now() < child.1,
                    "watch deadline: {}",
                    fs::read_to_string(root.join("stderr")).unwrap()
                );
                let log = fs::read_to_string(root.join("stderr")).unwrap();
                if log.contains(marker)
                    && fs::read_to_string(root.join("selected.log"))
                        .unwrap_or_default()
                        .lines()
                        .count()
                        >= count
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if count == 1 {
                fs::write(root.join("main.align"), "fn main() { print(2) }\n").unwrap();
            }
        }
        let pid = i32::try_from(child.0.as_ref().unwrap().id()).unwrap();
        // SAFETY: signal the still-owned watch process, which performs orderly watcher cleanup.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        let _ = child.wait();
        assert_eq!(
            fs::read_to_string(root.join("selected.log"))
                .unwrap()
                .lines()
                .count(),
            2
        );
    }
}

#[test]
fn nonrunning_verbs_reject_the_program_suffix_without_side_effects() {
    let stage = align_driver::ArtifactStage::temp("cc-nonrun-suffix").unwrap();
    let root = stage.path();
    let original = "fn main() {     print(1) }\n";
    fs::write(root.join("main.align"), original).unwrap();
    for arguments in [
        vec!["fmt", "main.align", "--", "--write"],
        vec!["fmt", "main.align", "--", "-w"],
        vec!["emit-llvm", "main.align", "--", "--stage", "optimized"],
        vec!["emit-obj", "main.align", "--", "out.o"],
        vec!["explain-opt", "main.align", "--", "--verbose"],
        vec!["build", "main.align", "--", "--cc=/not/a/compiler"],
        vec!["size", "main.align", "--"],
        vec!["test", "main.align", "--", "extra"],
        vec!["cache", "clear", "--"],
        vec!["db", "prepare", "main.align", "--", "--cc=/not/a/compiler"],
    ] {
        let (status, stdout, stderr) = run(root, &arguments);
        assert!(
            !status.success() && stdout.is_empty(),
            "{arguments:?}: {stdout}\n{stderr}"
        );
        assert!(
            stderr.contains("delimiter -- is only valid for `run`"),
            "{arguments:?}: {stderr}"
        );
        assert_eq!(
            fs::read_to_string(root.join("main.align")).unwrap(),
            original
        );
        assert!(!root.join("main").exists());
        assert!(!root.join("out.o").exists());
        assert!(!root.join(".cache").exists());
    }
}

#[test]
fn selection_retains_symlink_spelling_and_never_recovers_a_vanished_driver() {
    let stage = align_driver::ArtifactStage::temp("cc-identity").unwrap();
    let root = stage.path();
    let cc = driver(root);
    let alias = root.join("alias driver");
    std::os::unix::fs::symlink(&cc, &alias).unwrap();
    let selected = align_driver::CDriver::explicit(alias.clone()).unwrap();
    assert_eq!(selected.program(), alias);
    fs::remove_file(&alias).unwrap();
    let error = align_driver::link_objects(
        &selected,
        &[],
        &root.join("never-published"),
        &[],
        align_driver::Profile::Dev,
    )
    .unwrap_err();
    assert!(
        error.contains("cannot launch") && error.contains(alias.to_str().unwrap()),
        "{error}"
    );
    assert!(!root.join("selected.log").exists());
    assert!(!root.join("never-published").exists());
}

mod residual_arguments {
    use super::*;

    const SOURCE: &str = "fn  main() { print(1) }\n";

    fn fixture() -> align_driver::ArtifactStage {
        let stage = align_driver::ArtifactStage::temp("cli-residual").expect("acquire fixture");
        fs::write(stage.path().join("sample.align"), SOURCE).unwrap();
        fs::create_dir_all(stage.path().join("cache/actions")).unwrap();
        fs::write(stage.path().join("cache/actions/marker"), b"retained").unwrap();
        stage
    }

    fn run_fixture(stage: &align_driver::ArtifactStage, args: &[&str]) -> std::process::Output {
        let root = stage.path();
        let status = Process::spawn(
            command(root)
                .args(args)
                .env("PATH", "/usr/bin:/bin")
                .env("ALIGNC_CACHE", root.join("cache")),
        )
        .wait();
        std::process::Output {
            status,
            stdout: fs::read(root.join("stdout")).unwrap(),
            stderr: fs::read(root.join("stderr")).unwrap(),
        }
    }

    fn invalid(stage: &align_driver::ArtifactStage, args: &[&str], error: &str) {
        let root = stage.path();
        let output = run_fixture(stage, args);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!("alignc: {error}\n"),
            "{args:?}"
        );
        assert_eq!(
            fs::read_to_string(root.join("sample.align")).unwrap(),
            SOURCE
        );
        assert_eq!(
            fs::read(root.join("cache/actions/marker")).unwrap(),
            b"retained"
        );
        for artifact in ["sample", "sample.o", "chosen.o", "desired-output"] {
            assert!(
                !root.join(artifact).exists(),
                "unexpected artifact {artifact}"
            );
        }
    }
    #[test]
    fn residual_errors_precede_native_source_and_artifact_work() {
        let fixture = fixture();
        for verb in [
            "check",
            "check-per-unit",
            "emit-interface",
            "emit-mir",
            "build",
            "size",
            "fmt",
            "explain-opt",
            "emit-obj",
            "emit-llvm",
        ] {
            for source in ["sample.align", "missing.align"] {
                invalid(
                    &fixture,
                    &[verb, source, "--misspelled"],
                    &format!("unexpected argument '--misspelled' for '{verb}'"),
                );
            }
        }
        for (args, error) in [
            (
                vec!["build", "sample.align", "-o", "desired-output"],
                "unexpected argument '-o' for 'build'",
            ),
            (
                vec!["build", "sample.align", "--watch", "--misspelled"],
                "unexpected argument '--misspelled' for 'build'",
            ),
            (
                vec!["build", "--help", "--misspelled"],
                "unexpected argument '--misspelled' for 'build'",
            ),
            (
                vec![
                    "build",
                    "sample.align",
                    "--cc",
                    "/missing/compiler",
                    "--misspelled",
                ],
                "unexpected argument '--misspelled' for 'build'",
            ),
            (
                vec!["fmt", "sample.align", "--write", "--misspelled"],
                "unexpected argument '--misspelled' for 'fmt'",
            ),
            (
                vec!["cache", "clear", "--misspelled"],
                "unexpected argument '--misspelled' for 'cache'",
            ),
            (
                vec!["cache", "unknown", "--misspelled"],
                "unknown `cache` subcommand `unknown` (expected: clear)",
            ),
            (
                vec!["emit-obj", "sample.align", "chosen.o", "extra.o"],
                "unexpected argument 'extra.o' for 'emit-obj'",
            ),
            (
                vec!["test", "sample.align", "extra.align"],
                "test accepts exactly one entry path",
            ),
            (vec!["test"], "test accepts exactly one entry path"),
            (
                vec!["check", "sample.align", "unexpected\nargument"],
                "unexpected argument 'unexpected\nargument' for 'check'",
            ),
            (
                vec!["emit-llvm", "missing.align", "--misspelled", "--stage=bad"],
                "unexpected argument '--misspelled' for 'emit-llvm'",
            ),
            (
                vec!["emit-llvm", "missing.align", "--stage=bad", "--misspelled"],
                "unknown --stage 'bad' (expected `raw` or `optimized`)",
            ),
            (
                vec!["emit-llvm", "missing.align", "--stage"],
                "unknown --stage '' (expected `raw` or `optimized`)",
            ),
            (
                vec!["emit-llvm", "missing.align", "--stage=bad\nstage"],
                "unknown --stage 'bad\nstage' (expected `raw` or `optimized`)",
            ),
            (
                vec!["check", "sample.align", "--profile=bad", "--misspelled"],
                "unknown --profile 'bad' (expected one of: dev, release, fast, small, tiny)",
            ),
            (
                vec!["build", "sample.align", "--", "--misspelled"],
                "program argument delimiter -- is only valid for `run`",
            ),
        ] {
            invalid(&fixture, &args, error);
        }
    }

    #[test]
    fn legal_local_options_and_program_arguments_remain_usable() {
        let fixture = fixture();
        for args in [
            vec!["check", "sample.align"],
            vec!["build", "--help"],
            vec!["fmt", "sample.align"],
            vec!["fmt", "sample.align", "-w", "--write"],
        ] {
            let output = run_fixture(&fixture, &args);
            assert!(output.status.success(), "{args:?}: {output:?}");
        }
        assert_eq!(
            std::fs::read_to_string(fixture.path().join("sample.align")).unwrap(),
            "fn main() { print(1) }\n"
        );
        if align_driver::backend_available() {
            let raw = run_fixture(&fixture, &["emit-llvm", "sample.align"]);
            assert!(raw.status.success(), "{raw:?}");
            let optimized = run_fixture(
                &fixture,
                &["emit-llvm", "sample.align", "--stage=optimized"],
            );
            assert!(optimized.status.success(), "{optimized:?}");
            assert_ne!(raw.stdout, optimized.stdout, "stage must change emitted IR");
            for (args, expected) in [
                (
                    vec![
                        "emit-llvm",
                        "sample.align",
                        "--stage=optimized",
                        "--stage",
                        "raw",
                    ],
                    &raw.stdout,
                ),
                (
                    vec![
                        "emit-llvm",
                        "sample.align",
                        "--stage",
                        "raw",
                        "--stage=optimized",
                    ],
                    &optimized.stdout,
                ),
            ] {
                let output = run_fixture(&fixture, &args);
                assert!(output.status.success(), "{args:?}: {output:?}");
                assert_eq!(&output.stdout, expected, "last stage wins: {args:?}");
            }
            for args in [
                vec!["explain-opt", "sample.align", "-v", "--verbose"],
                vec!["emit-obj", "sample.align", "chosen.o"],
            ] {
                let output = run_fixture(&fixture, &args);
                assert!(output.status.success(), "{args:?}: {output:?}");
            }
            assert!(fixture.path().join("chosen.o").is_file());
            std::fs::write(fixture.path().join("argv.align"),
            "fn main(args: array<str>) -> Result<(), Error> { print(args[1]); print(args[2]); return Ok(()) }\n").unwrap();
            for args in [
                vec!["run", "argv.align", "--misspelled", "value"],
                vec!["run", "argv.align", "--", "--profile", "fast"],
            ] {
                let output = run_fixture(&fixture, &args);
                assert!(output.status.success(), "{args:?}: {output:?}");
                let expected = if args.contains(&"--") {
                    b"--profile\nfast\n".as_slice()
                } else {
                    b"--misspelled\nvalue\n".as_slice()
                };
                assert_eq!(output.stdout, expected);
            }
        }
        let output = run_fixture(&fixture, &["cache", "clear"]);
        assert!(output.status.success(), "{output:?}");
        assert!(!fixture.path().join("cache/actions/marker").exists());
    }
}
