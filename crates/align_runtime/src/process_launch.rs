//! One parent-marshalled native launch path. Bootstrap performs no application callbacks.
use super::process_live::{NativeChild, disjoint, valid_pointer};
use super::{AL_INVALID, AL_TIMEOUT, Command, Writer, WriterSink, io_error_to_status};
use std::ffi::{CString, OsString};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::sync::Mutex;
#[cfg(target_os = "macos")]
mod darwin;
#[cfg(target_os = "macos")]
use darwin::SignalMask;

pub(crate) struct CreationState {
    pub children: usize,
    pub scope: bool,
}
pub(crate) static CREATION: Mutex<CreationState> = Mutex::new(CreationState {
    children: 0,
    scope: false,
});
pub(crate) fn creation() -> std::sync::MutexGuard<'static, CreationState> {
    CREATION
        .lock()
        .unwrap_or_else(|_| super::panic_abort("process creation state poisoned"))
}
fn error() -> i32 {
    io_error_to_status(&std::io::Error::last_os_error())
}
#[cfg(test)]
std::thread_local! {
    static FAILURE_PHASE: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    static OBSERVED_PHASE: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn inject_acquisition_failure(phase:usize) {
    FAILURE_PHASE.with(|value|value.set(phase));
    OBSERVED_PHASE.with(|value|value.set(0));
}
// Parent-side acquisition checkpoints; never called from a forked bootstrap.
#[inline]
fn acquisition() -> Result<(), i32> {
    #[cfg(test)]
    {
        let phase = OBSERVED_PHASE.with(|value| {
            let next = value.get() + 1;
            value.set(next);
            next
        });
        if FAILURE_PHASE.with(|value| value.get() == phase) {
            return Err(io_error_to_status(&std::io::Error::from_raw_os_error(
                libc::EIO,
            )));
        }
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn native_error() -> i32 {
    unsafe { *libc::__errno_location() }
}
#[cfg(target_os = "macos")]
fn native_error() -> i32 {
    unsafe { *libc::__error() }
}
pub(crate) fn duplicate(fd: i32) -> Result<OwnedFd, i32> {
    acquisition()?;
    let copied = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
    if copied < 0 {
        Err(error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(copied) })
    }
}
fn above_standard(fd: OwnedFd) -> Result<OwnedFd, i32> {
    if fd.as_raw_fd() > 2 {
        Ok(fd)
    } else {
        duplicate(fd.as_raw_fd())
    }
}
fn pipe() -> Result<[OwnedFd; 2], i32> {
    acquisition()?;
    let mut fds = [-1; 2];
    #[cfg(target_os = "linux")]
    let status = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(not(target_os = "linux"))]
    let status = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if status != 0 {
        return Err(error());
    }
    let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    #[cfg(not(target_os = "linux"))]
    for fd in [&read, &write] {
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(error());
        }
    }
    Ok([above_standard(read)?, above_standard(write)?])
}
fn nonblocking(fd: &OwnedFd) -> Result<(), i32> {
    acquisition()?;
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(error());
    }
    Ok(())
}
fn inherited_standard(fd: i32) -> Result<Option<OwnedFd>, i32> {
    acquisition()?;
    let raw = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
    if raw >= 0 {
        return Ok(Some(unsafe { OwnedFd::from_raw_fd(raw) }));
    }
    if native_error() == libc::EBADF {
        Ok(None)
    } else {
        Err(error())
    }
}
fn standard_output(
    binding: Option<&OwnedFd>,
    capture: bool,
    inherited: i32,
) -> Result<(Option<OwnedFd>, Option<OwnedFd>), i32> {
    if let Some(binding) = binding {
        return Ok((Some(duplicate(binding.as_raw_fd())?), None));
    }
    if capture {
        let [read, write] = pipe()?;
        unsafe {
            super::set_capture_nonblocking(read.as_raw_fd(), inherited == 1)?;
        }
        Ok((Some(write), Some(read)))
    } else {
        Ok((inherited_standard(inherited)?, None))
    }
}
struct Prepared {
    argv: Vec<*const libc::c_char>,
    environment: Vec<CString>,
    envp: Vec<*const libc::c_char>,
    candidates: Vec<CString>,
    #[cfg(target_os = "linux")]
    image: Option<OwnedFd>,
    #[cfg(target_os = "linux")]
    bindings: Vec<(i32,OwnedFd)>,
}
impl Prepared {
    fn new(command: &Command) -> Result<Self, i32> {
        let mut variables: Vec<(OsString, OsString)> = if command.env_clear {
            Vec::new()
        } else {
            std::env::vars_os().collect()
        };
        for (key, value) in &command.env {
            let key = OsString::from(std::str::from_utf8(key.as_bytes()).map_err(|_| AL_INVALID)?);
            let value =
                OsString::from(std::str::from_utf8(value.as_bytes()).map_err(|_| AL_INVALID)?);
            if let Some(entry) = variables.iter_mut().find(|(name, _)| *name == key) {
                entry.1 = value;
            } else {
                variables.push((key, value));
            }
        }
        // Direct paths and retained images do not need PATH-search scratch.
        let search = if matches!(command.target, super::CommandTarget::Image(_))
            || command.target.direct_path()
        {
            Vec::new()
        } else {
            match variables
                .iter()
                .find(|(key, _)| key == "PATH")
                .map(|(_, value)| value.as_bytes().to_vec())
            {
                Some(value) => value,
                None => {
                    let size = unsafe { libc::confstr(libc::_CS_PATH, core::ptr::null_mut(), 0) };
                    if size == 0 {
                        return Err(error());
                    }
                    let mut bytes = vec![0u8; size];
                    if unsafe { libc::confstr(libc::_CS_PATH, bytes.as_mut_ptr().cast(), size) } != size
                    {
                        return Err(AL_INVALID);
                    }
                    bytes.pop();
                    bytes
                }
            }
        };
        let environment = variables
            .into_iter()
            .map(|(name, value)| {
                let mut bytes = name.as_bytes().to_vec();
                bytes.push(b'=');
                bytes.extend_from_slice(value.as_bytes());
                CString::new(bytes).map_err(|_| AL_INVALID)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut envp: Vec<_> = environment.iter().map(|entry| entry.as_ptr()).collect();
        envp.push(core::ptr::null());
        let mut argv: Vec<_> = command.argv.iter().map(|entry| entry.as_ptr()).collect();
        argv.push(core::ptr::null());
        let candidates = match &command.target {
            super::CommandTarget::Image(_) => Vec::new(),
            super::CommandTarget::Path(path) if command.target.direct_path() => vec![path.clone()],
            super::CommandTarget::Path(path) => search.split(|byte| *byte == b':').map(|prefix| {
                let mut bytes = prefix.to_vec();
                if !bytes.is_empty() { bytes.push(b'/'); }
                bytes.extend_from_slice(path.as_bytes());
                CString::new(bytes).map_err(|_| AL_INVALID)
            }).collect::<Result<Vec<_>,_>>()?,
        };
        #[cfg(target_os = "linux")]
        let minimum = command.inheritance.keys().next_back().copied().unwrap_or(2) + 1;
        #[cfg(target_os = "linux")]
        let image = match &command.target {
            super::CommandTarget::Path(_) => None,
            super::CommandTarget::Image(fd) => Some(super::process_verified::duplicate(fd, minimum)?),
        };
        #[cfg(target_os = "linux")]
        let bindings = super::process_verified::launch_bindings(command, minimum)?;
        Ok(Self {
            argv,
            environment,
            envp,
            candidates,
            #[cfg(target_os = "linux")]
            image,
            #[cfg(target_os = "linux")]
            bindings,
        })
    }
}

pub(crate) fn launch(
    command: &Command,
    capture: bool,
    force_group: bool,
) -> Result<Box<NativeChild>, i32> {
    launch_inner(command, capture, force_group, false)
}
#[cfg(target_os="linux")]
pub(crate) fn launch_scope_root(command: &Command) -> Result<Box<NativeChild>, i32> {
    launch_inner(command, true, false, true)
}
fn launch_inner(command: &Command, capture: bool, force_group: bool, scoped: bool) -> Result<Box<NativeChild>, i32> {
    if matches!(&command.target, super::CommandTarget::Path(path) if path.as_bytes().is_empty())
        || command
            .cwd
            .as_ref()
            .is_some_and(|cwd| cwd.as_bytes().is_empty())
    {
        return Err(AL_INVALID);
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if !shared_clone_supported() {
        return Err(io_error_to_status(&std::io::Error::from_raw_os_error(libc::ENOSYS)));
    }
    acquisition()?;
    let prepared = Prepared::new(command)?;
    #[cfg(target_os = "linux")]
    let reservation = creation();
    #[cfg(target_os = "macos")]
    let mut reservation = creation();
    if reservation.scope != scoped {
        return Err(AL_INVALID);
    }
    let mut disposition: libc::sigaction = unsafe { core::mem::zeroed() };
    if unsafe { libc::sigaction(libc::SIGCHLD, core::ptr::null(), &mut disposition) } < 0 {
        return Err(error());
    }
    if disposition.sa_sigaction == libc::SIG_IGN || disposition.sa_flags & libc::SA_NOCLDWAIT != 0 {
        return Err(AL_INVALID);
    }
    let minimum = command.inheritance.keys().next_back().copied().unwrap_or(2) + 1;
    let stage = |fd: OwnedFd| -> Result<OwnedFd,i32> {
        if fd.as_raw_fd() >= minimum { Ok(fd) } else { super::process_verified::duplicate(&fd, minimum) }
    };
    let stdin = if capture {
        acquisition()?;
        let raw = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
        if raw < 0 {
            return Err(error());
        }
        Some(above_standard(unsafe { OwnedFd::from_raw_fd(raw) })?)
    } else {
        inherited_standard(0)?
    };
    let (stdout, out_read) = standard_output(command.stdout_binding.as_ref(), capture, 1)?;
    let (stderr, err_read) = standard_output(command.stderr_binding.as_ref(), capture, 2)?;
    let stdin = stdin.map(&stage).transpose()?;
    let stdout = stdout.map(&stage).transpose()?;
    let stderr = stderr.map(&stage).transpose()?;
    let out_read = out_read.map(&stage).transpose()?;
    let err_read = err_read.map(&stage).transpose()?;
    let mut child = Box::new(NativeChild::new(0));
    child.stdout.fd = out_read;
    child.stderr.fd = err_read;
    let streams = [&stdin, &stdout, &stderr].map(|fd| fd.as_ref().map_or(-1, AsRawFd::as_raw_fd));
    let [error_read, error_write] = pipe()?;
    let error_read = stage(error_read)?;
    let error_write = stage(error_write)?;
    nonblocking(&error_read)?;
    #[cfg(target_os = "linux")]
    let stack = ChildStack::new()?;
    let mask = SignalMask::block()?;
    acquisition()?;
    // The CLONE_VFORK wait is unbounded (a child stopped before exec, a stalled
    // executable lookup), so it must not hold the process-wide creation lock.
    #[cfg(target_os = "linux")]
    let pending = PendingChild::reserve(reservation, scoped);
    #[cfg(target_os = "linux")]
    let pid = {
        let mut entry = Bootstrap {
            command,
            prepared: &prepared,
            streams: &streams,
            error_fd: error_write.as_raw_fd(),
            mask: &mask,
            force_group,
            scoped,
        };
        // Shared VM, private descriptor table, signal actions, cwd and credentials.
        // CLONE_VFORK suspends this thread until the child execs or exits, so the
        // borrowed launch state and child stack outlive every child access.
        unsafe {
            libc::clone(
                enter_bootstrap,
                stack.top(),
                libc::CLONE_VM | libc::CLONE_VFORK | libc::SIGCHLD | forced_clone_failure(),
                (&raw mut entry).cast(),
            )
        }
    };
    #[cfg(target_os = "macos")]
    let pid = unsafe {
        darwin::spawn(
            command,
            &prepared,
            &streams,
            error_write.as_raw_fd(),
            &mask,
            force_group,
        )?
    };
    if pid < 0 {
        return Err(error());
    }
    #[cfg(target_os = "linux")]
    drop(stack);
    child.started = std::time::Instant::now();
    child.pid = pid;
    #[cfg(target_os = "linux")]
    {
        child.tracked = pending.commit();
    }
    #[cfg(target_os = "macos")]
    if !scoped {
        reservation.children = reservation.children.checked_add(1)
            .unwrap_or_else(|| super::panic_abort("process owner count overflow"));
        child.tracked = true;
    }
    #[cfg(test)]
    if capture {
        super::CAPTURE_FORK_COUNT.with(|count| count.set(count.get() + 1));
        super::CAPTURE_LAST_PID.with(|last| last.set(child.pid));
        super::CAPTURE_LAST_FDS.with(|last| {
            last.set((
                child.stdout.fd.as_ref().map_or(-1, AsRawFd::as_raw_fd),
                child.stderr.fd.as_ref().map_or(-1, AsRawFd::as_raw_fd),
            ))
        });
    }
    drop(mask);
    #[cfg(target_os = "macos")]
    drop(reservation);
    drop(error_write);
    drop(stdin);
    drop(stdout);
    drop(stderr);
    // Keep marshalled string allocations alive until after the native launch call.
    debug_assert_eq!(prepared.envp.len(), prepared.environment.len() + 1);
    if let Err(failure) = acquisition()
        .and_then(|()| handshake(&mut child, error_read.as_raw_fd(), command.timeout_ns))
    {
        cleanup_failed(&mut child, force_group || command.new_session);
        return Err(failure);
    }
    child.new_session = command.new_session && unsafe { libc::getsid(child.pid) } == child.pid;
    #[cfg(target_os = "macos")]
    if command.timeout_ns == 0 {
        child.new_session = command.new_session;
    } // successful spawn committed SETSID

    if let Err(failure) = acquisition().and_then(|()| child.register_event()) {
        cleanup_failed(&mut child, force_group || command.new_session);
        return Err(failure);
    }
    Ok(child)
}
fn cleanup_failed(child: &mut NativeChild, group: bool) {
    child.stdout.fd.take();
    child.stderr.fd.take();
    if group && unsafe { libc::getpgid(child.pid) } == child.pid {
        unsafe {
            libc::kill(-child.pid, libc::SIGKILL);
        }
    }
    let _ = child.signal(i64::from(libc::SIGKILL), false);
    let _ = child.wait();
}
fn handshake(child: &mut NativeChild, fd: i32, timeout_ns: i64) -> Result<(), i32> {
    // The spawn handshake and the capture loop share one monotonic start-plus-budget clock
    // (`MonotonicTimeoutBudget` + `poll_timeout_ms`): the run's `timeout_ns` is charged from
    // `child.started`, every positive remainder rounds up to whole milliseconds, and the
    // post-syscall timeout-wins recheck keeps its order. Reusing the shared type rather than a
    // second local copy also means the test-only elapsed override governs the whole run, so a
    // virtualized-clock owner cannot be defeated by real fork/exec latency.
    let budget = super::MonotonicTimeoutBudget::from_positive_ns(timeout_ns).map(|mut budget| {
        budget.start = child.started;
        budget
    });
    let mut frame = [0u8; 9];
    let mut length = 0usize;
    loop {
        let timeout = match budget.as_ref() {
            Some(budget) => match budget.remaining().and_then(super::poll_timeout_ms) {
                Some(milliseconds) => milliseconds,
                None => return Err(AL_TIMEOUT),
            },
            None => -1,
        };
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
        if budget
            .as_ref()
            .is_some_and(super::MonotonicTimeoutBudget::is_exhausted)
        {
            return Err(AL_TIMEOUT);
        }
        if ready < 0 {
            if native_error() == libc::EINTR {
                continue;
            }
            return Err(error());
        }
        if ready == 0 {
            continue;
        }
        if poll.revents & libc::POLLNVAL != 0 {
            return Err(AL_INVALID);
        }
        let count = unsafe {
            libc::read(
                fd,
                frame[length..].as_mut_ptr().cast(),
                frame.len() - length,
            )
        };
        if count < 0 {
            if matches!(native_error(), libc::EINTR | libc::EAGAIN) {
                continue;
            }
            return Err(error());
        }
        if count == 0 {
            return decode_frame(&frame[..length]);
        }
        length += usize::try_from(count).map_err(|_| AL_INVALID)?;
        if length == frame.len() {
            return Err(AL_INVALID);
        }
    }
}
fn decode_frame(frame: &[u8]) -> Result<(), i32> {
    if frame.is_empty() {
        return Ok(());
    }
    if frame.len() != 8 || &frame[..4] != b"ALPE" {
        return Err(AL_INVALID);
    }
    let errno = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let errno = i32::try_from(errno)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(AL_INVALID)?;
    Err(io_error_to_status(&std::io::Error::from_raw_os_error(
        errno,
    )))
}

#[cfg(target_os = "linux")]
struct SignalMask {
    previous: u64,
}
#[cfg(target_os = "linux")]
impl SignalMask {
    fn block() -> Result<Self, i32> {
        let mut previous = 0u64;
        let mask = u64::MAX;
        if unsafe {
            libc::syscall(
                libc::SYS_rt_sigprocmask,
                libc::SIG_SETMASK,
                &mask,
                &mut previous,
                8usize,
            )
        } < 0
        {
            return Err(error());
        }
        Ok(Self { previous })
    }
    unsafe fn restore_native(&self) -> i32 {
        if unsafe {
            libc::syscall(
                libc::SYS_rt_sigprocmask,
                libc::SIG_SETMASK,
                &self.previous,
                core::ptr::null_mut::<u64>(),
                8usize,
            )
        } < 0
        {
            native_error()
        } else {
            0
        }
    }
}
#[cfg(target_os = "linux")]
impl Drop for SignalMask {
    fn drop(&mut self) {
        if unsafe { self.restore_native() } != 0 {
            super::panic_abort("cannot restore launch signal mask");
        }
    }
}
/// glibc before 2.24 (BZ #19957) made a CLONE_VM, non-CLONE_THREAD clone child store
/// -1 into the cached PID/TID of the thread control block it shares with the
/// launching thread. 2.24 removed that store and 2.25 removed the PID cache. Refuse
/// the shared-address-space launch on an older runtime glibc instead of corrupting it.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub(crate) fn shared_clone_supported() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering::Relaxed};
    static STATE: AtomicU8 = AtomicU8::new(0);
    match STATE.load(Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let version = unsafe { core::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };
            let supported = glibc_fixes_clone_vm_tcb(version.to_bytes());
            STATE.store(if supported { 1 } else { 2 }, Relaxed);
            supported
        }
    }
}
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn glibc_fixes_clone_vm_tcb(version: &[u8]) -> bool {
    let number = |part: Option<&[u8]>| -> Option<u32> {
        let digits = part?;
        let digits = &digits[..digits.iter().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len())];
        std::str::from_utf8(digits).ok()?.parse().ok()
    };
    let mut parts = version.split(|byte| *byte == b'.');
    match (number(parts.next()), number(parts.next())) {
        (Some(major), Some(minor)) => (major, minor) >= (2, 24),
        _ => false,
    }
}
/// A Linux launch admitted under the creation lock. Before the CLONE_VFORK wait it
/// converts the lock into a counted reservation and releases it: the reservation
/// keeps scope exclusion (a scope starts only with zero tracked children, and an
/// active scope already rejects ordinary admission), while reaping, scope checks
/// and signal subscriptions on other threads proceed. The child's inherited
/// signal actions are copied atomically by clone and inspected in the child, so
/// no subscription change needs to be excluded across the wait.
#[cfg(target_os = "linux")]
struct PendingChild(bool);
#[cfg(target_os = "linux")]
impl PendingChild {
    fn reserve(mut state: std::sync::MutexGuard<'static, CreationState>, scoped: bool) -> Self {
        if !scoped {
            state.children = state
                .children
                .checked_add(1)
                .unwrap_or_else(|| super::panic_abort("process owner count overflow"));
        }
        drop(state);
        Self(!scoped)
    }
    /// Transfers the counted reservation to the published child owner.
    fn commit(mut self) -> bool {
        core::mem::replace(&mut self.0, false)
    }
}
#[cfg(target_os = "linux")]
impl Drop for PendingChild {
    fn drop(&mut self) {
        if self.0 {
            let mut state = creation();
            state.children = state
                .children
                .checked_sub(1)
                .unwrap_or_else(|| super::panic_abort("process owner count underflow"));
        }
    }
}
#[cfg(all(test, target_os = "linux"))]
std::thread_local! {
    static FORCE_CLONE_FAILURE: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}
