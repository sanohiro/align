//! `pkg.kv` v1 control-flow, retained-owner, and exact boundary owner.
//!
//! The generated package is the shipped source with only its native transport symbol spellings
//! redirected to a deterministic C fixture.  Every connection gets distinct native shell tokens;
//! the fixture rejects duplicate or out-of-order cleanup, so moving a `client` without nulling its
//! source is observable.  Reply reads deliberately overwrite one shared C buffer, making retained
//! GET and Server strings prove ownership rather than accidentally surviving as stable views.

mod common;
use common::*;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// Keep the feature-built counter definitions live in the runtime archive used below.  Besides the
// native-shell order log, exact raw allocation/free deltas prove every lifecycle client state is
// freed once and invalid boundary inputs return before even the package's temporary output slot.
const _: extern "C" fn() -> i64 = align_runtime::align_rt_alloc_count;
const _: extern "C" fn() -> i64 = align_runtime::align_rt_free_count;
const _: unsafe extern "C" fn(*const u8, i64, i64, i64, *mut *mut align_runtime::TcpConn) -> i32 =
    align_runtime::align_rt_tcp_connect;
const _: unsafe extern "C" fn(*mut align_runtime::TcpConn, i64) -> i32 =
    align_runtime::align_rt_tcp_conn_set_io_timeout;
const _: unsafe extern "C" fn(*mut align_runtime::TcpConn) = align_runtime::align_rt_tcp_conn_free;
const _: unsafe extern "C" fn(*mut align_runtime::TcpConn) -> *mut align_runtime::Reader =
    align_runtime::align_rt_tcp_conn_reader;
const _: unsafe extern "C" fn(*mut align_runtime::TcpConn) -> *mut align_runtime::Writer =
    align_runtime::align_rt_tcp_conn_writer;
const _: unsafe extern "C" fn(*mut align_runtime::Reader, *mut align_runtime::Buffer) -> i64 =
    align_runtime::align_rt_io_reader_read;
const _: unsafe extern "C" fn(*mut align_runtime::Reader) = align_runtime::align_rt_io_reader_free;
const _: unsafe extern "C" fn(*mut align_runtime::Writer, *const u8, i64) -> i32 =
    align_runtime::align_rt_io_writer_write;
const _: unsafe extern "C" fn(*mut align_runtime::Writer) = align_runtime::align_rt_io_writer_free;
const _: extern "C" fn(i64) -> *mut align_runtime::Buffer = align_runtime::align_rt_buffer_new;
const _: unsafe extern "C" fn(*mut align_runtime::Buffer, *mut align_runtime::AlignStr) =
    align_runtime::align_rt_buffer_bytes;
const _: unsafe extern "C" fn(*mut align_runtime::Buffer) -> i64 =
    align_runtime::align_rt_buffer_capacity;
const _: unsafe extern "C" fn(*mut align_runtime::Buffer) = align_runtime::align_rt_buffer_free;

const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
const CASE_TIMEOUT: Duration = Duration::from_secs(2);
const PIPE_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const LINK_CHILD_ENV: &str = "ALIGN_PKG_KV_CONTROL_LINK_CHILD";
const LINK_EXE_ENV: &str = "ALIGN_PKG_KV_CONTROL_LINK_EXE";
const LINK_OBJECT_COUNT_ENV: &str = "ALIGN_PKG_KV_CONTROL_LINK_OBJECT_COUNT";
const LINK_LIBRARY_COUNT_ENV: &str = "ALIGN_PKG_KV_CONTROL_LINK_LIBRARY_COUNT";

struct RewriteContract {
    original: &'static str,
    replacement: &'static str,
    declaration: &'static str,
    live_calls: &'static [(&'static str, usize)],
    c_signature: &'static str,
}

