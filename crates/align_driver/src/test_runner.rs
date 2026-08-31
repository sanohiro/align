use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixDatagram;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

use super::signal_lease::DriverSignalLease;

#[cfg(not(target_has_atomic = "32"))]
compile_error!("the test signal controller requires lock-free 32-bit atomics");

const GRACEFUL_SIGNALS: [i32; 4] = [libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM];
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const ARBITRATION_IDLE: i32 = 0;
const ARBITRATION_WRITING: i32 = -1;
const ARBITRATION_PENDING_BASE: i32 = -1024;

static SIGNAL_STATE: AtomicI32 = AtomicI32::new(ARBITRATION_IDLE);
static SIGNAL_PIPE_WRITE: AtomicI32 = AtomicI32::new(-1);

fn pending_state(signal: i32) -> i32 {
    ARBITRATION_PENDING_BASE - signal
}

fn pending_signal(state: i32) -> Option<i32> {
    (state < ARBITRATION_PENDING_BASE).then_some(ARBITRATION_PENDING_BASE - state)
}

#[cfg(target_os = "linux")]
unsafe fn errno_location() -> *mut i32 {
    unsafe { libc::__errno_location() }
}

#[cfg(target_os = "macos")]
unsafe fn errno_location() -> *mut i32 {
    unsafe { libc::__error() }
}

extern "C" fn graceful_signal_handler(signal: i32) {
    let errno = unsafe { *errno_location() };
    loop {
        let state = SIGNAL_STATE.load(Ordering::Acquire);
        let next = if state == ARBITRATION_IDLE {
            signal
        } else if state == ARBITRATION_WRITING {
            pending_state(signal)
        } else {
            break;
        };
        if SIGNAL_STATE
            .compare_exchange(state, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            break;
        }
    }
    let descriptor = SIGNAL_PIPE_WRITE.load(Ordering::Acquire);
    if descriptor >= 0 {
        let byte = signal as u8;
        loop {
            let count = unsafe { libc::write(descriptor, &byte as *const u8 as *const _, 1) };
            if count == 1 {
                break;
            }
            let code = unsafe { *errno_location() };
            if count == -1 && code == libc::EINTR {
                continue;
            }
            break;
        }
    }
    unsafe { *errno_location() = errno };
}

struct SignalController {
    write: Option<File>,
    read: Option<File>,
    old_actions: [libc::sigaction; 4],
    lease: Option<DriverSignalLease>,
}

impl SignalController {
    fn acquire() -> Result<Self, io::Error> {
        let lease = DriverSignalLease::acquire()?;
        let result = Self::acquire_owned(lease);
        if result.is_err() {
            SIGNAL_STATE.store(ARBITRATION_IDLE, Ordering::Release);
        }
        result
    }

    fn acquire_owned(lease: DriverSignalLease) -> Result<Self, io::Error> {
        if !sigchld_is_compatible()? {
            return Err(io::Error::from_raw_os_error(0));
        }
        let mut original_mask = MaybeUninit::<libc::sigset_t>::zeroed();
        let mask_code = unsafe {
            libc::pthread_sigmask(
                libc::SIG_SETMASK,
                std::ptr::null(),
                original_mask.as_mut_ptr(),
            )
        };
        if mask_code != 0 {
            return Err(io::Error::from_raw_os_error(mask_code));
        }
        let original_mask = unsafe { original_mask.assume_init() };
        for signal in GRACEFUL_SIGNALS {
            if unsafe { libc::sigismember(&original_mask, signal) } == 1 {
                return Err(io::Error::from_raw_os_error(0));
            }
        }
        let mut old_actions = [unsafe { std::mem::zeroed() }; 4];
        for (index, signal) in GRACEFUL_SIGNALS.iter().copied().enumerate() {
            if unsafe { libc::sigaction(signal, std::ptr::null(), &mut old_actions[index]) } == -1 {
                return Err(io::Error::last_os_error());
            }
        }

        let mut blocked = MaybeUninit::<libc::sigset_t>::zeroed();
        let blocked = unsafe {
            libc::sigemptyset(blocked.as_mut_ptr());
            let mut blocked = blocked.assume_init();
            for signal in GRACEFUL_SIGNALS {
                libc::sigaddset(&mut blocked, signal);
            }
            blocked
        };
        let mask_code =
            unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, std::ptr::null_mut()) };
        if mask_code != 0 {
            return Err(io::Error::from_raw_os_error(mask_code));
        }

        let setup = (|| {
            let (read, write) = signal_pipe()?;
            for (installed, signal) in GRACEFUL_SIGNALS.into_iter().enumerate() {
                let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
                action.sa_sigaction = graceful_signal_handler as *const () as usize;
                action.sa_flags = 0;
                unsafe { libc::sigemptyset(&mut action.sa_mask) };
                if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == -1 {
                    let error = io::Error::last_os_error();
                    let mut restore_failed = false;
                    for restore in (0..installed).rev() {
                        if unsafe {
                            libc::sigaction(
                                GRACEFUL_SIGNALS[restore],
                                &old_actions[restore],
                                std::ptr::null_mut(),
                            )
                        } == -1
                        {
                            restore_failed = true;
                        }
                    }
                    if restore_failed {
                        // At least one live handler still targets these descriptors. Kernel teardown
                        // is the only safe cleanup; returning would let Rust close and reuse them.
                        unsafe { libc::_exit(1) }
                    }
                    return Err(error);
                }
            }
            SIGNAL_STATE.store(ARBITRATION_IDLE, Ordering::Release);
            SIGNAL_PIPE_WRITE.store(write.as_raw_fd(), Ordering::Release);
            Ok(Self {
                write: Some(write),
                read: Some(read),
                old_actions,
                lease: Some(lease),
            })
        })();
        let restore_code = unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &original_mask, std::ptr::null_mut())
        };
        if restore_code != 0 {
            if let Ok(controller) = setup {
                // A handler is installed and may still target the published pipe. Keep the
                // controller and process-global lease alive until the caller's terminal exit.
                std::mem::forget(controller);
            }
            return Err(io::Error::from_raw_os_error(restore_code));
        }
        setup
    }

    fn descriptor(&self) -> RawFd {
        self.read.as_ref().map_or(-1, AsRawFd::as_raw_fd)
    }

    fn selected(&self) -> Option<i32> {
        let state = SIGNAL_STATE.load(Ordering::Acquire);
        if state > 0 {
            Some(state)
        } else {
            pending_signal(state)
        }
    }

    fn drain(&mut self) -> io::Result<()> {
        let Some(read) = self.read.as_mut() else {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        };
        let mut bytes = [0u8; 64];
        loop {
            match read.read(&mut bytes) {
                Ok(0) => return Ok(()),
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }

    fn begin_write(&self) -> Result<(), i32> {
        loop {
            let state = SIGNAL_STATE.load(Ordering::Acquire);
            if state > 0 {
                return Err(state);
            }
            if let Some(signal) = pending_signal(state) {
                return Err(signal);
            }
            if state == ARBITRATION_IDLE
                && SIGNAL_STATE
                    .compare_exchange(
                        ARBITRATION_IDLE,
                        ARBITRATION_WRITING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
            {
                return Ok(());
            }
        }
    }

    fn finish_write(&self) -> Result<(), i32> {
        loop {
            let state = SIGNAL_STATE.load(Ordering::Acquire);
            if state == ARBITRATION_WRITING {
                if SIGNAL_STATE
                    .compare_exchange(
                        ARBITRATION_WRITING,
                        ARBITRATION_IDLE,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    return Ok(());
                }
                continue;
            }
            if let Some(signal) = pending_signal(state) {
                let _ = SIGNAL_STATE.compare_exchange(
                    state,
                    signal,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                return Err(signal);
            }
            return Err(state.max(1));
        }
    }
}

impl Drop for SignalController {
    fn drop(&mut self) {
        let mut blocked: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe { libc::sigemptyset(&mut blocked) };
        for signal in GRACEFUL_SIGNALS {
            unsafe { libc::sigaddset(&mut blocked, signal) };
        }
        let mut original: libc::sigset_t = unsafe { std::mem::zeroed() };
        let code = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut original) };
        if code != 0 {
            unsafe { libc::_exit(1) }
        }
        for index in (0..GRACEFUL_SIGNALS.len()).rev() {
            if unsafe {
                libc::sigaction(
                    GRACEFUL_SIGNALS[index],
                    &self.old_actions[index],
                    std::ptr::null_mut(),
                )
            } == -1
            {
                unsafe { libc::_exit(1) }
            }
        }
        SIGNAL_PIPE_WRITE.store(-1, Ordering::Release);
        SIGNAL_STATE.store(ARBITRATION_IDLE, Ordering::Release);
        if self
            .write
            .take()
            .is_some_and(|write| close_owned(write).is_err())
        {
            unsafe { libc::_exit(1) }
        }
        if self
            .read
            .take()
            .is_some_and(|read| close_owned(read).is_err())
        {
            unsafe { libc::_exit(1) }
        }
        drop(self.lease.take());
        let restore =
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &original, std::ptr::null_mut()) };
        if restore != 0 {
            unsafe { libc::_exit(1) }
        }
    }
}

