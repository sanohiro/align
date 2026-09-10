//! Darwin launch: parent-built actions, ordinary spawn or a single PID replaced by SETEXEC.
use super::*;

unsafe extern "C" {
    fn posix_spawn_file_actions_addinherit_np(
        actions: *mut libc::posix_spawn_file_actions_t,
        fd: i32,
    ) -> i32;
    fn posix_spawn_file_actions_addchdir_np(
        actions: *mut libc::posix_spawn_file_actions_t,
        path: *const libc::c_char,
    ) -> i32;
}
fn checked(errno: i32) -> Result<(), i32> {
    if errno == 0 {
        Ok(())
    } else {
        Err(io_error_to_status(&std::io::Error::from_raw_os_error(
            errno,
        )))
    }
}
pub(super) struct SignalMask {
    previous: libc::sigset_t,
}
impl SignalMask {
    pub(super) fn block() -> Result<Self, i32> {
        let mut all = 0;
        let mut previous = 0;
        unsafe {
            libc::sigfillset(&mut all);
        }
        checked(unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &all, &mut previous) })?;
        Ok(Self { previous })
    }
}
impl Drop for SignalMask {
    fn drop(&mut self) {
        if unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.previous, core::ptr::null_mut())
        } != 0
        {
            super::super::panic_abort("cannot restore launch signal mask");
        }
    }
}
struct Actions(libc::posix_spawn_file_actions_t);
impl Actions {
    fn new() -> Result<Self, i32> {
        acquisition()?;
        let mut value = core::ptr::null_mut();
        checked(unsafe { libc::posix_spawn_file_actions_init(&mut value) })?;
        Ok(Self(value))
    }
}
impl Drop for Actions {
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawn_file_actions_destroy(&mut self.0);
        }
    }
}
struct Attributes(libc::posix_spawnattr_t);
impl Attributes {
    fn new(
        command: &Command,
        force_group: bool,
        mask: &SignalMask,
        timed: bool,
    ) -> Result<Self, i32> {
        acquisition()?;
        let mut value = core::ptr::null_mut();
        checked(unsafe { libc::posix_spawnattr_init(&mut value) })?;
        let mut owner = Self(value);
        let mut defaults = 0;
        unsafe {
            libc::sigemptyset(&mut defaults);
        }
        for signal in 1..=31 {
            if signal == libc::SIGKILL || signal == libc::SIGSTOP {
                continue;
            }
            let mut disposition: libc::sigaction = unsafe { core::mem::zeroed() };
            if unsafe { libc::sigaction(signal, core::ptr::null(), &mut disposition) } != 0 {
                return Err(error());
            }
            if disposition.sa_sigaction != libc::SIG_DFL
                && disposition.sa_sigaction != libc::SIG_IGN
            {
                unsafe {
                    libc::sigaddset(&mut defaults, signal);
                }
            }
        }
        let mut flags = libc::POSIX_SPAWN_CLOEXEC_DEFAULT
            | libc::POSIX_SPAWN_SETSIGDEF
            | libc::POSIX_SPAWN_SETSIGMASK;
        if timed {
            flags |= libc::POSIX_SPAWN_SETEXEC;
        } else if command.new_session {
            flags |= 0x0400;
        }
        // XNU POSIX_SPAWN_SETSID
        else if force_group {
            flags |= libc::POSIX_SPAWN_SETPGROUP;
            checked(unsafe { libc::posix_spawnattr_setpgroup(&mut owner.0, 0) })?;
        }
        checked(unsafe {
            libc::posix_spawnattr_setflags(
                &mut owner.0,
                i16::try_from(flags).map_err(|_| AL_INVALID)?,
            )
        })?;
        checked(unsafe { libc::posix_spawnattr_setsigdefault(&mut owner.0, &defaults) })?;
        checked(unsafe { libc::posix_spawnattr_setsigmask(&mut owner.0, &mask.previous) })?;
        Ok(owner)
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawnattr_destroy(&mut self.0);
        }
    }
}
unsafe fn candidates(
    command: &Command,
    prepared: &Prepared,
    actions: &Actions,
    attrs: &Attributes,
) -> Result<i32, i32> {
    let mut failure = libc::ENOENT;
    let mut denied = false;
    for candidate in &prepared.candidates {
        let mut pid = 0;
        failure = unsafe {
            libc::posix_spawn(
                &mut pid,
                candidate.as_ptr(),
                &actions.0,
                &attrs.0,
                prepared.argv.as_ptr().cast(),
                prepared.envp.as_ptr().cast(),
            )
        };
        if failure == 0 {
            return Ok(pid);
        }
        if command.target.direct_path() {
            return Err(failure);
        }
        match failure {
            libc::EACCES => denied = true,
            libc::ENOENT | libc::ENOTDIR | libc::ESTALE | libc::ENODEV | libc::ETIMEDOUT => {}
            _ => return Err(failure),
        }
    }
    Err(if denied { libc::EACCES } else { failure })
}
pub(super) unsafe fn spawn(
    command: &Command,
    prepared: &Prepared,
    streams: &[i32; 3],
    error_fd: i32,
    mask: &SignalMask,
    force_group: bool,
) -> Result<i32, i32> {
    let timed = command.timeout_ns > 0;
    let mut actions = Actions::new()?;
    let attributes = Attributes::new(command, force_group, mask, timed)?;
    for (source, destination) in streams.iter().zip(0..3) {
        // An absent standard descriptor is already closed. Darwin executes close
        // actions strictly (EBADF aborts spawn); SETEXEC would also close it twice.
        // CLOEXEC_DEFAULT leaves only the explicitly inherited/duplicated streams.
        if *source < 0 {
            continue;
        }
        checked(if timed {
            unsafe { posix_spawn_file_actions_addinherit_np(&mut actions.0, destination) }
        } else {
            unsafe { libc::posix_spawn_file_actions_adddup2(&mut actions.0, *source, destination) }
        })?;
    }
    if !timed {
        if let Some(cwd) = &command.cwd {
            checked(unsafe { posix_spawn_file_actions_addchdir_np(&mut actions.0, cwd.as_ptr()) })?;
        }
        return unsafe { candidates(command, prepared, &actions, &attributes) }
            .map_err(|errno| io_error_to_status(&std::io::Error::from_raw_os_error(errno)));
    }
    // libSystem's fork owns Darwin's supported fork bookkeeping. Child never returns to Rust Drop.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(error());
    }
    if pid != 0 {
        return Ok(pid);
    }
    if let Some(cwd) = &command.cwd {
        if unsafe { libc::chdir(cwd.as_ptr()) } != 0 {
            unsafe {
                report_and_exit(error_fd, native_error());
            }
        }
    }
    if command.new_session {
        if unsafe { libc::setsid() } < 0 {
            unsafe {
                report_and_exit(error_fd, native_error());
            }
        }
    } else if force_group && unsafe { libc::setpgid(0, 0) } != 0 {
        unsafe {
            report_and_exit(error_fd, native_error());
        }
    }
    for (source, destination) in streams.iter().zip(0..3) {
        if *source < 0 {
            unsafe {
                libc::close(destination);
            }
        } else if unsafe { libc::dup2(*source, destination) } < 0 {
            unsafe {
                report_and_exit(error_fd, native_error());
            }
        }
    }
    // The action list contains no operation that consumes a CLOEXEC source or repeats setup.
    let errno = match unsafe { candidates(command, prepared, &actions, &attributes) } {
        Err(errno) => errno,
        Ok(_) => libc::EIO,
    };
    unsafe {
        report_and_exit(error_fd, errno);
    }
}
