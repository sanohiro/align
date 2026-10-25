use super::*;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

struct Request {
    ctx: Box<HttpRequestCtx>,
    peer: UnixStream,
    park: Arc<Mutex<ParkSlot>>,
}

fn request(method: &str, persistent: bool) -> Request {
    let (socket, mut peer) = UnixStream::pair().unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        let yes: i32 = 1;
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_NOSIGPIPE,
                    (&yes as *const i32).cast(),
                    4,
                )
            },
            0
        );
    }
    let connection = if persistent {
        ""
    } else {
        "Connection: close\r\n"
    };
    peer.write_all(format!("{method} / HTTP/1.1\r\nHost: h\r\n{connection}\r\n").as_bytes())
        .unwrap();
    let mut ctx = Box::new(unsafe { http_read_request(socket.into_raw_fd(), None) }.unwrap());
    let park = Arc::new(Mutex::new(ParkSlot::Live(Vec::new())));
    ctx.park = Some(park.clone());
    Request { ctx, peer, park }
}

fn response() -> Box<ResponseBuilder> {
    Box::new(ResponseBuilder {
        status: 200,
        headers: Vec::new(),
        body: Some(b"hello".to_vec()),
    })
}

// Tests own every parked descriptor, including during assertion unwind.
struct ParkCleanup(Arc<Mutex<ParkSlot>>);
impl Drop for ParkCleanup {
    fn drop(&mut self) {
        let mut park = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let ParkSlot::Live(fds) = &mut *park {
            for fd in fds.drain(..) {
                unsafe { close(fd) };
            }
        }
    }
}

#[derive(Default)]
struct Script {
    darwin: bool,
    calls: Vec<&'static str>,
    writes: VecDeque<SocketCallOutcome>,
    output: Vec<u8>,
    flags: i32,
    sets: Vec<i32>,
    fail_get: bool,
    fail_set: Option<usize>,
    clocks: usize,
    expire_at: Option<usize>,
    expire_pause: bool,
    starts: usize,
    expire_after: Option<&'static str>,
}

impl HttpStreamWriteOps for Script {
    fn requires_nonblocking_mode(&self) -> bool {
        self.darwin
    }
    fn start_budget(&mut self, ns: i64) -> Option<MonotonicTimeoutBudget> {
        self.starts += 1;
        MonotonicTimeoutBudget::from_positive_ns(ns)
    }
    fn remaining(&mut self, _: &MonotonicTimeoutBudget) -> Option<std::time::Duration> {
        let tick = self.clocks;
        self.clocks += 1;
        if self.expire_at.is_some_and(|at| tick >= at) {
            None
        } else {
            Some(std::time::Duration::from_nanos(1))
        }
    }
    fn get_flags(&mut self, _: i32) -> SocketCallOutcome {
        self.calls.push("get");
        if self.expire_after == Some("get") {
            self.expire_at = Some(self.clocks);
        }
        if self.fail_get {
            SocketCallOutcome::failed(libc::EINVAL)
        } else {
            SocketCallOutcome::returned(isize::try_from(self.flags).unwrap())
        }
    }
    fn set_flags(&mut self, _: i32, flags: i32) -> SocketCallOutcome {
        self.calls.push("set");
        self.sets.push(flags);
        if self.expire_after
            == Some(if self.sets.len() == 1 {
                "set"
            } else {
                "restore"
            })
        {
            self.expire_at = Some(self.clocks);
        }
        self.flags = flags;
        if self.fail_set == Some(self.sets.len()) {
            SocketCallOutcome::failed(libc::EINVAL)
        } else {
            SocketCallOutcome::returned(0)
        }
    }
    fn send_parts(&mut self, _: i32, parts: &[libc::iovec], flags: i32) -> SocketCallOutcome {
        self.calls.push("write");
        if self.expire_after == Some("write") {
            self.expire_at = Some(self.clocks);
        }
        if self.darwin {
            assert_ne!(self.flags & libc::O_NONBLOCK, 0);
        }
        #[cfg(target_os = "linux")]
        assert_ne!(flags & libc::MSG_DONTWAIT, 0);
        #[cfg(not(target_os = "linux"))]
        let _ = flags;
        let total: usize = parts.iter().map(|part| part.iov_len).sum();
        let result = self
            .writes
            .pop_front()
            .unwrap_or_else(|| SocketCallOutcome::returned(isize::try_from(total).unwrap()));
        if let Ok(mut count) = usize::try_from(result.result) {
            for part in parts {
                let take = count.min(part.iov_len);
                if take > 0 {
                    self.output.extend_from_slice(unsafe {
                        core::slice::from_raw_parts(part.iov_base.cast::<u8>(), take)
                    });
                }
                count -= take;
            }
        }
        result
    }
    fn wait_writable(&mut self, _: i32, timeout_ms: i32) -> SocketCallOutcome {
        assert!(timeout_ms > 0);
        self.calls.push("wait");
        if self.expire_after == Some("wait") {
            self.expire_at = Some(self.clocks);
            return SocketCallOutcome::failed(libc::EINVAL);
        }
        SocketCallOutcome::returned(1)
    }
    fn pause(&mut self, duration: std::time::Duration) {
        assert!(
            duration > std::time::Duration::ZERO && duration <= std::time::Duration::from_millis(1)
        );
        self.calls.push("pause");
        if self.expire_pause {
            self.expire_at = Some(self.clocks);
        }
    }
}