/// Test-only: CLONE_THREAD without CLONE_SIGHAND makes the real clone call fail with EINVAL.
#[cfg(target_os = "linux")]
#[inline]
fn forced_clone_failure() -> i32 {
    #[cfg(test)]
    if FORCE_CLONE_FAILURE.with(|value| value.replace(false)) {
        return libc::CLONE_THREAD;
    }
    0
}
#[cfg(all(test, target_os = "linux"))]
pub(crate) static PRE_EXEC_GATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Private stack for the shared-address-space launch child: a lowest guard page plus
/// fixed bootstrap storage (the /proc/self/fd fallback uses an 8 KiB stack buffer).
#[cfg(target_os = "linux")]
struct ChildStack {
    base: *mut libc::c_void,
    length: usize,
}
#[cfg(target_os = "linux")]
impl ChildStack {
    const USABLE: usize = 256 * 1024;
    fn new() -> Result<Self, i32> {
        acquisition()?;
        let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
            .ok()
            .filter(|page| page.is_power_of_two())
            .ok_or(AL_INVALID)?;
        let length = Self::USABLE + page;
        let base = unsafe {
            libc::mmap(
                core::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_STACK,
                -1,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return Err(error());
        }
        let stack = Self { base, length };
        if unsafe { libc::mprotect(base, page, libc::PROT_NONE) } != 0 {
            return Err(error());
        }
        Ok(stack)
    }
    fn top(&self) -> *mut libc::c_void {
        // mmap returns page-aligned memory, so the end is 16-byte aligned for every ABI.
        unsafe { self.base.cast::<u8>().add(self.length).cast() }
    }
}
#[cfg(target_os = "linux")]
impl Drop for ChildStack {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.base, self.length);
        }
    }
}
#[cfg(target_os = "linux")]
struct Bootstrap<'a> {
    command: &'a Command,
    prepared: &'a Prepared,
    streams: &'a [i32; 3],
    error_fd: i32,
    mask: &'a SignalMask,
    force_group: bool,
    scoped: bool,
}
/// Child entry on the private stack. The child shares the parent's memory and its
/// thread pointer, so libc wrappers write the suspended launching thread's errno slot;
/// CLONE_VFORK keeps that thread stopped until exec or exit, and it reads errno only
/// after its own later failing calls. The child never returns, unwinds, allocates,
/// runs Drop glue or takes a lock; every signal stays blocked until just before exec.
#[cfg(target_os = "linux")]
extern "C" fn enter_bootstrap(entry: *mut libc::c_void) -> libc::c_int {
    let entry = unsafe { &*entry.cast::<Bootstrap<'_>>() };
    unsafe {
        bootstrap(
            entry.command,
            entry.prepared,
            entry.streams,
            entry.error_fd,
            entry.mask,
            entry.force_group,
            entry.scoped,
        )
    }
}
#[cfg(target_os = "linux")]
unsafe fn reset_caught() -> i32 {
    // Linux x86_64/AArch64 UAPI: handler, flags, restorer, 64-bit kernel signal set.
    let default = [0u64; 4];
    for signal in 1..=64 {
        if signal == libc::SIGKILL || signal == libc::SIGSTOP {
            continue;
        }
        let mut previous = [0u64; 4];
        if unsafe {
            libc::syscall(
                libc::SYS_rt_sigaction,
                signal,
                core::ptr::null::<u64>(),
                previous.as_mut_ptr(),
                8usize,
            )
        } < 0
        {
            return native_error();
        }
        if previous[0] > 1
            && unsafe {
                libc::syscall(
                    libc::SYS_rt_sigaction,
                    signal,
                    default.as_ptr(),
                    core::ptr::null_mut::<u64>(),
                    8usize,
                )
            } < 0
        {
            return native_error();
        }
    }
    0
}
#[cfg(all(test, target_os = "linux"))]
pub(crate) static FORCE_FD_SCAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(target_os = "linux")]
unsafe fn close_unlisted(error_fd: i32) -> i32 {
    const CLOSE_RANGE_CLOEXEC: u32 = 4;
    #[cfg(test)]
    let force_scan = FORCE_FD_SCAN.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(test))]
    let force_scan = false;
    if !force_scan {
        if unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, CLOSE_RANGE_CLOEXEC) } == 0 { return 0; }
        if !matches!(native_error(), libc::ENOSYS | libc::EINVAL) { return native_error(); }
    }
    let fd = unsafe {
        libc::open(
            c"/proc/self/fd".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return native_error();
    }
    let mut bytes = [0u8; 8192];
    let mut failure = 0;
    'scan: loop {
        let count =
            unsafe { libc::syscall(libc::SYS_getdents64, fd, bytes.as_mut_ptr(), bytes.len()) };
        if count < 0 {
            if native_error() == libc::EINTR {
                continue;
            }
            failure = native_error();
            break;
        }
        if count == 0 {
            break;
        }
        let Ok(count) = usize::try_from(count) else {
            failure = libc::EIO;
            break;
        };
        if count > bytes.len() {
            failure = libc::EIO;
            break;
        }
        let mut offset = 0;
        while offset < count {
            if count - offset < 20 {
                failure = libc::EIO;
                break 'scan;
            }
            let length = usize::from(u16::from_ne_bytes([bytes[offset + 16], bytes[offset + 17]]));
            if length < 20 || length > count - offset {
                failure = libc::EIO;
                break 'scan;
            }
            let name = &bytes[offset + 19..offset + length];
            let Some(end) = name.iter().position(|byte| *byte == 0) else {
                failure = libc::EIO;
                break 'scan;
            };
            let name = &name[..end];
            if name != b"." && name != b".." {
                let mut number = 0i32;
                if name.is_empty() {
                    failure = libc::EIO;
                    break 'scan;
                }
                for byte in name {
                    if !byte.is_ascii_digit() {
                        failure = libc::EIO;
                        break 'scan;
                    }
                    let Some(next) = number
                        .checked_mul(10)
                        .and_then(|n| n.checked_add(i32::from(byte - b'0')))
                    else {
                        failure = libc::EIO;
                        break 'scan;
                    };
                    number = next;
                }
                if number >= 3 && number != fd && number != error_fd {
                    // Match CLOSE_RANGE_CLOEXEC: staged image/binding sources must
                    // survive until remapping/exec, while unlisted descriptors
                    // disappear at exec. Never close a still-needed source here.
                    let flags = unsafe { libc::fcntl(number, libc::F_GETFD) };
                    if flags < 0 || unsafe { libc::fcntl(number, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
                        failure = native_error();
                        break 'scan;
                    }
                }
            }
            offset += length;
        }
    }
    unsafe {
        libc::close(fd);
    }
    failure
}
unsafe fn report_and_exit(fd: i32, errno: i32) -> ! {
    let mut frame = [0u8; 8];
    frame[..4].copy_from_slice(b"ALPE");
    frame[4..].copy_from_slice(&u32::try_from(errno).unwrap_or(0).to_le_bytes());
    let mut done = 0;
    while done < frame.len() {
        let count = unsafe { libc::write(fd, frame[done..].as_ptr().cast(), frame.len() - done) };
        if count < 0 {
            if native_error() == libc::EINTR {
                continue;
            }
            break;
        }
        if count == 0 {
            break;
        }
        let Ok(count) = usize::try_from(count) else {
            break;
        };
        done += count;
    }
    unsafe { libc::_exit(127) }
}
#[cfg(target_os = "linux")]
unsafe fn bootstrap(
    command: &Command,
    prepared: &Prepared,
    streams: &[i32; 3],
    error_fd: i32,
    mask: &SignalMask,
    force_group: bool,
    scoped: bool,
) -> ! {
    // Test-only pre-exec stall: announce on the ready fd, then block on the gate fd.
    #[cfg(test)]
    {
        let gate = PRE_EXEC_GATE.load(std::sync::atomic::Ordering::SeqCst);
        if gate != 0 {
            let (ready, wait) = ((gate >> 32) as i32, (gate & 0xffff_ffff) as i32);
            let mut byte = 0u8;
            unsafe {
                libc::write(ready, (&raw const byte).cast(), 1);
                while libc::read(wait, (&raw mut byte).cast(), 1) < 0 && native_error() == libc::EINTR {}
            }
        }
    }
    if scoped && unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        unsafe { report_and_exit(error_fd, native_error()) }
    }
    let reset = unsafe { reset_caught() };
    if reset != 0 {
        unsafe { report_and_exit(error_fd, reset) }
    }
    if let Some(cwd) = &command.cwd
        && unsafe { libc::chdir(cwd.as_ptr()) } < 0
    {
        unsafe { report_and_exit(error_fd, native_error()) }
    }
    if command.new_session {
        if unsafe { libc::setsid() } < 0 {
            unsafe { report_and_exit(error_fd, native_error()) }
        }
    } else if force_group && unsafe { libc::setpgid(0, 0) } < 0 {
        unsafe { report_and_exit(error_fd, native_error()) }
    }
    for (source, destination) in streams.iter().zip(0..3) {
        if *source < 0 {
            unsafe {
                libc::close(destination);
            }
        } else if unsafe { libc::dup2(*source, destination) } < 0 {
            unsafe { report_and_exit(error_fd, native_error()) }
        }
    }
    let closed = unsafe { close_unlisted(error_fd) };
    if closed != 0 {
        unsafe { report_and_exit(error_fd, closed) }
    }
    for (destination, source) in &prepared.bindings {
        if unsafe { libc::dup2(source.as_raw_fd(), *destination) } < 0 {
            unsafe { report_and_exit(error_fd, native_error()) }
        }
    }
    let restored = unsafe { mask.restore_native() };
    if restored != 0 {
        unsafe { report_and_exit(error_fd, restored) }
    }
    if let Some(image) = &prepared.image {
        unsafe { libc::syscall(libc::SYS_execveat, image.as_raw_fd(), c"".as_ptr(), prepared.argv.as_ptr(), prepared.envp.as_ptr(), libc::AT_EMPTY_PATH); }
        unsafe { report_and_exit(error_fd, native_error()) }
    }
    let mut failure = libc::ENOENT;
    let mut denied = false;
    for candidate in &prepared.candidates {
        unsafe {
            libc::execve(
                candidate.as_ptr(),
                prepared.argv.as_ptr(),
                prepared.envp.as_ptr(),
            );
        }
        failure = native_error();
        if command.target.direct_path() {
            break;
        }
        match failure {
            libc::EACCES => denied = true,
            libc::ENOENT | libc::ENOTDIR | libc::ESTALE | libc::ENODEV | libc::ETIMEDOUT => {}
            _ => unsafe { report_and_exit(error_fd, failure) },
        }
    }
    unsafe { report_and_exit(error_fd, if denied { libc::EACCES } else { failure }) }
}

