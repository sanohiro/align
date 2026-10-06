//! Whole-file reads retain the first descriptor, including special-file fallbacks.
use super::*;
use std::io::{ErrorKind, Write};
use std::os::unix::{ffi::OsStrExt, fs::OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "align-whole-read-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .expect("exclusive read fixture");
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Owned(AlignStr);
impl Owned {
    fn bytes(&self) -> &[u8] {
        unsafe { bytes_view(self.0.ptr, self.0.len) }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { align_rt_free(self.0.ptr.cast_mut()) };
    }
}
struct Region(*mut Arena);
impl Region {
    fn new() -> Self {
        Self(align_rt_arena_begin())
    }
}
impl Drop for Region {
    fn drop(&mut self) {
        unsafe { align_rt_arena_end(self.0) };
    }
}
fn empty() -> AlignStr {
    AlignStr {
        ptr: core::ptr::null(),
        len: 0,
    }
}

#[test]
fn raw_fill_admits_only_complete_initialized_prefixes() {
    // Chunks are interleaved with EINTR; EOF, I/O failure and invalid counts never publish.
    for (steps, expected, initialized) in [
        (vec![Ok(2), Err(ErrorKind::Interrupted), Ok(3)], None, 5),
        (vec![Err(ErrorKind::Interrupted), Ok(5)], None, 5),
        (vec![Ok(2), Ok(0)], Some(ErrorKind::UnexpectedEof), 2),
        (
            vec![Ok(2), Err(ErrorKind::PermissionDenied)],
            Some(ErrorKind::PermissionDenied),
            2,
        ),
        (vec![Ok(2), Ok(4)], Some(ErrorKind::InvalidData), 2),
    ] {
        let mut storage = [core::mem::MaybeUninit::new(0xa5); 7];
        let start = storage[1..].as_mut_ptr().cast::<u8>();
        let mut cursor = 0;
        let mut steps = steps.into_iter();
        let result = unsafe {
            fs_read_exact_raw(start, 5, |dst, remaining| {
                assert_eq!(dst, start.add(cursor));
                assert_eq!(remaining, 5 - cursor);
                let step = steps.next().expect("scripted raw read");
                if let Ok(count) = step
                    && count <= remaining
                {
                    for offset in 0..count {
                        dst.add(offset)
                            .write(b'a' + u8::try_from(cursor + offset).unwrap());
                    }
                    cursor += count;
                }
                step.map_err(Into::into)
            })
        };
        assert_eq!(result.err().map(|e| e.kind()), expected);
        assert!(steps.next().is_none());
        let actual = storage.map(|byte| unsafe { byte.assume_init() });
        assert_eq!(actual[0], 0xa5);
        assert_eq!(&actual[1..1 + initialized], &b"abcde"[..initialized]);
        assert!(actual[1 + initialized..].iter().all(|byte| *byte == 0xa5));
    }
    unsafe { fs_read_exact_raw(core::ptr::null_mut(), 0, |_, _| panic!("empty read")) }.unwrap();
}

#[test]
fn owned_snapshot_fallback_rewinds_the_original_file() {
    let fixture = Fixture::new();
    let path = fixture.0.join("source");
    for original in [b"original".as_slice(), b"", b"\xffbad"] {
        for expected in [
            None,
            Some(0),
            Some(1),
            Some(original.len()),
            Some(original.len() + 8),
        ] {
            std::fs::write(&path, original).unwrap();
            let file = std::fs::File::open(&path).unwrap();
            std::fs::remove_file(&path).unwrap();
            std::fs::write(&path, b"replacement").unwrap();
            let result = fs_read_owned(file, expected);
            if validate_utf8(original) {
                let result = Owned(result.unwrap_or_else(|status| panic!("read status {status}")));
                assert_eq!(result.bytes(), original, "snapshot {expected:?}");
                if original.is_empty() {
                    assert!(result.0.ptr.is_null());
                }
            } else {
                assert!(matches!(result, Err(AL_INVALID)));
            }
            assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        }
    }
    // Read/rewind failure abandons the direct allocation and never publishes output.
    let directory = std::fs::File::open(&fixture.0).unwrap();
    assert!(matches!(fs_read_owned(directory, Some(4)), Err(1)));
}

#[test]
fn arena_fallback_uses_original_descriptor_and_validates_before_allocation() {
    let fixture = Fixture::new();
    let path = fixture.0.join("source");
    for original in [b"original".as_slice(), b"", b"\xffbad"] {
        for validate in [false, true] {
            std::fs::write(&path, original).unwrap();
            let file = std::fs::File::open(&path).unwrap();
            std::fs::remove_file(&path).unwrap();
            std::fs::write(&path, b"replacement").unwrap();
            let arena = Region::new();
            let mut out = empty();
            let status = unsafe { read_file_view_into_arena(file, arena.0, &mut out, validate) };
            if validate && !validate_utf8(original) {
                assert_eq!(status, AL_INVALID);
                assert!(out.ptr.is_null() && out.len == 0);
            } else {
                assert_eq!(status, 0);
                assert_eq!(unsafe { bytes_view(out.ptr, out.len) }, original);
                if original.is_empty() {
                    assert!(out.ptr.is_null());
                }
            }
        }
    }
}

struct ReaderChild {
    child: Option<std::process::Child>,
    deadline: Instant,
}
impl ReaderChild {
    fn tick(&self) {
        assert!(
            Instant::now() + Duration::from_secs(5) < self.deadline,
            "FIFO reader exceeded work deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    fn wait(&mut self) {
        loop {
            self.tick();
            match self.child.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    self.child.take();
                    assert!(status.success(), "FIFO child: {status}");
                    return;
                }
                Ok(None) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("poll FIFO child: {error}"),
            }
        }
    }
}
impl Drop for ReaderChild {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // The exact-filter child opens files only and has no descendants.
        loop {
            match child.kill() {
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => break,
            }
        }
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => {
                    eprintln!("FIFO child did not reap before deadline");
                    return;
                }
            }
        }
    }
}

