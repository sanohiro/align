use super::*;

struct ScriptedWrite {
    output: Vec<u8>,
    payload: *const u8,
    payload_len: usize,
    saw_payload: bool,
    first_limit: usize,
    interrupt: bool,
    failure: Option<SocketCallOutcome>,
    calls: usize,
}

impl HttpStreamWriteOps for ScriptedWrite {
    fn send_parts(&mut self, _fd: i32, parts: &[libc::iovec], flags: i32) -> SocketCallOutcome {
        assert_eq!(
            flags,
            if cfg!(target_os = "linux") {
                libc::MSG_NOSIGNAL
            } else {
                0
            }
        );
        self.calls += 1;
        for part in parts {
            let base = part.iov_base.addr();
            let payload = self.payload.addr();
            if self.payload_len > 0 && base >= payload && base < payload + self.payload_len {
                assert!(part.iov_len <= payload + self.payload_len - base);
                self.saw_payload = true;
            }
        }
        if self.interrupt {
            self.interrupt = false;
            return SocketCallOutcome {
                result: -1,
                errno: libc::EINTR,
            };
        }
        if let Some(failure) = self.failure.take() {
            return failure;
        }
        let total: usize = parts.iter().map(|part| part.iov_len).sum();
        let count = total.min(self.first_limit);
        self.first_limit = usize::MAX;
        let mut left = count;
        for part in parts {
            let take = left.min(part.iov_len);
            if take > 0 {
                let bytes =
                    unsafe { core::slice::from_raw_parts(part.iov_base.cast::<u8>(), take) };
                self.output.extend_from_slice(bytes);
            }
            left -= take;
        }
        SocketCallOutcome {
            result: isize::try_from(count).expect("small fixture"),
            errno: 0,
        }
    }
}

fn script(payload: &[u8], capacity: usize, first_limit: usize) -> ScriptedWrite {
    ScriptedWrite {
        output: Vec::with_capacity(capacity),
        payload: payload.as_ptr(),
        payload_len: payload.len(),
        saw_payload: false,
        first_limit,
        interrupt: true,
        failure: None,
        calls: 0,
    }
}

#[test]
fn http_stream_vectored_every_partial_prefix_preserves_wire_and_source_pointer() {
    for framed in [false, true] {
        for pending in [false, true] {
            for event in [false, true] {
                for data in [b"".as_slice(), b"a\0\xff\xe3\x81\x82z"] {
                    if !event && data.is_empty() {
                        continue;
                    }
                    let prefix = if event { b"data: ".as_slice() } else { &[] };
                    let suffix = if event { b"\n\n".as_slice() } else { &[] };
                    let mut expected = if pending {
                        b"HEAD\r\n\r\n".to_vec()
                    } else {
                        Vec::new()
                    };
                    let payload_len = prefix.len() + data.len() + suffix.len();
                    if framed {
                        expected.extend_from_slice(format!("{payload_len:x}\r\n").as_bytes());
                    }
                    expected.extend_from_slice(prefix);
                    expected.extend_from_slice(data);
                    expected.extend_from_slice(suffix);
                    if framed {
                        expected.extend_from_slice(b"\r\n");
                    }
                    for first_limit in 1..=expected.len() {
                        let mut stream = HttpStream {
                            fd: -1,
                            framed,
                            poisoned: false,
                            pending_head: pending.then(|| b"HEAD\r\n\r\n".to_vec()),
                            write_timeout_ns: 0,
                            nonblocking: false,
                        };
                        let mut ops = script(data, expected.len(), first_limit);
                        #[cfg(feature = "alloc-count")]
                        let allocations = global_alloc_count();
                        let status =
                            http_stream_send_parts_with(&mut stream, data, event, &mut ops);
                        #[cfg(feature = "alloc-count")]
                        assert_eq!(
                            global_alloc_count(),
                            allocations,
                            "no writer or framing allocation"
                        );
                        assert_eq!(status, 0);
                        assert_eq!(
                            ops.output, expected,
                            "{framed}/{pending}/{event}/{first_limit}"
                        );
                        assert_eq!(
                            ops.saw_payload,
                            !data.is_empty(),
                            "native vectors borrow the actual caller allocation"
                        );
                        assert!(stream.pending_head.is_none());
                        assert!(!stream.poisoned);
                        assert_eq!(ops.calls, if first_limit == expected.len() { 2 } else { 3 });
                    }
                }
            }
        }
    }
}

