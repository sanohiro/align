//! `pkg.kv` v1 adversarial owner for impossible native products, authenticated private state, and
//! exact package cleanup. The generated executable uses only uniquely named C stubs; every case is
//! a separately bounded subprocess of the same build.

mod common;
use common::*;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
const CASE_TIMEOUT: Duration = Duration::from_secs(2);
const PIPE_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const LINK_CHILD_ENV: &str = "ALIGN_PKG_KV_FAULT_LINK_CHILD";
const LINK_EXE_ENV: &str = "ALIGN_PKG_KV_FAULT_LINK_EXE";
const LINK_OBJECT_COUNT_ENV: &str = "ALIGN_PKG_KV_FAULT_LINK_OBJECT_COUNT";
const LINK_LIBRARY_COUNT_ENV: &str = "ALIGN_PKG_KV_FAULT_LINK_LIBRARY_COUNT";

fn sleep_until(deadline: Instant, interval: Duration) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        std::thread::sleep(remaining.min(interval));
    }
}

#[derive(Debug)]
struct ProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
struct Captured {
    bytes: Vec<u8>,
    overflowed: bool,
    read_error: Option<String>,
    eof: bool,
}

struct DrainHandle {
    join: Option<JoinHandle<()>>,
    captured: Arc<Mutex<Captured>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for DrainHandle {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if self.join.as_ref().is_some_and(JoinHandle::is_finished) {
            let _ = self.join.take().expect("finished drain handle").join();
        }
    }
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
    reap: Result<ExitStatus, String>,
    stdout: Option<DrainReport>,
    stderr: Option<DrainReport>,
}

struct ChildGuard {
    child: Option<std::process::Child>,
}

impl ChildGuard {
    fn new(child: std::process::Child) -> Self {
        Self { child: Some(child) }
    }

    fn child_mut(&mut self) -> &mut std::process::Child {
        self.child.as_mut().expect("armed fault child guard")
    }

