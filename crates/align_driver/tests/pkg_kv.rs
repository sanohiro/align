//! `pkg.kv` v1 owner: exact public/interface shape and deterministic RESP loopback acceptance.
//!
//! The runtime cases use only an in-process loopback peer. The peer compares complete request
//! bytes, scripts bounded replies, and requires client EOF after Drop or terminal retirement.

mod common;
use common::*;

use align_ast::ParamMode;
use align_interface::{
    Effect, Hash128, IResourceDef, IType, ITypeParam, ImportCompatibilityError,
    ReturnBorrowSummary, ReturnRegionSummary, deserialize, serialize, validate_for_import,
};
use align_mir::{DirectCall, Operand, Rvalue, Stmt};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
const CASE_TIMEOUT: Duration = Duration::from_secs(20);
const PIPE_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const LINK_CHILD_ENV: &str = "ALIGN_PKG_KV_LINK_CHILD";
const LINK_EXE_ENV: &str = "ALIGN_PKG_KV_LINK_EXE";
const LINK_OBJECT_COUNT_ENV: &str = "ALIGN_PKG_KV_LINK_OBJECT_COUNT";
const LINK_LIBRARY_COUNT_ENV: &str = "ALIGN_PKG_KV_LINK_LIBRARY_COUNT";

fn kv_source() -> &'static str {
    fixture("apps/kv/pkg/kv.align")
}

fn resource_source() -> &'static str {
    fixture("apps/kv/pkg/kv/internal/resource.align")
}

#[derive(Clone, Debug, Default)]
struct Captured {
    bytes: Vec<u8>,
    overflowed: bool,
    read_error: Option<String>,
}

struct DrainHandle {
    join: JoinHandle<()>,
    captured: Arc<Mutex<Captured>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Debug)]
struct DrainReport {
    captured: Captured,
    timed_out: bool,
    panicked: bool,
}

#[derive(Debug)]
struct CleanupReport {
    group_kill_error: Option<String>,
    direct_kill_error: Option<String>,
    reap: Result<std::process::ExitStatus, String>,
}

struct ChildGuard {
    child: Option<std::process::Child>,
}

impl ChildGuard {
    fn new(child: std::process::Child) -> Self {
        Self { child: Some(child) }
    }

    fn child_mut(&mut self) -> &mut std::process::Child {
        self.child.as_mut().expect("armed child guard")
    }

    fn finish_reaped(&mut self, deadline: Instant) -> Option<String> {
        let group_kill_error =
            kill_process_group(self.child.as_ref().expect("armed child guard"), deadline);
        self.child.take();
        group_kill_error
    }

    fn terminate_and_reap(&mut self, deadline: Instant) -> CleanupReport {
        let child = self.child.as_mut().expect("armed child guard");
        let (group_kill_error, direct_kill_error) = terminate_process_group(child, deadline);
        let reap = wait_reaped(child, deadline).map_err(|error| error.to_string());
        self.child.take();
        CleanupReport {
            group_kill_error,
            direct_kill_error,
            reap,
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        let deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
        let _ = terminate_process_group(child, deadline);
        let _ = wait_reaped(child, deadline);
    }
}

fn captured_snapshot(captured: &Arc<Mutex<Captured>>) -> Captured {
    captured
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn captured_snapshot_nonblocking(captured: &Arc<Mutex<Captured>>) -> Captured {
    match captured.try_lock() {
        Ok(captured) => captured.clone(),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner().clone(),
        Err(std::sync::TryLockError::WouldBlock) => Captured {
            read_error: Some("capture mutex remained busy at cleanup deadline".to_owned()),
            ..Captured::default()
        },
    }
}

fn drain_bounded<R: Read>(mut pipe: R, captured: &Arc<Mutex<Captured>>, cancel: &AtomicBool) {
    let mut chunk = [0_u8; 4096];
    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let count = match pipe.read(&mut chunk) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(error) => {
                captured
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .read_error = Some(error.to_string());
                break;
            }
        };
        if count == 0 {
            break;
        }
        let mut captured = captured
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(captured.bytes.len());
        let retained = remaining.min(count);
        captured.bytes.extend_from_slice(&chunk[..retained]);
        captured.overflowed |= retained != count;
    }
}

#[cfg(unix)]
fn make_pipe_nonblocking<R: std::os::fd::AsRawFd>(
    reader: &R,
    deadline: Instant,
) -> std::io::Result<()> {
    let fd = reader.as_raw_fd();
    // SAFETY: `fd` is the live descriptor borrowed from this owned child pipe. `F_GETFL` and
    // `F_SETFL` do not outlive the call or access Rust memory.
    let flags = loop {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags >= 0 {
            break flags;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline {
            continue;
        }
        return Err(error);
    };
    // SAFETY: same live descriptor; preserving the existing flags and adding `O_NONBLOCK` makes
    // the drain loop cancellable without changing pipe ownership.
    loop {
        if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline {
            continue;
        }
        return Err(error);
    }
}

#[cfg(not(unix))]
fn make_pipe_nonblocking<R>(_reader: &R, _deadline: Instant) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn start_drain<R>(reader: R, stream: &str, deadline: Instant) -> std::io::Result<DrainHandle>
where
    R: Read + Send + std::os::fd::AsRawFd + 'static,
{
    make_pipe_nonblocking(&reader, deadline)?;
    start_drain_thread(reader, stream)
}

#[cfg(not(unix))]
fn start_drain<R: Read + Send + 'static>(
    reader: R,
    stream: &str,
    deadline: Instant,
) -> std::io::Result<DrainHandle> {
    make_pipe_nonblocking(&reader, deadline)?;
    start_drain_thread(reader, stream)
}

fn start_drain_thread<R: Read + Send + 'static>(
    reader: R,
    stream: &str,
) -> std::io::Result<DrainHandle> {
    let captured = Arc::new(Mutex::new(Captured {
        bytes: Vec::new(),
        overflowed: false,
        read_error: None,
    }));
    let thread_capture = Arc::clone(&captured);
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = Arc::clone(&cancel);
    let join = std::thread::Builder::new()
        .name(format!("pkg-kv-{stream}-drain"))
        .spawn(move || drain_bounded(reader, &thread_capture, &thread_cancel))?;
    Ok(DrainHandle {
        join,
        captured,
        cancel,
    })
}

fn isolate_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

fn wait_reaped(
    child: &mut std::process::Child,
    deadline: Instant,
) -> std::io::Result<std::process::ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "child did not exit after process cleanup",
                ));
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline => {
            }
            Err(error) => return Err(error),
        }
    }
}

fn kill_process_group(child: &std::process::Child, deadline: Instant) -> Option<String> {
    #[cfg(unix)]
    let result = match i32::try_from(child.id()) {
        Ok(pid) => {
            // SAFETY: the child was placed in a fresh process group whose id is its positive pid.
            loop {
                let result = unsafe { libc::kill(-pid, libc::SIGKILL) };
                if result == 0 {
                    break None;
                }
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline {
                    continue;
                }
                break (error.raw_os_error() != Some(libc::ESRCH)).then(|| error.to_string());
            }
        }
        Err(error) => Some(format!("child pid is not representable as i32: {error}")),
    };
    #[cfg(not(unix))]
    let result = {
        let _ = child;
        None
    };

    result
}

fn terminate_process_group(
    child: &mut std::process::Child,
    deadline: Instant,
) -> (Option<String>, Option<String>) {
    let group_error = kill_process_group(child, deadline);
    let direct_error = loop {
        match child.kill() {
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline => {
            }
            Err(error) => break Some(error.to_string()),
            Ok(()) => break None,
        }
    };
    (group_error, direct_error)
}