/// # Safety
/// `command` is an exclusively borrowed command allocation; enabled is a canonical source bool.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_command_new_session(command: *mut Command, enabled: u8) {
    if !valid_pointer(command) || enabled > 1 {
        super::panic_abort("invalid process session setter");
    }
    unsafe {
        (*command).new_session = enabled != 0;
    }
}
unsafe fn bind_output(command: *mut Command, writer: *const Writer, stderr: bool) -> i32 {
    if !valid_pointer(command) || !valid_pointer(writer) || !disjoint(command, writer) {
        return AL_INVALID;
    }
    let writer = unsafe { &*writer };
    if writer.sink != WriterSink::GenericFd || !writer.buf.is_empty() {
        return AL_INVALID;
    }
    let flags = unsafe { libc::fcntl(writer.fd, libc::F_GETFL) };
    if flags < 0 {
        return error();
    }
    if !matches!(flags & libc::O_ACCMODE, libc::O_WRONLY | libc::O_RDWR) {
        return AL_INVALID;
    }
    let mut metadata: libc::stat = unsafe { core::mem::zeroed() };
    if unsafe { libc::fstat(writer.fd, &mut metadata) } < 0 {
        return error();
    }
    if metadata.st_mode & libc::S_IFMT != libc::S_IFREG {
        return AL_INVALID;
    }
    let copied = match duplicate(writer.fd) {
        Ok(fd) => fd,
        Err(error) => return error,
    };
    let command = unsafe { &mut *command };
    if stderr {
        command.stderr_binding = Some(copied);
    } else {
        command.stdout_binding = Some(copied);
    }
    0
}
/// # Safety
/// Command and writer are live, disjoint native allocations, borrowed for this call only.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_command_stdout_to(
    command: *mut Command,
    writer: *const Writer,
) -> i32 {
    unsafe { bind_output(command, writer, false) }
}
/// # Safety
/// Same borrow requirements as command_stdout_to.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_command_stderr_to(
    command: *mut Command,
    writer: *const Writer,
) -> i32 {
    unsafe { bind_output(command, writer, true) }
}
/// # Safety
/// Command is borrowed; out is a disjoint writable handle slot. No source borrow is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_command_start(
    command: *const Command,
    out: *mut *mut NativeChild,
) -> i32 {
    if !valid_pointer(command) || !valid_pointer(out) || !disjoint(command, out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(core::ptr::null_mut());
    }
    let command = unsafe { &*command };
    if command.timeout_ns > 0 || command.max_capture_bytes.is_some() {
        return AL_INVALID;
    }
    match launch(command, true, false) {
        Ok(child) => {
            unsafe {
                out.write(Box::into_raw(child));
            }
            0
        }
        Err(error) => error,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn isolated(name: &str, marker: &str) {
        use std::os::unix::process::CommandExt;
        struct Guard(std::process::Child, bool);
        impl Drop for Guard {
            fn drop(&mut self) {
                if !self.1 {
                    return;
                }
                if let Ok(pid) = i32::try_from(self.0.id()) {
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                }
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut guard = Guard(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", name, "--nocapture"])
                .env(marker, "1")
                .process_group(0)
                .spawn()
                .unwrap(),
            true,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(status) = guard.0.try_wait().unwrap() {
                // Reaping ends signal authority, including on an unsuccessful test exit.
                guard.1 = false;
                assert!(status.success(), "isolated native owner failed: {status:?}");
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "isolated process owner timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    pub(crate) fn command(script: &str) -> Command {
        Command {
            target: super::super::CommandTarget::Path(CString::new("/bin/sh").unwrap()),
            inheritance: std::collections::BTreeMap::new(),
            argv: ["sh", "-c", script]
                .map(|s| CString::new(s).unwrap())
                .to_vec(),
            cwd: None,
            env: Vec::new(),
            env_clear: false,
            timeout_ns: 0,
            max_capture_bytes: None,
            new_session: false,
            stdout_binding: None,
            stderr_binding: None,
        }
    }
    #[test]
    fn prepared_target_search_and_environment() {
        for (target, path, expected) in [
            ("/bin/sh", "a::/b", vec!["/bin/sh"]),
            ("./tool", "a::/b", vec!["./tool"]),
            ("sh", "a::/b", vec!["a/sh", "sh", "/b/sh"]),
            ("sh", "", vec!["sh"]),
        ] {
            let mut configuration = command("exit 0");
            configuration.target = super::super::CommandTarget::Path(CString::new(target).unwrap());
            configuration.env_clear = true;
            configuration.env.push((CString::new("PATH").unwrap(), CString::new(path).unwrap()));
            let prepared = Prepared::new(&configuration).unwrap();
            assert_eq!(
                prepared.candidates.iter().map(|candidate| candidate.to_str().unwrap()).collect::<Vec<_>>(),
                expected,
            );
            assert_eq!(prepared.environment[0].as_bytes(), format!("PATH={path}").as_bytes());
            assert_eq!(prepared.argv.len(), configuration.argv.len() + 1);
            assert_eq!(prepared.envp.len(), 2);
            assert_eq!(prepared.argv.last().copied(), Some(core::ptr::null()));
            assert_eq!(prepared.envp.last().copied(), Some(core::ptr::null()));
        }
        // An absent PATH uses the OS default without installing it in the child environment.
        let mut configuration = command("exit 0");
        configuration.env_clear = true;
        configuration.target = super::super::CommandTarget::Path(CString::new("sh").unwrap());
        let prepared = Prepared::new(&configuration).unwrap();
        let size = unsafe { libc::confstr(libc::_CS_PATH, core::ptr::null_mut(), 0) };
        assert!(size > 0);
        let mut default = vec![0_u8; size];
        assert_eq!(unsafe { libc::confstr(libc::_CS_PATH, default.as_mut_ptr().cast(), size) }, size);
        assert_eq!(default.pop(), Some(0));
        let expected: Vec<Vec<u8>> = default.split(|byte| *byte == b':').map(|prefix| {
            let mut candidate = prefix.to_vec();
            if !candidate.is_empty() { candidate.push(b'/'); }
            candidate.extend_from_slice(b"sh");
            candidate
        }).collect();
        assert_eq!(prepared.candidates.iter().map(|candidate| candidate.as_bytes()).collect::<Vec<_>>(), expected);
        assert!(prepared.environment.is_empty());
        // Bypassing search does not bypass validation of the supplied final environment.
        for target in ["/bin/sh", "sh"] {
            for invalid_key in [false, true] {
                let mut configuration = command("exit 0");
                configuration.env_clear = true;
                configuration.target = super::super::CommandTarget::Path(CString::new(target).unwrap());
                let invalid = CString::new(vec![0xff_u8]).unwrap();
                configuration.env.push(if invalid_key {
                    (invalid, CString::new("value").unwrap())
                } else {
                    (CString::new("PATH").unwrap(), invalid)
                });
                assert!(matches!(Prepared::new(&configuration), Err(AL_INVALID)));
            }
        }
    }
    #[cfg(feature = "alloc-count")]
    #[test]
    fn prepared_path_allocation_probe() {
        let before = super::super::global_alloc_count();
        std::hint::black_box(vec![17_u8; 64]);
        assert!(super::super::global_alloc_count() > before, "allocator probe is active");
        for image in [false, true] {
            if image && !cfg!(target_os = "linux") { continue; }
            let mut configuration = command("exit 0");
            configuration.env_clear = true;
            let path = "a".repeat(4096);
            configuration.env.push((CString::new("PATH").unwrap(), CString::new(path.as_str()).unwrap()));
            if image {
                configuration.target = super::super::CommandTarget::Image(
                    std::fs::File::open("/dev/null").unwrap().into(),
                );
            }
            let before = super::super::global_alloc_count();
            let prepared = std::hint::black_box(Prepared::new(&configuration).unwrap());
            let allocations = super::super::global_alloc_count() - before;
            assert_eq!(prepared.environment[0].as_bytes(), format!("PATH={path}").as_bytes());
            assert_eq!(prepared.argv.last().copied(), Some(core::ptr::null()));
            assert_eq!(prepared.envp.last().copied(), Some(core::ptr::null()));
            assert_eq!(prepared.candidates.len(), if image { 0 } else { 1 });
            assert!(allocations <= if image { 11 } else { 13 }, "unused PATH copy: image={image}, allocations={allocations}");
            println!("prepared image={image}: {allocations} Rust allocation/reallocation calls");
        }
    }
    #[test]
    fn closed_standard_streams_remain_closed() {
        const NAME: &str = "process_launch::tests::closed_standard_streams_remain_closed";
        if std::env::var_os("ALIGN_R65_CLOSED_STDIO").is_some() {
            unsafe {
                libc::close(0);
                libc::close(1);
                libc::close(2);
            }
            for (capture, timeout) in [(false,0),(true,0),(false,1_000_000_000),(true,1_000_000_000)] {
                let mut configuration = command("exit 7");
                configuration.timeout_ns = timeout;
                let mut child = launch(&configuration, capture, false).unwrap();
                child.stdout.fd.take();
                child.stderr.fd.take();
                assert_eq!(child.wait().unwrap().termination.value, 7);
            }
            std::process::exit(0);
        }
        isolated(NAME, "ALIGN_R65_CLOSED_STDIO");
    }
    #[test]
    fn partial_launch_acquisitions_leave_no_child_or_descriptor() {
        const NAME: &str =
            "process_launch::tests::partial_launch_acquisitions_leave_no_child_or_descriptor";
        const MARKER: &str = "ALIGN_R65_PARTIAL_LAUNCH";
        if std::env::var_os(MARKER).is_none() {
            isolated(NAME, MARKER);
            return;
        }
        let descriptor_count = || {
            std::fs::read_dir("/dev/fd")
                .unwrap()
                .map(|entry| entry.unwrap())
                .count()
        };
        let baseline = descriptor_count();
        for (capture, timeout) in [(false, 0), (true, 0), (true, 1_000_000_000)] {
            let mut completed = false;
            for phase in 1..64 {
                FAILURE_PHASE.with(|value| value.set(phase));
                OBSERVED_PHASE.with(|value| value.set(0));
                let mut configuration = command("exit 0");
                configuration.timeout_ns = timeout;
                let launched = launch(&configuration, capture, false);
                FAILURE_PHASE.with(|value| value.set(0));
                match launched {
                    Ok(mut child) => {
                        child.stdout.fd.take();
                        child.stderr.fd.take();
                        assert_eq!(child.wait().unwrap().termination.value, 0);
                        completed = true;
                    }
                    Err(error) => assert_eq!(
                        error,
                        io_error_to_status(&std::io::Error::from_raw_os_error(libc::EIO))
                    ),
                }
                assert_eq!(creation().children, 0, "phase {phase}");
                assert_eq!(descriptor_count(), baseline, "phase {phase}");
                assert_eq!(
                    unsafe { libc::waitpid(-1, core::ptr::null_mut(), libc::WNOHANG) },
                    -1
                );
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::ECHILD)
                );
                if completed {
                    break;
                }
            }
            assert!(completed, "all acquisition phases must be exercised");
        }
    }
    #[test]
    fn empty_cwd_rejects_before_any_launch_acquisition() {
        let mut configuration = command("exit 0");
        configuration.cwd = Some(CString::new("").unwrap());
        for capture in [false, true] {
            OBSERVED_PHASE.with(|value| value.set(0));
            assert!(matches!(
                launch(&configuration, capture, false),
                Err(AL_INVALID)
            ));
            assert_eq!(OBSERVED_PHASE.with(|value| value.get()), 0);
        }
    }
    #[test]
    fn launch_error_frame_golden() {
        assert_eq!(decode_frame(&[]), Ok(()));
        assert_eq!(
            decode_frame(&[65, 76, 80, 69, 2, 0, 0, 0]),
            Err(super::super::AL_NOT_FOUND)
        );
        for bytes in [
            &[65, 76, 80, 69, 2, 0, 0][..],
            &[65, 76, 80, 69, 2, 0, 0, 0, 0],
            &[65, 76, 80, 69, 0, 0, 0, 0],
            &[65, 76, 80, 69, 0, 0, 0, 128],
            &[66, 76, 80, 69, 2, 0, 0, 0],
        ] {
            assert_eq!(decode_frame(bytes), Err(AL_INVALID));
        }
    }
    #[test]
    fn live_capture_and_pending_wait() {
        let mut command = command("printf x; exec sleep 30");
        command.new_session = true;
        let mut child = launch(&command, true, false).unwrap();
        assert!(child.new_session);
        assert_eq!(child.wait(), Err(AL_INVALID));
        let mut observed = [0u8; 3];
        assert_eq!(
            unsafe {
                super::super::process_live::align_rt_child_poll(
                    &mut *child,
                    1,
                    1_000_000_000,
                    observed.as_mut_ptr().cast(),
                )
            },
            0
        );
        assert_eq!(observed[0], 1);
        let mut bytes = [0x55; 8];
        assert_eq!(child.stdout.read(&mut bytes).unwrap(), Some(1));
        assert_eq!(bytes, [b'x', 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55]);
        assert_eq!(child.stdout.read(&mut []).unwrap_err(), AL_INVALID);
        assert_eq!(child.stdout.read(&mut bytes).unwrap(), None);
        child.signal(i64::from(libc::SIGKILL), true).unwrap();
        assert_eq!(
            unsafe {
                super::super::process_live::align_rt_child_poll(
                    &mut *child,
                    4,
                    1_000_000_000,
                    observed.as_mut_ptr().cast(),
                )
            },
            0
        );
        assert_eq!(observed[2], 1);
        assert_eq!(
            child.wait().unwrap().termination.value,
            i64::from(libc::SIGKILL)
        );
        // This fixture has one process: exec preserves the writer PID. Observe
        // pipe EOF independently of the terminal-status notification.
        assert_eq!(unsafe { super::super::process_live::align_rt_child_poll(
            &mut *child, 1, 1_000_000_000, observed.as_mut_ptr().cast(),
        ) }, 0);
        assert_eq!(observed[0], 1);
        assert_eq!(child.stdout.read(&mut bytes).unwrap(), Some(0));
        assert_eq!(child.stdout.read(&mut bytes).unwrap(), Some(0));
        assert_eq!(child.signal(0, true), Err(AL_INVALID));
    }
    #[test]
    fn native_launch_error_and_final_environment() {
        let mut command = command("exit 127");
        let mut child = launch(&command, false, false).unwrap();
        assert_eq!(child.wait().unwrap().termination.value, 127);
        command.target = super::super::CommandTarget::Path(CString::new("/nonexistent/align-r65-executable").unwrap());
        assert!(matches!(
            launch(&command, false, false),
            Err(super::super::AL_NOT_FOUND)
        ));
        command.target = super::super::CommandTarget::Path(CString::new("sh").unwrap());
        command.env_clear = true;
        command.env.push((
            CString::new("PATH").unwrap(),
            CString::new("/nonexistent").unwrap(),
        ));
        assert!(matches!(
            launch(&command, false, false),
            Err(super::super::AL_NOT_FOUND)
        ));
        command.env.push((
            CString::new("PATH").unwrap(),
            CString::new("/bin:/usr/bin").unwrap(),
        ));
        assert_eq!(
            launch(&command, false, false)
                .unwrap()
                .wait()
                .unwrap()
                .termination
                .value,
            127
        );
    }
    #[test]
    fn child_setup_and_exec_errno_cross_the_error_frame() {
        use std::os::unix::fs::PermissionsExt;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        // Exclusively created, removed by Drop on every path.
        let directory = crate::tests::FileFixtureDir::new("launch-errno");
        let create = |name: &str, mode: u32, bytes: &[u8]| {
            let path = directory.0.join(name);
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&path).unwrap();
            file.write_all(bytes).unwrap();
            // mode() is filtered by the umask; set the exact bits.
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        };
        let plain = create("plain", 0o644, b"not executable");
        let garbage = create("garbage", 0o755, &[0x7f, b'E', b'L', b'F', 0, 0, 0, 0]);
        let code = |errno: i32| io_error_to_status(&std::io::Error::from_raw_os_error(errno));
        let path = |path: &std::path::Path| CString::new(path.as_os_str().as_bytes()).unwrap();
        for capture in [false, true] {
            for timeout in [0, 5_000_000_000] {
                if capture && timeout == 0 { continue; }
                let mut configuration = command("exit 0");
                configuration.timeout_ns = timeout;
                // exec errors: EACCES, ENOEXEC (an exact errno outside the named categories).
                configuration.target = super::super::CommandTarget::Path(path(&plain));
                assert_eq!(launch(&configuration, capture, false).err(), Some(code(libc::EACCES)));
                configuration.target = super::super::CommandTarget::Path(path(&garbage));
                assert_eq!(launch(&configuration, capture, false).err(), Some(code(libc::ENOEXEC)));
                // A setup error before exec: chdir into a regular file.
                configuration.target = super::super::CommandTarget::Path(CString::new("/bin/sh").unwrap());
                configuration.cwd = Some(path(&plain));
                assert_eq!(launch(&configuration, capture, true).err(), Some(code(libc::ENOTDIR)));
            }
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn child_signal_state_and_group_placement() {
        const NAME: &str = "process_launch::tests::child_signal_state_and_group_placement";
        const MARKER: &str = "ALIGN_R154_CHILD_SIGNAL_STATE";
        if std::env::var_os(MARKER).is_none() {
            isolated(NAME, MARKER);
            return;
        }
        extern "C" fn caught(_: i32) {}
        fn field(status: &str, name: &str) -> u64 {
            let line = status.lines().find(|line| line.starts_with(name)).unwrap();
            u64::from_str_radix(line[name.len()..].trim(), 16).unwrap()
        }
        unsafe {
            let mut blocked: libc::sigset_t = core::mem::zeroed();
            libc::sigemptyset(&mut blocked);
            libc::sigaddset(&mut blocked, libc::SIGUSR1);
            assert_eq!(libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, core::ptr::null_mut()), 0);
            let handler: extern "C" fn(i32) = caught;
            assert_ne!(libc::signal(libc::SIGUSR2, handler as libc::sighandler_t), libc::SIG_ERR);
            assert_ne!(libc::signal(libc::SIGHUP, libc::SIG_IGN), libc::SIG_ERR);
        }
        let parent = std::fs::read_to_string("/proc/thread-self/status").unwrap();
        let directory = crate::tests::FileFixtureDir::new("launch-signal");
        let parent_cwd = std::env::current_dir().unwrap();
        let parent_group = unsafe { (libc::getpgrp(), libc::getsid(0)) };
        let stdout_identity = || {
            let mut metadata: libc::stat = unsafe { core::mem::zeroed() };
            assert_eq!(unsafe { libc::fstat(1, &mut metadata) }, 0);
            (metadata.st_dev, metadata.st_ino)
        };
        let parent_stdout = stdout_identity();
        for (index, (force_group, new_session)) in [(false, false), (true, false), (false, true)].into_iter().enumerate() {
            let output = directory.0.join(format!("status-{index}"));
            let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&output).unwrap();
            let mut configuration = command("exit 0");
            configuration.target = super::super::CommandTarget::Path(CString::new("/bin/cat").unwrap());
            configuration.argv = ["cat", "/proc/self/status", "/proc/self/stat"]
                .map(|s| CString::new(s).unwrap())
                .to_vec();
            configuration.new_session = new_session;
            configuration.cwd = Some(CString::new("/").unwrap());
            configuration.stdout_binding = Some(file.into());
            let mut child = launch(&configuration, false, force_group).unwrap();
            let pid = child.pid;
            assert_eq!(child.wait().unwrap().termination.value, 0);
            let observed = std::fs::read_to_string(&output).unwrap();
            // The executed image sees the caller's exact thread mask, inherited ignored
            // dispositions, and no caught handler.
            assert_eq!(field(&observed, "SigBlk:"), field(&parent, "SigBlk:"));
            assert_ne!(field(&parent, "SigBlk:") & (1 << (libc::SIGUSR1 - 1)), 0);
            assert_eq!(field(&observed, "SigIgn:"), field(&parent, "SigIgn:"));
            assert_ne!(field(&parent, "SigIgn:") & (1 << (libc::SIGHUP - 1)), 0);
            assert_ne!(field(&parent, "SigCgt:") & (1 << (libc::SIGUSR2 - 1)), 0);
            assert_eq!(field(&observed, "SigCgt:"), 0);
            let stat = &observed[observed.rfind(')').unwrap() + 2..];
            let fields: Vec<i32> = stat.split_whitespace().skip(1).take(3).map(|f| f.parse().unwrap()).collect();
            let (group, session) = (fields[1], fields[2]);
            if new_session {
                assert_eq!((group, session), (pid, pid));
            } else if force_group {
                assert_eq!((group, session), (pid, unsafe { libc::getsid(0) }));
            } else {
                assert_eq!((group, session), (unsafe { libc::getpgrp() }, unsafe { libc::getsid(0) }));
            }
            // The launching thread's own mask is restored after launch.
            let after = std::fs::read_to_string("/proc/thread-self/status").unwrap();
            assert_eq!(field(&after, "SigBlk:"), field(&parent, "SigBlk:"));
            // The child's disposition reset, chdir and stdout remap stayed private: the
            // handler checks detect CLONE_SIGHAND, cwd detects CLONE_FS and stdout identity
            // detects CLONE_FILES. The group/session check detects only CLONE_THREAD, which
            // would make setsid/setpgid act on this process.
            let mut action: libc::sigaction = unsafe { core::mem::zeroed() };
            assert_eq!(unsafe { libc::sigaction(libc::SIGUSR2, core::ptr::null(), &mut action) }, 0);
            assert_eq!(action.sa_sigaction, caught as extern "C" fn(i32) as libc::sighandler_t);
            assert_eq!(unsafe { libc::sigaction(libc::SIGHUP, core::ptr::null(), &mut action) }, 0);
            assert_eq!(action.sa_sigaction, libc::SIG_IGN);
            assert_eq!(field(&after, "SigCgt:"), field(&parent, "SigCgt:"));
            assert_eq!(std::env::current_dir().unwrap(), parent_cwd);
            assert_eq!(unsafe { (libc::getpgrp(), libc::getsid(0)) }, parent_group);
            assert_eq!(stdout_identity(), parent_stdout);
        }
        // exit() skips Drop; release the fixture first.
        drop(directory);
        std::process::exit(0);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn vfork_wait_holds_no_creation_lock() {
        const NAME: &str = "process_launch::tests::vfork_wait_holds_no_creation_lock";
        const MARKER: &str = "ALIGN_R154_VFORK_WAIT_LOCK";
        if std::env::var_os(MARKER).is_none() {
            isolated(NAME, MARKER);
            return;
        }
        let pipe = || {
            let mut fds = [-1; 2];
            assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
            fds.map(|fd| unsafe { OwnedFd::from_raw_fd(fd) })
        };
        let mut other = launch(&command("exit 3"), false, false).unwrap();
        let [ready_read, ready_write] = pipe();
        let [gate_read, gate_write] = pipe();
        let gate = (u64::try_from(ready_write.as_raw_fd()).unwrap() << 32)
            | u64::try_from(gate_read.as_raw_fd()).unwrap();
        PRE_EXEC_GATE.store(gate, std::sync::atomic::Ordering::SeqCst);
        let launcher = std::thread::spawn(|| {
            let mut child = launch(&command("exit 0"), false, false).unwrap();
            child.wait().unwrap().termination.value
        });
        let mut byte = 0u8;
        assert_eq!(unsafe { libc::read(ready_read.as_raw_fd(), (&raw mut byte).cast(), 1) }, 1);
        PRE_EXEC_GATE.store(0, std::sync::atomic::Ordering::SeqCst);
        // The launcher is suspended in clone(CLONE_VFORK) behind a child blocked before exec.
        assert!(CREATION.try_lock().is_ok(), "the vfork wait holds the creation lock");
        // Another thread can reap (release_tracking) while the pending launch stays counted,
        // which keeps a process scope from starting.
        assert_eq!(other.wait().unwrap().termination.value, 3);
        assert_eq!(creation().children, 1);
        assert_eq!(unsafe { libc::write(gate_write.as_raw_fd(), (&raw const byte).cast(), 1) }, 1);
        assert_eq!(launcher.join().unwrap(), 0);
        assert_eq!(creation().children, 0);
        std::process::exit(0);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn failed_clone_returns_pending_reservation() {
        const NAME: &str = "process_launch::tests::failed_clone_returns_pending_reservation";
        const MARKER: &str = "ALIGN_R154_FAILED_CLONE";
        if std::env::var_os(MARKER).is_none() {
            isolated(NAME, MARKER);
            return;
        }
        let descriptors = || std::fs::read_dir("/proc/self/fd").unwrap().count();
        let baseline = descriptors();
        for (capture, timeout) in [(false, 0), (true, 0), (true, 1_000_000_000)] {
            let mut configuration = command("exit 0");
            configuration.timeout_ns = timeout;
            FORCE_CLONE_FAILURE.with(|value| value.set(true));
            assert_eq!(launch(&configuration, capture, false).err(), Some(AL_INVALID));
            assert!(!FORCE_CLONE_FAILURE.with(core::cell::Cell::get), "clone was reached");
            assert_eq!(creation().children, 0);
            assert_eq!(descriptors(), baseline);
            assert_eq!(unsafe { libc::waitpid(-1, core::ptr::null_mut(), libc::WNOHANG) }, -1);
            let mut child = launch(&configuration, capture, false).unwrap();
            assert_eq!(creation().children, 1);
            child.stdout.fd.take();
            child.stderr.fd.take();
            assert_eq!(child.wait().unwrap().termination.value, 0);
            assert_eq!(creation().children, 0);
        }
        std::process::exit(0);
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn glibc_clone_vm_floor() {
        for (version, supported) in [
            (&b"2.17"[..], false), (b"2.23", false), (b"2.23.90", false), (b"2.24", true),
            (b"2.25", true), (b"2.39", true), (b"2.40.9000", true), (b"3.0", true),
            (b"1.99", false), (b"", false), (b"2", false), (b"x.y", false),
        ] {
            assert_eq!(glibc_fixes_clone_vm_tcb(version), supported, "{version:?}");
        }
        assert!(shared_clone_supported());
    }
    // Manual launch-latency probe (plan 49, Linux shared-address-space launch):
    // ALIGN_LAUNCH_PROBE_MIB=4096 scripts/cargo.sh test -p align_runtime --lib \
    //   process_launch::tests::launch_resident_size_probe -- --ignored --nocapture
    // Not a correctness gate; it prints start+reap latency of /bin/true with that much
    // touched (and, when RLIMIT_MEMLOCK permits, locked) private anonymous memory.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "manual launch-latency probe"]
    fn launch_resident_size_probe() {
        let mib: usize = std::env::var("ALIGN_LAUNCH_PROBE_MIB").ok().map_or(4096, |v| v.parse().unwrap());
        let length = mib << 20;
        let mut locked = false;
        let region = if length == 0 { core::ptr::null_mut() } else {
            let region = unsafe { libc::mmap(core::ptr::null_mut(), length, libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0) };
            assert_ne!(region, libc::MAP_FAILED);
            unsafe { libc::madvise(region, length, libc::MADV_NOHUGEPAGE); }
            let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).unwrap();
            for offset in (0..length).step_by(page) {
                unsafe { region.cast::<u8>().add(offset).write_volatile(1); }
            }
            locked = unsafe { libc::mlock(region, length) } == 0;
            region
        };
        let mut configuration = command("exit 0");
        configuration.target = super::super::CommandTarget::Path(CString::new("/bin/true").unwrap());
        configuration.argv = vec![CString::new("true").unwrap()];
        let mut samples = Vec::new();
        for _ in 0..21 {
            let start = std::time::Instant::now();
            let mut child = launch(&configuration, false, false).unwrap();
            assert_eq!(child.wait().unwrap().termination.value, 0);
            samples.push(start.elapsed().as_secs_f64() * 1e3);
        }
        samples.sort_by(f64::total_cmp);
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let rss = status.lines().find(|l| l.starts_with("VmRSS:")).unwrap_or("").to_string();
        println!("launch probe: touched_mib={mib} mlocked={locked} {rss} start+reap ms min={:.3} median={:.3} max={:.3}",
            samples[0], samples[samples.len() / 2], samples[samples.len() - 1]);
        if length != 0 {
            unsafe { libc::munmap(region, length); }
        }
    }
    #[test]
    fn cached_readiness_does_not_hide_another_stream() {
        let mut child = launch(&command("printf x >&2"), true, false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while child.status().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let mut byte = [0];
        let mut readiness = [0u8; 3];
        // Terminal status is not a pipe-lifetime witness. Cache stdout EOF only
        // after its own readiness; another concurrent native launch can retain a
        // transient pre-exec duplicate even after this child has exited.
        assert_eq!(unsafe { super::super::process_live::align_rt_child_poll(
            &mut *child, 1, 5_000_000_000, readiness.as_mut_ptr().cast(),
        ) }, 0);
        assert_eq!(readiness, [1,0,0]);
        assert_eq!(child.stdout.read(&mut byte).unwrap(), Some(0));
        assert_eq!(
            unsafe {
                super::super::process_live::align_rt_child_poll(
                    &mut *child,
                    7,
                    0,
                    readiness.as_mut_ptr().cast(),
                )
            },
            0
        );
        assert_eq!(readiness, [1, 1, 1]);
        assert_eq!(child.stderr.read(&mut byte).unwrap(), Some(1));
        assert_eq!(byte, [b'x']);
        let result = child.wait().unwrap();
        assert_eq!(result.termination.value, 0);
        assert_eq!(unsafe { super::super::process_live::align_rt_child_poll(
            &mut *child, 2, 5_000_000_000, readiness.as_mut_ptr().cast(),
        ) }, 0);
        assert_eq!(readiness, [0,1,0]);
        assert_eq!(child.stderr.read(&mut byte).unwrap(), Some(0));
        for (interest, timeout) in [(0, 0), (8, 0), (1, -1)] {
            assert_eq!(
                unsafe {
                    super::super::process_live::align_rt_child_poll(
                        &mut *child,
                        interest,
                        timeout,
                        readiness.as_mut_ptr().cast(),
                    )
                },
                AL_INVALID
            );
        }
    }
    #[test]
    fn observed_session_retains_group_authority_until_reap() {
        let mut configuration = command("exit 143");
        configuration.new_session = true;
        let mut child = launch(&configuration, false, false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while child.status().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        // The unreaped leader pins this group identity. Compare with the native
        // observation: zombie-only groups differ across kernels (including EPERM).
        let native = if unsafe { libc::kill(-child.pid, 0) } == 0 {
            Ok(())
        } else {
            Err(io_error_to_status(&std::io::Error::last_os_error()))
        };
        assert_eq!(child.signal(0, true), native);
        for signal in [-1, super::super::MAX_SIGNAL + 1, i64::MAX] {
            assert_eq!(child.signal(signal, true), Err(AL_INVALID));
        }
        assert_eq!(child.wait().unwrap().termination.value, 143);
        assert_eq!(child.signal(0, true), Err(AL_INVALID));
        assert_eq!(child.signal(0, false), Err(AL_INVALID));
    }
    #[test]
    fn unlisted_descriptor_closes_on_each_launch_path() {
        let null = std::fs::File::open("/dev/null").unwrap();
        // Deliberately non-CLOEXEC foreign fd. Every launch must close it without altering parent flags.
        let raw = unsafe { libc::fcntl(null.as_raw_fd(), libc::F_DUPFD, 200) };
        assert!(raw >= 200);
        let foreign = unsafe { OwnedFd::from_raw_fd(raw) };
        let script = format!("test ! -e /dev/fd/{raw}");
        for (capture, timeout) in [(false, 0), (true, 0), (true, 1_000_000_000)] {
            let mut command = command(&script);
            command.timeout_ns = timeout;
            let mut child = launch(&command, capture, false).unwrap();
            child.stdout.fd.take();
            child.stderr.fd.take();
            assert_eq!(child.wait().unwrap().termination.value, 0);
        }
        assert_eq!(
            unsafe { libc::fcntl(foreign.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
    }
}