const NATIVE_REWRITES: [RewriteContract; 13] = [
    RewriteContract {
        original: "align_rt_tcp_connect",
        replacement: "align_kv_control_tcp_connect",
        declaration: "fn align_rt_tcp_connect(host: str, host_len: i64, port: i64, timeout_ns: i64, out: raw) -> i32",
        live_calls: &[("status := align_rt_tcp_connect(\n", 1)],
        c_signature: "int32_t align_kv_control_tcp_connect(\n    const uint8_t *host,\n    int64_t host_length,\n    int64_t port,\n    int64_t timeout_ns,\n    void **output\n)",
    },
    RewriteContract {
        original: "align_rt_tcp_conn_set_io_timeout",
        replacement: "align_kv_control_set_io_timeout",
        declaration: "fn align_rt_tcp_conn_set_io_timeout(connection: raw, timeout_ns: i64) -> i32",
        live_calls: &[(
            "timeout_status := align_rt_tcp_conn_set_io_timeout(connection, options.io_timeout_ns)",
            1,
        )],
        c_signature: "int32_t align_kv_control_set_io_timeout(void *connection, int64_t timeout_ns)",
    },
    RewriteContract {
        original: "align_rt_tcp_conn_free",
        replacement: "align_kv_control_conn_free",
        declaration: "fn align_rt_tcp_conn_free(connection: raw)",
        live_calls: &[("      align_rt_tcp_conn_free(connection)\n", 1)],
        c_signature: "void align_kv_control_conn_free(void *connection)",
    },
    RewriteContract {
        original: "align_rt_tcp_conn_reader",
        replacement: "align_kv_control_conn_reader",
        declaration: "fn align_rt_tcp_conn_reader(connection: raw) -> raw",
        live_calls: &[("reader := align_rt_tcp_conn_reader(connection)", 1)],
        c_signature: "void *align_kv_control_conn_reader(void *connection)",
    },
    RewriteContract {
        original: "align_rt_tcp_conn_writer",
        replacement: "align_kv_control_conn_writer",
        declaration: "fn align_rt_tcp_conn_writer(connection: raw) -> raw",
        live_calls: &[("writer := align_rt_tcp_conn_writer(connection)", 1)],
        c_signature: "void *align_kv_control_conn_writer(void *connection)",
    },
    RewriteContract {
        original: "align_rt_io_reader_read",
        replacement: "align_kv_control_reader_read",
        declaration: "fn align_rt_io_reader_read(reader: raw, output: raw) -> i64",
        live_calls: &[("count := align_rt_io_reader_read(reader, buffer)", 1)],
        c_signature: "int64_t align_kv_control_reader_read(void *reader, void *buffer)",
    },
    RewriteContract {
        original: "align_rt_io_reader_free",
        replacement: "align_kv_control_reader_free",
        declaration: "fn align_rt_io_reader_free(reader: raw)",
        live_calls: &[],
        c_signature: "void align_kv_control_reader_free(void *reader)",
    },
    RewriteContract {
        original: "align_rt_io_writer_write",
        replacement: "align_kv_control_writer_write",
        declaration: "fn align_rt_io_writer_write(writer: raw, bytes: slice<u8>, length: i64) -> i32",
        live_calls: &[(
            "status := align_rt_io_writer_write(writer, bytes, bytes.len())",
            1,
        )],
        c_signature: "int32_t align_kv_control_writer_write(\n    void *writer,\n    const uint8_t *bytes,\n    int64_t length\n)",
    },
    RewriteContract {
        original: "align_rt_io_writer_free",
        replacement: "align_kv_control_writer_free",
        declaration: "fn align_rt_io_writer_free(writer: raw)",
        live_calls: &[],
        c_signature: "void align_kv_control_writer_free(void *writer)",
    },
    RewriteContract {
        original: "align_rt_buffer_new",
        replacement: "align_kv_control_buffer_new",
        declaration: "fn align_rt_buffer_new(capacity: i64) -> raw",
        live_calls: &[("buffer := align_rt_buffer_new(READ_CHUNK_BYTES)", 1)],
        c_signature: "void *align_kv_control_buffer_new(int64_t capacity)",
    },
    RewriteContract {
        original: "align_rt_buffer_bytes",
        replacement: "align_kv_control_buffer_bytes",
        declaration: "fn align_rt_buffer_bytes(buffer: raw, out: raw)",
        live_calls: &[("align_rt_buffer_bytes(buffer, header)", 1)],
        c_signature: "void align_kv_control_buffer_bytes(void *buffer, void *output)",
    },
    RewriteContract {
        original: "align_rt_buffer_capacity",
        replacement: "align_kv_control_buffer_capacity",
        declaration: "fn align_rt_buffer_capacity(buffer: raw) -> i64",
        live_calls: &[("align_rt_buffer_capacity(buffer) != READ_CHUNK_BYTES", 1)],
        c_signature: "int64_t align_kv_control_buffer_capacity(void *buffer)",
    },
    RewriteContract {
        original: "align_rt_buffer_free",
        replacement: "align_kv_control_buffer_free",
        declaration: "fn align_rt_buffer_free(buffer: raw)",
        live_calls: &[("    align_rt_buffer_free(buffer)\n", 1)],
        c_signature: "void align_kv_control_buffer_free(void *buffer)",
    },
];

const INTERNAL_REWRITES: [RewriteContract; 3] = [
    RewriteContract {
        original: "align_rt_tcp_conn_free",
        replacement: "align_kv_control_conn_free",
        declaration: "fn align_rt_tcp_conn_free(connection: raw)",
        live_calls: &[("    align_rt_tcp_conn_free(connection)\n", 1)],
        c_signature: "void align_kv_control_conn_free(void *connection)",
    },
    RewriteContract {
        original: "align_rt_io_reader_free",
        replacement: "align_kv_control_reader_free",
        declaration: "fn align_rt_io_reader_free(reader: raw)",
        live_calls: &[("    align_rt_io_reader_free(reader)\n", 1)],
        c_signature: "void align_kv_control_reader_free(void *reader)",
    },
    RewriteContract {
        original: "align_rt_io_writer_free",
        replacement: "align_kv_control_writer_free",
        declaration: "fn align_rt_io_writer_free(writer: raw)",
        live_calls: &[("    align_rt_io_writer_free(writer)\n", 1)],
        c_signature: "void align_kv_control_writer_free(void *writer)",
    },
];

fn source_fingerprint(source: &str) -> u64 {
    source.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn identifier_ranges(source: &str) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if bytes[index] == b'"' {
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == b'"' {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
        } else if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            ranges.push((start, index));
        } else {
            index += 1;
        }
    }
    ranges
}

fn lexical_positions(source: &str, identifier: &str) -> Vec<usize> {
    identifier_ranges(source)
        .into_iter()
        .filter_map(|(start, end)| (&source[start..end] == identifier).then_some(start))
        .collect()
}