fn collect_drains(
    stdout: Option<DrainHandle>,
    stderr: Option<DrainHandle>,
    deadline: Instant,
) -> (Option<DrainReport>, Option<DrainReport>) {
    while stdout
        .as_ref()
        .is_some_and(|drain| !drain.join.is_finished())
        || stderr
            .as_ref()
            .is_some_and(|drain| !drain.join.is_finished())
    {
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if stdout
        .as_ref()
        .is_some_and(|drain| !drain.join.is_finished())
    {
        stdout
            .as_ref()
            .expect("stdout drain")
            .cancel
            .store(true, Ordering::Relaxed);
    }
    if stderr
        .as_ref()
        .is_some_and(|drain| !drain.join.is_finished())
    {
        stderr
            .as_ref()
            .expect("stderr drain")
            .cancel
            .store(true, Ordering::Relaxed);
    }

    fn finish(drain: Option<DrainHandle>) -> Option<DrainReport> {
        let drain = drain?;
        let captured = Arc::clone(&drain.captured);
        let timed_out = !drain.join.is_finished();
        let panicked = if timed_out {
            drop(drain.join);
            false
        } else {
            drain.join.join().is_err()
        };
        Some(DrainReport {
            captured: if timed_out {
                captured_snapshot_nonblocking(&captured)
            } else {
                captured_snapshot(&captured)
            },
            timed_out,
            panicked,
        })
    }

    (finish(stdout), finish(stderr))
}

fn panic_after_cleanup(
    label: &str,
    reason: &str,
    cleanup: &CleanupReport,
    stdout: Option<DrainHandle>,
    stderr: Option<DrainHandle>,
    deadline: Instant,
) -> ! {
    let (stdout, stderr) = collect_drains(stdout, stderr, deadline);
    panic!(
        "{reason}; `{label}` group-kill error {:?}; direct-kill error {:?}; reap {:?}; stdout \
         {stdout:?}; stderr {stderr:?}",
        cleanup.group_kill_error, cleanup.direct_kill_error, cleanup.reap,
    );
}

fn finish_output(
    status: std::process::ExitStatus,
    stdout: DrainHandle,
    stderr: DrainHandle,
    label: &str,
    group_kill_error: Option<String>,
    deadline: Instant,
) -> std::process::Output {
    let (stdout, stderr) = collect_drains(Some(stdout), Some(stderr), deadline);
    let stdout = stdout.expect("stdout drain report");
    let stderr = stderr.expect("stderr drain report");
    assert!(
        group_kill_error.is_none()
            && !stdout.timed_out
            && !stderr.timed_out
            && !stdout.panicked
            && !stderr.panicked
            && stdout.captured.read_error.is_none()
            && stderr.captured.read_error.is_none(),
        "`{label}` cleanup/drain failed; group-kill error {group_kill_error:?}; stdout \
         {stdout:?}; stderr {stderr:?}",
    );
    assert!(
        !stdout.captured.overflowed && !stderr.captured.overflowed,
        "`{label}` exceeded the {MAX_CAPTURE_BYTES}-byte capture limit; stdout `{}`; stderr `{}`",
        String::from_utf8_lossy(&stdout.captured.bytes),
        String::from_utf8_lossy(&stderr.captured.bytes),
    );
    std::process::Output {
        status,
        stdout: stdout.captured.bytes,
        stderr: stderr.captured.bytes,
    }
}

fn try_run_command_bounded(
    command: &mut Command,
    timeout: Duration,
    label: &str,
) -> std::io::Result<std::process::Output> {
    isolate_process_group(command);

    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut child = ChildGuard::new(child);
    let deadline = Instant::now() + timeout;
    let stdout = match child.child_mut().stdout.take() {
        Some(stdout) => stdout,
        None => {
            let deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
            let cleanup = child.terminate_and_reap(deadline);
            panic_after_cleanup(
                label,
                "spawned child has no stdout pipe",
                &cleanup,
                None,
                None,
                deadline,
            );
        }
    };
    let stderr = match child.child_mut().stderr.take() {
        Some(stderr) => stderr,
        None => {
            drop(stdout);
            let deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
            let cleanup = child.terminate_and_reap(deadline);
            panic_after_cleanup(
                label,
                "spawned child has no stderr pipe",
                &cleanup,
                None,
                None,
                deadline,
            );
        }
    };
    let stdout = match start_drain(stdout, "stdout", deadline) {
        Ok(stdout) => stdout,
        Err(error) => {
            drop(stderr);
            let deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
            let cleanup = child.terminate_and_reap(deadline);
            panic_after_cleanup(
                label,
                &format!("start stdout drain thread: {error}"),
                &cleanup,
                None,
                None,
                deadline,
            );
        }
    };
    let stderr = match start_drain(stderr, "stderr", deadline) {
        Ok(stderr) => stderr,
        Err(error) => {
            let deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
            let cleanup = child.terminate_and_reap(deadline);
            panic_after_cleanup(
                label,
                &format!("start stderr drain thread: {error}"),
                &cleanup,
                Some(stdout),
                None,
                deadline,
            );
        }
    };
    loop {
        match child.child_mut().try_wait() {
            Ok(Some(status)) => {
                let cleanup_deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
                let group_kill_error = child.finish_reaped(cleanup_deadline);
                return Ok(finish_output(
                    status,
                    stdout,
                    stderr,
                    label,
                    group_kill_error,
                    cleanup_deadline,
                ));
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                let cleanup_deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
                let cleanup = child.terminate_and_reap(cleanup_deadline);
                panic_after_cleanup(
                    label,
                    &format!("exceeded its {timeout:?} deadline"),
                    &cleanup,
                    Some(stdout),
                    Some(stderr),
                    cleanup_deadline,
                );
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline => {
            }
            Err(error) => {
                let cleanup_deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
                let cleanup = child.terminate_and_reap(cleanup_deadline);
                panic_after_cleanup(
                    label,
                    &format!("poll child: {error}"),
                    &cleanup,
                    Some(stdout),
                    Some(stderr),
                    cleanup_deadline,
                );
            }
        }
    }
}

fn run_command_bounded(
    command: &mut Command,
    timeout: Duration,
    label: &str,
) -> std::process::Output {
    try_run_command_bounded(command, timeout, label)
        .unwrap_or_else(|error| panic!("spawn `{label}`: {error}"))
}

fn run_output_bounded(executable: &Path, label: &str) -> std::process::Output {
    run_command_bounded(&mut Command::new(executable), CASE_TIMEOUT, label)
}

fn cc_available_bounded() -> bool {
    let mut command = Command::new("cc");
    command.arg("--version");
    let output = match try_run_command_bounded(&mut command, Duration::from_secs(5), "cc --version")
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(error) => panic!("spawn `cc --version`: {error}"),
    };
    assert!(
        output.status.success(),
        "`cc --version` failed as {}; stdout `{}`; stderr `{}`",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    true
}

fn kv_files(main: &str) -> [(&str, &str); 3] {
    [
        ("pkg/kv/internal/resource.align", resource_source()),
        ("pkg/kv.align", kv_source()),
        ("main.align", main),
    ]
}

struct KvProject {
    dir: PathBuf,
}

impl Drop for KvProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct BuiltKvExe {
    exe: PathBuf,
    _project: KvProject,
}

fn link_env_count(name: &str) -> usize {
    std::env::var(name)
        .unwrap_or_else(|error| panic!("read pkg.kv link-child `{name}`: {error}"))
        .parse()
        .unwrap_or_else(|error| panic!("parse pkg.kv link-child `{name}`: {error}"))
}

#[test]
fn pkg_kv_link_child() {
    if std::env::var_os(LINK_CHILD_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let object_count = link_env_count(LINK_OBJECT_COUNT_ENV);
    let objects = (0..object_count)
        .map(|index| {
            PathBuf::from(
                std::env::var_os(format!("{LINK_CHILD_ENV}_OBJECT_{index}"))
                    .unwrap_or_else(|| panic!("missing pkg.kv link-child object {index}")),
            )
        })
        .collect::<Vec<_>>();
    let object_refs = objects.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let library_count = link_env_count(LINK_LIBRARY_COUNT_ENV);
    let libraries = (0..library_count)
        .map(|index| {
            std::env::var(format!("{LINK_CHILD_ENV}_LIBRARY_{index}"))
                .unwrap_or_else(|error| panic!("read pkg.kv link-child library {index}: {error}"))
        })
        .collect::<Vec<_>>();
    let executable = PathBuf::from(
        std::env::var_os(LINK_EXE_ENV).expect("missing pkg.kv link-child executable"),
    );
    link_objects(&object_refs, &executable, &libraries, Profile::Release)
        .unwrap_or_else(|error| panic!("production pkg.kv link failed: {error}"));
}

fn link_objects_bounded(objects: &[PathBuf], executable: &Path, libraries: &[String]) {
    let mut command =
        Command::new(std::env::current_exe().expect("resolve pkg.kv test executable"));
    command
        .args(["--exact", "pkg_kv_link_child", "--nocapture"])
        .env(LINK_CHILD_ENV, "1")
        .env(LINK_EXE_ENV, executable)
        .env(LINK_OBJECT_COUNT_ENV, objects.len().to_string())
        .env(LINK_LIBRARY_COUNT_ENV, libraries.len().to_string());
    for (index, object) in objects.iter().enumerate() {
        command.env(format!("{LINK_CHILD_ENV}_OBJECT_{index}"), object);
    }
    for (index, library) in libraries.iter().enumerate() {
        command.env(format!("{LINK_CHILD_ENV}_LIBRARY_{index}"), library);
    }
    let output = run_command_bounded(&mut command, PROCESS_TIMEOUT, "link pkg.kv executable");
    assert!(
        output.status.success(),
        "production pkg.kv link child failed as {}; stdout `{}`; stderr `{}`",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn build_exe_multi_bounded(name: &str, files: &[(&str, &str)], entry: &str) -> BuiltKvExe {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "align-pkg-kv-{}-{}-{nonce}",
        std::process::id(),
        name,
    ));
    assert_eq!(directory.parent(), Some(std::env::temp_dir().as_path()));
    std::fs::create_dir(&directory).expect("create pkg.kv project directory");
    let project = KvProject { dir: directory };

    for &(path, source) in files {
        let path = project.dir.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create pkg.kv module directory");
        }
        std::fs::write(path, source).expect("write pkg.kv project source");
    }

    let entry_path = project.dir.join(entry);
    let entry_source = std::fs::read_to_string(&entry_path).expect("read pkg.kv project entry");
    let mut source_map = SourceMap::new();
    let checked = check(
        &mut source_map,
        &entry_path.display().to_string(),
        &entry_source,
    );
    assert!(
        !checked.diags.has_errors(),
        "unexpected pkg.kv project errors:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags),
    );
    let mir = lower_to_mir(&checked.hir);
    let object = project.dir.join("main.o");
    emit_object_file(
        &mir,
        &object,
        BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
    )
    .expect("emit pkg.kv project object");

    let executable = project
        .dir
        .join(format!("pkg-kv{}", std::env::consts::EXE_SUFFIX));
    link_objects_bounded(&[object], &executable, &mir.link_libs);

    BuiltKvExe {
        exe: executable,
        _project: project,
    }
}

fn assert_rejected_both(name: &str, main: &str) {
    let files = kv_files(main);
    let checked = diff_check_multi(name, &files, "main.align");
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "`{name}` must fail in both compilation modes:\nwhole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
}

fn push_codec_u32(bytes: &mut Vec<u8>, value: usize) {
    bytes.extend_from_slice(
        &u32::try_from(value)
            .expect("resource codec fixture length fits u32")
            .to_le_bytes(),
    );
}

fn push_codec_str(bytes: &mut Vec<u8>, value: &str) {
    push_codec_u32(bytes, value.len());
    bytes.extend_from_slice(value.as_bytes());
}

fn resource_record_bytes(resource: &IResourceDef) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_codec_str(&mut bytes, &resource.name);
    push_codec_u32(&mut bytes, resource.type_params.len());
    for parameter in &resource.type_params {
        push_codec_str(&mut bytes, &parameter.name);
        match &parameter.bound {
            Some(bound) => {
                bytes.push(1);
                push_codec_str(&mut bytes, bound);
            }
            None => bytes.push(0),
        }
    }
    bytes.extend_from_slice(&resource.generic_arity.to_le_bytes());
    bytes.extend_from_slice(&resource.representation_version.to_le_bytes());
    push_codec_str(&mut bytes, &resource.drop_thunk);
    bytes.extend_from_slice(&resource.drop_abi_fingerprint);
    bytes
}

fn artifact_with_resource_record(
    encoded: &[u8],
    resource_start: usize,
    canonical_record_len: usize,
    replacement: &[u8],
) -> Vec<u8> {
    let resource_end = resource_start + canonical_record_len;
    assert_eq!(
        &encoded[resource_end..resource_end + 4],
        &[0; 4],
        "pkg.kv resource must be followed by the empty public-const sequence",
    );
    let mut artifact = Vec::with_capacity(encoded.len() - canonical_record_len + replacement.len());
    artifact.extend_from_slice(&encoded[..resource_start]);
    artifact.extend_from_slice(replacement);
    artifact.extend_from_slice(&encoded[resource_end..]);

    let surface_len = resource_start + replacement.len() + 4;
    let hash_offset = artifact
        .len()
        .checked_sub(32)
        .expect("interface artifact has both trailing hashes");
    assert!(
        surface_len <= hash_offset,
        "capabilities follow the public surface"
    );
    let interface_hash = Hash128::of(&artifact[..surface_len]);
    artifact[hash_offset..hash_offset + 8].copy_from_slice(&interface_hash.lo.to_le_bytes());
    artifact[hash_offset + 8..hash_offset + 16].copy_from_slice(&interface_hash.hi.to_le_bytes());
    artifact
}

fn package_guide_example(document: &str) -> &str {
    document
        .split_once("## `pkg.kv`")
        .expect("pkg.kv guide section")
        .1
        .split_once("```align\n")
        .expect("pkg.kv guide example start")
        .1
        .split_once("\n```")
        .expect("pkg.kv guide example end")
        .0
}

#[test]
fn english_and_japanese_guide_share_one_syntax_checked_example() {
    let english = package_guide_example(fixture("docs/guide/23-packages.md"));
    let japanese = package_guide_example(fixture("docs/guide/ja/23-packages.md"));
    assert_eq!(english, japanese, "the translated guide example drifted");

    let main = format!("module main\n{english}\n\nfn main() -> i32 = 0\n");
    let files = kv_files(&main);
    let checked = diff_check_multi("pkg-kv-guide-example", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "the published guide example must type-check in both compilation modes:\nwhole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
}

const FUNCTION_VALUE_FORMS_MAIN: &str = r#"module main
import pkg.kv

fn forward_connect(host: str, port: i64, options: pkg.kv.ClientOptions) -> Result<pkg.kv.client, pkg.kv.Error> =
  pkg.kv.connect(host, port, options)
fn forward_get(borrow mut owner: pkg.kv.client, key: str) -> Result<Option<string>, pkg.kv.Error> =
  pkg.kv.get(owner, key)
fn forward_set(borrow mut owner: pkg.kv.client, key: str, value: str, options: pkg.kv.SetOptions) -> Result<bool, pkg.kv.Error> =
  pkg.kv.set(owner, key, value, options)
fn forward_delete(borrow mut owner: pkg.kv.client, key: str) -> Result<bool, pkg.kv.Error> =
  pkg.kv.delete(owner, key)

fn local_connect(host: str, port: i64, options: pkg.kv.ClientOptions) -> Result<pkg.kv.client, pkg.kv.Error> {
  call := pkg.kv.connect
  return call(host, port, options)
}
fn local_get(borrow mut owner: pkg.kv.client, key: str) -> Result<Option<string>, pkg.kv.Error> {
  call := pkg.kv.get
  return call(owner, key)
}
fn local_set(borrow mut owner: pkg.kv.client, key: str, value: str, options: pkg.kv.SetOptions) -> Result<bool, pkg.kv.Error> {
  call := pkg.kv.set
  return call(owner, key, value, options)
}
fn local_delete(borrow mut owner: pkg.kv.client, key: str) -> Result<bool, pkg.kv.Error> {
  call := pkg.kv.delete
  return call(owner, key)
}

fn joined_connect(choose: bool, host: str, port: i64, options: pkg.kv.ClientOptions) -> Result<pkg.kv.client, pkg.kv.Error> {
  mut call := pkg.kv.connect
  if choose { call = forward_connect } else { call = pkg.kv.connect }
  return call(host, port, options)
}
fn joined_get(choose: bool, borrow mut owner: pkg.kv.client, key: str) -> Result<Option<string>, pkg.kv.Error> {
  mut call := pkg.kv.get
  if choose { call = forward_get } else { call = pkg.kv.get }
  return call(owner, key)
}
fn joined_set(choose: bool, borrow mut owner: pkg.kv.client, key: str, value: str, options: pkg.kv.SetOptions) -> Result<bool, pkg.kv.Error> {
  mut call := pkg.kv.set
  if choose { call = forward_set } else { call = pkg.kv.set }
  return call(owner, key, value, options)
}
fn joined_delete(choose: bool, borrow mut owner: pkg.kv.client, key: str) -> Result<bool, pkg.kv.Error> {
  mut call := pkg.kv.delete
  if choose { call = forward_delete } else { call = pkg.kv.delete }
  return call(owner, key)
}

fn parameter_connect(call: fn(str, i64, pkg.kv.ClientOptions) -> Result<pkg.kv.client, pkg.kv.Error>, host: str, port: i64, options: pkg.kv.ClientOptions) -> Result<pkg.kv.client, pkg.kv.Error> =
  call(host, port, options)
fn parameter_get(call: fn(borrow mut pkg.kv.client, str) -> Result<Option<string>, pkg.kv.Error>, borrow mut owner: pkg.kv.client, key: str) -> Result<Option<string>, pkg.kv.Error> =
  call(owner, key)
fn parameter_set(call: fn(borrow mut pkg.kv.client, str, str, pkg.kv.SetOptions) -> Result<bool, pkg.kv.Error>, borrow mut owner: pkg.kv.client, key: str, value: str, options: pkg.kv.SetOptions) -> Result<bool, pkg.kv.Error> =
  call(owner, key, value, options)
fn parameter_delete(call: fn(borrow mut pkg.kv.client, str) -> Result<bool, pkg.kv.Error>, borrow mut owner: pkg.kv.client, key: str) -> Result<bool, pkg.kv.Error> =
  call(owner, key)

fn parameter_connect_value(host: str, port: i64, options: pkg.kv.ClientOptions) -> Result<pkg.kv.client, pkg.kv.Error> =
  parameter_connect(pkg.kv.connect, host, port, options)
fn parameter_get_value(borrow mut owner: pkg.kv.client, key: str) -> Result<Option<string>, pkg.kv.Error> =
  parameter_get(pkg.kv.get, owner, key)
fn parameter_set_value(borrow mut owner: pkg.kv.client, key: str, value: str, options: pkg.kv.SetOptions) -> Result<bool, pkg.kv.Error> =
  parameter_set(pkg.kv.set, owner, key, value, options)
fn parameter_delete_value(borrow mut owner: pkg.kv.client, key: str) -> Result<bool, pkg.kv.Error> =
  parameter_delete(pkg.kv.delete, owner, key)

Calls {
  connect_call: fn(str, i64, pkg.kv.ClientOptions) -> Result<pkg.kv.client, pkg.kv.Error>,
  get_call: fn(borrow mut pkg.kv.client, str) -> Result<Option<string>, pkg.kv.Error>,
  set_call: fn(borrow mut pkg.kv.client, str, str, pkg.kv.SetOptions) -> Result<bool, pkg.kv.Error>,
  delete_call: fn(borrow mut pkg.kv.client, str) -> Result<bool, pkg.kv.Error>,
}
fn calls() -> Calls = Calls {
  connect_call: pkg.kv.connect,
  get_call: pkg.kv.get,
  set_call: pkg.kv.set,
  delete_call: pkg.kv.delete,
}
fn field_connect(host: str, port: i64, options: pkg.kv.ClientOptions) -> Result<pkg.kv.client, pkg.kv.Error> {
  table := calls()
  return table.connect_call(host, port, options)
}
fn field_get(borrow mut owner: pkg.kv.client, key: str) -> Result<Option<string>, pkg.kv.Error> {
  table := calls()
  return table.get_call(owner, key)
}
fn field_set(borrow mut owner: pkg.kv.client, key: str, value: str, options: pkg.kv.SetOptions) -> Result<bool, pkg.kv.Error> {
  table := calls()
  return table.set_call(owner, key, value, options)
}
fn field_delete(borrow mut owner: pkg.kv.client, key: str) -> Result<bool, pkg.kv.Error> {
  table := calls()
  return table.delete_call(owner, key)
}

fn generic<T>(value: T) -> T = value
fn generic_connect(host: str, port: i64, options: pkg.kv.ClientOptions) -> Result<pkg.kv.client, pkg.kv.Error> =
  generic(pkg.kv.connect(host, port, options))
fn generic_get(borrow mut owner: pkg.kv.client, key: str) -> Result<Option<string>, pkg.kv.Error> =
  generic(pkg.kv.get(owner, key))
fn generic_set(borrow mut owner: pkg.kv.client, key: str, value: str, options: pkg.kv.SetOptions) -> Result<bool, pkg.kv.Error> =
  generic(pkg.kv.set(owner, key, value, options))
fn generic_delete(borrow mut owner: pkg.kv.client, key: str) -> Result<bool, pkg.kv.Error> =
  generic(pkg.kv.delete(owner, key))

fn main() -> i32 = 0
"#;

fn expected_indirect_modes(name: &str) -> Vec<ParamMode> {
    if name.ends_with("connect") {
        vec![ParamMode::ByValue, ParamMode::ByValue, ParamMode::ByValue]
    } else if name.ends_with("set") {
        vec![
            ParamMode::BorrowMut,
            ParamMode::ByValue,
            ParamMode::ByValue,
            ParamMode::ByValue,
        ]
    } else {
        vec![ParamMode::BorrowMut, ParamMode::ByValue]
    }
}

fn assert_function_value_mir(program: &align_mir::Program, mode: &str) {
    let wrappers = [
        "local_connect",
        "local_get",
        "local_set",
        "local_delete",
        "joined_connect",
        "joined_get",
        "joined_set",
        "joined_delete",
        "parameter_connect",
        "parameter_get",
        "parameter_set",
        "parameter_delete",
        "field_connect",
        "field_get",
        "field_set",
        "field_delete",
    ];
    let mut observed = 0;
    for name in wrappers {
        let function = program
            .fns
            .iter()
            .find(|function| function.name.as_str() == name)
            .unwrap_or_else(|| panic!("{mode}: missing function-value wrapper `{name}`"));
        let calls: Vec<_> = function
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .filter_map(|statement| match statement {
                Stmt::Let(_, Rvalue::CallIndirectWithCleanup(call)) => Some(call.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 1, "{mode}: `{name}` indirect-cleanup calls");
        let call = calls[0];
        assert_eq!(
            call.signature.param_modes,
            expected_indirect_modes(name),
            "{mode}: `{name}` indirect parameter modes",
        );
        assert_eq!(
            call.signature.return_borrow,
            ReturnBorrowSummary::None,
            "{mode}: `{name}` return borrow",
        );
        assert_eq!(
            call.signature.return_region,
            ReturnRegionSummary::None,
            "{mode}: `{name}` return region",
        );
        assert_eq!(
            call.signature.return_cleanup,
            align_sema::hir::ReturnCleanupAbi::DynamicBit,
            "{mode}: `{name}` return cleanup",
        );
        observed += 1;
    }

    let total = program
        .fns
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.stmts)
        .filter(|statement| matches!(statement, Stmt::Let(_, Rvalue::CallIndirectWithCleanup(_))))
        .count();
    assert_eq!(observed, 16);
    assert_eq!(total, 16, "{mode}: exact indirect-cleanup call count");

    for target in ["connect", "get", "set", "delete"] {
        let mangled = format!("pkg.kv${target}");
        let addresses = program
            .fns
            .iter()
            .flat_map(|function| &function.blocks)
            .flat_map(|block| &block.stmts)
            .filter_map(|statement| match statement {
                Stmt::Let(_, Rvalue::FnAddr { target, signature })
                    if target.as_str() == mangled =>
                {
                    Some(signature.as_ref())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            !addresses.is_empty(),
            "{mode}: missing named function address for `{mangled}`",
        );
        for signature in addresses {
            assert_eq!(signature.param_modes, expected_indirect_modes(target));
            assert_eq!(
                signature.return_cleanup,
                align_sema::hir::ReturnCleanupAbi::DynamicBit,
            );
        }
    }
}

#[test]
fn all_public_function_value_forms_preserve_indirect_cleanup_in_whole_and_per_unit() {
    let package_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/kv")
        .canonicalize()
        .expect("canonical apps/kv fixture directory");
    let virtual_entry = package_dir.join("__pkg_kv_function_values.align");
    let virtual_entry = virtual_entry.to_string_lossy();

    let mut whole_map = SourceMap::new();
    let checked = check(&mut whole_map, &virtual_entry, FUNCTION_VALUE_FORMS_MAIN);
    assert!(
        !checked.diags.has_errors(),
        "whole-program function-value fixture:\n{}",
        align_driver::format_diagnostics(&whole_map, &checked.diags),
    );
    let whole = lower_to_mir(&checked.hir);
    assert_function_value_mir(&whole, "whole-program");
    let whole_text = align_mir::print::program_to_string(&whole);
    assert_eq!(
        whole_text.matches("call_indirect_with_cleanup").count(),
        16,
        "whole-program MIR:\n{whole_text}",
    );

    let mut per_unit_map = SourceMap::new();
    let per_unit = build_per_unit(&mut per_unit_map, &virtual_entry, FUNCTION_VALUE_FORMS_MAIN);
    assert!(
        !per_unit.diags.has_errors(),
        "per-unit function-value fixture:\n{}",
        align_driver::format_diagnostics(&per_unit_map, &per_unit.diags),
    );
    let main = per_unit
        .units
        .iter()
        .find(|unit| unit.unit == "main")
        .expect("per-unit main artifact");
    assert_function_value_mir(&main.mir, "per-unit");
    let per_unit_text = align_mir::print::program_to_string(&main.mir);
    assert_eq!(
        per_unit_text.matches("call_indirect_with_cleanup").count(),
        16,
        "per-unit MIR:\n{per_unit_text}",
    );

    if !backend_available() {
        return;
    }
    emit_llvm_ir(&whole, BuildTarget::Baseline, false, &[], false)
        .expect("whole-program function-value LLVM emission");
    for unit in &per_unit.units {
        emit_llvm_ir(&unit.mir, BuildTarget::Baseline, false, &[], false)
            .unwrap_or_else(|error| panic!("per-unit LLVM emission for `{}`: {error}", unit.unit));
    }
}

#[test]
fn public_surface_interface_and_compile_modes_are_exact() {
    let public: Vec<_> = kv_source()
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub "))
        .map(|line| line.split([' ', '(']).take(3).collect::<Vec<_>>().join(" "))
        .collect();
    assert_eq!(
        public,
        [
            "pub resource client",
            "pub ClientOptions {",
            "pub SetCondition {",
            "pub SetOptions {",
            "pub Error {",
            "pub fn connect",
            "pub fn get",
            "pub fn set",
            "pub fn delete",
        ],
        "the root module must expose only the accepted v1 surface",
    );

    let main = r#"module main
import pkg.kv

fn exercise(
  borrow mut owner: pkg.kv.client,
  key: str,
  value: str,
) -> Result<bool, pkg.kv.Error> {
  read := pkg.kv.get(owner, key)?
  wrote := pkg.kv.set(owner, key, value, pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.Always,
    expires_in_ns: None,
  })?
  return pkg.kv.delete(owner, key)
}

fn indirect(
  borrow mut owner: pkg.kv.client,
  key: str,
) -> Result<Option<string>, pkg.kv.Error> {
  call := pkg.kv.get
  return call(owner, key)
}

fn open() -> Result<pkg.kv.client, pkg.kv.Error> = pkg.kv.connect(
  "127.0.0.1",
  6379,
  pkg.kv.ClientOptions {
    connect_timeout_ns: 1,
    io_timeout_ns: 1,
    max_response_bytes: 0,
  },
)

fn main() -> i32 = 0
"#;
    let files = kv_files(main);
    let checked = diff_check_multi("pkg-kv-public-interface", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );

    let summary = checked
        .per_unit
        .summaries
        .iter()
        .find(|summary| summary.unit == "pkg.kv")
        .expect("pkg.kv interface summary");
    assert_eq!(
        summary
            .fns
            .iter()
            .map(|function| function.name.as_str())
            .collect::<Vec<_>>(),
        ["connect", "delete", "get", "set"]
    );
    let named = |path: &str, args: Vec<IType>| IType::Named {
        path: path.to_owned(),
        args,
    };
    for (name, modes, types, ret, parallel_transfer_params) in [
        (
            "connect",
            vec![ParamMode::ByValue, ParamMode::ByValue, ParamMode::ByValue],
            vec![
                named("str", vec![]),
                named("i64", vec![]),
                named("ClientOptions", vec![]),
            ],
            named(
                "Result",
                vec![named("client", vec![]), named("Error", vec![])],
            ),
            vec![0],
        ),
        (
            "delete",
            vec![ParamMode::BorrowMut, ParamMode::ByValue],
            vec![named("client", vec![]), named("str", vec![])],
            named(
                "Result",
                vec![named("bool", vec![]), named("Error", vec![])],
            ),
            vec![1],
        ),
        (
            "get",
            vec![ParamMode::BorrowMut, ParamMode::ByValue],
            vec![named("client", vec![]), named("str", vec![])],
            named(
                "Result",
                vec![
                    named("Option", vec![named("string", vec![])]),
                    named("Error", vec![]),
                ],
            ),
            vec![1],
        ),
        (
            "set",
            vec![
                ParamMode::BorrowMut,
                ParamMode::ByValue,
                ParamMode::ByValue,
                ParamMode::ByValue,
            ],
            vec![
                named("client", vec![]),
                named("str", vec![]),
                named("str", vec![]),
                named("SetOptions", vec![]),
            ],
            named(
                "Result",
                vec![named("bool", vec![]), named("Error", vec![])],
            ),
            vec![1, 2],
        ),
    ] {
        let function = summary
            .fns
            .iter()
            .find(|function| function.name == name)
            .unwrap_or_else(|| panic!("missing `{name}` signature"));
        assert_eq!(
            function
                .params
                .iter()
                .map(|parameter| parameter.mode)
                .collect::<Vec<_>>(),
            modes,
            "parameter modes for `{name}`",
        );
        assert_eq!(
            function
                .params
                .iter()
                .map(|parameter| parameter.ty.clone())
                .collect::<Vec<_>>(),
            types,
            "parameter types for `{name}`",
        );
        assert_eq!(function.ret, ret, "return type for `{name}`");
        assert!(function.type_params.is_empty());
        assert_eq!(function.return_borrow, ReturnBorrowSummary::None);
        assert_eq!(function.return_region, ReturnRegionSummary::None);
        assert_eq!(
            function.return_cleanup,
            align_sema::hir::ReturnCleanupAbi::DynamicBit,
            "indirect calls of `{name}` must preserve the owned Result cleanup bit",
        );
        assert_eq!(function.effect, Effect::Impure);
        assert_eq!(
            function.parallel_transfer_params, parallel_transfer_params,
            "parallel-transfer roots for `{name}`",
        );
        assert!(function.resource_hook_body);
        assert!(function.generic_body.is_none());
    }
    assert_eq!(
        summary
            .structs
            .iter()
            .map(|structure| structure.name.as_str())
            .collect::<Vec<_>>(),
        ["ClientOptions", "SetOptions"]
    );
    let client_options = summary
        .structs
        .iter()
        .find(|structure| structure.name == "ClientOptions")
        .expect("ClientOptions");
    assert_eq!(
        client_options.fields,
        [
            ("connect_timeout_ns".into(), named("i64", vec![])),
            ("io_timeout_ns".into(), named("i64", vec![])),
            ("max_response_bytes".into(), named("i64", vec![])),
        ]
    );
    assert!(client_options.type_params.is_empty());
    assert_eq!(client_options.align, None);
    assert!(!client_options.c_repr);
    assert!(client_options.generic_body.is_none());
    let set_options = summary
        .structs
        .iter()
        .find(|structure| structure.name == "SetOptions")
        .expect("SetOptions");
    assert_eq!(
        set_options.fields,
        [
            ("condition".into(), named("SetCondition", vec![])),
            (
                "expires_in_ns".into(),
                named("Option", vec![named("i64", vec![])]),
            ),
        ]
    );
    assert!(set_options.type_params.is_empty());
    assert_eq!(set_options.align, None);
    assert!(!set_options.c_repr);
    assert!(set_options.generic_body.is_none());
    assert_eq!(
        summary
            .enums
            .iter()
            .map(|enumeration| enumeration.name.as_str())
            .collect::<Vec<_>>(),
        ["Error", "SetCondition"]
    );
    let set_condition = summary
        .enums
        .iter()
        .find(|enumeration| enumeration.name == "SetCondition")
        .expect("SetCondition");
    assert_eq!(
        set_condition.variants,
        [
            ("Always".into(), vec![]),
            ("IfAbsent".into(), vec![]),
            ("IfPresent".into(), vec![]),
        ]
    );
    assert!(set_condition.type_params.is_empty());
    assert!(set_condition.generic_body.is_none());
    let error = summary
        .enums
        .iter()
        .find(|enumeration| enumeration.name == "Error")
        .expect("Error");
    assert_eq!(
        error.variants,
        [
            ("Invalid".into(), vec![]),
            ("Io".into(), vec![named("core.Error", vec![])]),
            ("Server".into(), vec![named("string", vec![])]),
            ("Decode".into(), vec![]),
            ("ResponseTooLarge".into(), vec![]),
            ("Protocol".into(), vec![]),
            ("Closed".into(), vec![]),
        ]
    );
    assert!(error.type_params.is_empty());
    assert!(error.generic_body.is_none());
    assert!(summary.owned_json_graphs.is_empty());
    assert!(summary.consts.is_empty());
    assert_eq!(summary.resources.len(), 1);
    let resource = &summary.resources[0];
    assert_eq!(resource.name, "client");
    assert!(resource.type_params.is_empty());
    assert_eq!(resource.generic_arity, 0);
    assert_eq!(resource.representation_version, 1);
    assert_eq!(resource.drop_thunk, "__align_resource_drop$pkg.kv$client");
    assert_eq!(resource.drop_abi_fingerprint, *b"align-res-drop-1");
    assert_eq!(validate_for_import(summary), Ok(()));
    assert_eq!(
        deserialize(&serialize(summary)).expect("round-trip pkg.kv summary"),
        *summary
    );

    let canonical_record = resource_record_bytes(resource);
    let encoded = serialize(summary);
    let record_positions = encoded
        .windows(canonical_record.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == canonical_record).then_some(offset))
        .collect::<Vec<_>>();
    assert_eq!(
        record_positions.len(),
        1,
        "the exact independently encoded six-field resource record must occur once",
    );
    let resource_start = record_positions[0];

    let mut mutations = Vec::new();
    let mut name = resource.clone();
    name.name = "clienx".into();
    mutations.push((
        "name",
        name,
        ImportCompatibilityError::ResourceDropThunk("clienx".into()),
    ));
    let mut type_params = resource.clone();
    type_params.type_params.push(ITypeParam {
        name: "T".into(),
        bound: None,
    });
    mutations.push((
        "type_params",
        type_params,
        ImportCompatibilityError::ResourceArityMismatch("client".into()),
    ));
    let mut arity = resource.clone();
    arity.generic_arity = 1;
    mutations.push((
        "generic_arity",
        arity,
        ImportCompatibilityError::ResourceArityMismatch("client".into()),
    ));
    let mut version = resource.clone();
    version.representation_version = 2;
    mutations.push((
        "representation_version",
        version,
        ImportCompatibilityError::ResourceRepresentationVersion {
            name: "client".into(),
            version: 2,
        },
    ));
    let mut thunk = resource.clone();
    thunk.drop_thunk = "redirected".into();
    mutations.push((
        "drop_thunk",
        thunk,
        ImportCompatibilityError::ResourceDropThunk("client".into()),
    ));
    let mut fingerprint = resource.clone();
    fingerprint.drop_abi_fingerprint = [0; 16];
    mutations.push((
        "drop_abi_fingerprint",
        fingerprint,
        ImportCompatibilityError::ResourceDropAbi("client".into()),
    ));
    for (field, mutation, expected_import_error) in mutations {
        let replacement = resource_record_bytes(&mutation);
        let artifact = artifact_with_resource_record(
            &encoded,
            resource_start,
            canonical_record.len(),
            &replacement,
        );
        let decoded = deserialize(&artifact)
            .unwrap_or_else(|error| panic!("decode raw `{field}` resource mutation: {error}"));
        assert_eq!(
            decoded.resources,
            [mutation],
            "raw `{field}` bytes must decode to the independently specified resource semantics",
        );
        assert_eq!(
            validate_for_import(&decoded),
            Err(expected_import_error),
            "decoded `{field}` mutation must reach its exact import-semantic rejection",
        );
    }

    assert_rejected_both(
        "pkg-kv-connect-arity",
        "module main\nimport pkg.kv\nfn main() -> i32 { pkg.kv.connect(\"host\", 1); return 0 }\n",
    );
    assert_rejected_both(
        "pkg-kv-mutability",
        r#"module main
import pkg.kv
fn invalid(borrow owner: pkg.kv.client) -> Result<Option<string>, pkg.kv.Error> =
  pkg.kv.get(owner, "key")
fn main() -> i32 = 0
"#,
    );
    for (name, source) in [
        (
            "pkg-kv-near-type",
            "module main\nimport pkg.kv\nfn invalid(value: pkg.kv.ClientOption) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "pkg-kv-resource-generic-alias",
            "module main\nimport pkg.kv\nfn invalid(value: pkg.kv.client<i64>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "pkg-kv-unqualified-type",
            "module main\nimport pkg.kv\nfn invalid(value: client) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "pkg-kv-connect-port-type",
            "module main\nimport pkg.kv\nfn main() -> i32 { pkg.kv.connect(\"host\", \"6379\", pkg.kv.ClientOptions { connect_timeout_ns: 1, io_timeout_ns: 1, max_response_bytes: 1 }); return 0 }\n",
        ),
        (
            "pkg-kv-get-key-type",
            "module main\nimport pkg.kv\nfn invalid(borrow mut owner: pkg.kv.client) -> Result<Option<string>, pkg.kv.Error> = pkg.kv.get(owner, 1)\nfn main() -> i32 = 0\n",
        ),
        (
            "pkg-kv-set-options-type",
            "module main\nimport pkg.kv\nfn invalid(borrow mut owner: pkg.kv.client) -> Result<bool, pkg.kv.Error> = pkg.kv.set(owner, \"k\", \"v\", pkg.kv.ClientOptions { connect_timeout_ns: 1, io_timeout_ns: 1, max_response_bytes: 1 })\nfn main() -> i32 = 0\n",
        ),
        (
            "pkg-kv-delete-arity",
            "module main\nimport pkg.kv\nfn invalid(borrow mut owner: pkg.kv.client) -> Result<bool, pkg.kv.Error> = pkg.kv.delete(owner, \"a\", \"b\")\nfn main() -> i32 = 0\n",
        ),
        (
            "pkg-kv-near-variant",
            "module main\nimport pkg.kv\nfn main() -> i32 { value := pkg.kv.SetCondition.IfExist; return 0 }\n",
        ),
        (
            "pkg-kv-near-field",
            "module main\nimport pkg.kv\nfn main() -> i32 { value := pkg.kv.SetOptions { condition: pkg.kv.SetCondition.Always, expire_in_ns: None }; return 0 }\n",
        ),
    ] {
        assert_rejected_both(name, source);
    }
}

#[test]
fn private_resource_module_and_process_abort_dependencies_are_explicit() {
    let main = "module main\nimport pkg.kv\nfn main() -> i32 = 0\n";
    let root_without_process = kv_source().replacen("import std.process\n", "", 1);
    assert_ne!(root_without_process, kv_source());
    let missing_root_dependency = [
        ("pkg/kv/internal/resource.align", resource_source()),
        ("pkg/kv.align", root_without_process.as_str()),
        ("main.align", main),
    ];
    let checked = diff_check_multi(
        "pkg-kv-root-process-abort-dependency",
        &missing_root_dependency,
        "main.align",
    );
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "pkg.kv must retain its explicit std.process dependency:\nwhole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );

    let internal_without_process = resource_source().replacen("import std.process\n", "", 1);
    assert_ne!(internal_without_process, resource_source());
    let missing_internal_dependency = [
        (
            "pkg/kv/internal/resource.align",
            internal_without_process.as_str(),
        ),
        ("pkg/kv.align", kv_source()),
        ("main.align", main),
    ];
    let checked = diff_check_multi(
        "pkg-kv-internal-process-abort-dependency",
        &missing_internal_dependency,
        "main.align",
    );
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "the private state authenticator must retain its explicit std.process dependency:\nwhole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );

    let private_import = [
        ("pkg/kv/internal/resource.align", resource_source()),
        ("pkg/kv.align", kv_source()),
        (
            "main.align",
            "module main\nimport pkg.kv.internal.resource\nfn main() -> i32 = 0\n",
        ),
    ];
    let checked = diff_check_multi("pkg-kv-private-module", &private_import, "main.align");
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "a package consumer must not import pkg.kv.internal.resource:\nwhole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
}

#[derive(Debug)]
enum Reply {
    Parts(Vec<Vec<u8>>),
    PartsThenClose(Vec<Vec<u8>>),
    Close,
}

#[derive(Debug)]
struct Exchange {
    request: Vec<u8>,
    reply: Reply,
}

#[derive(Debug)]
struct Session {
    exchanges: Vec<Exchange>,
    expect_client_eof: bool,
}

#[derive(Clone, Copy)]
enum ModelCondition {
    Always,
    IfAbsent,
    IfPresent,
}

#[derive(Default)]
struct RedisSetModel {
    now_ms: u64,
    values: BTreeMap<&'static str, (&'static str, Option<u64>)>,
}

impl RedisSetModel {
    fn advance(&mut self, milliseconds: u64) {
        self.now_ms += milliseconds;
    }

    fn set(
        &mut self,
        key: &'static str,
        value: &'static str,
        condition: ModelCondition,
        expires_in_ms: Option<u64>,
    ) -> bool {
        if self
            .values
            .get(key)
            .and_then(|(_, expiry)| *expiry)
            .is_some_and(|expiry| expiry <= self.now_ms)
        {
            self.values.remove(key);
        }
        let present = self.values.contains_key(key);
        let admitted = match condition {
            ModelCondition::Always => true,
            ModelCondition::IfAbsent => !present,
            ModelCondition::IfPresent => present,
        };
        if admitted {
            self.values.insert(
                key,
                (value, expires_in_ms.map(|duration| self.now_ms + duration)),
            );
        }
        admitted
    }

    fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|(value, _)| *value)
    }
}

