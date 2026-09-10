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
        let search = variables
            .iter()
            .find(|(key, _)| key == "PATH")
            .map(|(_, value)| value.as_bytes().to_vec());
        let search = match search {
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
        let candidates = if command.cmd.as_bytes().contains(&b'/') {
            vec![command.cmd.clone()]
        } else {
            search
                .split(|byte| *byte == b':')
                .map(|prefix| {
                    let mut bytes = prefix.to_vec();
                    if !bytes.is_empty() {
                        bytes.push(b'/');
                    }
                    bytes.extend_from_slice(command.cmd.as_bytes());
                    CString::new(bytes).map_err(|_| AL_INVALID)
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Self {
            argv,
            environment,
            envp,
            candidates,
        })
    }
}

pub(crate) fn launch(
    command: &Command,
    capture: bool,
    force_group: bool,
) -> Result<Box<NativeChild>, i32> {
    if command.cmd.as_bytes().is_empty()
        || command
            .cwd
            .as_ref()
            .is_some_and(|cwd| cwd.as_bytes().is_empty())
    {
        return Err(AL_INVALID);
    }
    acquisition()?;
    let prepared = Prepared::new(command)?;
    let mut reservation = creation();
    if reservation.scope {
        return Err(AL_INVALID);
    }
    let mut disposition: libc::sigaction = unsafe { core::mem::zeroed() };
    if unsafe { libc::sigaction(libc::SIGCHLD, core::ptr::null(), &mut disposition) } < 0 {
        return Err(error());
    }
    if disposition.sa_sigaction == libc::SIG_IGN || disposition.sa_flags & libc::SA_NOCLDWAIT != 0 {
        return Err(AL_INVALID);
    }
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
    let mut child = Box::new(NativeChild::new(0));
    child.stdout.fd = out_read;
    child.stderr.fd = err_read;
    let streams = [&stdin, &stdout, &stderr].map(|fd| fd.as_ref().map_or(-1, AsRawFd::as_raw_fd));
    let [error_read, error_write] = pipe()?;
    nonblocking(&error_read)?;
    let mask = SignalMask::block()?;
    acquisition()?;
    #[cfg(target_os = "linux")]
    let pid = unsafe {
        libc::syscall(
            libc::SYS_clone,
            libc::SIGCHLD,
            0usize,
            0usize,
            0usize,
            0usize,
        )
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
    if pid == 0 {
        // Nothing below can unwind or return to parent-side owners or their locks.
        unsafe {
            bootstrap(
                command,
                &prepared,
                &streams,
                error_write.as_raw_fd(),
                &mask,
                force_group,
            )
        }
    }
    child.started = std::time::Instant::now();
    child.pid =
        i32::try_from(pid).unwrap_or_else(|_| super::panic_abort("invalid native child PID"));
    reservation.children = reservation
        .children
        .checked_add(1)
        .unwrap_or_else(|| super::panic_abort("process owner count overflow"));
    child.tracked = true;
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
    let mut frame = [0u8; 9];
    let mut length = 0usize;
    loop {
        let budget = (timeout_ns > 0)
            .then(|| std::time::Duration::from_nanos(u64::try_from(timeout_ns).unwrap_or(0)));
        let remaining = budget.map(|budget| budget.saturating_sub(child.started.elapsed()));
        if remaining.is_some_and(|duration| duration.is_zero()) {
            return Err(AL_TIMEOUT);
        }
        let timeout = remaining
            .map(|duration| {
                i32::try_from(duration.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX)
            })
            .unwrap_or(-1);
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
        if budget.is_some_and(|budget| child.started.elapsed() >= budget) {
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
#[cfg(target_os = "linux")]
unsafe fn close_unlisted(error_fd: i32) -> i32 {
    const CLOSE_RANGE_CLOEXEC: u32 = 4;
    if unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, CLOSE_RANGE_CLOEXEC) } == 0 {
        return 0;
    }
    if !matches!(native_error(), libc::ENOSYS | libc::EINVAL) {
        return native_error();
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
                    unsafe {
                        libc::close(number);
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
) -> ! {
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
    let restored = unsafe { mask.restore_native() };
    if restored != 0 {
        unsafe { report_and_exit(error_fd, restored) }
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
        if command.cmd.as_bytes().contains(&b'/') {
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
                assert!(status.success());
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "isolated process owner timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    fn command(script: &str) -> Command {
        Command {
            cmd: CString::new("/bin/sh").unwrap(),
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
    fn closed_standard_streams_remain_closed() {
        const NAME: &str = "process_launch::tests::closed_standard_streams_remain_closed";
        if std::env::var_os("ALIGN_R65_CLOSED_STDIO").is_some() {
            unsafe {
                libc::close(0);
                libc::close(1);
                libc::close(2);
            }
            let mut child = launch(&command("exit 7"), false, false).unwrap();
            assert_eq!(child.wait().unwrap().termination.exited, 7);
            let mut captured = launch(&command("exit 8"), true, false).unwrap();
            captured.stdout.fd.take();
            captured.stderr.fd.take();
            assert_eq!(captured.wait().unwrap().termination.exited, 8);
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
                        assert_eq!(child.wait().unwrap().termination.exited, 0);
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
        let mut command = command("printf x; sleep 1");
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
            child.wait().unwrap().termination.signaled,
            i64::from(libc::SIGKILL)
        );
        assert_eq!(child.stdout.read(&mut bytes).unwrap(), Some(0));
        assert_eq!(child.stdout.read(&mut bytes).unwrap(), Some(0));
        assert_eq!(child.signal(0, true), Err(AL_INVALID));
    }
    #[test]
    fn native_launch_error_and_final_environment() {
        let mut command = command("exit 127");
        let mut child = launch(&command, false, false).unwrap();
        assert_eq!(child.wait().unwrap().termination.exited, 127);
        command.cmd = CString::new("/nonexistent/align-r65-executable").unwrap();
        assert!(matches!(
            launch(&command, false, false),
            Err(super::super::AL_NOT_FOUND)
        ));
        command.cmd = CString::new("sh").unwrap();
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
                .exited,
            127
        );
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
        assert_eq!(child.stdout.read(&mut byte).unwrap(), Some(0));
        let mut readiness = [0u8; 3];
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
        assert_eq!(result.termination.exited, 0);
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
        assert_eq!(child.signal(0, true), Ok(()));
        for signal in [-1, super::super::MAX_SIGNAL + 1, i64::MAX] {
            assert_eq!(child.signal(signal, true), Err(AL_INVALID));
        }
        assert_eq!(child.wait().unwrap().termination.exited, 143);
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
            assert_eq!(child.wait().unwrap().termination.exited, 0);
        }
        assert_eq!(
            unsafe { libc::fcntl(foreign.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
    }
}
