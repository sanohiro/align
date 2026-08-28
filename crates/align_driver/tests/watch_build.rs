use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("align-watch-build-{}-{id}", std::process::id()));
        std::fs::create_dir(&path).expect("create temp directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn alignc() -> Command {
    Command::new(env!("CARGO_BIN_EXE_alignc"))
}

fn source(value: i32) -> String {
    format!("fn main() -> i32 {{\n  return {value}\n}}\n")
}

#[cfg(unix)]
#[test]
fn observed_api_resolves_imports_from_a_non_utf8_entry_directory() {
    use align_driver::{ObservedPerUnitBuild, build_path_per_unit_observed};
    use std::os::unix::ffi::OsStringExt;

    let temp = TempDir::new();
    let root = temp
        .0
        .join(std::ffi::OsString::from_vec(b"project-\xff".to_vec()));
    std::fs::create_dir(&root).expect("create non-UTF-8 project directory");
    let entry = root.join("main.align");
    let dependency = root.join("dep.align");
    std::fs::write(&entry, "import dep\nfn main() -> i32 = dep.value()\n").expect("write entry");
    std::fs::write(&dependency, "module dep\npub fn value() -> i32 = 7\n")
        .expect("write dependency");
    let mut source_map = align_span::SourceMap::new();
    match build_path_per_unit_observed(&mut source_map, &entry) {
        ObservedPerUnitBuild::Walk { walk, inputs } => {
            assert!(!walk.diags.has_errors());
            assert!(
                inputs
                    .inputs()
                    .iter()
                    .any(|input| input.path() == dependency)
            );
        }
        ObservedPerUnitBuild::ObservationFailed { error } => {
            panic!("observation failed: {error}")
        }
        ObservedPerUnitBuild::SourceFailed { .. } => panic!("entry source failed"),
    }
}

fn spawn_watch(root: &Path) -> (Child, Receiver<String>) {
    spawn_watch_entry(root, "main.align", None)
}

fn spawn_watch_with_path(root: &Path, path: Option<&std::ffi::OsStr>) -> (Child, Receiver<String>) {
    spawn_watch_entry(root, "main.align", path)
}

fn spawn_watch_entry(
    root: &Path,
    entry: &str,
    path: Option<&std::ffi::OsStr>,
) -> (Child, Receiver<String>) {
    spawn_watch_args(root, entry, path, &[])
}

fn spawn_watch_args(
    root: &Path,
    entry: &str,
    path: Option<&std::ffi::OsStr>,
    extra: &[&str],
) -> (Child, Receiver<String>) {
    let mut command = alignc();
    command
        .current_dir(root)
        .args(["--watch", "build", entry])
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let mut child = command.spawn().expect("spawn watch compiler");
    let stderr = child.stderr.take().expect("stderr pipe");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            if tx
                .send(line.unwrap_or_else(|error| format!("<read error: {error}>")))
                .is_err()
            {
                break;
            }
        }
    });
    (child, rx)
}

fn wait_for(rx: &Receiver<String>, needle: &str) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut lines = Vec::new();
    loop {
        let timeout = deadline.saturating_duration_since(Instant::now());
        assert!(
            !timeout.is_zero(),
            "timed out waiting for {needle:?}; lines={lines:#?}"
        );
        match rx.recv_timeout(timeout) {
            Ok(line) => {
                let found = line.contains(needle);
                lines.push(line);
                if found {
                    return lines;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                panic!("timed out waiting for {needle:?}; lines={lines:#?}")
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("watch process closed before {needle:?}; lines={lines:#?}")
            }
        }
    }
}

fn stop(child: Child) -> (std::process::ExitStatus, Vec<u8>) {
    send_signal(&child, libc::SIGTERM);
    finish_stopped(child)
}

fn send_signal(child: &Child, signal: i32) {
    let pid = i32::try_from(child.id()).expect("pid fits i32");
    // SAFETY: the pid belongs to the still-owned watch child.
    assert_eq!(unsafe { libc::kill(pid, signal) }, 0);
}

