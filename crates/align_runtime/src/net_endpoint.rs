//! Owned numeric socket addresses at the native boundary.
use super::{AL_INVALID, AlignStr, Buffer, TcpConn, TcpListener, UdpSocket, io_error_to_status, owned_str_copy};
use std::{io, mem, net::{Ipv4Addr, Ipv6Addr}, ptr};

#[cfg(test)]
#[path = "net_endpoint_tests.rs"]
mod tests;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NetEndpoint {
    pub host: AlignStr,
    pub port: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NetDatagram {
    pub n: i64,
    pub peer: NetEndpoint,
}

pub(super) const EMPTY: NetEndpoint = NetEndpoint { host: AlignStr { ptr: ptr::null(), len: 0 }, port: 0 };
const _: () = {
    assert!(mem::size_of::<NetEndpoint>() == 24 && mem::align_of::<NetEndpoint>() == 8);
    assert!(mem::offset_of!(NetEndpoint, host) == 0 && mem::offset_of!(NetEndpoint, port) == 16);
    assert!(mem::size_of::<NetDatagram>() == 32 && mem::align_of::<NetDatagram>() == 8);
    assert!(mem::offset_of!(NetDatagram, n) == 0 && mem::offset_of!(NetDatagram, peer) == 8);
};

fn decode_address(storage: &libc::sockaddr_storage, len: libc::socklen_t) -> Result<NetEndpoint, i32> {
    use io::Write;
    let len = usize::try_from(len).map_err(|_| AL_INVALID)?;
    if !(mem::offset_of!(libc::sockaddr_storage, ss_family) + mem::size_of::<libc::sa_family_t>()
        ..=mem::size_of::<libc::sockaddr_storage>()).contains(&len) {
        return Err(AL_INVALID);
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    if usize::from(storage.ss_len) != len {
        return Err(AL_INVALID);
    }
    let address = ptr::from_ref(storage);
    // IPv6 text plus '%' and a decimal u32 fits in 64 bytes.
    let mut bytes = [0; 64];
    let mut text = io::Cursor::new(bytes.as_mut_slice());
    let port = match i32::from(storage.ss_family) {
        libc::AF_INET if len == mem::size_of::<libc::sockaddr_in>() => {
            let v4 = unsafe { &*address.cast::<libc::sockaddr_in>() };
            write!(text, "{}", Ipv4Addr::from(v4.sin_addr.s_addr.to_ne_bytes())).map_err(|_| AL_INVALID)?;
            u16::from_be(v4.sin_port)
        }
        libc::AF_INET6 if len == mem::size_of::<libc::sockaddr_in6>() => {
            let v6 = unsafe { &*address.cast::<libc::sockaddr_in6>() };
            write!(text, "{}", Ipv6Addr::from(v6.sin6_addr.s6_addr)).map_err(|_| AL_INVALID)?;
            if v6.sin6_scope_id != 0 {
                write!(text, "%{}", v6.sin6_scope_id).map_err(|_| AL_INVALID)?;
            }
            u16::from_be(v6.sin6_port)
        }
        _ => return Err(AL_INVALID),
    };
    let used = usize::try_from(text.position()).map_err(|_| AL_INVALID)?;
    Ok(NetEndpoint { host: owned_str_copy(&bytes[..used]), port: i64::from(port) })
}

fn observe(mut syscall: impl FnMut(*mut libc::sockaddr, &mut libc::socklen_t) -> io::Result<usize>)
    -> Result<(usize, NetEndpoint), i32> {
    loop {
        // Each attempt supplies the full capacity, even if an interrupted syscall changed it.
        let mut address: libc::sockaddr_storage = unsafe { mem::zeroed() };
        let mut len = mem::size_of_val(&address) as libc::socklen_t;
        match syscall(ptr::from_mut(&mut address).cast(), &mut len) {
            Ok(n) => return Ok((n, decode_address(&address, len)?)),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(io_error_to_status(&e)),
        }
    }
}

unsafe fn local_addr(fd: Option<i32>, out: *mut NetEndpoint) -> i32 {
    let Some(out) = (unsafe { out.as_mut() }) else { return AL_INVALID };
    *out = EMPTY;
    let Some(fd) = fd else { return AL_INVALID };
    match observe(|addr, len| {
        if unsafe { libc::getsockname(fd, addr, len) } == 0 { Ok(0) } else { Err(io::Error::last_os_error()) }
    }) {
        Ok((_, endpoint)) => { *out = endpoint; 0 }
        Err(status) => status,
    }
}

/// Read the local numeric address of a connected socket.
/// # Safety
/// A non-null handle must be live and a non-null output must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_tcp_conn_local_addr(socket: *const TcpConn, out: *mut NetEndpoint) -> i32 {
    unsafe { local_addr(socket.as_ref().map(|s| s.fd), out) }
}

/// Read the numeric address bound by a listener.
/// # Safety
/// A non-null handle must be live and a non-null output must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_tcp_listener_local_addr(socket: *const TcpListener, out: *mut NetEndpoint) -> i32 {
    unsafe { local_addr(socket.as_ref().map(|s| s.fd), out) }
}

/// Read the numeric address bound by a datagram socket.
/// # Safety
/// A non-null handle must be live and a non-null output must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_udp_local_addr(socket: *const UdpSocket, out: *mut NetEndpoint) -> i32 {
    unsafe { local_addr(socket.as_ref().map(|s| s.fd), out) }
}

/// Receive one datagram, retaining its sender and the number of bytes that fit.
/// # Safety
/// Non-null arguments must be valid, with exclusive access to the buffer and output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_udp_recv_datagram(socket: *const UdpSocket, buffer: *mut Buffer, out: *mut NetDatagram) -> i32 {
    let Some(out) = (unsafe { out.as_mut() }) else { return AL_INVALID };
    *out = NetDatagram { n: 0, peer: EMPTY };
    let (Some(socket), Some(buf)) = (unsafe { socket.as_ref() }, unsafe { buffer.as_mut() }) else { return AL_INVALID };
    buf.len = 0;
    if buf.cap == 0 { return AL_INVALID; }
    let Some(dst) = buf.prepare_uninit_window() else { return AL_INVALID };
    match observe(|addr, len| {
        let n = unsafe { libc::recvfrom(socket.fd, dst.cast(), buf.cap, 0, addr, len) };
        usize::try_from(n).map_err(|_| io::Error::last_os_error())
    }) {
        Ok((n, peer)) => {
            // With flags zero, recvfrom returns exactly the initialized prefix, bounded by cap.
            buf.data.with_mut(|data| unsafe { data.set_len(n) });
            buf.len = n;
            *out = NetDatagram { n: n as i64, peer };
            0
        }
        Err(status) => status,
    }
}
