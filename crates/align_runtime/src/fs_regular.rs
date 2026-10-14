//! Ordinary-path regular-file admission; the opened descriptor owns the kind proof.
use super::{AL_INVALID, BeneathFd, Reader, RwFile, io_error_to_status, safe_len};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    Open,
    Stat,
    GetFlags,
    SetFlags,
}

fn syscall(_step: Step, mut action: impl FnMut() -> i32) -> Result<i32, i32> {
    loop {
        let call = || {
            let result = action();
            if result < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(result)
            }
        };
        #[cfg(test)]
        let result = test_state::error(_step).map_or_else(call, Err);
        #[cfg(not(test))]
        let result = {
            let mut call = call;
            call()
        };
        match result {
            Ok(value) => return Ok(value),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error_to_status(&error)),
        }
    }
}

fn acquire(path: &std::ffi::CStr) -> Result<BeneathFd, i32> {
    let fd = BeneathFd(syscall(Step::Open, || unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    })?);
    #[cfg(test)]
    test_state::opened(fd.0);
    let mut metadata = core::mem::MaybeUninit::<libc::stat>::zeroed();
    syscall(Step::Stat, || unsafe {
        libc::fstat(fd.0, metadata.as_mut_ptr())
    })?;
    // SAFETY: successful fstat initialized the complete record of this retained descriptor.
    let metadata = unsafe { metadata.assume_init() };
    if metadata.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(AL_INVALID);
    }
    let flags = syscall(Step::GetFlags, || unsafe {
        libc::fcntl(fd.0, libc::F_GETFL)
    })?;
    syscall(Step::SetFlags, || unsafe {
        libc::fcntl(fd.0, libc::F_SETFL, flags & !libc::O_NONBLOCK)
    })?;
    Ok(fd)
}

/// Numerical preflight precedes every byte read and output write. Live ranges remain the caller's
/// unsafe obligation. An invalid range/overlap leaves output untouched; later errors leave null.
pub(super) unsafe fn open(path: *const u8, len: i64, out: *mut *mut Reader) -> i32 {
    unsafe { open_with(path, len, out, |fd| Reader::unbuffered(fd.into_raw(), true)) }
}

pub(super) unsafe fn open_file(path: *const u8, len: i64, out: *mut *mut RwFile) -> i32 {
    unsafe { open_with(path, len, out, |fd| RwFile { fd: fd.into_raw() }) }
}

unsafe fn open_with<T>(
    path: *const u8,
    len: i64,
    out: *mut *mut T,
    publish: impl FnOnce(BeneathFd) -> T,
) -> i32 {
    let out_start = out.addr();
    if out.is_null() || !out_start.is_multiple_of(core::mem::align_of::<*mut T>()) {
        return AL_INVALID;
    }
    let Some(out_end) = out_start.checked_add(core::mem::size_of::<*mut T>()) else {
        return AL_INVALID;
    };
    let Ok(n) = safe_len(len) else {
        return AL_INVALID;
    };
    if n.checked_add(1)
        .is_none_or(|size| isize::try_from(size).is_err())
        || (n > 0 && path.is_null())
    {
        return AL_INVALID;
    }
    let Some(path_end) = path.addr().checked_add(n) else {
        return AL_INVALID;
    };
    if n > 0 && path.addr() < out_end && out_start < path_end {
        return AL_INVALID;
    }
    // SAFETY: the output is a caller-owned writable, aligned slot, disjoint from the input.
    unsafe {
        *out = core::ptr::null_mut();
    }
    let bytes = if n == 0 {
        &[][..]
    } else {
        // SAFETY: positive input is nonnull and numerically bounded; liveness is the ABI contract.
        unsafe { core::slice::from_raw_parts(path, n) }
    };
    if std::str::from_utf8(bytes).is_err() || bytes.contains(&0) {
        return AL_INVALID;
    }
    let Ok(path) = std::ffi::CString::new(bytes) else {
        return AL_INVALID;
    };
    match acquire(&path) {
        Ok(fd) => {
            let owner = Box::new(publish(fd));
            unsafe {
                *out = Box::into_raw(owner);
            }
            0
        }
        Err(status) => status,
    }
}