#[test]
fn http_stream_vectored_zero_error_and_impossible_count_poison_without_retry() {
    for outcome in [
        SocketCallOutcome {
            result: 0,
            errno: libc::EINTR,
        },
        SocketCallOutcome {
            result: -1,
            errno: libc::EPIPE,
        },
        SocketCallOutcome {
            result: isize::MAX,
            errno: 0,
        },
    ] {
        let mut stream = HttpStream {
            fd: -1,
            framed: true,
            poisoned: false,
            pending_head: Some(b"HEAD".to_vec()),
            write_timeout_ns: 0,
            nonblocking: false,
        };
        let mut ops = script(b"x", 32, usize::MAX);
        ops.interrupt = false;
        ops.failure = Some(outcome);
        let status = http_stream_send_parts_with(&mut stream, b"x", false, &mut ops);
        assert_eq!(
            status,
            if outcome.result == 0 {
                AL_CODE
            } else if outcome.result > 0 {
                AL_INVALID
            } else {
                io_read_write_status(&std::io::Error::from_raw_os_error(libc::EPIPE))
            }
        );
        assert!(stream.poisoned);
        assert!(stream.pending_head.is_none());
        assert_eq!(ops.calls, 1);
        assert_eq!(
            http_stream_send_parts_with(&mut stream, b"x", false, &mut ops),
            AL_INVALID
        );
        assert_eq!(ops.calls, 1);
        assert_eq!(
            unsafe { align_rt_http_stream_send(&mut stream, core::ptr::null(), 0) },
            0
        );
        assert_eq!(
            unsafe { align_rt_http_stream_send_event(&mut stream, core::ptr::null(), 0) },
            AL_INVALID
        );
    }
}

#[test]
fn http_stream_vectored_raw_extent_validation_precedes_head_commit() {
    let mut stream = HttpStream {
        fd: -1,
        framed: true,
        poisoned: false,
        pending_head: Some(b"HEAD".to_vec()),
        write_timeout_ns: 0,
        nonblocking: false,
    };
    let invalid = core::ptr::without_provenance::<u8>(usize::MAX - 1);
    for event in [false, true] {
        let send = if event {
            align_rt_http_stream_send_event
        } else {
            align_rt_http_stream_send
        };
        assert_eq!(unsafe { send(&mut stream, invalid, 3) }, AL_INVALID);
        assert!(stream.pending_head.is_some());
        assert!(!stream.poisoned);
    }
    for (ptr, len) in [(core::ptr::null(), 123), (invalid, -1), (invalid, 0)] {
        assert_eq!(
            unsafe { align_rt_http_stream_send(&mut stream, ptr, len) },
            0
        );
        assert!(stream.pending_head.is_some());
    }
}

#[test]
fn http_stream_vectored_native_binary_octets_and_buffer_reuse() {
    use std::io::Read;
    use std::os::fd::IntoRawFd;
    for framed in [false, true] {
        let (writer, mut reader) = std::os::unix::net::UnixStream::pair().expect("socket pair");
        reader
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("bounded reader");
        let fd = http_prepare_accepted_fd(
            writer.into_raw_fd(),
            NATIVE_SOCKET_SIGPIPE_POLICY,
            &mut NativeSocketWriteOps,
        )
        .unwrap_or_else(|_| panic!("SIGPIPE setup failed"));
        let mut stream = HttpStream {
            fd,
            framed,
            poisoned: false,
            pending_head: Some(b"HEAD\r\n\r\n".to_vec()),
            write_timeout_ns: 0,
            nonblocking: false,
        };
        let mut data: Vec<u8> = (u8::MIN..=u8::MAX).collect();
        assert_eq!(
            unsafe { align_rt_http_stream_send(&mut stream, data.as_ptr(), 256) },
            0
        );
        data.fill(42);
        assert_eq!(
            unsafe { align_rt_http_stream_send(&mut stream, data.as_ptr(), 256) },
            0
        );
        drop(stream);
        let mut actual = Vec::new();
        reader.read_to_end(&mut actual).expect("complete bytes");
        let mut expected = b"HEAD\r\n\r\n".to_vec();
        for body in [(u8::MIN..=u8::MAX).collect::<Vec<_>>(), data] {
            if framed {
                expected.extend_from_slice(b"100\r\n");
            }
            expected.extend_from_slice(&body);
            if framed {
                expected.extend_from_slice(b"\r\n");
            }
        }
        assert_eq!(actual, expected);
    }
}

