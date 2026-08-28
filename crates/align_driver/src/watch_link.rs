//! Captured linker/strip execution and watch-safe executable publication.

use crate::{ArtifactStage, LinkPlan, ObjectFormat, Profile};
use align_interface::Hash128Stream;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const PUMP_BYTES: usize = 4_096;
const POST_EXIT_LIMIT: usize = 1_048_576;
const OUTPUT_LIMIT: u64 = 8 * 1_024 * 1_024 * 1_024;
const COPY_BYTES: usize = 64 * 1_024;
const POLL_MILLIS: i32 = 50;
const STOP_GRACE: Duration = Duration::from_millis(250);

static CAPTURED_CHILD_ACTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static TEST_POLL_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

struct CapturedChildGuard {
    child: Child,
    stdout: Option<File>,
    stderr: Option<File>,
    pid: i32,
    armed: bool,
}

impl CapturedChildGuard {
    fn new(mut child: Child, stdout: File, stderr: File) -> Result<Self, String> {
        let pid = match i32::try_from(child.id()) {
            Ok(pid) => pid,
            Err(_) => {
                drop(stdout);
                drop(stderr);
                let _ = child.kill();
                let _ = child.wait();
                return Err("child wait setup: pid exceeds i32".to_string());
            }
        };
        Ok(Self {
            child,
            stdout: Some(stdout),
            stderr: Some(stderr),
            pid,
            armed: true,
        })
    }

    fn close_reads(&mut self) {
        self.stdout.take();
        self.stderr.take();
    }

    fn finish(&mut self, first_error: &mut Option<String>) -> Result<ExitStatus, String> {
        self.close_reads();
        cleanup_child(self.pid, first_error);
        let status = loop {
            match self.child.wait() {
                Ok(status) => break status,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(format!("child wait: {error}")),
            }
        };
        self.armed = false;
        Ok(status)
    }
}