fn finish_stopped(mut child: Child) -> (std::process::ExitStatus, Vec<u8>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll watch child") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("watch child did not stop within ten seconds");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .expect("stdout pipe")
        .read_to_end(&mut stdout)
        .expect("read stdout");
    (status, stdout)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn wrapper_path(temp: &TempDir, body: &str) -> std::ffi::OsString {
    use std::os::unix::fs::PermissionsExt;
    let bin = temp.0.join("bin");
    std::fs::create_dir(&bin).expect("create wrapper directory");
    let wrapper = bin.join("cc");
    std::fs::write(&wrapper, body).expect("write cc wrapper");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
        .expect("make cc wrapper executable");
    let mut path = bin.into_os_string();
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    path
}

fn real_cc() -> String {
    let output = Command::new("sh")
        .args(["-c", "command -v cc"])
        .output()
        .expect("locate cc");
    assert!(output.status.success(), "cc is required for watch tests");
    String::from_utf8(output.stdout)
        .expect("cc path is UTF-8")
        .trim()
        .to_string()
}

#[test]
fn one_process_builds_edit_and_revert_then_stops_cleanly() {
    let temp = TempDir::new();
    let entry = temp.0.join("main.align");
    std::fs::write(&entry, source(1)).expect("write source");
    let (child, lines) = spawn_watch(&temp.0);
    wait_for(&lines, "revision 1 ready");
    std::fs::write(&entry, source(2)).expect("edit source");
    wait_for(&lines, "revision 2 ready");
    std::fs::write(&entry, source(1)).expect("revert source");
    wait_for(&lines, "revision 3 ready");
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(
        stdout.is_empty(),
        "watch protocol must not use stdout: {stdout:?}"
    );
    assert_eq!(
        Command::new(temp.0.join("main"))
            .status()
            .expect("run output")
            .code(),
        Some(1)
    );
}

#[test]
fn failed_revision_keeps_watching_and_recovers() {
    let temp = TempDir::new();
    let entry = temp.0.join("main.align");
    std::fs::write(&entry, source(0)).expect("write source");
    let (child, lines) = spawn_watch(&temp.0);
    wait_for(&lines, "revision 1 ready");
    std::fs::write(&entry, "fn main( {\n").expect("write malformed source");
    wait_for(&lines, "revision 2 failed");
    assert_eq!(
        Command::new(temp.0.join("main"))
            .status()
            .expect("run last-good output")
            .code(),
        Some(0)
    );
    std::fs::write(&entry, source(3)).expect("repair source");
    wait_for(&lines, "revision 3 ready");
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
    assert_eq!(
        Command::new(temp.0.join("main"))
            .status()
            .expect("run output")
            .code(),
        Some(3)
    );
}

#[test]
fn diagnostic_paths_are_encoded_once_inside_the_record_protocol() {
    let temp = TempDir::new();
    let root = temp.0.join("space %");
    std::fs::create_dir(&root).expect("create special path");
    std::fs::write(root.join("main.align"), "fn main( {\n").expect("write malformed source");
    let (child, lines) = spawn_watch(&root);
    let failed = wait_for(&lines, "revision 1 failed");
    assert!(
        failed
            .iter()
            .any(|line| line.contains("space%252520%252525/main.align")),
        "single-encoded diagnostic path absent: {failed:#?}"
    );
    assert!(
        failed.iter().all(|line| !line.contains("space%25252520")),
        "diagnostic path was encoded twice: {failed:#?}"
    );
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
}

#[test]
fn imported_diagnostic_paths_use_the_watch_path_codec() {
    let temp = TempDir::new();
    let root = temp.0.join("space %");
    std::fs::create_dir(&root).expect("create special path");
    std::fs::write(
        root.join("main.align"),
        "import dep\nfn main() -> i32 = dep.value()\n",
    )
    .expect("write entry source");
    let (child, lines) = spawn_watch(&root);
    let missing = wait_for(&lines, "revision 1 failed");
    assert!(
        missing
            .iter()
            .any(|line| line.contains("space%252520%252525/dep.align")),
        "single-encoded missing-import path absent: {missing:#?}"
    );

    std::fs::write(
        root.join("dep.align"),
        "module wrong\npub fn value() -> i32 = 1\n",
    )
    .expect("write mismatched import");
    let mismatch = wait_for(&lines, "revision 2 failed");
    assert!(
        mismatch
            .iter()
            .any(|line| line.contains("space%252520%252525/dep.align")),
        "single-encoded module-declaration path absent: {mismatch:#?}"
    );

    std::fs::write(root.join("dep.align"), "module dep\nfn value( {\n")
        .expect("write malformed import");
    let malformed = wait_for(&lines, "revision 3 failed");
    assert!(
        malformed
            .iter()
            .any(|line| line.contains("space%252520%252525/dep.align")),
        "single-encoded SourceMap path absent: {malformed:#?}"
    );
    assert!(
        missing
            .iter()
            .chain(&mismatch)
            .chain(&malformed)
            .all(|line| !line.contains("space%25252520")),
        "imported diagnostic path was encoded twice"
    );
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
}

#[test]
fn watch_cache_stats_preserve_frontend_labels_and_stage_summaries() {
    let temp = TempDir::new();
    std::fs::write(temp.0.join("main.align"), source(0)).expect("write source");
    let (child, lines) = spawn_watch_args(&temp.0, "main.align", None, &["--cache-stats"]);
    let ready = wait_for(&lines, "revision 1 ready");
    assert!(
        ready.iter().any(|line| line.contains("main frontend ")),
        "frontend outcome label absent: {ready:#?}"
    );
    assert!(
        ready
            .iter()
            .any(|line| line.contains("frontend:") && line.contains(" hit,")),
        "frontend summary absent: {ready:#?}"
    );
    assert!(
        ready
            .iter()
            .any(|line| line.contains("unit(s):") && line.contains(" hit,")),
        "codegen summary drifted from one-shot output: {ready:#?}"
    );
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
}

#[test]
fn pgo_instrument_watch_names_the_profile_destination_before_ready() {
    let target = align_driver::BuildTarget::Native;
    if align_driver::profile_runtime_archive(&target).is_err() {
        return;
    }
    let temp = TempDir::new();
    std::fs::write(temp.0.join("main.align"), source(0)).expect("write source");
    let (child, lines) = spawn_watch_args(
        &temp.0,
        "main.align",
        None,
        &["--profile", "release", "--pgo-instrument"],
    );
    let ready = wait_for(&lines, "revision 1 ready");
    assert!(
        ready.iter().any(|line| {
            line.contains("record notice")
                && line.contains("--pgo-instrument:")
                && line.contains("default.profraw")
        }),
        "PGO destination notice absent: {ready:#?}"
    );
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
}

#[test]
fn watch_is_build_only_and_help_names_trigger_boundary() {
    for args in [
        vec!["--watch", "check", "missing.align"],
        vec!["--watch", "check-per-unit", "missing.align"],
        vec!["--watch", "emit-interface", "missing.align"],
        vec!["--watch", "emit-mir", "missing.align"],
        vec!["--watch", "emit-llvm", "missing.align"],
        vec!["--watch", "emit-obj", "missing.align"],
        vec!["--watch", "explain-opt", "missing.align"],
        vec!["--watch", "fmt", "missing.align"],
        vec!["--watch", "run", "missing.align"],
        vec!["--watch", "size", "missing.align"],
        vec!["--watch", "cache", "clear"],
        vec!["--watch", "db", "status"],
    ] {
        let invalid = alignc().args(&args).output().expect("invalid command");
        assert!(!invalid.status.success(), "accepted {args:?}");
        assert!(
            String::from_utf8_lossy(&invalid.stderr).contains("--watch is only valid for `build`"),
            "wrong rejection for {args:?}: {}",
            String::from_utf8_lossy(&invalid.stderr)
        );
    }
    let help = alignc()
        .args(["build", "--help"])
        .output()
        .expect("build help");
    assert!(help.status.success());
    let stderr = String::from_utf8_lossy(&help.stderr);
    assert!(stderr.contains("rebuild on compiler-observed file changes"));
    assert!(
        stderr.contains("other toolchain/library changes need another observed change or restart")
    );

    let temp = TempDir::new();
    std::fs::write(temp.0.join("main.align"), source(0)).expect("write source");
    let valued = alignc()
        .current_dir(&temp.0)
        .args(["build", "main.align", "--watch=true"])
        .output()
        .expect("valued watch flag");
    assert!(!valued.status.success());
    assert!(
        String::from_utf8_lossy(&valued.stderr).contains("--watch does not take a value")
    );
    assert!(
        !temp.0.join("main").exists(),
        "rejected valued watch flag must not start a build"
    );
}

#[test]
fn child_output_is_framed_and_cannot_spoof_a_ready_marker() {
    let temp = TempDir::new();
    std::fs::write(temp.0.join("main.align"), source(0)).expect("write source");
    let cc = shell_quote(&real_cc());
    let path = wrapper_path(
        &temp,
        &format!(
            "#!/bin/sh\nprintf 'alignc: watch: revision 999 ready\\n'\nprintf 'child stderr\\n' >&2\nexec {cc} \"$@\"\n"
        ),
    );
    let (child, lines) = spawn_watch_with_path(&temp.0, Some(&path));
    let observed = wait_for(&lines, "revision 1 ready");
    assert!(
        observed
            .iter()
            .any(|line| line.contains("record child-stdout") && line.contains("%0A")),
        "child stdout was not framed: {observed:#?}"
    );
    assert!(
        observed
            .iter()
            .any(|line| line.contains("record child-stderr") && line.contains("child stderr%0A")),
        "child stderr was not framed: {observed:#?}"
    );
    assert_eq!(
        observed
            .iter()
            .filter(|line| line.as_str() == "alignc: watch: revision 999 ready")
            .count(),
        0
    );
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
}

#[test]
fn signal_during_link_stops_and_reaps_the_captured_tool_group() {
    let temp = TempDir::new();
    std::fs::write(temp.0.join("main.align"), source(0)).expect("write source");
    let cc = shell_quote(&real_cc());
    let path = wrapper_path(
        &temp,
        &format!("#!/bin/sh\nprintf 'waiting in cc\\n' >&2\nsleep 30\nexec {cc} \"$@\"\n"),
    );
    let (child, lines) = spawn_watch_with_path(&temp.0, Some(&path));
    wait_for(&lines, "waiting in cc%0A");
    let started = Instant::now();
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(stdout.is_empty());
    let residue = std::fs::read_dir(&temp.0)
        .expect("read temp directory")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.contains("align-watch"))
        .collect::<Vec<_>>();
    assert!(residue.is_empty(), "watch stages leaked: {residue:?}");
}

