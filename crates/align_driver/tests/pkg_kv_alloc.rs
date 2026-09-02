//! `pkg.kv` v1 allocation owner for successful GET payloads and `Error.Server` messages.
//!
//! `align_rt_alloc_count` deliberately does not count builder growth through `realloc`, while
//! `align_rt_free_count` does count the eventual non-null free. The owner therefore compares a
//! canonical empty reply with its nonempty twin and takes a second snapshot after the published
//! value leaves scope. That separates the parser builder's one scratch free from the final owned
//! string's one allocation and exactly-once Drop.

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
const LINK_CHILD_ENV: &str = "ALIGN_PKG_KV_ALLOC_LINK_CHILD";
const LINK_EXE_ENV: &str = "ALIGN_PKG_KV_ALLOC_LINK_EXE";
const LINK_OBJECT_COUNT_ENV: &str = "ALIGN_PKG_KV_ALLOC_LINK_OBJECT_COUNT";
const LINK_LIBRARY_COUNT_ENV: &str = "ALIGN_PKG_KV_ALLOC_LINK_LIBRARY_COUNT";

// Keep the test dependency and its feature live. The generated executable resolves these same
// C-ABI definitions from the feature-built `libalign_runtime.a` selected by `link_objects`.
const _: extern "C" fn() -> i64 = align_runtime::align_rt_alloc_count;
const _: extern "C" fn() -> i64 = align_runtime::align_rt_free_count;

fn sleep_until(deadline: Instant, interval: Duration) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        std::thread::sleep(remaining.min(interval));
    }
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

fn rewrite_native_symbols(mut source: String, rewrites: &[NativeRewrite]) -> String {
    let spans = identifier_spans(&source);
    let mut edits = Vec::new();
    for rewrite in rewrites {
        assert_eq!(
            native_inventory(&source, rewrite.original),
            (rewrite.declarations, rewrite.calls, 0),
            "allocation harness declaration/live-call inventory drift for `{}`",
            rewrite.original,
        );
        assert_eq!(
            native_inventory(&source, rewrite.replacement),
            (0, 0, 0),
            "allocation harness replacement `{}` already exists",
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
            "allocation harness rewritten inventory drift for `{}`",
            rewrite.replacement,
        );
    }
    source
}

fn allocation_sources() -> (String, String) {
    // Isolate only the transport handles and bytes. Allocation/free symbols and both counter
    // getters remain the real feature-built runtime definitions under test.
    let root_source = fixture("apps/kv/pkg/kv.align");
    assert_eq!(
        (root_source.len(), source_fingerprint(root_source)),
        (22_712, 0x8aa6_186e_52cf_8e79),
        "pkg.kv source changed; refresh the allocation rewrite inventory",
    );
    let native = [
        NativeRewrite {
            original: "align_rt_tcp_connect",
            replacement: "align_kv_alloc_tcp_connect",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_tcp_conn_set_io_timeout",
            replacement: "align_kv_alloc_set_io_timeout",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_tcp_conn_free",
            replacement: "align_kv_alloc_conn_free",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_tcp_conn_reader",
            replacement: "align_kv_alloc_conn_reader",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_tcp_conn_writer",
            replacement: "align_kv_alloc_conn_writer",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_io_reader_read",
            replacement: "align_kv_alloc_reader_read",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_io_reader_free",
            replacement: "align_kv_alloc_reader_free",
            declarations: 1,
            calls: 0,
        },
        NativeRewrite {
            original: "align_rt_io_writer_write",
            replacement: "align_kv_alloc_writer_write",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_io_writer_free",
            replacement: "align_kv_alloc_writer_free",
            declarations: 1,
            calls: 0,
        },
        NativeRewrite {
            original: "align_rt_buffer_new",
            replacement: "align_kv_alloc_buffer_new",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_buffer_bytes",
            replacement: "align_kv_alloc_buffer_bytes",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_buffer_capacity",
            replacement: "align_kv_alloc_buffer_capacity",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_buffer_free",
            replacement: "align_kv_alloc_buffer_free",
            declarations: 1,
            calls: 1,
        },
    ];
    let root = rewrite_native_symbols(root_source.to_owned(), &native);

    let internal_source = fixture("apps/kv/pkg/kv/internal/resource.align");
    assert_eq!(
        (internal_source.len(), source_fingerprint(internal_source)),
        (4_007, 0x37db_4750_f57b_2011),
        "pkg.kv internal resource source changed; refresh the allocation rewrite inventory",
    );
    let internal_native = [
        NativeRewrite {
            original: "align_rt_tcp_conn_free",
            replacement: "align_kv_alloc_conn_free",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_io_reader_free",
            replacement: "align_kv_alloc_reader_free",
            declarations: 1,
            calls: 1,
        },
        NativeRewrite {
            original: "align_rt_io_writer_free",
            replacement: "align_kv_alloc_writer_free",
            declarations: 1,
            calls: 1,
        },
    ];
    let internal = rewrite_native_symbols(internal_source.to_owned(), &internal_native);
    (root, internal)
}