fn inventoried_positions(
    source: &str,
    identifier: &str,
    fragment: &str,
    expected: usize,
    label: &str,
) -> Vec<usize> {
    let matches = source.match_indices(fragment).collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        expected,
        "{label} exact fragment drift for `{fragment}`",
    );
    let relative = fragment
        .match_indices(identifier)
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    assert_eq!(
        relative.len(),
        1,
        "{label} inventory fragment must name `{identifier}` exactly once",
    );
    matches
        .into_iter()
        .map(|(start, _)| start + relative[0])
        .collect()
}

fn validate_rewrite_inventory(source: &str, contracts: &[RewriteContract], label: &str) {
    for contract in contracts {
        assert!(
            lexical_positions(source, contract.replacement).is_empty(),
            "{label} already declares control symbol `{}`",
            contract.replacement,
        );
        let mut inventory =
            inventoried_positions(source, contract.original, contract.declaration, 1, label);
        for &(fragment, expected) in contract.live_calls {
            inventory.extend(inventoried_positions(
                source,
                contract.original,
                fragment,
                expected,
                label,
            ));
        }
        inventory.sort_unstable();
        let lexical = lexical_positions(source, contract.original);
        assert_eq!(
            lexical, inventory,
            "{label} lexical declaration/live-call inventory drift for `{}`",
            contract.original,
        );
    }
}

fn replace_required(source: &str, contracts: &[RewriteContract], label: &str) -> String {
    validate_rewrite_inventory(source, contracts, label);
    let ranges = identifier_ranges(source);
    let mut rewritten = String::with_capacity(source.len());
    let mut copied = 0;
    for (start, end) in ranges {
        rewritten.push_str(&source[copied..start]);
        let identifier = &source[start..end];
        let replacement = contracts
            .iter()
            .find(|contract| contract.original == identifier)
            .map_or(identifier, |contract| contract.replacement);
        rewritten.push_str(replacement);
        copied = end;
    }
    rewritten.push_str(&source[copied..]);
    for contract in contracts {
        assert!(lexical_positions(&rewritten, contract.original).is_empty());
        assert_eq!(
            lexical_positions(&rewritten, contract.replacement).len(),
            1 + contract
                .live_calls
                .iter()
                .map(|(_, expected)| expected)
                .sum::<usize>(),
        );
    }
    rewritten
}

fn assert_c_shim_contract(root: &str, c_source: &str) {
    assert_eq!(std::mem::size_of::<*mut std::ffi::c_void>(), 8);
    assert_eq!(std::mem::size_of::<align_runtime::AlignStr>(), 16);
    assert_eq!(std::mem::align_of::<align_runtime::AlignStr>(), 8);
    assert_eq!(std::mem::offset_of!(align_runtime::AlignStr, ptr), 0);
    assert_eq!(std::mem::offset_of!(align_runtime::AlignStr, len), 8);
    for contract in &NATIVE_REWRITES {
        assert_eq!(
            c_source.match_indices(contract.c_signature).count(),
            1,
            "C shim signature drift for `{}`",
            contract.replacement,
        );
    }
    for fragment in [
        "output := raw.alloc(8)\n    raw.store(output, 0, raw.null())",
        "connection: raw := raw.load(output, 0)\n    raw.free(output)",
        "header := raw.alloc(16)\n    input := raw.alloc(40)",
        "pointer: raw := raw.load(header, 0)\n    length: i64 := raw.load(header, 8)",
    ] {
        assert_eq!(
            root.match_indices(fragment).count(),
            1,
            "Align native ABI layout drift for `{fragment}`",
        );
    }
    for fragment in [
        "_Static_assert(sizeof(void *) == 8",
        "_Static_assert(sizeof(int64_t) == 8",
        "_Static_assert(sizeof(int32_t) == 4",
        "*output = &connection_tokens[index];",
        "memcpy(output, &pointer, sizeof(pointer));\n    memcpy((uint8_t *)output + sizeof(pointer), &reply_length, sizeof(reply_length));",
    ] {
        assert_eq!(
            c_source.match_indices(fragment).count(),
            1,
            "C native ABI layout drift for `{fragment}`",
        );
    }
}

