// Included in the native owner module to reuse its exclusively owned file/handle fixtures.

#[test]
fn aligned_buffer_construction_growth_reads_and_matching_drop() {
    use crate::buffer_storage::{AllocationEvent, allocation_event_count, observe_allocations};
    use std::os::fd::{AsRawFd, IntoRawFd};
    let root = FileFixtureDir::new("aligned-buffer");
    let path = root.0.join("input");
    let mut input = vec![b'x'; 8193];
    input.extend_from_slice(b"\r\nlast");
    std::fs::write(&path, &input).unwrap();
    for alignment in [1, 8, 64, 4096, 16384] {
        for filled in [false, true] {
            let events = observe_allocations(|| {
                let owner = BufferTestHandle(if filled {
                    align_rt_buffer_filled(3, 0xa5, alignment)
                } else {
                    align_rt_buffer_new(3, alignment)
                });
                assert_eq!(
                    allocation_event_count(),
                    1,
                    "one constructor payload allocation"
                );
                let inspect = |owner: &BufferTestHandle| {
                    let buffer = unsafe { &*owner.0 };
                    assert_eq!(
                        buffer.data.writable_ptr().addr() % usize::try_from(alignment).unwrap(),
                        0
                    );
                    assert!(buffer.len <= buffer.cap && buffer.cap <= buffer.data.capacity());
                    buffer.data.writable_ptr()
                };
                inspect(&owner);
                unsafe {
                    align_rt_buffer_put(owner.0, 0x04030201, 4, 0);
                    align_rt_buffer_append(owner.0, b"hello".as_ptr(), 5);
                    align_rt_buffer_append_filled(owner.0, 9000, 0x5a);
                }
                inspect(&owner);
                let before = unsafe { (*owner.0).data.to_vec() };
                let ptr = inspect(&owner);
                unsafe {
                    align_rt_buffer_append(owner.0, ptr, i64::try_from(before.len()).unwrap())
                };
                inspect(&owner);
                assert_eq!(unsafe { (*owner.0).data.as_slice() }, before.repeat(2));
                let events_before_move = allocation_event_count();
                // Moving the native owner moves only its handle, with no payload acquisition.
                let moved = owner;
                let pointer = inspect(&moved);
                let file_owner = std::fs::File::open(&path).unwrap();
                let mut file = RwFile {
                    fd: file_owner.as_raw_fd(),
                };
                assert_eq!(
                    unsafe { align_rt_io_file_pread_into(&mut file, moved.0, 2, 7, 0) },
                    7
                );
                assert_eq!(inspect(&moved), pointer);
                assert_eq!(unsafe { &(*moved.0).data.as_slice()[2..9] }, b"xxxxxxx");
                assert_eq!(
                    unsafe { align_rt_io_file_pread_into(&mut file, moved.0, 0, 1, 99999) },
                    0
                );
                assert_eq!(inspect(&moved), pointer);
                drop(file_owner);
                file.fd = -1;
                assert!(unsafe { align_rt_io_file_pread_into(&mut file, moved.0, 0, 1, 0) } < 0);
                assert_eq!(inspect(&moved), pointer);
                assert_eq!(
                    allocation_event_count(),
                    events_before_move,
                    "move and bounded reads retain the allocation"
                );
            });
            let mut live = std::collections::BTreeMap::new();
            for AllocationEvent(acquire, pointer, size, selected) in events {
                assert_eq!(selected, usize::try_from(alignment).unwrap());
                if acquire {
                    assert!(live.insert(pointer, (size, selected)).is_none());
                } else {
                    assert_eq!(live.remove(&pointer), Some((size, selected)));
                }
            }
            assert!(live.is_empty());
        }
        let events = observe_allocations(|| {
            let empty = BufferTestHandle(align_rt_buffer_filled(0, 0, alignment));
            assert_eq!(
                unsafe { (*empty.0).data.writable_ptr().addr() }
                    % usize::try_from(alignment).unwrap(),
                0
            );
            let failed = BufferTestHandle(align_rt_buffer_new(i64::MAX, alignment));
            assert_eq!(unsafe { align_rt_buffer_capacity(failed.0) }, 0);
        });
        assert!(events.is_empty(), "empty/failed hints acquire no payload");
        let reader = ReaderTestHandle(Box::into_raw(Box::new(Reader::unbuffered(
            std::fs::File::open(&path).unwrap().into_raw_fd(),
            true,
        ))));
        let owner = BufferTestHandle(align_rt_buffer_new(1, alignment));
        unsafe { align_rt_io_reader_buffered(reader.0) };
        assert_eq!(
            unsafe { align_rt_io_reader_read_line(reader.0, owner.0) },
            8195
        );
        assert_eq!(unsafe { (*owner.0).data.as_slice() }, &input[..8193]);
        let pointer = unsafe { (*owner.0).data.writable_ptr() };
        assert_eq!(pointer.addr() % usize::try_from(alignment).unwrap(), 0);
        assert_eq!(
            unsafe { align_rt_io_reader_read_line(reader.0, owner.0) },
            4
        );
        assert_eq!(unsafe { (*owner.0).data.as_slice() }, b"last");
        assert_eq!(
            unsafe { align_rt_io_reader_read_line(reader.0, owner.0) },
            0
        );
        assert_eq!(unsafe { (*owner.0).data.writable_ptr() }, pointer);
    }
}