#[cfg(test)]
mod test_state {
    use super::Step;
    use core::cell::{Cell, RefCell};
    thread_local! {
        pub(super) static FAILURE: Cell<Option<(Step, i32)>> = const { Cell::new(None) };
        pub(super) static LAST_FD: Cell<i32> = const { Cell::new(-1) };
        pub(super) static CALLS: Cell<usize> = const { Cell::new(0) };
        pub(super) static AFTER_OPEN: RefCell<Option<Box<dyn FnOnce(i32)>>> = const { RefCell::new(None) };
    }
    pub(super) fn error(step: Step) -> Option<std::io::Error> {
        CALLS.with(|calls| calls.set(calls.get() + 1));
        FAILURE.with(|failure| {
            let (at, errno) = failure.get()?;
            if at != step {
                return None;
            }
            failure.set(None);
            Some(std::io::Error::from_raw_os_error(errno))
        })
    }
    pub(super) fn opened(fd: i32) {
        LAST_FD.with(|value| value.set(fd));
        AFTER_OPEN.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook(fd);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, PermissionsExt},
    };
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let nonce = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("align-regular-{}-{nonce}", std::process::id()));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .expect("private fixture");
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[derive(Clone, Copy, Debug)]
    enum Publisher {
        Reader,
        File,
    }
    impl Publisher {
        unsafe fn open(self, path: *const u8, len: i64, out: *mut *mut ()) -> i32 {
            unsafe {
                match self {
                    Self::Reader => crate::align_rt_io_reader_open_regular(path, len, out.cast()),
                    Self::File => crate::align_rt_io_file_open_ro_regular(path, len, out.cast()),
                }
            }
        }
    }
    struct Owner {
        pointer: *mut (),
        publisher: Publisher,
    }
    impl Owner {
        fn fd(&self) -> i32 {
            unsafe {
                match self.publisher {
                    Publisher::Reader => (*self.pointer.cast::<Reader>()).fd,
                    Publisher::File => (*self.pointer.cast::<RwFile>()).fd,
                }
            }
        }
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            unsafe {
                match self.publisher {
                    Publisher::Reader => crate::align_rt_io_reader_free(self.pointer.cast()),
                    Publisher::File => crate::align_rt_io_file_free(self.pointer.cast()),
                }
            }
        }
    }
    fn open_path(publisher: Publisher, path: &std::path::Path) -> (i32, Owner) {
        let bytes = path.as_os_str().as_bytes();
        let mut pointer = core::ptr::null_mut();
        let status = unsafe {
            publisher.open(
                bytes.as_ptr(),
                i64::try_from(bytes.len()).unwrap(),
                &mut pointer,
            )
        };
        (status, Owner { pointer, publisher })
    }
    fn assert_closed(fd: i32) {
        assert!(fd >= 0, "positive acquisition witness required");
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EBADF)
        );
    }

    #[test]
    fn regular_reader_abi_preflight() {
        for publisher in [Publisher::Reader, Publisher::File] {
            let sentinel = core::ptr::without_provenance_mut::<()>(1);
            for (path, len) in [
                (core::ptr::null(), 1),
                (b"x".as_ptr(), -1),
                (b"x".as_ptr(), i64::MAX),
                (core::ptr::without_provenance::<u8>(usize::MAX), 2),
            ] {
                let mut out = sentinel;
                assert_eq!(unsafe { publisher.open(path, len, &mut out) }, AL_INVALID);
                assert_eq!(out, sentinel);
            }
            assert_eq!(
                unsafe { publisher.open(b"x".as_ptr(), 1, core::ptr::null_mut()) },
                AL_INVALID
            );
            let overflowing_output = core::ptr::without_provenance_mut::<*mut ()>(usize::MAX - 7);
            assert_eq!(
                unsafe { publisher.open(core::ptr::null(), -1, overflowing_output) },
                AL_INVALID
            );
            let mut empty = sentinel;
            assert_eq!(
                unsafe { publisher.open(core::ptr::null(), 0, &mut empty) },
                crate::AL_NOT_FOUND
            );
            assert!(empty.is_null());
            let mut storage = [usize::MAX; 2];
            let bytes = storage.as_mut_ptr().cast::<u8>();
            assert_eq!(
                unsafe { publisher.open(bytes, 8, bytes.cast()) },
                AL_INVALID
            );
            assert_eq!(storage, [usize::MAX; 2], "overlap must not clear the input");
            assert_eq!(
                unsafe { publisher.open(b"x".as_ptr(), 1, bytes.add(1).cast()) },
                AL_INVALID
            );
            for bytes in [&b"a\0b"[..], &b"\xff"[..]] {
                test_state::CALLS.with(|calls| calls.set(0));
                let mut out = sentinel;
                assert_eq!(
                    unsafe {
                        publisher.open(
                            bytes.as_ptr(),
                            i64::try_from(bytes.len()).unwrap(),
                            &mut out,
                        )
                    },
                    AL_INVALID
                );
                assert!(out.is_null());
                assert_eq!(test_state::CALLS.with(|value| value.get()), 0);
            }
        }
    }

    // Keep descriptor-number assertions and the FIFO timeout isolated from other libtest threads.
    #[test]
    fn regular_reader_native_matrix() {
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        for (publisher, mode) in [
            ("reader", "matrix"),
            ("reader", "fifo"),
            ("file", "matrix"),
            ("file", "fifo"),
        ] {
            let mut child = Child(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--ignored",
                        "--exact",
                        "fs_regular::tests::regular_reader_native_child",
                        "--test-threads=1",
                    ])
                    .env("ALIGN_REGULAR_READER_CHILD", mode)
                    .env("ALIGN_REGULAR_PUBLISHER", publisher)
                    .stdout(std::process::Stdio::inherit())
                    .spawn()
                    .expect("spawn regular-reader owner"),
            );
            let deadline = std::time::Instant::now()
                + std::time::Duration::from_secs(if mode == "fifo" { 1 } else { 10 });
            loop {
                if let Some(status) = child.0.try_wait().expect("poll child") {
                    assert!(
                        status.success(),
                        "regular-reader {mode} child failed: {status}"
                    );
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "regular-reader {mode} exceeded deadline"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    #[test]
    #[ignore = "isolated subprocess of regular_reader_native_matrix"]
    fn regular_reader_native_child() {
        let mode = std::env::var("ALIGN_REGULAR_READER_CHILD").expect("explicit child mode");
        let publisher = match std::env::var("ALIGN_REGULAR_PUBLISHER").as_deref() {
            Ok("reader") => Publisher::Reader,
            Ok("file") => Publisher::File,
            _ => panic!("explicit publisher required"),
        };
        let fixture = Fixture::new();
        let root = std::fs::canonicalize(&fixture.0).unwrap();
        let fifo = root.join("fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        if mode == "fifo" {
            let (status, owner) = open_path(publisher, &fifo);
            assert_eq!(status, AL_INVALID);
            assert!(owner.pointer.is_null());
            assert_closed(test_state::LAST_FD.with(|value| value.get()));
            return;
        }
        assert_eq!(mode, "matrix");
        let data = root.join("data");
        std::fs::write(&data, b"hello").unwrap();
        std::fs::write(root.join("empty"), b"").unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o400)).unwrap();
        std::os::unix::fs::symlink("data", root.join("link")).unwrap();
        std::os::unix::fs::symlink("missing", root.join("dangling")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("ancestor")).unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap(); // isolated single-test child owns cwd
        for (path, size) in [
            ("data", 5),
            ("./data", 5),
            ("link", 5),
            ("ancestor/data", 5),
            ("empty", 0),
        ] {
            let (status, owner) = open_path(publisher, std::path::Path::new(path));
            assert_eq!(status, 0, "{path}");
            let fd = owner.fd();
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_GETFL) } & libc::O_NONBLOCK,
                0
            );
            assert_ne!(
                unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
                0
            );
            let mut bytes = [0u8; 8];
            assert_eq!(
                unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) },
                size
            );
            if size > 0 {
                assert_eq!(&bytes[..5], b"hello");
            }
            drop(owner);
            assert_closed(fd);
        }
        std::env::set_current_dir(cwd).unwrap();
        for path in [root.clone(), std::path::PathBuf::from("/dev/null")] {
            let (status, owner) = open_path(publisher, &path);
            assert_eq!(status, AL_INVALID);
            assert!(owner.pointer.is_null());
            assert_closed(test_state::LAST_FD.with(|value| value.get()));
        }
        for path in [
            root.join("missing"),
            root.join("dangling"),
            std::path::PathBuf::new(),
        ] {
            let (status, owner) = open_path(publisher, &path);
            assert_eq!(
                status,
                io_error_to_status(&std::io::Error::from_raw_os_error(libc::ENOENT))
            );
            assert!(owner.pointer.is_null());
        }
        for step in [Step::Open, Step::Stat, Step::GetFlags, Step::SetFlags] {
            test_state::LAST_FD.with(|fd| fd.set(-1));
            test_state::FAILURE.with(|failure| failure.set(Some((step, libc::EIO))));
            let (status, owner) = open_path(publisher, &data);
            assert_eq!(
                status,
                io_error_to_status(&std::io::Error::from_raw_os_error(libc::EIO)),
                "{step:?}"
            );
            assert!(owner.pointer.is_null());
            if step != Step::Open {
                assert_closed(test_state::LAST_FD.with(|value| value.get()));
            }
            test_state::FAILURE.with(|failure| failure.set(Some((step, libc::EINTR))));
            let (status, owner) = open_path(publisher, &data);
            assert_eq!(status, 0, "retry {step:?}");
            assert!(!owner.pointer.is_null());
        }
        // Replace a pathname after open: kind and bytes must follow the retained descriptor.
        let selected = data.clone();
        let replacement = fifo.clone();
        test_state::AFTER_OPEN.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |_| {
                std::fs::rename(&replacement, &selected).unwrap();
            }))
        });
        let (status, owner) = open_path(publisher, &data);
        assert_eq!(status, 0);
        let mut bytes = [0u8; 5];
        assert_eq!(
            unsafe { libc::read(owner.fd(), bytes.as_mut_ptr().cast(), bytes.len()) },
            5
        );
        assert_eq!(&bytes, b"hello");
        if let Publisher::File = publisher {
            // Cursor now sits at EOF. Positional operations must still use this admitted fd.
            let file = owner.pointer.cast::<RwFile>();
            let buffer = crate::align_rt_buffer_new(8, 1, 0);
            unsafe {
                assert_eq!(crate::align_rt_io_file_len(file), 5);
                assert_eq!(crate::align_rt_io_file_pread_into(file, buffer, 0, 8, 2), 3);
                assert_eq!((*buffer).data.as_slice(), b"llo");
                assert_eq!(crate::align_rt_io_file_pread_into(file, buffer, 0, 8, 5), 0);
                assert_eq!((*buffer).data.as_slice(), b"llo");
                assert_eq!(crate::align_rt_io_file_pread(file, buffer, 0), 5);
                assert_eq!((*buffer).data.as_slice(), b"hello");
                assert_eq!(
                    crate::align_rt_io_file_pwrite(file, b"x".as_ptr(), 1, 0),
                    -i64::from(crate::AL_DENIED)
                );
                assert_eq!(
                    crate::align_rt_io_file_pwrite(file, core::ptr::null(), 0, 0),
                    -i64::from(crate::AL_DENIED)
                );
                assert_eq!(libc::lseek(owner.fd(), 0, libc::SEEK_CUR), 5);
                crate::align_rt_buffer_free(buffer);
            }
        }
        drop(owner);
        // The inverse replacement must still refuse the opened FIFO, even though its path is now regular.
        let replacement = root.join("replacement");
        std::fs::write(&replacement, b"regular").unwrap();
        let selected = data.clone();
        test_state::AFTER_OPEN.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |_| {
                std::fs::rename(&replacement, &selected).unwrap();
            }))
        });
        let (status, owner) = open_path(publisher, &data);
        assert_eq!(status, AL_INVALID);
        assert!(owner.pointer.is_null());
        assert_closed(test_state::LAST_FD.with(|value| value.get()));
    }
}