fn exchange(request: &[u8], reply_parts: &[&[u8]]) -> Exchange {
    Exchange {
        request: request.to_vec(),
        reply: Reply::Parts(reply_parts.iter().map(|part| part.to_vec()).collect()),
    }
}

fn close_exchange(request: &[u8]) -> Exchange {
    Exchange {
        request: request.to_vec(),
        reply: Reply::Close,
    }
}

fn reply_then_close_exchange(request: &[u8], reply_parts: &[&[u8]]) -> Exchange {
    Exchange {
        request: request.to_vec(),
        reply: Reply::PartsThenClose(reply_parts.iter().map(|part| part.to_vec()).collect()),
    }
}

fn accept_before(listener: &TcpListener, deadline: Instant) -> TcpStream {
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    loop {
        match listener.accept() {
            Ok((socket, _)) => return socket,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "Align client did not connect before deadline"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("accept scripted peer: {error}"),
        }
    }
}

fn spawn_scripted_peer(listener: TcpListener, sessions: Vec<Session>) -> JoinHandle<usize> {
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut served = 0;
        for (session_index, session) in sessions.into_iter().enumerate() {
            let mut socket = accept_before(&listener, deadline);
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("peer read timeout");
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .expect("peer write timeout");
            let exchange_count = session.exchanges.len();
            let mut peer_closed = false;
            for (exchange_index, exchange) in session.exchanges.into_iter().enumerate() {
                let mut observed = vec![0; exchange.request.len()];
                socket.read_exact(&mut observed).unwrap_or_else(|error| {
                    panic!(
                        "session {session_index} exchange {exchange_index} request read: {error}; observed={observed:?}"
                    )
                });
                assert_eq!(
                    observed, exchange.request,
                    "session {session_index} exchange {exchange_index} request bytes",
                );
                match exchange.reply {
                    Reply::Parts(parts) => {
                        for part in parts {
                            socket.write_all(&part).unwrap_or_else(|error| {
                                panic!(
                                    "session {session_index} exchange {exchange_index} reply write: {error}"
                                )
                            });
                        }
                    }
                    Reply::PartsThenClose(parts) => {
                        assert_eq!(
                            exchange_index + 1,
                            exchange_count,
                            "a reply-then-close exchange must be the session's last exchange",
                        );
                        for part in parts {
                            socket.write_all(&part).unwrap_or_else(|error| {
                                panic!(
                                    "session {session_index} exchange {exchange_index} reply write: {error}"
                                )
                            });
                        }
                        peer_closed = true;
                        break;
                    }
                    Reply::Close => {
                        assert_eq!(
                            exchange_index + 1,
                            exchange_count,
                            "a peer-close exchange must be the session's last exchange",
                        );
                        peer_closed = true;
                        break;
                    }
                }
            }
            if peer_closed {
                drop(socket);
            } else if session.expect_client_eof {
                let mut trailing = Vec::new();
                match socket.read_to_end(&mut trailing) {
                    Ok(_) => {}
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::ConnectionAborted
                        ) => {}
                    Err(error) => {
                        panic!("session {session_index} did not end in client EOF: {error}")
                    }
                }
                assert!(
                    trailing.is_empty(),
                    "session {session_index} wrote unexpected bytes before closing: {trailing:?}",
                );
            }
            served += 1;
        }
        served
    })
}