struct AllocationProject {
    dir: PathBuf,
}

impl Drop for AllocationProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct BuiltAllocationExe {
    exe: PathBuf,
    _project: AllocationProject,
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
        self.child.as_mut().expect("armed allocation child guard")
    }

    fn cleanup(
        &mut self,
        known_status: Option<ExitStatus>,
        stdout: Option<DrainHandle>,
        stderr: Option<DrainHandle>,
    ) -> CleanupReport {
        let deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
        let child = self.child.as_mut().expect("armed allocation child guard");
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
        .name(format!("pkg-kv-alloc-{stream}-drain"))
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

fn try_run_bounded(
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

fn run_bounded(command: &mut Command, timeout: Duration, label: &str) -> ProcessOutput {
    try_run_bounded(command, timeout, label)
        .unwrap_or_else(|error| panic!("spawn `{label}`: {error}"))
}

fn cc_available_bounded() -> bool {
    let mut command = Command::new("cc");
    command.arg("--version");
    let output = match try_run_bounded(&mut command, Duration::from_secs(5), "cc --version") {
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
fn pkg_kv_allocation_link_child() {
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
    let executable = PathBuf::from(
        std::env::var_os(LINK_EXE_ENV).expect("missing allocation link-child executable"),
    );
    link_objects(&object_refs, &executable, &libraries, Profile::Release)
        .expect("link pkg.kv allocation executable");
}

fn link_objects_bounded(objects: &[PathBuf], exe: &Path, libraries: &[String]) {
    let mut command =
        Command::new(std::env::current_exe().expect("resolve pkg.kv allocation test executable"));
    command
        .args(["--exact", "pkg_kv_allocation_link_child", "--nocapture"])
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
    let linked = run_bounded(
        &mut command,
        PROCESS_TIMEOUT,
        "link pkg.kv allocation executable",
    );
    assert!(
        linked.status.success(),
        "pkg.kv allocation executable link failed as {}; stdout `{}`; stderr `{}`",
        linked.status,
        String::from_utf8_lossy(&linked.stdout),
        String::from_utf8_lossy(&linked.stderr),
    );
}

fn build_allocation_exe(files: &[(&str, &str)], c_source: &str) -> BuiltAllocationExe {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("align-pkg-kv-alloc-{}-{nonce}", std::process::id(),));
    assert_eq!(dir.parent(), Some(std::env::temp_dir().as_path()));
    std::fs::create_dir(&dir).expect("create unique allocation-project directory");
    let project = AllocationProject { dir };
    let dir = &project.dir;

    for &(name, source) in files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create allocation-project module directory");
        }
        std::fs::write(path, source).expect("write allocation-project source");
    }

    let entry = dir.join("main.align");
    let entry_source = std::fs::read_to_string(&entry).expect("read allocation-project entry");
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

    let c_path = dir.join("alloc.c");
    let c_object = dir.join("alloc.o");
    std::fs::write(&c_path, c_source).expect("write C allocation fixture");
    let compiled = run_bounded(
        Command::new("cc")
            .args(["-std=c11", "-c", "-O0"])
            .arg(&c_path)
            .arg("-o")
            .arg(&c_object),
        PROCESS_TIMEOUT,
        "compile pkg.kv allocation C fixture",
    );
    assert!(
        compiled.status.success(),
        "C allocation fixture failed: {}",
        String::from_utf8_lossy(&compiled.stderr),
    );
    objects.push(c_object);

    let exe = dir.join(format!("pkg-kv-alloc{}", std::env::consts::EXE_SUFFIX));
    let ordered_libraries = order_link_libs(&link_libraries);
    link_objects_bounded(&objects, &exe, &ordered_libraries);
    BuiltAllocationExe {
        exe,
        _project: project,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AllocationObservation {
    held_alloc: i64,
    held_free: i64,
    after_alloc: i64,
    after_free: i64,
}

fn run_case(executable: &Path, case: &str) -> AllocationObservation {
    let output = run_bounded(Command::new(executable).arg(case), CASE_TIMEOUT, case);
    assert!(
        output.status.success(),
        "allocation case `{case}` failed as {}; stdout `{}`; stderr `{}`",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let text = std::str::from_utf8(&output.stdout)
        .unwrap_or_else(|error| panic!("allocation case `{case}` emitted invalid UTF-8: {error}"));
    let values = text
        .lines()
        .map(|line| {
            line.parse::<i64>().unwrap_or_else(|error| {
                panic!("allocation case `{case}` emitted non-i64 `{line}`: {error}")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        values.len(),
        4,
        "allocation case `{case}` must emit exactly four counter deltas; stdout `{text}`",
    );
    AllocationObservation {
        held_alloc: values[0],
        held_free: values[1],
        after_alloc: values[2],
        after_free: values[3],
    }
}

fn assert_empty_has_no_final_owner(label: &str, empty: AllocationObservation) {
    assert_eq!(
        empty.after_alloc, empty.held_alloc,
        "{label}: returning the canonical empty string must not allocate during cleanup",
    );
    assert_eq!(
        empty.after_free, empty.held_free,
        "{label}: canonical empty string Drop must not free a result buffer",
    );
}

fn assert_nonempty_has_one_final_owner(
    label: &str,
    empty: AllocationObservation,
    nonempty: AllocationObservation,
) {
    assert_eq!(
        nonempty.held_alloc,
        empty.held_alloc + 1,
        "{label}: the nonempty published string must add exactly one allocation",
    );
    assert_eq!(
        nonempty.held_free,
        empty.held_free + 1,
        "{label}: the nonempty parser payload adds one scratch-builder free before publication",
    );
    assert_eq!(
        nonempty.after_alloc, nonempty.held_alloc,
        "{label}: result cleanup must not allocate",
    );
    assert_eq!(
        nonempty.after_free,
        nonempty.held_free + 1,
        "{label}: the live published string must be freed exactly once on Drop",
    );
    assert_eq!(
        nonempty.after_alloc,
        empty.after_alloc + 1,
        "{label}: only the final owned string is additionally counted as allocated",
    );
    assert_eq!(
        nonempty.after_free,
        empty.after_free + 2,
        "{label}: the paired free delta is parser scratch plus the final owned string",
    );
}

const ALLOCATION_MAIN: &str = r#"module main
import std.process
import pkg.kv

extern "C" {
  fn align_rt_alloc_count() -> i64
  fn align_rt_free_count() -> i64
  fn align_kv_alloc_configure(case_id: i32)
  fn align_kv_alloc_string_shape(value: str, length: i64) -> i32
  fn align_kv_alloc_snapshot_valid() -> i32
}

Probe {
  valid: bool,
  before_alloc: i64,
  before_free: i64,
  held_alloc: i64,
  held_free: i64,
}

fn allocation_count() -> i64 {
  unsafe { return align_rt_alloc_count() }
}

fn free_count() -> i64 {
  unsafe { return align_rt_free_count() }
}

fn invalid_probe(before_alloc: i64, before_free: i64) -> Probe = Probe {
  valid: false,
  before_alloc: before_alloc,
  before_free: before_free,
  held_alloc: allocation_count(),
  held_free: free_count(),
}

fn held_string_probe(
  borrow value: string,
  expected: str,
  before_alloc: i64,
  before_free: i64,
) -> Probe {
  view := value.bytes().as_str() else { return invalid_probe(before_alloc, before_free) }
  shape := unsafe { align_kv_alloc_string_shape(view, value.len()) }
  return Probe {
    valid: value == expected && shape == 1,
    before_alloc: before_alloc,
    before_free: before_free,
    held_alloc: allocation_count(),
    held_free: free_count(),
  }
}

fn probe_get(borrow mut owner: pkg.kv.client, expected: str) -> Probe {
  before_alloc := allocation_count()
  before_free := free_count()
  result := pkg.kv.get(owner, "k")
  return match result {
    Ok(value) => match value {
      Some(text) => {
        moved := text
        held_string_probe(moved, expected, before_alloc, before_free)
      }
      None => invalid_probe(before_alloc, before_free)
    }
    Err(_) => invalid_probe(before_alloc, before_free)
  }
}

fn probe_server(borrow mut owner: pkg.kv.client, expected: str) -> Probe {
  before_alloc := allocation_count()
  before_free := free_count()
  result := pkg.kv.get(owner, "k")
  return match result {
    Err(error) => match error {
      Server(message) => {
        moved := message
        held_string_probe(moved, expected, before_alloc, before_free)
      }
      _ => invalid_probe(before_alloc, before_free)
    }
    Ok(_) => invalid_probe(before_alloc, before_free)
  }
}

fn options() -> pkg.kv.ClientOptions = pkg.kv.ClientOptions {
  connect_timeout_ns: 1000000,
  io_timeout_ns: 2000000,
  max_response_bytes: 32,
}

fn case_id(name: str) -> i32 {
  if name == "get-empty" { return 1 }
  if name == "get-nonempty" { return 2 }
  if name == "server-empty" { return 3 }
  if name == "server-nonempty" { return 4 }
  return 0
}

fn run(name: str) -> i32 {
  id := case_id(name)
  if id == 0 { return 10 }
  unsafe { align_kv_alloc_configure(id) }
  mut owner := pkg.kv.connect("127.0.0.1", 6380, options()) else { return 11 }
  probe := if id == 1 {
    probe_get(owner, "")
  } else if id == 2 {
    probe_get(owner, "abc")
  } else if id == 3 {
    probe_server(owner, "")
  } else {
    probe_server(owner, "ERR")
  }
  after_alloc := allocation_count()
  after_free := free_count()
  if !probe.valid { return 12 }
  unsafe {
    if align_kv_alloc_snapshot_valid() != 1 { return 13 }
  }
  print(probe.held_alloc - probe.before_alloc)
  print(probe.held_free - probe.before_free)
  print(after_alloc - probe.before_alloc)
  print(after_free - probe.before_free)
  return 0
}

pub fn main(args: array<str>) -> Result<(), Error> {
  if args.len() != 2 { process.exit(14) }
  code := run(args[1])
  if code != 0 { process.exit(code as i64) }
  return Ok(())
}
"#;

const ALLOCATION_C_STUB: &str = r#"
#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

_Static_assert(sizeof(void *) == 8, "Align pkg.kv allocation owner requires 64-bit pointers");

static uint8_t connection_token;
static uint8_t reader_token;
static uint8_t writer_token;
static uint8_t buffer_token;

static int32_t selected_case;
static int32_t protocol_errors;
static int32_t connect_calls;
static int32_t timeout_calls;
static int32_t reader_ctor_calls;
static int32_t writer_ctor_calls;
static int32_t writer_calls;
static int32_t reader_calls;
static int32_t header_calls;
static int32_t buffer_new_calls;
static int32_t buffer_free_calls;
static int32_t connection_free_calls;
static int32_t reader_free_calls;
static int32_t writer_free_calls;
static const uint8_t *view_pointer;
static int64_t view_length;

static const uint8_t GET_EMPTY[] = "$0\r\n\r\n";
static const uint8_t GET_NONEMPTY[] = "$3\r\nabc\r\n";
static const uint8_t SERVER_EMPTY[] = "-\r\n";
static const uint8_t SERVER_NONEMPTY[] = "-ERR\r\n";

static void validate_final_cleanup(void) {
    if (protocol_errors != 0 || connection_free_calls != 1
        || reader_free_calls != 1 || writer_free_calls != 1) {
        _Exit(90);
    }
}

void align_kv_alloc_configure(int32_t case_id) {
    selected_case = case_id;
    protocol_errors = 0;
    connect_calls = 0;
    timeout_calls = 0;
    reader_ctor_calls = 0;
    writer_ctor_calls = 0;
    writer_calls = 0;
    reader_calls = 0;
    header_calls = 0;
    buffer_new_calls = 0;
    buffer_free_calls = 0;
    connection_free_calls = 0;
    reader_free_calls = 0;
    writer_free_calls = 0;
    view_pointer = NULL;
    view_length = 0;
    if (case_id < 1 || case_id > 4 || atexit(validate_final_cleanup) != 0) {
        _Exit(91);
    }
}

int32_t align_kv_alloc_string_shape(const uint8_t *value, int64_t length) {
    if (selected_case == 1 || selected_case == 3) {
        return value == NULL && length == 0;
    }
    if (selected_case == 2) {
        return value != NULL && length == 3 && memcmp(value, "abc", 3) == 0;
    }
    if (selected_case == 4) {
        return value != NULL && length == 3 && memcmp(value, "ERR", 3) == 0;
    }
    return 0;
}

int32_t align_kv_alloc_tcp_connect(
    const uint8_t *host,
    int64_t host_len,
    int64_t port,
    int64_t timeout_ns,
    void **out
) {
    connect_calls += 1;
    if (host == NULL || host_len != 9 || memcmp(host, "127.0.0.1", 9) != 0
        || port != 6380 || timeout_ns != 1000000 || out == NULL) {
        protocol_errors += 1;
    }
    if (out != NULL) *out = &connection_token;
    return 0;
}

int32_t align_kv_alloc_set_io_timeout(void *connection, int64_t timeout_ns) {
    timeout_calls += 1;
    if (connection != &connection_token || timeout_ns != 2000000) {
        protocol_errors += 1;
    }
    return 0;
}

void *align_kv_alloc_conn_reader(void *connection) {
    reader_ctor_calls += 1;
    if (connection != &connection_token) protocol_errors += 1;
    return &reader_token;
}

void *align_kv_alloc_conn_writer(void *connection) {
    writer_ctor_calls += 1;
    if (connection != &connection_token) protocol_errors += 1;
    return &writer_token;
}

void align_kv_alloc_conn_free(void *connection) {
    connection_free_calls += 1;
    if (connection != &connection_token) protocol_errors += 1;
}

void align_kv_alloc_reader_free(void *reader) {
    reader_free_calls += 1;
    if (reader != &reader_token) protocol_errors += 1;
}

void align_kv_alloc_writer_free(void *writer) {
    writer_free_calls += 1;
    if (writer != &writer_token) protocol_errors += 1;
}

int32_t align_kv_alloc_writer_write(
    void *writer,
    const uint8_t *bytes,
    int64_t length
) {
    writer_calls += 1;
    if (writer != &writer_token || length < 0 || (length > 0 && bytes == NULL)) {
        protocol_errors += 1;
    }
    return 0;
}

int64_t align_kv_alloc_reader_read(void *reader, void *buffer) {
    reader_calls += 1;
    if (reader != &reader_token || buffer != &buffer_token || reader_calls != 1) {
        protocol_errors += 1;
    }
    switch (selected_case) {
        case 1:
            view_pointer = GET_EMPTY;
            view_length = (int64_t)(sizeof(GET_EMPTY) - 1);
            break;
        case 2:
            view_pointer = GET_NONEMPTY;
            view_length = (int64_t)(sizeof(GET_NONEMPTY) - 1);
            break;
        case 3:
            view_pointer = SERVER_EMPTY;
            view_length = (int64_t)(sizeof(SERVER_EMPTY) - 1);
            break;
        case 4:
            view_pointer = SERVER_NONEMPTY;
            view_length = (int64_t)(sizeof(SERVER_NONEMPTY) - 1);
            break;
        default:
            _Exit(92);
    }
    return view_length;
}

void *align_kv_alloc_buffer_new(int64_t capacity) {
    buffer_new_calls += 1;
    if (capacity != 32768) protocol_errors += 1;
    return &buffer_token;
}

int64_t align_kv_alloc_buffer_capacity(void *buffer) {
    if (buffer != &buffer_token) protocol_errors += 1;
    return 32768;
}

void align_kv_alloc_buffer_bytes(void *buffer, void *out) {
    header_calls += 1;
    if (buffer != &buffer_token || out == NULL || view_pointer == NULL || view_length <= 0) {
        _Exit(93);
    }
    memcpy(out, &view_pointer, sizeof(view_pointer));
    memcpy((uint8_t *)out + sizeof(view_pointer), &view_length, sizeof(view_length));
}

void align_kv_alloc_buffer_free(void *buffer) {
    buffer_free_calls += 1;
    if (buffer != &buffer_token) protocol_errors += 1;
}

int32_t align_kv_alloc_snapshot_valid(void) {
    return protocol_errors == 0
        && connect_calls == 1
        && timeout_calls == 1
        && reader_ctor_calls == 1
        && writer_ctor_calls == 1
        && writer_calls == 3
        && reader_calls == 1
        && header_calls == 1
        && buffer_new_calls == 1
        && buffer_free_calls == 1
        && connection_free_calls == 0
        && reader_free_calls == 0
        && writer_free_calls == 0;
}
"#;

#[test]
fn empty_and_nonempty_get_and_server_own_exact_final_string_allocation() {
    if !backend_available() {
        return;
    }
    if !cc_available_bounded() {
        return;
    }

    let (root, internal) = allocation_sources();
    let files = [
        ("pkg/kv/internal/resource.align", internal.as_str()),
        ("pkg/kv.align", root.as_str()),
        ("main.align", ALLOCATION_MAIN),
    ];
    let built = build_allocation_exe(&files, ALLOCATION_C_STUB);

    let get_empty = run_case(&built.exe, "get-empty");
    let get_nonempty = run_case(&built.exe, "get-nonempty");
    assert_empty_has_no_final_owner("GET empty Some(\"\")", get_empty);
    assert_nonempty_has_one_final_owner("GET nonempty Some(\"abc\")", get_empty, get_nonempty);

    let server_empty = run_case(&built.exe, "server-empty");
    let server_nonempty = run_case(&built.exe, "server-nonempty");
    assert_empty_has_no_final_owner("Error.Server empty", server_empty);
    assert_nonempty_has_one_final_owner("Error.Server nonempty", server_empty, server_nonempty);
}