#[test]
fn aligned_buffer_invalid_alignment_precedes_every_allocation() {
    use std::os::unix::process::ExitStatusExt;
    use std::time::{Duration, Instant};
    const CHILD: &str = "ALIGN_BUFFER_ALIGNMENT_CHILD";
    let bad = [0, -1, 3, (1 << 29) + 1, 1 << 30, i64::MIN, i64::MAX];
    let sizes = [0, -1, 1, i64::MAX];
    if let Ok(value) = std::env::var(CHILD) {
        let index: usize = value.parse().unwrap();
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) }, 0);
        OWNED_ALLOC_FAIL_AFTER.store(0, core::sync::atomic::Ordering::Relaxed);
        crate::buffer_storage::ALLOCATION_PROBE.set(2);
        if index == 56 {
            align_rt_buffer_filled(1, 0, 1);
        } else if index == 57 {
            align_rt_buffer_new(0, 1);
        } else if index == 58 {
            align_rt_buffer_new(1, 1);
        } else {
            let alignment = bad[index / 8];
            let size = sizes[(index / 2) % 4];
            if index % 2 == 0 {
                align_rt_buffer_new(size, alignment);
            } else {
                align_rt_buffer_filled(size, 0, alignment);
            }
        }
        panic!("invalid alignment or active allocation witness returned");
    }
    struct Child {
        process: std::process::Child,
        deadline: Instant,
        reaped: bool,
    }
    impl Child {
        fn wait(&mut self) -> std::process::ExitStatus {
            loop {
                assert!(
                    Instant::now() + Duration::from_secs(2) < self.deadline,
                    "alignment child stalled"
                );
                match self.process.try_wait() {
                    Ok(Some(status)) => {
                        self.reaped = true;
                        return status;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => panic!("wait: {error}"),
                }
            }
        }
    }
    impl Drop for Child {
        fn drop(&mut self) {
            if self.reaped {
                return;
            }
            loop {
                match self.process.kill() {
                    Err(error)
                        if error.kind() == std::io::ErrorKind::Interrupted
                            && Instant::now() < self.deadline => {}
                    _ => break,
                }
            }
            while Instant::now() < self.deadline {
                match self.process.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    _ => return,
                }
            }
        }
    }
    let root = FileFixtureDir::new("aligned-admission");
    for index in 0..59 {
        let stderr = root.0.join(format!("{index}.err"));
        let process = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::aligned_buffer_invalid_alignment_precedes_every_allocation",
                "--nocapture",
            ])
            .env(CHILD, index.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create_new(&stderr).unwrap())
            .spawn()
            .unwrap();
        let mut child = Child {
            process,
            deadline: Instant::now() + Duration::from_secs(10),
            reaped: false,
        };
        let status = child.wait();
        assert_eq!(status.signal(), Some(libc::SIGABRT), "case {index}");
        assert!(std::fs::metadata(&stderr).unwrap().len() < 16384);
        let output = std::fs::read_to_string(stderr).unwrap();
        let expected = if index < 56 {
            "buffer alignment must be a power of two between 1 and 536870912"
        } else if index == 56 {
            "buffer allocation failed"
        } else if index == 57 {
            "buffer header allocation failed"
        } else {
            "buffer test allocation reached"
        };
        assert!(output.contains(expected), "case {index}: {output}");
    }
}