fn scripted(
    ctx: Box<HttpRequestCtx>,
    builder: Box<ResponseBuilder>,
    ns: i64,
    script: &mut Script,
) -> i32 {
    unsafe { http_respond_impl(Box::into_raw(ctx), Box::into_raw(builder), Some(ns), script) }
}

#[test]
fn http_response_budget_partial_retry_head_and_mode_restoration() {
    for darwin in [false, true] {
        for method in ["GET", "HEAD", "head"] {
            for persistent in [false, true] {
                let Request {
                    ctx,
                    mut peer,
                    park,
                } = request(method, persistent);
                let _cleanup = ParkCleanup(park.clone());
                let mut script = Script {
                    darwin,
                    flags: libc::O_APPEND,
                    writes: [
                        SocketCallOutcome::returned(3),
                        SocketCallOutcome::failed(libc::EINTR),
                        SocketCallOutcome::failed(libc::EAGAIN),
                    ]
                    .into(),
                    ..Script::default()
                };
                assert_eq!(scripted(ctx, response(), i64::MAX, &mut script), 0);
                assert_eq!(script.starts, 1);
                let body = if method == "HEAD" { "" } else { "hello" };
                let connection = if persistent {
                    ""
                } else {
                    "Connection: close\r\n"
                };
                assert_eq!(
                    script.output,
                    format!("HTTP/1.1 200 OK\r\n{connection}Content-Length: 5\r\n\r\n{body}")
                        .as_bytes()
                );
                assert_eq!(
                    script.sets,
                    if darwin {
                        if persistent {
                            vec![libc::O_APPEND | libc::O_NONBLOCK, libc::O_APPEND]
                        } else {
                            vec![libc::O_APPEND | libc::O_NONBLOCK]
                        }
                    } else {
                        vec![]
                    }
                );
                let guard = park.lock().unwrap();
                let ParkSlot::Live(fds) = &*guard else {
                    panic!("live")
                };
                assert_eq!(fds.len(), usize::from(persistent));
                if !persistent {
                    assert_eq!(peer.read(&mut [0u8; 1]).unwrap(), 0);
                }
            }
        }
    }
}

