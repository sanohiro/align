//! Explicit process-global signal observation. Handlers access only permanent atomic storage.
use super::process_live::{disjoint, valid_pointer};
use super::{AL_INVALID, io_error_to_status, panic_abort};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering::SeqCst},
};

static RESERVED: Mutex<bool> = Mutex::new(false);
static WORD: AtomicU64 = AtomicU64::new(0);
const SIGNALS: [i32; 4] = [libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM];
// sigaction.sa_sigaction is libc's pointer-width integer field. Function-pointer
// conversions below preserve its complete address; they never narrow a count.
const GENERATION_MAX: u64 = u64::MAX >> 8;

fn errno() -> i32 {
    io_error_to_status(&std::io::Error::last_os_error())
}
#[cfg(target_os = "linux")]
unsafe fn errno_address() -> *mut i32 {
    unsafe { libc::__errno_location() }
}
#[cfg(target_os = "macos")]
unsafe fn errno_address() -> *mut i32 {
    unsafe { libc::__error() }
}

extern "C" fn observe(signal: i32) {
    // No owner pointer, allocation, lock, fd, forwarding, or user callback crosses this boundary.
    let pointer = unsafe { errno_address() };
    let saved = unsafe { *pointer };
    if let Some(index) = SIGNALS.iter().position(|native| *native == signal) {
        publish(&WORD, WORD.load(SeqCst), index);
    }
    unsafe {
        *pointer = saved;
    }
}
fn publish(word: &AtomicU64, mut observed: u64, index: usize) {
    let generation = observed >> 8;
    let bit = 1u64 << index;
    while generation != 0 && observed >> 8 == generation && observed & bit != 0 {
        match word.compare_exchange(observed, observed | (bit << 4), SeqCst, SeqCst) {
            Ok(_) => return,
            Err(current) => observed = current,
        }
    }
}

