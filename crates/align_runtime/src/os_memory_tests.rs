use super::*;

#[test]
fn meminfo_grammar_ranges_and_order() {
    for (input, total, available) in [
        (
            &b"MemTotal: 8 kB\nMemAvailable: 3 kB"[..],
            Ok(8192),
            Ok(3072),
        ),
        (
            b"ignored:\xff\0\nMemAvailable:\t0\tkB\t\r\nMemTotal:\t1 kB\r\n",
            Ok(1024),
            Ok(0),
        ),
        (
            b"MemTotal: 9007199254740991 kB\nMemAvailable: 9007199254740991 kB",
            Ok(9223372036854774784),
            Ok(9223372036854774784),
        ),
        (
            b"MemTotal: 0 kB\nMemAvailable: 0 kB",
            Err(AL_INVALID),
            Err(AL_INVALID),
        ),
        (
            b"MemTotal: 1 kB\nMemAvailable: 2 kB",
            Ok(1024),
            Err(AL_INVALID),
        ),
        (
            b"MemTotal: 1 kB\nMemAvailable: bad\nMemAvailable: bad",
            Ok(1024),
            Err(AL_INVALID),
        ),
        (b"MemAvailable: 1 kB", Err(AL_INVALID), Err(AL_INVALID)),
        (b"MemTotal: 1 kB", Ok(1024), Err(AL_INVALID)),
        (
            b"MemTotal: 1 kB\nMemTotal: 1 kB\nMemAvailable: 0 kB",
            Err(AL_INVALID),
            Err(AL_INVALID),
        ),
    ] {
        assert_eq!(meminfo_with(input, false, kib_bytes), total, "{input:?}");
        assert_eq!(meminfo_with(input, true, kib_bytes), available, "{input:?}");
    }
    for field in [
        "1 kB",
        " +1 kB",
        " -1 kB",
        " 1kB",
        " 1 KB",
        " 1 kBz",
        " 1 kB\0",
        " 1\u{a0}kB",
        " 1 2 kB",
        " 18446744073709551616 kB",
        " 9007199254740992 kB",
        " 1 kB\r\r",
        "  kB",
    ] {
        let text = format!("MemTotal:{field}\nMemAvailable: 0 kB");
        for selector in [false, true] {
            assert_eq!(
                meminfo_with(text.as_bytes(), selector, kib_bytes),
                Err(AL_INVALID),
                "{field:?}"
            );
        }
        let text = format!("MemTotal: 1 kB\nMemAvailable:{field}");
        assert_eq!(meminfo_with(text.as_bytes(), false, kib_bytes), Ok(1024));
        assert_eq!(
            meminfo_with(text.as_bytes(), true, kib_bytes),
            Err(AL_INVALID)
        );
    }
    for input in [
        b"MemAvailable: bad\nMemTotal: bad".as_slice(),
        b"MemTotal: bad\nMemAvailable: bad".as_slice(),
    ] {
        let mut parsed = Vec::new();
        assert_eq!(
            meminfo_with(input, true, |field| {
                parsed.push(field.to_vec());
                Err(17)
            }),
            Err(17)
        );
        assert_eq!(
            parsed,
            [b" bad".to_vec()],
            "total is validated before available"
        );
    }
    assert_eq!(
        meminfo_with(
            b"MemAvailable: bad\nMemAvailable: bad\nMemTotal: bad",
            true,
            |_| Err(19)
        ),
        Err(19)
    );
}

#[test]
fn bounded_reads_interruptions_errors_and_fd_cleanup() {
    use std::os::fd::{AsRawFd, OwnedFd};
    let prefix = b"MemTotal: 8 kB\nMemAvailable: 3 kB\n";
    for length in [prefix.len(), MEMINFO_CAP, MEMINFO_CAP + 1] {
        let mut input = vec![b'X'; length];
        input[..prefix.len()].copy_from_slice(prefix);
        for available in [false, true] {
            let mut cursor = 0;
            let mut calls = 0;
            let result = read_meminfo(available, |out| {
                calls += 1;
                if calls == 1 {
                    return Err(std::io::Error::from_raw_os_error(libc::EINTR));
                }
                let n = out.len().min(31).min(input.len() - cursor);
                out[..n].copy_from_slice(&input[cursor..cursor + n]);
                cursor += n;
                Ok(n)
            });
            assert_eq!(cursor, length);
            assert_eq!(
                result,
                if length > MEMINFO_CAP {
                    Err(AL_INVALID)
                } else {
                    Ok(if available { 3072 } else { 8192 })
                }
            );
        }
    }
    let mut calls = 0;
    assert_eq!(
        read_meminfo(false, |out| {
            calls += 1;
            if calls == 1 {
                out[..prefix.len()].copy_from_slice(prefix);
                Ok(prefix.len())
            } else {
                Err(std::io::Error::from_raw_os_error(libc::EACCES))
            }
        }),
        Err(super::super::AL_DENIED)
    );
    assert_eq!(
        read_meminfo(false, |out| Ok(out.len() + 1)),
        Err(AL_INVALID)
    );
    let mut reads = 0;
    assert_eq!(
        linux_query(
            false,
            || Err(71),
            |_, _| {
                reads += 1;
                Ok(0)
            }
        ),
        Err(71)
    );
    assert_eq!(reads, 0);
    // Peer EOF observes this acquired owner even if other workers reuse its descriptor number.
    for failure in [false, true] {
        use std::io::Read;
        let (owned, mut peer) = std::os::unix::net::UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let fd: OwnedFd = owned.into();
        let raw = fd.as_raw_fd();
        assert!(unsafe { libc::fcntl(raw, libc::F_GETFD) } >= 0);
        let mut sent = false;
        let result = linux_query(
            false,
            || Ok(fd),
            |observed, out| {
                assert_eq!(observed, raw);
                assert!(unsafe { libc::fcntl(raw, libc::F_GETFD) } >= 0);
                if failure {
                    return Err(std::io::Error::from_raw_os_error(libc::EACCES));
                }
                if sent {
                    return Ok(0);
                }
                sent = true;
                out[..prefix.len()].copy_from_slice(prefix);
                Ok(prefix.len())
            },
        );
        assert_eq!(
            result,
            if failure {
                Err(super::super::AL_DENIED)
            } else {
                Ok(8192)
            }
        );
        assert_eq!(peer.read(&mut [0u8; 1]).unwrap(), 0);
    }
}