#[test]
fn every_supported_signal_stops_an_active_link_with_the_conventional_exit() {
    for (signal, name, code) in [
        (libc::SIGHUP, "SIGHUP", 129),
        (libc::SIGINT, "SIGINT", 130),
        (libc::SIGQUIT, "SIGQUIT", 131),
        (libc::SIGTERM, "SIGTERM", 143),
    ] {
        let temp = TempDir::new();
        std::fs::write(temp.0.join("main.align"), source(0)).expect("write source");
        let cc = shell_quote(&real_cc());
        let path = wrapper_path(
            &temp,
            &format!("#!/bin/sh\nprintf 'waiting in cc\\n' >&2\nsleep 30\nexec {cc} \"$@\"\n"),
        );
        let (child, lines) = spawn_watch_with_path(&temp.0, Some(&path));
        wait_for(&lines, "waiting in cc%0A");
        send_signal(&child, signal);
        wait_for(&lines, &format!("stopped by {name}"));
        let (status, stdout) = finish_stopped(child);
        assert_eq!(status.code(), Some(code), "wrong exit for {name}");
        assert!(stdout.is_empty());
        let residue = std::fs::read_dir(&temp.0)
            .expect("read temp directory")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.contains("align-watch"))
            .collect::<Vec<_>>();
        assert!(
            residue.is_empty(),
            "watch stages leaked after {name}: {residue:?}"
        );
    }
}