fn bind_loopback() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind scripted loopback peer");
    let port = listener.local_addr().expect("loopback address").port();
    (listener, port)
}

fn run_built(name: &str, main: &str, listener: TcpListener, sessions: Vec<Session>) {
    let files = kv_files(main);
    run_built_files(name, &files, listener, sessions);
}

fn run_built_files(
    name: &str,
    files: &[(&str, &str)],
    listener: TcpListener,
    sessions: Vec<Session>,
) {
    let built = build_exe_multi_bounded(name, files, "main.align");
    let expected_sessions = sessions.len();
    let peer = spawn_scripted_peer(listener, sessions);
    let output = run_output_bounded(&built.exe, name);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{name} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        peer.join().expect("scripted peer thread"),
        expected_sessions
    );
}

fn program_call_args<'a>(rvalue: &'a Rvalue, target_name: &str) -> Option<&'a [Operand]> {
    match rvalue {
        Rvalue::Call(DirectCall::Program(target), args) if target.as_str() == target_name => {
            Some(args)
        }
        Rvalue::CallWithCleanup(call) if call.target.as_str() == target_name => Some(&call.args),
        _ => None,
    }
}

fn assert_send_set_arg_dataflow(function: &align_mir::Function, arg: u32, label: &str) {
    let structural = format!("{function:?}");
    let slot = function.params[arg as usize];
    assert_eq!(
        function
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .filter(|statement| {
                matches!(statement, Stmt::Store(found_slot, Operand::Arg(found_arg)) if *found_slot == slot && *found_arg == arg)
            })
            .count(),
        1,
        "{label} parameter must enter exactly its declared slot:\n{structural}",
    );
    let loads = function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter_map(|statement| match statement {
            Stmt::Let(value, Rvalue::Load(found)) if *found == slot => Some(*value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        loads.len(),
        2,
        "{label} parameter slot must be read only for framing and its direct byte view:\n{structural}",
    );
    let mut view_values = Vec::new();
    let mut length_values = Vec::new();
    let mut view_sources = Vec::new();
    let mut length_sources = Vec::new();
    for statement in function.blocks.iter().flat_map(|block| &block.stmts) {
        match statement {
            Stmt::Let(value, Rvalue::Use(Operand::Value(source))) if loads.contains(source) => {
                view_sources.push(*source);
                view_values.push(*value);
            }
            Stmt::Let(value, Rvalue::SliceLen(Operand::Value(source)))
                if loads.contains(source) =>
            {
                length_sources.push(*source);
                length_values.push(*value);
            }
            _ => {}
        }
    }
    assert_eq!(
        view_values.len(),
        1,
        "{label} must form one zero-copy byte view:\n{structural}"
    );
    assert_eq!(
        length_values.len(),
        1,
        "{label} must compute its length once"
    );
    let view = view_values[0];
    let length = length_values[0];
    let view_source = view_sources[0];
    let length_source = length_sources[0];
    assert_ne!(
        view_source, length_source,
        "{label} framing and byte-write paths require distinct slot reads",
    );

    let mut direct_writes = 0;
    let mut framing_lengths = 0;
    for statement in function.blocks.iter().flat_map(|block| &block.stmts) {
        let Stmt::Let(_, rvalue) = statement else {
            continue;
        };
        if program_call_args(rvalue, "pkg.kv$write_part").is_some_and(
            |args| matches!(args.get(1), Some(Operand::Value(found)) if *found == view),
        ) {
            direct_writes += 1;
        }
        if matches!(
            rvalue,
            Rvalue::BuilderWriteInt(_, Operand::Value(found)) if *found == length
        ) || matches!(
            rvalue,
            Rvalue::BuilderWriteStrIntStr(_, _, Operand::Value(found), _) if *found == length
        ) {
            framing_lengths += 1;
        }
    }
    assert_eq!(
        direct_writes, 1,
        "{label} byte view must feed only write_part"
    );
    assert_eq!(
        framing_lengths, 1,
        "{label} length must feed only its framing builder",
    );

    assert_eq!(
        structural.matches(&format!("Arg({arg})")).count(),
        1,
        "{label} parameter acquired an unclassified MIR consumer:\n{structural}",
    );
    assert_eq!(
        structural.matches(&format!("Load({slot})")).count(),
        2,
        "{label} parameter slot acquired an unclassified MIR read:\n{structural}",
    );
    assert_eq!(
        structural
            .matches(&format!("BorrowedPlace {{ slot: {slot},"))
            .count(),
        0,
        "{label} parameter acquired an unclassified borrowed-place path:\n{structural}",
    );
    for source in [view_source, length_source] {
        assert_eq!(
            structural.matches(&format!("Value({source})")).count(),
            1,
            "{label} slot read acquired a second consumer or copy path:\n{structural}",
        );
    }
    assert_eq!(
        structural.matches(&format!("Value({view})")).count(),
        1,
        "{label} byte view acquired a second consumer or copy path:\n{structural}",
    );
    assert_eq!(
        structural.matches(&format!("Value({length})")).count(),
        1,
        "{label} length acquired a second allocation/copy path:\n{structural}",
    );
}

fn assert_write_part_dataflow(function: &align_mir::Function) {
    let structural = format!("{function:?}");
    let slot = function.params[1];
    assert_eq!(
        function
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .filter(|statement| {
                matches!(statement, Stmt::Store(found_slot, Operand::Arg(1)) if *found_slot == slot)
            })
            .count(),
        1,
        "write_part bytes must enter exactly their declared slot:\n{structural}",
    );
    let loads = function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter_map(|statement| match statement {
            Stmt::Let(value, Rvalue::Load(found)) if *found == slot => Some(*value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        loads.len(),
        2,
        "write_part must read bytes only for the native view and its length:\n{structural}",
    );
    let mut lengths = Vec::new();
    let mut length_sources = Vec::new();
    for statement in function.blocks.iter().flat_map(|block| &block.stmts) {
        if let Stmt::Let(value, Rvalue::SliceLen(Operand::Value(source))) = statement
            && loads.contains(source)
        {
            length_sources.push(*source);
            lengths.push(*value);
        }
    }
    assert_eq!(
        lengths.len(),
        1,
        "write_part must compute the view length once"
    );
    let length = lengths[0];
    let length_source = length_sources[0];
    let bytes = *loads
        .iter()
        .find(|value| **value != length_source)
        .expect("write_part direct byte load");
    let mut writes = 0;
    for statement in function.blocks.iter().flat_map(|block| &block.stmts) {
        let Stmt::Let(_, rvalue) = statement else {
            continue;
        };
        if program_call_args(rvalue, "align_rt_io_writer_write").is_some_and(|args| {
            matches!(args.get(1), Some(Operand::Value(found)) if *found == bytes)
                && matches!(args.get(2), Some(Operand::Value(found)) if *found == length)
        }) {
            writes += 1;
        }
    }
    assert_eq!(
        writes, 1,
        "write_part must pass the caller view directly to native I/O"
    );
    assert_eq!(
        structural.matches("Arg(1)").count(),
        1,
        "write_part bytes acquired an unclassified MIR consumer:\n{structural}",
    );
    assert_eq!(
        structural.matches(&format!("Load({slot})")).count(),
        2,
        "write_part bytes slot acquired an unclassified MIR read:\n{structural}",
    );
    assert_eq!(
        structural
            .matches(&format!("BorrowedPlace {{ slot: {slot},"))
            .count(),
        0,
        "write_part bytes acquired an unclassified borrowed-place path:\n{structural}",
    );
    for source in [bytes, length_source] {
        assert_eq!(
            structural.matches(&format!("Value({source})")).count(),
            1,
            "write_part byte load acquired a second consumer or copy path:\n{structural}",
        );
    }
    assert_eq!(
        structural.matches(&format!("Value({length})")).count(),
        1,
        "write_part length acquired a second allocation/copy path:\n{structural}",
    );
}

fn assert_set_streaming_mir_dataflow() {
    const MAIN: &str = "module main\nimport pkg.kv\nfn main() -> i32 = 0\n";
    let package_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/kv")
        .canonicalize()
        .expect("canonical apps/kv fixture directory");
    let virtual_entry = package_dir.join("__pkg_kv_streaming_mir.align");
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, &virtual_entry.to_string_lossy(), MAIN);
    assert!(
        !checked.diags.has_errors(),
        "production pkg.kv streaming MIR fixture:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags),
    );
    let mir = lower_to_mir(&checked.hir);
    let send_set = mir
        .fns
        .iter()
        .find(|function| function.name.as_str() == "pkg.kv$send_set")
        .expect("production send_set MIR");
    assert_send_set_arg_dataflow(send_set, 1, "SET key");
    assert_send_set_arg_dataflow(send_set, 2, "SET value");
    let write_part = mir
        .fns
        .iter()
        .find(|function| function.name.as_str() == "pkg.kv$write_part")
        .expect("production write_part MIR");
    assert_write_part_dataflow(write_part);
}

#[test]
fn exact_request_bounds_preserve_streaming_key_and_value_writes() {
    let source = kv_source();
    const VALUE_BOUNDARY: &str = "MAX_VALUE_BYTES: i64 := 536870912";
    assert_eq!(
        source
            .lines()
            .filter(|line| *line == VALUE_BOUNDARY)
            .count(),
        1,
        "the public 512-MiB key/value declaration must remain exact",
    );
    assert_set_streaming_mir_dataflow();
    if !backend_available() {
        return;
    }
    if !cc_available_bounded() {
        return;
    }
    // Exercise the exact/next comparisons without allocating a 512-MiB test string. The source
    // constant above pins the production magnitude; this scale-only substitution keeps the same
    // admission and streaming code and makes the two sides of each boundary cheap to execute.
    let scaled_root = source.replacen(VALUE_BOUNDARY, "MAX_VALUE_BYTES: i64 := 8", 1);
    assert_ne!(scaled_root, source);
    let (listener, port) = bind_loopback();
    let main = r#"module main
import pkg.kv

fn invalid_text(result: Result<Option<string>, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Invalid => true, _ => false }
  Ok(_) => false
}
fn invalid_bool(result: Result<bool, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Invalid => true, _ => false }
  Ok(_) => false
}
fn missing(value: Option<string>) -> bool = match value {
  None => true
  Some(_) => false
}
fn main() -> i32 {
  mut owner := pkg.kv.connect("127.0.0.1", __PORT__, pkg.kv.ClientOptions {
    connect_timeout_ns: 5000000000,
    io_timeout_ns: 5000000000,
    max_response_bytes: 8,
  }) else { return 1 }
  found := pkg.kv.get(owner, "12345678") else { return 2 }
  if !missing(found) { return 3 }
  if !invalid_text(pkg.kv.get(owner, "123456789")) { return 4 }
  wrote := pkg.kv.set(owner, "12345678", "abcdefgh", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.Always,
    expires_in_ns: None,
  }) else { return 5 }
  if !wrote { return 6 }
  if !invalid_bool(pkg.kv.set(owner, "123456789", "abcdefgh", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.Always,
    expires_in_ns: None,
  })) { return 7 }
  if !invalid_bool(pkg.kv.set(owner, "12345678", "abcdefghi", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.Always,
    expires_in_ns: None,
  })) { return 8 }
  removed := pkg.kv.delete(owner, "12345678") else { return 9 }
  if removed { return 10 }
  if !invalid_bool(pkg.kv.delete(owner, "123456789")) { return 11 }
  return 0
}
"#
    .replace("__PORT__", &port.to_string());
    let files = [
        ("pkg/kv/internal/resource.align", resource_source()),
        ("pkg/kv.align", scaled_root.as_str()),
        ("main.align", main.as_str()),
    ];
    let sessions = vec![Session {
        exchanges: vec![
            exchange(b"*2\r\n$3\r\nGET\r\n$8\r\n12345678\r\n", &[b"$-1\r\n"]),
            exchange(
                b"*3\r\n$3\r\nSET\r\n$8\r\n12345678\r\n$8\r\nabcdefgh\r\n",
                &[b"+OK\r\n"],
            ),
            exchange(b"*2\r\n$3\r\nDEL\r\n$8\r\n12345678\r\n", &[b":0\r\n"]),
        ],
        expect_client_eof: true,
    }];
    run_built_files("pkg-kv-request-bounds", &files, listener, sessions);
}

