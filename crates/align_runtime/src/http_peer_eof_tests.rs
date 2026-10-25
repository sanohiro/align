//! Plan243: one-attempt observation, native admission and unchanged request ownership.
use super::*;
use std::cell::Cell;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

fn request(residual: bool) -> (Box<HttpRequestCtx>, UnixStream) {
    let (socket, mut peer) = UnixStream::pair().unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    peer.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
    let wire = if residual {
        b"GET /first HTTP/1.1\r\nHost: h\r\n\r\nGET /next HTTP/1.1\r\nHost: h\r\n\r\n".as_slice()
    } else { b"GET /first HTTP/1.1\r\nHost: h\r\n\r\n".as_slice() };
    peer.write_all(wire).unwrap();
    let ctx = Box::new(unsafe { http_read_request(socket.into_raw_fd(), None) }.unwrap());
    (ctx, peer)
}

#[test]
fn http_peer_eof_one_attempt_outcomes_and_allocation() {
    let _: unsafe extern "C" fn(*const HttpRequestCtx, *mut u8) -> i32 = align_rt_http_ctx_peer_eof;
    let (ctx, _peer) = request(false);
    for (count, errno, expected_status, expected_value) in [
        (0, 0, 0, 1), (1, 0, 0, 0),
        (0, libc::EAGAIN, 0, 0), (0, libc::EWOULDBLOCK, 0, 0),
        (0, libc::EINTR, AL_CODE + libc::EINTR, 0),
        (0, libc::ECONNRESET, AL_CODE + libc::ECONNRESET, 0),
    ] {
        let calls = Cell::new(0);
        let mut out = 9;
        #[cfg(feature = "alloc-count")]
        let before = (global_alloc_count(), GLOBAL_DEALLOC_CALLS.with(Cell::get));
        let status = unsafe { http_peer_eof_with(&*ctx, &mut out, |fd, byte, length, flags| {
            assert!(!byte.is_null());
            assert_eq!((length, flags), (1, libc::MSG_PEEK | libc::MSG_DONTWAIT));
            assert_eq!(fd, ctx.fd);
            calls.set(calls.get() + 1);
            if errno == 0 { Ok(count) } else { Err(std::io::Error::from_raw_os_error(errno)) }
        }) };
        #[cfg(feature = "alloc-count")]
        let after = (global_alloc_count(), GLOBAL_DEALLOC_CALLS.with(Cell::get));
        assert_eq!((status, out, calls.get()), (expected_status, expected_value, 1));
        #[cfg(feature = "alloc-count")]
        assert_eq!(before, after, "observation must not allocate or free");
    }
}

#[test]
fn http_peer_eof_native_admission_precedence() {
    let (mut ctx, _peer) = request(false);
    let pointer = (&mut *ctx) as *mut HttpRequestCtx;
    let mut output = 77u8;
    let output_ptr = &mut output as *mut u8;
    let overflow = usize::MAX & !(core::mem::align_of::<HttpRequestCtx>() - 1);
    for (input, out) in [
        (core::ptr::null(), output_ptr),
        (pointer.cast::<u8>().wrapping_add(1).cast_const().cast(), output_ptr),
        (pointer.cast_const(), core::ptr::null_mut()),
        (pointer.cast_const(), pointer.cast::<u8>()),
        (pointer.cast_const(), pointer.cast::<u8>().wrapping_add(core::mem::size_of::<HttpRequestCtx>() - 1)),
        (core::ptr::without_provenance::<HttpRequestCtx>(overflow), output_ptr),
        (pointer.cast_const(), core::ptr::without_provenance_mut::<u8>(usize::MAX)),
    ] {
        let calls = Cell::new(0);
        let before_fd = ctx.fd;
        assert_eq!(unsafe { http_peer_eof_with(input, out, |_, _, _, _| { calls.set(calls.get() + 1); Ok(0) }) }, AL_INVALID);
        assert_eq!((calls.get(), output, ctx.fd), (0, 77, before_fd));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(std::mem::replace(&mut ctx.fd, -1)) };
    assert_eq!(unsafe { http_peer_eof_with(&*ctx, &mut output, |_, _, _, _| panic!("invalid fd reached recv")) }, AL_INVALID);
    ctx.fd = fd.into_raw_fd();
    assert_eq!(output, 0);
    assert_eq!(unsafe { http_peer_eof_with(&*ctx, &mut output, |_, _, _, _| Ok(0)) }, 0);
    assert_eq!(output, 1);
}

fn observe(ctx: &HttpRequestCtx) -> (i32, u8) {
    let mut value = 99;
    #[cfg(feature = "alloc-count")]
    let before = (global_alloc_count(), GLOBAL_DEALLOC_CALLS.with(Cell::get));
    let status = unsafe { align_rt_http_ctx_peer_eof(ctx, &mut value) };
    #[cfg(feature = "alloc-count")]
    assert_eq!((global_alloc_count(), GLOBAL_DEALLOC_CALLS.with(Cell::get)), before);
    (status, value)
}

#[test]
fn http_peer_eof_live_half_close_and_queued_bytes() {
    let (ctx, mut peer) = request(false);
    let before = (ctx.buf.clone(), ctx.keep_alive, ctx.upgrade_ready,
        unsafe { libc::fcntl(ctx.fd, libc::F_GETFL) });
    assert_eq!(observe(&ctx), (0, 0));
    assert_eq!(observe(&ctx), (0, 0));
    peer.write_all(b"queued").unwrap();
    peer.shutdown(Shutdown::Write).unwrap();
    assert_eq!(observe(&ctx), (0, 0), "queued bytes hide later EOF");
    let mut queued = [0u8; 6];
    assert_eq!(unsafe { libc::recv(ctx.fd, queued.as_mut_ptr().cast(), queued.len(), 0) }, 6);
    assert_eq!(&queued, b"queued");
    assert_eq!(observe(&ctx), (0, 1));
    assert_eq!(observe(&ctx), (0, 1));
    assert_eq!((ctx.buf.clone(), ctx.keep_alive, ctx.upgrade_ready,
        unsafe { libc::fcntl(ctx.fd, libc::F_GETFL) }), before);
    let response = Box::new(ResponseBuilder { status: 200, headers: Vec::new(), body: Some(b"ok".to_vec()) });
    assert_eq!(unsafe { align_rt_http_respond_timeout_ns(Box::into_raw(ctx), Box::into_raw(response), 1_000_000_000) }, 0);
    let mut bytes = Vec::new();
    peer.read_to_end(&mut bytes).unwrap();
    assert!(bytes.ends_with(b"\r\n\r\nok"), "send-half-close still receives a response");
}

#[test]
fn http_peer_eof_co_read_residual_and_drop() {
    let (ctx, mut peer) = request(true);
    assert!(!ctx.keep_alive && !ctx.upgrade_ready, "fixture must co-read residual bytes");
    let before = ctx.buf.clone();
    peer.shutdown(Shutdown::Write).unwrap();
    assert_eq!(observe(&ctx), (0, 1), "co-read residual data is outside the kernel queue");
    assert_eq!(ctx.buf, before);
    drop(ctx);
    let mut bytes = Vec::new();
    peer.read_to_end(&mut bytes).unwrap();
    assert!(bytes.is_empty());
    let (ctx, peer) = request(false);
    drop(peer);
    assert_eq!(observe(&ctx), (0, 1), "orderly full close also observes EOF");
}
