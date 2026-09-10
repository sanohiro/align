//! Native process state shared by synchronous capture and explicit live owners.
use super::{AL_INVALID, io_error_to_status};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OptionalCount {
    pub tag: u8,
    pub padding: [u8; 7],
    pub value: i64,
}
impl OptionalCount {
    pub(crate) fn from_nonnegative(value: i128) -> Self {
        match i64::try_from(value).ok().filter(|value| *value >= 0) {
            Some(value) => Self {
                tag: 1,
                padding: [0; 7],
                value,
            },
            None => Self::default(),
        }
    }
}

/// Align natural sums retain all variant payload fields, not a C union.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Termination {
    pub tag: i32,
    pub padding: [u8; 4],
    pub exited: i64,
    pub signaled: i64,
}
impl Termination {
    pub(crate) fn from_wait(status: i32) -> Result<Self, i32> {
        if libc::WIFEXITED(status) {
            Ok(Self {
                exited: i64::from(libc::WEXITSTATUS(status)),
                ..Self::default()
            })
        } else if libc::WIFSIGNALED(status) {
            Ok(Self {
                tag: 1,
                signaled: i64::from(libc::WTERMSIG(status)),
                ..Self::default()
            })
        } else {
            Err(AL_INVALID)
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WaitResult {
    pub termination: Termination,
    pub max_rss_bytes: OptionalCount,
}

#[derive(Default)]
pub(crate) struct Capture {
    pub fd: Option<OwnedFd>,
    pub eof: bool,
}
impl Capture {
    /// The caller has admitted exclusive writable storage. No allocation or retained borrow.
    pub fn read(&mut self, destination: &mut [u8]) -> Result<Option<i64>, i32> {
        let fd = self.fd.as_ref().ok_or(AL_INVALID)?;
        if destination.is_empty() {
            return Err(AL_INVALID);
        }
        if self.eof {
            return Ok(Some(0));
        }
        // SAFETY: the fd is owned, and the exclusive slice supplies the complete writable range.
        let count = unsafe {
            libc::read(
                fd.as_raw_fd(),
                destination.as_mut_ptr().cast(),
                destination.len(),
            )
        };
        if count < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock
                || error.kind() == std::io::ErrorKind::Interrupted
            {
                return Ok(None);
            }
            return Err(io_error_to_status(&error));
        }
        self.eof = count == 0;
        i64::try_from(count).map(Some).map_err(|_| AL_INVALID)
    }
}

pub struct NativeChild {
    pub(crate) started: std::time::Instant,
    pub(crate) tracked: bool,
    pub(crate) pid: libc::pid_t,
    pub(crate) observed: Option<Termination>,
    pub(crate) reaped: Option<WaitResult>,
    pub(crate) lost: bool,
    pub(crate) new_session: bool,
    pub(crate) stdout: Capture,
    pub(crate) stderr: Capture,
    pub(crate) event: Option<OwnedFd>,
    event_missed: bool,
}
impl NativeChild {
    pub(crate) fn new(pid: libc::pid_t) -> Self {
        Self {
            started: std::time::Instant::now(),
            tracked: false,
            pid,
            observed: None,
            reaped: None,
            lost: false,
            new_session: false,
            stdout: Capture::default(),
            stderr: Capture::default(),
            event: None,
            event_missed: false,
        }
    }
    pub(crate) fn status(&mut self) -> Result<Option<Termination>, i32> {
        if self.lost {
            return Err(AL_INVALID);
        }
        if let Some(result) = self.reaped {
            return Ok(Some(result.termination));
        }
        if self.observed.is_some() {
            return Ok(self.observed);
        }
        let pid = libc::id_t::try_from(self.pid).map_err(|_| AL_INVALID)?;
        if self.pid <= 0 {
            return Err(AL_INVALID);
        }
        // SAFETY: waitid initializes this native record; WNOWAIT retains our exclusive reap rights.
        let mut info: libc::siginfo_t = unsafe { core::mem::zeroed() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                pid,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } != 0
        {
            return self
                .wait_error(std::io::Error::last_os_error())
                .map(|()| None);
        }
        // SAFETY: successful waitid initializes the process-status union, with zero PID when pending.
        let observed_pid = unsafe { info.si_pid() };
        if observed_pid == 0 {
            return Ok(None);
        }
        if observed_pid != self.pid {
            return Err(AL_INVALID);
        }
        let value = i64::from(unsafe { info.si_status() });
        let result = match info.si_code {
            libc::CLD_EXITED if (0..=255).contains(&value) => Termination {
                exited: value,
                ..Termination::default()
            },
            libc::CLD_KILLED | libc::CLD_DUMPED if value > 0 => Termination {
                tag: 1,
                signaled: value,
                ..Termination::default()
            },
            _ => {
                return Err(AL_INVALID);
            }
        };
        self.observed = Some(result);
        Ok(Some(result))
    }
    fn wait_error(&mut self, error: std::io::Error) -> Result<(), i32> {
        if error.kind() == std::io::ErrorKind::Interrupted {
            return Ok(());
        }
        if error.raw_os_error() == Some(libc::ECHILD) {
            self.lost = true;
            self.release_tracking();
        }
        Err(io_error_to_status(&error))
    }
    pub(crate) fn try_wait(&mut self) -> Result<Option<WaitResult>, i32> {
        self.wait_native(false)
    }
    pub(crate) fn wait(&mut self) -> Result<WaitResult, i32> {
        if let Some(result) = self.try_wait()? {
            return Ok(result);
        }
        if [&self.stdout, &self.stderr]
            .iter()
            .any(|stream| stream.fd.is_some() && !stream.eof)
        {
            return Err(AL_INVALID);
        }
        loop {
            if let Some(result) = self.wait_native(true)? {
                return Ok(result);
            }
        }
    }
    fn wait_native(&mut self, blocking: bool) -> Result<Option<WaitResult>, i32> {
        if self.lost || self.pid <= 0 {
            return Err(AL_INVALID);
        }
        if self.reaped.is_some() {
            return Ok(self.reaped);
        }
        let mut status = 0;
        // SAFETY: both outputs are writable native records; PID remains exclusively owned until reap.
        let mut usage: libc::rusage = unsafe { core::mem::zeroed() };
        let result = match unsafe {
            super::capture_wait4(
                self.pid,
                &mut status,
                if blocking { 0 } else { libc::WNOHANG },
                &mut usage,
            )
        } {
            Ok(value) => value,
            Err(error) => return self.wait_error(error).map(|()| None),
        };
        if result == 0 {
            return Ok(None);
        }
        if result != self.pid {
            return Err(AL_INVALID);
        }
        self.release_tracking();
        let termination = match Termination::from_wait(status) {
            Ok(value) => value,
            Err(error) => {
                self.lost = true;
                return Err(error);
            }
        };
        if self.observed.is_some_and(|cached| cached != termination) {
            self.lost = true;
            return Err(AL_INVALID);
        }
        let rss = i128::from(usage.ru_maxrss);
        #[cfg(target_os = "linux")]
        let rss = rss * 1024;
        let result = WaitResult {
            termination,
            max_rss_bytes: OptionalCount::from_nonnegative(rss),
        };
        self.reaped = Some(result);
        self.observed = Some(termination);
        Ok(Some(result))
    }
    fn release_tracking(&mut self) {
        if self.tracked {
            let mut state = super::process_launch::creation();
            state.children = state
                .children
                .checked_sub(1)
                .unwrap_or_else(|| super::panic_abort("process owner count underflow"));
            self.tracked = false;
        }
    }
    pub(crate) fn signal(&self, signal: i64, group: bool) -> Result<(), i32> {
        if self.lost || self.reaped.is_some() || self.pid <= 0 || (group && !self.new_session) {
            return Err(AL_INVALID);
        }
        if !(0..=super::MAX_SIGNAL).contains(&signal) {
            return Err(AL_INVALID);
        }
        let signal = i32::try_from(signal).map_err(|_| AL_INVALID)?;
        let pid = if group { -self.pid } else { self.pid };
        // SAFETY: the unreaped owner pins the PID and, for a session leader, its group identity.
        if unsafe { libc::kill(pid, signal) } != 0 {
            return Err(io_error_to_status(&std::io::Error::last_os_error()));
        }
        Ok(())
    }
}

impl Drop for NativeChild {
    fn drop(&mut self) {
        self.stdout.fd.take();
        self.stderr.fd.take();
        if self.pid > 0 && self.reaped.is_none() && !self.lost {
            loop {
                match self.wait_native(true) {
                    Ok(None) => continue,
                    Ok(Some(_)) => break,
                    Err(_) if self.lost => break,
                    Err(_) => super::panic_abort("cannot reap owned process during Drop"),
                }
            }
        }
    }
}
pub(crate) fn valid_pointer<T>(pointer: *const T) -> bool {
    !pointer.is_null()
        && pointer.addr().is_multiple_of(core::mem::align_of::<T>())
        && pointer
            .addr()
            .checked_add(core::mem::size_of::<T>())
            .is_some()
}
pub(crate) fn valid_range<T>(pointer: *const T, count: i64) -> Option<(usize, usize)> {
    let count = usize::try_from(count).ok()?;
    let bytes = count.checked_mul(core::mem::size_of::<T>())?;
    if bytes > isize::MAX as usize
        || (bytes != 0 && pointer.is_null())
        || (!pointer.is_null() && !pointer.addr().is_multiple_of(core::mem::align_of::<T>()))
    {
        return None;
    }
    Some((pointer.addr(), pointer.addr().checked_add(bytes)?))
}
pub(crate) fn disjoint<A, B>(a: *const A, b: *const B) -> bool {
    let Some(a_end) = a.addr().checked_add(core::mem::size_of::<A>()) else {
        return false;
    };
    let Some(b_end) = b.addr().checked_add(core::mem::size_of::<B>()) else {
        return false;
    };
    a_end <= b.addr() || b_end <= a.addr()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};
    std::thread_local! {
        static FINISH_DURING_FALLBACK: core::cell::Cell<bool> = const {core::cell::Cell::new(false)};
    }
    pub(super) fn finish_during_fallback(pid: i32) {
        if !FINISH_DURING_FALLBACK.with(|value| value.replace(false)) {
            return;
        }
        assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
        let mut info = unsafe { core::mem::zeroed::<libc::siginfo_t>() };
        loop {
            if unsafe {
                libc::waitid(
                    libc::P_PID,
                    u32::try_from(pid).unwrap(),
                    &mut info,
                    libc::WEXITED | libc::WNOWAIT,
                )
            } == 0
            {
                break;
            }
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EINTR)
            );
        }
        // Native terminal state is visible, but no NativeChild cache was updated.
    }
    #[test]
    fn final_fallback_chunk_observes_terminal_transition() {
        let command = crate::process_launch::tests::command("exec sleep 30");
        let mut child = crate::process_launch::launch(&command, false, false).unwrap();
        let event = std::fs::File::open("/dev/null").unwrap().into();
        child
            .finish_event_registration(event, Err(std::io::Error::from_raw_os_error(libc::ESRCH)))
            .unwrap();
        assert!(child.observed.is_none());
        FINISH_DURING_FALLBACK.with(|value| value.set(true));
        // One nanosecond forces this to be the final (possibly zero-rounded) chunk.
        assert_eq!(child.poll(4, 1).unwrap().status, 1);
        assert!(!FINISH_DURING_FALLBACK.with(core::cell::Cell::get));
        assert_eq!(
            child.wait().unwrap().termination.signaled,
            i64::from(libc::SIGKILL)
        );
    }
    #[test]
    fn registration_exit_window_retains_finite_status_observation() {
        let command = crate::process_launch::tests::command("exec sleep 30");
        let mut child = crate::process_launch::launch(&command, false, false).unwrap();
        let event = std::fs::File::open("/dev/null").unwrap().into();
        assert_eq!(
            child.finish_event_registration(
                event,
                Err(std::io::Error::from_raw_os_error(libc::EACCES))
            ),
            Err(crate::AL_DENIED)
        );
        let event = std::fs::File::open("/dev/null").unwrap().into();
        child
            .finish_event_registration(event, Err(std::io::Error::from_raw_os_error(libc::ESRCH)))
            .unwrap();
        assert!(child.event_missed);
        assert!(child.observed.is_none());
        assert_eq!(child.poll(4, 0).unwrap().status, 0);
        assert_eq!(child.poll(4, 5_000_000).unwrap().status, 0);
        child.signal(i64::from(libc::SIGKILL), false).unwrap();
        assert_eq!(child.poll(4, 1_000_000_000).unwrap().status, 1);
        assert_eq!(
            child.wait().unwrap().termination.signaled,
            i64::from(libc::SIGKILL)
        );
        assert_eq!(child.poll(4, 0).unwrap().status, 1);
    }
    #[test]
    fn native_status_layout_and_domains() {
        assert_eq!(
            (size_of::<Termination>(), align_of::<Termination>()),
            (24, 8)
        );
        assert_eq!(
            [
                offset_of!(Termination, tag),
                offset_of!(Termination, exited),
                offset_of!(Termination, signaled)
            ],
            [0, 8, 16]
        );
        assert_eq!(
            (
                size_of::<WaitResult>(),
                offset_of!(WaitResult, max_rss_bytes)
            ),
            (40, 24)
        );
        assert_eq!(offset_of!(OptionalCount, value), 8);
        let exit = Termination::from_wait(143 << 8).unwrap();
        let signal = Termination::from_wait(15).unwrap();
        assert_ne!(exit, signal);
        assert_eq!((exit.tag, exit.exited, exit.signaled), (0, 143, 0));
        assert_eq!((signal.tag, signal.exited, signal.signaled), (1, 0, 15));
        assert_eq!(Termination::from_wait(0x7f), Err(AL_INVALID));
        assert_eq!(
            OptionalCount::from_nonnegative(-1),
            OptionalCount::default()
        );
        assert_eq!(
            OptionalCount::from_nonnegative(i128::from(i64::MAX) + 1),
            OptionalCount::default()
        );
    }
    #[test]
    fn scratch_aliases_and_buffer_ranges_reject_before_effects() {
        let mut child = NativeChild::new(-1);
        let pointer = &mut child as *mut NativeChild;
        let mut count = OptionalCount {
            tag: 1,
            padding: [0; 7],
            value: 99,
        };
        let mut bytes = [0x55u8; 4];
        for length in [-1, 0, i64::MAX] {
            assert_eq!(
                unsafe {
                    align_rt_child_read_stdout(
                        pointer,
                        bytes.as_mut_ptr(),
                        length,
                        (&mut count as *mut OptionalCount).cast(),
                    )
                },
                AL_INVALID
            );
            assert_eq!(count.value, 99);
            assert_eq!(bytes, [0x55; 4]);
        }
        assert_eq!(
            unsafe { align_rt_child_status(pointer, pointer.cast()) },
            AL_INVALID
        );
        assert_eq!(child.pid, -1);
        assert_eq!(
            unsafe {
                align_rt_child_read_stdout(
                    pointer,
                    (&mut count as *mut OptionalCount).cast(),
                    1,
                    (&mut count as *mut OptionalCount).cast(),
                )
            },
            AL_INVALID
        );
        assert_eq!(count.value, 99);
        let unaligned = pointer.cast::<u8>().wrapping_add(1).cast::<NativeChild>();
        assert_eq!(
            unsafe { align_rt_child_try_wait(unaligned, core::ptr::null_mut()) },
            AL_INVALID
        );
        assert_eq!(child.pid, -1);
    }
    #[test]
    fn lost_wait_ownership_never_signals_a_replacement_pid() {
        let mut process = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let mut child = NativeChild::new(i32::try_from(process.id()).unwrap());
        assert!(process.wait().unwrap().success());
        assert!(child.status().is_err());
        assert!(child.lost);
        // A detectable foreign reap invalidates authority even if that numeric PID is reused.
        child.pid = i32::try_from(std::process::id()).unwrap();
        child.new_session = true;
        assert_eq!(child.signal(0, false), Err(AL_INVALID));
        assert_eq!(child.signal(0, true), Err(AL_INVALID));
        assert_eq!(child.try_wait(), Err(AL_INVALID));
    }
    #[test]
    fn observation_retains_wait_ownership_and_reap_caches() {
        let process = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 143"])
            .spawn()
            .unwrap();
        let mut child = NativeChild::new(i32::try_from(process.id()).unwrap());
        drop(process); // std Child does not reap on Drop; transfer exclusive native ownership.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while child.status().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let observed = child.status().unwrap().unwrap();
        assert_eq!(observed.exited, 143);
        assert!(child.reaped.is_none());
        let result = child.wait().unwrap();
        assert_eq!(result.termination, observed);
        assert_eq!(child.wait().unwrap(), result);
        assert_eq!(child.try_wait().unwrap(), Some(result));
        assert_eq!(child.status().unwrap(), Some(observed));
        assert_eq!(child.signal(0, false), Err(AL_INVALID));
        assert_eq!(
            unsafe { libc::waitpid(child.pid, core::ptr::null_mut(), libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct OptionalValue<T: Copy + Default> {
    tag: u8,
    padding: [u8; 7],
    value: T,
}
impl<T: Copy + Default> OptionalValue<T> {
    fn new(value: Option<T>) -> Self {
        match value {
            Some(value) => Self {
                tag: 1,
                padding: [0; 7],
                value,
            },
            None => Self::default(),
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Readiness {
    stdout: u8,
    stderr: u8,
    status: u8,
}

unsafe fn child_output<T: Default>(
    child: *mut NativeChild,
    out: *mut T,
    action: impl FnOnce(&mut NativeChild) -> Result<T, i32>,
) -> i32 {
    if !valid_pointer(child) || !valid_pointer(out) || !disjoint(child, out) {
        return AL_INVALID;
    }
    // SAFETY: callers owe live disjoint allocations; range and alignment were checked above.
    unsafe {
        out.write(T::default());
    }
    match action(unsafe { &mut *child }) {
        Ok(value) => {
            unsafe {
                out.write(value);
            }
            0
        }
        Err(error) => error,
    }
}
/// # Safety
/// `child` is a live, exclusively owned Align child allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_child_id(child: *const NativeChild) -> i64 {
    if !valid_pointer(child) {
        super::panic_abort("invalid process native value");
    }
    if unsafe { (*child).pid } <= 0 {
        super::panic_abort("invalid process native identity");
    }
    i64::from(unsafe { (*child).pid })
}
/// # Safety
/// `child` is exclusively borrowed; `out` is a disjoint, aligned 32-byte output allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_child_status(
    child: *mut NativeChild,
    out: *mut core::ffi::c_void,
) -> i32 {
    unsafe {
        child_output(child, out.cast::<OptionalValue<Termination>>(), |child| {
            child.status().map(OptionalValue::new)
        })
    }
}
/// # Safety
/// `child` is exclusively borrowed; `out` is a disjoint, aligned 48-byte output allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_child_try_wait(
    child: *mut NativeChild,
    out: *mut core::ffi::c_void,
) -> i32 {
    unsafe {
        child_output(child, out.cast::<OptionalValue<WaitResult>>(), |child| {
            child.try_wait().map(OptionalValue::new)
        })
    }
}
unsafe fn child_read(
    child: *mut NativeChild,
    bytes: *mut u8,
    len: i64,
    out: *mut OptionalCount,
    stderr: bool,
) -> i32 {
    if !valid_pointer(child) || unsafe { (*child).pid } <= 0 {
        return AL_INVALID;
    }
    let Ok(length) = usize::try_from(len) else {
        return AL_INVALID;
    };
    if length == 0
        || length > usize::MAX / 2
        || bytes.is_null()
        || bytes.addr().checked_add(length).is_none()
        || !valid_pointer(child)
        || !valid_pointer(out)
        || !disjoint(child, out)
    {
        return AL_INVALID;
    }
    let end = bytes.addr() + length;
    if (bytes.addr() < child.addr() + core::mem::size_of::<NativeChild>() && child.addr() < end)
        || (bytes.addr() < out.addr() + core::mem::size_of::<OptionalCount>() && out.addr() < end)
    {
        return AL_INVALID;
    }
    // SAFETY: all ranges are validated and disjoint; caller supplies exclusive writable bytes.
    unsafe {
        child_output(child, out, |child| {
            let stream = if stderr {
                &mut child.stderr
            } else {
                &mut child.stdout
            };
            if stream.fd.is_none() {
                return Err(AL_INVALID);
            }
            stream
                .read(core::slice::from_raw_parts_mut(bytes, length))
                .map(|value| {
                    value
                        .map(|value| OptionalCount {
                            tag: 1,
                            padding: [0; 7],
                            value,
                        })
                        .unwrap_or_default()
                })
        })
    }
}
/// # Safety
/// Inputs designate valid, mutually disjoint child, writable byte, and 16-byte output allocations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_child_read_stdout(
    child: *mut NativeChild,
    bytes: *mut u8,
    len: i64,
    out: *mut core::ffi::c_void,
) -> i32 {
    unsafe { child_read(child, bytes, len, out.cast(), false) }
}
/// # Safety
/// Same allocation and exclusive-borrow requirements as child_read_stdout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_child_read_stderr(
    child: *mut NativeChild,
    bytes: *mut u8,
    len: i64,
    out: *mut core::ffi::c_void,
) -> i32 {
    unsafe { child_read(child, bytes, len, out.cast(), true) }
}
/// # Safety
/// `child` denotes a live child allocation whose native wait ownership remains exclusive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_child_kill_group(child: *mut NativeChild, signal: i64) -> i32 {
    if !valid_pointer(child) {
        return AL_INVALID;
    }
    match unsafe { &*child }.signal(signal, true) {
        Ok(()) => 0,
        Err(error) => error,
    }
}
/// Map the exact source sum tag to its native signal number.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_process_signal_number(tag: i32) -> i64 {
    let signal = match tag {
        0 => libc::SIGHUP,
        1 => libc::SIGINT,
        2 => libc::SIGQUIT,
        3 => libc::SIGTERM,
        4 => libc::SIGKILL,
        _ => super::panic_abort("invalid process native value"),
    };
    i64::from(signal)
}