#[test]
fn http_response_budget_every_expiry_and_native_failure_closes_without_parking() {
    for darwin in [false, true] {
        let Request { ctx, park, .. } = request("GET", true);
        let _cleanup = ParkCleanup(park);
        let mut control = Script {
            darwin,
            ..Script::default()
        };
        assert_eq!(scripted(ctx, response(), 1, &mut control), 0);
        for checkpoint in 0..control.clocks {
            let Request {
                ctx,
                mut peer,
                park,
            } = request("GET", true);
            let _cleanup = ParkCleanup(park.clone());
            let mut script = Script {
                darwin,
                expire_at: Some(checkpoint),
                ..Script::default()
            };
            assert_eq!(
                scripted(ctx, response(), 1, &mut script),
                AL_TIMEOUT,
                "{darwin}/{checkpoint}"
            );
            assert!(matches!(&*park.lock().unwrap(), ParkSlot::Live(fds) if fds.is_empty()));
            assert_eq!(peer.read(&mut [0u8; 1]).unwrap(), 0);
        }
    }
    for (get, set, write) in [
        (true, None, false),
        (false, Some(1), false),
        (false, Some(2), false),
        (false, None, true),
    ] {
        let Request {
            ctx,
            mut peer,
            park,
        } = request("GET", true);
        let _cleanup = ParkCleanup(park.clone());
        let mut script = Script {
            darwin: true,
            fail_get: get,
            fail_set: set,
            ..Script::default()
        };
        if write {
            script
                .writes
                .push_back(SocketCallOutcome::failed(libc::EPIPE));
        }
        assert_ne!(scripted(ctx, response(), 1, &mut script), 0);
        assert!(matches!(&*park.lock().unwrap(), ParkSlot::Live(fds) if fds.is_empty()));
        assert_eq!(peer.read(&mut [0u8; 1]).unwrap(), 0);
        if get || set == Some(1) {
            assert!(!script.calls.contains(&"write"));
        }
    }
}

#[test]
fn http_response_budget_admission_and_busy_park_are_bounded() {
    for ns in [i64::MIN, -1, 0] {
        let Request { ctx, mut peer, .. } = request("GET", true);
        let mut script = Script::default();
        assert_eq!(scripted(ctx, response(), ns, &mut script), AL_INVALID);
        assert_eq!(script.starts, 0);
        assert!(script.calls.is_empty());
        assert_eq!(peer.read(&mut [0u8; 1]).unwrap(), 0);
    }
    let Request {
        ctx,
        mut peer,
        park,
    } = request("GET", true);
    let guard = park.lock().unwrap();
    let mut script = Script {
        expire_pause: true,
        ..Script::default()
    };
    assert_eq!(scripted(ctx, response(), 1, &mut script), AL_TIMEOUT);
    assert_eq!(script.calls, ["pause"]);
    drop(guard);
    assert_eq!(peer.read(&mut [0u8; 1]).unwrap(), 0);
}

#[test]
fn http_response_budget_native_head_reuse_and_stalled_peer_recovery() {
    let Request {
        ctx,
        mut peer,
        park,
    } = request("HEAD", true);
    let _cleanup = ParkCleanup(park.clone());
    assert_eq!(
        unsafe {
            align_rt_http_respond_timeout_ns(
                Box::into_raw(ctx),
                Box::into_raw(response()),
                1_000_000_000,
            )
        },
        0
    );
    let expected = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n";
    let mut head = vec![0; expected.len()];
    peer.read_exact(&mut head).unwrap();
    assert_eq!(head, expected);
    let fd = {
        let mut guard = park.lock().unwrap();
        let ParkSlot::Live(fds) = &mut *guard else {
            panic!("live")
        };
        assert_eq!(fds.len(), 1);
        unsafe { OwnedFd::from_raw_fd(fds.remove(0)) }
    };
    assert_eq!(
        unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) } & libc::O_NONBLOCK,
        0
    );
    peer.write_all(b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
        .unwrap();
    let next = Box::new(unsafe { http_read_request(fd.into_raw_fd(), None) }.unwrap());
    assert_eq!(
        unsafe {
            align_rt_http_respond_timeout_ns(
                Box::into_raw(next),
                Box::into_raw(response()),
                1_000_000_000,
            )
        },
        0
    );
    let mut rest = Vec::new();
    peer.read_to_end(&mut rest).unwrap();
    assert_eq!(
        rest,
        b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 5\r\n\r\nhello"
    );

    let Request {
        ctx,
        peer: stalled_peer,
        park,
    } = request("GET", true);
    let _cleanup = ParkCleanup(park.clone());
    let mut large = response();
    large.body = Some(vec![b'x'; 8 * 1024 * 1024]);
    let before = std::time::Instant::now();
    assert_eq!(
        unsafe {
            align_rt_http_respond_timeout_ns(Box::into_raw(ctx), Box::into_raw(large), 20_000_000)
        },
        AL_TIMEOUT
    );
    assert!(before.elapsed() < std::time::Duration::from_secs(2));
    assert!(matches!(&*park.lock().unwrap(), ParkSlot::Live(fds) if fds.is_empty()));
    drop(stalled_peer);
    let Request { ctx, mut peer, .. } = request("GET", false);
    assert_eq!(
        unsafe {
            align_rt_http_respond_timeout_ns(
                Box::into_raw(ctx),
                Box::into_raw(response()),
                1_000_000_000,
            )
        },
        0
    );
    let mut output = Vec::new();
    peer.read_to_end(&mut output).unwrap();
    assert!(output.ends_with(b"\r\n\r\nhello"));
}