#[test]
fn invalid_connect_inputs_precede_all_socket_side_effects() {
    if !backend_available() {
        return;
    }
    if !cc_available_bounded() {
        return;
    }
    let (listener, port) = bind_loopback();
    let main = r#"module main
import pkg.kv

fn options(connect_ns: i64, io_ns: i64, cap: i64) -> pkg.kv.ClientOptions =
  pkg.kv.ClientOptions {
    connect_timeout_ns: connect_ns,
    io_timeout_ns: io_ns,
    max_response_bytes: cap,
  }

fn invalid(result: Result<pkg.kv.client, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Invalid => true, _ => false }
  Ok(_) => false
}

fn main() -> i32 {
  a := invalid(pkg.kv.connect("", __PORT__, options(1, 1, 0)))
  b := invalid(pkg.kv.connect("bad\0host", __PORT__, options(1, 1, 0)))
  c := invalid(pkg.kv.connect("127.0.0.1", 0, options(1, 1, 0)))
  d := invalid(pkg.kv.connect("127.0.0.1", 65536, options(1, 1, 0)))
  e := invalid(pkg.kv.connect("127.0.0.1", __PORT__, options(0, 1, 0)))
  f := invalid(pkg.kv.connect("127.0.0.1", __PORT__, options(86400000000001, 1, 0)))
  g := invalid(pkg.kv.connect("127.0.0.1", __PORT__, options(1, 0, 0)))
  h := invalid(pkg.kv.connect("127.0.0.1", __PORT__, options(1, 86400000000001, 0)))
  i := invalid(pkg.kv.connect("127.0.0.1", __PORT__, options(1, 1, -1)))
  j := invalid(pkg.kv.connect("127.0.0.1", __PORT__, options(1, 1, 536870913)))
  if a && b && c && d && e && f && g && h && i && j { return 0 }
  return 1
}
"#
    .replace("__PORT__", &port.to_string());
    let files = kv_files(&main);
    let built = build_exe_multi_bounded("pkg-kv-connect-validation", &files, "main.align");
    let output = run_output_bounded(&built.exe, "pkg-kv-connect-validation");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    listener
        .set_nonblocking(true)
        .expect("nonblocking validation listener");
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "invalid connect inputs must not reach the socket boundary",
    );
}