    fn cleanup(
        &mut self,
        known_status: Option<ExitStatus>,
        stdout: Option<DrainHandle>,
        stderr: Option<DrainHandle>,
    ) -> CleanupReport {
        let deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
        let child = self.child.as_mut().expect("armed fault child guard");
        let group_kill_error = kill_process_group_until(child, deadline);
        let direct_kill_error = kill_child_until(child, known_status.is_some(), deadline);
        let reap = match known_status {
            Some(status) => Ok(status),
            None => try_reap_until(child, deadline).map_err(|error| error.to_string()),
        };
        self.child.take();
        let (stdout, stderr) = finish_drains(stdout, stderr, deadline);
        CleanupReport {
            group_kill_error,
            direct_kill_error,
            reap,
            stdout,
            stderr,
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        let deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
        let _ = kill_process_group_until(child, deadline);
        let _ = kill_child_until(child, false, deadline);
        let _ = try_reap_until(child, deadline);
        self.child.take();
    }
}

fn drain_bounded<R: Read>(
    mut reader: R,
    captured: &Arc<Mutex<Captured>>,
    cancelled: &std::sync::atomic::AtomicBool,
) {
    let mut chunk = [0_u8; 4096];
    loop {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let count = match reader.read(&mut chunk) {
            Ok(count) => count,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) =>
            {
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
            captured
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .eof = true;
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
fn set_nonblocking<R: std::os::fd::AsRawFd>(reader: &R, deadline: Instant) -> Result<(), String> {
    let fd = reader.as_raw_fd();
    let flags = loop {
        // SAFETY: `fd` belongs to the live child pipe, and F_GETFL takes no third argument.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags >= 0 {
            break flags;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline {
            continue;
        }
        return Err(format!("read pipe flags: {error}"));
    };
    loop {
        // SAFETY: `fd` remains live and the existing flags are valid with O_NONBLOCK added.
        let result = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if result >= 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline {
            continue;
        }
        return Err(format!("make read pipe nonblocking: {error}"));
    }
}

#[cfg(not(unix))]
fn set_nonblocking<R>(reader: &R, deadline: Instant) -> Result<(), String> {
    let _ = (reader, deadline);
    Ok(())
}

fn start_drain<R: Read + Send + 'static>(reader: R, stream: &str) -> std::io::Result<DrainHandle> {
    let captured = Arc::new(Mutex::new(Captured {
        bytes: Vec::new(),
        overflowed: false,
        read_error: None,
        eof: false,
    }));
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_capture = Arc::clone(&captured);
    let thread_cancelled = Arc::clone(&cancelled);
    let join = std::thread::Builder::new()
        .name(format!("pkg-kv-fault-{stream}-drain"))
        .spawn(move || drain_bounded(reader, &thread_capture, &thread_cancelled))?;
    Ok(DrainHandle {
        join: Some(join),
        captured,
        cancelled,
    })
}

fn isolate_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

fn try_reap_until(
    child: &mut std::process::Child,
    deadline: Instant,
) -> std::io::Result<ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => {
                sleep_until(deadline, Duration::from_millis(5));
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

fn kill_process_group_until(child: &std::process::Child, deadline: Instant) -> Option<String> {
    #[cfg(unix)]
    let result = match i32::try_from(child.id()) {
        Ok(pid) => {
            // SAFETY: this child was placed in a fresh process group whose id is its positive pid.
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
        let _ = (child, deadline);
        None
    };

    result
}

fn kill_child_until(
    child: &mut std::process::Child,
    already_reaped: bool,
    deadline: Instant,
) -> Option<String> {
    loop {
        match child.kill() {
            Ok(()) => return None,
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline => {
            }
            Err(error) if already_reaped && child_already_exited_error(&error) => {
                return None;
            }
            Err(error) => return Some(error.to_string()),
        }
    }
}

fn child_already_exited_error(error: &std::io::Error) -> bool {
    if matches!(
        error.kind(),
        std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotFound
    ) {
        return true;
    }
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn drains_finished(stdout: &Option<DrainHandle>, stderr: &Option<DrainHandle>) -> bool {
    [&stdout, &stderr].into_iter().all(|drain| {
        drain
            .as_ref()
            .and_then(|drain| drain.join.as_ref())
            .is_none_or(JoinHandle::is_finished)
    })
}

fn snapshot_until(captured: &Arc<Mutex<Captured>>, deadline: Instant) -> Captured {
    loop {
        match captured.try_lock() {
            Ok(captured) => return captured.clone(),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                return poisoned.into_inner().clone();
            }
            Err(std::sync::TryLockError::WouldBlock) if Instant::now() < deadline => {
                sleep_until(deadline, Duration::from_millis(1));
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                return Captured {
                    read_error: Some("capture snapshot lock remained busy at deadline".to_owned()),
                    ..Captured::default()
                };
            }
        }
    }
}

fn finish_drains(
    mut stdout: Option<DrainHandle>,
    mut stderr: Option<DrainHandle>,
    deadline: Instant,
) -> (Option<DrainReport>, Option<DrainReport>) {
    let cancellation_tail = Duration::from_millis(100);
    let cancel_at = deadline.checked_sub(cancellation_tail).unwrap_or(deadline);
    while !drains_finished(&stdout, &stderr) && Instant::now() < cancel_at {
        sleep_until(cancel_at, Duration::from_millis(2));
    }
    for drain in [&stdout, &stderr].into_iter().flatten() {
        drain.cancelled.store(true, Ordering::Release);
    }
    while !drains_finished(&stdout, &stderr) && Instant::now() < deadline {
        sleep_until(deadline, Duration::from_millis(2));
    }

    fn finish(mut drain: DrainHandle, deadline: Instant) -> DrainReport {
        let timed_out = !drain
            .join
            .as_ref()
            .expect("armed drain handle")
            .is_finished();
        let panicked = if timed_out {
            false
        } else {
            drain
                .join
                .take()
                .expect("finished drain handle")
                .join()
                .is_err()
        };
        let captured = snapshot_until(&drain.captured, deadline);
        DrainReport {
            captured,
            timed_out,
            panicked,
        }
    }

    (
        stdout.take().map(|drain| finish(drain, deadline)),
        stderr.take().map(|drain| finish(drain, deadline)),
    )
}

fn panic_after_cleanup(label: &str, reason: &str, cleanup: CleanupReport) -> ! {
    let stdout = cleanup
        .stdout
        .as_ref()
        .map(|report| String::from_utf8_lossy(&report.captured.bytes).into_owned())
        .unwrap_or_default();
    let stderr = cleanup
        .stderr
        .as_ref()
        .map(|report| String::from_utf8_lossy(&report.captured.bytes).into_owned())
        .unwrap_or_default();
    panic!(
        "{reason}; `{label}` cleanup report: {cleanup:?}; partial stdout `{stdout}`; partial stderr \
         `{stderr}`"
    )
}

fn output_after_cleanup(label: &str, cleanup: CleanupReport) -> ProcessOutput {
    let CleanupReport {
        group_kill_error,
        direct_kill_error,
        reap,
        stdout,
        stderr,
    } = cleanup;
    let status = reap.unwrap_or_else(|error| {
        panic!("`{label}` child was not reaped after bounded cleanup: {error}")
    });
    let stdout = stdout.unwrap_or_else(|| panic!("`{label}` has no stdout drain report"));
    let stderr = stderr.unwrap_or_else(|| panic!("`{label}` has no stderr drain report"));
    assert!(
        group_kill_error.is_none()
            && direct_kill_error.is_none()
            && !stdout.timed_out
            && !stdout.panicked
            && !stderr.timed_out
            && !stderr.panicked
            && stdout.captured.read_error.is_none()
            && stderr.captured.read_error.is_none()
            && stdout.captured.eof
            && stderr.captured.eof,
        "`{label}` cleanup/drain failed; group kill {group_kill_error:?}; direct kill \
         {direct_kill_error:?}; stdout {stdout:?}; stderr {stderr:?}",
    );
    assert!(
        !stdout.captured.overflowed && !stderr.captured.overflowed,
        "`{label}` exceeded the {MAX_CAPTURE_BYTES}-byte capture limit; stdout `{}`; stderr `{}`",
        String::from_utf8_lossy(&stdout.captured.bytes),
        String::from_utf8_lossy(&stderr.captured.bytes),
    );
    ProcessOutput {
        status,
        stdout: stdout.captured.bytes,
        stderr: stderr.captured.bytes,
    }
}

fn try_run_command_bounded(
    command: &mut Command,
    timeout: Duration,
    label: &str,
) -> std::io::Result<ProcessOutput> {
    isolate_process_group(command);
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut child = ChildGuard::new(child);
    let deadline = Instant::now() + timeout;
    let stdout_pipe = child.child_mut().stdout.take();
    let stderr_pipe = child.child_mut().stderr.take();
    let mut setup_errors = Vec::new();
    let stdout = stdout_pipe.and_then(|pipe| {
        if let Err(error) = set_nonblocking(&pipe, deadline) {
            setup_errors.push(format!("stdout {error}"));
        }
        match start_drain(pipe, "stdout") {
            Ok(drain) => Some(drain),
            Err(error) => {
                setup_errors.push(format!("start stdout drain: {error}"));
                None
            }
        }
    });
    if stdout.is_none()
        && !setup_errors
            .iter()
            .any(|error| error.starts_with("start stdout"))
    {
        setup_errors.push("spawned child has no stdout pipe".to_owned());
    }
    let stderr = stderr_pipe.and_then(|pipe| {
        if let Err(error) = set_nonblocking(&pipe, deadline) {
            setup_errors.push(format!("stderr {error}"));
        }
        match start_drain(pipe, "stderr") {
            Ok(drain) => Some(drain),
            Err(error) => {
                setup_errors.push(format!("start stderr drain: {error}"));
                None
            }
        }
    });
    if stderr.is_none()
        && !setup_errors
            .iter()
            .any(|error| error.starts_with("start stderr"))
    {
        setup_errors.push("spawned child has no stderr pipe".to_owned());
    }
    if !setup_errors.is_empty() {
        let cleanup = child.cleanup(None, stdout, stderr);
        panic_after_cleanup(label, &setup_errors.join("; "), cleanup);
    }

    loop {
        match child.child_mut().try_wait() {
            Ok(Some(status)) => {
                let cleanup = child.cleanup(Some(status), stdout, stderr);
                return Ok(output_after_cleanup(label, cleanup));
            }
            Ok(None) if Instant::now() < deadline => {
                sleep_until(deadline, Duration::from_millis(5));
            }
            Ok(None) => {
                let cleanup = child.cleanup(None, stdout, stderr);
                panic_after_cleanup(
                    label,
                    &format!("exceeded its {timeout:?} deadline"),
                    cleanup,
                );
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline => {
            }
            Err(error) => {
                let cleanup = child.cleanup(None, stdout, stderr);
                panic_after_cleanup(label, &format!("poll child: {error}"), cleanup);
            }
        }
    }
}

fn run_command_bounded(command: &mut Command, timeout: Duration, label: &str) -> ProcessOutput {
    try_run_command_bounded(command, timeout, label)
        .unwrap_or_else(|error| panic!("spawn `{label}`: {error}"))
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

fn link_env_count(name: &str) -> usize {
    std::env::var(name)
        .unwrap_or_else(|error| panic!("read link-child `{name}`: {error}"))
        .parse()
        .unwrap_or_else(|error| panic!("parse link-child `{name}`: {error}"))
}

#[test]
fn pkg_kv_fault_link_child() {
    if std::env::var_os(LINK_CHILD_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let object_count = link_env_count(LINK_OBJECT_COUNT_ENV);
    let objects = (0..object_count)
        .map(|index| {
            PathBuf::from(
                std::env::var_os(format!("{LINK_CHILD_ENV}_OBJECT_{index}"))
                    .unwrap_or_else(|| panic!("missing link-child object {index}")),
            )
        })
        .collect::<Vec<_>>();
    let object_refs = objects.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let library_count = link_env_count(LINK_LIBRARY_COUNT_ENV);
    let libraries = (0..library_count)
        .map(|index| {
            std::env::var(format!("{LINK_CHILD_ENV}_LIBRARY_{index}"))
                .unwrap_or_else(|error| panic!("read link-child library {index}: {error}"))
        })
        .collect::<Vec<_>>();
    let executable =
        PathBuf::from(std::env::var_os(LINK_EXE_ENV).expect("missing fault link-child executable"));
    link_objects(&object_refs, &executable, &libraries, Profile::Release)
        .expect("link pkg.kv fault executable");
}

fn link_objects_bounded(objects: &[PathBuf], exe: &Path, libraries: &[String]) {
    let mut command =
        Command::new(std::env::current_exe().expect("resolve pkg.kv fault test executable"));
    command
        .args(["--exact", "pkg_kv_fault_link_child", "--nocapture"])
        .env(LINK_CHILD_ENV, "1")
        .env(LINK_EXE_ENV, exe)
        .env(LINK_OBJECT_COUNT_ENV, objects.len().to_string())
        .env(LINK_LIBRARY_COUNT_ENV, libraries.len().to_string());
    for (index, object) in objects.iter().enumerate() {
        command.env(format!("{LINK_CHILD_ENV}_OBJECT_{index}"), object);
    }
    for (index, library) in libraries.iter().enumerate() {
        command.env(format!("{LINK_CHILD_ENV}_LIBRARY_{index}"), library);
    }
    let linked = run_command_bounded(
        &mut command,
        PROCESS_TIMEOUT,
        "link pkg.kv fault executable",
    );
    assert!(
        linked.status.success(),
        "pkg.kv fault executable link failed as {}; stdout `{}`; stderr `{}`",
        linked.status,
        String::from_utf8_lossy(&linked.stdout),
        String::from_utf8_lossy(&linked.stderr),
    );
}

fn source_fingerprint(source: &str) -> u64 {
    source.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[derive(Clone, Copy)]
struct NativeRewrite {
    original: &'static str,
    replacement: &'static str,
    declarations: usize,
    calls: usize,
}

#[derive(Clone, Copy)]
struct IdentifierSpan {
    start: usize,
    end: usize,
}

fn identifier_spans(source: &str) -> Vec<IdentifierSpan> {
    let bytes = source.as_bytes();
    let mut spans = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                let mut depth = 1_usize;
                while index < bytes.len() && depth != 0 {
                    if bytes[index..].starts_with(b"/*") {
                        depth += 1;
                        index += 2;
                    } else if bytes[index..].starts_with(b"*/") {
                        depth -= 1;
                        index += 2;
                    } else {
                        index += 1;
                    }
                }
                assert_eq!(depth, 0, "unterminated block comment in pkg.kv source");
            }
            quote @ (b'"' | b'\'') => {
                index += 1;
                let mut closed = false;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == quote {
                        index += 1;
                        closed = true;
                        break;
                    } else {
                        index += 1;
                    }
                }
                assert!(closed, "unterminated literal in pkg.kv source");
            }
            byte if byte == b'_' || byte.is_ascii_alphabetic() => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index] == b'_' || bytes[index].is_ascii_alphanumeric())
                {
                    index += 1;
                }
                spans.push(IdentifierSpan { start, end: index });
            }
            _ => index += 1,
        }
    }
    spans
}

fn next_code_byte(source: &str, mut index: usize) -> Option<u8> {
    let bytes = source.as_bytes();
    loop {
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index..index.saturating_add(2)) == Some(b"//") {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes.get(index..index.saturating_add(2)) == Some(b"/*") {
            index += 2;
            let mut depth = 1_usize;
            while index < bytes.len() && depth != 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            assert_eq!(depth, 0, "unterminated block comment in pkg.kv source");
            continue;
        }
        return bytes.get(index).copied();
    }
}

fn native_inventory(source: &str, symbol: &str) -> (usize, usize, usize) {
    let spans = identifier_spans(source);
    let mut declarations = 0;
    let mut calls = 0;
    let mut other = 0;
    for (position, span) in spans.iter().enumerate() {
        if &source[span.start..span.end] != symbol {
            continue;
        }
        if next_code_byte(source, span.end) != Some(b'(') {
            other += 1;
            continue;
        }
        let declared = position.checked_sub(1).is_some_and(|previous| {
            let previous = spans[previous];
            &source[previous.start..previous.end] == "fn"
        });
        if declared {
            declarations += 1;
        } else {
            calls += 1;
        }
    }
    (declarations, calls, other)
}

fn declared_function_start(source: &str, name: &str) -> usize {
    let spans = identifier_spans(source);
    let starts = spans
        .windows(2)
        .filter_map(|pair| {
            let declaration = pair[0];
            let function = pair[1];
            (&source[declaration.start..declaration.end] == "fn"
                && &source[function.start..function.end] == name
                && next_code_byte(source, function.end) == Some(b'('))
            .then_some(declaration.start)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        starts.len(),
        1,
        "fault harness requires exactly one live `{name}` declaration",
    );
    starts[0]
}

fn rewrite_native_symbols(mut source: String, rewrites: &[NativeRewrite]) -> String {
    let spans = identifier_spans(&source);
    let mut edits = Vec::new();
    for rewrite in rewrites {
        assert_eq!(
            native_inventory(&source, rewrite.original),
            (rewrite.declarations, rewrite.calls, 0),
            "fault harness declaration/live-call inventory drift for `{}`",
            rewrite.original,
        );
        assert_eq!(
            native_inventory(&source, rewrite.replacement),
            (0, 0, 0),
            "fault harness replacement `{}` already exists",
            rewrite.replacement,
        );
        for span in &spans {
            if &source[span.start..span.end] == rewrite.original {
                edits.push((span.start, span.end, rewrite.replacement));
            }
        }
    }
    edits.sort_unstable_by_key(|(start, _, _)| *start);
    for &(start, end, replacement) in edits.iter().rev() {
        source.replace_range(start..end, replacement);
    }
    for rewrite in rewrites {
        assert_eq!(native_inventory(&source, rewrite.original), (0, 0, 0));
        assert_eq!(
            native_inventory(&source, rewrite.replacement),
            (rewrite.declarations, rewrite.calls, 0),
            "fault harness rewritten inventory drift for `{}`",
            rewrite.replacement,
        );
    }
    source
}

fn fault_sources() -> (String, String) {
    let root_source = fixture("apps/kv/pkg/kv.align");
    assert_eq!(
        (root_source.len(), source_fingerprint(root_source)),
        (22_712, 0x8aa6_186e_52cf_8e79),
        "pkg.kv source changed; refresh the fault rewrite inventory",
    );
    let native = [
        NativeRewrite {
            original: "align_rt_tcp_connect",
            replacement: "align_kv_fault_tcp_connect",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_tcp_conn_set_io_timeout",
            replacement: "align_kv_fault_set_io_timeout",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_tcp_conn_free",
            replacement: "align_kv_fault_conn_free",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_tcp_conn_reader",
            replacement: "align_kv_fault_conn_reader",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_tcp_conn_writer",
            replacement: "align_kv_fault_conn_writer",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_io_reader_read",
            replacement: "align_kv_fault_reader_read",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_io_reader_free",
            replacement: "align_kv_fault_reader_free",
            declarations: 1,
            calls: 0,
        },
        NativeRewrite {
            original: "align_rt_io_writer_write",
            replacement: "align_kv_fault_writer_write",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_io_writer_free",
            replacement: "align_kv_fault_writer_free",
            declarations: 1,
            calls: 0,
        },
        NativeRewrite {
            original: "align_rt_buffer_new",
            replacement: "align_kv_fault_buffer_new",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_buffer_bytes",
            replacement: "align_kv_fault_buffer_bytes",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_buffer_capacity",
            replacement: "align_kv_fault_buffer_capacity",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_buffer_free",
            replacement: "align_kv_fault_buffer_free",
            declarations: 1,
            calls: 1,
        },
    ];
    let mut root = rewrite_native_symbols(root_source.to_owned(), &native);
    const PRODUCTION_REFILL: &str = r#"fn refill(
  reader: raw,
  input: raw,
) -> Result<bool, Error> {
  unsafe {
    buffer: raw := raw.load(input, 0)
    header: raw := raw.load(input, 8)
    count := align_kv_fault_reader_read(reader, buffer)
    if count == -9223372036854775808 || count < 0 - I32_MAX
      || count > READ_CHUNK_BYTES {
      process.abort()
    }

    align_kv_fault_buffer_bytes(buffer, header)
    pointer: raw := raw.load(header, 0)
    length: i64 := raw.load(header, 8)
    if count < 0 {
      if length != 0 { process.abort() }
      status_value := 0 - count
      if status_value <= 0 || status_value > I32_MAX { process.abort() }
      status: i32 := status_value as i32
      return Err(Error.Io(native_status_error(status)))
    }
    if count == 0 {
      if length != 0 { process.abort() }
      raw.store(input, 16, pointer)
      raw.store(input, 24, 0 as i64)
      raw.store(input, 32, 0 as i64)
      return Ok(false)
    }
    if pointer.is_null() || length != count { process.abort() }
    raw.store(input, 16, pointer)
    raw.store(input, 24, length)
    raw.store(input, 32, 0 as i64)
    return Ok(true)
  }
}"#;
    const GUARDED_PRODUCTION_REFILL: &str = r#"fn refill(
  reader: raw,
  input: raw,
) -> Result<bool, Error> {
  unsafe {
    buffer: raw := raw.load(input, 0)
    header: raw := raw.load(input, 8)
    count := align_kv_fault_reader_read(reader, buffer)
    if count == -9223372036854775808 || count < 0 - I32_MAX
      || count > READ_CHUNK_BYTES {
      process.abort()
    }

    align_kv_fault_buffer_bytes(buffer, header)
    pointer: raw := raw.load(header, 0)
    length: i64 := raw.load(header, 8)
    if count < 0 {
      if length != 0 { process.abort() }
      status_value := 0 - count
      if status_value <= 0 || status_value > I32_MAX { process.abort() }
      align_kv_fault_reader_product_validated(1)
      status: i32 := status_value as i32
      return Err(Error.Io(native_status_error(status)))
    }
    if count == 0 {
      if length != 0 { process.abort() }
      align_kv_fault_reader_product_validated(2)
      raw.store(input, 16, pointer)
      raw.store(input, 24, 0 as i64)
      raw.store(input, 32, 0 as i64)
      return Ok(false)
    }
    if pointer.is_null() || length != count { process.abort() }
    align_kv_fault_reader_product_validated(3)
    raw.store(input, 16, pointer)
    raw.store(input, 24, length)
    raw.store(input, 32, 0 as i64)
    return Ok(true)
  }
}"#;
    let refill_start = declared_function_start(&root, "refill");
    let next_byte_start = declared_function_start(&root, "next_byte");
    let refill_end = next_byte_start
        .checked_sub(2)
        .expect("refill precedes next_byte separator");
    assert_eq!(&root[refill_end..next_byte_start], "\n\n");
    assert_eq!(
        &root[refill_start..refill_end],
        PRODUCTION_REFILL,
        "fault harness must instrument the exact complete live refill declaration",
    );
    root.replace_range(refill_start..refill_end, GUARDED_PRODUCTION_REFILL);
    let refill_start = declared_function_start(&root, "refill");
    let next_byte_start = declared_function_start(&root, "next_byte");
    let refill_end = next_byte_start
        .checked_sub(2)
        .expect("guarded refill precedes next_byte separator");
    assert_eq!(&root[refill_end..next_byte_start], "\n\n");
    let refill = &root[refill_start..refill_end];
    assert_eq!(
        refill, GUARDED_PRODUCTION_REFILL,
        "fault harness guarded refill rewrite drifted",
    );
    for forbidden in [
        "resource.view_from_raw(",
        "next_byte(",
        "required_byte(",
        "bulk_length(",
        "parse_error_reply(",
        "parse_bulk_reply(",
        "parse_reply(",
    ] {
        assert!(
            !refill.contains(forbidden),
            "production refill must not reach typed views or parser `{forbidden}`",
        );
    }
    for product in [1, 2, 3] {
        assert_eq!(
            refill
                .matches(&format!(
                    "align_kv_fault_reader_product_validated({product})"
                ))
                .count(),
            1,
            "fault harness requires one guarded reader-product boundary for product {product}",
        );
    }
    const REFILL_PUBLICATION_BEFORE_TYPED_VIEW: &str = r#"    if input_index >= input_length {
      available := refill(reader, input)?
      if !available { return Ok(None) }
      input_length = raw.load(input, 24)
      input_index = raw.load(input, 32)
    }
    input_pointer: raw := raw.load(input, 16)
    bytes: slice<u8> := resource.view_from_raw("#;
    assert_eq!(
        root.matches("resource.view_from_raw(").count(),
        1,
        "fault harness requires one production typed-view constructor",
    );
    assert_eq!(
        root.matches(REFILL_PUBLICATION_BEFORE_TYPED_VIEW).count(),
        1,
        "next_byte must construct its sole typed view only after successful refill publication",
    );
    root.push_str(ROOT_FAULT_HOOKS);

    let internal_source = fixture("apps/kv/pkg/kv/internal/resource.align");
    assert_eq!(
        (internal_source.len(), source_fingerprint(internal_source)),
        (4_007, 0x37db_4750_f57b_2011),
        "pkg.kv internal source changed; refresh the fault rewrite inventory",
    );
    let internal_native = [
        NativeRewrite {
            original: "align_rt_tcp_conn_free",
            replacement: "align_kv_fault_conn_free",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_io_reader_free",
            replacement: "align_kv_fault_reader_free",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_io_writer_free",
            replacement: "align_kv_fault_writer_free",
            declarations: 1,
            calls: 1,
        },
    ];
    let mut internal = rewrite_native_symbols(internal_source.to_owned(), &internal_native);
    let spans = identifier_spans(&internal);
    let state_free = spans
        .windows(2)
        .filter_map(|pair| {
            let qualifier = pair[0];
            let method = pair[1];
            (&internal[qualifier.start..qualifier.end] == "raw"
                && &internal[qualifier.end..method.start] == "."
                && &internal[method.start..method.end] == "free"
                && next_code_byte(&internal, method.end) == Some(b'(')
                && internal[qualifier.start..].starts_with("raw.free(state)"))
            .then_some(qualifier.start..qualifier.start + "raw.free(state)".len())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        state_free.len(),
        1,
        "fault harness requires exactly one live raw.free(state) call",
    );
    internal.replace_range(state_free[0].clone(), "align_kv_fault_state_free(state)");
    assert_eq!(
        native_inventory(&internal, "align_kv_fault_state_free"),
        (0, 1, 0),
        "fault state-free hook must replace exactly one live call",
    );
    internal.push_str(INTERNAL_FAULT_HOOKS);
    (root, internal)
}

struct FaultProject {
    dir: PathBuf,
}

impl Drop for FaultProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct BuiltFaultExe {
    exe: PathBuf,
    _project: FaultProject,
}

fn build_fault_exe(files: &[(&str, &str)], c_source: &str) -> BuiltFaultExe {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "align-pkg-kv-faults-{}-{nonce}",
        std::process::id()
    ));
    assert_eq!(dir.parent(), Some(std::env::temp_dir().as_path()));
    std::fs::create_dir(&dir).expect("create unique fault-project directory");
    let project = FaultProject { dir };
    let dir = &project.dir;
    for &(name, source) in files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fault-project module directory");
        }
        std::fs::write(path, source).expect("write fault-project source");
    }

    let entry = dir.join("main.align");
    let entry_source = std::fs::read_to_string(&entry).expect("read fault-project entry");
    let mut source_map = SourceMap::new();
    let walk = build_per_unit(&mut source_map, &entry.display().to_string(), &entry_source);
    assert!(
        !walk.diags.has_errors(),
        "unexpected per-unit errors:\n{}",
        align_driver::format_diagnostics(&source_map, &walk.diags),
    );

    let mut objects = Vec::with_capacity(walk.units.len() + 1);
    let mut link_libraries = Vec::new();
    for (index, unit) in walk.units.iter().enumerate() {
        let object = dir.join(format!("unit-{index}.o"));
        emit_object_file(
            &unit.mir,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
        )
        .unwrap_or_else(|error| panic!("codegen for unit `{}`: {error}", unit.unit));
        for library in &unit.mir.link_libs {
            if !link_libraries.contains(library) {
                link_libraries.push(library.clone());
            }
        }
        objects.push(object);
    }

    let c_path = dir.join("faults.c");
    let c_object = dir.join("faults.o");
    std::fs::write(&c_path, c_source).expect("write C fault fixture");
    let mut compiler = Command::new("cc");
    compiler
        .args(["-std=c11", "-c", "-O0"])
        .arg(&c_path)
        .arg("-o")
        .arg(&c_object);
    let compiled = run_command_bounded(
        &mut compiler,
        PROCESS_TIMEOUT,
        "pkg.kv C fault fixture compilation",
    );
    assert!(
        compiled.status.success(),
        "C fault fixture failed with {}; stdout `{}`; stderr `{}`",
        compiled.status,
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr),
    );
    objects.push(c_object);

    let exe = dir.join(format!("pkg-kv-faults{}", std::env::consts::EXE_SUFFIX));
    let ordered_libraries = order_link_libs(&link_libraries);
    link_objects_bounded(&objects, &exe, &ordered_libraries);
    BuiltFaultExe {
        exe,
        _project: project,
    }
}