#[test]
fn http_response_budget_expiry_wins_over_each_native_error() {
    for operation in ["get", "set", "restore", "write", "wait"] {
        let Request {
            ctx,
            mut peer,
            park,
        } = request("GET", true);
        let _cleanup = ParkCleanup(park.clone());
        let mut script = Script {
            darwin: true,
            expire_after: Some(operation),
            ..Script::default()
        };
        match operation {
            "get" => script.fail_get = true,
            "set" => script.fail_set = Some(1),
            "restore" => script.fail_set = Some(2),
            "write" => script
                .writes
                .push_back(SocketCallOutcome::failed(libc::EPIPE)),
            "wait" => script
                .writes
                .push_back(SocketCallOutcome::failed(libc::EAGAIN)),
            _ => unreachable!(),
        }
        assert_eq!(
            scripted(ctx, response(), 1, &mut script),
            AL_TIMEOUT,
            "{operation}"
        );
        assert!(matches!(&*park.lock().unwrap(), ParkSlot::Live(fds) if fds.is_empty()));
        assert_eq!(peer.read(&mut [0u8; 1]).unwrap(), 0);
    }
}

#[test]
fn http_response_budget_null_spent_and_invalid_response_cleanup() {
    let Request { ctx, mut peer, .. } = request("GET", true);
    assert_eq!(
        unsafe { align_rt_http_respond_timeout_ns(Box::into_raw(ctx), core::ptr::null_mut(), 1) },
        AL_INVALID
    );
    assert_eq!(peer.read(&mut [0u8; 1]).unwrap(), 0);
    assert_eq!(
        unsafe {
            align_rt_http_respond_timeout_ns(core::ptr::null_mut(), Box::into_raw(response()), 1)
        },
        AL_INVALID
    );
    let Request { ctx, peer, .. } = request("GET", true);
    let mut ctx = ctx;
    let mut stream = core::ptr::null_mut();
    let header = Box::new(ResponseBuilder {
        status: 200,
        headers: Vec::new(),
        body: None,
    });
    assert_eq!(
        unsafe { align_rt_http_respond_stream(ctx.as_mut(), Box::into_raw(header), &mut stream) },
        0
    );
    assert_eq!(
        unsafe {
            align_rt_http_respond_timeout_ns(Box::into_raw(ctx), Box::into_raw(response()), 1)
        },
        AL_INVALID
    );
    unsafe { align_rt_http_stream_free(stream) };
    drop(peer);
    for expiration in [false, true] {
        let Request { ctx, mut peer, .. } = request("HEAD", true);
        let mut invalid = response();
        invalid.status = 204; // A forbidden body still rejects before HEAD suppression.
        let mut script = Script {
            expire_at: expiration.then_some(4),
            ..Script::default()
        };
        assert_eq!(
            scripted(ctx, invalid, 1, &mut script),
            if expiration { AL_TIMEOUT } else { AL_INVALID }
        );
        assert!(script.calls.is_empty());
        assert_eq!(peer.read(&mut [0u8; 1]).unwrap(), 0);
    }
}
