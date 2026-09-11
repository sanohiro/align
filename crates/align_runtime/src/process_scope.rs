//! Exclusive Linux child creation, subreaping and kernel-empty release authority.
use super::process_live::{NativeChild, WaitResult, disjoint, valid_pointer};
#[cfg(target_os = "linux")]
use super::process_live::{OptionalCount, Termination};
use super::{AL_INVALID, AlignStr, Command, io_error_to_status, panic_abort};
#[cfg(target_os = "linux")]
use std::os::fd::FromRawFd;
use std::os::fd::{AsRawFd, OwnedFd};

#[repr(C)]
pub struct Scope {
    root: NativeChild,
    owner: i64,
    closed: bool,
    lost: bool,
}
pub struct Member {
    fd: OwnedFd,
}
#[repr(C)]
struct MemberInfo {
    handle: *mut Member,
    pid: i64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Reaped {
    pid: i64,
    status: WaitResult,
}
const _: () = assert!(core::mem::offset_of!(Scope, root) == 0);
const _: () = assert!(core::mem::size_of::<MemberInfo>() == 16);
const _: () = assert!(core::mem::size_of::<Reaped>() == 48);
fn status(errno: i32) -> i32 {
    io_error_to_status(&std::io::Error::from_raw_os_error(errno))
}
fn error() -> i32 {
    io_error_to_status(&std::io::Error::last_os_error())
}
fn capacity<T>(count: i64) -> Result<usize, i32> {
    let count = usize::try_from(count)
        .ok()
        .filter(|n| *n > 0)
        .ok_or(AL_INVALID)?;
    count
        .checked_mul(core::mem::size_of::<T>())
        .filter(|n| isize::try_from(*n).is_ok())
        .ok_or(AL_INVALID)?;
    Ok(count)
}
#[cfg(target_os = "linux")]
fn disposition() -> Result<bool, i32> {
    let mut action = unsafe { core::mem::zeroed::<libc::sigaction>() };
    if unsafe { libc::sigaction(libc::SIGCHLD, core::ptr::null(), &mut action) } != 0 {
        return Err(error());
    }
    Ok(action.sa_sigaction != libc::SIG_IGN && action.sa_flags & libc::SA_NOCLDWAIT == 0)
}
#[cfg(target_os = "linux")]
fn subreaper() -> Result<i32, i32> {
    let mut value = 0i32;
    if unsafe { libc::prctl(libc::PR_GET_CHILD_SUBREAPER, &raw mut value, 0, 0, 0) } != 0 {
        Err(error())
    } else {
        Ok(value)
    }
}
#[cfg(test)]
std::thread_local! {
    static FAIL_SUBREAPER: core::cell::Cell<i32> = const {core::cell::Cell::new(-1)};
    static FAIL_ADMIT_AT: core::cell::Cell<usize> = const {core::cell::Cell::new(0)};
    static FAIL_MEMBER_KILL: core::cell::Cell<bool> = const {core::cell::Cell::new(false)};
    static FAIL_REAP_AT: core::cell::Cell<usize> = const {core::cell::Cell::new(0)};
}
#[cfg(target_os = "linux")]
fn set_subreaper(value: i32) -> Result<(), i32> {
    #[cfg(test)]
    if FAIL_SUBREAPER.with(|fail| {
        if fail.get() == value {
            fail.set(-1);
            true
        } else {
            false
        }
    }) {
        return Err(status(libc::EIO));
    }
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, value, 0, 0, 0) } != 0 {
        Err(error())
    } else {
        Ok(())
    }
}
/// The only absence witness. A successful pending or terminal observation is not empty.
#[cfg(target_os = "linux")]
fn empty() -> Result<bool, i32> {
    let mut info = unsafe { core::mem::zeroed::<libc::siginfo_t>() };
    if unsafe {
        libc::waitid(
            libc::P_ALL,
            0,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT | libc::__WALL,
        )
    } == 0
    {
        return Ok(false);
    }
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD) {
        Ok(true)
    } else {
        Err(error())
    }
}
fn start(command: &Command) -> Result<Box<Scope>, i32> {
    if command.timeout_ns > 0
        || command.max_capture_bytes.is_some()
        || matches!(&command.target,super::CommandTarget::Path(path) if path.as_bytes().is_empty())
        || command
            .cwd
            .as_ref()
            .is_some_and(|cwd| cwd.as_bytes().is_empty())
    {
        return Err(AL_INVALID);
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(status(libc::ENOTSUP))
    }
    #[cfg(target_os = "linux")]
    {
        let mut state = super::process_launch::creation();
        if state.scope || state.children != 0 {
            return Err(AL_INVALID);
        }
        if subreaper()? != 0 {
            return Err(AL_INVALID);
        }
        if !disposition()? {
            return Err(AL_INVALID);
        }
        if !empty()? {
            return Err(AL_INVALID);
        }
        set_subreaper(1)?;
        state.scope = true;
        let mut scope = Box::new(Scope {
            root: NativeChild::new(0),
            owner: i64::from(unsafe { libc::getpid() }),
            closed: false,
            lost: false,
        });
        drop(state);
        // The armed scope owns all adoption even if post-exec setup fails.
        match super::process_launch::launch_scope_root(command) {
            Ok(root) => {
                scope.root = *root;
                Ok(scope)
            }
            Err(error) => {
                drop(scope);
                Err(error)
            }
        }
    }
}
impl Scope {
    #[cfg(target_os = "linux")]
    fn valid(&mut self) -> Result<(), i32> {
        if self.closed || self.lost || self.root.lost {
            return Err(AL_INVALID);
        }
        let state = super::process_launch::creation();
        if !state.scope || state.children != 0 || subreaper()? != 1 || !disposition()? {
            self.lost = true;
            return Err(AL_INVALID);
        }
        Ok(())
    }
    fn release(&mut self) -> Result<bool, i32> {
        if self.closed {
            return Ok(true);
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(status(libc::ENOTSUP))
        }
        #[cfg(target_os = "linux")]
        {
            self.valid()?;
            let mut state = super::process_launch::creation();
            if !empty()? {
                return Ok(false);
            }
            if self.root.pid > 0 && self.root.reaped.is_none() {
                self.lost = true;
                return Err(AL_INVALID);
            }
            set_subreaper(0)?;
            state.scope = false;
            self.closed = true;
            Ok(true)
        }
    }
    fn children(&mut self, budget: i64) -> Result<Vec<(i32, Box<Member>)>, i32> {
        let limit = capacity::<MemberInfo>(budget)?;
        #[cfg(not(target_os = "linux"))]
        {
            let _ = limit;
            Err(status(libc::ENOTSUP))
        }
        #[cfg(target_os = "linux")]
        {
            use std::io::Read;
            self.valid()?;
            let mut seen = std::collections::BTreeSet::new();
            let mut used = 0usize;
            let mut charge = || {
                used = used.checked_add(1).ok_or(AL_INVALID)?;
                if used > limit {
                    Err(AL_INVALID)
                } else {
                    Ok(())
                }
            };
            let mut rows = Vec::new();
            let tasks = std::fs::read_dir("/proc/self/task").map_err(|e| io_error_to_status(&e))?;
            for task in tasks {
                charge()?;
                let task = match task {
                    Ok(t) => t,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(io_error_to_status(&e)),
                };
                let mut file = match std::fs::File::open(task.path().join("children")) {
                    Ok(file) => file,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(io_error_to_status(&e)),
                };
                let mut scratch = [0u8; 4096];
                let mut number = 0i32;
                let mut digits = false;
                loop {
                    let count = match file.read(&mut scratch) {
                        Ok(n) => n,
                        Err(e) => return Err(io_error_to_status(&e)),
                    };
                    for byte in scratch[..count]
                        .iter()
                        .copied()
                        .chain((count == 0).then_some(b' '))
                    {
                        if byte.is_ascii_digit() {
                            digits = true;
                            number = number
                                .checked_mul(10)
                                .and_then(|n| n.checked_add(i32::from(byte - b'0')))
                                .ok_or(AL_INVALID)?;
                        } else if byte.is_ascii_whitespace() {
                            if digits {
                                charge()?;
                                if number <= 0 {
                                    return Err(AL_INVALID);
                                }
                                if seen.insert(number)
                                    && let Some(member) = admit(number)?
                                {
                                    rows.push((number, Box::new(member)));
                                }
                                number = 0;
                                digits = false;
                            }
                        } else {
                            return Err(AL_INVALID);
                        }
                    }
                    if count == 0 {
                        break;
                    }
                }
            }
            rows.sort_unstable_by_key(|(pid, _)| *pid);
            Ok(rows)
        }
    }
    fn reap(&mut self, limit: i64) -> Result<Vec<Reaped>, i32> {
        let limit = capacity::<Reaped>(limit)?;
        #[cfg(not(target_os = "linux"))]
        {
            let _ = limit;
            Err(status(libc::ENOTSUP))
        }
        #[cfg(target_os = "linux")]
        {
            self.valid()?;
            let mut rows = Vec::with_capacity(limit);
            while rows.len() < limit {
                #[cfg(test)]
                if FAIL_REAP_AT.with(|fail| {
                    let value = fail.get();
                    if value > 0 {
                        fail.set(value - 1);
                    }
                    value == 1
                }) {
                    return Err(status(libc::EIO));
                }
                let mut raw = 0;
                let mut usage = unsafe { core::mem::zeroed::<libc::rusage>() };
                let pid =
                    unsafe { libc::wait4(-1, &mut raw, libc::WNOHANG | libc::__WALL, &mut usage) };
                if pid < 0 {
                    match std::io::Error::last_os_error().raw_os_error() {
                        Some(libc::ECHILD | libc::EINTR) => break,
                        _ => return Err(error()),
                    }
                }
                if pid == 0 {
                    break;
                }
                let termination = Termination::from_wait(raw).inspect_err(|_| {
                    self.lost = true;
                })?;
                let status = WaitResult {
                    termination,
                    max_rss_bytes: OptionalCount::from_nonnegative(
                        i128::from(usage.ru_maxrss) * 1024,
                    ),
                };
                if pid == self.root.pid {
                    if self.root.reaped.is_some()
                        || self.root.observed.is_some_and(|known| known != termination)
                    {
                        self.lost = true;
                        return Err(AL_INVALID);
                    }
                    self.root.reaped = Some(status);
                    self.root.observed = Some(termination);
                }
                rows.push(Reaped {
                    pid: i64::from(pid),
                    status,
                });
            }
            Ok(rows)
        }
    }
}
#[cfg(target_os = "linux")]
fn admit(pid: i32) -> Result<Option<Member>, i32> {
    #[cfg(test)]
    if FAIL_ADMIT_AT.with(|fail| {
        let value = fail.get();
        if value > 0 {
            fail.set(value - 1);
        }
        value == 1
    }) {
        return Err(status(libc::EIO));
    }
    let mut info = unsafe { core::mem::zeroed::<libc::siginfo_t>() };
    let id = libc::id_t::try_from(pid).map_err(|_| AL_INVALID)?;
    if unsafe {
        libc::waitid(
            libc::P_PID,
            id,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT | libc::__WALL,
        )
    } != 0
    {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ECHILD | libc::ESRCH | libc::ENOENT) => Ok(None),
            _ => Err(error()),
        };
    }
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH | libc::ENOENT) => Ok(None),
            _ => Err(error()),
        };
    }
    let fd = i32::try_from(fd).unwrap_or_else(|_| panic_abort("invalid process pidfd"));
    Ok(Some(Member {
        fd: unsafe { OwnedFd::from_raw_fd(fd) },
    }))
}
impl Member {
    fn kill(&self, signal: i64) -> Result<(), i32> {
        if !(0..=super::MAX_SIGNAL).contains(&signal) {
            return Err(AL_INVALID);
        }
        #[cfg(test)]
        if FAIL_MEMBER_KILL.with(|fail| fail.replace(false)) {
            return Err(status(libc::EPERM));
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(status(libc::ENOTSUP))
        }
        #[cfg(target_os = "linux")]
        {
            let signal = i32::try_from(signal).map_err(|_| AL_INVALID)?;
            if unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    self.fd.as_raw_fd(),
                    signal,
                    core::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            } != 0
            {
                Err(error())
            } else {
                Ok(())
            }
        }
    }
    fn finished(&self) -> Result<bool, i32> {
        let mut fd = libc::pollfd {
            fd: self.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut fd, 1, 0) } < 0 {
            return Err(error());
        }
        if fd.revents & (libc::POLLNVAL | libc::POLLERR) != 0 {
            return Err(AL_INVALID);
        }
        Ok(fd.revents & (libc::POLLIN | libc::POLLHUP) != 0)
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            if self.closed {
                return;
            }
            self.root.stdout.fd.take();
            self.root.stderr.fd.take();
            loop {
                match self.release() {
                    Ok(true) => return,
                    Ok(false) => {}
                    Err(_) => panic_abort("lost exclusive child scope during Drop"),
                }
                let children = match self.children(i64::MAX / 48) {
                    Ok(children) => children,
                    Err(e) if e == status(libc::EINTR) => continue,
                    Err(_) => panic_abort("cannot enumerate exclusive children during Drop"),
                };
                for (_, member) in children {
                    match member.kill(i64::from(libc::SIGKILL)) {
                        Ok(()) => {}
                        Err(e) if e == status(libc::ESRCH) => {}
                        Err(_) => panic_abort("cannot kill exclusive child during Drop"),
                    }
                }
                if self.reap(1024).is_err() {
                    panic_abort("cannot reap exclusive children during Drop");
                }
                std::thread::yield_now();
            }
        }
    }
}
/// # Safety
/// command is shared-borrowed; out is aligned writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_command_start_scope(
    command: *const Command,
    out: *mut *mut Scope,
) -> i32 {
    if !valid_pointer(command) || !valid_pointer(out) || !disjoint(command, out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(core::ptr::null_mut());
    }
    match start(unsafe { &*command }) {
        Ok(scope) => {
            unsafe {
                out.write(Box::into_raw(scope));
            }
            0
        }
        Err(e) => e,
    }
}
/// # Safety
/// scope is a valid shared owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_scope_owner_id(scope: *const Scope) -> i64 {
    if !valid_pointer(scope) {
        panic_abort("invalid scope owner");
    }
    unsafe { (*scope).owner }
}
/// # Safety
/// scope is exclusively borrowed; out is a disjoint aligned writable array header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_scope_children(
    scope: *mut Scope,
    limit: i64,
    out: *mut AlignStr,
) -> i32 {
    if !valid_pointer(scope) || !valid_pointer(out) || !disjoint(scope, out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(AlignStr {
            ptr: core::ptr::null(),
            len: 0,
        });
    }
    let rows = match unsafe { &mut *scope }.children(limit) {
        Ok(rows) => rows,
        Err(e) => return e,
    };
    let count = rows.len();
    let bytes = count
        .checked_mul(core::mem::size_of::<MemberInfo>())
        .and_then(|n| i64::try_from(n).ok())
        .unwrap_or_else(|| panic_abort("invalid bounded member array"));
    let data = super::align_rt_alloc(bytes).cast::<MemberInfo>();
    for (index, (pid, member)) in rows.into_iter().enumerate() {
        unsafe {
            data.add(index).write(MemberInfo {
                handle: Box::into_raw(member),
                pid: i64::from(pid),
            });
        }
    }
    unsafe {
        out.write(AlignStr {
            ptr: data.cast(),
            len: i64::try_from(count).unwrap_or_else(|_| panic_abort("invalid member count")),
        });
    }
    0
}
/// # Safety
/// scope is exclusively borrowed; out is a disjoint aligned writable array header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_scope_reap(
    scope: *mut Scope,
    limit: i64,
    out: *mut AlignStr,
) -> i32 {
    if !valid_pointer(scope) || !valid_pointer(out) || !disjoint(scope, out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(AlignStr {
            ptr: core::ptr::null(),
            len: 0,
        });
    }
    match unsafe { &mut *scope }.reap(limit) {
        Ok(rows) => {
            let bytes = rows
                .len()
                .checked_mul(core::mem::size_of::<Reaped>())
                .and_then(|n| i64::try_from(n).ok())
                .unwrap_or_else(|| panic_abort("invalid reap array"));
            let data = super::align_rt_alloc(bytes).cast::<Reaped>();
            unsafe {
                core::ptr::copy_nonoverlapping(rows.as_ptr(), data, rows.len());
                out.write(AlignStr {
                    ptr: data.cast(),
                    len: i64::try_from(rows.len())
                        .unwrap_or_else(|_| panic_abort("invalid reap count")),
                });
            }
            0
        }
        Err(e) => e,
    }
}
/// # Safety
/// scope is exclusively borrowed; out is a disjoint aligned writable bool.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_scope_release(scope: *mut Scope, out: *mut u8) -> i32 {
    if !valid_pointer(scope) || !valid_pointer(out) || !disjoint(scope, out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(0);
    }
    match unsafe { &mut *scope }.release() {
        Ok(value) => {
            unsafe {
                out.write(u8::from(value));
            }
            0
        }
        Err(e) => e,
    }
}
/// # Safety
/// member is shared-borrowed for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_member_kill(member: *const Member, signal: i64) -> i32 {
    if !valid_pointer(member) {
        return AL_INVALID;
    }
    match unsafe { &*member }.kill(signal) {
        Ok(()) => 0,
        Err(e) => e,
    }
}
/// # Safety
/// member is shared-borrowed; out is a disjoint aligned writable bool.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_member_finished(
    member: *const Member,
    out: *mut u8,
) -> i32 {
    if !valid_pointer(member) || !valid_pointer(out) || !disjoint(member, out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(0);
    }
    match unsafe { &*member }.finished() {
        Ok(value) => {
            unsafe {
                out.write(u8::from(value));
            }
            0
        }
        Err(e) => e,
    }
}
/// # Safety
/// owner is null or one uniquely owned scope shell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_scope_free(owner: *mut Scope) {
    if !owner.is_null() {
        unsafe {
            drop(Box::from_raw(owner));
        }
    }
}
/// # Safety
/// owner is null or one uniquely owned member shell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_member_free(owner: *mut Member) {
    if !owner.is_null() {
        unsafe {
            drop(Box::from_raw(owner));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_layout_and_ranges() {
        assert_eq!(core::mem::offset_of!(Scope, root), 0);
        assert_eq!(
            (
                core::mem::size_of::<MemberInfo>(),
                core::mem::offset_of!(MemberInfo, pid)
            ),
            (16, 8)
        );
        assert_eq!(
            (
                core::mem::size_of::<Reaped>(),
                core::mem::offset_of!(Reaped, status)
            ),
            (48, 8)
        );
        for count in [0, -1, i64::MAX] {
            assert!(capacity::<Reaped>(count).is_err());
        }
        unsafe {
            align_rt_scope_free(core::ptr::null_mut());
            align_rt_process_member_free(core::ptr::null_mut());
        }
        let mut closed = Scope {
            root: NativeChild::new(0),
            owner: 0,
            closed: true,
            lost: false,
        };
        let owner = &raw mut closed;
        let mut output = AlignStr {
            ptr: core::ptr::dangling(),
            len: 99,
        };
        for operation in [align_rt_scope_children, align_rt_scope_reap] {
            assert_eq!(
                unsafe { operation(core::ptr::null_mut(), 1, &mut output) },
                AL_INVALID
            );
            assert_eq!(output.len, 99);
            assert_eq!(unsafe { operation(owner, 1, owner.cast()) }, AL_INVALID);
            assert_eq!(
                unsafe { operation(owner, 1, owner.cast::<u8>().wrapping_add(1).cast()) },
                AL_INVALID
            );
            for limit in [0, -1, i64::MAX]
                .into_iter()
                .chain(cfg!(target_os = "linux").then_some(1))
            {
                output = AlignStr {
                    ptr: core::ptr::dangling(),
                    len: 99,
                };
                assert_eq!(unsafe { operation(owner, limit, &mut output) }, AL_INVALID);
                assert!(output.ptr.is_null());
                assert_eq!(output.len, 0);
            }
            output = AlignStr {
                ptr: core::ptr::dangling(),
                len: 99,
            };
        }
        let mut ready = 99u8;
        assert_eq!(
            unsafe { align_rt_scope_release(core::ptr::null_mut(), &mut ready) },
            AL_INVALID
        );
        assert_eq!(ready, 99);
        assert_eq!(
            unsafe { align_rt_scope_release(owner, owner.cast()) },
            AL_INVALID
        );
        assert_eq!(unsafe { align_rt_scope_release(owner, &mut ready) }, 0);
        assert_eq!(ready, 1);
        let command = crate::process_launch::tests::command("exit 0");
        let command_ptr = &raw const command;
        assert_eq!(
            unsafe { align_rt_command_start_scope(command_ptr, command_ptr.cast_mut().cast()) },
            AL_INVALID
        );
        let mut member = Member {
            fd: std::fs::File::open("/dev/null").unwrap().into(),
        };
        let member_ptr = &raw mut member;
        assert_eq!(
            unsafe { align_rt_process_member_finished(member_ptr, member_ptr.cast()) },
            AL_INVALID
        );
        for signal in [-1, i64::MAX] {
            assert_eq!(
                unsafe { align_rt_process_member_kill(member_ptr, signal) },
                AL_INVALID
            );
        }
        #[cfg(target_os = "macos")]
        {
            let command = crate::process_launch::tests::command("exit 0");
            assert!(matches!(start(&command),Err(e) if e==super::super::AL_CODE+libc::ENOTSUP));
        }
    }
    #[cfg(target_os = "linux")]
    fn isolated(name: &str, marker: &str) -> bool {
        if std::env::var_os(marker).is_some() {
            return false;
        }
        crate::process_launch::tests::isolated(name, marker);
        true
    }
    #[cfg(target_os = "linux")]
    fn finish(scope: &mut Scope) {
        scope.root.stdout.fd.take();
        scope.root.stderr.fd.take();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            for (_, member) in scope.children(4096).unwrap() {
                let result = member.kill(i64::from(libc::SIGKILL));
                assert!(result == Ok(()) || result == Err(status(libc::ESRCH)));
            }
            scope.reap(4096).unwrap();
            if scope.release().unwrap() {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn exclusive_scope() {
        if isolated(
            "process_scope::tests::exclusive_scope",
            "ALIGN_R65_SCOPE_EXCLUSIVE",
        ) {
            return;
        }
        let command = crate::process_launch::tests::command("exec sleep 30");
        let mut ordinary = crate::process_launch::launch(&command, false, false).unwrap();
        assert!(matches!(start(&command), Err(AL_INVALID)));
        ordinary.signal(i64::from(libc::SIGKILL), false).unwrap();
        ordinary.wait().unwrap();
        // Cached ordinary handles do not keep creation reserved.
        let mut scope = start(&command).unwrap();
        assert!(matches!(start(&command), Err(AL_INVALID)));
        assert!(matches!(
            crate::process_launch::launch(&command, false, false),
            Err(AL_INVALID)
        ));
        assert_eq!(scope.release(), Ok(false));
        std::thread::spawn(|| {
            let command = crate::process_launch::tests::command("exit 0");
            assert!(matches!(
                crate::process_launch::launch(&command, false, false),
                Err(AL_INVALID)
            ));
        })
        .join()
        .unwrap();
        assert!(scope.children(1).is_err());
        assert_eq!(scope.release(), Ok(false));
        let mut first = scope.children(4096).unwrap();
        assert_eq!(first.len(), 1);
        let (pid, member) = first.pop().unwrap();
        assert_eq!(pid, scope.root.pid);
        assert_eq!(member.finished(), Ok(false));
        assert_eq!(member.kill(-1), Err(AL_INVALID));
        member.kill(i64::from(libc::SIGKILL)).unwrap();
        finish(&mut scope);
        assert_eq!(member.finished(), Ok(true));
        assert_eq!(member.kill(0), Err(status(libc::ESRCH)));
        assert_eq!(scope.release(), Ok(true));
        let mut second = start(&command).unwrap();
        assert!(scope.children(4096).is_err());
        assert!(scope.reap(4096).is_err());
        drop(scope);
        assert_eq!(second.release(), Ok(false));
        finish(&mut second);
        assert_eq!(subreaper(), Ok(0));
        assert!(empty().unwrap());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn foreign_children_and_failed_constructor() {
        if isolated(
            "process_scope::tests::foreign_children_and_failed_constructor",
            "ALIGN_R65_SCOPE_FOREIGN",
        ) {
            return;
        }
        let command = crate::process_launch::tests::command("exit 0");
        for flags in [libc::SIGCHLD, 0] {
            let pid =
                unsafe { libc::syscall(libc::SYS_clone, flags, 0usize, 0usize, 0usize, 0usize) };
            assert!(pid >= 0);
            if pid == 0 {
                unsafe { libc::_exit(0) }
            }
            assert!(matches!(start(&command), Err(AL_INVALID)));
            let mut status = 0;
            assert_eq!(
                unsafe { libc::waitpid(i32::try_from(pid).unwrap(), &mut status, libc::__WALL) },
                i32::try_from(pid).unwrap()
            );
            assert_eq!(subreaper(), Ok(0));
        }
        set_subreaper(1).unwrap();
        assert!(matches!(start(&command), Err(AL_INVALID)));
        set_subreaper(0).unwrap();
        let mut invalid = crate::process_launch::tests::command("exit 0");
        invalid.target =
            crate::CommandTarget::Path(std::ffi::CString::new("/no/align/r65/executable").unwrap());
        assert!(start(&invalid).is_err());
        assert_eq!(subreaper(), Ok(0));
        assert!(empty().unwrap());
        let mut original = unsafe { core::mem::zeroed::<libc::sigaction>() };
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGCHLD, core::ptr::null(), &mut original) },
            0
        );
        for (handler, flags) in [(libc::SIG_IGN, 0), (libc::SIG_DFL, libc::SA_NOCLDWAIT)] {
            let mut changed = original;
            changed.sa_sigaction = handler;
            changed.sa_flags = flags;
            assert_eq!(
                unsafe { libc::sigaction(libc::SIGCHLD, &changed, core::ptr::null_mut()) },
                0
            );
            let rejected = matches!(start(&command), Err(AL_INVALID));
            assert_eq!(
                unsafe { libc::sigaction(libc::SIGCHLD, &original, core::ptr::null_mut()) },
                0
            );
            assert!(rejected);
        }
        let mut good = start(&command).unwrap();
        finish(&mut good);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn adoption_and_drop() {
        if isolated(
            "process_scope::tests::adoption_and_drop",
            "ALIGN_R65_SCOPE_ADOPTION",
        ) {
            return;
        }
        // Different process groups/session identities do not escape subreaping.
        let command =
            crate::process_launch::tests::command("setsid sh -c 'setsid sleep 30 &' & wait");
        let mut scope = start(&command).unwrap();
        scope.root.stdout.fd.take();
        scope.root.stderr.fd.take();
        let root = scope.root.wait().unwrap();
        assert_eq!(root.termination.exited, 0);
        assert_eq!(scope.release(), Ok(false));
        let members = scope.children(4096).unwrap();
        assert!(!members.is_empty());
        // Negative control: omitting Drop leaves the native lease and children live.
        let raw = Box::into_raw(scope);
        assert_eq!(subreaper(), Ok(1));
        assert!(!empty().unwrap());
        assert!(matches!(start(&command), Err(AL_INVALID)));
        unsafe {
            drop(Box::from_raw(raw));
        }
        assert_eq!(subreaper(), Ok(0));
        assert!(empty().unwrap());
        for (_, member) in members {
            assert_eq!(member.finished(), Ok(true));
            assert_eq!(member.kill(0), Err(status(libc::ESRCH)));
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn restoration_and_reap_failure_state() {
        if isolated(
            "process_scope::tests::restoration_and_reap_failure_state",
            "ALIGN_R65_SCOPE_FAILURES",
        ) {
            return;
        }
        let command = crate::process_launch::tests::command("exit 7");
        FAIL_SUBREAPER.with(|fail| fail.set(1));
        assert!(matches!(start(&command),Err(e) if e==status(libc::EIO)));
        assert_eq!(subreaper(), Ok(0));
        assert!(!crate::process_launch::creation().scope);
        let mut scope = start(&command).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while scope.root.status().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        FAIL_REAP_AT.with(|fail| fail.set(2));
        let mut output = AlignStr {
            ptr: core::ptr::dangling(),
            len: 99,
        };
        assert_eq!(
            unsafe { align_rt_scope_reap(&mut *scope, 2, &mut output) },
            status(libc::EIO)
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);
        assert_eq!(scope.root.wait().unwrap().termination.exited, 7);
        FAIL_SUBREAPER.with(|fail| fail.set(0));
        assert_eq!(scope.release(), Err(status(libc::EIO)));
        assert_eq!(subreaper(), Ok(1));
        assert!(crate::process_launch::creation().scope);
        assert_eq!(scope.release(), Ok(true));
        assert_eq!(subreaper(), Ok(0));
        let mut other = start(&crate::process_launch::tests::command("exec sleep 30")).unwrap();
        set_subreaper(0).unwrap();
        assert_eq!(other.release(), Err(AL_INVALID));
        set_subreaper(1).unwrap();
        assert_eq!(other.release(), Err(AL_INVALID)); // A contradictory native owner stays lost.
        other.lost = false; // Test-only repair permits finalizing our deliberately violated fixture.
        finish(&mut other);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn constructor_phases_and_partial_observations() {
        if isolated(
            "process_scope::tests::constructor_phases_and_partial_observations",
            "ALIGN_R65_SCOPE_PHASES",
        ) {
            return;
        }
        let descriptors = || std::fs::read_dir("/proc/self/fd").unwrap().count();
        let baseline = descriptors();
        let command = crate::process_launch::tests::command("sleep 30 & exit 0");
        let mut completed = false;
        for phase in 1..64 {
            crate::process_launch::inject_acquisition_failure(phase);
            let result = start(&command);
            crate::process_launch::inject_acquisition_failure(0);
            if let Ok(scope) = result {
                drop(scope);
                completed = true;
            }
            assert!(empty().unwrap());
            assert_eq!(subreaper(), Ok(0));
            assert!(!crate::process_launch::creation().scope);
            assert_eq!(descriptors(), baseline, "constructor phase {phase}");
            if completed {
                break;
            }
        }
        assert!(completed);
        let mut scope = start(&command).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while scope.root.status().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let before = descriptors();
        FAIL_ADMIT_AT.with(|fail| fail.set(2));
        assert!(matches!(scope.children(4096),Err(e) if e==status(libc::EIO)));
        assert_eq!(descriptors(), before, "partial members close their pidfds");
        let members = scope.children(4096).unwrap();
        assert!(members.len() >= 2);
        FAIL_MEMBER_KILL.with(|fail| fail.set(true));
        assert_eq!(members[0].1.kill(0), Err(super::super::AL_DENIED));
        assert_eq!(scope.release(), Ok(false));
        drop(members);
        finish(&mut scope);
        let mut second = start(&crate::process_launch::tests::command("exec sleep 30")).unwrap();
        assert!(scope.children(4096).is_err());
        drop(scope);
        assert_eq!(second.release(), Ok(false));
        finish(&mut second);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn non_sigchld_wait_domain() {
        if isolated(
            "process_scope::tests::non_sigchld_wait_domain",
            "ALIGN_R65_SCOPE_WALL",
        ) {
            return;
        }
        let command = crate::process_launch::tests::command("exit 0");
        let mut scope = start(&command).unwrap();
        scope.root.stdout.fd.take();
        scope.root.stderr.fd.take();
        assert_eq!(scope.root.wait().unwrap().termination.exited, 0);
        // Deliberate test-only foreign injection: a direct non-SIGCHLD child.
        // CLONE_PARENT inherits the creator's exit signal and cannot supply this.
        let pid = unsafe { libc::syscall(libc::SYS_clone, 0, 0usize, 0usize, 0usize, 0usize) };
        assert!(pid >= 0);
        if pid == 0 {
            loop {
                unsafe {
                    libc::pause();
                }
            }
        }
        let pid = i32::try_from(pid).unwrap();
        let mut info = unsafe { core::mem::zeroed::<libc::siginfo_t>() };
        // Negative control: plain wait reports absence despite the living child.
        assert_eq!(
            unsafe {
                libc::waitid(
                    libc::P_ALL,
                    0,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
        assert_eq!(scope.release(), Ok(false));
        let members = scope.children(4096).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].0, pid);
        members[0].1.kill(i64::from(libc::SIGKILL)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let rows = loop {
            let rows = scope.reap(1).unwrap();
            if !rows.is_empty() {
                break rows;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(rows[0].pid, i64::from(pid));
        assert_eq!(
            rows[0].status.termination.signaled,
            i64::from(libc::SIGKILL)
        );
        assert!(scope.reap(1).unwrap().is_empty());
        assert_eq!(scope.release(), Ok(true));
    }
}