fn run_bounded(executable: &Path, arguments: &[&str], case: &str) -> ExitStatus {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    run_command_bounded(&mut command, CASE_TIMEOUT, case).status
}

const ROOT_FAULT_HOOKS: &str = r#"

extern "C" {
  fn align_kv_fault_arm_forbidden()
  fn align_kv_fault_unexpected_return()
  fn align_kv_fault_reader_product_validated(product: i32)
  fn align_kv_fault_state_matches_live(state: raw) -> i32
  fn align_kv_fault_state_matches_closed(state: raw) -> i32
  fn align_kv_fault_state_install_live(state: raw)
  fn align_kv_fault_state_install_closed(state: raw)
  fn align_kv_fault_state_install_permuted(state: raw)
  fn align_kv_fault_state_live_semantics(reader: raw, writer: raw, cap: i64) -> i32
  fn align_kv_fault_state_closed_semantics(reader: raw, writer: raw, cap: i64) -> i32
  fn align_kv_fault_state_permuted_semantics(reader: raw, writer: raw, cap: i64) -> i32
}

pub fn test_write_part() -> Result<(), Error> {
  unsafe { return write_part(raw.null(), "".bytes()) }
}

pub fn test_refill() -> Result<bool, Error> {
  input := new_read_input()
  result := unsafe { refill(raw.null(), input) }
  free_read_input(input)
  return result
}