struct Subscription {
    previous: [libc::sigaction; 4],
    installed: u8,
    generation: u64,
    closed: bool,
}
impl Subscription {
    fn restore(&mut self) -> Result<u8, i32> {
        let mut pending = 0;
        for index in (0..4).rev() {
            let bit = 1u8 << index;
            if self.installed & bit == 0 {
                continue;
            }
            let mut current: libc::sigaction = unsafe { core::mem::zeroed() };
            if unsafe { libc::sigaction(SIGNALS[index], core::ptr::null(), &mut current) } != 0 {
                return Err(errno());
            }
            if !our_action(&current) {
                return Err(AL_INVALID);
            }
            set_action(index, &self.previous[index], true)?;
            let old = WORD.fetch_and(!(u64::from(bit) | (u64::from(bit) << 4)), SeqCst);
            pending |= u8::try_from((old >> 4) & 15).unwrap_or(0) & bit;
            self.installed &= !bit;
        }
        Ok(pending)
    }
    fn close(&mut self) -> Result<(), i32> {
        if self.closed {
            return Ok(());
        }
        let _launch = super::process_launch::creation();
        let mut reserved = RESERVED
            .lock()
            .unwrap_or_else(|_| panic_abort("signal reservation poisoned"));
        if !*reserved || WORD.load(SeqCst) >> 8 != self.generation {
            return Err(AL_INVALID);
        }
        self.restore()?;
        WORD.fetch_and(!255, SeqCst);
        self.closed = true;
        *reserved = false;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<i32>, i32> {
        if self.closed {
            return Err(AL_INVALID);
        }
        let mut observed = WORD.load(SeqCst);
        loop {
            if observed >> 8 != self.generation {
                return Err(AL_INVALID);
            }
            let pending = ((observed >> 4) & observed) & 15;
            if pending == 0 {
                return Ok(None);
            }
            let index = pending.trailing_zeros();
            match WORD.compare_exchange(observed, observed & !(1u64 << (index + 4)), SeqCst, SeqCst)
            {
                Ok(_) => return Ok(Some(i32::try_from(index).map_err(|_| AL_INVALID)?)),
                Err(current) => observed = current,
            }
        }
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        if self.close().is_err() {
            panic_abort("cannot restore signal subscription during Drop");
        }
    }
}
fn our_action(action: &libc::sigaction) -> bool {
    // Linux's libc supplies its own restorer bit; it does not change handler semantics.
    #[cfg(target_os = "linux")]
    let ignored_flags = 0x04000000;
    #[cfg(target_os = "macos")]
    let ignored_flags = 0;
    if action.sa_sigaction != observe as *const () as usize || action.sa_flags & !ignored_flags != 0
    {
        return false;
    }
    for signal in 1..=super::MAX_SIGNAL {
        if unsafe { libc::sigismember(&action.sa_mask, i32::try_from(signal).unwrap_or(0)) } != 0 {
            return false;
        }
    }
    true
}
// Parent-only syscall seam; signal handlers never enter this function.
fn set_action(index: usize, action: &libc::sigaction, restoring: bool) -> Result<(), i32> {
    #[cfg(test)]
    if tests::FAIL_ACTION.load(SeqCst)
        == i32::try_from(index).unwrap_or(-1) + if restoring { 4 } else { 0 }
    {
        tests::FAIL_ACTION.store(-1, SeqCst);
        if !restoring && index > 0 {
            unsafe {
                libc::raise(SIGNALS[0]);
            }
        }
        return Err(libc::EIO);
    }
    let _ = restoring;
    if unsafe { libc::sigaction(SIGNALS[index], action, core::ptr::null_mut()) } == 0 {
        Ok(())
    } else {
        Err(errno())
    }
}
fn create(selection: i32) -> Result<Box<Subscription>, i32> {
    if selection <= 0 || selection & !15 != 0 {
        return Err(AL_INVALID);
    }
    let _launch = super::process_launch::creation();
    let mut reserved = RESERVED
        .lock()
        .unwrap_or_else(|_| panic_abort("signal reservation poisoned"));
    if *reserved {
        return Err(AL_INVALID);
    }
    let generation = WORD.load(SeqCst) >> 8;
    if generation == GENERATION_MAX {
        return Err(AL_INVALID);
    }
    let mut mask: libc::sigset_t = unsafe { core::mem::zeroed() };
    let result = unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, core::ptr::null(), &mut mask) };
    if result != 0 {
        return Err(io_error_to_status(&std::io::Error::from_raw_os_error(
            result,
        )));
    }
    let mut previous = unsafe { core::mem::zeroed::<[libc::sigaction; 4]>() };
    for index in 0..4 {
        if selection & (1 << index) == 0 {
            continue;
        }
        if unsafe { libc::sigismember(&mask, SIGNALS[index]) } != 0 {
            return Err(AL_INVALID);
        }
        if unsafe { libc::sigaction(SIGNALS[index], core::ptr::null(), &mut previous[index]) } != 0
        {
            return Err(errno());
        }
    }
    let mut owner = Box::new(Subscription {
        previous,
        installed: 0,
        generation: generation + 1,
        closed: false,
    });
    *reserved = true;
    WORD.store(owner.generation << 8, SeqCst);
    let mut action: libc::sigaction = unsafe { core::mem::zeroed() };
    action.sa_sigaction = observe as *const () as usize;
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
    }
    for index in 0..4 {
        let bit = 1u8 << index;
        if selection & i32::from(bit) == 0 {
            continue;
        }
        WORD.fetch_or(u64::from(bit), SeqCst);
        if let Err(original) = set_action(index, &action, false) {
            let old = WORD.fetch_and(!(u64::from(bit) | (u64::from(bit) << 4)), SeqCst);
            let pending = (u8::try_from((old >> 4) & 15).unwrap_or(0) & bit)
                | owner
                    .restore()
                    .unwrap_or_else(|_| panic_abort("cannot roll back signal subscription"));
            WORD.fetch_and(!255, SeqCst);
            owner.closed = true;
            *reserved = false;
            drop(reserved);
            for (index, signal) in SIGNALS.iter().enumerate() {
                if pending & (1 << index) != 0 && unsafe { libc::raise(*signal) } != 0 {
                    panic_abort("cannot redeliver signal after subscription rollback");
                }
            }
            return Err(original);
        }
        owner.installed |= bit;
    }
    Ok(owner)
}
#[repr(C)]
#[derive(Default)]
struct OptionalSignal {
    tag: u8,
    padding: [u8; 3],
    signal: i32,
}