fn signal_pipe() -> io::Result<(File, File)> {
    let mut descriptors = [-1; 2];
    if unsafe { libc::pipe(descriptors.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let configured = set_cloexec(descriptors[0])
        .and_then(|()| set_cloexec(descriptors[1]))
        .and_then(|()| set_nonblocking(descriptors[0]))
        .and_then(|()| set_nonblocking(descriptors[1]));
    if let Err(error) = configured {
        unsafe {
            libc::close(descriptors[1]);
            libc::close(descriptors[0]);
        }
        return Err(error);
    }
    Ok(unsafe {
        (
            File::from_raw_fd(descriptors[0]),
            File::from_raw_fd(descriptors[1]),
        )
    })
}

fn sigchld_is_compatible() -> io::Result<bool> {
    let mut action = MaybeUninit::<libc::sigaction>::zeroed();
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), action.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let action = unsafe { action.assume_init() };
    Ok(action.sa_sigaction == libc::SIG_DFL && action.sa_flags & libc::SA_NOCLDWAIT == 0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub timeout_ns: u64,
    pub max_output_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogEntry {
    pub canonical_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Completion {
    Ok,
    Error { tag: u8, code: i32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Failure {
    canonical_id: String,
    reason: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

enum TerminalWrite {
    Signal(i32),
    Io(io::Error),
}

fn controlled_write(
    controller: &SignalController,
    descriptor: RawFd,
    mut bytes: &[u8],
) -> Result<(), TerminalWrite> {
    let mut sigpipe: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut sigpipe);
        libc::sigaddset(&mut sigpipe, libc::SIGPIPE);
    }
    let mut original: libc::sigset_t = unsafe { std::mem::zeroed() };
    let code = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &sigpipe, &mut original) };
    if code != 0 {
        return Err(TerminalWrite::Io(io::Error::from_raw_os_error(code)));
    }
    while !bytes.is_empty() {
        if let Err(signal) = controller.begin_write() {
            return Err(TerminalWrite::Signal(signal));
        }
        let count = unsafe { libc::write(descriptor, bytes.as_ptr() as *const _, bytes.len()) };
        let write_error = (count < 0).then(io::Error::last_os_error);
        if let Err(signal) = controller.finish_write() {
            return Err(TerminalWrite::Signal(signal));
        }
        if count > 0 {
            bytes = &bytes[count as usize..];
            continue;
        }
        if count == 0 {
            return Err(TerminalWrite::Io(io::Error::from_raw_os_error(libc::EIO)));
        }
        let error = write_error.unwrap_or_else(io::Error::last_os_error);
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(TerminalWrite::Io(error));
    }
    let restore =
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &original, std::ptr::null_mut()) };
    if restore != 0 {
        return Err(TerminalWrite::Io(io::Error::from_raw_os_error(restore)));
    }
    Ok(())
}

fn write_failure(controller: &SignalController, failure: &Failure) -> Result<(), TerminalWrite> {
    controlled_write(
        controller,
        libc::STDOUT_FILENO,
        format!(
            "FAIL {}\nreason: {}\n",
            failure.canonical_id, failure.reason
        )
        .as_bytes(),
    )?;
    for (heading, bytes) in [
        (b"--- stdout ---\n".as_slice(), failure.stdout.as_slice()),
        (b"--- stderr ---\n".as_slice(), failure.stderr.as_slice()),
    ] {
        if bytes.is_empty() {
            continue;
        }
        controlled_write(controller, libc::STDOUT_FILENO, heading)?;
        controlled_write(controller, libc::STDOUT_FILENO, bytes)?;
        if bytes.last() != Some(&b'\n') {
            controlled_write(controller, libc::STDOUT_FILENO, b"\n")?;
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum RunnerError {
    Infrastructure { operation: &'static str, code: i32 },
    LaunchTimeout { timeout_ns: u64 },
    LaunchOutput { stream: &'static str, limit: usize },
    Signal { signal: i32 },
}

struct RowError {
    terminal: RunnerError,
    evidence: Option<Failure>,
}

impl From<RunnerError> for RowError {
    fn from(terminal: RunnerError) -> Self {
        Self {
            terminal,
            evidence: None,
        }
    }
}

impl RunnerError {
    fn message(&self) -> Option<String> {
        match self {
            Self::Infrastructure { operation, code } => Some(format!(
                "alignc: test runner {operation} failed (os error {code})\n"
            )),
            Self::LaunchTimeout { timeout_ns } => Some(format!(
                "alignc: test runner launch timed out after {timeout_ns} ns\n"
            )),
            Self::LaunchOutput { stream, limit } => Some(format!(
                "alignc: test runner launch {stream} exceeded {limit} bytes\n"
            )),
            Self::Signal { .. } => None,
        }
    }

    pub fn write(&self) {
        if let Some(message) = self.message() {
            eprint!("{message}");
        }
    }
}

fn raw_code(error: &io::Error) -> i32 {
    error.raw_os_error().unwrap_or(0)
}

fn infrastructure(operation: &'static str, error: &io::Error) -> RunnerError {
    RunnerError::Infrastructure {
        operation,
        code: raw_code(error),
    }
}

fn set_cloexec(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn launch_record(ordinal: u32) -> [u8; 16] {
    let mut record = [0; 16];
    record[..8].copy_from_slice(b"ALTESTL\x01");
    record[8..12].copy_from_slice(&ordinal.to_le_bytes());
    record
}

fn decode_ack(record: &[u8], ordinal: u32) -> bool {
    record.len() == 16
        && &record[..8] == b"ALTESTA\x01"
        && record[8..12] == ordinal.to_le_bytes()
        && record[12..16] == [0; 4]
}

fn completion_magic(record: &[u8]) -> bool {
    record.len() >= 8 && &record[..8] == b"ALTEST\0\x01"
}

fn decode_completion(record: &[u8], ordinal: u32) -> Result<Completion, &'static str> {
    if record.len() != 20 {
        return Err("length");
    }
    if !completion_magic(record) {
        return Err("magic/version");
    }
    let outcome = record[8];
    if outcome > 1 {
        return Err("outcome");
    }
    let tag = record[9];
    if (outcome == 0 && tag != 255) || (outcome == 1 && tag > 4) {
        return Err("error tag");
    }
    if record[10] != 0 || record[11] != 0 {
        return Err("reserved bytes");
    }
    let code = i32::from_le_bytes([record[12], record[13], record[14], record[15]]);
    if (outcome == 0 || tag != 4) && code != 0 {
        return Err("error code");
    }
    let actual = u32::from_le_bytes([record[16], record[17], record[18], record[19]]);
    if actual != ordinal {
        return Err("ordinal");
    }
    if outcome == 0 {
        Ok(Completion::Ok)
    } else {
        Ok(Completion::Error { tag, code })
    }
}

struct Capture {
    bytes: Vec<u8>,
    filled: usize,
    probe: [u8; 1],
}

impl Capture {
    fn into_bytes(mut self) -> Vec<u8> {
        self.bytes.truncate(self.filled);
        self.bytes
    }
}

fn reserve_capture(limit: usize) -> Result<Capture, RunnerError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(limit)
        .map_err(|_| RunnerError::Infrastructure {
            operation: "capture allocation",
            code: 0,
        })?;
    bytes.resize(limit, 0);
    Ok(Capture {
        bytes,
        filled: 0,
        probe: [0],
    })
}

fn drain_stream(reader: &mut impl Read, capture: &mut Capture) -> io::Result<(bool, bool)> {
    let mut eof = false;
    let mut exceeded = false;
    loop {
        let target = if capture.filled < capture.bytes.len() {
            &mut capture.bytes[capture.filled..]
        } else {
            &mut capture.probe[..]
        };
        match reader.read(target) {
            Ok(0) => {
                eof = true;
                break;
            }
            Ok(count) => {
                if capture.filled == capture.bytes.len() {
                    exceeded = true;
                    break;
                }
                capture.filled += count;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok((eof, exceeded))
}

enum TargetSignal {
    Sent,
    Missing,
    Failed(io::Error),
}

struct TargetSignals {
    group: TargetSignal,
    direct: TargetSignal,
}

enum SignalDispatch {
    Verified(TargetSignals),
    Direct(TargetSignal),
}

fn send_signal(target: i32, signal: i32) -> TargetSignal {
    loop {
        if unsafe { libc::kill(target, signal) } == 0 {
            return TargetSignal::Sent;
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if error.raw_os_error() == Some(libc::ESRCH) {
            return TargetSignal::Missing;
        }
        return TargetSignal::Failed(error);
    }
}

fn signal_verified_targets(pid: i32, signal: i32) -> TargetSignals {
    TargetSignals {
        group: send_signal(-pid, signal),
        direct: send_signal(pid, signal),
    }
}

fn signal_child(pid: i32, verified_group: bool, signal: i32) -> SignalDispatch {
    if verified_group {
        SignalDispatch::Verified(signal_verified_targets(pid, signal))
    } else {
        SignalDispatch::Direct(send_signal(pid, signal))
    }
}

fn target_signal_error(attempt: TargetSignals, terminal: bool) -> Option<io::Error> {
    let direct_sent = matches!(attempt.direct, TargetSignal::Sent);
    let group_error = match attempt.group {
        TargetSignal::Sent => None,
        TargetSignal::Missing if terminal || direct_sent => None,
        TargetSignal::Missing => Some(io::Error::from_raw_os_error(libc::ESRCH)),
        TargetSignal::Failed(error) => Some(error),
    };
    let direct_error = match attempt.direct {
        TargetSignal::Sent => None,
        TargetSignal::Missing if terminal => None,
        TargetSignal::Missing => Some(io::Error::from_raw_os_error(libc::ESRCH)),
        TargetSignal::Failed(error) => Some(error),
    };
    group_error.or(direct_error)
}

fn signal_dispatch_error(attempt: SignalDispatch, terminal: bool) -> Option<io::Error> {
    match attempt {
        SignalDispatch::Verified(attempt) => target_signal_error(attempt, terminal),
        SignalDispatch::Direct(TargetSignal::Sent) => None,
        SignalDispatch::Direct(TargetSignal::Missing) if terminal => None,
        SignalDispatch::Direct(TargetSignal::Missing) => {
            Some(io::Error::from_raw_os_error(libc::ESRCH))
        }
        SignalDispatch::Direct(TargetSignal::Failed(error)) => Some(error),
    }
}

fn kill_direct(pid: i32, deadline: Instant) -> io::Result<()> {
    match send_signal(pid, libc::SIGKILL) {
        TargetSignal::Sent => Ok(()),
        TargetSignal::Missing if child_is_terminal_until(pid, deadline)? => Ok(()),
        TargetSignal::Missing => Err(io::Error::from_raw_os_error(libc::ESRCH)),
        TargetSignal::Failed(error) => Err(error),
    }
}

fn child_is_terminal_until(pid: i32, deadline: Instant) -> io::Result<bool> {
    let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
    loop {
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            let info = unsafe { info.assume_init() };
            return Ok(unsafe { info.si_pid() } != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
        if Instant::now() >= deadline {
            return Err(cleanup_timed_out());
        }
    }
}

fn cleanup_deadline() -> Instant {
    let now = Instant::now();
    now.checked_add(CLEANUP_TIMEOUT).unwrap_or(now)
}

fn row_deadlines(started: Instant, timeout_ns: u64) -> (Instant, Instant) {
    let timeout = Duration::from_nanos(timeout_ns);
    let row_deadline = started.checked_add(timeout).unwrap_or(started);
    // The public timeout bounds launch, execution, and cleanup together. Stop ordinary work before
    // the one terminal deadline so signalling and reap do not need a second budget. Very short
    // rows retain half their budget; normal rows retain at most the established cleanup bound.
    let cleanup_reserve = CLEANUP_TIMEOUT.min(timeout / 2);
    let work_deadline = row_deadline.checked_sub(cleanup_reserve).unwrap_or(started);
    (work_deadline, row_deadline)
}

fn cleanup_timed_out() -> io::Error {
    io::Error::from_raw_os_error(libc::ETIMEDOUT)
}

fn reap_child(pid: i32, deadline: Instant) -> io::Result<ExitStatus> {
    let mut status = 0;
    loop {
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if result == pid {
            return Ok(ExitStatus::from_raw(status));
        }
        if result == 0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(cleanup_timed_out());
            }
            std::thread::sleep(remaining.min(Duration::from_millis(1)));
            continue;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
        if Instant::now() >= deadline {
            return Err(cleanup_timed_out());
        }
    }
}

fn wait_process_group_empty(pid: i32, deadline: Instant) -> io::Result<()> {
    loop {
        match send_signal(-pid, 0) {
            TargetSignal::Missing => return Ok(()),
            TargetSignal::Sent => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(cleanup_timed_out());
                }
                // The leader is reaped immediately before this check. Keep the observation window
                // finite so a persistent zombie or a subsequently reused PGID fails the row closed
                // instead of hanging the runner or being followed into the next catalog row.
                std::thread::sleep(remaining.min(Duration::from_millis(1)));
            }
            TargetSignal::Failed(error) => return Err(error),
        }
    }
}

struct ChildGuard {
    pid: i32,
    verified_group: bool,
    armed: bool,
}

impl ChildGuard {
    fn new(pid: i32) -> Self {
        Self {
            pid,
            verified_group: false,
            armed: true,
        }
    }

    fn verify_group(&mut self) {
        self.verified_group = true;
    }

    fn kill(&self, deadline: Instant) -> io::Result<()> {
        if self.verified_group {
            let attempt = signal_verified_targets(self.pid, libc::SIGKILL);
            let terminal = child_is_terminal_until(self.pid, deadline)?;
            target_signal_error(attempt, terminal).map_or(Ok(()), Err)
        } else {
            kill_direct(self.pid, deadline)
        }
    }

    fn reap(&mut self, deadline: Instant) -> io::Result<ExitStatus> {
        let status = reap_child(self.pid, deadline)?;
        self.armed = false;
        Ok(status)
    }

    fn abandon(&mut self) {
        self.armed = false;
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.armed {
            let deadline = cleanup_deadline();
            let _ = self.kill(deadline);
            let _ = reap_child(self.pid, deadline);
            self.armed = false;
        }
    }
}

fn poll_timeout(deadline: Instant) -> i32 {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return 0;
    }
    let millis = remaining.as_millis().saturating_add(1);
    i32::try_from(millis.min(i32::MAX as u128)).unwrap_or(i32::MAX)
}

fn poll_row(
    signal: RawFd,
    control: RawFd,
    stdout: RawFd,
    stderr: RawFd,
    deadline: Instant,
) -> io::Result<()> {
    let mut fds = [
        libc::pollfd {
            fd: signal,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        },
        libc::pollfd {
            fd: control,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        },
        libc::pollfd {
            fd: stdout,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        },
        libc::pollfd {
            fd: stderr,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        },
    ];
    loop {
        let status =
            unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, poll_timeout(deadline)) };
        if status >= 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
        if Instant::now() >= deadline {
            return Ok(());
        }
    }
}

fn close_owned<T: IntoRawFd>(owner: T) -> io::Result<()> {
    let descriptor = owner.into_raw_fd();
    if unsafe { libc::close(descriptor) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

struct SpawnedChild {
    pid: i32,
    started: Instant,
    stdout: File,
    stderr: File,
    parent_closes: [File; 3],
}

struct FileActions(libc::posix_spawn_file_actions_t);

impl FileActions {
    fn new() -> io::Result<Self> {
        let mut actions = MaybeUninit::uninit();
        let code = unsafe { libc::posix_spawn_file_actions_init(actions.as_mut_ptr()) };
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code));
        }
        Ok(Self(unsafe { actions.assume_init() }))
    }

    fn dup2(&mut self, source: RawFd, destination: RawFd) -> io::Result<()> {
        posix_result(unsafe {
            libc::posix_spawn_file_actions_adddup2(&mut self.0, source, destination)
        })
    }

    fn chdir(&mut self, path: &CString) -> io::Result<()> {
        posix_result(unsafe { spawn_actions_addchdir(&mut self.0, path.as_ptr()) })
    }

    #[cfg(target_os = "linux")]
    fn close_from(&mut self, descriptor: RawFd) -> io::Result<()> {
        posix_result(unsafe { spawn_actions_addclosefrom(&mut self.0, descriptor) })
    }
}

#[cfg(target_os = "linux")]
unsafe fn spawn_actions_addchdir(
    actions: *mut libc::posix_spawn_file_actions_t,
    path: *const libc::c_char,
) -> i32 {
    unsafe { libc::posix_spawn_file_actions_addchdir_np(actions, path) }
}

#[cfg(target_os = "linux")]
unsafe fn spawn_actions_addclosefrom(
    actions: *mut libc::posix_spawn_file_actions_t,
    descriptor: RawFd,
) -> i32 {
    unsafe { libc::posix_spawn_file_actions_addclosefrom_np(actions, descriptor) }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    #[link_name = "posix_spawn_file_actions_addchdir_np"]
    fn macos_spawn_actions_addchdir(
        actions: *mut libc::posix_spawn_file_actions_t,
        path: *const libc::c_char,
    ) -> i32;
}

#[cfg(target_os = "macos")]
unsafe fn spawn_actions_addchdir(
    actions: *mut libc::posix_spawn_file_actions_t,
    path: *const libc::c_char,
) -> i32 {
    unsafe { macos_spawn_actions_addchdir(actions, path) }
}

impl Drop for FileActions {
    fn drop(&mut self) {
        let _ = unsafe { libc::posix_spawn_file_actions_destroy(&mut self.0) };
    }
}

struct SpawnAttributes(libc::posix_spawnattr_t);

impl SpawnAttributes {
    fn process_group() -> io::Result<Self> {
        let mut attributes = MaybeUninit::uninit();
        posix_result(unsafe { libc::posix_spawnattr_init(attributes.as_mut_ptr()) })?;
        let mut attributes = Self(unsafe { attributes.assume_init() });
        posix_result(unsafe { libc::posix_spawnattr_setpgroup(&mut attributes.0, 0) })?;
        posix_result(unsafe {
            libc::posix_spawnattr_setflags(
                &mut attributes.0,
                libc::POSIX_SPAWN_SETPGROUP as libc::c_short,
            )
        })?;
        Ok(attributes)
    }

    #[cfg(target_os = "macos")]
    fn close_ambient(mut self) -> io::Result<Self> {
        let flags =
            (libc::POSIX_SPAWN_SETPGROUP | libc::POSIX_SPAWN_CLOEXEC_DEFAULT) as libc::c_short;
        posix_result(unsafe { libc::posix_spawnattr_setflags(&mut self.0, flags) })?;
        Ok(self)
    }
}

impl Drop for SpawnAttributes {
    fn drop(&mut self) {
        let _ = unsafe { libc::posix_spawnattr_destroy(&mut self.0) };
    }
}

fn posix_result(code: i32) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

fn capture_pipe() -> io::Result<(File, File)> {
    let mut descriptors = [-1; 2];
    if unsafe { libc::pipe(descriptors.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let configured = set_cloexec(descriptors[0])
        .and_then(|()| set_cloexec(descriptors[1]))
        .and_then(|()| set_nonblocking(descriptors[0]));
    if let Err(error) = configured {
        unsafe {
            libc::close(descriptors[1]);
            libc::close(descriptors[0]);
        }
        return Err(error);
    }
    Ok(unsafe {
        (
            File::from_raw_fd(descriptors[0]),
            File::from_raw_fd(descriptors[1]),
        )
    })
}

fn duplicate_spawn_source(descriptor: RawFd) -> io::Result<File> {
    let duplicate = unsafe { libc::fcntl(descriptor, libc::F_DUPFD_CLOEXEC, 4) };
    if duplicate == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(duplicate) })
    }
}

unsafe extern "C" {
    static mut environ: *mut *mut libc::c_char;
}

fn spawn_child(
    executable: &Path,
    suite_cwd: &Path,
    child_control: RawFd,
    stdout: File,
    stdout_write: File,
    stderr: File,
    stderr_write: File,
) -> Result<SpawnedChild, RunnerError> {
    let stdin_path = c"/dev/null";
    let stdin_fd = unsafe { libc::open(stdin_path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if stdin_fd == -1 {
        return Err(infrastructure("stdin open", &io::Error::last_os_error()));
    }
    let stdin = unsafe { File::from_raw_fd(stdin_fd) };
    // Keep every action source above the four exact child targets. This owns both the
    // source-equals-target case (where dup2 would preserve FD_CLOEXEC) and cross-target aliasing
    // when an embedding process entered with any of fd 0..3 closed.
    let stdin_source = duplicate_spawn_source(stdin.as_raw_fd())
        .map_err(|error| infrastructure("descriptor mapping", &error))?;
    let stdout_source = duplicate_spawn_source(stdout_write.as_raw_fd())
        .map_err(|error| infrastructure("descriptor mapping", &error))?;
    let stderr_source = duplicate_spawn_source(stderr_write.as_raw_fd())
        .map_err(|error| infrastructure("descriptor mapping", &error))?;
    let control_source = duplicate_spawn_source(child_control)
        .map_err(|error| infrastructure("descriptor mapping", &error))?;
    let executable_c = CString::new(executable.as_os_str().as_bytes()).map_err(|_| {
        RunnerError::Infrastructure {
            operation: "spawn",
            code: 0,
        }
    })?;
    let cwd_c = CString::new(suite_cwd.as_os_str().as_bytes()).map_err(|_| {
        RunnerError::Infrastructure {
            operation: "working directory",
            code: 0,
        }
    })?;
    let mut actions =
        FileActions::new().map_err(|error| infrastructure("descriptor mapping", &error))?;
    actions
        .chdir(&cwd_c)
        .map_err(|error| infrastructure("working directory", &error))?;
    actions
        .dup2(stdin_source.as_raw_fd(), 0)
        .and_then(|()| actions.dup2(stdout_source.as_raw_fd(), 1))
        .and_then(|()| actions.dup2(stderr_source.as_raw_fd(), 2))
        .and_then(|()| actions.dup2(control_source.as_raw_fd(), 3))
        .map_err(|error| infrastructure("descriptor mapping", &error))?;
    #[cfg(target_os = "linux")]
    actions
        .close_from(4)
        .map_err(|error| infrastructure("descriptor mapping", &error))?;
    let attributes = SpawnAttributes::process_group()
        .map_err(|error| infrastructure("process group", &error))?;
    #[cfg(target_os = "macos")]
    let attributes = attributes
        .close_ambient()
        .map_err(|error| infrastructure("descriptor mapping", &error))?;
    let mut pid = 0;
    let mut argv = [
        executable_c.as_ptr() as *mut libc::c_char,
        std::ptr::null_mut(),
    ];
    let started = Instant::now();
    let code = unsafe {
        libc::posix_spawn(
            &mut pid,
            executable_c.as_ptr(),
            &actions.0,
            &attributes.0,
            argv.as_mut_ptr(),
            environ,
        )
    };
    if code != 0 {
        return Err(infrastructure("spawn", &io::Error::from_raw_os_error(code)));
    }
    Ok(SpawnedChild {
        pid,
        started,
        stdout,
        stderr,
        parent_closes: [stdout_write, stderr_write, stdin],
    })
}

fn drain_control(
    control: &UnixDatagram,
    ordinal: u32,
    acknowledged: &mut bool,
    completion: &mut Option<Completion>,
    record_detail: &mut Option<&'static str>,
) -> io::Result<()> {
    loop {
        let mut record = [0u8; 21];
        match control.recv(&mut record) {
            Ok(count) => {
                let record = &record[..count];
                if !*acknowledged {
                    if record_detail.is_none() && decode_ack(record, ordinal) {
                        *acknowledged = true;
                    } else {
                        record_detail.get_or_insert("order");
                    }
                    continue;
                }
                if completion.is_some() {
                    record_detail.get_or_insert(
                        if record.len() == 20 && completion_magic(record) {
                            "repetition"
                        } else {
                            "order"
                        },
                    );
                    continue;
                }
                match decode_completion(record, ordinal) {
                    Ok(value) => *completion = Some(value),
                    Err(detail) => {
                        record_detail.get_or_insert(detail);
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn returned_error(tag: u8, code: i32) -> String {
    match tag {
        0 => "returned Error.NotFound".to_owned(),
        1 => "returned Error.Invalid".to_owned(),
        2 => "returned Error.Denied".to_owned(),
        3 => "returned Error.Timeout".to_owned(),
        4 => format!("returned Error.Code({code})"),
        _ => format!("returned unknown Error tag {tag} with code {code}"),
    }
}

fn status_reason(status: ExitStatus, detail: &str) -> String {
    if let Some(code) = status.code() {
        format!("exited with status {code}; completion record: {detail}")
    } else {
        format!(
            "terminated by signal {}; completion record: {detail}",
            status.signal().unwrap_or(0)
        )
    }
}

fn mismatch_reason(status: ExitStatus, completion: &Completion) -> String {
    let outcome = if matches!(completion, Completion::Ok) {
        "Ok"
    } else {
        "Err"
    };
    if let Some(code) = status.code() {
        format!("completion record {outcome} mismatched exit status {code}")
    } else {
        format!(
            "completion record {outcome} mismatched exit status signal {}",
            status.signal().unwrap_or(0)
        )
    }
}

struct Quiesced {
    status: Option<ExitStatus>,
    cleanup_error: Option<RunnerError>,
    interrupted: Option<i32>,
}

fn remember_cleanup_error(
    first: &mut Option<RunnerError>,
    operation: &'static str,
    error: io::Error,
) {
    if first.is_none() {
        *first = Some(infrastructure(operation, &error));
    }
}

#[allow(clippy::too_many_arguments)]
fn quiesce_child(
    controller: &mut SignalController,
    child_guard: &mut ChildGuard,
    parent_control: UnixDatagram,
    mut stdout: File,
    mut stderr: File,
    ordinal: u32,
    acknowledged: &mut bool,
    completion: &mut Option<Completion>,
    record_detail: &mut Option<&'static str>,
    stdout_bytes: &mut Capture,
    stderr_bytes: &mut Capture,
    stdout_eof: &mut bool,
    stderr_eof: &mut bool,
    stdout_exceeded: &mut bool,
    stderr_exceeded: &mut bool,
    cleanup_deadline: Instant,
    mut interrupted: Option<i32>,
) -> Quiesced {
    let mut cleanup_error = None;
    let verified_group = child_guard.verified_group;
    if interrupted.is_none() {
        interrupted = controller.selected();
    }

    let graceful_attempt = interrupted.map(|signal| {
        let attempt = signal_child(child_guard.pid, verified_group, signal);
        let grace_started = Instant::now();
        let grace_deadline = grace_started
            .checked_add(Duration::from_millis(250))
            .unwrap_or(grace_started)
            .min(cleanup_deadline);
        std::thread::sleep(grace_deadline.saturating_duration_since(Instant::now()));
        attempt
    });
    let kill_attempt = signal_child(child_guard.pid, verified_group, libc::SIGKILL);

    let terminal = loop {
        match child_is_terminal_until(child_guard.pid, cleanup_deadline) {
            Ok(true) => break true,
            Ok(false) => {
                if Instant::now() >= cleanup_deadline {
                    remember_cleanup_error(&mut cleanup_error, "wait", cleanup_timed_out());
                    break false;
                }
                if interrupted.is_none() {
                    interrupted = controller.selected();
                }
                let wake = Instant::now()
                    .checked_add(Duration::from_millis(10))
                    .unwrap_or_else(Instant::now)
                    .min(cleanup_deadline);
                if let Err(error) = poll_row(
                    controller.descriptor(),
                    parent_control.as_raw_fd(),
                    if *stdout_eof || *stdout_exceeded {
                        -1
                    } else {
                        stdout.as_raw_fd()
                    },
                    if *stderr_eof || *stderr_exceeded {
                        -1
                    } else {
                        stderr.as_raw_fd()
                    },
                    wake,
                ) {
                    remember_cleanup_error(&mut cleanup_error, "poll", error);
                    break false;
                }
            }
            Err(error) => {
                remember_cleanup_error(&mut cleanup_error, "wait", error);
                break false;
            }
        }
    };

    if let Some(attempt) = graceful_attempt
        && let Some(error) = signal_dispatch_error(attempt, terminal)
    {
        remember_cleanup_error(&mut cleanup_error, "kill", error);
    }
    if let Some(error) = signal_dispatch_error(kill_attempt, terminal) {
        remember_cleanup_error(&mut cleanup_error, "kill", error);
    }

    if let Err(error) = controller.drain() {
        remember_cleanup_error(&mut cleanup_error, "poll", error);
    }
    if let Err(error) = drain_control(
        &parent_control,
        ordinal,
        acknowledged,
        completion,
        record_detail,
    ) {
        remember_cleanup_error(&mut cleanup_error, "control read", error);
    }
    if !*stdout_eof && !*stdout_exceeded {
        match drain_stream(&mut stdout, stdout_bytes) {
            Ok((eof, exceeded)) => {
                *stdout_eof |= eof;
                *stdout_exceeded |= exceeded;
            }
            Err(error) => remember_cleanup_error(&mut cleanup_error, "stdout read", error),
        }
    }
    if !*stderr_eof && !*stderr_exceeded {
        match drain_stream(&mut stderr, stderr_bytes) {
            Ok((eof, exceeded)) => {
                *stderr_eof |= eof;
                *stderr_exceeded |= exceeded;
            }
            Err(error) => remember_cleanup_error(&mut cleanup_error, "stderr read", error),
        }
    }
    if let Err(error) = drain_control(
        &parent_control,
        ordinal,
        acknowledged,
        completion,
        record_detail,
    ) {
        remember_cleanup_error(&mut cleanup_error, "control read", error);
    }

    for result in [
        close_owned(stdout),
        close_owned(stderr),
        close_owned(parent_control),
    ] {
        if let Err(error) = result {
            remember_cleanup_error(&mut cleanup_error, "close", error);
        }
    }
    let status = if terminal {
        match child_guard.reap(cleanup_deadline) {
            Ok(status) => {
                if verified_group
                    && let Err(error) = wait_process_group_empty(child_guard.pid, cleanup_deadline)
                {
                    remember_cleanup_error(&mut cleanup_error, "process group", error);
                }
                Some(status)
            }
            Err(error) => {
                child_guard.abandon();
                remember_cleanup_error(&mut cleanup_error, "reap", error);
                None
            }
        }
    } else {
        child_guard.abandon();
        None
    };
    if interrupted.is_none() {
        interrupted = controller.selected();
    }
    Quiesced {
        status,
        cleanup_error,
        interrupted,
    }
}

// A row error deliberately retains both preallocated capture stores through terminal reporting.
// Boxing it would add another allocation after child cleanup, outside that fixed evidence budget.
#[allow(clippy::result_large_err)]
fn run_row(
    controller: &mut SignalController,
    executable: &Path,
    suite_cwd: &Path,
    entry: &CatalogEntry,
    ordinal: u32,
    limits: Limits,
) -> Result<Option<Failure>, RowError> {
    let mut stdout_bytes = reserve_capture(limits.max_output_bytes)?;
    let mut stderr_bytes = reserve_capture(limits.max_output_bytes)?;
    let (stdout, stdout_write) = capture_pipe().map_err(|error| infrastructure("pipe", &error))?;
    let (stderr, stderr_write) = capture_pipe().map_err(|error| infrastructure("pipe", &error))?;
    let (parent_control, child_control) =
        UnixDatagram::pair().map_err(|error| infrastructure("control socket", &error))?;
    set_cloexec(parent_control.as_raw_fd())
        .and_then(|()| set_cloexec(child_control.as_raw_fd()))
        .and_then(|()| set_nonblocking(parent_control.as_raw_fd()))
        .map_err(|error| infrastructure("descriptor flags", &error))?;

    let child = spawn_child(
        executable,
        suite_cwd,
        child_control.as_raw_fd(),
        stdout,
        stdout_write,
        stderr,
        stderr_write,
    )?;
    let (deadline, row_deadline) = row_deadlines(child.started, limits.timeout_ns);
    let pid = child.pid;
    let mut child_guard = ChildGuard::new(pid);
    let mut stdout = child.stdout;
    let mut stderr = child.stderr;
    let mut acknowledged = false;
    let mut completion = None;
    let mut record_detail = None;
    let mut stdout_eof = false;
    let mut stderr_eof = false;
    let mut stdout_exceeded = false;
    let mut stderr_exceeded = false;
    let mut timed_out = false;
    let mut interrupted = None;
    let mut event_error = None;
    for result in child
        .parent_closes
        .into_iter()
        .map(close_owned)
        .chain(std::iter::once(close_owned(child_control)))
    {
        if let Err(error) = result
            && event_error.is_none()
        {
            event_error = Some(infrastructure("close", &error));
        }
    }
    let actual_group = loop {
        let group = unsafe { libc::getpgid(pid) };
        if group != -1 {
            break Ok(group);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            break Err(error);
        }
    };
    match actual_group {
        Ok(group) if group == pid => child_guard.verify_group(),
        Ok(_) => {
            event_error.get_or_insert(RunnerError::Infrastructure {
                operation: "process group",
                code: 0,
            });
        }
        Err(error) => {
            event_error.get_or_insert(infrastructure("process group", &error));
        }
    }
    if event_error.is_none() {
        interrupted = controller.selected();
    }
    if event_error.is_none() && interrupted.is_none() {
        let launch = launch_record(ordinal);
        match parent_control.send(&launch) {
            Ok(16) => {}
            Ok(_) => {
                event_error = Some(RunnerError::Infrastructure {
                    operation: "control write",
                    code: libc::EIO,
                });
            }
            Err(error) => {
                event_error = Some(infrastructure("control write", &error));
            }
        }
    }

    while event_error.is_none() && interrupted.is_none() {
        if let Err(error) = controller.drain() {
            event_error = Some(infrastructure("poll", &error));
            break;
        }
        if let Some(signal) = controller.selected() {
            interrupted = Some(signal);
            break;
        }
        if acknowledged {
            let (eof, exceeded) = match drain_stream(&mut stdout, &mut stdout_bytes) {
                Ok(result) => result,
                Err(error) => {
                    event_error = Some(infrastructure("stdout read", &error));
                    break;
                }
            };
            stdout_eof |= eof;
            stdout_exceeded |= exceeded;
            let (eof, exceeded) = match drain_stream(&mut stderr, &mut stderr_bytes) {
                Ok(result) => result,
                Err(error) => {
                    event_error = Some(infrastructure("stderr read", &error));
                    break;
                }
            };
            stderr_eof |= eof;
            stderr_exceeded |= exceeded;
            if let Err(error) = drain_control(
                &parent_control,
                ordinal,
                &mut acknowledged,
                &mut completion,
                &mut record_detail,
            ) {
                event_error = Some(infrastructure("control read", &error));
                break;
            }
        } else {
            if let Err(error) = drain_control(
                &parent_control,
                ordinal,
                &mut acknowledged,
                &mut completion,
                &mut record_detail,
            ) {
                event_error = Some(infrastructure("control read", &error));
                break;
            }
            let (eof, exceeded) = match drain_stream(&mut stdout, &mut stdout_bytes) {
                Ok(result) => result,
                Err(error) => {
                    event_error = Some(infrastructure("stdout read", &error));
                    break;
                }
            };
            stdout_eof |= eof;
            stdout_exceeded |= exceeded;
            let (eof, exceeded) = match drain_stream(&mut stderr, &mut stderr_bytes) {
                Ok(result) => result,
                Err(error) => {
                    event_error = Some(infrastructure("stderr read", &error));
                    break;
                }
            };
            stderr_eof |= eof;
            stderr_exceeded |= exceeded;
        }

        if !acknowledged && record_detail.is_some() {
            break;
        }
        if stdout_exceeded || stderr_exceeded {
            break;
        }
        match child_is_terminal_until(pid, deadline) {
            Ok(true) => break,
            Ok(false) => {}
            Err(error) if error.raw_os_error() == Some(libc::ETIMEDOUT) => {
                timed_out = true;
                break;
            }
            Err(error) => {
                event_error = Some(infrastructure("wait", &error));
                break;
            }
        }
        if Instant::now() >= deadline {
            timed_out = true;
            break;
        }
        if let Err(error) = poll_row(
            controller.descriptor(),
            parent_control.as_raw_fd(),
            if stdout_eof || stdout_exceeded {
                -1
            } else {
                stdout.as_raw_fd()
            },
            if stderr_eof || stderr_exceeded {
                -1
            } else {
                stderr.as_raw_fd()
            },
            deadline,
        ) {
            event_error = Some(infrastructure("poll", &error));
            break;
        }
    }

    let quiesced = quiesce_child(
        controller,
        &mut child_guard,
        parent_control,
        stdout,
        stderr,
        ordinal,
        &mut acknowledged,
        &mut completion,
        &mut record_detail,
        &mut stdout_bytes,
        &mut stderr_bytes,
        &mut stdout_eof,
        &mut stderr_eof,
        &mut stdout_exceeded,
        &mut stderr_exceeded,
        row_deadline,
        interrupted,
    );

    if let Some(signal) = quiesced.interrupted {
        return Err(RunnerError::Signal { signal }.into());
    }
    if let Some(error) = event_error {
        return Err(error.into());
    }
    let Some(status) = quiesced.status else {
        return Err(quiesced
            .cleanup_error
            .unwrap_or(RunnerError::Infrastructure {
                operation: "reap",
                code: 0,
            })
            .into());
    };

    if !acknowledged {
        if let Some(error) = quiesced.cleanup_error {
            return Err(error.into());
        }
        if timed_out {
            return Err(RunnerError::LaunchTimeout {
                timeout_ns: limits.timeout_ns,
            }
            .into());
        }
        if stdout_exceeded {
            return Err(RunnerError::LaunchOutput {
                stream: "stdout",
                limit: limits.max_output_bytes,
            }
            .into());
        }
        if stderr_exceeded {
            return Err(RunnerError::LaunchOutput {
                stream: "stderr",
                limit: limits.max_output_bytes,
            }
            .into());
        }
        let operation = match status.code() {
            Some(121) => "descriptor flags",
            Some(122) => "control write",
            _ => "launch",
        };
        return Err(RunnerError::Infrastructure { operation, code: 0 }.into());
    }
    if status.code() == Some(123) {
        return Err(RunnerError::Infrastructure {
            operation: "control write",
            code: 0,
        }
        .into());
    }

    let reason = if stdout_exceeded {
        Some(format!("stdout exceeded {} bytes", limits.max_output_bytes))
    } else if stderr_exceeded {
        Some(format!("stderr exceeded {} bytes", limits.max_output_bytes))
    } else if timed_out {
        Some(format!("timed out after {} ns", limits.timeout_ns))
    } else if let Some(detail) = record_detail {
        Some(status_reason(status, detail))
    } else {
        match completion.as_ref() {
            Some(Completion::Ok) if status.code() == Some(0) => None,
            Some(Completion::Error { tag, code }) if status.code() == Some(1) => {
                Some(returned_error(*tag, *code))
            }
            Some(completion) => Some(mismatch_reason(status, completion)),
            None => Some(status_reason(status, "length")),
        }
    };
    let failure = reason.map(|reason| Failure {
        canonical_id: entry.canonical_id.clone(),
        reason,
        stdout: stdout_bytes.into_bytes(),
        stderr: stderr_bytes.into_bytes(),
    });
    if let Some(error) = quiesced.cleanup_error {
        return Err(RowError {
            terminal: error,
            evidence: failure,
        });
    }
    Ok(failure)
}

fn raw_exit(status: i32) -> ! {
    unsafe { libc::_exit(status) }
}

fn remove_stage_or_exit(controller: &SignalController, mut stage: align_driver::ArtifactStage) {
    if let Err(error) = stage.try_remove() {
        let diagnostic = format!(
            "alignc: test runner stage cleanup failed (os error {})\n",
            raw_code(&error)
        );
        match controlled_write(controller, libc::STDERR_FILENO, diagnostic.as_bytes()) {
            Err(TerminalWrite::Signal(signal)) => raw_exit(128 + signal),
            Ok(()) | Err(TerminalWrite::Io(_)) => raw_exit(1),
        }
    }
}

fn terminal_error(
    controller: &SignalController,
    stage: align_driver::ArtifactStage,
    error: RowError,
) -> ! {
    if let Some(evidence) = error.evidence
        && let Err(write_error) = write_failure(controller, &evidence)
    {
        terminal_write_error(controller, stage, write_error);
    }
    let error = error.terminal;
    remove_stage_or_exit(controller, stage);
    if let RunnerError::Signal { signal } = error {
        raw_exit(128 + signal);
    }
    let Some(message) = error.message() else {
        raw_exit(1);
    };
    match controlled_write(controller, libc::STDERR_FILENO, message.as_bytes()) {
        Err(TerminalWrite::Signal(signal)) => raw_exit(128 + signal),
        Ok(()) | Err(TerminalWrite::Io(_)) => raw_exit(1),
    }
}

fn terminal_write_error(
    controller: &SignalController,
    stage: align_driver::ArtifactStage,
    error: TerminalWrite,
) -> ! {
    remove_stage_or_exit(controller, stage);
    if let TerminalWrite::Signal(signal) = error {
        raw_exit(128 + signal);
    }
    let TerminalWrite::Io(error) = error else {
        raw_exit(1);
    };
    let code = raw_code(&error);
    let diagnostic = format!("alignc: test runner report write failed (os error {code})\n");
    match controlled_write(controller, libc::STDERR_FILENO, diagnostic.as_bytes()) {
        Err(TerminalWrite::Signal(signal)) => raw_exit(128 + signal),
        Ok(()) | Err(TerminalWrite::Io(_)) => raw_exit(1),
    }
}

fn block_graceful_signals() -> Result<(), i32> {
    let mut blocked: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::sigemptyset(&mut blocked) };
    for signal in GRACEFUL_SIGNALS {
        unsafe { libc::sigaddset(&mut blocked, signal) };
    }
    let code = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, std::ptr::null_mut()) };
    if code == 0 { Ok(()) } else { Err(code) }
}

pub fn run_and_exit(
    executable: PathBuf,
    mut stage: align_driver::ArtifactStage,
    catalog: Vec<CatalogEntry>,
    suite_cwd: PathBuf,
    limits: Limits,
    cache_report: Vec<u8>,
) -> ! {
    let mut controller = match SignalController::acquire() {
        Ok(controller) => controller,
        Err(error) => {
            let cleanup = stage.try_remove();
            if let Err(cleanup) = cleanup {
                eprintln!(
                    "alignc: test runner stage cleanup failed (os error {})",
                    raw_code(&cleanup)
                );
            } else {
                eprintln!(
                    "alignc: test runner signal handler failed (os error {})",
                    raw_code(&error)
                );
            }
            raw_exit(1);
        }
    };
    if !cache_report.is_empty()
        && let Err(error) = controlled_write(&controller, libc::STDERR_FILENO, &cache_report)
    {
        terminal_write_error(&controller, stage, error);
    }
    drop(cache_report);
    let mut passed = 0;
    let mut failed = 0;
    for (ordinal, entry) in catalog.iter().enumerate() {
        if let Some(signal) = controller.selected() {
            terminal_error(&controller, stage, RunnerError::Signal { signal }.into());
        }
        let outcome = match run_row(
            &mut controller,
            &executable,
            &suite_cwd,
            entry,
            ordinal as u32,
            limits,
        ) {
            Ok(outcome) => outcome,
            Err(error) => terminal_error(&controller, stage, error),
        };
        match outcome {
            Some(failure) => {
                if let Err(error) = write_failure(&controller, &failure) {
                    terminal_write_error(&controller, stage, error);
                }
                failed += 1;
            }
            None => passed += 1,
        }
    }
    remove_stage_or_exit(&controller, stage);
    let (status, summary) = if failed == 0 {
        (0, format!("test result: ok. {passed} passed; 0 failed\n"))
    } else {
        (
            1,
            format!("test result: FAILED. {passed} passed; {failed} failed\n"),
        )
    };
    if let Err(error) = controlled_write(&controller, libc::STDOUT_FILENO, summary.as_bytes()) {
        match error {
            TerminalWrite::Signal(signal) => raw_exit(128 + signal),
            TerminalWrite::Io(error) => {
                let diagnostic = format!(
                    "alignc: test runner report write failed (os error {})\n",
                    raw_code(&error)
                );
                let _ = controlled_write(&controller, libc::STDERR_FILENO, diagnostic.as_bytes());
                raw_exit(1);
            }
        }
    }
    if block_graceful_signals().is_err() {
        raw_exit(1);
    }
    raw_exit(controller.selected().map_or(status, |signal| 128 + signal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_spawn_attributes_close_ambient_descriptors_by_default() {
        let attributes = SpawnAttributes::process_group()
            .expect("process-group attributes")
            .close_ambient()
            .expect("close-on-exec-default attributes");
        let mut flags = 0;
        posix_result(unsafe { libc::posix_spawnattr_getflags(&attributes.0, &mut flags) })
            .expect("read spawn flags");
        let expected =
            (libc::POSIX_SPAWN_SETPGROUP | libc::POSIX_SPAWN_CLOEXEC_DEFAULT) as libc::c_short;
        assert_eq!(flags, expected);
    }

    extern "C" fn prior_signal_handler(_: i32) {}

    fn current_action(signal: i32) -> libc::sigaction {
        let mut action = MaybeUninit::<libc::sigaction>::zeroed();
        assert_eq!(
            unsafe { libc::sigaction(signal, std::ptr::null(), action.as_mut_ptr()) },
            0
        );
        unsafe { action.assume_init() }
    }

    fn install_action(signal: i32, handler: usize) -> libc::sigaction {
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = handler;
        unsafe { libc::sigemptyset(&mut action.sa_mask) };
        let mut previous = MaybeUninit::<libc::sigaction>::zeroed();
        assert_eq!(
            unsafe { libc::sigaction(signal, &action, previous.as_mut_ptr()) },
            0
        );
        unsafe { previous.assume_init() }
    }

    fn restore_action(signal: i32, action: &libc::sigaction) {
        assert_eq!(
            unsafe { libc::sigaction(signal, action, std::ptr::null_mut()) },
            0
        );
    }

    #[test]
    #[ignore = "spawned by signal_controller_process_owner in an isolated process"]
    fn signal_controller_process_probe() {
        let original_hup = install_action(libc::SIGHUP, libc::SIG_IGN);
        let original_int = install_action(libc::SIGINT, prior_signal_handler as *const () as usize);

        let mut controller = SignalController::acquire().expect("signal controller");
        assert!(
            SignalController::acquire().is_err(),
            "a second lease succeeded"
        );
        controller.begin_write().expect("writer permit");
        unsafe { *errno_location() = 777 };
        graceful_signal_handler(libc::SIGINT);
        assert_eq!(unsafe { *errno_location() }, 777);
        assert_eq!(controller.finish_write(), Err(libc::SIGINT));
        assert_eq!(controller.selected(), Some(libc::SIGINT));
        controller.drain().expect("self-pipe drain");
        drop(controller);

        assert_eq!(current_action(libc::SIGHUP).sa_sigaction, libc::SIG_IGN);
        assert_eq!(
            current_action(libc::SIGINT).sa_sigaction,
            prior_signal_handler as *const () as usize
        );

        let mut hup: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut hup);
            libc::sigaddset(&mut hup, libc::SIGHUP);
        }
        let mut prior_mask: libc::sigset_t = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &hup, &mut prior_mask) },
            0
        );
        assert!(SignalController::acquire().is_err());
        assert_eq!(
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &prior_mask, std::ptr::null_mut(),) },
            0
        );
        drop(DriverSignalLease::acquire().expect("blocked setup released lease"));

        let previous_chld = install_action(libc::SIGCHLD, libc::SIG_IGN);
        assert!(SignalController::acquire().is_err());
        restore_action(libc::SIGCHLD, &previous_chld);
        drop(DriverSignalLease::acquire().expect("SIGCHLD rejection released lease"));

        restore_action(libc::SIGHUP, &original_hup);
        restore_action(libc::SIGINT, &original_int);
    }

    #[test]
    fn signal_controller_process_owner() {
        let output =
            std::process::Command::new(std::env::current_exe().expect("current test executable"))
                .args([
                    "--ignored",
                    "--exact",
                    "test_runner::tests::signal_controller_process_probe",
                    "--test-threads=1",
                ])
                .output()
                .expect("spawn signal probe");
        assert!(
            output.status.success(),
            "signal probe failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn set_sigpipe_mask(blocked: bool) {
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, libc::SIGPIPE);
        }
        assert_eq!(
            unsafe {
                libc::pthread_sigmask(
                    if blocked {
                        libc::SIG_BLOCK
                    } else {
                        libc::SIG_UNBLOCK
                    },
                    &set,
                    std::ptr::null_mut(),
                )
            },
            0
        );
    }

    fn sigpipe_is_blocked() -> bool {
        let mut empty: libc::sigset_t = unsafe { std::mem::zeroed() };
        let mut current: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe { libc::sigemptyset(&mut empty) };
        assert_eq!(
            unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &empty, &mut current) },
            0
        );
        unsafe { libc::sigismember(&current, libc::SIGPIPE) == 1 }
    }

    fn sigpipe_is_pending() -> bool {
        let mut pending: libc::sigset_t = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::sigpending(&mut pending) }, 0);
        unsafe { libc::sigismember(&pending, libc::SIGPIPE) == 1 }
    }

    #[test]
    #[ignore = "spawned by controlled_write_sigpipe_process_owner in an isolated process"]
    fn controlled_write_sigpipe_process_probe() {
        let case = std::env::var("ALIGN_TEST_SIGPIPE_CASE").expect("SIGPIPE case");
        let (initially_blocked, pre_pending, closed) = match case.as_str() {
            "success-unblocked" => (false, false, false),
            "success-blocked-pending" => (true, true, false),
            "closed-unblocked" => (false, false, true),
            "closed-blocked-pending" => (true, true, true),
            _ => panic!("unknown SIGPIPE case `{case}`"),
        };

        let _original_action = install_action(libc::SIGPIPE, libc::SIG_DFL);
        set_sigpipe_mask(initially_blocked);
        if pre_pending {
            assert_eq!(unsafe { libc::raise(libc::SIGPIPE) }, 0);
            assert!(sigpipe_is_pending());
        }

        let controller = SignalController::acquire().expect("signal controller");
        let (read, write) = capture_pipe().expect("writer pipe");
        let mut read = Some(read);
        if closed {
            drop(read.take());
        }
        let result = controlled_write(&controller, write.as_raw_fd(), b"x");
        if closed {
            match result {
                Err(TerminalWrite::Io(error)) => {
                    assert_eq!(error.raw_os_error(), Some(libc::EPIPE));
                }
                Err(TerminalWrite::Signal(signal)) => {
                    panic!("closed sink selected signal {signal}")
                }
                Ok(()) => panic!("closed sink write succeeded"),
            }
            assert!(
                sigpipe_is_blocked(),
                "terminal EPIPE path restored the mask"
            );
            assert!(sigpipe_is_pending(), "terminal EPIPE path lost SIGPIPE");
        } else {
            match result {
                Ok(()) => {}
                Err(TerminalWrite::Io(error)) => panic!("successful sink failed: {error}"),
                Err(TerminalWrite::Signal(signal)) => {
                    panic!("successful sink selected signal {signal}")
                }
            }
            assert_eq!(sigpipe_is_blocked(), initially_blocked);
            assert_eq!(sigpipe_is_pending(), pre_pending);
            let mut byte = [0_u8; 1];
            read.as_mut()
                .expect("open reader")
                .read_exact(&mut byte)
                .expect("read controlled byte");
            assert_eq!(byte, [b'x']);
        }
        drop(controller);
    }

    #[test]
    fn controlled_write_sigpipe_process_owner() {
        for case in [
            "success-unblocked",
            "success-blocked-pending",
            "closed-unblocked",
            "closed-blocked-pending",
        ] {
            let output = std::process::Command::new(
                std::env::current_exe().expect("current test executable"),
            )
            .args([
                "--ignored",
                "--exact",
                "test_runner::tests::controlled_write_sigpipe_process_probe",
                "--test-threads=1",
            ])
            .env("ALIGN_TEST_SIGPIPE_CASE", case)
            .output()
            .expect("spawn SIGPIPE probe");
            assert!(
                output.status.success(),
                "SIGPIPE probe `{case}` failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn cleanup_waits_honor_deadlines() {
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .expect("spawn cleanup probe");
        let child_pid = i32::try_from(child.id()).expect("child pid fits i32");
        let child_deadline = Instant::now()
            .checked_add(Duration::from_millis(10))
            .expect("child deadline");
        let error = reap_child(child_pid, child_deadline).expect_err("live child reaped");
        assert_eq!(error.raw_os_error(), Some(libc::ETIMEDOUT));
        child.kill().expect("kill cleanup probe");
        reap_child(child_pid, cleanup_deadline()).expect("reap cleanup probe");

        let group = unsafe { libc::getpgrp() };
        let group_deadline = Instant::now()
            .checked_add(Duration::from_millis(10))
            .expect("group deadline");
        let error =
            wait_process_group_empty(group, group_deadline).expect_err("live group disappeared");
        assert_eq!(error.raw_os_error(), Some(libc::ETIMEDOUT));
    }

    #[test]
    #[ignore = "spawned by row_cleanup_reuses_original_deadline in an isolated process"]
    fn row_cleanup_deadline_process_probe() {
        let mut controller = SignalController::acquire().expect("signal controller");
        let child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .expect("spawn cleanup deadline probe");
        let pid = i32::try_from(child.id()).expect("child pid fits i32");
        let mut child_guard = ChildGuard::new(pid);
        let (parent_control, _child_control) = UnixDatagram::pair().expect("control pair");
        parent_control
            .set_nonblocking(true)
            .expect("nonblocking control");
        let stdout = File::open("/dev/null").expect("open null stdout");
        let stderr = File::open("/dev/null").expect("open null stderr");
        let mut acknowledged = true;
        let mut completion = None;
        let mut record_detail = None;
        let mut stdout_bytes = reserve_capture(0).expect("stdout capture");
        let mut stderr_bytes = reserve_capture(0).expect("stderr capture");
        let mut stdout_eof = false;
        let mut stderr_eof = false;
        let mut stdout_exceeded = false;
        let mut stderr_exceeded = false;
        let deadline = Instant::now();
        let started = Instant::now();
        let quiesced = quiesce_child(
            &mut controller,
            &mut child_guard,
            parent_control,
            stdout,
            stderr,
            0,
            &mut acknowledged,
            &mut completion,
            &mut record_detail,
            &mut stdout_bytes,
            &mut stderr_bytes,
            &mut stdout_eof,
            &mut stderr_eof,
            &mut stdout_exceeded,
            &mut stderr_exceeded,
            deadline,
            Some(libc::SIGTERM),
        );
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "cleanup reset the expired row deadline and waited a new grace period"
        );
        if quiesced.status.is_none() {
            reap_child(pid, cleanup_deadline()).expect("reap cleanup deadline probe");
        }
        assert_eq!(quiesced.interrupted, Some(libc::SIGTERM));
        drop(child);
    }

    #[test]
    fn row_cleanup_reuses_original_deadline() {
        let output = std::process::Command::new(
            std::env::current_exe().expect("current test executable"),
        )
        .args([
            "--ignored",
            "--exact",
            "test_runner::tests::row_cleanup_deadline_process_probe",
            "--test-threads=1",
        ])
        .output()
        .expect("spawn cleanup deadline owner");
        assert!(
            output.status.success(),
            "cleanup deadline probe failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn parent_protocol_codecs_match_golden_vectors() {
        assert_eq!(launch_record(7), *b"ALTESTL\x01\x07\0\0\0\0\0\0\0");
        assert!(decode_ack(b"ALTESTA\x01\x07\0\0\0\0\0\0\0", 7));
        assert_eq!(
            decode_completion(b"ALTEST\0\x01\0\xff\0\0\0\0\0\0\x07\0\0\0", 7),
            Ok(Completion::Ok)
        );
        assert_eq!(
            decode_completion(b"ALTEST\0\x01\x01\x01\0\0\0\0\0\0\x07\0\0\0", 7),
            Ok(Completion::Error { tag: 1, code: 0 })
        );
        assert_eq!(
            decode_completion(b"ALTEST\0\x01\x01\x04\0\0\xf7\xff\xff\xff\x07\0\0\0", 7),
            Ok(Completion::Error { tag: 4, code: -9 })
        );
        let repeated_long = b"ALTEST\0\x01\0\xff\0\0\0\0\0\0\x07\0\0\0x";
        assert!(completion_magic(repeated_long));
        assert_ne!(repeated_long.len(), 20);
        let (parent, child) = UnixDatagram::pair().expect("control pair");
        parent.set_nonblocking(true).expect("nonblocking parent");
        child.send(b"ALTESTA\x01\x07\0\0\0\0\0\0\0").expect("ack");
        child
            .send(b"ALTEST\0\x01\0\xff\0\0\0\0\0\0\x07\0\0\0")
            .expect("completion");
        child
            .send(repeated_long)
            .expect("long repetition candidate");
        let mut acknowledged = false;
        let mut completion = None;
        let mut detail = None;
        drain_control(&parent, 7, &mut acknowledged, &mut completion, &mut detail)
            .expect("control drain");
        assert!(acknowledged);
        assert_eq!(completion, Some(Completion::Ok));
        assert_eq!(detail, Some("order"));

        let (parent, child) = UnixDatagram::pair().expect("control pair");
        parent.set_nonblocking(true).expect("nonblocking parent");
        child.send(b"invalid").expect("malformed pre-ack record");
        child
            .send(b"ALTESTA\x01\x07\0\0\0\0\0\0\0")
            .expect("late ack");
        let mut acknowledged = false;
        let mut completion = None;
        let mut detail = None;
        drain_control(&parent, 7, &mut acknowledged, &mut completion, &mut detail)
            .expect("malformed pre-ack drain");
        assert!(
            !acknowledged,
            "a later acknowledgement cannot erase a launch error"
        );
        assert_eq!(completion, None);
        assert_eq!(detail, Some("order"));
    }

    #[test]
    fn capture_uses_the_selected_bound_and_one_probe_byte() {
        for (limit, input, expected, eof, exceeded) in [
            (0, b"".as_slice(), b"".as_slice(), true, false),
            (0, b"x".as_slice(), b"".as_slice(), false, true),
            (3, b"abc".as_slice(), b"abc".as_slice(), true, false),
            (3, b"abcd".as_slice(), b"abc".as_slice(), false, true),
        ] {
            let mut capture = reserve_capture(limit).expect("capture allocation");
            let mut input = std::io::Cursor::new(input);
            let actual = drain_stream(&mut input, &mut capture).expect("capture read");
            assert_eq!(actual, (eof, exceeded));
            assert_eq!(capture.into_bytes(), expected);
        }
    }
}