#[test]
fn get_set_delete_emit_exact_resp_bytes_and_accept_fragmented_replies() {
    if !backend_available() {
        return;
    }
    if !cc_available_bounded() {
        return;
    }
    let (listener, port) = bind_loopback();
    let main = r#"module main
import pkg.kv

fn invalid_set(result: Result<bool, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Invalid => true, _ => false }
  Ok(_) => false
}

fn missing(result: Result<Option<string>, pkg.kv.Error>) -> bool = match result {
  Ok(value) => match value { None => true, Some(_) => false }
  Err(_) => false
}

fn main() -> i32 {
  open := pkg.kv.connect
  read := pkg.kv.get
  write := pkg.kv.set
  remove := pkg.kv.delete
  mut owner := open("127.0.0.1", __PORT__, pkg.kv.ClientOptions {
    connect_timeout_ns: 5000000000,
    io_timeout_ns: 5000000000,
    max_response_bytes: 1024,
  }) else { return 1 }

  found := read(owner, "a\0b") else { return 2 }
  text := found else { return 3 }
  if text != "v\0x" { return 4 }

  always := write(owner, "k\r\n", "v\0\n", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.Always,
    expires_in_ns: None,
  }) else { return 5 }
  if !always { return 6 }

  if !invalid_set(write(owner, "unwritten", "unwritten", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.Always,
    expires_in_ns: Some(0),
  })) { return 7 }

  always_expiring := write(owner, "always-exp", "x", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.Always,
    expires_in_ns: Some(1000000),
  }) else { return 8 }
  if !always_expiring { return 9 }

  next_nanosecond := write(owner, "next-exp", "x", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.Always,
    expires_in_ns: Some(1000001),
  }) else { return 70 }
  if !next_nanosecond { return 71 }

  fractional_millisecond := write(owner, "ceil-exp", "x", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.Always,
    expires_in_ns: Some(1500001),
  }) else { return 72 }
  if !fractional_millisecond { return 73 }

  absent_without_expiry := write(owner, "nx-none", "", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.IfAbsent,
    expires_in_ns: None,
  }) else { return 10 }
  if !absent_without_expiry { return 11 }

  absent_with_expiry := write(owner, "nx-exp", "", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.IfAbsent,
    expires_in_ns: Some(1),
  }) else { return 12 }
  if absent_with_expiry { return 13 }

  present_without_expiry := write(owner, "xx-none", "z", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.IfPresent,
    expires_in_ns: None,
  }) else { return 14 }
  if present_without_expiry { return 15 }

  present_with_max_expiry := write(owner, "xx-max", "z", pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.IfPresent,
    expires_in_ns: Some(9223372036854775807),
  }) else { return 16 }
  if !present_with_max_expiry { return 17 }

  if !missing(read(owner, "missing")) { return 18 }

  removed := remove(owner, "") else { return 19 }
  if !removed { return 20 }
  plus_zero := remove(owner, "plus-zero") else { return 21 }
  if plus_zero { return 22 }
  minus_zero := remove(owner, "minus-zero") else { return 23 }
  if minus_zero { return 24 }
  plus_one := remove(owner, "plus-one") else { return 25 }
  if !plus_one { return 26 }
  return 0
}
"#
    .replace("__PORT__", &port.to_string());

    let sessions = vec![Session {
        exchanges: vec![
            exchange(
                b"*2\r\n$3\r\nGET\r\n$3\r\na\0b\r\n",
                &[b"$3\r\n", b"v\0", b"x\r", b"\n"],
            ),
            exchange(
                b"*3\r\n$3\r\nSET\r\n$3\r\nk\r\n\r\n$3\r\nv\0\n\r\n",
                &[b"+O", b"K\r\n"],
            ),
            exchange(
                b"*5\r\n$3\r\nSET\r\n$10\r\nalways-exp\r\n$1\r\nx\r\n$2\r\nPX\r\n$1\r\n1\r\n",
                &[b"+OK\r\n"],
            ),
            exchange(
                b"*5\r\n$3\r\nSET\r\n$8\r\nnext-exp\r\n$1\r\nx\r\n$2\r\nPX\r\n$1\r\n2\r\n",
                &[b"+OK\r\n"],
            ),
            exchange(
                b"*5\r\n$3\r\nSET\r\n$8\r\nceil-exp\r\n$1\r\nx\r\n$2\r\nPX\r\n$1\r\n2\r\n",
                &[b"+OK\r\n"],
            ),
            exchange(
                b"*4\r\n$3\r\nSET\r\n$7\r\nnx-none\r\n$0\r\n\r\n$2\r\nNX\r\n",
                &[b"+OK\r\n"],
            ),
            exchange(
                b"*6\r\n$3\r\nSET\r\n$6\r\nnx-exp\r\n$0\r\n\r\n$2\r\nNX\r\n$2\r\nPX\r\n$1\r\n1\r\n",
                &[b"$-", b"1\r\n"],
            ),
            exchange(
                b"*4\r\n$3\r\nSET\r\n$7\r\nxx-none\r\n$1\r\nz\r\n$2\r\nXX\r\n",
                &[b"$-1\r\n"],
            ),
            exchange(
                b"*6\r\n$3\r\nSET\r\n$6\r\nxx-max\r\n$1\r\nz\r\n$2\r\nXX\r\n$2\r\nPX\r\n$13\r\n9223372036855\r\n",
                &[b"+", b"OK", b"\r", b"\n"],
            ),
            exchange(b"*2\r\n$3\r\nGET\r\n$7\r\nmissing\r\n", &[b"$-1\r\n"]),
            exchange(b"*2\r\n$3\r\nDEL\r\n$0\r\n\r\n", &[b":00", b"01\r\n"]),
            exchange(
                b"*2\r\n$3\r\nDEL\r\n$9\r\nplus-zero\r\n",
                &[b":+0\r\n"],
            ),
            exchange(
                b"*2\r\n$3\r\nDEL\r\n$10\r\nminus-zero\r\n",
                &[b":-0\r\n"],
            ),
            exchange(
                b"*2\r\n$3\r\nDEL\r\n$8\r\nplus-one\r\n",
                &[b":+01\r\n"],
            ),
        ],
        expect_client_eof: true,
    }];
    run_built("pkg-kv-request-bytes", &main, listener, sessions);
}

