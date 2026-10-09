//! Initialized-prefix relocation and actual self-append scratch allocation ownership.
use super::*;
use std::io::{ErrorKind, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const TEST: &str = "buffer_self_append_tests::prefix_copy_preserves_bytes_and_removes_scratch";
const CHILD: &str = "ALIGN_BUFFER_SELF_APPEND_CHILD";

struct ChildOwner {
    child: Option<Child>,
    deadline: Instant,
}
impl ChildOwner {
    fn wait(&mut self) -> (ExitStatus, String) {
        loop {
            assert!(
                Instant::now() + Duration::from_secs(5) < self.deadline,
                "buffer child deadline"
            );
            match self.child.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    let mut child = self.child.take().unwrap();
                    let mut stderr = String::new();
                    // Exact native child has no descendants; its only captured output is one
                    // abort diagnostic. All writers have exited before this bounded read.
                    if let Some(pipe) = child.stderr.take() {
                        pipe.take(4096).read_to_string(&mut stderr).unwrap();
                    }
                    return (status, stderr);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("poll buffer child: {error}"),
            }
        }
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
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
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => {
                    eprintln!("buffer child did not reap before deadline");
                    break;
                }
            }
        }
    }
}

fn child(mode: &str, capture: bool) -> (ExitStatus, String) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST, "--nocapture"])
        .env(CHILD, mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(if capture {
            Stdio::piped()
        } else {
            Stdio::inherit()
        });
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setrlimit is async-signal-safe; this isolated abort owner needs no core file.
        unsafe {
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::setrlimit(libc::RLIMIT_CORE, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    ChildOwner {
        child: Some(command.spawn().unwrap()),
        deadline,
    }
    .wait()
}

struct Owner(*mut Buffer);
impl Drop for Owner {
    fn drop(&mut self) {
        unsafe { align_rt_buffer_free(self.0) };
    }
}

#[derive(Clone, Copy, Debug)]
enum Source {
    Full,
    Interior,
    Last,
    Retained,
    External,
    Empty,
    Null,
    Negative,
}

fn matrix() {
    #[cfg(feature = "alloc-count")]
    {
        assert!(!REQUESTED_LIVE_PROBE.lock().unwrap().active);
        let before = global_alloc_count();
        let witness = std::hint::black_box(vec![std::hint::black_box(1u8); 32]);
        assert!(global_alloc_count() > before, "actual allocator witness");
        drop(witness);
    }
    for alignment in [1, 8, 64, 4096, 16384] {
        for size in [8, 65536] {
            let initial: Vec<u8> = (0..size)
                .map(|i| b'a' + u8::try_from(i % 26).unwrap())
                .collect();
            for reserved in [true, false] {
                for source in [
                    Source::Full,
                    Source::Interior,
                    Source::Last,
                    Source::Retained,
                    Source::External,
                    Source::Empty,
                    Source::Null,
                    Source::Negative,
                ] {
                    #[cfg(feature = "alloc-count")]
                    let mut measured = [(0u64, 0usize); 2];
                    for reference in [false, true] {
                        let capacity = if reserved { size * 2 } else { size };
                        let owner = Owner(align_rt_buffer_new(i64::try_from(capacity).unwrap(), alignment, 0));
                        unsafe {
                            align_rt_buffer_append(
                                owner.0,
                                initial.as_ptr(),
                                i64::try_from(size).unwrap(),
                            )
                        };
                        let original = unsafe { (*owner.0).data.as_ptr() };
                        let (offset, length, logical) = match source {
                            Source::Full => (0, size, size),
                            Source::Interior => (2, 3, size),
                            Source::Last => (size - 1, 1, size),
                            Source::Retained => (3, 4, 4),
                            Source::External => (0, 3, size),
                            Source::Empty | Source::Null | Source::Negative => (0, 0, size),
                        };
                        unsafe {
                            (*owner.0).len = logical;
                        }
                        let mut expected = initial[..logical].to_vec();
                        let ptr = match source {
                            Source::External => b"xyz".as_ptr(),
                            Source::Null => core::ptr::null(),
                            _ => unsafe { original.add(offset) },
                        };
                        let native_len = match source {
                            Source::Null => 3,
                            Source::Negative => -1,
                            _ => i64::try_from(length).unwrap(),
                        };
                        if matches!(source, Source::External) {
                            expected.extend_from_slice(b"xyz");
                        } else {
                            expected.extend_from_slice(&initial[offset..offset + length]);
                        }
                        let grows = expected.len() > capacity;
                        if grows {
                            buffer_storage::ALLOCATION_PROBE.set(3);
                        }
                        let aliases = matches!(
                            source,
                            Source::Full | Source::Interior | Source::Last | Source::Retained
                        );
                        #[cfg(feature = "alloc-count")]
                        let before = (global_alloc_count(), global_alloc_bytes());
                        if reference && aliases {
                            // Original behavior: snapshot the raw alias, then append an external slice.
                            let snapshot =
                                unsafe { core::slice::from_raw_parts(ptr, length) }.to_vec();
                            unsafe {
                                align_rt_buffer_append(owner.0, snapshot.as_ptr(), native_len)
                            };
                        } else {
                            unsafe { align_rt_buffer_append(owner.0, ptr, native_len) };
                        }
                        #[cfg(feature = "alloc-count")]
                        {
                            measured[usize::from(reference)] = (
                                global_alloc_count() - before.0,
                                global_alloc_bytes() - before.1,
                            );
                        }
                        let b = unsafe { &*owner.0 };
                        assert_eq!(
                            buffer_storage::ALLOCATION_PROBE.get(),
                            0,
                            "forced relocation consumed"
                        );
                        assert_eq!(
                            b.data.as_slice(),
                            expected,
                            "{source:?}/{alignment}/{size}/{reserved}"
                        );
                        assert_eq!(b.len, expected.len());
                        assert_eq!(b.cap, capacity.max(expected.len()));
                        assert_eq!(
                            b.data.as_ptr().addr() % usize::try_from(alignment).unwrap(),
                            0
                        );
                        if grows {
                            assert_ne!(b.data.as_ptr(), original, "relocation must be observed");
                        } else {
                            assert_eq!(b.data.as_ptr(), original);
                        }
                        #[cfg(feature = "alloc-count")]
                        {
                            let snapshot =
                                aliases && (reference || matches!(source, Source::Retained));
                            assert_eq!(
                                measured[usize::from(reference)],
                                (
                                    u64::from(grows) + u64::from(snapshot),
                                    if grows { b.data.capacity() } else { 0 }
                                        + if snapshot { length } else { 0 }
                                ),
                                "{source:?}/{alignment}/{size}/{reserved}/reference={reference}"
                            );
                        }
                    }
                    // Keep resource evidence compact; all other rows assert the same exact counts.
                    #[cfg(feature = "alloc-count")]
                    if alignment == 64
                        && matches!(
                            source,
                            Source::Full | Source::Retained | Source::External | Source::Empty
                        )
                    {
                        eprintln!(
                            "self-append {source:?} bytes={size} reserved={reserved} candidate={:?} snapshot-reference={:?}",
                            measured[0], measured[1]
                        );
                    }
                }
            }
        }
    }
    // Empty storage never performs arithmetic on a dereferenced allocation or grows.
    let mut empty = BufferStorage::new(16384);
    empty.extend_from_within(0, 0);
    assert_eq!(empty.capacity(), 0);
    unsafe { align_rt_buffer_append(core::ptr::null_mut(), core::ptr::null(), 1) };
}

#[test]
fn prefix_copy_preserves_bytes_and_removes_scratch() {
    match std::env::var(CHILD).as_deref() {
        Ok("matrix") => {
            matrix();
            return;
        }
        Ok(mode @ ("outside" | "overflow" | "allocation-control")) => {
            let mut storage = BufferStorage::new(64);
            storage.reserve_exact(3);
            storage.extend_from_slice(b"abc");
            buffer_storage::ALLOCATION_PROBE.set(2);
            match mode {
                "outside" => storage.extend_from_within(3, 1),
                "overflow" => storage.extend_from_within(usize::MAX, 1),
                _ => storage.extend_from_within(0, 3),
            }
            panic!("invalid helper or allocation failpoint did not abort");
        }
        _ => {}
    }
    assert!(child("matrix", false).0.success());
    for mode in ["outside", "overflow", "allocation-control"] {
        let (status, stderr) = child(mode, true);
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(status.signal(), Some(libc::SIGABRT));
        }
        assert!(!status.success());
        let expected = if mode == "allocation-control" {
            "buffer test allocation reached"
        } else {
            "buffer copy source is outside initialized prefix"
        };
        assert!(stderr.contains(expected), "{mode}: {stderr}");
    }
}