fn test_client_state(borrow owner: client) -> raw {
  unsafe {
    reference := resource.borrow(owner)
    return resource.raw(reference)
  }
}

// This exists only in the copied fault-project source. The expected byte images are built
// independently by the C fixture; no production state constant or corruption hook is an oracle.
pub fn test_state_golden(borrow owner: client) -> bool {
  unsafe {
    state := test_client_state(owner)
    if align_kv_fault_state_matches_live(state) != 1 { return false }
    if !pkg.kv.internal.resource.client_live(state) { return false }
    if align_kv_fault_state_live_semantics(
      pkg.kv.internal.resource.client_reader(state),
      pkg.kv.internal.resource.client_writer(state),
      pkg.kv.internal.resource.client_max_response_bytes(state),
    ) != 1 { return false }

    // Keep every scalar product valid while rotating the three distinct pointer tokens and
    // changing the in-range cap. Accessors must observe the documented offsets, while the exact
    // live byte golden must reject this semantic permutation. Restore before any cleanup edge.
    align_kv_fault_state_install_permuted(state)
    permuted_semantics := pkg.kv.internal.resource.client_live(state)
      && align_kv_fault_state_permuted_semantics(
        pkg.kv.internal.resource.client_reader(state),
        pkg.kv.internal.resource.client_writer(state),
        pkg.kv.internal.resource.client_max_response_bytes(state),
      ) == 1
    permuted_bytes_rejected := align_kv_fault_state_matches_live(state) == 0
    align_kv_fault_state_install_live(state)
    if !permuted_semantics || !permuted_bytes_rejected { return false }
    if align_kv_fault_state_matches_live(state) != 1 { return false }
    if !pkg.kv.internal.resource.client_live(state) { return false }
    if align_kv_fault_state_live_semantics(
      pkg.kv.internal.resource.client_reader(state),
      pkg.kv.internal.resource.client_writer(state),
      pkg.kv.internal.resource.client_max_response_bytes(state),
    ) != 1 { return false }

    pkg.kv.internal.resource.retire_client(state)
    if align_kv_fault_state_matches_closed(state) != 1 { return false }
    if pkg.kv.internal.resource.client_live(state) { return false }
    if align_kv_fault_state_closed_semantics(
      pkg.kv.internal.resource.client_reader(state),
      pkg.kv.internal.resource.client_writer(state),
      pkg.kv.internal.resource.client_max_response_bytes(state),
    ) != 1 { return false }

    // Poison all 40 bytes before installing the C-built closed image. Reading it back through the
    // package accessors is the byte-to-semantic half; the enclosing resource Drop must free only
    // the state because the three native values were already retired above.
    align_kv_fault_state_install_closed(state)
    return align_kv_fault_state_matches_closed(state) == 1
      && !pkg.kv.internal.resource.client_live(state)
      && align_kv_fault_state_closed_semantics(
        pkg.kv.internal.resource.client_reader(state),
        pkg.kv.internal.resource.client_writer(state),
        pkg.kv.internal.resource.client_max_response_bytes(state),
      ) == 1
  }
}

pub fn test_malformed_client(field: i32, operation: i32) {
  mut owner := connect("127.0.0.1", 6380, ClientOptions {
    connect_timeout_ns: 1000000,
    io_timeout_ns: 2000000,
    max_response_bytes: 32,
  }) else {
    unsafe { align_kv_fault_unexpected_return() }
    process.abort()
  }
  unsafe {
    state := test_client_state(owner)
    align_kv_fault_arm_forbidden()
    pkg.kv.internal.resource.test_corrupt_client_state(state, field)
  }
  if operation == 0 {
    _ := get(owner, "k")
    unsafe { align_kv_fault_unexpected_return() }
    process.abort()
  }
  if operation == 1 {
    _ := set(owner, "k", "v", SetOptions {
      condition: SetCondition.Always,
      expires_in_ns: None,
    })
    unsafe { align_kv_fault_unexpected_return() }
    process.abort()
  }
  if operation == 2 {
    _ := delete(owner, "k")
    unsafe { align_kv_fault_unexpected_return() }
    process.abort()
  }
  if operation == 3 { return }
  process.abort()
}
"#;

const INTERNAL_FAULT_HOOKS: &str = r#"

extern "C" {
  fn align_kv_fault_state_free(state: raw)
}

pub fn test_corrupt_client_state(state: raw, field: i32) {
  unsafe {
    connection: raw := raw.load(state, 8)
    reader: raw := raw.load(state, 16)
    writer: raw := raw.load(state, 24)
    if field == 1 { raw.store(state, 0, 2 as u32); return }
    if field == 2 { raw.store(state, 4, 2 as u8); return }
    if field == 3 { raw.store(state, 5, 1 as u8); return }
    if field == 4 { raw.store(state, 6, 1 as u16); return }
    if field == 5 { raw.store(state, 8, raw.null()); return }
    if field == 6 { raw.store(state, 16, raw.null()); return }
    if field == 7 { raw.store(state, 24, raw.null()); return }
    if field == 8 { raw.store(state, 32, -1 as i64); return }
    if field == 9 { raw.store(state, 32, 536870913 as i64); return }
    raw.store(state, 4, 1 as u8)
    raw.store(state, 8, raw.null())
    raw.store(state, 16, raw.null())
    raw.store(state, 24, raw.null())
    if field == 10 { raw.store(state, 8, connection); return }
    if field == 11 { raw.store(state, 16, reader); return }
    if field == 12 { raw.store(state, 24, writer); return }
    process.abort()
  }
}
"#;

const FAULT_MAIN: &str = r#"module main
import std.process
import pkg.kv

extern "C" {
  fn align_kv_fault_configure(case_id: i32)
  fn align_kv_fault_unexpected_return()
  fn align_kv_fault_protocol_errors() -> i32
  fn align_kv_fault_connect_calls() -> i32
  fn align_kv_fault_timeout_calls() -> i32
  fn align_kv_fault_reader_ctor_calls() -> i32
  fn align_kv_fault_writer_ctor_calls() -> i32
  fn align_kv_fault_constructor_event_count() -> i32
  fn align_kv_fault_constructor_event(index: i32) -> i32
  fn align_kv_fault_writer_calls() -> i32
  fn align_kv_fault_reader_calls() -> i32
  fn align_kv_fault_reader_product_calls() -> i32
  fn align_kv_fault_header_calls() -> i32
  fn align_kv_fault_buffer_new_calls() -> i32
  fn align_kv_fault_buffer_free_calls() -> i32
  fn align_kv_fault_cleanup_event_count() -> i32
  fn align_kv_fault_cleanup_event(index: i32) -> i32
  fn align_kv_fault_state_golden_counts() -> i32
}

fn configure(case_id: i32) {
  unsafe { align_kv_fault_configure(case_id) }
}

fn impossible_return() -> i32 {
  unsafe { align_kv_fault_unexpected_return() }
  process.abort()
}

fn options() -> pkg.kv.ClientOptions = pkg.kv.ClientOptions {
  connect_timeout_ns: 1000000,
  io_timeout_ns: 2000000,
  max_response_bytes: 32,
}

fn state_golden_options() -> pkg.kv.ClientOptions = pkg.kv.ClientOptions {
  connect_timeout_ns: 1000000,
  io_timeout_ns: 2000000,
  max_response_bytes: 19088743,
}

fn io_not_found<T>(result: Result<T, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error {
    Io(native) => match native { NotFound => true _ => false }
    _ => false
  }
  Ok(_) => false
}

fn io_invalid<T>(result: Result<T, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error {
    Io(native) => match native { Invalid => true _ => false }
    _ => false
  }
  Ok(_) => false
}

fn io_denied<T>(result: Result<T, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error {
    Io(native) => match native { Denied => true _ => false }
    _ => false
  }
  Ok(_) => false
}

fn io_timeout<T>(result: Result<T, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error {
    Io(native) => match native { Timeout => true _ => false }
    _ => false
  }
  Ok(_) => false
}

fn io_code<T>(result: Result<T, pkg.kv.Error>, expected: i32) -> bool = match result {
  Err(error) => match error {
    Io(native) => match native { Code(code) => code == expected _ => false }
    _ => false
  }
  Ok(_) => false
}

fn unit_ok(result: Result<(), pkg.kv.Error>) -> bool = match result {
  Ok(_) => true
  Err(_) => false
}

fn refill_value(result: Result<bool, pkg.kv.Error>, expected: bool) -> bool = match result {
  Ok(value) => value == expected
  Err(_) => false
}

fn missing(result: Result<Option<string>, pkg.kv.Error>) -> bool = match result {
  Ok(value) => match value { None => true Some(_) => false }
  Err(_) => false
}

fn server(result: Result<Option<string>, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Server(message) => message == "ERR nope" _ => false }
  Ok(_) => false
}

fn decode(result: Result<Option<string>, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Decode => true _ => false }
  Ok(_) => false
}

fn get_closed(result: Result<Option<string>, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Closed => true _ => false }
  Ok(_) => false
}

fn bool_closed(result: Result<bool, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Closed => true _ => false }
  Ok(_) => false
}

fn get_protocol(result: Result<Option<string>, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Protocol => true _ => false }
  Ok(_) => false
}

fn writer_error(
  result: Result<Option<string>, pkg.kv.Error>,
  status: i32,
) -> bool = match result {
  Err(error) => match error {
    Io(native) => match native {
      NotFound => status == 1
      Invalid => status == 2
      Denied => status == 3
      Timeout => status == 4
      Code(code) => (status == 5 && code == 0)
        || (status == 2147483647 && code == 2147483642)
    }
    _ => false
  }
  Ok(_) => false
}