/// # Safety
/// out points to an aligned writable owner slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_signals(
    selection: i32,
    out: *mut *mut core::ffi::c_void,
) -> i32 {
    if !valid_pointer(out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(core::ptr::null_mut());
    }
    match create(selection) {
        Ok(owner) => {
            unsafe {
                out.write(Box::into_raw(owner).cast());
            }
            0
        }
        Err(error) => error,
    }
}
/// # Safety
/// handle is exclusively borrowed; out is disjoint aligned writable Option<signal> storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_signal_next(
    handle: *mut core::ffi::c_void,
    out: *mut core::ffi::c_void,
) -> i32 {
    let handle = handle.cast::<Subscription>();
    let out = out.cast::<OptionalSignal>();
    if !valid_pointer(handle) || !valid_pointer(out) || !disjoint(handle, out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(OptionalSignal::default());
    }
    match unsafe { &mut *handle }.next() {
        Ok(value) => {
            if let Some(signal) = value {
                unsafe {
                    out.write(OptionalSignal {
                        tag: 1,
                        padding: [0; 3],
                        signal,
                    });
                }
            }
            0
        }
        Err(error) => error,
    }
}
/// # Safety
/// handle is one exclusively borrowed live subscription shell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_signal_close(handle: *mut core::ffi::c_void) -> i32 {
    let handle = handle.cast::<Subscription>();
    if !valid_pointer(handle) {
        return AL_INVALID;
    }
    match unsafe { &mut *handle }.close() {
        Ok(()) => 0,
        Err(error) => error,
    }
}
/// # Safety
/// handle is null or one uniquely owned subscription shell; no aliases survive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_signal_free(handle: *mut core::ffi::c_void) {
    if handle.is_null() {
        return;
    }
    let handle = handle.cast::<Subscription>();
    if !valid_pointer(handle) {
        panic_abort("invalid signal subscription Drop");
    }
    unsafe {
        drop(Box::from_raw(handle));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) static FAIL_ACTION: std::sync::atomic::AtomicI32 =
        std::sync::atomic::AtomicI32::new(-1);
    static REDELIVERED: AtomicU64 = AtomicU64::new(0);
    extern "C" fn prior(_: i32) {
        REDELIVERED.fetch_add(1, SeqCst);
    }
    #[test]
    fn partial_install_restore_retry_and_blocked_selection() {
        const MARKER: &str = "ALIGN_R65_SIGNAL_FAILURES";
        if std::env::var_os(MARKER).is_none() {
            crate::process_launch::tests::isolated(
                "process_signal::tests::partial_install_restore_retry_and_blocked_selection",
                MARKER,
            );
            return;
        }
        let mut previous: libc::sigaction = unsafe { core::mem::zeroed() };
        previous.sa_sigaction = prior as *const () as usize;
        unsafe {
            libc::sigemptyset(&mut previous.sa_mask);
        }
        for signal in SIGNALS {
            assert_eq!(
                unsafe { libc::sigaction(signal, &previous, core::ptr::null_mut()) },
                0
            );
        }
        for failed in 0..4 {
            REDELIVERED.store(0, SeqCst);
            FAIL_ACTION.store(failed, SeqCst);
            assert!(matches!(create(15), Err(libc::EIO)));
            assert!(!*RESERVED.lock().unwrap());
            assert_eq!(WORD.load(SeqCst) & 255, 0);
            assert_eq!(REDELIVERED.load(SeqCst), u64::from(failed > 0));
            for signal in SIGNALS {
                let mut current: libc::sigaction = unsafe { core::mem::zeroed() };
                assert_eq!(
                    unsafe { libc::sigaction(signal, core::ptr::null(), &mut current) },
                    0
                );
                assert_eq!(current.sa_sigaction, previous.sa_sigaction);
            }
        }
        for failed in (0..4).rev() {
            let mut owner = create(15).unwrap();
            FAIL_ACTION.store(failed + 4, SeqCst);
            assert_eq!(owner.close(), Err(libc::EIO));
            assert!(matches!(create(1), Err(AL_INVALID)));
            assert_eq!(owner.installed, (1u8 << (failed + 1)) - 1);
            owner.close().unwrap();
            assert_eq!(owner.installed, 0);
        }
        let mut owner = create(8).unwrap();
        let mut ours: libc::sigaction = unsafe { core::mem::zeroed() };
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGTERM, &previous, &mut ours) },
            0
        );
        assert_eq!(owner.close(), Err(AL_INVALID));
        assert!(matches!(create(1), Err(AL_INVALID)));
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGTERM, &ours, core::ptr::null_mut()) },
            0
        );
        owner.close().unwrap();
        let mut blocked: libc::sigset_t = unsafe { core::mem::zeroed() };
        let mut old: libc::sigset_t = unsafe { core::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut blocked);
            libc::sigaddset(&mut blocked, libc::SIGTERM);
        }
        assert_eq!(
            unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut old) },
            0
        );
        assert!(matches!(create(8), Err(AL_INVALID)));
        assert!(!*RESERVED.lock().unwrap());
        assert_eq!(
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &old, core::ptr::null_mut()) },
            0
        );
    }
    #[test]
    fn captured_generation_cannot_publish_into_a_new_lease() {
        let word = AtomicU64::new((1 << 8) | 15);
        let old = word.load(SeqCst);
        publish(&word, old, 3);
        publish(&word, word.load(SeqCst), 0);
        assert_eq!(word.load(SeqCst), (1 << 8) | 15 | 0x90);
        word.store((2 << 8) | 15, SeqCst);
        publish(&word, old, 2);
        assert_eq!(word.load(SeqCst), (2 << 8) | 15);
        let same = word.load(SeqCst);
        word.fetch_and(!1, SeqCst);
        publish(&word, same, 0);
        assert_eq!(word.load(SeqCst), (2 << 8) | 14);
        assert_eq!(core::mem::size_of::<OptionalSignal>(), 8);
        assert_eq!(core::mem::offset_of!(OptionalSignal, signal), 4);
    }
    #[test]
    fn real_signal_coalescing_overlap_close_and_restoration() {
        const MARKER: &str = "ALIGN_R65_SIGNAL_OWNER";
        if std::env::var_os(MARKER).is_none() {
            crate::process_launch::tests::isolated(
                "process_signal::tests::real_signal_coalescing_overlap_close_and_restoration",
                MARKER,
            );
            return;
        }
        for selection in [-1, 0, 16, i32::MAX] {
            assert!(matches!(create(selection), Err(AL_INVALID)));
        }
        let mut previous: libc::sigaction = unsafe { core::mem::zeroed() };
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGTERM, core::ptr::null(), &mut previous) },
            0
        );
        let mut rejected = core::ptr::dangling_mut::<core::ffi::c_void>();
        assert_eq!(unsafe { align_rt_process_signals(0, &mut rejected) }, AL_INVALID);
        assert!(rejected.is_null());
        assert_eq!(unsafe { align_rt_process_signals(15, core::ptr::null_mut()) }, AL_INVALID);
        assert!(!*RESERVED.lock().unwrap());
        let mut owner = create(15).unwrap();
        let pointer = (&raw mut *owner).cast::<core::ffi::c_void>();
        let before = WORD.load(SeqCst);
        assert_eq!(unsafe { align_rt_process_signal_next(pointer, pointer) }, AL_INVALID);
        assert_eq!(WORD.load(SeqCst), before);
        assert!(matches!(create(1), Err(AL_INVALID)));
        assert_eq!(owner.next(), Ok(None));
        for signal in [
            libc::SIGTERM,
            libc::SIGHUP,
            libc::SIGTERM,
            libc::SIGINT,
            libc::SIGQUIT,
        ] {
            assert_eq!(unsafe { libc::raise(signal) }, 0);
        }
        for expected in 0..4 {
            assert_eq!(owner.next(), Ok(Some(expected)));
        }
        assert_eq!(owner.next(), Ok(None));
        owner.close().unwrap();
        owner.close().unwrap();
        assert_eq!(owner.next(), Err(AL_INVALID));
        let mut restored: libc::sigaction = unsafe { core::mem::zeroed() };
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGTERM, core::ptr::null(), &mut restored) },
            0
        );
        assert_eq!(restored.sa_sigaction, previous.sa_sigaction);
        assert!(create(8).is_ok()); // temporary owner Drop restores and releases the lease
        assert!(!*RESERVED.lock().unwrap());
        WORD.store(GENERATION_MAX << 8, SeqCst);
        assert!(matches!(create(1), Err(AL_INVALID)));
        assert!(!*RESERVED.lock().unwrap());
    }
}