#[test]
fn output_alias_failure_repairs_without_an_extra_source_edit() {
    let temp = TempDir::new();
    let entry = temp.0.join("main.align");
    let output = temp.0.join("main");
    std::fs::write(&entry, source(0)).expect("write source");
    std::fs::hard_link(&entry, &output).expect("make output alias input");
    let (child, lines) = spawn_watch(&temp.0);
    let failed = wait_for(&lines, "revision 1 failed");
    assert!(
        failed
            .iter()
            .any(|line| line.contains("aliases observed input")),
        "alias diagnostic absent: {failed:#?}"
    );
    std::fs::remove_file(&output).expect("remove output alias");
    wait_for(&lines, "revision 2 ready");
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
    assert_eq!(
        Command::new(output)
            .status()
            .expect("run repaired output")
            .code(),
        Some(0)
    );
}

#[test]
fn same_bytes_regular_replacement_rearms_without_a_revision() {
    let temp = TempDir::new();
    let entry = temp.0.join("main.align");
    let replacement = temp.0.join("replacement.align");
    let original = source(0);
    std::fs::write(&entry, &original).expect("write source");
    let (child, lines) = spawn_watch(&temp.0);
    wait_for(&lines, "revision 1 ready");
    std::fs::write(&replacement, &original).expect("write same-byte replacement");
    std::fs::rename(&replacement, &entry).expect("replace source inode");
    match lines.recv_timeout(Duration::from_millis(750)) {
        Err(RecvTimeoutError::Timeout) => {}
        Ok(line) => panic!("same-byte inode replacement emitted a protocol record: {line}"),
        Err(RecvTimeoutError::Disconnected) => panic!("watch process exited after replacement"),
    }
    std::fs::write(&entry, source(2)).expect("write semantic edit");
    wait_for(&lines, "revision 2 ready");
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn same_bytes_symlink_retarget_starts_a_revision() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new();
    let first = temp.0.join("first.align");
    let second = temp.0.join("second.align");
    let entry = temp.0.join("main.align");
    let original = source(0);
    std::fs::write(&first, &original).expect("write first source");
    std::fs::write(&second, &original).expect("write second source");
    symlink(&first, &entry).expect("create entry symlink");
    let (child, lines) = spawn_watch(&temp.0);
    wait_for(&lines, "revision 1 ready");
    std::fs::remove_file(&entry).expect("remove entry symlink");
    symlink(&second, &entry).expect("retarget entry symlink");
    wait_for(&lines, "revision 2 ready");
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
}