impl Drop for CapturedChildGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.close_reads();
        let mut ignored = None;
        cleanup_child(self.pid, &mut ignored);
        loop {
            match self.child.wait() {
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkOutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkStopSignal {
    SigHup,
    SigInt,
    SigQuit,
    SigTerm,
}

pub trait LinkOutputSink {
    fn write(&mut self, stream: LinkOutputStream, bytes: &[u8]) -> io::Result<()>;

    fn stop_signal(&mut self) -> Option<LinkStopSignal> {
        None
    }
}

struct ChildLease;

impl ChildLease {
    fn acquire() -> Result<Self, String> {
        CAPTURED_CHILD_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "child wait setup: another captured child is active".to_string())?;
        if !sigchld_is_compatible() {
            CAPTURED_CHILD_ACTIVE.store(false, Ordering::Release);
            return Err("child wait setup: incompatible SIGCHLD disposition".to_string());
        }
        Ok(Self)
    }
}

impl Drop for ChildLease {
    fn drop(&mut self) {
        CAPTURED_CHILD_ACTIVE.store(false, Ordering::Release);
    }
}

struct OwnedStage {
    path: PathBuf,
    file: File,
    identity: (u64, u64),
}

impl OwnedStage {
    fn create(parent: &Path, kind: &str) -> Result<Self, String> {
        let stage = ArtifactStage::in_dir(parent, kind)
            .map_err(|error| format!("cannot create executable staging directory: {error}"))?;
        let path = stage.path().join("output");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|error| format!("cannot create link output stage: {error}"))?;
        let metadata = file
            .metadata()
            .map_err(|error| format!("cannot inspect link output stage: {error}"))?;
        let identity = (metadata.dev(), metadata.ino());
        // Transfer directory cleanup to this identity-aware owner. `ArtifactStage`'s recursive
        // cleanup cannot be used here because a foreign replacement must be preserved.
        let _directory = stage.into_owned_dir();
        Ok(Self {
            path,
            file,
            identity,
        })
    }

    fn verify_name(&self, message: &'static str) -> Result<(), String> {
        let metadata = fs::symlink_metadata(&self.path).map_err(|_| message.to_string())?;
        if metadata.file_type().is_symlink() || (metadata.dev(), metadata.ino()) != self.identity {
            return Err(message.to_string());
        }
        Ok(())
    }

    fn remove_name(&self, message: &'static str) -> Result<(), String> {
        match fs::symlink_metadata(&self.path) {
            Ok(metadata)
                if !metadata.file_type().is_symlink()
                    && (metadata.dev(), metadata.ino()) == self.identity =>
            {
                fs::remove_file(&self.path).map_err(|error| format!("{message}: {error}"))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            _ => Err(message.to_string()),
        }
    }
}

impl Drop for OwnedStage {
    fn drop(&mut self) {
        let _ = self.remove_name("stage ownership lost");
        if let Some(parent) = self.path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

pub fn link_objects_with_output(
    objs: &[&Path],
    exe: &Path,
    link_libs: &[String],
    profile: Profile,
    sink: &mut dyn LinkOutputSink,
) -> Result<(), String> {
    link_captured(objs, exe, link_libs, profile, None, sink)
}

pub fn link_objects_instrumented_with_output(
    objs: &[&Path],
    exe: &Path,
    link_libs: &[String],
    profile: Profile,
    profile_rt: &Path,
    sink: &mut dyn LinkOutputSink,
) -> Result<(), String> {
    link_captured(objs, exe, link_libs, profile, Some(profile_rt), sink)
}

fn link_captured(
    objs: &[&Path],
    exe: &Path,
    link_libs: &[String],
    profile: Profile,
    profile_rt: Option<&Path>,
    sink: &mut dyn LinkOutputSink,
) -> Result<(), String> {
    let lease = ChildLease::acquire()?;
    let format = crate::target_object_format()?;
    let runtime = crate::runtime_archive()?;
    let ordered_link_libs = crate::order_link_libs(link_libs);
    let linker = crate::select_linker(format)?;
    let parent = exe
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tool = OwnedStage::create(parent, "align-watch-tool")?;
    let args = crate::link_command_args(&LinkPlan {
        objs,
        exe: &tool.path,
        runtime: &runtime,
        ordered_link_libs: &ordered_link_libs,
        format,
        profile,
        profile_rt,
        linker: &linker,
    });
    let status = run_captured("cc", &args, sink)?;
    if !status.success() {
        return Err(crate::link_failure_message(
            status.code(),
            &ordered_link_libs,
            &linker,
        ));
    }
    if profile.strip() && format == ObjectFormat::MachO {
        let strip_args = [tool.path.as_os_str().to_os_string()];
        let status = run_captured("strip", &strip_args, sink)?;
        if !status.success() {
            return Err(format!("strip failed (exit code {:?})", status.code()));
        }
    }
    drop(lease);
    isolate_and_publish(&mut tool, exe)
}

fn run_captured(
    program: &str,
    args: &[std::ffi::OsString],
    sink: &mut dyn LinkOutputSink,
) -> Result<ExitStatus, String> {
    use std::os::unix::process::CommandExt;

    let mut command = Command::new(program);
    let (stdout, child_stdout) =
        child_output_pipe().map_err(|error| format!("child wait setup: stdout pipe: {error}"))?;
    let (stderr, child_stderr) =
        child_output_pipe().map_err(|error| format!("child wait setup: stderr pipe: {error}"))?;
    command.args(args).stdout(child_stdout).stderr(child_stderr);
    // SAFETY: the closure performs only the async-signal-safe `setpgid` syscall between fork and
    // exec. It allocates nothing and touches no Rust synchronization state.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command
        .spawn()
        .map_err(|error| format!("cannot launch {program}: {error}"))?;
    drop(command);
    let mut child = CapturedChildGuard::new(child, stdout, stderr)?;
    let pid = child.pid;

    let mut first_error: Option<String> = None;
    let mut panic_payload = None;
    let mut stopped = None;
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut observed_exit = false;
    let mut pump_failed = false;

    while !observed_exit && panic_payload.is_none() {
        match verify_group(pid) {
            Ok(()) => {}
            Err(error) => {
                first_error.get_or_insert(error);
                break;
            }
        }
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink.stop_signal())) {
            Ok(Some(signal)) => {
                stopped = Some(signal);
                match send_signal(pid, signal_number(signal)) {
                    Ok(true) => {
                        if let Err(error) = wait_grace(pid, STOP_GRACE) {
                            first_error.get_or_insert(format!("child wait: {error}"));
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        first_error.get_or_insert(format!("child group cleanup: {error}"));
                    }
                }
                break;
            }
            Ok(None) => {}
            Err(payload) => {
                panic_payload = Some(payload);
                break;
            }
        }
        match child_exited_without_reap(pid) {
            Ok(value) => observed_exit = value,
            Err(error) => {
                first_error.get_or_insert(format!("child wait: {error}"));
                break;
            }
        }
        if observed_exit {
            break;
        }
        let mut fds = [
            libc::pollfd {
                fd: if stdout_open {
                    child.stdout.as_ref().map_or(-1, AsRawFd::as_raw_fd)
                } else {
                    -1
                },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if stderr_open {
                    child.stderr.as_ref().map_or(-1, AsRawFd::as_raw_fd)
                } else {
                    -1
                },
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        #[cfg(test)]
        TEST_POLL_COUNT.fetch_add(1, Ordering::Relaxed);
        // SAFETY: `fds` is a live two-element array for the duration of poll.
        let polled = unsafe { libc::poll(fds.as_mut_ptr(), 2, POLL_MILLIS) };
        if polled < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            first_error.get_or_insert(format!("child output poll: {error}"));
            pump_failed = true;
            break;
        }
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink.stop_signal())) {
            Ok(Some(signal)) => {
                stopped = Some(signal);
                match send_signal(pid, signal_number(signal)) {
                    Ok(true) => {
                        if let Err(error) = wait_grace(pid, STOP_GRACE) {
                            first_error.get_or_insert(format!("child wait: {error}"));
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        first_error.get_or_insert(format!("child group cleanup: {error}"));
                    }
                }
                break;
            }
            Ok(None) => {}
            Err(payload) => {
                panic_payload = Some(payload);
                break;
            }
        }
        if fds
            .iter()
            .any(|fd| fd.revents & (libc::POLLERR | libc::POLLNVAL) != 0)
        {
            first_error.get_or_insert("child output poll: descriptor error".to_string());
            pump_failed = true;
            break;
        }
        for stream in [LinkOutputStream::Stdout, LinkOutputStream::Stderr] {
            let open = match stream {
                LinkOutputStream::Stdout => &mut stdout_open,
                LinkOutputStream::Stderr => &mut stderr_open,
            };
            if !*open || panic_payload.is_some() {
                continue;
            }
            let index = usize::from(matches!(stream, LinkOutputStream::Stderr));
            if fds[index].revents & (libc::POLLIN | libc::POLLHUP) == 0 {
                continue;
            }
            let readable = match stream {
                LinkOutputStream::Stdout => child.stdout.as_mut(),
                LinkOutputStream::Stderr => child.stderr.as_mut(),
            };
            let Some(readable) = readable else {
                first_error.get_or_insert(format!(
                    "child {} read: descriptor owner missing",
                    stream_name(stream)
                ));
                pump_failed = true;
                break;
            };
            match read_one(readable, stream, sink, &mut panic_payload, &mut first_error) {
                Ok(true) => {
                    *open = false;
                    match stream {
                        LinkOutputStream::Stdout => child.stdout.take(),
                        LinkOutputStream::Stderr => child.stderr.take(),
                    };
                }
                Ok(false) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                    *open = false;
                    pump_failed = true;
                }
            }
        }
    }

    if observed_exit && panic_payload.is_none() {
        if let Some(stdout) = child.stdout.as_mut() {
            drain_queued(
                stdout,
                LinkOutputStream::Stdout,
                sink,
                &mut panic_payload,
                &mut first_error,
            );
        }
        if let Some(stderr) = child.stderr.as_mut() {
            drain_queued(
                stderr,
                LinkOutputStream::Stderr,
                sink,
                &mut panic_payload,
                &mut first_error,
            );
        }
    }
    child.close_reads();
    if pump_failed && stopped.is_none() && panic_payload.is_none() {
        match send_signal(pid, libc::SIGTERM) {
            Ok(true) => {
                if let Err(error) = wait_grace(pid, STOP_GRACE) {
                    first_error.get_or_insert(format!("child wait: {error}"));
                }
            }
            Ok(false) => {}
            Err(error) => {
                first_error.get_or_insert(format!("child group cleanup: {error}"));
            }
        }
    }
    let status = child.finish(&mut first_error);
    if let Some(payload) = panic_payload {
        std::panic::resume_unwind(payload);
    }
    let status = status?;
    if let Some(error) = first_error {
        return Err(error);
    }
    if let Some(signal) = stopped {
        return Err(format!("child stopped by {}", signal_name(signal)));
    }
    Ok(status)
}

fn read_one(
    reader: &mut dyn Read,
    stream: LinkOutputStream,
    sink: &mut dyn LinkOutputSink,
    panic_payload: &mut Option<Box<dyn std::any::Any + Send>>,
    first_error: &mut Option<String>,
) -> Result<bool, String> {
    let mut buffer = [0u8; PUMP_BYTES];
    match reader.read(&mut buffer) {
        Ok(0) => Ok(true),
        Ok(count) => {
            if first_error.is_some() {
                return Ok(false);
            }
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sink.write(stream, &buffer[..count])
            })) {
                Ok(Ok(())) => Ok(false),
                Ok(Err(error)) => {
                    let name = stream_name(stream);
                    first_error.get_or_insert(format!("child {name} output: {error}"));
                    Ok(false)
                }
                Err(payload) => {
                    *panic_payload = Some(payload);
                    Ok(false)
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(false),
        Err(error) => Err(format!("child {} read: {error}", stream_name(stream))),
    }
}

fn drain_queued(
    reader: &mut (impl Read + AsRawFd),
    stream: LinkOutputStream,
    sink: &mut dyn LinkOutputSink,
    panic_payload: &mut Option<Box<dyn std::any::Any + Send>>,
    first_error: &mut Option<String>,
) {
    let mut queued: libc::c_int = 0;
    // SAFETY: `queued` is a valid output integer and the descriptor remains owned by `reader`.
    if unsafe { libc::ioctl(reader.as_raw_fd(), libc::FIONREAD, &mut queued) } == -1 {
        first_error.get_or_insert(format!(
            "child {} read: {}",
            stream_name(stream),
            io::Error::last_os_error()
        ));
        return;
    }
    if queued < 0 {
        first_error.get_or_insert(format!(
            "child {} read: invalid buffered-byte count",
            stream_name(stream)
        ));
        return;
    }
    let Ok(mut remaining) = usize::try_from(queued) else {
        return;
    };
    if remaining > POST_EXIT_LIMIT {
        first_error.get_or_insert(format!(
            "child {} read: buffered output exceeds {POST_EXIT_LIMIT} bytes",
            stream_name(stream)
        ));
        return;
    }
    while remaining > 0 && panic_payload.is_none() {
        let mut limited =
            reader.take(u64::try_from(remaining.min(PUMP_BYTES)).unwrap_or(PUMP_BYTES as u64));
        let before = remaining;
        if let Err(error) = read_one(&mut limited, stream, sink, panic_payload, first_error) {
            first_error.get_or_insert(error);
            break;
        }
        let consumed = before.min(PUMP_BYTES) - usize::try_from(limited.limit()).unwrap_or(0);
        if consumed == 0 {
            first_error.get_or_insert(format!("child {} read: early EOF", stream_name(stream)));
            break;
        }
        remaining -= consumed;
    }
}

fn cleanup_child(pid: i32, first_error: &mut Option<String>) {
    for target in [-pid, pid] {
        loop {
            // SAFETY: `target` names the retained direct pid or its pinned process group.
            let result = unsafe { libc::kill(target, libc::SIGKILL) };
            if result == 0 {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.raw_os_error() == Some(libc::ESRCH) {
                break;
            }
            first_error.get_or_insert(format!("child group cleanup: {error}"));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn wait_grace(pid: i32, duration: Duration) -> io::Result<()> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if child_exited_without_reap(pid)? {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn child_exited_without_reap(pid: i32) -> io::Result<bool> {
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    // SAFETY: `info` is writable and waitid is called with WNOWAIT so ownership stays with Child.
    let id = libc::id_t::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "child pid exceeds id_t"))?;
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            id,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful waitid initialized the fixed siginfo record.
    let info = unsafe { info.assume_init() };
    // SAFETY: a successful `waitid` initializes the SIGCHLD payload; POSIX specifies a zero pid
    // when WNOHANG has no status to report.
    Ok(unsafe { info.si_pid() } != 0)
}

fn verify_group(pid: i32) -> Result<(), String> {
    // SAFETY: getpgid is read-only for a retained child pid.
    let group = unsafe { libc::getpgid(pid) };
    if group == pid {
        Ok(())
    } else if group >= 0 {
        Err("child group changed".to_string())
    } else {
        Err(format!("child group check: {}", io::Error::last_os_error()))
    }
}

fn send_signal(pid: i32, signal: i32) -> io::Result<bool> {
    let mut first_error = None;
    let mut delivered = false;
    for target in [-pid, pid] {
        loop {
            // SAFETY: target is the retained direct pid or its private group.
            if unsafe { libc::kill(target, signal) } == 0 {
                delivered = true;
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.raw_os_error() == Some(libc::ESRCH) {
                break;
            }
            if first_error.is_none() {
                first_error = Some(error);
            }
            break;
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(delivered),
    }
}

fn set_nonblocking(fd: i32) -> io::Result<()> {
    // SAFETY: fcntl reads/updates only flags on the live owned pipe descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn child_output_pipe() -> io::Result<(File, Stdio)> {
    let mut descriptors = [-1; 2];
    // SAFETY: `descriptors` is a writable two-element descriptor array.
    if unsafe { libc::pipe(descriptors.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let setup = (|| {
        set_cloexec(descriptors[0])?;
        set_cloexec(descriptors[1])?;
        set_nonblocking(descriptors[0])
    })();
    if let Err(error) = setup {
        // SAFETY: both descriptors were returned by `pipe` and remain owned by this setup path.
        unsafe {
            libc::close(descriptors[1]);
            libc::close(descriptors[0]);
        }
        return Err(error);
    }
    // SAFETY: each live descriptor is transferred exactly once to its Rust owner.
    let read = unsafe { File::from_raw_fd(descriptors[0]) };
    // SAFETY: the write descriptor is distinct and transferred exactly once to `Stdio`.
    let write = unsafe { File::from_raw_fd(descriptors[1]) };
    Ok((read, Stdio::from(write)))
}

fn set_cloexec(fd: i32) -> io::Result<()> {
    // SAFETY: fcntl reads/updates only descriptor flags on the live owned pipe descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn sigchld_is_compatible() -> bool {
    let mut action = std::mem::MaybeUninit::<libc::sigaction>::zeroed();
    // SAFETY: a null second argument queries the process disposition into `action`.
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), action.as_mut_ptr()) } == -1 {
        return false;
    }
    // SAFETY: successful sigaction initialized the record.
    let action = unsafe { action.assume_init() };
    action.sa_sigaction == libc::SIG_DFL && action.sa_flags & libc::SA_NOCLDWAIT == 0
}

fn isolate_and_publish(tool: &mut OwnedStage, exe: &Path) -> Result<(), String> {
    tool.verify_name("tool stage ownership lost")?;
    let before = tool
        .file
        .metadata()
        .map_err(|error| format!("cannot inspect link output: {error}"))?;
    if before.len() > OUTPUT_LIMIT {
        return Err(format!(
            "link output exceeds {OUTPUT_LIMIT}-byte isolation limit"
        ));
    }
    let mode = before.mode() & 0o777;
    let mut buffer = [0u8; COPY_BYTES];
    let source_hash = hash_exact(
        &mut tool.file,
        before.len(),
        &mut buffer,
        "cannot read link output",
    )?;
    let parent = exe
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut publication = OwnedStage::create(parent, "align-watch-publish")?;
    tool.file
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("cannot seek link output: {error}"))?;
    publication
        .file
        .set_len(0)
        .map_err(|error| format!("cannot truncate publication stage: {error}"))?;
    let mut copied = 0u64;
    while copied < before.len() {
        let remaining =
            usize::try_from((before.len() - copied).min(COPY_BYTES as u64)).unwrap_or(COPY_BYTES);
        let count = tool
            .file
            .read(&mut buffer[..remaining])
            .map_err(|error| format!("cannot read link output: {error}"))?;
        if count == 0 {
            return Err("link output changed during isolation".to_string());
        }
        publication
            .file
            .write_all(&buffer[..count])
            .map_err(|error| format!("cannot copy link output: {error}"))?;
        copied += u64::try_from(count).unwrap_or(u64::MAX);
    }
    let mut extra = [0u8; 1];
    if tool
        .file
        .read(&mut extra)
        .map_err(|error| format!("cannot read link output: {error}"))?
        != 0
    {
        return Err("link output changed during isolation".to_string());
    }
    publication
        .file
        .flush()
        .map_err(|error| format!("cannot flush publication stage: {error}"))?;
    fs::set_permissions(&publication.path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("cannot set publication mode: {error}"))?;
    let after = tool
        .file
        .metadata()
        .map_err(|error| format!("cannot inspect link output: {error}"))?;
    let destination = publication
        .file
        .metadata()
        .map_err(|error| format!("cannot inspect publication stage: {error}"))?;
    let source_after_hash = hash_exact(
        &mut tool.file,
        after.len(),
        &mut buffer,
        "cannot reread link output",
    )?;
    let destination_hash = hash_exact(
        &mut publication.file,
        destination.len(),
        &mut buffer,
        "cannot read publication stage",
    )?;
    if (before.dev(), before.ino()) != (after.dev(), after.ino())
        || before.len() != after.len()
        || before.len() != destination.len()
        || before.mode() & 0o777 != after.mode() & 0o777
        || mode != destination.mode() & 0o777
        || source_hash != source_after_hash
        || source_hash != destination_hash
    {
        return Err("link output changed during isolation".to_string());
    }
    tool.remove_name("tool stage ownership lost")?;
    publication.verify_name("publication stage ownership lost")?;
    fs::rename(&publication.path, exe)
        .map_err(|error| format!("cannot publish executable {}: {error}", exe.display()))?;
    Ok(())
}

fn hash_exact(
    file: &mut File,
    len: u64,
    buffer: &mut [u8; COPY_BYTES],
    operation: &str,
) -> Result<align_interface::Hash128, String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("{operation}: {error}"))?;
    let expected = usize::try_from(len).map_err(|_| "link output changed during isolation")?;
    let mut hash = Hash128Stream::for_len(expected);
    let mut consumed = 0u64;
    while consumed < len {
        let remaining =
            usize::try_from((len - consumed).min(COPY_BYTES as u64)).unwrap_or(COPY_BYTES);
        let count = file
            .read(&mut buffer[..remaining])
            .map_err(|error| format!("{operation}: {error}"))?;
        if count == 0 || !hash.update(&buffer[..count]) {
            return Err("link output changed during isolation".to_string());
        }
        consumed = consumed
            .checked_add(u64::try_from(count).unwrap_or(u64::MAX))
            .ok_or_else(|| "link output changed during isolation".to_string())?;
    }
    let mut extra = [0u8; 1];
    if file
        .read(&mut extra)
        .map_err(|error| format!("{operation}: {error}"))?
        != 0
    {
        return Err("link output changed during isolation".to_string());
    }
    hash.finish()
        .ok_or_else(|| "link output changed during isolation".to_string())
}

fn stream_name(stream: LinkOutputStream) -> &'static str {
    match stream {
        LinkOutputStream::Stdout => "stdout",
        LinkOutputStream::Stderr => "stderr",
    }
}

fn signal_number(signal: LinkStopSignal) -> i32 {
    match signal {
        LinkStopSignal::SigHup => libc::SIGHUP,
        LinkStopSignal::SigInt => libc::SIGINT,
        LinkStopSignal::SigQuit => libc::SIGQUIT,
        LinkStopSignal::SigTerm => libc::SIGTERM,
    }
}

fn signal_name(signal: LinkStopSignal) -> &'static str {
    match signal {
        LinkStopSignal::SigHup => "SIGHUP",
        LinkStopSignal::SigInt => "SIGINT",
        LinkStopSignal::SigQuit => "SIGQUIT",
        LinkStopSignal::SigTerm => "SIGTERM",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    static CAPTURE_TEST: Mutex<()> = Mutex::new(());

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("align-watch-link-{}-{id}", std::process::id()));
            fs::create_dir(&path).expect("create temp directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Default)]
    struct Sink {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    }

    impl LinkOutputSink for Sink {
        fn write(&mut self, stream: LinkOutputStream, bytes: &[u8]) -> io::Result<()> {
            match stream {
                LinkOutputStream::Stdout => self.stdout.extend_from_slice(bytes),
                LinkOutputStream::Stderr => self.stderr.extend_from_slice(bytes),
            }
            Ok(())
        }
    }

    struct PanicSink;

    impl LinkOutputSink for PanicSink {
        fn write(&mut self, _stream: LinkOutputStream, _bytes: &[u8]) -> io::Result<()> {
            std::panic::panic_any(0x51a7_u32)
        }
    }

    struct StopSink;

    impl LinkOutputSink for StopSink {
        fn write(&mut self, _stream: LinkOutputStream, _bytes: &[u8]) -> io::Result<()> {
            Ok(())
        }

        fn stop_signal(&mut self) -> Option<LinkStopSignal> {
            Some(LinkStopSignal::SigTerm)
        }
    }

    #[test]
    fn captured_link_publishes_only_the_isolated_inode() {
        let _serial = CAPTURE_TEST.lock().expect("capture test lock");
        let temp = TempDir::new();
        let source = temp.0.join("main.c");
        let object = temp.0.join("main.o");
        let executable = temp.0.join("program");
        fs::write(&source, b"int main(void) { return 7; }\n").expect("write C source");
        let status = Command::new("cc")
            .args(["-c"])
            .arg(&source)
            .arg("-o")
            .arg(&object)
            .status()
            .expect("compile C object");
        assert!(status.success());
        let mut sink = Sink::default();
        link_objects_with_output(
            &[object.as_path()],
            &executable,
            &[],
            Profile::Release,
            &mut sink,
        )
        .expect("captured link");
        assert!(sink.stdout.is_empty());
        assert!(sink.stderr.is_empty());
        assert_eq!(
            Command::new(&executable)
                .status()
                .expect("run output")
                .code(),
            Some(7)
        );
        let residue = fs::read_dir(&temp.0)
            .expect("read temp")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.contains("align-watch"))
            .collect::<Vec<_>>();
        assert!(
            residue.is_empty(),
            "private stages must be cleaned: {residue:?}"
        );
    }

    #[test]
    fn sink_panic_resumes_only_after_child_cleanup() {
        let _serial = CAPTURE_TEST.lock().expect("capture test lock");
        let mut sink = PanicSink;
        let args = [
            std::ffi::OsString::from("-c"),
            std::ffi::OsString::from("printf payload; sleep 30"),
        ];
        let started = Instant::now();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = run_captured("sh", &args, &mut sink);
        }))
        .expect_err("sink panic must resume");
        assert_eq!(panic.downcast_ref::<u32>(), Some(&0x51a7));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn stop_control_forwards_then_kills_and_reaps() {
        let _serial = CAPTURE_TEST.lock().expect("capture test lock");
        let mut sink = StopSink;
        let args = [
            std::ffi::OsString::from("-c"),
            std::ffi::OsString::from("sleep 30"),
        ];
        let started = Instant::now();
        let error = run_captured("sh", &args, &mut sink).expect_err("stop must interrupt child");
        assert_eq!(error, "child stopped by SIGTERM");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn direct_exit_does_not_wait_for_descendant_pipe_eof() {
        let _serial = CAPTURE_TEST.lock().expect("capture test lock");
        let mut sink = Sink::default();
        let args = [
            std::ffi::OsString::from("-c"),
            std::ffi::OsString::from("sleep 30 & printf done"),
        ];
        let started = Instant::now();
        let status = run_captured("sh", &args, &mut sink).expect("capture child");
        assert!(status.success());
        assert_eq!(sink.stdout, b"done");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn closed_child_streams_are_removed_from_the_poll_set() {
        let _serial = CAPTURE_TEST.lock().expect("capture test lock");
        TEST_POLL_COUNT.store(0, Ordering::Relaxed);
        let mut sink = Sink::default();
        let args = [
            std::ffi::OsString::from("-c"),
            std::ffi::OsString::from("exec 1>&- 2>&-; sleep 1"),
        ];
        let status = run_captured("sh", &args, &mut sink).expect("capture child");
        assert!(status.success());
        assert!(sink.stdout.is_empty());
        assert!(sink.stderr.is_empty());
        assert!(
            TEST_POLL_COUNT.load(Ordering::Relaxed) < 100,
            "closed streams must leave poll sleeping instead of spinning"
        );
    }
}