struct FakeMac {
    total: Result<u64, i32>,
    host: u32,
    page_size: Result<u64, i32>,
    pages: Result<Pages, i32>,
    release: Result<(), i32>,
    calls: Vec<&'static str>,
}
impl Default for FakeMac {
    fn default() -> Self {
        Self {
            total: Ok(4096),
            host: 9,
            page_size: Ok(1024),
            pages: Ok(Pages {
                free: 1,
                inactive: 2,
            }),
            release: Ok(()),
            calls: Vec::new(),
        }
    }
}
impl MacOps for FakeMac {
    fn total(&mut self) -> Result<u64, i32> {
        self.calls.push("total");
        self.total
    }
    fn host(&mut self) -> u32 {
        self.calls.push("host");
        self.host
    }
    fn page_size(&mut self, host: u32) -> Result<u64, i32> {
        assert_eq!(host, 9);
        self.calls.push("page");
        self.page_size
    }
    fn pages(&mut self, host: u32) -> Result<Pages, i32> {
        assert_eq!(host, 9);
        self.calls.push("stats");
        self.pages
    }
    fn release(&mut self, host: u32) -> Result<(), i32> {
        assert_eq!(host, 9);
        self.calls.push("release");
        self.release
    }
}

#[test]
fn mac_right_owner_order_and_checked_units() {
    let complete = ["total", "host", "page", "stats", "release"];
    for (mut ops, expected, calls) in [
        (FakeMac::default(), Ok(3072), complete.as_slice()),
        (
            FakeMac {
                total: Err(73),
                ..Default::default()
            },
            Err(73),
            &["total"],
        ),
        (
            FakeMac {
                total: Ok(0),
                ..Default::default()
            },
            Err(AL_INVALID),
            &["total"],
        ),
        (
            FakeMac {
                total: Ok(u64::MAX),
                ..Default::default()
            },
            Err(AL_INVALID),
            &["total"],
        ),
        (
            FakeMac {
                host: 0,
                ..Default::default()
            },
            Err(AL_INVALID),
            &["total", "host"],
        ),
        (
            FakeMac {
                host: u32::MAX,
                ..Default::default()
            },
            Err(AL_INVALID),
            &["total", "host"],
        ),
        (
            FakeMac {
                page_size: Err(75),
                release: Err(77),
                ..Default::default()
            },
            Err(75),
            &["total", "host", "page", "release"],
        ),
        (
            FakeMac {
                page_size: Ok(0),
                ..Default::default()
            },
            Err(AL_INVALID),
            &["total", "host", "page", "release"],
        ),
        (
            FakeMac {
                pages: Err(76),
                release: Err(77),
                ..Default::default()
            },
            Err(76),
            complete.as_slice(),
        ),
        (
            FakeMac {
                release: Err(77),
                ..Default::default()
            },
            Err(77),
            complete.as_slice(),
        ),
        (
            FakeMac {
                pages: Ok(Pages {
                    free: 0,
                    inactive: 0,
                }),
                ..Default::default()
            },
            Ok(0),
            complete.as_slice(),
        ),
        (
            FakeMac {
                pages: Ok(Pages {
                    free: 4,
                    inactive: 1,
                }),
                ..Default::default()
            },
            Err(AL_INVALID),
            complete.as_slice(),
        ),
        (
            FakeMac {
                pages: Ok(Pages {
                    free: u64::MAX,
                    inactive: 1,
                }),
                ..Default::default()
            },
            Err(AL_INVALID),
            complete.as_slice(),
        ),
        (
            FakeMac {
                page_size: Ok(u64::MAX),
                ..Default::default()
            },
            Err(AL_INVALID),
            complete.as_slice(),
        ),
        (
            FakeMac {
                total: Ok(i64::MAX as u64),
                page_size: Ok(1),
                pages: Ok(Pages {
                    free: i64::MAX as u64,
                    inactive: 1,
                }),
                ..Default::default()
            },
            Err(AL_INVALID),
            complete.as_slice(),
        ),
    ] {
        assert_eq!(mac_query(true, &mut ops), expected);
        assert_eq!(ops.calls, calls);
    }
    let mut ops = FakeMac::default();
    assert_eq!(mac_query(false, &mut ops), Ok(4096));
    assert_eq!(ops.calls, ["total"]);
    let mut ops = FakeMac::default();
    drop(HostRight {
        ops: &mut ops,
        host: 9,
        owned: true,
    });
    assert_eq!(ops.calls, ["release"]);
    assert_eq!(sysctl_value(4096, 8), Ok(4096));
    for size in [0, 4, 7, 9, usize::MAX] {
        assert_eq!(sysctl_value(4096, size), Err(AL_INVALID));
    }
}