fn write_once(path: &Path, payload: &[u8], child: &ReaderChild) {
    let mut writer = loop {
        child.tick();
        match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => break file,
            Err(error)
                if error.raw_os_error() == Some(libc::ENXIO)
                    || error.kind() == ErrorKind::Interrupted => {}
            Err(error) => panic!("open FIFO writer: {error}"),
        }
    };
    // Payloads fit PIPE_BUF; the nonblocking write is bounded, with exactly one writer lifetime.
    let mut rest = payload;
    while !rest.is_empty() {
        child.tick();
        match writer.write(rest) {
            Ok(0) => panic!("zero FIFO write"),
            Ok(count) => rest = &rest[count..],
            Err(error)
                if matches!(error.kind(), ErrorKind::Interrupted | ErrorKind::WouldBlock) => {}
            Err(error) => panic!("write FIFO: {error}"),
        }
    }
}

#[test]
fn fifo_readers_complete_after_one_writer_closes() {
    const ENV: &str = "ALIGN_WHOLE_FILE_FIFO_CHILD";
    let payloads = [
        b"".as_slice(),
        "single writer 日本語\n".as_bytes(),
        b"\xff\x00\x80",
    ];
    if let Some(root) = std::env::var_os(ENV) {
        let root = PathBuf::from(root);
        for mode in 0..3 {
            for (case, expected) in payloads.iter().enumerate() {
                let path = root.join(format!("fifo-{mode}-{case}"));
                let path = path.as_os_str().as_bytes();
                let len = i64::try_from(path.len()).unwrap();
                let arena = Region::new();
                let mut owned = Owned(empty());
                let mut view = empty();
                let (status, out) = unsafe {
                    match mode {
                        0 => (
                            align_rt_fs_read_file(path.as_ptr(), len, &mut owned.0),
                            &owned.0,
                        ),
                        1 => (
                            align_rt_fs_read_file_view(path.as_ptr(), len, arena.0, &mut view),
                            &view,
                        ),
                        _ => (
                            align_rt_fs_read_bytes_view(path.as_ptr(), len, arena.0, &mut view),
                            &view,
                        ),
                    }
                };
                if mode != 2 && case == 2 {
                    assert_eq!(status, AL_INVALID);
                    assert!(out.ptr.is_null() && out.len == 0);
                } else {
                    assert_eq!(status, 0);
                    assert_eq!(unsafe { bytes_view(out.ptr, out.len) }, *expected);
                    if expected.is_empty() {
                        assert!(out.ptr.is_null());
                    }
                }
            }
        }
        return;
    }
    let fixture = Fixture::new();
    for mode in 0..3 {
        for case in 0..payloads.len() {
            let path = fixture.0.join(format!("fifo-{mode}-{case}"));
            let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        }
    }
    let mut child = ReaderChild {
        child: Some(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "fs_read_tests::fifo_readers_complete_after_one_writer_closes",
                    "--nocapture",
                ])
                .env(ENV, &fixture.0)
                .stdin(std::process::Stdio::null())
                .spawn()
                .expect("spawn FIFO reader"),
        ),
        deadline: Instant::now() + Duration::from_secs(20),
    };
    for mode in 0..3 {
        for (case, payload) in payloads.iter().enumerate() {
            write_once(
                &fixture.0.join(format!("fifo-{mode}-{case}")),
                payload,
                &child,
            );
        }
    }
    child.wait();
}