#[test]
fn replaced_directory_rearms_the_new_native_generation() {
    let temp = TempDir::new();
    let current = temp.0.join("current");
    std::fs::create_dir(&current).expect("create current directory");
    std::fs::write(current.join("main.align"), source(0)).expect("write source");
    let (child, lines) = spawn_watch_entry(&temp.0, "current/main.align", None);
    wait_for(&lines, "revision 1 ready");

    let replacement = temp.0.join("replacement");
    std::fs::create_dir(&replacement).expect("create replacement directory");
    std::fs::write(replacement.join("main.align"), source(0)).expect("write replacement source");
    std::fs::rename(&current, temp.0.join("retired")).expect("retire watched directory");
    std::fs::rename(&replacement, &current).expect("install replacement directory");
    wait_for(&lines, "revision 2 ready");

    std::fs::write(current.join("main.align"), source(4)).expect("edit replacement source");
    wait_for(&lines, "revision 3 ready");
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
}

#[test]
fn imported_source_disappearance_and_recovery_are_observed() {
    let temp = TempDir::new();
    let entry = temp.0.join("main.align");
    let dependency = temp.0.join("dep.align");
    std::fs::write(
        &entry,
        "module main\nimport dep\nfn main() -> i32 = dep.value()\n",
    )
    .expect("write entry");
    std::fs::write(&dependency, "module dep\npub fn value() -> i32 = 1\n")
        .expect("write dependency");
    let (child, lines) = spawn_watch(&temp.0);
    wait_for(&lines, "revision 1 ready");
    std::fs::remove_file(&dependency).expect("remove dependency");
    wait_for(&lines, "revision 2 failed");
    std::fs::write(&dependency, "module dep\npub fn value() -> i32 = 6\n")
        .expect("restore dependency");
    wait_for(&lines, "revision 3 ready");
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
    assert_eq!(
        Command::new(temp.0.join("main"))
            .status()
            .expect("run output")
            .code(),
        Some(6)
    );
}

#[test]
fn thin_lto_watch_reuses_the_observed_per_unit_route() {
    let temp = TempDir::new();
    let entry = temp.0.join("main.align");
    let dependency = temp.0.join("dep.align");
    std::fs::write(&entry, "import dep\nfn main() -> i32 = dep.value()\n").expect("write source");
    std::fs::write(&dependency, "module dep\npub fn value() -> i32 = 1\n")
        .expect("write dependency");
    let (child, lines) = spawn_watch_args(
        &temp.0,
        "main.align",
        None,
        &["--thin-lto", "--cache-stats"],
    );
    let ready = wait_for(&lines, "revision 1 ready");
    assert!(
        ready.iter().any(|line| line.contains("prelink:"))
            && ready.iter().any(|line| line.contains("backend:")),
        "ThinLTO cache stage summaries absent: {ready:#?}"
    );
    std::fs::write(&dependency, "module dep\npub fn value() -> i32 = 5\n")
        .expect("edit dependency");
    wait_for(&lines, "revision 2 ready");
    let (status, stdout) = stop(child);
    assert_eq!(status.code(), Some(143));
    assert!(stdout.is_empty());
    assert_eq!(
        Command::new(temp.0.join("main"))
            .status()
            .expect("run output")
            .code(),
        Some(5)
    );
}