fn connect_error(result: Result<pkg.kv.client, pkg.kv.Error>, status: i32) -> bool = match result {
  Err(error) => match error {
    Io(native) => match native {
      NotFound => status == 1
      Invalid => status == 2
      Denied => status == 3
      Timeout => status == 4
      Code(code) => (status == 5 && code == 0)
        || (status == 2147483647 && code == 2147483642)
    }
    _ => false
  }
  Ok(_) => false
}

fn timeout_failure_snapshot() -> bool {
  unsafe {
    return align_kv_fault_protocol_errors() == 0
      && align_kv_fault_connect_calls() == 1
      && align_kv_fault_timeout_calls() == 1
      && align_kv_fault_reader_ctor_calls() == 0
      && align_kv_fault_writer_ctor_calls() == 0
      && align_kv_fault_constructor_event_count() == 0
      && align_kv_fault_writer_calls() == 0
      && align_kv_fault_reader_calls() == 0
      && align_kv_fault_reader_product_calls() == 0
      && align_kv_fault_header_calls() == 0
      && align_kv_fault_buffer_new_calls() == 0
      && align_kv_fault_buffer_free_calls() == 0
      && align_kv_fault_cleanup_event_count() == 1
      && align_kv_fault_cleanup_event(0) == 3
  }
}

fn connect_failure_snapshot() -> bool {
  unsafe {
    return align_kv_fault_protocol_errors() == 0
      && align_kv_fault_connect_calls() == 1
      && align_kv_fault_timeout_calls() == 0
      && align_kv_fault_reader_ctor_calls() == 0
      && align_kv_fault_writer_ctor_calls() == 0
      && align_kv_fault_constructor_event_count() == 0
      && align_kv_fault_writer_calls() == 0
      && align_kv_fault_reader_calls() == 0
      && align_kv_fault_reader_product_calls() == 0
      && align_kv_fault_header_calls() == 0
      && align_kv_fault_buffer_new_calls() == 0
      && align_kv_fault_buffer_free_calls() == 0
      && align_kv_fault_cleanup_event_count() == 0
  }
}

fn hook_snapshot(
  writer_calls: i32,
  reader_calls: i32,
  header_calls: i32,
  buffer_calls: i32,
) -> bool {
  unsafe {
    return align_kv_fault_protocol_errors() == 0
      && align_kv_fault_connect_calls() == 0
      && align_kv_fault_timeout_calls() == 0
      && align_kv_fault_reader_ctor_calls() == 0
      && align_kv_fault_writer_ctor_calls() == 0
      && align_kv_fault_writer_calls() == writer_calls
      && align_kv_fault_reader_calls() == reader_calls
      && align_kv_fault_reader_product_calls() == reader_calls
      && align_kv_fault_header_calls() == header_calls
      && align_kv_fault_buffer_new_calls() == buffer_calls
      && align_kv_fault_buffer_free_calls() == buffer_calls
      && align_kv_fault_cleanup_event_count() == 0
  }
}

fn construction_and_cleanup_exact() -> bool {
  unsafe {
    return align_kv_fault_constructor_event_count() == 2
      && align_kv_fault_constructor_event(0) == 1
      && align_kv_fault_constructor_event(1) == 2
      && align_kv_fault_cleanup_event_count() == 4
      && align_kv_fault_cleanup_event(0) == 1
      && align_kv_fault_cleanup_event(1) == 2
      && align_kv_fault_cleanup_event(2) == 3
      && align_kv_fault_cleanup_event(3) == 4
  }
}

fn cycle_snapshot(
  writer_calls: i32,
  reader_calls: i32,
  header_calls: i32,
  buffer_calls: i32,
) -> bool {
  unsafe {
    return align_kv_fault_protocol_errors() == 0
      && align_kv_fault_connect_calls() == 1
      && align_kv_fault_timeout_calls() == 1
      && align_kv_fault_reader_ctor_calls() == 1
      && align_kv_fault_writer_ctor_calls() == 1
      && align_kv_fault_writer_calls() == writer_calls
      && align_kv_fault_reader_calls() == reader_calls
      && align_kv_fault_reader_product_calls() == reader_calls
      && align_kv_fault_header_calls() == header_calls
      && align_kv_fault_buffer_new_calls() == buffer_calls
      && align_kv_fault_buffer_free_calls() == buffer_calls
      && construction_and_cleanup_exact()
  }
}

fn normal_cycle() -> bool {
  mut owner := pkg.kv.connect("127.0.0.1", 6380, options()) else { return false }
  return missing(pkg.kv.get(owner, "k"))
}

fn state_golden_cycle() -> bool {
  owner := pkg.kv.connect("127.0.0.1", 6380, state_golden_options()) else { return false }
  return pkg.kv.test_state_golden(owner)
}

fn server_cycle() -> bool {
  mut owner := pkg.kv.connect("127.0.0.1", 6380, options()) else { return false }
  if !server(pkg.kv.get(owner, "k")) { return false }
  before: i32 := unsafe { align_kv_fault_cleanup_event_count() }
  if before != 0 { return false }
  if !missing(pkg.kv.get(owner, "k")) { return false }
  after: i32 := unsafe { align_kv_fault_cleanup_event_count() }
  return after == 0
}

fn decode_cycle() -> bool {
  mut owner := pkg.kv.connect("127.0.0.1", 6380, options()) else { return false }
  if !decode(pkg.kv.get(owner, "k")) { return false }
  before: i32 := unsafe { align_kv_fault_cleanup_event_count() }
  if before != 0 { return false }
  if !missing(pkg.kv.get(owner, "k")) { return false }
  after: i32 := unsafe { align_kv_fault_cleanup_event_count() }
  return after == 0
}

fn writer_terminal_cycle(status: i32) -> bool {
  mut owner := pkg.kv.connect("127.0.0.1", 6380, options()) else { return false }
  if !writer_error(pkg.kv.get(owner, "k"), status) { return false }
  retired: i32 := unsafe { align_kv_fault_cleanup_event_count() }
  writes: i32 := unsafe { align_kv_fault_writer_calls() }
  if retired != 3 || writes != 1 { return false }
  if !bool_closed(pkg.kv.delete(owner, "later")) { return false }
  later_writes: i32 := unsafe { align_kv_fault_writer_calls() }
  return later_writes == 1
}

fn reader_terminal_cycle() -> bool {
  mut owner := pkg.kv.connect("127.0.0.1", 6380, options()) else { return false }
  if !io_not_found(pkg.kv.get(owner, "k")) { return false }
  retired: i32 := unsafe { align_kv_fault_cleanup_event_count() }
  reads: i32 := unsafe { align_kv_fault_reader_calls() }
  if retired != 3 || reads != 1 { return false }
  if !get_closed(pkg.kv.get(owner, "later")) { return false }
  later_reads: i32 := unsafe { align_kv_fault_reader_calls() }
  return later_reads == 1
}

fn protocol_terminal_cycle() -> bool {
  mut owner := pkg.kv.connect("127.0.0.1", 6380, options()) else { return false }
  if !get_protocol(pkg.kv.get(owner, "k")) { return false }
  retired: i32 := unsafe { align_kv_fault_cleanup_event_count() }
  reads: i32 := unsafe { align_kv_fault_reader_calls() }
  if retired != 3 || reads != 1 { return false }
  if !bool_closed(pkg.kv.delete(owner, "later")) { return false }
  later_reads: i32 := unsafe { align_kv_fault_reader_calls() }
  return later_reads == 1
}

fn malformed_field(name: str) -> i32 {
  if name == "version" { return 1 }
  if name == "lifecycle" { return 2 }
  if name == "reserved-byte" { return 3 }
  if name == "reserved-word" { return 4 }
  if name == "live-connection-null" { return 5 }
  if name == "live-reader-null" { return 6 }
  if name == "live-writer-null" { return 7 }
  if name == "cap-below" { return 8 }
  if name == "cap-above" { return 9 }
  if name == "closed-connection-live" { return 10 }
  if name == "closed-reader-live" { return 11 }
  if name == "closed-writer-live" { return 12 }
  return 0
}

fn malformed_operation(name: str) -> i32 {
  if name == "get" { return 0 }
  if name == "set" { return 1 }
  if name == "delete" { return 2 }
  if name == "drop" { return 3 }
  return -1
}

fn run_abort_case(name: str) -> i32 {
  if name == "connect-zero-null" {
    configure(1)
    _ := pkg.kv.connect("127.0.0.1", 6380, options())
    return impossible_return()
  }
  if name == "connect-error-nonnull" {
    configure(2)
    _ := pkg.kv.connect("127.0.0.1", 6380, options())
    return impossible_return()
  }
  if name == "connect-negative-null" {
    configure(3)
    _ := pkg.kv.connect("127.0.0.1", 6380, options())
    return impossible_return()
  }
  if name == "connect-negative-nonnull" {
    configure(52)
    _ := pkg.kv.connect("127.0.0.1", 6380, options())
    return impossible_return()
  }
  if name == "reader-constructor-null" {
    configure(18)
    _ := pkg.kv.connect("127.0.0.1", 6380, options())
    return impossible_return()
  }
  if name == "writer-constructor-null" {
    configure(19)
    _ := pkg.kv.connect("127.0.0.1", 6380, options())
    return impossible_return()
  }
  if name == "writer-negative" {
    configure(10)
    _ := pkg.kv.test_write_part()
    return impossible_return()
  }
  if name == "timeout-negative" {
    configure(76)
    _ := pkg.kv.connect("127.0.0.1", 6380, options())
    return impossible_return()
  }
  if name == "reader-min" { configure(20) } else if name == "reader-below-status" {
    configure(21)
  } else if name == "reader-oversized" {
    configure(22)
  } else if name == "reader-negative-length" {
    configure(23)
  } else if name == "reader-zero-length" {
    configure(24)
  } else if name == "reader-positive-null" {
    configure(25)
  } else if name == "reader-positive-length" {
    configure(26)
  } else if name == "reader-negative-length-null" {
    configure(27)
  } else if name == "reader-zero-length-null" {
    configure(28)
  } else if name == "reader-positive-length-null" {
    configure(29)
  } else { return 99 }
  _ := pkg.kv.test_refill()
  return impossible_return()
}