#[cfg(feature = "alloc-count")]
#[test]
fn direct_payload_allocation_and_abandoned_buffers_balance() {
    const ENV: &str = "ALIGN_WHOLE_FILE_ALLOCATION_CHILD";
    if std::env::var_os(ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
        let mut child = ReaderChild {
            child: Some(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "fs_read_tests::direct_payload_allocation_and_abandoned_buffers_balance",
                        "--nocapture",
                    ])
                    .env(ENV, "1")
                    .stdin(std::process::Stdio::null())
                    .spawn()
                    .expect("spawn allocation owner"),
            ),
            deadline: Instant::now() + Duration::from_secs(20),
        };
        child.wait();
        return;
    }
    // Isolate process-global runtime counts and keep the requested-live map inactive, so its
    // instrumentation cannot add Rust allocations or interfere with other libtest owners.
    assert!(!REQUESTED_LIVE_PROBE.lock().unwrap().active);
    let witness = align_rt_alloc(1);
    unsafe { align_rt_free(witness) };
    let fixture = Fixture::new();
    let path = fixture.0.join("source");
    std::fs::write(&path, b"original").unwrap();
    let raw = path.as_os_str().as_bytes();
    let length = i64::try_from(raw.len()).unwrap();
    let mut result = Owned(empty());
    let allocated = align_rt_alloc_count();
    let rust_allocated = global_alloc_count();
    let status = unsafe { align_rt_fs_read_file(raw.as_ptr(), length, &mut result.0) };
    let rust_delta = global_alloc_count() - rust_allocated;
    assert_eq!(status, 0);
    assert_eq!(align_rt_alloc_count() - allocated, 1);
    assert_eq!(
        rust_delta, 0,
        "ordinary regular file needs no temporary payload"
    );
    drop(result);

    for original in [b"original".as_slice(), b"", b"\xffbad"] {
        for expected in [
            None,
            Some(0),
            Some(1),
            Some(original.len()),
            Some(original.len() + 8),
        ] {
            std::fs::write(&path, original).unwrap();
            let file = std::fs::File::open(&path).unwrap();
            let allocated = align_rt_alloc_count();
            let freed = align_rt_free_count();
            let result = fs_read_owned(file, expected).map(Owned);
            drop(result);
            assert_eq!(
                align_rt_alloc_count() - allocated,
                align_rt_free_count() - freed,
                "payload balance for {original:?}, snapshot {expected:?}"
            );
        }
    }
    let directory = std::fs::File::open(&fixture.0).unwrap();
    let allocated = align_rt_alloc_count();
    let freed = align_rt_free_count();
    assert!(matches!(fs_read_owned(directory, Some(4)), Err(1)));
    assert_eq!(align_rt_alloc_count() - allocated, 1);
    assert_eq!(align_rt_free_count() - freed, 1);
}

#[test]
fn entry_admission_preserves_status_and_empty_output() {
    let region = Region::new();
    for (path, len, expected) in [
        (b"\xff".as_slice(), 1, AL_INVALID),
        (b"a\0b".as_slice(), 3, AL_INVALID),
        (b"".as_slice(), -1, AL_NOT_FOUND),
    ] {
        let mut owned = Owned(empty());
        assert_eq!(
            unsafe { align_rt_fs_read_file(path.as_ptr(), len, &mut owned.0) },
            1
        );
        assert!(owned.0.ptr.is_null() && owned.0.len == 0);
        for validate in [false, true] {
            let mut view = AlignStr {
                ptr: path.as_ptr(),
                len: 1,
            };
            assert_eq!(
                unsafe { fs_read_view_impl(path.as_ptr(), len, region.0, &mut view, validate) },
                expected
            );
            assert!(view.ptr.is_null() && view.len == 0);
        }
    }
    assert_eq!(
        unsafe { align_rt_fs_read_file(b"x".as_ptr(), 1, core::ptr::null_mut()) },
        1
    );
    assert_eq!(
        unsafe { fs_read_view_impl(b"x".as_ptr(), 1, region.0, core::ptr::null_mut(), true) },
        AL_INVALID
    );
    let mut view = empty();
    assert_eq!(
        unsafe { fs_read_view_impl(b"x".as_ptr(), 1, core::ptr::null_mut(), &mut view, true) },
        AL_INVALID
    );
    assert!(view.ptr.is_null() && view.len == 0);
}