#[test]
fn set_results_follow_redis_collision_expiry_refresh_and_persistence_model() {
    if !backend_available() {
        return;
    }
    if !cc_available_bounded() {
        return;
    }

    // The scripted replies below are derived from this independent logical-clock model, not from
    // the package implementation. It pins Redis SET's NX/XX admission, TTL replacement/removal,
    // expiry, and the rule that a conditional refresh cannot resurrect an expired key.
    let mut model = RedisSetModel::default();
    assert!(model.set("collision", "old", ModelCondition::Always, None));
    assert!(!model.set("collision", "new", ModelCondition::IfAbsent, None));
    assert_eq!(model.value("collision"), Some("old"));

    assert!(model.set("persistent", "ephemeral", ModelCondition::Always, Some(2)));
    model.advance(1);
    assert!(model.set("persistent", "durable", ModelCondition::Always, None));
    model.advance(100);
    assert!(model.set("persistent", "stable", ModelCondition::IfPresent, None));
    assert_eq!(model.value("persistent"), Some("stable"));

    assert!(model.set("refresh", "first", ModelCondition::Always, Some(2)));
    model.advance(1);
    assert!(model.set("refresh", "second", ModelCondition::IfPresent, Some(2)));
    model.advance(1);
    assert!(!model.set("refresh", "third", ModelCondition::IfAbsent, None));
    assert_eq!(model.value("refresh"), Some("second"));
    model.advance(2);
    assert!(!model.set("refresh", "third", ModelCondition::IfPresent, None));
    assert_eq!(model.value("refresh"), None);

    let (listener, port) = bind_loopback();
    let main = r#"module main
import pkg.kv

fn options(condition: pkg.kv.SetCondition, expiry: Option<i64>) -> pkg.kv.SetOptions =
  pkg.kv.SetOptions { condition: condition, expires_in_ns: expiry }

fn main() -> i32 {
  mut owner := pkg.kv.connect("127.0.0.1", __PORT__, pkg.kv.ClientOptions {
    connect_timeout_ns: 5000000000,
    io_timeout_ns: 5000000000,
    max_response_bytes: 64,
  }) else { return 1 }

  a := pkg.kv.set(owner, "collision", "old", options(pkg.kv.SetCondition.Always, None)) else { return 2 }
  if !a { return 3 }
  b := pkg.kv.set(owner, "collision", "new", options(pkg.kv.SetCondition.IfAbsent, None)) else { return 4 }
  if b { return 5 }

  c := pkg.kv.set(owner, "persistent", "ephemeral", options(pkg.kv.SetCondition.Always, Some(2000000))) else { return 6 }
  if !c { return 7 }
  d := pkg.kv.set(owner, "persistent", "durable", options(pkg.kv.SetCondition.Always, None)) else { return 8 }
  if !d { return 9 }
  e := pkg.kv.set(owner, "persistent", "stable", options(pkg.kv.SetCondition.IfPresent, None)) else { return 10 }
  if !e { return 11 }

  f := pkg.kv.set(owner, "refresh", "first", options(pkg.kv.SetCondition.Always, Some(2000000))) else { return 12 }
  if !f { return 13 }
  g := pkg.kv.set(owner, "refresh", "second", options(pkg.kv.SetCondition.IfPresent, Some(1500001))) else { return 14 }
  if !g { return 15 }
  h := pkg.kv.set(owner, "refresh", "third", options(pkg.kv.SetCondition.IfAbsent, None)) else { return 16 }
  if h { return 17 }
  i := pkg.kv.set(owner, "refresh", "third", options(pkg.kv.SetCondition.IfPresent, None)) else { return 18 }
  if i { return 19 }
  return 0
}
"#
    .replace("__PORT__", &port.to_string());
    let sessions = vec![Session {
        exchanges: vec![
            exchange(
                b"*3\r\n$3\r\nSET\r\n$9\r\ncollision\r\n$3\r\nold\r\n",
                &[b"+OK\r\n"],
            ),
            exchange(
                b"*4\r\n$3\r\nSET\r\n$9\r\ncollision\r\n$3\r\nnew\r\n$2\r\nNX\r\n",
                &[b"$-1\r\n"],
            ),
            exchange(
                b"*5\r\n$3\r\nSET\r\n$10\r\npersistent\r\n$9\r\nephemeral\r\n$2\r\nPX\r\n$1\r\n2\r\n",
                &[b"+OK\r\n"],
            ),
            exchange(
                b"*3\r\n$3\r\nSET\r\n$10\r\npersistent\r\n$7\r\ndurable\r\n",
                &[b"+OK\r\n"],
            ),
            exchange(
                b"*4\r\n$3\r\nSET\r\n$10\r\npersistent\r\n$6\r\nstable\r\n$2\r\nXX\r\n",
                &[b"+OK\r\n"],
            ),
            exchange(
                b"*5\r\n$3\r\nSET\r\n$7\r\nrefresh\r\n$5\r\nfirst\r\n$2\r\nPX\r\n$1\r\n2\r\n",
                &[b"+OK\r\n"],
            ),
            exchange(
                b"*6\r\n$3\r\nSET\r\n$7\r\nrefresh\r\n$6\r\nsecond\r\n$2\r\nXX\r\n$2\r\nPX\r\n$1\r\n2\r\n",
                &[b"+OK\r\n"],
            ),
            exchange(
                b"*4\r\n$3\r\nSET\r\n$7\r\nrefresh\r\n$5\r\nthird\r\n$2\r\nNX\r\n",
                &[b"$-1\r\n"],
            ),
            exchange(
                b"*4\r\n$3\r\nSET\r\n$7\r\nrefresh\r\n$5\r\nthird\r\n$2\r\nXX\r\n",
                &[b"$-1\r\n"],
            ),
        ],
        expect_client_eof: true,
    }];
    run_built("pkg-kv-set-model", &main, listener, sessions);
}

