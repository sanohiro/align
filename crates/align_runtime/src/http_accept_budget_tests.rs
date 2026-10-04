use super::*;
use std::collections::{HashMap, VecDeque};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};

fn server(ns: i64) -> HttpServer {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    HttpServer {
        fd: listener.into_raw_fd(),
        accept_timeout_ns: ns,
        listener_mode: HttpListenerMode::Blocking,
        max_request_body_bytes: None,
        park: std::sync::Arc::new(std::sync::Mutex::new(ParkSlot::Live(Vec::new()))),
        poll_buf: Vec::new(),
        poll_cursor: 0,
    }
}

#[derive(Default)]
struct AcceptScript {
    accepted: VecDeque<Result<OwnedFd, i32>>,
    reads: VecDeque<Result<Vec<u8>, i32>>,
    polls: VecDeque<Result<usize, i32>>,
    flags: HashMap<i32, i32>,
    effective: HashMap<i32, bool>,
    fail_get: Option<i32>,
    fail_set: Option<i32>,
    fail_get_at: Option<usize>,
    fail_set_at: Option<usize>,
    fail_nosigpipe: bool,
    expire_on_failure: bool,
    expire_at: Option<usize>,
    expired: bool,
    expire_after_backoff: bool,
    clocks: usize,
    starts: usize,
    accepts: usize,
    gets: usize,
    sets: usize,
    reads_count: usize,
    waits: Vec<i32>,
    backoffs: Vec<std::time::Duration>,
}
impl HttpAcceptOps for AcceptScript {
    fn start_budget(&mut self, ns: i64) -> Option<MonotonicTimeoutBudget> {
        self.starts += 1;
        MonotonicTimeoutBudget::from_positive_ns(ns)
    }
    fn remaining(&mut self, _: &MonotonicTimeoutBudget) -> Option<std::time::Duration> {
        let index = self.clocks;
        self.clocks += 1;
        if self.expired || self.expire_at.is_some_and(|at| index >= at) {
            None
        } else {
            Some(std::time::Duration::from_nanos(1))
        }
    }
    fn get_flags(&mut self, fd: i32) -> SocketCallOutcome {
        self.gets += 1;
        if self.fail_get_at == Some(self.gets) {
            self.expired = self.expire_on_failure;
            return SocketCallOutcome::failed(libc::EINVAL);
        }
        if let Some(errno) = self.fail_get.take() {
            return SocketCallOutcome::failed(errno);
        }
        SocketCallOutcome::returned(
            isize::try_from(*self.flags.get(&fd).unwrap_or(&libc::O_APPEND)).unwrap(),
        )
    }
    fn set_flags(&mut self, fd: i32, flags: i32) -> SocketCallOutcome {
        self.sets += 1;
        self.flags.insert(fd, flags);
        // File-status flags can change before a failed native ioctl changes socket-effective mode.
        if self.fail_set_at == Some(self.sets) {
            self.expired = self.expire_on_failure;
            return SocketCallOutcome::failed(libc::EINVAL);
        }
        if let Some(errno) = self.fail_set.take() {
            return SocketCallOutcome::failed(errno);
        }
        self.effective.insert(fd, flags & libc::O_NONBLOCK != 0);
        SocketCallOutcome::returned(0)
    }
    fn accept(&mut self, fd: i32) -> SocketCallOutcome {
        self.accepts += 1;
        assert_eq!(
            self.effective.get(&fd),
            Some(&true),
            "successful mode reassertion precedes accept"
        );
        match self.accepted.pop_front().unwrap_or(Err(libc::EAGAIN)) {
            Ok(fd) => SocketCallOutcome::returned(isize::try_from(fd.into_raw_fd()).unwrap()),
            Err(errno) => SocketCallOutcome::failed(errno),
        }
    }
    unsafe fn read(
        &mut self,
        fd: i32,
        ptr: *mut u8,
        len: usize,
        nonblocking: bool,
    ) -> SocketCallOutcome {
        self.reads_count += 1;
        assert!(nonblocking);
        assert_eq!(
            self.effective.get(&fd),
            Some(&false),
            "fresh socket is restored to effective blocking mode"
        );
        match self.reads.pop_front().unwrap_or(Err(libc::EAGAIN)) {
            Ok(bytes) => {
                let n = len.min(bytes.len());
                unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, n) };
                if n < bytes.len() {
                    self.reads.push_front(Ok(bytes[n..].to_vec()));
                }
                SocketCallOutcome::returned(isize::try_from(n).unwrap())
            }
            Err(errno) => SocketCallOutcome::failed(errno),
        }
    }
    fn poll(&mut self, fds: &mut [PollFd], timeout_ms: i32) -> SocketCallOutcome {
        self.waits.push(timeout_ms);
        assert!(timeout_ms == -1 || timeout_ms > 0, "no final zero probe");
        match self.polls.pop_front().unwrap_or(Ok(fds.len() - 1)) {
            Ok(index) => {
                fds[index].revents = POLLIN;
                SocketCallOutcome::returned(1)
            }
            Err(errno) => {
                self.expired = self.expire_on_failure;
                SocketCallOutcome::failed(errno)
            }
        }
    }
    fn set_option(&mut self, _: i32, _: i32, _: i32) -> SocketCallOutcome {
        SocketCallOutcome::returned(0)
    }
    fn install_nosigpipe(&mut self, _: i32) -> SocketCallOutcome {
        if self.fail_nosigpipe {
            self.expired = self.expire_on_failure;
            SocketCallOutcome::failed(libc::EINVAL)
        } else {
            SocketCallOutcome::returned(0)
        }
    }
    fn backoff(&mut self, duration: std::time::Duration) {
        self.backoffs.push(duration);
        if self.expire_after_backoff {
            self.expired = true;
        }
    }
}
fn scripted_peer(bytes: &[u8]) -> (AcceptScript, std::os::unix::net::UnixStream, i32) {
    let (fd, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let fd: OwnedFd = fd.into();
    let raw = fd.as_raw_fd();
    let mut ops = AcceptScript::default();
    ops.accepted.push_back(Ok(fd));
    ops.reads.push_back(Ok(bytes.to_vec()));
    (ops, peer, raw)
}
fn assert_peer_closed(peer: &std::os::unix::net::UnixStream) {
    use std::io::Read;
    peer.set_nonblocking(true).unwrap();
    let mut probe = [0_u8; 1];
    assert_eq!(
        (&mut &*peer).read(&mut probe).unwrap(),
        0,
        "selected socket owner closed"
    );
}
fn run(server: &mut HttpServer, ops: &mut AcceptScript) -> (i32, *mut HttpRequestCtx) {
    let mut out = core::ptr::NonNull::<HttpRequestCtx>::dangling().as_ptr();
    let result = unsafe { http_accept_with(server, &mut out, ops) };
    (result, out)
}

#[test]
fn http_server_accept_budget_expiry_at_every_checkpoint_closes_unpublished_input() {
    const REQUEST: &[u8] = b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\n\x00\xff";
    let mut baseline = server(i64::MAX);
    let (mut ops, _peer, _) = scripted_peer(REQUEST);
    let (status, ctx) = run(&mut baseline, &mut ops);
    assert_eq!(status, 0);
    assert!(!ctx.is_null());
    unsafe { align_rt_http_ctx_free(ctx) };
    let observations = ops.clocks;
    assert!(observations > 20);
    for at in 0..observations {
        let mut srv = server(if at % 2 == 0 { 1 } else { i64::MAX });
        let (mut ops, peer, _raw) = scripted_peer(REQUEST);
        ops.expire_at = Some(at);
        let (status, out) = run(&mut srv, &mut ops);
        assert_eq!(status, AL_TIMEOUT, "checkpoint {at}");
        assert!(out.is_null());
        assert_eq!(
            ops.starts, 1,
            "one budget across selection/admission/parser/publication"
        );
        if ops.accepts != 0 {
            assert_peer_closed(&peer);
        }
        assert!(
            unsafe { libc::fcntl(srv.fd, libc::F_GETFD) } >= 0,
            "listener survives timeout"
        );
    }
}

#[test]
fn http_server_accept_budget_unknown_recovery_reasserts_effective_mode_even_after_zero_reset() {
    for ns in [0, 1, i64::MAX] {
        let mut srv = server(1);
        let mut ops = AcceptScript {
            fail_set: Some(libc::EINVAL),
            ..AcceptScript::default()
        };
        assert_eq!(
            http_listener_nonblocking(
                &mut srv,
                MonotonicTimeoutBudget::from_positive_ns(1).as_ref(),
                &mut ops
            ),
            Err(AL_INVALID)
        );
        assert!(srv.listener_mode == HttpListenerMode::Unknown);
        assert_ne!(ops.flags[&srv.fd] & libc::O_NONBLOCK, 0);
        assert!(!ops.effective.get(&srv.fd).copied().unwrap_or(false));
        srv.accept_timeout_ns = ns;
        let (mut later, _peer, _) = scripted_peer(b"GET / HTTP/1.1\r\n\r\n");
        later.flags = ops.flags;
        let (status, ctx) = run(&mut srv, &mut later);
        assert_eq!(status, 0);
        assert!(srv.listener_mode == HttpListenerMode::Nonblocking);
        assert_eq!(
            later.sets, 2,
            "listener reassertion then unconditional connected blocking setup"
        );
        assert_eq!(later.clocks == 0, ns == 0);
        assert_eq!(later.waits, vec![if ns == 0 { -1 } else { 1 }]);
        unsafe { align_rt_http_ctx_free(ctx) };
    }
    for failure in [libc::EINVAL, libc::EINTR] {
        let mut srv = server(0);
        srv.listener_mode = HttpListenerMode::Unknown;
        let mut ops = AcceptScript {
            fail_get: Some(failure),
            ..AcceptScript::default()
        };
        let (status, ctx) = run(&mut srv, &mut ops);
        assert_ne!(status, 0);
        assert!(ctx.is_null());
        assert_eq!(ops.accepts, 0);
        assert!(ops.waits.is_empty());
        assert!(srv.listener_mode == HttpListenerMode::Unknown);
        assert_eq!(ops.clocks, 0);
    }
}

#[test]
fn http_server_accept_budget_retry_readiness_malformed_and_body_cap_keep_one_budget() {
    let mut srv = server(i64::MAX);
    let (mut ops, peer, _raw) = scripted_peer(b"BAD\n\n");
    let (second, _second_peer) = std::os::unix::net::UnixStream::pair().unwrap();
    ops.accepted.push_front(Err(libc::EINTR));
    ops.accepted.push_back(Ok(second.into()));
    ops.polls.push_back(Err(libc::EINTR));
    ops.reads.push_front(Err(libc::EINTR));
    ops.reads.push_back(Err(libc::EAGAIN));
    ops.reads
        .push_back(Ok(b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\na".to_vec()));
    ops.reads.push_back(Ok(b"b".to_vec()));
    let (status, ctx) = run(&mut srv, &mut ops);
    assert_eq!(status, 0);
    assert_eq!(ops.starts, 1);
    assert_eq!(ops.accepts, 3);
    assert_peer_closed(&peer);
    assert_eq!(unsafe { &(&(*ctx).buf)[(*ctx).body_start..] }, b"ab");
    unsafe { align_rt_http_ctx_free(ctx) };
    for limit in [0, 1, 2] {
        let mut srv = server(1);
        srv.max_request_body_bytes = Some(limit);
        let (mut ops, peer, _raw) =
            scripted_peer(b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\nab");
        let (status, ctx) = run(&mut srv, &mut ops);
        assert_eq!(status, if limit < 2 { AL_INVALID } else { 0 });
        if ctx.is_null() {
            assert_peer_closed(&peer);
        } else {
            unsafe { align_rt_http_ctx_free(ctx) };
        }
    }
}

#[test]
fn http_server_accept_budget_pressure_caps_backoff_and_expiry_wins_over_native_errors() {
    let mut srv = server(1);
    let mut ops = AcceptScript {
        expire_after_backoff: true,
        ..AcceptScript::default()
    };
    ops.accepted.push_back(Err(libc::EMFILE));
    let (status, ctx) = run(&mut srv, &mut ops);
    assert_eq!(status, AL_TIMEOUT);
    assert!(ctx.is_null());
    assert_eq!(ops.accepts, 1);
    assert_eq!(ops.backoffs, vec![std::time::Duration::from_nanos(1)]);
    for errno in [libc::EINTR, libc::EAGAIN, libc::EMFILE, libc::EBADF] {
        let mut srv = server(1);
        srv.listener_mode = HttpListenerMode::Nonblocking;
        let mut ops = AcceptScript {
            expire_at: Some(5),
            ..AcceptScript::default()
        };
        ops.effective.insert(srv.fd, true);
        ops.accepted.push_back(Err(errno));
        let (status, ctx) = run(&mut srv, &mut ops);
        assert_eq!(status, AL_TIMEOUT);
        assert!(ctx.is_null());
        assert_eq!(ops.accepts, 1);
        assert!(ops.backoffs.is_empty());
    }
}

#[test]
fn http_server_accept_budget_native_partial_then_success_and_zero_preserve_listener() {
    use std::io::Write;
    let mut srv = server(5_000_000);
    let flags = unsafe { libc::fcntl(srv.fd, libc::F_GETFL) };
    for ns in [0, 1, i64::MAX, 5_000_000] {
        assert_eq!(
            unsafe { align_rt_http_server_accept_timeout_ns(&mut srv, ns) },
            0
        );
        assert_eq!(
            unsafe { align_rt_http_server_accept_timeout_ns(&mut srv, -1) },
            AL_INVALID
        );
        assert_eq!(srv.accept_timeout_ns, ns);
        assert_eq!(unsafe { libc::fcntl(srv.fd, libc::F_GETFL) }, flags);
    }
    let mut out = core::ptr::null_mut();
    let start = std::time::Instant::now();
    assert_eq!(
        unsafe { align_rt_http_accept(&mut srv, &mut out) },
        AL_TIMEOUT
    );
    assert!(out.is_null());
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    let borrowed =
        std::mem::ManuallyDrop::new(unsafe { std::net::TcpListener::from_raw_fd(srv.fd) });
    let address = borrowed.local_addr().unwrap();
    for bytes in [
        b"POST / HTTP/1.1\r\nX: part".as_slice(),
        b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nx",
    ] {
        let mut peer = std::net::TcpStream::connect(address).unwrap();
        peer.write_all(bytes).unwrap();
        assert_eq!(
            unsafe { align_rt_http_accept(&mut srv, &mut out) },
            AL_TIMEOUT
        );
        assert!(out.is_null());
    }
    for ns in [1_000_000_000, 0] {
        assert_eq!(
            unsafe { align_rt_http_server_accept_timeout_ns(&mut srv, ns) },
            0
        );
        let mut peer = std::net::TcpStream::connect(address).unwrap();
        peer.write_all(b"POST /ok HTTP/1.1\r\nContent-Length: 2\r\n\r\n\x00\xff")
            .unwrap();
        assert_eq!(unsafe { align_rt_http_accept(&mut srv, &mut out) }, 0);
        assert!(!out.is_null());
        assert_eq!(
            unsafe { libc::fcntl((*out).fd, libc::F_GETFL) } & libc::O_NONBLOCK,
            0
        );
        assert_eq!(unsafe { &(&(*out).buf)[(*out).body_start..] }, b"\x00\xff");
        unsafe { align_rt_http_ctx_free(out) };
    }
}

#[test]
fn http_server_accept_budget_parked_partial_timeout_preserves_unselected_fd() {
    use std::io::Write;
    let mut srv = server(5_000_000);
    let (selected, mut peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let (unselected, _silent) = std::os::unix::net::UnixStream::pair().unwrap();
    let selected = selected.into_raw_fd();
    let unselected = unselected.into_raw_fd();
    *srv.park.lock().unwrap() = ParkSlot::Live(vec![selected, unselected]);
    peer.write_all(b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\na")
        .unwrap();
    let mut out = core::ptr::null_mut();
    assert_eq!(
        unsafe { align_rt_http_accept(&mut srv, &mut out) },
        AL_TIMEOUT
    );
    assert!(out.is_null());
    assert_peer_closed(&peer);
    assert!(unsafe { libc::fcntl(unselected, libc::F_GETFD) } >= 0);
    let slot = srv.park.lock().unwrap();
    assert!(matches!(&*slot, ParkSlot::Live(fds) if fds == &[unselected]));
}

#[test]
fn http_server_accept_budget_setup_and_poll_failures_close_only_selected_owners() {
    for stage in 0..6 {
        if stage == 5 && NATIVE_SOCKET_SIGPIPE_POLICY != SocketSigpipePolicy::InstallSocketOption {
            continue;
        }
        for expired in [false, true] {
            let mut srv = server(i64::MAX);
            let (mut ops, peer, _raw) = scripted_peer(b"GET / HTTP/1.1\r\n\r\n");
            ops.expire_on_failure = expired;
            match stage {
                0 => ops.fail_get_at = Some(1),
                1 => ops.fail_set_at = Some(1),
                2 => ops.fail_get_at = Some(2),
                3 => ops.fail_set_at = Some(2),
                4 => ops.polls.push_back(Err(libc::EINVAL)),
                _ => ops.fail_nosigpipe = true,
            }
            let (status, ctx) = run(&mut srv, &mut ops);
            assert_eq!(
                status,
                if expired { AL_TIMEOUT } else { AL_INVALID },
                "stage {stage}"
            );
            assert!(ctx.is_null());
            assert!(unsafe { libc::fcntl(srv.fd, libc::F_GETFD) } >= 0);
            assert_eq!(ops.reads_count, 0);
            if ops.accepts != 0 {
                assert_peer_closed(&peer);
            }
            assert_eq!(srv.listener_mode == HttpListenerMode::Unknown, stage == 1);
        }
    }
}

#[test]
fn http_server_accept_budget_native_setter_invalid_inputs_preserve_previous_policy() {
    let mut srv = server(19);
    assert_eq!(
        unsafe { align_rt_http_server_accept_timeout_ns(core::ptr::null_mut(), -1) },
        AL_INVALID
    );
    assert_eq!(
        unsafe { align_rt_http_server_accept_timeout_ns(core::ptr::null_mut(), 0) },
        AL_INVALID
    );
    let misaligned = (&mut srv as *mut HttpServer)
        .cast::<u8>()
        .wrapping_add(1)
        .cast::<HttpServer>();
    assert_eq!(
        unsafe { align_rt_http_server_accept_timeout_ns(misaligned, 0) },
        AL_INVALID
    );
    assert_eq!(srv.accept_timeout_ns, 19);
    // Arm the detached fd first, so the deliberately closed native shell owns no live socket.
    let _fd = unsafe { OwnedFd::from_raw_fd(std::mem::replace(&mut srv.fd, -1)) };
    for ns in [-1, 0, 1, i64::MAX] {
        assert_eq!(
            unsafe { align_rt_http_server_accept_timeout_ns(&mut srv, ns) },
            AL_INVALID
        );
        assert_eq!(srv.accept_timeout_ns, 19);
    }
}