#[test]
fn output_admission_publication_and_native_observation() {
    let mut out = 123i64;
    let mut queried = false;
    for pointer in [
        core::ptr::null_mut(),
        core::ptr::without_provenance_mut(1),
        core::ptr::without_provenance_mut(usize::MAX - 7),
    ] {
        assert_eq!(
            unsafe {
                write_count(pointer, || {
                    queried = true;
                    Ok(1)
                })
            },
            AL_INVALID
        );
        assert!(!queried);
    }
    for result in [Err(AL_INVALID), Err(73), Ok(0), Ok(i64::MAX)] {
        let pointer = &mut out as *mut i64;
        let status = unsafe { write_count(pointer, || { assert_eq!(*pointer, 0); result }) };
        assert_eq!(status, result.err().unwrap_or(0));
        assert_eq!(out, result.unwrap_or(0));
    }
    let mut total = 0;
    let mut available = -1;
    assert_eq!(unsafe { align_rt_os_physical_memory(&mut total) }, 0);
    assert_eq!(unsafe { align_rt_os_available_memory(&mut available) }, 0);
    assert!(total > 0 && available >= 0);
    // Independent calls are not an atomic snapshot; the operation itself owns its upper bound.
    #[cfg(target_os = "linux")]
    {
        let proc = std::fs::read_to_string("/proc/meminfo").unwrap();
        let native: i64 = proc
            .lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(total, native * 1024);
        assert_eq!(available % 1024, 0);
    }
    #[cfg(target_os = "macos")]
    {
        assert_eq!(core::mem::size_of::<libc::vm_statistics64>(), 152);
        let mut stats: libc::vm_statistics64 = unsafe { core::mem::zeroed() };
        stats.free_count = 2;
        stats.inactive_count = 3;
        stats.speculative_count = 17;
        assert_eq!(
            native_pages(&stats, 38),
            Ok(Pages {
                free: 2,
                inactive: 3
            })
        );
        for count in [0, 37, 39, u32::MAX] {
            assert_eq!(native_pages(&stats, count), Err(AL_INVALID));
        }
        let mut native = NativeMac;
        assert_eq!(total, total_bytes(native.total().unwrap()).unwrap());
        let host = native.host();
        let page = native.page_size(host).unwrap();
        native.release(host).unwrap();
        assert_eq!(available as u64 % page, 0);
    }
}

#[test]
#[cfg(feature = "alloc-count")]
fn query_allocation_parity_with_positive_control() {
    use super::super::global_alloc_count;
    let before = global_alloc_count();
    let control = Box::new(17i64);
    std::hint::black_box(&control);
    assert!(global_alloc_count() > before);
    drop(control);
    for available in [false, true] {
        let before = global_alloc_count();
        let value = observe(available);
        let after = global_alloc_count();
        assert!(value.is_ok());
        assert_eq!(after, before);
        let before = global_alloc_count();
        let error = read_meminfo(available, |_| {
            Err(std::io::Error::from_raw_os_error(libc::EACCES))
        });
        let after = global_alloc_count();
        assert_eq!(error, Err(super::super::AL_DENIED));
        assert_eq!(after, before);
    }
    // The injected Mach owner uses pre-reserved inspection storage outside the measured call.
    for failure in 0..4 {
        let mut ops = FakeMac::default();
        ops.calls.reserve(5);
        match failure {
            0 => ops.total = Err(73),
            1 => ops.page_size = Err(73),
            2 => ops.pages = Err(73),
            _ => ops.release = Err(73),
        }
        let before = global_alloc_count();
        let error = mac_query(true, &mut ops);
        let after = global_alloc_count();
        assert_eq!(error, Err(73));
        assert_eq!(after, before);
    }
}