#[test]
fn reusable_reply_errors_and_terminal_frames_have_exact_connection_disposition() {
    if !backend_available() {
        return;
    }
    if !cc_available_bounded() {
        return;
    }
    let (listener, port) = bind_loopback();
    let main = r#"module main
import pkg.kv

fn options(cap: i64) -> pkg.kv.ClientOptions = pkg.kv.ClientOptions {
  connect_timeout_ns: 5000000000,
  io_timeout_ns: 5000000000,
  max_response_bytes: cap,
}

fn server_message(result: Result<Option<string>, pkg.kv.Error>, expected: str) -> bool = match result {
  Err(error) => match error { Server(message) => message == expected, _ => false }
  Ok(_) => false
}

fn decode_error(result: Result<Option<string>, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Decode => true, _ => false }
  Ok(_) => false
}

fn empty_value(result: Result<Option<string>, pkg.kv.Error>) -> bool = match result {
  Ok(value) => match value { Some(text) => text.len() == 0, None => false }
  Err(_) => false
}

fn missing(result: Result<Option<string>, pkg.kv.Error>) -> bool = match result {
  Ok(value) => match value { None => true, Some(_) => false }
  Err(_) => false
}

fn protocol<T>(result: Result<T, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Protocol => true, _ => false }
  Ok(_) => false
}

fn closed<T>(result: Result<T, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Closed => true, _ => false }
  Ok(_) => false
}

fn too_large(result: Result<Option<string>, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { ResponseTooLarge => true, _ => false }
  Ok(_) => false
}

fn reusable() -> i32 {
  mut owner := pkg.kv.connect("127.0.0.1", __PORT__, options(16)) else { return 1 }
  if !server_message(pkg.kv.get(owner, "server"), "ERR nope") { return 2 }
  if !decode_error(pkg.kv.get(owner, "bad-error")) { return 3 }
  if !decode_error(pkg.kv.get(owner, "decode")) { return 4 }
  if !empty_value(pkg.kv.get(owner, "empty")) { return 5 }
  return 0
}

fn exact_error_cap() -> i32 {
  mut owner := pkg.kv.connect("127.0.0.1", __PORT__, options(8)) else { return 6 }
  if !server_message(pkg.kv.get(owner, "exact-error"), "12345678") { return 7 }
  if !missing(pkg.kv.get(owner, "after-exact")) { return 8 }
  return 0
}

fn next_error_cap() -> i32 {
  mut owner := pkg.kv.connect("127.0.0.1", __PORT__, options(8)) else { return 9 }
  if !too_large(pkg.kv.get(owner, "over-error")) { return 10 }
  if !closed(pkg.kv.delete(owner, "after-error-cap")) { return 11 }
  return 0
}

fn malformed_integer() -> i32 {
  mut owner := pkg.kv.connect("127.0.0.1", __PORT__, options(16)) else { return 12 }
  if !protocol(pkg.kv.delete(owner, "bad")) { return 13 }
  if !closed(pkg.kv.get(owner, "after-protocol")) { return 14 }
  return 0
}

fn over_cap() -> i32 {
  mut owner := pkg.kv.connect("127.0.0.1", __PORT__, options(3)) else { return 15 }
  if !too_large(pkg.kv.get(owner, "large")) { return 16 }
  if !closed(pkg.kv.delete(owner, "after-cap")) { return 17 }
  return 0
}

fn trailing() -> i32 {
  mut owner := pkg.kv.connect("127.0.0.1", __PORT__, options(16)) else { return 18 }
  if !protocol(pkg.kv.get(owner, "trailing")) { return 19 }
  if !closed(pkg.kv.get(owner, "after-trailing")) { return 20 }
  return 0
}

fn partial() -> i32 {
  mut owner := pkg.kv.connect("127.0.0.1", __PORT__, options(16)) else { return 21 }
  if !protocol(pkg.kv.get(owner, "partial")) { return 22 }
  if !closed(pkg.kv.get(owner, "after-partial")) { return 23 }
  return 0
}

fn eof() -> i32 {
  mut owner := pkg.kv.connect("127.0.0.1", __PORT__, options(16)) else { return 24 }
  if !closed(pkg.kv.get(owner, "eof")) { return 25 }
  if !closed(pkg.kv.get(owner, "after-eof")) { return 26 }
  return 0
}

fn main() -> i32 {
  first := reusable()
  if first != 0 { return first }
  second := exact_error_cap()
  if second != 0 { return second }
  third := next_error_cap()
  if third != 0 { return third }
  fourth := malformed_integer()
  if fourth != 0 { return fourth }
  fifth := over_cap()
  if fifth != 0 { return fifth }
  sixth := trailing()
  if sixth != 0 { return sixth }
  seventh := partial()
  if seventh != 0 { return seventh }
  return eof()
}
"#
    .replace("__PORT__", &port.to_string());

    let sessions = vec![
        Session {
            exchanges: vec![
                exchange(
                    b"*2\r\n$3\r\nGET\r\n$6\r\nserver\r\n",
                    &[b"-ERR", b" nope\r", b"\n"],
                ),
                exchange(
                    b"*2\r\n$3\r\nGET\r\n$9\r\nbad-error\r\n",
                    &[b"-", &[0xff], b"\r\n"],
                ),
                exchange(
                    b"*2\r\n$3\r\nGET\r\n$6\r\ndecode\r\n",
                    &[b"$1\r\n", &[0xff], b"\r\n"],
                ),
                exchange(b"*2\r\n$3\r\nGET\r\n$5\r\nempty\r\n", &[b"$0\r", b"\n\r\n"]),
            ],
            expect_client_eof: true,
        },
        Session {
            exchanges: vec![
                exchange(
                    b"*2\r\n$3\r\nGET\r\n$11\r\nexact-error\r\n",
                    &[b"-12345678\r\n"],
                ),
                exchange(b"*2\r\n$3\r\nGET\r\n$11\r\nafter-exact\r\n", &[b"$-1\r\n"]),
            ],
            expect_client_eof: true,
        },
        Session {
            exchanges: vec![exchange(
                b"*2\r\n$3\r\nGET\r\n$10\r\nover-error\r\n",
                &[b"-123456789"],
            )],
            expect_client_eof: true,
        },
        Session {
            exchanges: vec![exchange(
                b"*2\r\n$3\r\nDEL\r\n$3\r\nbad\r\n",
                &[b":", b"2\r\n"],
            )],
            expect_client_eof: true,
        },
        Session {
            exchanges: vec![exchange(
                b"*2\r\n$3\r\nGET\r\n$5\r\nlarge\r\n",
                &[b"$4", b"\r\n"],
            )],
            expect_client_eof: true,
        },
        Session {
            exchanges: vec![exchange(
                b"*2\r\n$3\r\nGET\r\n$8\r\ntrailing\r\n",
                &[b"$-1\r\nX"],
            )],
            expect_client_eof: true,
        },
        Session {
            exchanges: vec![reply_then_close_exchange(
                b"*2\r\n$3\r\nGET\r\n$7\r\npartial\r\n",
                &[b"$3\r\n", b"ab"],
            )],
            expect_client_eof: false,
        },
        Session {
            exchanges: vec![close_exchange(b"*2\r\n$3\r\nGET\r\n$3\r\neof\r\n")],
            expect_client_eof: false,
        },
    ];
    run_built("pkg-kv-reply-disposition", &main, listener, sessions);
}