fn control_sources() -> (String, String) {
    let root = fixture("apps/kv/pkg/kv.align");
    let internal = fixture("apps/kv/pkg/kv/internal/resource.align");
    assert_eq!(
        (root.len(), source_fingerprint(root)),
        (22_712, 0x8aa6_186e_52cf_8e79)
    );
    assert_eq!(
        (internal.len(), source_fingerprint(internal)),
        (4_007, 0x37db_4750_f57b_2011),
    );
    assert_c_shim_contract(root, CONTROL_C);
    (
        replace_required(root, &NATIVE_REWRITES, "pkg.kv"),
        replace_required(internal, &INTERNAL_REWRITES, "pkg.kv.internal.resource"),
    )
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

fn drain_bounded<R>(mut reader: R, captured: &Arc<Mutex<Captured>>, cancel: &AtomicBool)
where
    R: Read,
{
    let mut chunk = [0_u8; 4096];
    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let count = match reader.read(&mut chunk) {
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
        .name(format!("pkg-kv-control-{stream}-drain"))
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
            // SAFETY: `pid` is the positive id returned for the child we placed in its own process
            // group.  Negating it targets exactly that group; SIGKILL needs no borrowed memory.
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

fn try_run_bounded(
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

fn run_bounded(command: &mut Command, timeout: Duration, label: &str) -> std::process::Output {
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

struct TempProject {
    dir: PathBuf,
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct BuiltControlExe {
    exe: PathBuf,
    _project: TempProject,
}

fn link_env_count(name: &str) -> usize {
    std::env::var(name)
        .unwrap_or_else(|error| panic!("read pkg.kv control link-child `{name}`: {error}"))
        .parse()
        .unwrap_or_else(|error| panic!("parse pkg.kv control link-child `{name}`: {error}"))
}

#[test]
fn pkg_kv_control_link_child() {
    if std::env::var_os(LINK_CHILD_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let object_count = link_env_count(LINK_OBJECT_COUNT_ENV);
    let objects = (0..object_count)
        .map(|index| {
            PathBuf::from(
                std::env::var_os(format!("{LINK_CHILD_ENV}_OBJECT_{index}"))
                    .unwrap_or_else(|| panic!("missing pkg.kv control link-child object {index}")),
            )
        })
        .collect::<Vec<_>>();
    let object_refs = objects.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let library_count = link_env_count(LINK_LIBRARY_COUNT_ENV);
    let libraries = (0..library_count)
        .map(|index| {
            std::env::var(format!("{LINK_CHILD_ENV}_LIBRARY_{index}")).unwrap_or_else(|error| {
                panic!("read pkg.kv control link-child library {index}: {error}")
            })
        })
        .collect::<Vec<_>>();
    let executable = PathBuf::from(
        std::env::var_os(LINK_EXE_ENV).expect("missing pkg.kv control link-child executable"),
    );
    link_objects(&object_refs, &executable, &libraries, Profile::Release)
        .unwrap_or_else(|error| panic!("production pkg.kv control link failed: {error}"));
}

fn link_objects_bounded(objects: &[PathBuf], executable: &Path, libraries: &[String]) {
    let mut command =
        Command::new(std::env::current_exe().expect("resolve pkg.kv control test executable"));
    command
        .args(["--exact", "pkg_kv_control_link_child", "--nocapture"])
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
    let output = run_bounded(
        &mut command,
        PROCESS_TIMEOUT,
        "link pkg.kv control executable",
    );
    assert!(
        output.status.success(),
        "production pkg.kv control link child failed as {}; stdout `{}`; stderr `{}`",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn build_control_exe(files: &[(&str, &str)], c_source: &str) -> BuiltControlExe {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "align-pkg-kv-control-{}-{nonce}",
        std::process::id(),
    ));
    assert!(
        directory.parent() == Some(std::env::temp_dir().as_path()),
        "temporary control-project path escaped the system temp directory",
    );
    std::fs::create_dir(&directory).expect("create pkg.kv control-project directory");
    let project = TempProject { dir: directory };

    for &(name, source) in files {
        let path = project.dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create control-project module directory");
        }
        std::fs::write(path, source).expect("write control-project source");
    }

    let entry = project.dir.join("main.align");
    let entry_source = std::fs::read_to_string(&entry).expect("read control-project entry");
    let mut source_map = SourceMap::new();
    let walk = build_per_unit(&mut source_map, &entry.display().to_string(), &entry_source);
    assert!(
        !walk.diags.has_errors(),
        "unexpected per-unit control-project errors:\n{}",
        align_driver::format_diagnostics(&source_map, &walk.diags),
    );

    let mut objects = Vec::with_capacity(walk.units.len() + 1);
    let mut link_libraries = Vec::new();
    for (index, unit) in walk.units.iter().enumerate() {
        let object = project.dir.join(format!("unit-{index}.o"));
        emit_object_file(
            &unit.mir,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
        )
        .unwrap_or_else(|error| panic!("codegen for control unit `{}`: {error}", unit.unit));
        for library in &unit.mir.link_libs {
            if !link_libraries.contains(library) {
                link_libraries.push(library.clone());
            }
        }
        objects.push(object);
    }

    let c_path = project.dir.join("control.c");
    let c_object = project.dir.join("control.o");
    std::fs::write(&c_path, c_source).expect("write pkg.kv control C fixture");
    let compiled = run_bounded(
        Command::new("cc")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-c", "-O0"])
            .arg(&c_path)
            .arg("-o")
            .arg(&c_object),
        PROCESS_TIMEOUT,
        "compile pkg.kv control C fixture",
    );
    assert!(
        compiled.status.success(),
        "C control fixture failed as {}; stdout `{}`; stderr `{}`",
        compiled.status,
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr),
    );
    objects.push(c_object);

    let executable = project
        .dir
        .join(format!("pkg-kv-control{}", std::env::consts::EXE_SUFFIX));
    link_objects_bounded(&objects, &executable, &link_libraries);

    BuiltControlExe {
        exe: executable,
        _project: project,
    }
}

fn run_case(executable: &Path, case: &str) {
    let output = run_bounded(Command::new(executable).arg(case), CASE_TIMEOUT, case);
    assert!(
        output.status.success(),
        "pkg.kv control case `{case}` failed as {}; stdout `{}`; stderr `{}`",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        output.stdout.is_empty() && output.stderr.is_empty(),
        "pkg.kv control case `{case}` must be silent; stdout `{}`; stderr `{}`",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

const CONTROL_MAIN: &str = r#"module main
import std.process
import pkg.kv

extern "C" {
  fn align_kv_control_configure(case_id: i32, expected_connections: i32)
  fn align_kv_control_verify() -> i32
}

MAX_TIMEOUT_NS: i64 := 86400000000000

fn options() -> pkg.kv.ClientOptions = pkg.kv.ClientOptions {
  connect_timeout_ns: 1000000,
  io_timeout_ns: 2000000,
  max_response_bytes: 64,
}

fn boundary_options(connect_timeout_ns: i64, io_timeout_ns: i64) -> pkg.kv.ClientOptions =
pkg.kv.ClientOptions {
  connect_timeout_ns: connect_timeout_ns,
  io_timeout_ns: io_timeout_ns,
  max_response_bytes: 64,
}

fn open() -> Result<pkg.kv.client, pkg.kv.Error> =
pkg.kv.connect("127.0.0.1", 6380, options())

fn invalid_open() -> Result<pkg.kv.client, pkg.kv.Error> =
pkg.kv.connect("127.0.0.1", 0, options())

fn consume(owner: pkg.kv.client) {}

fn relay(owner: pkg.kv.client) -> pkg.kv.client {
  moved := owner
  return moved
}

fn move_in_out() -> Result<(), pkg.kv.Error> {
  source := open()?
  moved := relay(source)
  consume(moved)
  return Ok(())
}

fn returned_owner() -> Result<pkg.kv.client, pkg.kv.Error> {
  source := open()?
  return Ok(source)
}

fn return_path() -> Result<(), pkg.kv.Error> {
  owner := returned_owner()?
  consume(owner)
  return Ok(())
}

fn replacement() -> Result<(), pkg.kv.Error> {
  mut owner := open()?
  owner = open()?
  return Ok(())
}

fn if_join(flag: bool) -> Result<(), pkg.kv.Error> {
  owner := if flag { open()? } else { open()? }
  consume(owner)
  return Ok(())
}

fn match_path() -> Result<(), pkg.kv.Error> {
  owner := match open() {
    Ok(value) => value
    Err(error) => { return Err(error) }
  }
  consume(owner)
  return Ok(())
}

fn match_early() -> Result<(), pkg.kv.Error> {
  held := open()?
  result := invalid_open()
  return match result {
    Ok(owner) => {
      consume(owner)
      Ok(())
    }
    Err(error) => Err(error)
  }
}

fn else_path() {
  owner := open() else { return }
  consume(owner)
}

fn else_early() {
  held := open() else { return }
  owner := invalid_open() else { return }
  consume(owner)
}

fn try_path() -> Result<(), pkg.kv.Error> {
  owner := open()?
  consume(owner)
  return Ok(())
}

fn try_early() -> Result<(), pkg.kv.Error> {
  held := open()?
  owner := invalid_open()?
  consume(owner)
  return Ok(())
}

fn keep_error(value: pkg.kv.Error) -> pkg.kv.Error = value

fn map_path() -> Result<(), pkg.kv.Error> {
  owner := open().map_err(keep_error)?
  consume(owner)
  return Ok(())
}

fn map_early() -> Result<(), pkg.kv.Error> {
  held := open()?
  owner := invalid_open().map_err(keep_error)?
  consume(owner)
  return Ok(())
}

fn branch_replacement(flag: bool) -> Result<(), pkg.kv.Error> {
  mut owner := open()?
  if flag { owner = open()? } else { owner = open()? }
  consume(owner)
  return Ok(())
}

fn loop_join() -> Result<(), pkg.kv.Error> {
  owner := loop { break open()? }
  consume(owner)
  return Ok(())
}

fn loop_replacement() -> Result<(), pkg.kv.Error> {
  mut owner := open()?
  mut done := false
  loop {
    if done { break }
    owner = open()?
    done = true
  }
  consume(owner)
  return Ok(())
}

fn early_return() -> Result<(), pkg.kv.Error> {
  owner := open()?
  return Ok(())
}

fn invalid_unit(result: Result<(), pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Invalid => true, _ => false }
  Ok(_) => false
}

fn run_lifecycle() -> Result<i32, pkg.kv.Error> {
  move_in_out()?
  return_path()?
  replacement()?
  if_join(true)?
  if_join(false)?
  match_path()?
  if !invalid_unit(match_early()) { return Err(pkg.kv.Error.Protocol) }
  else_path()
  else_early()
  try_path()?
  if !invalid_unit(try_early()) { return Err(pkg.kv.Error.Protocol) }
  map_path()?
  if !invalid_unit(map_early()) { return Err(pkg.kv.Error.Protocol) }
  branch_replacement(true)?
  branch_replacement(false)?
  loop_join()?
  loop_replacement()?
  early_return()?
  return Ok(1)
}

fn returned_get(borrow mut owner: pkg.kv.client) -> Result<string, pkg.kv.Error> {
  value := pkg.kv.get(owner, "first")?
  return match value {
    Some(text) => Ok(text)
    None => Err(pkg.kv.Error.Protocol)
  }
}

fn returned_server(borrow mut owner: pkg.kv.client) -> Result<string, pkg.kv.Error> {
  result := pkg.kv.get(owner, "server")
  return match result {
    Err(error) => match error {
      Server(message) => Ok(message)
      _ => Err(pkg.kv.Error.Protocol)
    }
    Ok(_) => Err(pkg.kv.Error.Protocol)
  }
}

fn required_get(
  borrow mut owner: pkg.kv.client,
  key: str,
) -> Result<string, pkg.kv.Error> {
  value := pkg.kv.get(owner, key)?
  return match value {
    Some(text) => Ok(text)
    None => Err(pkg.kv.Error.Protocol)
  }
}

fn run_strings() -> Result<i32, pkg.kv.Error> {
  mut owner := pkg.kv.connect("127.0.0.1", 6381, options())?
  get_value := returned_get(owner)?
  later := required_get(owner, "later")?
  if later != "NEXT" || get_value != "get-owned" {
    return Err(pkg.kv.Error.Protocol)
  }
  server_message := returned_server(owner)?
  after := required_get(owner, "after")?
  if after != "AFTER" || server_message != "ERR-owned" {
    return Err(pkg.kv.Error.Protocol)
  }
  return Ok(2)
}

fn invalid_client(result: Result<pkg.kv.client, pkg.kv.Error>) -> bool = match result {
  Err(error) => match error { Invalid => true, _ => false }
  Ok(_) => false
}

fn run_boundary_accept() -> Result<i32, pkg.kv.Error> {
  owner := pkg.kv.connect(
    "127.0.0.1",
    65535,
    boundary_options(MAX_TIMEOUT_NS, MAX_TIMEOUT_NS),
  )?
  consume(owner)
  return Ok(3)
}

fn run_boundary_reject() -> Result<i32, pkg.kv.Error> {
  valid := boundary_options(1, 1)
  if !invalid_client(pkg.kv.connect("127.0.0.1", 0, valid)) {
    return Err(pkg.kv.Error.Protocol)
  }
  if !invalid_client(pkg.kv.connect("127.0.0.1", 65536, valid)) {
    return Err(pkg.kv.Error.Protocol)
  }
  if !invalid_client(pkg.kv.connect(
    "127.0.0.1",
    1,
    boundary_options(0, 1),
  )) { return Err(pkg.kv.Error.Protocol) }
  if !invalid_client(pkg.kv.connect(
    "127.0.0.1",
    1,
    boundary_options(MAX_TIMEOUT_NS + 1, 1),
  )) { return Err(pkg.kv.Error.Protocol) }
  if !invalid_client(pkg.kv.connect(
    "127.0.0.1",
    1,
    boundary_options(1, 0),
  )) { return Err(pkg.kv.Error.Protocol) }
  if !invalid_client(pkg.kv.connect(
    "127.0.0.1",
    1,
    boundary_options(1, MAX_TIMEOUT_NS + 1),
  )) { return Err(pkg.kv.Error.Protocol) }
  return Ok(4)
}

fn selected_case(name: str) -> i32 {
  if name == "lifecycle" { return 1 }
  if name == "strings" { return 2 }
  if name == "boundary-accept" { return 3 }
  if name == "boundary-reject" { return 4 }
  return 0
}

fn expected_connections(case_id: i32) -> i32 {
  if case_id == 1 { return 22 }
  if case_id == 2 || case_id == 3 { return 1 }
  return 0
}

fn run(name: str) -> i32 {
  case_id := selected_case(name)
  if case_id == 0 { return 40 }
  unsafe { align_kv_control_configure(case_id, expected_connections(case_id)) }
  result := if case_id == 1 {
    run_lifecycle()
  } else if case_id == 2 {
    run_strings()
  } else if case_id == 3 {
    run_boundary_accept()
  } else {
    run_boundary_reject()
  }
  value := result else { return 41 }
  if value != case_id { return 42 }
  unsafe {
    if align_kv_control_verify() != 1 { return 43 }
  }
  return 0
}

pub fn main(args: array<str>) -> Result<(), Error> {
  if args.len() != 2 { process.exit(44) }
  status := run(args[1])
  if status != 0 { process.exit(status as i64) }
  return Ok(())
}
"#;

const CONTROL_C: &str = r#"
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

_Static_assert(sizeof(void *) == 8, "pkg.kv control owner requires 64-bit pointers");
_Static_assert(sizeof(int64_t) == 8, "pkg.kv control owner requires 64-bit int64_t");
_Static_assert(sizeof(int32_t) == 4, "pkg.kv control owner requires 32-bit int32_t");

int64_t align_rt_alloc_count(void);
int64_t align_rt_free_count(void);

enum {
    CASE_LIFECYCLE = 1,
    CASE_STRINGS = 2,
    CASE_BOUNDARY_ACCEPT = 3,
    CASE_BOUNDARY_REJECT = 4,
    MAX_CONNECTIONS = 32,
};

static uint8_t connection_tokens[MAX_CONNECTIONS];
static uint8_t reader_tokens[MAX_CONNECTIONS];
static uint8_t writer_tokens[MAX_CONNECTIONS];
static uint8_t buffer_token;
static uint8_t reply_storage[64];

static int32_t selected_case;
static int32_t expected_connections;
static int32_t configured;
static int32_t verified;
static int32_t errors;
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
static int32_t construction_stage[MAX_CONNECTIONS];
static int32_t cleanup_stage[MAX_CONNECTIONS];
static int32_t buffer_live;
static int64_t reply_length;
static int64_t alloc_before;
static int64_t free_before;

static int32_t token_index(void *value, uint8_t *tokens) {
    for (int32_t index = 0; index < MAX_CONNECTIONS; index += 1) {
        if (value == &tokens[index]) return index;
    }
    return -1;
}

static int32_t state_is_valid(void) {
    if (!configured || errors != 0 || connect_calls != expected_connections
        || timeout_calls != expected_connections
        || reader_ctor_calls != expected_connections
        || writer_ctor_calls != expected_connections
        || connection_free_calls != expected_connections
        || reader_free_calls != expected_connections
        || writer_free_calls != expected_connections
        || buffer_live != 0) {
        return 0;
    }
    for (int32_t index = 0; index < expected_connections; index += 1) {
        if (construction_stage[index] != 4 || cleanup_stage[index] != 3) return 0;
    }
    int64_t allocated = align_rt_alloc_count() - alloc_before;
    int64_t freed = align_rt_free_count() - free_before;
    if (selected_case == CASE_LIFECYCLE
        && (allocated != INT64_C(2) * expected_connections
            || freed != INT64_C(2) * expected_connections)) {
        return 0;
    }
    if (selected_case == CASE_BOUNDARY_ACCEPT && (allocated != 2 || freed != 2)) {
        return 0;
    }
    if (selected_case == CASE_BOUNDARY_REJECT && (allocated != 0 || freed != 0)) {
        return 0;
    }
    if (selected_case == CASE_STRINGS) {
        return writer_calls == 12 && reader_calls == 4 && header_calls == 4
            && buffer_new_calls == 4 && buffer_free_calls == 4;
    }
    return writer_calls == 0 && reader_calls == 0 && header_calls == 0
        && buffer_new_calls == 0 && buffer_free_calls == 0;
}

static void validate_at_exit(void) {
    // `main`'s argv owner was allocated before `configure` and is freed after Align returns, so the
    // raw counter delta intentionally changes once more between `verify` and this callback.  The
    // full shell/order/allocation product is therefore checked at `verify`; every later native
    // shell call is itself fail-fast, and atexit only proves that the in-scope check completed.
    if (!verified) {
        fprintf(
            stderr,
            "case=%d verified=%d alloc=%lld free=%lld connections=%d/%d cleanup=%d/%d/%d errors=%d\n",
            selected_case,
            verified,
            (long long)(align_rt_alloc_count() - alloc_before),
            (long long)(align_rt_free_count() - free_before),
            connect_calls,
            expected_connections,
            writer_free_calls,
            reader_free_calls,
            connection_free_calls,
            errors
        );
        fflush(stderr);
        _Exit(90);
    }
}

void align_kv_control_configure(int32_t case_id, int32_t expected) {
    selected_case = case_id;
    expected_connections = expected;
    configured = 1;
    verified = 0;
    errors = 0;
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
    buffer_live = 0;
    reply_length = 0;
    memset(construction_stage, 0, sizeof(construction_stage));
    memset(cleanup_stage, 0, sizeof(cleanup_stage));
    memset(reply_storage, 0, sizeof(reply_storage));
    if (case_id < CASE_LIFECYCLE || case_id > CASE_BOUNDARY_REJECT
        || expected < 0 || expected > MAX_CONNECTIONS
        || (case_id == CASE_LIFECYCLE && expected != 22)
        || ((case_id == CASE_STRINGS || case_id == CASE_BOUNDARY_ACCEPT)
            && expected != 1)
        || (case_id == CASE_BOUNDARY_REJECT && expected != 0)
        || atexit(validate_at_exit) != 0) {
        _Exit(91);
    }
    alloc_before = align_rt_alloc_count();
    free_before = align_rt_free_count();
}

int32_t align_kv_control_verify(void) {
    if (!state_is_valid()) return 0;
    verified = 1;
    return 1;
}

int32_t align_kv_control_tcp_connect(
    const uint8_t *host,
    int64_t host_length,
    int64_t port,
    int64_t timeout_ns,
    void **output
) {
    if (selected_case == CASE_BOUNDARY_REJECT) _Exit(70);
    int32_t index = connect_calls;
    connect_calls += 1;
    if (index < 0 || index >= expected_connections || index >= MAX_CONNECTIONS
        || host == NULL || host_length != 9
        || memcmp(host, "127.0.0.1", 9) != 0 || output == NULL
        || *output != NULL) {
        _Exit(71);
    }
    int64_t expected_port = selected_case == CASE_LIFECYCLE ? 6380
        : selected_case == CASE_STRINGS ? 6381 : 65535;
    int64_t expected_timeout = selected_case == CASE_BOUNDARY_ACCEPT
        ? INT64_C(86400000000000) : INT64_C(1000000);
    if (port != expected_port || timeout_ns != expected_timeout) errors += 1;
    construction_stage[index] = 1;
    *output = &connection_tokens[index];
    return 0;
}

int32_t align_kv_control_set_io_timeout(void *connection, int64_t timeout_ns) {
    int32_t index = token_index(connection, connection_tokens);
    timeout_calls += 1;
    int64_t expected_timeout = selected_case == CASE_BOUNDARY_ACCEPT
        ? INT64_C(86400000000000) : INT64_C(2000000);
    if (index < 0 || index >= expected_connections
        || construction_stage[index] != 1 || timeout_ns != expected_timeout) {
        errors += 1;
        return 0;
    }
    construction_stage[index] = 2;
    return 0;
}

void *align_kv_control_conn_reader(void *connection) {
    int32_t index = token_index(connection, connection_tokens);
    reader_ctor_calls += 1;
    if (index < 0 || index >= expected_connections
        || construction_stage[index] != 2) {
        _Exit(72);
    }
    construction_stage[index] = 3;
    return &reader_tokens[index];
}

void *align_kv_control_conn_writer(void *connection) {
    int32_t index = token_index(connection, connection_tokens);
    writer_ctor_calls += 1;
    if (index < 0 || index >= expected_connections
        || construction_stage[index] != 3) {
        _Exit(73);
    }
    construction_stage[index] = 4;
    return &writer_tokens[index];
}

void align_kv_control_writer_free(void *writer) {
    int32_t index = token_index(writer, writer_tokens);
    writer_free_calls += 1;
    if (index < 0 || index >= expected_connections
        || construction_stage[index] != 4 || cleanup_stage[index] != 0) {
        _Exit(74);
    }
    cleanup_stage[index] = 1;
}

void align_kv_control_reader_free(void *reader) {
    int32_t index = token_index(reader, reader_tokens);
    reader_free_calls += 1;
    if (index < 0 || index >= expected_connections || cleanup_stage[index] != 1) {
        _Exit(75);
    }
    cleanup_stage[index] = 2;
}

void align_kv_control_conn_free(void *connection) {
    int32_t index = token_index(connection, connection_tokens);
    connection_free_calls += 1;
    if (index < 0 || index >= expected_connections || cleanup_stage[index] != 2) {
        _Exit(76);
    }
    cleanup_stage[index] = 3;
}

int32_t align_kv_control_writer_write(
    void *writer,
    const uint8_t *bytes,
    int64_t length
) {
    int32_t index = token_index(writer, writer_tokens);
    writer_calls += 1;
    if (selected_case != CASE_STRINGS || index != 0 || cleanup_stage[index] != 0
        || length < 0 || (length > 0 && bytes == NULL)) {
        _Exit(77);
    }
    return 0;
}

int64_t align_kv_control_reader_read(void *reader, void *buffer) {
    static const uint8_t REPLY_GET[] = "$9\r\nget-owned\r\n";
    static const uint8_t REPLY_LATER[] = "$4\r\nNEXT\r\n";
    static const uint8_t REPLY_SERVER[] = "-ERR-owned\r\n";
    static const uint8_t REPLY_AFTER[] = "$5\r\nAFTER\r\n";
    const uint8_t *reply = NULL;
    size_t length = 0;
    int32_t index = token_index(reader, reader_tokens);
    if (selected_case != CASE_STRINGS || index != 0 || buffer != &buffer_token
        || !buffer_live || reader_calls < 0 || reader_calls >= 4) {
        _Exit(78);
    }
    switch (reader_calls) {
        case 0:
            reply = REPLY_GET;
            length = sizeof(REPLY_GET) - 1;
            break;
        case 1:
            reply = REPLY_LATER;
            length = sizeof(REPLY_LATER) - 1;
            break;
        case 2:
            reply = REPLY_SERVER;
            length = sizeof(REPLY_SERVER) - 1;
            break;
        case 3:
            reply = REPLY_AFTER;
            length = sizeof(REPLY_AFTER) - 1;
            break;
        default:
            _Exit(79);
    }
    reader_calls += 1;
    if (length > sizeof(reply_storage)) _Exit(80);
    memset(reply_storage, 0xA5, sizeof(reply_storage));
    memcpy(reply_storage, reply, length);
    reply_length = (int64_t)length;
    return reply_length;
}

void *align_kv_control_buffer_new(int64_t capacity) {
    buffer_new_calls += 1;
    if (selected_case != CASE_STRINGS || buffer_live || capacity != 32768) _Exit(81);
    buffer_live = 1;
    reply_length = 0;
    return &buffer_token;
}

int64_t align_kv_control_buffer_capacity(void *buffer) {
    if (buffer != &buffer_token || !buffer_live) _Exit(82);
    return 32768;
}

void align_kv_control_buffer_bytes(void *buffer, void *output) {
    const uint8_t *pointer = reply_storage;
    header_calls += 1;
    if (buffer != &buffer_token || !buffer_live || output == NULL || reply_length <= 0) {
        _Exit(83);
    }
    memcpy(output, &pointer, sizeof(pointer));
    memcpy((uint8_t *)output + sizeof(pointer), &reply_length, sizeof(reply_length));
}

void align_kv_control_buffer_free(void *buffer) {
    buffer_free_calls += 1;
    if (buffer != &buffer_token || !buffer_live) _Exit(84);
    buffer_live = 0;
}
"#;

#[test]
fn pkg_kv_is_not_ambiently_available_without_its_source_tree() {
    let files = [(
        "main.align",
        "module main\nimport pkg.kv\nfn main() -> i32 = 0\n",
    )];
    let checked = diff_check_multi("pkg-kv-control-absent", &files, "main.align");
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "pkg.kv without vendored source must fail in both compile modes; whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
    for (mode, diagnostics) in [
        ("whole", checked.whole_diags.as_str()),
        ("per-unit", checked.per_unit_diags.as_str()),
    ] {
        assert_eq!(
            diagnostics.matches("cannot find module `pkg.kv`").count(),
            1,
            "{mode} compilation must emit one deterministic missing-package diagnostic:\n\
             {diagnostics}",
        );
        assert!(
            diagnostics.contains("pkg/kv.align"),
            "{mode} missing-package diagnostic must name the explicit expected source path:\n\
             {diagnostics}",
        );
    }
}

#[test]
fn client_control_strings_and_boundaries_are_owned_end_to_end() {
    let (root, internal) = control_sources();
    if !backend_available() {
        return;
    }
    if !cc_available_bounded() {
        return;
    }

    let files = [
        ("pkg/kv/internal/resource.align", internal.as_str()),
        ("pkg/kv.align", root.as_str()),
        ("main.align", CONTROL_MAIN),
    ];
    let checked = diff_check_multi("pkg-kv-control", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "control owner must compile in whole and per-unit modes; whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );

    let executable = build_control_exe(&files, CONTROL_C);
    for case in ["lifecycle", "strings", "boundary-accept", "boundary-reject"] {
        run_case(&executable.exe, case);
    }
}