fn run_regular_case(name: str) -> i32 {
  if name == "connect-not-found" {
    configure(4)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 1)
      && connect_failure_snapshot() { return 0 }
    return 4
  }
  if name == "connect-invalid" {
    configure(5)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 2)
      && connect_failure_snapshot() { return 0 }
    return 5
  }
  if name == "connect-denied" {
    configure(6)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 3)
      && connect_failure_snapshot() { return 0 }
    return 6
  }
  if name == "connect-timeout" {
    configure(7)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 4)
      && connect_failure_snapshot() { return 0 }
    return 7
  }
  if name == "connect-code-zero" {
    configure(8)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 5)
      && connect_failure_snapshot() { return 0 }
    return 8
  }
  if name == "connect-code-max" {
    configure(9)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 2147483647)
      && connect_failure_snapshot() { return 0 }
    return 9
  }
  if name == "writer-zero" {
    configure(11)
    if unit_ok(pkg.kv.test_write_part()) && hook_snapshot(1, 0, 0, 0) { return 0 }
    return 11
  }
  if name == "writer-not-found" {
    configure(12)
    if io_not_found(pkg.kv.test_write_part()) && hook_snapshot(1, 0, 0, 0) { return 0 }
    return 12
  }
  if name == "writer-invalid" {
    configure(13)
    if io_invalid(pkg.kv.test_write_part()) && hook_snapshot(1, 0, 0, 0) { return 0 }
    return 13
  }
  if name == "writer-denied" {
    configure(14)
    if io_denied(pkg.kv.test_write_part()) && hook_snapshot(1, 0, 0, 0) { return 0 }
    return 14
  }
  if name == "writer-timeout" {
    configure(15)
    if io_timeout(pkg.kv.test_write_part()) && hook_snapshot(1, 0, 0, 0) { return 0 }
    return 15
  }
  if name == "writer-code-zero" {
    configure(16)
    if io_code(pkg.kv.test_write_part(), 0) && hook_snapshot(1, 0, 0, 0) { return 0 }
    return 16
  }
  if name == "writer-code-max" {
    configure(17)
    if io_code(pkg.kv.test_write_part(), 2147483642) && hook_snapshot(1, 0, 0, 0) { return 0 }
    return 17
  }
  if name == "reader-negative-null" {
    configure(30)
    if io_not_found(pkg.kv.test_refill()) && hook_snapshot(0, 1, 1, 1) { return 0 }
    return 30
  }
  if name == "reader-negative-nonnull" {
    configure(31)
    if io_code(pkg.kv.test_refill(), 2147483642) && hook_snapshot(0, 1, 1, 1) { return 0 }
    return 31
  }
  if name == "reader-zero-null" {
    configure(32)
    if refill_value(pkg.kv.test_refill(), false) && hook_snapshot(0, 1, 1, 1) { return 0 }
    return 32
  }
  if name == "reader-zero-nonnull" {
    configure(33)
    if refill_value(pkg.kv.test_refill(), false) && hook_snapshot(0, 1, 1, 1) { return 0 }
    return 33
  }
  if name == "reader-positive" {
    configure(34)
    if refill_value(pkg.kv.test_refill(), true) && hook_snapshot(0, 1, 1, 1) { return 0 }
    return 34
  }
  if name == "reader-capacity" {
    configure(35)
    if refill_value(pkg.kv.test_refill(), true) && hook_snapshot(0, 1, 1, 1) { return 0 }
    return 35
  }
  if name == "normal-drop" {
    configure(40)
    if normal_cycle() && cycle_snapshot(3, 1, 1, 1) { return 0 }
    return 40
  }
  if name == "state-golden" {
    configure(50)
    if state_golden_cycle() && cycle_snapshot(0, 0, 0, 0)
      && unsafe { align_kv_fault_state_golden_counts() == 1 } { return 0 }
    return 50
  }
  if name == "server-reuse" {
    configure(41)
    if server_cycle() && cycle_snapshot(6, 2, 2, 2) { return 0 }
    return 41
  }
  if name == "decode-reuse" {
    configure(42)
    if decode_cycle() && cycle_snapshot(6, 2, 2, 2) { return 0 }
    return 42
  }
  if name == "writer-retire" {
    configure(43)
    if writer_terminal_cycle(1) && cycle_snapshot(1, 0, 0, 0) { return 0 }
    return 43
  }
  if name == "reader-retire" {
    configure(44)
    if reader_terminal_cycle() && cycle_snapshot(3, 1, 1, 1) { return 0 }
    return 44
  }
  if name == "protocol-retire" {
    configure(45)
    if protocol_terminal_cycle() && cycle_snapshot(3, 1, 1, 1) { return 0 }
    return 45
  }
  if name == "writer-retire-invalid" {
    configure(46)
    if writer_terminal_cycle(2) && cycle_snapshot(1, 0, 0, 0) { return 0 }
    return 46
  }
  if name == "writer-retire-denied" {
    configure(47)
    if writer_terminal_cycle(3) && cycle_snapshot(1, 0, 0, 0) { return 0 }
    return 47
  }
  if name == "writer-retire-timeout" {
    configure(48)
    if writer_terminal_cycle(4) && cycle_snapshot(1, 0, 0, 0) { return 0 }
    return 48
  }
  if name == "writer-retire-code-zero" {
    configure(49)
    if writer_terminal_cycle(5) && cycle_snapshot(1, 0, 0, 0) { return 0 }
    return 49
  }
  if name == "writer-retire-code-max" {
    configure(51)
    if writer_terminal_cycle(2147483647) && cycle_snapshot(1, 0, 0, 0) { return 0 }
    return 51
  }
  if name == "timeout-not-found" {
    configure(70)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 1)
      && timeout_failure_snapshot() { return 0 }
    return 70
  }
  if name == "timeout-invalid" {
    configure(71)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 2)
      && timeout_failure_snapshot() { return 0 }
    return 71
  }
  if name == "timeout-denied" {
    configure(72)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 3)
      && timeout_failure_snapshot() { return 0 }
    return 72
  }
  if name == "timeout-timeout" {
    configure(73)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 4)
      && timeout_failure_snapshot() { return 0 }
    return 73
  }
  if name == "timeout-code-zero" {
    configure(74)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 5)
      && timeout_failure_snapshot() { return 0 }
    return 74
  }
  if name == "timeout-code-max" {
    configure(75)
    if connect_error(pkg.kv.connect("127.0.0.1", 6380, options()), 2147483647)
      && timeout_failure_snapshot() { return 0 }
    return 75
  }
  return 100
}

fn run(args: array<str>) -> i32 {
  if args.len() == 4 && args[1] == "malformed" {
    field := malformed_field(args[2])
    operation := malformed_operation(args[3])
    if field == 0 || operation < 0 { return 96 }
    configure(60)
    pkg.kv.test_malformed_client(field, operation)
    return impossible_return()
  }
  if args.len() != 2 { return 95 }
  name := args[1]
  if name == "connect-zero-null" || name == "connect-error-nonnull"
    || name == "connect-negative-null" || name == "connect-negative-nonnull"
    || name == "reader-constructor-null" || name == "writer-constructor-null"
    || name == "writer-negative"
    || name == "reader-min" || name == "reader-below-status"
    || name == "reader-oversized" || name == "reader-negative-length"
    || name == "reader-zero-length" || name == "reader-positive-null"
    || name == "reader-positive-length" || name == "reader-negative-length-null"
    || name == "reader-zero-length-null" || name == "reader-positive-length-null"
    || name == "timeout-negative" {
    return run_abort_case(name)
  }
  return run_regular_case(name)
}

pub fn main(args: array<str>) -> Result<(), Error> {
  code := run(args)
  if code != 0 { process.exit(code as i64) }
  return Ok(())
}
"#;

const FAULT_C_STUB: &str = r#"
#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

_Static_assert(sizeof(void *) == 8, "Align pkg.kv fault owner requires 64-bit pointers");

enum {
    CONNECT_ZERO_NULL = 1,
    CONNECT_ERROR_NONNULL = 2,
    CONNECT_NEGATIVE_NULL = 3,
    CONNECT_NOT_FOUND = 4,
    CONNECT_INVALID = 5,
    CONNECT_DENIED = 6,
    CONNECT_TIMEOUT = 7,
    CONNECT_CODE_ZERO = 8,
    CONNECT_CODE_MAX = 9,
    WRITER_NEGATIVE = 10,
    WRITER_ZERO = 11,
    WRITER_NOT_FOUND = 12,
    WRITER_INVALID = 13,
    WRITER_DENIED = 14,
    WRITER_TIMEOUT = 15,
    WRITER_CODE_ZERO = 16,
    WRITER_CODE_MAX = 17,
    READER_CONSTRUCTOR_NULL = 18,
    WRITER_CONSTRUCTOR_NULL = 19,
    READER_MIN = 20,
    READER_BELOW_STATUS = 21,
    READER_OVERSIZED = 22,
    READER_NEGATIVE_LENGTH = 23,
    READER_ZERO_LENGTH = 24,
    READER_POSITIVE_NULL = 25,
    READER_POSITIVE_LENGTH = 26,
    READER_NEGATIVE_LENGTH_NULL = 27,
    READER_ZERO_LENGTH_NULL = 28,
    READER_POSITIVE_LENGTH_NULL = 29,
    READER_NEGATIVE_NULL = 30,
    READER_NEGATIVE_NONNULL = 31,
    READER_ZERO_NULL = 32,
    READER_ZERO_NONNULL = 33,
    READER_POSITIVE = 34,
    READER_CAPACITY = 35,
    NORMAL_DROP = 40,
    SERVER_REUSE = 41,
    DECODE_REUSE = 42,
    WRITER_RETIRE = 43,
    READER_RETIRE = 44,
    PROTOCOL_RETIRE = 45,
    WRITER_RETIRE_INVALID = 46,
    WRITER_RETIRE_DENIED = 47,
    WRITER_RETIRE_TIMEOUT = 48,
    WRITER_RETIRE_CODE_ZERO = 49,
    STATE_GOLDEN = 50,
    WRITER_RETIRE_CODE_MAX = 51,
    CONNECT_NEGATIVE_NONNULL = 52,
    MALFORMED = 60,
    TIMEOUT_NOT_FOUND = 70,
    TIMEOUT_INVALID = 71,
    TIMEOUT_DENIED = 72,
    TIMEOUT_TIMEOUT = 73,
    TIMEOUT_CODE_ZERO = 74,
    TIMEOUT_CODE_MAX = 75,
    TIMEOUT_NEGATIVE = 76,
};

static int connection_token;
static int reader_token;
static int writer_token;
static int buffer_token;
static uint8_t storage[32768];

static int32_t selected_case;
static int32_t forbidden;
static int32_t protocol_errors;
static int32_t connect_calls;
static int32_t timeout_calls;
static int32_t reader_ctor_calls;
static int32_t writer_ctor_calls;
static int32_t constructor_events[4];
static int32_t constructor_event_count;
static int32_t writer_calls;
static int32_t reader_calls;
static int32_t reader_product_calls;
static int32_t header_calls;
static int32_t buffer_new_calls;
static int32_t buffer_free_calls;
static int32_t cleanup_events[8];
static int32_t cleanup_event_count;
static int32_t state_match_calls;
static int32_t state_install_calls;
static int32_t state_semantic_calls;
static const uint8_t *view_pointer;
static int64_t view_length;

