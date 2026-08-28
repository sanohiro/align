//! Thin native event adapters for foreground watch builds.

use super::watch::WakeState;
use align_watch::WatchRegistration;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeWatchErrorKind {
    Lost,
    Other,
}

pub(super) struct NativeWatchError {
    pub(super) kind: NativeWatchErrorKind,
    message: String,
}

impl fmt::Display for NativeWatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::CString;
    use std::os::fd::RawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::thread::{self, JoinHandle};

    const WATCH_MASK: u32 = libc::IN_ATTRIB
        | libc::IN_CREATE
        | libc::IN_DELETE
        | libc::IN_DELETE_SELF
        | libc::IN_CLOSE_WRITE
        | libc::IN_MODIFY
        | libc::IN_MOVE_SELF
        | libc::IN_MOVED_FROM
        | libc::IN_MOVED_TO
        | libc::IN_UNMOUNT;

    #[derive(Clone)]
    pub(crate) struct NativeHandle {
        descriptor: i32,
        path: PathBuf,
        generation: u64,
    }

    impl NativeHandle {
        pub(crate) fn path(&self) -> &Path {
            &self.path
        }
    }

    pub(crate) struct NativeWatcher {
        wake: Arc<WakeState>,
        backends: BTreeMap<u64, Backend>,
    }

    struct Backend {
        event_fd: RawFd,
        stop_read: RawFd,
        stop_write: RawFd,
        descriptor_references: BTreeMap<i32, usize>,
        thread: Option<JoinHandle<()>>,
    }

    impl NativeWatcher {
        pub(crate) fn new(wake: Arc<WakeState>) -> io::Result<Self> {
            let mut backends = BTreeMap::new();
            backends.insert(1, Backend::new(Arc::clone(&wake))?);
            Ok(Self { wake, backends })
        }

        pub(crate) fn watch(
            &mut self,
            registration: &WatchRegistration,
            generation: u64,
        ) -> Result<NativeHandle, NativeWatchError> {
            if !self.backends.contains_key(&generation) {
                let backend =
                    Backend::new(Arc::clone(&self.wake)).map_err(|error| NativeWatchError {
                        kind: NativeWatchErrorKind::Other,
                        message: error.to_string(),
                    })?;
                self.backends.insert(generation, backend);
            }
            let Some(backend) = self.backends.get_mut(&generation) else {
                return Err(NativeWatchError {
                    kind: NativeWatchErrorKind::Other,
                    message: "inotify generation setup failed".to_string(),
                });
            };
            let path = CString::new(registration.path().as_os_str().as_bytes()).map_err(|_| {
                NativeWatchError {
                    kind: NativeWatchErrorKind::Other,
                    message: "watch path contains NUL".to_string(),
                }
            })?;
            // SAFETY: `path` is NUL-terminated and the generation backend remains live.
            let descriptor =
                unsafe { libc::inotify_add_watch(backend.event_fd, path.as_ptr(), WATCH_MASK) };
            if descriptor == -1 {
                return Err(classify_add_error(io::Error::last_os_error()));
            }
            let references = backend.descriptor_references.entry(descriptor).or_default();
            *references = references.checked_add(1).ok_or_else(|| NativeWatchError {
                kind: NativeWatchErrorKind::Other,
                message: "inotify descriptor reference count exhausted".to_string(),
            })?;
            Ok(NativeHandle {
                descriptor,
                path: registration.path().to_path_buf(),
                generation,
            })
        }

        pub(crate) fn unwatch(&mut self, handle: &NativeHandle) -> Result<(), NativeWatchError> {
            let Some(backend) = self.backends.get_mut(&handle.generation) else {
                return Ok(());
            };
            let Some(references) = backend
                .descriptor_references
                .get(&handle.descriptor)
                .copied()
            else {
                return Ok(());
            };
            if references > 1 {
                backend
                    .descriptor_references
                    .insert(handle.descriptor, references - 1);
                return Ok(());
            }
            // SAFETY: the watch descriptor belongs to the retained generation backend.
            if unsafe { libc::inotify_rm_watch(backend.event_fd, handle.descriptor) } == -1 {
                let error = io::Error::last_os_error();
                let kind = if matches!(
                    error.raw_os_error(),
                    Some(libc::EINVAL) | Some(libc::ENOENT)
                ) {
                    NativeWatchErrorKind::Lost
                } else {
                    NativeWatchErrorKind::Other
                };
                if kind == NativeWatchErrorKind::Lost {
                    backend.descriptor_references.remove(&handle.descriptor);
                }
                return Err(NativeWatchError {
                    kind,
                    message: error.to_string(),
                });
            }
            backend.descriptor_references.remove(&handle.descriptor);
            Ok(())
        }

        pub(crate) fn retain_generations(&mut self, generations: &BTreeSet<u64>) {
            self.backends
                .retain(|generation, _| generations.contains(generation));
        }

        pub(crate) fn retargets_by_path(&self) -> bool {
            false
        }

        pub(crate) fn wake(&self) -> Arc<WakeState> {
            Arc::clone(&self.wake)
        }
    }

    impl Backend {
        fn new(wake: Arc<WakeState>) -> io::Result<Self> {
            // SAFETY: `inotify_init1` has no pointer arguments and returns an owned descriptor.
            let event_fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
            if event_fd == -1 {
                return Err(io::Error::last_os_error());
            }
            let mut stop = [-1; 2];
            // SAFETY: `stop` is a writable two-descriptor array.
            if unsafe { libc::pipe2(stop.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } == -1 {
                let error = io::Error::last_os_error();
                // SAFETY: `event_fd` is the live descriptor returned above.
                unsafe { libc::close(event_fd) };
                return Err(error);
            }
            let thread = match thread::Builder::new()
                .name("align-watch-inotify".to_string())
                .spawn({
                    let stop_read = stop[0];
                    move || event_loop(event_fd, stop_read, &wake)
                }) {
                Ok(thread) => thread,
                Err(error) => {
                    // SAFETY: all three descriptors are live and remain owned by this setup path.
                    unsafe {
                        libc::close(stop[1]);
                        libc::close(stop[0]);
                        libc::close(event_fd);
                    }
                    return Err(error);
                }
            };
            Ok(Backend {
                event_fd,
                stop_read: stop[0],
                stop_write: stop[1],
                descriptor_references: BTreeMap::new(),
                thread: Some(thread),
            })
        }
    }

    impl Drop for Backend {
        fn drop(&mut self) {
            let byte = [1u8];
            loop {
                // SAFETY: the stop descriptor remains live until after the event thread joins.
                let result = unsafe { libc::write(self.stop_write, byte.as_ptr().cast(), 1) };
                if result == 1 {
                    break;
                }
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            // SAFETY: the event loop has stopped and these owned descriptors are each closed once.
            unsafe {
                libc::close(self.stop_write);
                libc::close(self.stop_read);
                libc::close(self.event_fd);
            }
        }
    }

    fn classify_add_error(error: io::Error) -> NativeWatchError {
        let kind = if matches!(
            error.raw_os_error(),
            Some(libc::ENOENT) | Some(libc::ENOTDIR) | Some(libc::ESTALE)
        ) {
            NativeWatchErrorKind::Lost
        } else {
            NativeWatchErrorKind::Other
        };
        NativeWatchError {
            kind,
            message: error.to_string(),
        }
    }

    fn event_loop(event_fd: RawFd, stop_fd: RawFd, wake: &WakeState) {
        let mut bytes = [0u8; 4_096];
        loop {
            let mut poll = [
                libc::pollfd {
                    fd: stop_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: event_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: `poll` is a live two-element poll array.
            let result = unsafe { libc::poll(poll.as_mut_ptr(), 2, -1) };
            if result == -1 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                wake.set_fatal(2);
                return;
            }
            if poll[0].revents != 0 {
                return;
            }
            if poll[1].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                wake.set_fatal(1);
                return;
            }
            if poll[1].revents & libc::POLLIN == 0 {
                continue;
            }
            loop {
                // SAFETY: `bytes` is writable and `event_fd` is a live nonblocking descriptor.
                let count = unsafe { libc::read(event_fd, bytes.as_mut_ptr().cast(), bytes.len()) };
                if count == -1 {
                    let error = io::Error::last_os_error();
                    if error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    if error.kind() == io::ErrorKind::WouldBlock {
                        break;
                    }
                    wake.set_fatal(2);
                    return;
                }
                if count == 0 {
                    wake.set_fatal(1);
                    return;
                }
                classify_events(&bytes[..usize::try_from(count).unwrap_or(0)], wake);
            }
        }
    }

    fn classify_events(mut bytes: &[u8], wake: &WakeState) {
        let header = std::mem::size_of::<libc::inotify_event>();
        let mut dirty = false;
        let mut uncertain = false;
        while bytes.len() >= header {
            // SAFETY: the header bytes are present; inotify records need not be Rust-aligned.
            let event =
                unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<libc::inotify_event>()) };
            let record = match header.checked_add(event.len as usize) {
                Some(record) if record <= bytes.len() => record,
                _ => {
                    wake.set_fatal(2);
                    return;
                }
            };
            if event.mask & libc::IN_Q_OVERFLOW != 0
                || event.mask
                    & (libc::IN_IGNORED
                        | libc::IN_DELETE_SELF
                        | libc::IN_MOVE_SELF
                        | libc::IN_UNMOUNT)
                    != 0
            {
                uncertain = true;
            } else {
                dirty = true;
            }
            bytes = &bytes[record..];
        }
        if uncertain {
            wake.set_uncertain();
        } else if dirty {
            wake.set_dirty();
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use notify::{RecommendedWatcher, RecursiveMode, Watcher};
    use std::collections::BTreeSet;

    #[derive(Clone)]
    pub(crate) struct NativeHandle {
        path: PathBuf,
    }

    impl NativeHandle {
        pub(crate) fn path(&self) -> &Path {
            &self.path
        }
    }

    pub(crate) struct NativeWatcher {
        watcher: RecommendedWatcher,
        wake: Arc<WakeState>,
    }

    impl NativeWatcher {
        pub(crate) fn new(wake: Arc<WakeState>) -> io::Result<Self> {
            let callback_wake = Arc::clone(&wake);
            let watcher = notify::recommended_watcher(
                move |event: notify::Result<notify::Event>| match event {
                    Ok(event) if event.need_rescan() => callback_wake.set_uncertain(),
                    Ok(_) => callback_wake.set_dirty(),
                    Err(error) => match error.kind {
                        notify::ErrorKind::PathNotFound | notify::ErrorKind::WatchNotFound => {
                            callback_wake.set_uncertain()
                        }
                        notify::ErrorKind::Io(_) => callback_wake.set_fatal(2),
                        notify::ErrorKind::MaxFilesWatch => callback_wake.set_fatal(3),
                        notify::ErrorKind::InvalidConfig(_) => callback_wake.set_fatal(4),
                        notify::ErrorKind::Generic(_) => callback_wake.set_fatal(5),
                    },
                },
            )
            .map_err(io::Error::other)?;
            Ok(Self { watcher, wake })
        }

        pub(crate) fn watch(
            &mut self,
            registration: &WatchRegistration,
            _generation: u64,
        ) -> Result<NativeHandle, NativeWatchError> {
            self.watcher
                .watch(registration.path(), RecursiveMode::NonRecursive)
                .map_err(classify_notify_error)?;
            Ok(NativeHandle {
                path: registration.path().to_path_buf(),
            })
        }

        pub(crate) fn unwatch(&mut self, handle: &NativeHandle) -> Result<(), NativeWatchError> {
            self.watcher
                .unwatch(&handle.path)
                .map_err(classify_notify_error)
        }

        pub(crate) fn retargets_by_path(&self) -> bool {
            true
        }

        pub(crate) fn retain_generations(&mut self, _generations: &BTreeSet<u64>) {}

        pub(crate) fn wake(&self) -> Arc<WakeState> {
            Arc::clone(&self.wake)
        }
    }

    fn classify_notify_error(error: notify::Error) -> NativeWatchError {
        let kind = if matches!(
            error.kind,
            notify::ErrorKind::PathNotFound | notify::ErrorKind::WatchNotFound
        ) {
            NativeWatchErrorKind::Lost
        } else {
            NativeWatchErrorKind::Other
        };
        NativeWatchError {
            kind,
            message: error.to_string(),
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("foreground watch builds support only Linux and macOS");

pub(crate) use platform::{NativeHandle, NativeWatcher};
