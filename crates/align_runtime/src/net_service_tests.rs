//! Native numeric-service consumers and unchanged admission.

use super::*;

#[test]
fn numeric_service_decimal_and_admission() {
    for port in 1..=65535 {
        let mut expected = port.to_string().into_bytes();
        expected.push(0);
        #[cfg(feature = "alloc-count")]
        let before = global_alloc_count();
        // Moving the value must not preserve a pointer into the constructor's former stack slot.
        let moved = std::hint::black_box([NumericService::new(std::hint::black_box(port)).unwrap()]);
        let service = &moved[0];
        #[cfg(feature = "alloc-count")]
        assert_eq!(global_alloc_count() - before, 0);
        assert_eq!(&service.bytes[service.start..], expected);
        assert_eq!(service.as_ptr(), service.bytes[service.start..].as_ptr());
        assert!(service.bytes[..service.start].iter().all(|byte| *byte == 0));
    }
    for port in [i64::MIN, -1, 0, 65536, i64::MAX] {
        assert!(NumericService::new(port).is_none());
    }
}

#[cfg(feature = "alloc-count")]
#[test]
fn numeric_service_native_call_allocations() {
    use std::os::fd::AsRawFd;
    use std::time::{Duration, Instant};

    struct Handle<T> {
        ptr: *mut T,
        free: unsafe extern "C" fn(*mut T),
    }
    impl<T> Drop for Handle<T> {
        fn drop(&mut self) {
            // SAFETY: the matching constructor publishes only its own handle or null.
            unsafe { (self.free)(self.ptr) };
        }
    }
    fn receive<T>(mut call: impl FnMut() -> std::io::Result<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match call() {
                Ok(value) => return value,
                Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {
                    assert!(Instant::now() < deadline, "native service peer deadline");
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("native service peer: {error}"),
            }
        }
    }

    let witness_before = global_alloc_count();
    let witness = std::hint::black_box(vec![std::hint::black_box(7_u8); 32]);
    assert!(global_alloc_count() > witness_before, "allocation counter must be active");
    drop(witness);

    // Every port remains exclusively owned: no reserve/drop/rebind gap and no peer thread.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let tcp_port = i64::from(listener.local_addr().unwrap().port());
    let receiver = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver.set_nonblocking(true).unwrap();
    let udp_port = i64::from(receiver.local_addr().unwrap().port());
    let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    sender.set_nonblocking(true).unwrap();
    // send_to borrows this stack carrier; the Rust socket alone owns/closes the descriptor.
    let mut socket = UdpSocket { fd: sender.as_raw_fd() };
    let host = b"127.0.0.1";
    let mut counts = [0_u64; 4];
    let mut connection = Handle { ptr: core::ptr::null_mut(), free: align_rt_tcp_conn_free };
    let mut denied_tcp = Handle { ptr: core::ptr::null_mut(), free: align_rt_tcp_listener_free };
    let mut denied_udp = Handle { ptr: core::ptr::null_mut(), free: align_rt_udp_socket_free };

    let before = global_alloc_count();
    let status = unsafe { align_rt_tcp_connect(host.as_ptr(), 9, tcp_port, 1_000_000_000, &mut connection.ptr) };
    counts[0] = global_alloc_count() - before;
    assert_eq!(status, 0);
    assert!(!connection.ptr.is_null());
    let (peer, _) = receive(|| listener.accept());
    drop(peer);

    let before = global_alloc_count();
    let status = unsafe { align_rt_tcp_listen(host.as_ptr(), 9, tcp_port, &mut denied_tcp.ptr) };
    counts[1] = global_alloc_count() - before;
    assert_eq!(status, AL_CODE + libc::EADDRINUSE);
    assert!(denied_tcp.ptr.is_null());

    let before = global_alloc_count();
    let status = unsafe { align_rt_udp_bind(host.as_ptr(), 9, udp_port, &mut denied_udp.ptr) };
    counts[2] = global_alloc_count() - before;
    assert_eq!(status, AL_CODE + libc::EADDRINUSE);
    assert!(denied_udp.ptr.is_null());

    let bytes = b"\0\xffhi";
    let before = global_alloc_count();
    let sent = unsafe { align_rt_udp_send_to(&mut socket, bytes.as_ptr(), 4, host.as_ptr(), 9, udp_port) };
    counts[3] = global_alloc_count() - before;
    assert_eq!(sent, 4);
    let mut received = [0_u8; 16];
    let (n, from) = receive(|| receiver.recv_from(&mut received));
    assert_eq!(&received[..n], bytes);
    assert_eq!(from, sender.local_addr().unwrap());

    eprintln!("native service allocations [connect, occupied listen, occupied bind, send]: {counts:?}");
    // One host CString per call, plus one connection shell on successful connect.
    // Native resolver allocations are not Rust allocations and are outside this claim.
    assert_eq!(counts, [2, 1, 1, 1]);
}

#[test]
fn numeric_service_native_admission() {
    let mut socket = UdpSocket { fd: -1 }; // invalid calls never perform descriptor I/O
    for host in [b"127.0.0.1".as_slice(), b"\xff", b"host\0tail"] {
        let len = i64::try_from(host.len()).unwrap();
        for port in [i64::MIN, -1, 0, 65536, i64::MAX] {
            let mut tcp = core::ptr::dangling_mut::<TcpConn>();
            let mut listener = core::ptr::dangling_mut::<TcpListener>();
            let mut udp = core::ptr::dangling_mut::<UdpSocket>();
            #[cfg(feature = "alloc-count")]
            let before = global_alloc_count();
            assert_eq!(unsafe { align_rt_tcp_connect(host.as_ptr(), len, port, 1, &mut tcp) }, AL_INVALID);
            assert_eq!(unsafe { align_rt_tcp_listen(host.as_ptr(), len, port, &mut listener) }, AL_INVALID);
            assert_eq!(unsafe { align_rt_udp_bind(host.as_ptr(), len, port, &mut udp) }, AL_INVALID);
            assert_eq!(unsafe { align_rt_udp_send_to(&mut socket, b"x".as_ptr(), 1, host.as_ptr(), len, port) }, -i64::from(AL_INVALID));
            #[cfg(feature = "alloc-count")]
            assert_eq!(global_alloc_count() - before, 0, "invalid port precedes host allocation");
            assert!(tcp.is_null() && listener.is_null() && udp.is_null());
        }
    }
    for host in [b"\xff".as_slice(), b"host\0tail"] {
        for port in [1, 65535] {
            let len = i64::try_from(host.len()).unwrap();
            let mut tcp = core::ptr::dangling_mut::<TcpConn>();
            let mut listener = core::ptr::dangling_mut::<TcpListener>();
            let mut udp = core::ptr::dangling_mut::<UdpSocket>();
            assert_eq!(unsafe { align_rt_tcp_connect(host.as_ptr(), len, port, 1, &mut tcp) }, AL_INVALID);
            assert_eq!(unsafe { align_rt_tcp_listen(host.as_ptr(), len, port, &mut listener) }, AL_INVALID);
            assert_eq!(unsafe { align_rt_udp_bind(host.as_ptr(), len, port, &mut udp) }, AL_INVALID);
            assert_eq!(unsafe { align_rt_udp_send_to(&mut socket, b"x".as_ptr(), 1, host.as_ptr(), len, port) }, -i64::from(AL_INVALID));
            assert!(tcp.is_null() && listener.is_null() && udp.is_null());
        }
    }
}