enum {
    STATE_IMAGE_LIVE = 0,
    STATE_IMAGE_CLOSED = 1,
    STATE_IMAGE_PERMUTED = 2,
    STATE_IMAGE_BYTES = 40,
};

static const int64_t state_cap_token = INT64_C(19088743);
static const int64_t state_permuted_cap_token = INT64_C(124076833);

static void guard_native(void) {
    if (forbidden) {
        _Exit(97);
    }
}

static void constructor_event(int32_t event) {
    if (constructor_event_count >= 4) {
        _Exit(96);
    }
    constructor_events[constructor_event_count++] = event;
}

static void cleanup_event(int32_t event) {
    if (cleanup_event_count >= 8) {
        _Exit(96);
    }
    cleanup_events[cleanup_event_count++] = event;
}

static void state_image_store(
    uint8_t image[STATE_IMAGE_BYTES],
    size_t offset,
    const void *value,
    size_t width
) {
    if (offset > STATE_IMAGE_BYTES || width > STATE_IMAGE_BYTES - offset) {
        _Exit(96);
    }
    memcpy(image + offset, value, width);
}

// Independent native-layout oracle. These literals and offsets come from the normative 40-byte
// ledger, not from package source or its injected corruption helper. Typed memcpy deliberately
// follows the host ABI used by raw.load/raw.store while fixing every byte, including padding.
static void build_state_image(uint8_t image[STATE_IMAGE_BYTES], int32_t kind) {
    uint32_t version = UINT32_C(1);
    uint8_t lifecycle = kind == STATE_IMAGE_CLOSED ? UINT8_C(1) : UINT8_C(0);
    uint8_t reserved_byte = UINT8_C(0);
    uint16_t reserved_word = UINT16_C(0);
    void *connection = &connection_token;
    void *reader = &reader_token;
    void *writer = &writer_token;
    int64_t cap = state_cap_token;

    if (kind == STATE_IMAGE_CLOSED) {
        connection = NULL;
        reader = NULL;
        writer = NULL;
    } else if (kind == STATE_IMAGE_PERMUTED) {
        connection = &reader_token;
        reader = &writer_token;
        writer = &connection_token;
        cap = state_permuted_cap_token;
    } else if (kind != STATE_IMAGE_LIVE) {
        _Exit(96);
    }

    memset(image, 0, STATE_IMAGE_BYTES);
    state_image_store(image, 0, &version, sizeof(version));
    state_image_store(image, 4, &lifecycle, sizeof(lifecycle));
    state_image_store(image, 5, &reserved_byte, sizeof(reserved_byte));
    state_image_store(image, 6, &reserved_word, sizeof(reserved_word));
    state_image_store(image, 8, &connection, sizeof(connection));
    state_image_store(image, 16, &reader, sizeof(reader));
    state_image_store(image, 24, &writer, sizeof(writer));
    state_image_store(image, 32, &cap, sizeof(cap));
}

static int32_t state_matches(void *state, int32_t kind) {
    uint8_t expected[STATE_IMAGE_BYTES];
    state_match_calls += 1;
    if (selected_case != STATE_GOLDEN || state == NULL) {
        return 0;
    }
    build_state_image(expected, kind);
    return memcmp(state, expected, STATE_IMAGE_BYTES) == 0;
}

static void state_install(void *state, int32_t kind) {
    uint8_t expected[STATE_IMAGE_BYTES];
    state_install_calls += 1;
    if (selected_case != STATE_GOLDEN || state == NULL) {
        _Exit(96);
    }
    build_state_image(expected, kind);
    memset(state, 0xa5, STATE_IMAGE_BYTES);
    memcpy(state, expected, STATE_IMAGE_BYTES);
}

int32_t align_kv_fault_state_matches_live(void *state) {
    return state_matches(state, STATE_IMAGE_LIVE);
}

int32_t align_kv_fault_state_matches_closed(void *state) {
    return state_matches(state, STATE_IMAGE_CLOSED);
}

void align_kv_fault_state_install_live(void *state) {
    state_install(state, STATE_IMAGE_LIVE);
}

void align_kv_fault_state_install_closed(void *state) {
    state_install(state, STATE_IMAGE_CLOSED);
}

void align_kv_fault_state_install_permuted(void *state) {
    state_install(state, STATE_IMAGE_PERMUTED);
}

int32_t align_kv_fault_state_live_semantics(void *reader, void *writer, int64_t cap) {
    state_semantic_calls += 1;
    return selected_case == STATE_GOLDEN
        && reader == &reader_token
        && writer == &writer_token
        && cap == state_cap_token;
}

int32_t align_kv_fault_state_closed_semantics(void *reader, void *writer, int64_t cap) {
    state_semantic_calls += 1;
    return selected_case == STATE_GOLDEN
        && reader == NULL
        && writer == NULL
        && cap == state_cap_token;
}

int32_t align_kv_fault_state_permuted_semantics(void *reader, void *writer, int64_t cap) {
    state_semantic_calls += 1;
    return selected_case == STATE_GOLDEN
        && reader == &writer_token
        && writer == &connection_token
        && cap == state_permuted_cap_token;
}

void align_kv_fault_configure(int32_t case_id) {
    selected_case = case_id;
    forbidden = 0;
    protocol_errors = 0;
    connect_calls = 0;
    timeout_calls = 0;
    reader_ctor_calls = 0;
    writer_ctor_calls = 0;
    constructor_event_count = 0;
    writer_calls = 0;
    reader_calls = 0;
    reader_product_calls = 0;
    header_calls = 0;
    buffer_new_calls = 0;
    buffer_free_calls = 0;
    cleanup_event_count = 0;
    state_match_calls = 0;
    state_install_calls = 0;
    state_semantic_calls = 0;
    view_pointer = NULL;
    view_length = 0;
    memset(storage, 0, sizeof(storage));
}

void align_kv_fault_arm_forbidden(void) {
    forbidden = 1;
}

void align_kv_fault_unexpected_return(void) {
    _Exit(98);
}

int32_t align_kv_fault_tcp_connect(
    const uint8_t *host,
    int64_t host_len,
    int64_t port,
    int64_t timeout_ns,
    void **out
) {
    guard_native();
    connect_calls += 1;
    if (host == NULL || host_len != 9 || memcmp(host, "127.0.0.1", 9) != 0
        || port != 6380 || timeout_ns != 1000000 || out == NULL) {
        protocol_errors += 1;
    }
    if (selected_case == CONNECT_ZERO_NULL) {
        if (out != NULL) *out = NULL;
        forbidden = 1;
        return 0;
    }
    if (selected_case == CONNECT_ERROR_NONNULL) {
        if (out != NULL) *out = &connection_token;
        forbidden = 1;
        return 1;
    }
    if (selected_case == CONNECT_NEGATIVE_NULL) {
        if (out != NULL) *out = NULL;
        forbidden = 1;
        return -1;
    }
    if (selected_case == CONNECT_NEGATIVE_NONNULL) {
        if (out != NULL) *out = &connection_token;
        forbidden = 1;
        return -1;
    }
    if (selected_case >= CONNECT_NOT_FOUND && selected_case <= CONNECT_CODE_MAX) {
        if (out != NULL) *out = NULL;
        switch (selected_case) {
            case CONNECT_NOT_FOUND: return 1;
            case CONNECT_INVALID: return 2;
            case CONNECT_DENIED: return 3;
            case CONNECT_TIMEOUT: return 4;
            case CONNECT_CODE_ZERO: return 5;
            default: return INT32_MAX;
        }
    }
    if (out != NULL) *out = &connection_token;
    return 0;
}

int32_t align_kv_fault_set_io_timeout(void *connection, int64_t timeout_ns) {
    guard_native();
    timeout_calls += 1;
    if (connection != &connection_token || timeout_ns != 2000000) {
        protocol_errors += 1;
    }
    switch (selected_case) {
        case TIMEOUT_NOT_FOUND: return 1;
        case TIMEOUT_INVALID: return 2;
        case TIMEOUT_DENIED: return 3;
        case TIMEOUT_TIMEOUT: return 4;
        case TIMEOUT_CODE_ZERO: return 5;
        case TIMEOUT_CODE_MAX: return INT32_MAX;
        case TIMEOUT_NEGATIVE:
            forbidden = 1;
            return -1;
        default: return 0;
    }
}

void *align_kv_fault_conn_reader(void *connection) {
    guard_native();
    reader_ctor_calls += 1;
    constructor_event(1);
    if (connection != &connection_token) protocol_errors += 1;
    if (selected_case == READER_CONSTRUCTOR_NULL) {
        forbidden = 1;
        return NULL;
    }
    return &reader_token;
}

void *align_kv_fault_conn_writer(void *connection) {
    guard_native();
    writer_ctor_calls += 1;
    constructor_event(2);
    if (connection != &connection_token) protocol_errors += 1;
    if (selected_case == WRITER_CONSTRUCTOR_NULL) {
        forbidden = 1;
        return NULL;
    }
    return &writer_token;
}

void align_kv_fault_conn_free(void *connection) {
    guard_native();
    cleanup_event(3);
    if (connection != &connection_token) protocol_errors += 1;
}

void align_kv_fault_reader_free(void *reader) {
    guard_native();
    cleanup_event(2);
    if (reader != &reader_token) protocol_errors += 1;
}

void align_kv_fault_writer_free(void *writer) {
    guard_native();
    cleanup_event(1);
    if (writer != &writer_token) protocol_errors += 1;
}

void align_kv_fault_state_free(void *state) {
    guard_native();
    cleanup_event(4);
    if (state == NULL) protocol_errors += 1;
    free(state);
}

static int32_t writer_status(void) {
    switch (selected_case) {
        case WRITER_NEGATIVE: return -1;
        case WRITER_NOT_FOUND:
        case WRITER_RETIRE: return 1;
        case WRITER_INVALID:
        case WRITER_RETIRE_INVALID: return 2;
        case WRITER_DENIED:
        case WRITER_RETIRE_DENIED: return 3;
        case WRITER_TIMEOUT:
        case WRITER_RETIRE_TIMEOUT: return 4;
        case WRITER_CODE_ZERO:
        case WRITER_RETIRE_CODE_ZERO: return 5;
        case WRITER_CODE_MAX:
        case WRITER_RETIRE_CODE_MAX: return INT32_MAX;
        default: return 0;
    }
}

