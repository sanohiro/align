use super::*;
use crate::{align_rt_buffer_free, align_rt_buffer_new, align_rt_free, align_rt_http_serve, align_rt_http_serve_shared};
use std::os::fd::AsRawFd;

fn host(endpoint: NetEndpoint) -> String {
    let text = unsafe { std::slice::from_raw_parts(endpoint.host.ptr, usize::try_from(endpoint.host.len).unwrap()) };
    let s = std::str::from_utf8(text).unwrap().to_owned();
    unsafe { align_rt_free(endpoint.host.ptr.cast_mut()) };
    s
}

#[test]
fn native_address_shape_scope_and_retry() {
    let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
    let v6 = unsafe { &mut *ptr::from_mut(&mut storage).cast::<libc::sockaddr_in6>() };
    v6.sin6_family = libc::AF_INET6 as _;
    v6.sin6_port = 4182_u16.to_be();
    v6.sin6_scope_id = 31;
    v6.sin6_addr.s6_addr = "fe80::2".parse::<Ipv6Addr>().unwrap().octets();
    let size = mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    { v6.sin6_len = size as u8; }
    let mut calls = 0;
    let (n, observed) = observe(|address, len| {
        assert_eq!(usize::try_from(*len).unwrap(), mem::size_of::<libc::sockaddr_storage>());
        calls += 1;
        if calls == 1 {
            *len = 0;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        unsafe { ptr::copy_nonoverlapping(ptr::from_ref(&storage).cast::<u8>(), address.cast(), usize::try_from(size).unwrap()) };
        *len = size;
        Ok(9)
    }).unwrap();
    assert_eq!((n, observed.port), (9, 4182));
    assert_eq!(host(observed), "fe80::2%31");
    for bad in [0, 1, size - 1, size + 1, libc::socklen_t::MAX] {
        assert_eq!(decode_address(&storage, bad).err(), Some(AL_INVALID));
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        storage.ss_len -= 1;
        assert_eq!(decode_address(&storage, size).err(), Some(AL_INVALID));
    }
    storage.ss_family = libc::AF_UNSPEC as _;
    assert_eq!(decode_address(&storage, size).err(), Some(AL_INVALID));
}

#[test]
fn udp_receive_admission_errors_and_initialized_output() {
    let native = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
    native.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
    let sender = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
    let socket = UdpSocket { fd: native.as_raw_fd() };
    let dest = native.local_addr().unwrap();
    let empty = align_rt_buffer_new(0, 1, 0);
    let buf = align_rt_buffer_new(6, 1, 0);
    let mut packet = NetDatagram { n: -1, peer: EMPTY };
    unsafe {
        (*buf).data.with_mut(|data| data.extend_from_slice(b"old"));
        (*buf).len = 3;
        sender.send_to(b"", dest).unwrap();
        assert_eq!(align_rt_udp_recv_datagram(ptr::null(), buf, &mut packet), AL_INVALID);
        assert_eq!(align_rt_udp_recv_datagram(&socket, buf, ptr::null_mut()), AL_INVALID);
        { let b = &*buf; assert_eq!(&b.data[..b.len], b"old"); }
        assert_eq!(align_rt_udp_recv_datagram(&socket, empty, &mut packet), AL_INVALID);
        assert_eq!(packet.n, 0);
        assert!(packet.peer.host.ptr.is_null());
        // A capacity that cannot be represented by Vec fails before consuming the queued message.
        (*buf).cap = usize::MAX;
        assert_eq!(align_rt_udp_recv_datagram(&socket, buf, &mut packet), AL_INVALID);
        assert_eq!((*buf).len, 0);
        (*buf).cap = 6;
        assert_eq!(align_rt_udp_recv_datagram(&socket, buf, &mut packet), 0);
        assert_eq!(packet.n, 0);
        assert_eq!(packet.peer.port, i64::from(sender.local_addr().unwrap().port()));
        assert_eq!(host(packet.peer), "127.0.0.1");
        sender.send_to(b"abcdefghi", dest).unwrap();
        assert_eq!(align_rt_udp_recv_datagram(&socket, buf, &mut packet), 0);
        assert_eq!(packet.n, 6);
        { let b = &*buf; assert_eq!(&b.data[..b.len], b"abcdef"); }
        assert_eq!(host(packet.peer), "127.0.0.1");
        let closed = UdpSocket { fd: -1 };
        let error = io_error_to_status(&io::Error::from_raw_os_error(libc::EBADF));
        assert_eq!(align_rt_udp_recv_datagram(&closed, buf, &mut packet), error);
        assert_eq!((*buf).len, 0);
        assert_eq!(packet.n, 0);
        assert_eq!(packet.peer.port, 0);
        assert!(packet.peer.host.ptr.is_null());
        let mut endpoint = EMPTY;
        assert_eq!(align_rt_udp_local_addr(&closed, &mut endpoint), error);
        assert!(endpoint.host.ptr.is_null());
        align_rt_buffer_free(empty);
        align_rt_buffer_free(buf);
    }
}

#[test]
fn ipv6_socket_observation_and_http_port_policy() {
    for serve in [align_rt_http_serve, align_rt_http_serve_shared] {
        let mut server = ptr::null_mut();
        assert_eq!(unsafe { serve(b"::1".as_ptr(), 3, 0, &mut server) }, AL_INVALID);
        assert!(server.is_null());
    }
    match std::net::UdpSocket::bind(("::1", 0)) {
        Ok(bound) => {
            let socket = UdpSocket { fd: bound.as_raw_fd() };
            let mut address = EMPTY;
            assert_eq!(unsafe { align_rt_udp_local_addr(&socket, &mut address) }, 0);
            assert_eq!(address.port, i64::from(bound.local_addr().unwrap().port()));
            assert_eq!(host(address), "::1");
        }
        Err(e) if matches!(e.raw_os_error(), Some(libc::EAFNOSUPPORT | libc::EADDRNOTAVAIL)) => {}
        Err(e) => panic!("IPv6 loopback bind: {e}"),
    }
}