impl NativeChild {
    pub(crate) fn register_event(&mut self) -> Result<(), i32> {
        if self.event.is_some() || self.reaped.is_some() {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        {
            // SAFETY: the exclusively wait-owned PID cannot be reused before this acquisition.
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, self.pid, 0u32) };
            if fd < 0 {
                return Err(io_error_to_status(&std::io::Error::last_os_error()));
            }
            let fd = i32::try_from(fd).map_err(|_| AL_INVALID)?;
            self.event = Some(unsafe { OwnedFd::from_raw_fd(fd) });
        }
        #[cfg(target_os = "macos")]
        {
            let fd = unsafe { libc::kqueue() };
            if fd < 0 {
                return Err(io_error_to_status(&std::io::Error::last_os_error()));
            }
            let event = unsafe { OwnedFd::from_raw_fd(fd) };
            if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(io_error_to_status(&std::io::Error::last_os_error()));
            }
            let mut change: libc::kevent = unsafe { core::mem::zeroed() };
            change.ident = usize::try_from(self.pid).map_err(|_| AL_INVALID)?;
            change.filter = libc::EVFILT_PROC;
            change.flags = libc::EV_ADD | libc::EV_ENABLE;
            change.fflags = libc::NOTE_EXIT;
            let registered = if unsafe {
                libc::kevent(fd, &change, 1, core::ptr::null_mut(), 0, core::ptr::null())
            } < 0
            {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            };
            self.finish_event_registration(event, registered)?;
        }
        Ok(())
    }
    #[cfg(any(target_os = "macos", test))]
    fn finish_event_registration(
        &mut self,
        event: OwnedFd,
        registered: Result<(), std::io::Error>,
    ) -> Result<(), i32> {
        if let Err(error) = registered {
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(io_error_to_status(&error));
            }
            // XNU drains proc references before completing exit (which may block).
            // EVFILT_PROC can thus report ESRCH before waitid exposes termination.
            // The unreaped owner still pins the PID: retain a bounded status fallback.
            self.status()?;
            self.event_missed = true;
        }
        self.event = Some(event);
        Ok(())
    }
    fn poll(&mut self, interest: i32, wait_ns: i64) -> Result<Readiness, i32> {
        if self.lost
            || interest <= 0
            || interest & !7 != 0
            || wait_ns < 0
            || (interest & 1 != 0 && self.stdout.fd.is_none())
            || (interest & 2 != 0 && self.stderr.fd.is_none())
        {
            return Err(AL_INVALID);
        }
        let started = std::time::Instant::now();
        let budget =
            std::time::Duration::from_nanos(u64::try_from(wait_ns).map_err(|_| AL_INVALID)?);
        if interest & 4 != 0 {
            self.register_event()?;
        }
        loop {
            let mut ready = Readiness {
                stdout: u8::from(interest & 1 != 0 && self.stdout.eof),
                stderr: u8::from(interest & 2 != 0 && self.stderr.eof),
                status: u8::from(interest & 4 != 0 && self.status()?.is_some()),
            };
            let immediate = ready.stdout | ready.stderr | ready.status != 0;
            let fd = |stream: &Capture, bit| {
                if interest & bit != 0 {
                    stream.fd.as_ref().map(AsRawFd::as_raw_fd).unwrap_or(-1)
                } else {
                    -1
                }
            };
            let mut fds = [
                libc::pollfd {
                    fd: fd(&self.stdout, 1),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: fd(&self.stderr, 2),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: if interest & 4 != 0 && !self.event_missed {
                        self.event.as_ref().map(AsRawFd::as_raw_fd).unwrap_or(-1)
                    } else {
                        -1
                    },
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let remaining = budget.saturating_sub(started.elapsed());
            let remaining = if self.event_missed && interest & 4 != 0 {
                remaining.min(std::time::Duration::from_millis(1))
            } else {
                remaining
            };
            let timeout = if immediate {
                0
            } else {
                i32::try_from(remaining.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX)
            };
            let count = unsafe { libc::poll(fds.as_mut_ptr(), 3, timeout) };
            if count < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(io_error_to_status(&error));
                }
            } else if count > 0 {
                if fds.iter().any(|fd| fd.revents & libc::POLLNVAL != 0) {
                    return Err(AL_INVALID);
                }
                ready.stdout |=
                    u8::from(fds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0);
                ready.stderr |=
                    u8::from(fds[1].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0);
                ready.status = u8::from(interest & 4 != 0 && self.status()?.is_some());
                if ready.stdout | ready.stderr | ready.status != 0 {
                    return Ok(ready);
                }
            }
            if count == 0 && self.event_missed && interest & 4 != 0 {
                #[cfg(test)]
                tests::finish_during_fallback(self.pid);
                ready.status = u8::from(self.status()?.is_some());
            }
            if ready.stdout | ready.stderr | ready.status != 0 {
                return Ok(ready);
            }
            if started.elapsed() >= budget {
                return Ok(Readiness::default());
            }
        }
    }
}
/// # Safety
/// `child` is exclusively borrowed and `out` is a disjoint writable 3-byte record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_child_poll(
    child: *mut NativeChild,
    interest: i32,
    wait_ns: i64,
    out: *mut core::ffi::c_void,
) -> i32 {
    unsafe {
        child_output(child, out.cast::<Readiness>(), |child| {
            child.poll(interest, wait_ns)
        })
    }
}

/// Copy the completed typed status without borrowing capture storage.
/// # Safety
/// The owner is live and the output is disjoint writable WaitResult storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_run_output_status(
    owner: *const super::RunOutput,
    out: *mut WaitResult,
) {
    if !valid_pointer(owner) || !valid_pointer(out) || !disjoint(owner, out) {
        super::panic_abort("invalid run_output status storage");
    }
    unsafe {
        *out = (*owner).status;
    }
}
/// # Safety
/// The owner is live and the output is disjoint writable WaitResult storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_run_bytes_status(
    owner: *const super::RunBytes,
    out: *mut WaitResult,
) {
    if !valid_pointer(owner) || !valid_pointer(out) || !disjoint(owner, out) {
        super::panic_abort("invalid run_bytes status storage");
    }
    unsafe {
        *out = (*owner).status;
    }
}