int32_t align_kv_fault_writer_write(
    void *writer,
    const uint8_t *bytes,
    int64_t length
) {
    guard_native();
    writer_calls += 1;
    if (selected_case >= NORMAL_DROP && writer != &writer_token) {
        protocol_errors += 1;
    }
    if (length < 0 || (length > 0 && bytes == NULL)) {
        protocol_errors += 1;
    }
    int32_t status = writer_status();
    if (selected_case == WRITER_NEGATIVE) forbidden = 1;
    return status;
}

static int64_t set_view(const uint8_t *pointer, int64_t length, int64_t count) {
    view_pointer = pointer;
    view_length = length;
    return count;
}

void align_kv_fault_reader_product_validated(int32_t product) {
    guard_native();
    reader_product_calls += 1;
    int32_t expected = 0;
    switch (selected_case) {
        case READER_NEGATIVE_NULL:
        case READER_NEGATIVE_NONNULL:
        case READER_RETIRE:
            expected = 1;
            break;
        case READER_ZERO_NULL:
        case READER_ZERO_NONNULL:
            expected = 2;
            break;
        case READER_POSITIVE:
        case READER_CAPACITY:
        case NORMAL_DROP:
        case SERVER_REUSE:
        case DECODE_REUSE:
        case PROTOCOL_RETIRE:
            expected = 3;
            break;
        default:
            protocol_errors += 1;
            return;
    }
    if (product != expected) protocol_errors += 1;
}

int64_t align_kv_fault_reader_read(void *reader, void *buffer) {
    guard_native();
    reader_calls += 1;
    if (selected_case >= NORMAL_DROP && reader != &reader_token) {
        protocol_errors += 1;
    }
    if (buffer != &buffer_token) protocol_errors += 1;
    switch (selected_case) {
        case READER_MIN:
            forbidden = 1;
            return INT64_MIN;
        case READER_BELOW_STATUS:
            forbidden = 1;
            return -2147483648LL;
        case READER_OVERSIZED:
            forbidden = 1;
            return 32769;
        case READER_NEGATIVE_LENGTH:
            return set_view(storage, 1, -1);
        case READER_ZERO_LENGTH:
            return set_view(storage, 1, 0);
        case READER_POSITIVE_NULL:
            return set_view(NULL, 1, 1);
        case READER_POSITIVE_LENGTH:
            return set_view(storage, 0, 1);
        case READER_NEGATIVE_LENGTH_NULL:
            return set_view(NULL, 1, -1);
        case READER_ZERO_LENGTH_NULL:
            return set_view(NULL, 1, 0);
        case READER_POSITIVE_LENGTH_NULL:
            return set_view(NULL, 0, 1);
        case READER_NEGATIVE_NULL:
        case READER_RETIRE:
            return set_view(NULL, 0, -1);
        case READER_NEGATIVE_NONNULL:
            return set_view(storage, 0, -2147483647LL);
        case READER_ZERO_NULL:
            return set_view(NULL, 0, 0);
        case READER_ZERO_NONNULL:
            return set_view(storage, 0, 0);
        case READER_POSITIVE:
            storage[0] = '$';
            return set_view(storage, 1, 1);
        case READER_CAPACITY:
            return set_view(storage, 32768, 32768);
        case NORMAL_DROP:
            memcpy(storage, "$-1\r\n", 5);
            return set_view(storage, 5, 5);
        case SERVER_REUSE:
            if (reader_calls == 1) {
                memcpy(storage, "-ERR nope\r\n", 11);
                return set_view(storage, 11, 11);
            }
            memcpy(storage, "$-1\r\n", 5);
            return set_view(storage, 5, 5);
        case DECODE_REUSE:
            if (reader_calls == 1) {
                storage[0] = '$'; storage[1] = '1'; storage[2] = '\r'; storage[3] = '\n';
                storage[4] = 0xff; storage[5] = '\r'; storage[6] = '\n';
                return set_view(storage, 7, 7);
            }
            memcpy(storage, "$-1\r\n", 5);
            return set_view(storage, 5, 5);
        case PROTOCOL_RETIRE:
            storage[0] = '?';
            return set_view(storage, 1, 1);
        default:
            return set_view(NULL, 0, 0);
    }
}

void *align_kv_fault_buffer_new(int64_t capacity) {
    guard_native();
    buffer_new_calls += 1;
    if (capacity != 32768) protocol_errors += 1;
    return &buffer_token;
}

int64_t align_kv_fault_buffer_capacity(void *buffer) {
    guard_native();
    if (buffer != &buffer_token) protocol_errors += 1;
    return 32768;
}

void align_kv_fault_buffer_bytes(void *buffer, void *out) {
    guard_native();
    header_calls += 1;
    if (selected_case == READER_MIN || selected_case == READER_BELOW_STATUS
        || selected_case == READER_OVERSIZED) {
        _Exit(97);
    }
    if (buffer != &buffer_token || out == NULL) {
        _Exit(96);
    }
    memcpy(out, &view_pointer, sizeof(view_pointer));
    memcpy((uint8_t *)out + sizeof(view_pointer), &view_length, sizeof(view_length));
    if (selected_case == READER_NEGATIVE_LENGTH || selected_case == READER_ZERO_LENGTH
        || selected_case == READER_POSITIVE_NULL || selected_case == READER_POSITIVE_LENGTH
        || selected_case == READER_NEGATIVE_LENGTH_NULL
        || selected_case == READER_ZERO_LENGTH_NULL
        || selected_case == READER_POSITIVE_LENGTH_NULL) {
        forbidden = 1;
    }
}

void align_kv_fault_buffer_free(void *buffer) {
    guard_native();
    buffer_free_calls += 1;
    if (buffer != &buffer_token) protocol_errors += 1;
}

int32_t align_kv_fault_protocol_errors(void) { return protocol_errors; }
int32_t align_kv_fault_connect_calls(void) { return connect_calls; }
int32_t align_kv_fault_timeout_calls(void) { return timeout_calls; }
int32_t align_kv_fault_reader_ctor_calls(void) { return reader_ctor_calls; }
int32_t align_kv_fault_writer_ctor_calls(void) { return writer_ctor_calls; }
int32_t align_kv_fault_constructor_event_count(void) { return constructor_event_count; }
int32_t align_kv_fault_constructor_event(int32_t index) {
    if (index < 0 || index >= constructor_event_count) return -1;
    return constructor_events[index];
}
int32_t align_kv_fault_writer_calls(void) { return writer_calls; }
int32_t align_kv_fault_reader_calls(void) { return reader_calls; }
int32_t align_kv_fault_reader_product_calls(void) { return reader_product_calls; }
int32_t align_kv_fault_header_calls(void) { return header_calls; }
int32_t align_kv_fault_buffer_new_calls(void) { return buffer_new_calls; }
int32_t align_kv_fault_buffer_free_calls(void) { return buffer_free_calls; }
int32_t align_kv_fault_cleanup_event_count(void) { return cleanup_event_count; }
int32_t align_kv_fault_cleanup_event(int32_t index) {
    if (index < 0 || index >= cleanup_event_count) return -1;
    return cleanup_events[index];
}
int32_t align_kv_fault_state_golden_counts(void) {
    return selected_case == STATE_GOLDEN
        && protocol_errors == 0
        && state_match_calls == 5
        && state_install_calls == 3
        && state_semantic_calls == 5;
}
"#;

#[test]
fn native_products_authenticated_state_and_cleanup_order_fail_closed() {
    if !backend_available() {
        return;
    }
    if !cc_available_bounded() {
        return;
    }

    let (root, internal) = fault_sources();
    let files = [
        ("pkg/kv/internal/resource.align", internal.as_str()),
        ("pkg/kv.align", root.as_str()),
        ("main.align", FAULT_MAIN),
    ];
    let built = build_fault_exe(&files, FAULT_C_STUB);

    let regular = [
        "connect-not-found",
        "connect-invalid",
        "connect-denied",
        "connect-timeout",
        "connect-code-zero",
        "connect-code-max",
        "writer-zero",
        "writer-not-found",
        "writer-invalid",
        "writer-denied",
        "writer-timeout",
        "writer-code-zero",
        "writer-code-max",
        "reader-negative-null",
        "reader-negative-nonnull",
        "reader-zero-null",
        "reader-zero-nonnull",
        "reader-positive",
        "reader-capacity",
        "normal-drop",
        "state-golden",
        "server-reuse",
        "decode-reuse",
        "writer-retire",
        "reader-retire",
        "protocol-retire",
        "writer-retire-invalid",
        "writer-retire-denied",
        "writer-retire-timeout",
        "writer-retire-code-zero",
        "writer-retire-code-max",
        "timeout-not-found",
        "timeout-invalid",
        "timeout-denied",
        "timeout-timeout",
        "timeout-code-zero",
        "timeout-code-max",
    ];
    for case in regular {
        let status = run_bounded(&built.exe, &[case], case);
        assert_eq!(status.code(), Some(0), "regular fault case `{case}`");
    }

    let aborts = [
        "connect-zero-null",
        "connect-error-nonnull",
        "connect-negative-null",
        "connect-negative-nonnull",
        "reader-constructor-null",
        "writer-constructor-null",
        "writer-negative",
        "reader-min",
        "reader-below-status",
        "reader-oversized",
        "reader-negative-length",
        "reader-zero-length",
        "reader-positive-null",
        "reader-positive-length",
        "reader-negative-length-null",
        "reader-zero-length-null",
        "reader-positive-length-null",
        "timeout-negative",
    ];
    for case in aborts {
        let status = run_bounded(&built.exe, &[case], case);
        assert_eq!(
            status.code(),
            Some(1),
            "`{case}` must take package `process.abort`, not native-forbidden (97) or returned (98)",
        );
    }

    let fields = [
        "version",
        "lifecycle",
        "reserved-byte",
        "reserved-word",
        "live-connection-null",
        "live-reader-null",
        "live-writer-null",
        "cap-below",
        "cap-above",
        "closed-connection-live",
        "closed-reader-live",
        "closed-writer-live",
    ];
    let operations = ["get", "set", "delete", "drop"];
    for field in fields {
        for operation in operations {
            let case = format!("malformed-{field}-{operation}");
            let status = run_bounded(&built.exe, &["malformed", field, operation], &case);
            assert_eq!(
                status.code(),
                Some(1),
                "`{case}` must authenticate the complete state before any armed native call",
            );
        }
    }

    // `raw.alloc`/`raw.free` are compiler builtins, so replacing source-level extern spellings
    // cannot count the parser header/input pair without overriding the process-wide allocator.
    // Each completed parser path instead pins buffer new/free parity. The internal test variant
    // rewrites only the final state free, making writer -> reader -> connection -> state observable.
}