struct BudgetWrite {
    inner: ScriptedWrite,
    observations: std::collections::VecDeque<Option<std::time::Duration>>,
    waits: std::collections::VecDeque<SocketCallOutcome>,
    poll_timeouts: Vec<i32>,
    starts: usize,
    clocks: usize,
}

impl HttpStreamWriteOps for BudgetWrite {
    fn requires_nonblocking_mode(&self) -> bool {
        false
    } // Mode transitions have a separate owner.
    fn send_parts(&mut self, fd: i32, parts: &[libc::iovec], flags: i32) -> SocketCallOutcome {
        assert_eq!(
            flags & libc::MSG_DONTWAIT,
            if cfg!(target_os = "linux") {
                libc::MSG_DONTWAIT
            } else {
                0
            }
        );
        self.inner
            .send_parts(fd, parts, flags & !libc::MSG_DONTWAIT)
    }
    fn start_budget(&mut self, timeout_ns: i64) -> Option<MonotonicTimeoutBudget> {
        self.starts += 1;
        MonotonicTimeoutBudget::from_positive_ns(timeout_ns)
    }
    fn remaining(&mut self, _: &MonotonicTimeoutBudget) -> Option<std::time::Duration> {
        self.clocks += 1;
        self.observations
            .pop_front()
            .unwrap_or(Some(std::time::Duration::from_secs(60)))
    }
    fn wait_writable(&mut self, _: i32, timeout_ms: i32) -> SocketCallOutcome {
        assert!(timeout_ms > 0, "never probe after expiration");
        self.poll_timeouts.push(timeout_ms);
        self.waits
            .pop_front()
            .unwrap_or(SocketCallOutcome::returned(1))
    }
}

fn budget_script(data: &[u8], observations: &[Option<std::time::Duration>]) -> BudgetWrite {
    let mut inner = script(data, 256, usize::MAX);
    inner.interrupt = false;
    BudgetWrite {
        inner,
        observations: observations.iter().copied().collect(),
        waits: std::collections::VecDeque::new(),
        poll_timeouts: Vec::with_capacity(16),
        starts: 0,
        clocks: 0,
    }
}

fn timed_stream(framed: bool, pending: bool, timeout_ns: i64) -> HttpStream {
    HttpStream {
        fd: -1,
        framed,
        poisoned: false,
        pending_head: pending.then(|| b"HEAD\r\n\r\n".to_vec()),
        write_timeout_ns: timeout_ns,
        nonblocking: false,
    }
}

#[test]
fn http_stream_budget_expiration_wins_over_every_native_result_and_poisons() {
    let live = Some(std::time::Duration::from_nanos(1));
    for framed in [false, true] {
        for pending in [false, true] {
            for event in [false, true] {
                for data in [b"".as_slice(), b"x"] {
                    if data.is_empty() && !event {
                        continue;
                    }
                    for timeout_ns in [1, i64::MAX] {
                        for (outcome, partial) in [
                            (None, usize::MAX), // Success, including a completed write.
                            (None, 1),          // A prefix already reached the peer.
                            (Some(SocketCallOutcome::failed(libc::EINTR)), usize::MAX),
                            (Some(SocketCallOutcome::failed(libc::EAGAIN)), usize::MAX),
                            (Some(SocketCallOutcome::failed(libc::EPIPE)), usize::MAX),
                            (
                                Some(SocketCallOutcome {
                                    result: 0,
                                    errno: libc::EINTR,
                                }),
                                usize::MAX,
                            ),
                        ] {
                            for before in [false, true] {
                                let mut stream = timed_stream(framed, pending, timeout_ns);
                                let observations = [live, None];
                                let mut ops = budget_script(
                                    data,
                                    if before {
                                        &observations[1..]
                                    } else {
                                        &observations
                                    },
                                );
                                ops.inner.failure = outcome;
                                ops.inner.first_limit = partial;
                                assert_eq!(
                                    http_stream_send_parts_with(&mut stream, data, event, &mut ops),
                                    AL_TIMEOUT
                                );
                                assert_eq!(ops.starts, 1);
                                assert_eq!(ops.inner.calls, usize::from(!before));
                                assert!(ops.poll_timeouts.is_empty());
                                assert!(stream.poisoned);
                                assert!(stream.pending_head.is_none());
                                assert_eq!(
                                    http_stream_send_parts_with(&mut stream, b"x", false, &mut ops),
                                    AL_INVALID
                                );
                                assert_eq!(
                                    unsafe {
                                        align_rt_http_stream_send(&mut stream, core::ptr::null(), 0)
                                    },
                                    0
                                );
                                assert_eq!(
                                    ops.starts, 1,
                                    "poison/empty paths start no later budget"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn http_stream_budget_eagain_poll_eintr_and_partial_progress_share_one_budget() {
    let live = Some(std::time::Duration::from_nanos(1));
    let huge = Some(std::time::Duration::from_nanos(u64::MAX));
    for remaining in [live, huge] {
        let mut stream = timed_stream(true, true, i64::MAX);
        let mut ops = budget_script(b"xy", &[remaining; 12]);
        ops.inner.failure = Some(SocketCallOutcome::failed(libc::EAGAIN));
        ops.inner.first_limit = 1;
        ops.waits = [
            SocketCallOutcome::failed(libc::EINTR),
            SocketCallOutcome::returned(0),
        ]
        .into();
        assert_eq!(
            http_stream_send_parts_with(&mut stream, b"xy", false, &mut ops),
            0
        );
        assert_eq!(ops.starts, 1);
        assert_eq!(ops.inner.calls, 3, "would-block, partial, remainder");
        assert_eq!(
            ops.poll_timeouts,
            vec![if remaining == live { 1 } else { i32::MAX }; 2]
        );
        assert_eq!(ops.inner.output, b"HEAD\r\n\r\n2\r\nxy\r\n");
        assert!(!stream.poisoned);
    }
    for observations in [vec![live, live, None], vec![live, live, live, None]] {
        for waited in [
            SocketCallOutcome::returned(1),
            SocketCallOutcome::failed(libc::EINVAL),
        ] {
            let mut stream = timed_stream(false, false, 1);
            let mut ops = budget_script(b"x", &observations);
            ops.inner.failure = Some(SocketCallOutcome::failed(libc::EAGAIN));
            ops.waits.push_back(waited);
            assert_eq!(
                http_stream_send_parts_with(&mut stream, b"x", false, &mut ops),
                AL_TIMEOUT
            );
            assert_eq!(
                ops.poll_timeouts.len(),
                usize::from(observations.len() == 4)
            );
            assert_eq!(ops.inner.calls, 1);
        }
    }
    let mut stream = timed_stream(false, false, 1);
    let mut ops = budget_script(b"x", &[live; 4]);
    ops.inner.failure = Some(SocketCallOutcome::failed(libc::EAGAIN));
    ops.waits.push_back(SocketCallOutcome::failed(libc::EINVAL));
    assert_eq!(
        http_stream_send_parts_with(&mut stream, b"x", false, &mut ops),
        io_read_write_status(&std::io::Error::from_raw_os_error(libc::EINVAL))
    );
    assert!(stream.poisoned);
}

#[test]
fn http_stream_budget_finish_reject_and_close_only_paths() {
    use std::os::fd::IntoRawFd;
    for framed in [false, true] {
        for pending in [false, true] {
            for reject in [false, true] {
                let (writer, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
                let fd = writer.into_raw_fd();
                let mut stream = timed_stream(framed, pending, 1);
                stream.fd = fd;
                let mut ops = budget_script(b"", &[None]);
                let close_only = !reject && !framed && !pending;
                let invalid = reject && !pending;
                let status = if reject {
                    let rb = unsafe { Box::from_raw(align_rt_http_response_new(400)) };
                    http_stream_reject_with(Box::new(stream), rb, &mut ops)
                } else {
                    http_stream_finish_with(Box::new(stream), &mut ops)
                };
                assert_eq!(
                    status,
                    if close_only {
                        0
                    } else if invalid {
                        AL_INVALID
                    } else {
                        AL_TIMEOUT
                    }
                );
                assert_eq!(ops.starts, usize::from(!close_only && !invalid));
                assert_eq!(ops.clocks, usize::from(!close_only && !invalid));
                assert_eq!(ops.inner.calls, 0);
                assert_eq!(
                    unsafe { libc::fcntl(fd, libc::F_GETFD) },
                    -1,
                    "consuming return closes the fd"
                );
            }
        }
    }
    let mut stream = timed_stream(false, true, i64::MAX);
    assert_eq!(
        unsafe { align_rt_http_stream_send(&mut stream, core::ptr::null(), 0) },
        0
    );
    assert!(stream.pending_head.is_some());
    let mut ops = script(b"x", 32, usize::MAX);
    stream.write_timeout_ns = 0;
    assert_eq!(
        http_stream_send_parts_with(&mut stream, b"x", false, &mut ops),
        0,
        "zero mode never supplies nonblocking flags"
    );
}

#[test]
fn http_stream_budget_native_stalled_reader_and_setter_preservation() {
    use std::os::fd::IntoRawFd;
    let (unix_writer, unix_reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let tcp_reader = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (tcp_writer, _) = listener.accept().unwrap();
    let pairs: [(std::os::fd::OwnedFd, std::os::fd::OwnedFd); 2] = [
        (unix_writer.into(), unix_reader.into()),
        (tcp_writer.into(), tcp_reader.into()),
    ];
    for (writer, _reader) in pairs {
        let fd = http_prepare_accepted_fd(
            writer.into_raw_fd(),
            NATIVE_SOCKET_SIGPIPE_POLICY,
            &mut NativeSocketWriteOps,
        )
        .unwrap_or_else(|_| panic!("SIGPIPE setup"));
        let send_buffer: libc::c_int = 4096;
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    (&send_buffer as *const libc::c_int).cast(),
                    libc::socklen_t::try_from(core::mem::size_of_val(&send_buffer)).unwrap(),
                )
            },
            0
        );
        let mut stream = timed_stream(false, true, 0);
        stream.fd = fd;
        let original_flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        assert!(original_flags >= 0);
        for ns in [1, i64::MAX, 0, 5_000_000] {
            assert_eq!(
                unsafe { align_rt_http_stream_write_timeout_ns(&mut stream, ns) },
                0
            );
            assert_eq!(stream.write_timeout_ns, ns);
            assert_eq!(
                unsafe { align_rt_http_stream_write_timeout_ns(&mut stream, -1) },
                AL_INVALID
            );
            assert_eq!(stream.write_timeout_ns, ns);
        }
        let payload = vec![42_u8; 8 * 1024 * 1024];
        let start = std::time::Instant::now();
        assert_eq!(
            unsafe {
                align_rt_http_stream_send(
                    &mut stream,
                    payload.as_ptr(),
                    i64::try_from(payload.len()).unwrap(),
                )
            },
            AL_TIMEOUT
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        assert!(stream.poisoned);
        let current_flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if cfg!(any(target_os = "macos", target_os = "ios")) {
            assert_ne!(
                current_flags & libc::O_NONBLOCK,
                0,
                "sole owned Darwin socket latches"
            );
            assert_eq!(current_flags & original_flags, original_flags);
            assert!(stream.nonblocking);
        } else {
            assert_eq!(
                current_flags, original_flags,
                "per-call Linux flags preserve mode"
            );
            assert!(!stream.nonblocking);
        }
        assert_eq!(
            unsafe { align_rt_http_stream_write_timeout_ns(&mut stream, 0) },
            AL_INVALID
        );
        assert_eq!(stream.write_timeout_ns, 5_000_000);
        assert_eq!(
            unsafe { align_rt_http_stream_send(&mut stream, core::ptr::null(), 0) },
            0
        );
        assert_eq!(
            unsafe { align_rt_http_stream_send(&mut stream, b"x".as_ptr(), 1) },
            AL_INVALID
        );
        assert_eq!(
            unsafe { align_rt_http_stream_finish(Box::into_raw(Box::new(stream))) },
            AL_INVALID
        );
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
    }
    assert_eq!(
        unsafe { align_rt_http_stream_write_timeout_ns(core::ptr::null_mut(), -1) },
        AL_INVALID
    );
    assert_eq!(
        unsafe { align_rt_http_stream_write_timeout_ns(core::ptr::null_mut(), 0) },
        AL_INVALID
    );
}

struct ModeWrite {
    budget: BudgetWrite,
    native_flags: i32,
    get_failure: Option<i32>,
    set_failure: Option<i32>,
    gets: usize,
    sets: usize,
}

impl HttpStreamWriteOps for ModeWrite {
    fn requires_nonblocking_mode(&self) -> bool {
        true
    }
    fn get_flags(&mut self, _: i32) -> SocketCallOutcome {
        self.gets += 1;
        self.get_failure.map_or(
            SocketCallOutcome::returned(isize::try_from(self.native_flags).unwrap()),
            SocketCallOutcome::failed,
        )
    }
    fn set_flags(&mut self, _: i32, flags: i32) -> SocketCallOutcome {
        self.sets += 1;
        self.native_flags = flags; // Darwin may mutate mode even when the following ioctl fails.
        self.set_failure
            .map_or(SocketCallOutcome::returned(0), SocketCallOutcome::failed)
    }
    fn start_budget(&mut self, ns: i64) -> Option<MonotonicTimeoutBudget> {
        self.budget.start_budget(ns)
    }
    fn remaining(&mut self, budget: &MonotonicTimeoutBudget) -> Option<std::time::Duration> {
        self.budget.remaining(budget)
    }
    fn send_parts(&mut self, fd: i32, parts: &[libc::iovec], flags: i32) -> SocketCallOutcome {
        assert_ne!(self.native_flags & libc::O_NONBLOCK, 0);
        self.budget
            .inner
            .send_parts(fd, parts, flags & !libc::MSG_DONTWAIT)
    }
    fn wait_writable(&mut self, _: i32, timeout_ms: i32) -> SocketCallOutcome {
        self.budget.poll_timeouts.push(timeout_ms);
        self.budget
            .waits
            .pop_front()
            .unwrap_or(SocketCallOutcome::returned(1))
    }
}

fn mode_script(observations: &[Option<std::time::Duration>]) -> ModeWrite {
    ModeWrite {
        budget: budget_script(b"x", observations),
        native_flags: libc::O_APPEND,
        get_failure: None,
        set_failure: None,
        gets: 0,
        sets: 0,
    }
}

#[test]
fn http_stream_budget_mode_setup_failure_expiration_and_mutation_order() {
    let live = Some(std::time::Duration::from_secs(60));
    for (
        observations,
        get_failure,
        set_failure,
        initially_nonblocking,
        expected,
        gets,
        sets,
        latched,
    ) in [
        (vec![None], None, None, false, AL_TIMEOUT, 0, 0, false),
        (
            vec![live, live],
            Some(libc::EINVAL),
            None,
            false,
            AL_INVALID,
            1,
            0,
            false,
        ),
        (
            vec![live, None],
            Some(libc::EINVAL),
            None,
            false,
            AL_TIMEOUT,
            1,
            0,
            false,
        ),
        (
            vec![live; 4],
            None,
            Some(libc::EINVAL),
            false,
            AL_INVALID,
            1,
            1,
            false,
        ),
        (
            vec![live, live, live, None],
            None,
            Some(libc::EINVAL),
            false,
            AL_TIMEOUT,
            1,
            1,
            false,
        ),
        (
            vec![live, live, live, None],
            None,
            None,
            false,
            AL_TIMEOUT,
            1,
            1,
            true,
        ),
        (vec![live, None], None, None, true, AL_TIMEOUT, 1, 0, true),
    ] {
        let mut stream = timed_stream(true, true, 1);
        let mut ops = mode_script(&observations);
        ops.get_failure = get_failure;
        ops.set_failure = set_failure;
        if initially_nonblocking {
            ops.native_flags |= libc::O_NONBLOCK;
        }
        assert_eq!(
            http_stream_send_parts_with(&mut stream, b"x", false, &mut ops),
            expected
        );
        assert_eq!(
            (ops.gets, ops.sets, stream.nonblocking),
            (gets, sets, latched)
        );
        assert!(stream.poisoned);
        assert!(stream.pending_head.is_none());
        assert_eq!(
            ops.budget.inner.calls, 0,
            "no write follows failed or expired setup"
        );
        let flags = ops.native_flags;
        assert_eq!(
            http_stream_send_parts_with(&mut stream, b"x", false, &mut ops),
            AL_INVALID
        );
        assert_eq!(
            ops.native_flags, flags,
            "poison does not attempt restoration"
        );
        if sets == 1 {
            assert_ne!(
                flags & libc::O_NONBLOCK,
                0,
                "mutation-then-error is retained until close"
            );
        }
        assert_ne!(
            flags & libc::O_APPEND,
            0,
            "setup preserves unrelated mode bits"
        );
    }
}

#[test]
fn http_stream_budget_latched_zero_retries_indefinitely_without_clock_or_setup() {
    let mut stream = timed_stream(true, true, i64::MAX);
    let mut ops = mode_script(&[]);
    assert_eq!(
        http_stream_send_parts_with(&mut stream, b"x", false, &mut ops),
        0
    );
    assert!(stream.nonblocking);
    assert_eq!((ops.gets, ops.sets), (1, 1));
    assert_eq!(
        http_stream_send_parts_with(&mut stream, b"x", false, &mut ops),
        0
    );
    assert_eq!((ops.gets, ops.sets), (1, 1), "latch setup occurs once");
    stream.write_timeout_ns = 0;
    let clocks = ops.budget.clocks;
    ops.budget.inner.failure = Some(SocketCallOutcome::failed(libc::EAGAIN));
    ops.budget.waits = [
        SocketCallOutcome::failed(libc::EINTR),
        SocketCallOutcome::returned(1),
    ]
    .into();
    assert_eq!(
        http_stream_send_parts_with(&mut stream, b"x", false, &mut ops),
        0
    );
    assert_eq!(
        ops.budget.clocks, clocks,
        "configured-zero observes no deadline clock"
    );
    assert_eq!(ops.budget.poll_timeouts, vec![-1, -1]);
    assert_eq!((ops.gets, ops.sets), (1, 1));
}

// The writer fd remains sole-owned. The reader clone exists only to interrupt/join cleanup.
struct NativePeerDrain {
    stop: std::os::unix::net::UnixStream,
    thread: Option<std::thread::JoinHandle<Vec<u8>>>,
}
impl Drop for NativePeerDrain {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = self.stop.shutdown(std::net::Shutdown::Both);
            let _ = thread.join();
        }
    }
}

#[test]
fn http_stream_budget_native_positive_then_zero_preserves_exact_drained_bytes() {
    use std::io::Read;
    use std::os::fd::IntoRawFd;
    let (writer, mut reader) = std::os::unix::net::UnixStream::pair().unwrap();
    reader
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let fd = http_prepare_accepted_fd(
        writer.into_raw_fd(),
        NATIVE_SOCKET_SIGPIPE_POLICY,
        &mut NativeSocketWriteOps,
    )
    .unwrap_or_else(|_| panic!("SIGPIPE setup"));
    let mut stream = timed_stream(false, false, 1_000_000_000);
    stream.fd = fd;
    assert_eq!(
        unsafe { align_rt_http_stream_send(&mut stream, b"first".as_ptr(), 5) },
        0
    );
    let payload = vec![0xff; 8 * 1024 * 1024];
    let mut drain = NativePeerDrain {
        stop: reader.try_clone().unwrap(),
        thread: Some(std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let mut output = Vec::new();
            reader.read_to_end(&mut output).unwrap();
            output
        })),
    };
    assert_eq!(
        unsafe { align_rt_http_stream_write_timeout_ns(&mut stream, 0) },
        0
    );
    assert_eq!(
        unsafe {
            align_rt_http_stream_send(
                &mut stream,
                payload.as_ptr(),
                i64::try_from(payload.len()).unwrap(),
            )
        },
        0
    );
    assert_eq!(
        unsafe { align_rt_http_stream_finish(Box::into_raw(Box::new(stream))) },
        0
    );
    let output = drain.thread.take().unwrap().join().unwrap();
    assert_eq!(&output[..5], b"first");
    assert_eq!(&output[5..], &payload);
}
