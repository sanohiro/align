//! HTTP client destruction traverses existing storage and closes every native owner.

use super::*;
use std::io::Read;
use std::os::fd::{AsRawFd, IntoRawFd, OwnedFd};
use std::os::unix::net::UnixStream;

struct Client(*mut HttpClient);
impl Client {
    fn new() -> Self {
        Self(align_rt_http_client_new())
    }
    fn get(&self) -> &HttpClient {
        // SAFETY: this guard exclusively owns the successful native constructor result.
        unsafe { &*self.0 }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: the guard frees its matching constructor result exactly once.
        unsafe { align_rt_http_client_free(self.0) };
    }
}

fn pair() -> (OwnedFd, UnixStream) {
    let (connection, peer) = UnixStream::pair().unwrap();
    connection.set_nonblocking(true).unwrap();
    peer.set_nonblocking(true).unwrap();
    (connection.into(), peer)
}

fn assert_closed(peer: &mut UnixStream, closed: bool) {
    let mut byte = [0];
    match peer.read(&mut byte) {
        Ok(0) => assert!(closed, "retained connection was closed"),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            assert!(!closed, "removed connection remains open");
        }
        other => panic!("unexpected pool peer observation: {other:?}"),
    }
}

#[test]
fn client_destruction_reuses_owned_pool_storage() {
    #[cfg(feature = "alloc-count")]
    {
        let before = global_alloc_count();
        let witness = std::hint::black_box(vec![std::hint::black_box(1_u8); 32]);
        assert!(
            global_alloc_count() > before,
            "allocation counter must be active"
        );
        drop(witness);
    }
    for poisoned in [false, true] {
        let client = Client::new();
        let mut peers = Vec::new();
        for host in ["first.test", "second.test", "third.test"] {
            for scheme in [HttpScheme::Http, HttpScheme::Https] {
                for port in [80, 443] {
                    for _ in 0..HTTP_POOL_MAX_IDLE_PER_HOST {
                        let (connection, peer) = pair();
                        peers.push(peer);
                        client.get().put_idle(
                            scheme,
                            host,
                            port,
                            connection.into_raw_fd(),
                            core::ptr::null_mut(),
                        );
                    }
                }
            }
        }
        for peer in &mut peers {
            assert_closed(peer, false);
        }
        if poisoned {
            // Poison only this fixture's private mutex; retain normal ownership during unwind.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _lock = client.get().idle.lock().unwrap();
                panic!("private pool poison probe");
            }));
            assert!(result.is_err());
        }
        #[cfg(feature = "alloc-count")]
        let before = global_alloc_count();
        drop(client);
        #[cfg(feature = "alloc-count")]
        let allocations = global_alloc_count() - before;
        for peer in &mut peers {
            assert_closed(peer, true);
        }
        #[cfg(feature = "alloc-count")]
        {
            eprintln!(
                "client destruction: poisoned={poisoned}, peers={}, allocations={allocations}",
                peers.len()
            );
            assert_eq!(allocations, 0, "destruction must traverse existing storage");
        }
    }
}

#[test]
fn client_destruction_closes_real_ssl_and_descriptor_pairs() {
    struct Context(*mut c_void);
    impl Drop for Context {
        fn drop(&mut self) {
            unsafe { SSL_CTX_free(self.0) };
        }
    }
    struct Connection {
        ssl: *mut c_void,
        fd: Option<OwnedFd>,
    }
    impl Connection {
        fn into_pool(mut self) -> (i32, *mut c_void) {
            let fd = self.fd.take().unwrap().into_raw_fd();
            (fd, core::mem::replace(&mut self.ssl, core::ptr::null_mut()))
        }
    }
    impl Drop for Connection {
        fn drop(&mut self) {
            // An unpublished SSL owns its BIO but not the OwnedFd; BIO_NOCLOSE is the existing ABI.
            unsafe { SSL_free(self.ssl) };
        }
    }
    let context = Context(unsafe { SSL_CTX_new(TLS_client_method()) });
    assert!(!context.0.is_null());
    for count in [0, 1, HTTP_POOL_MAX_IDLE_PER_HOST] {
        let client = Client::new();
        let mut peers = Vec::new();
        for _ in 0..count {
            let (fd, peer) = pair();
            let connection = Connection {
                ssl: unsafe { SSL_new(context.0) },
                fd: Some(fd),
            };
            assert!(!connection.ssl.is_null());
            assert_eq!(
                unsafe { SSL_set_fd(connection.ssl, connection.fd.as_ref().unwrap().as_raw_fd(),) },
                1
            );
            peers.push(peer);
            let (fd, ssl) = connection.into_pool();
            client
                .get()
                .put_idle(HttpScheme::Https, "tls.test", 443, fd, ssl);
        }
        for peer in &mut peers {
            assert_closed(peer, false);
        }
        let before = TLS_SIGPIPE_GUARD_COUNTS.with(|counts| counts.get().1);
        drop(client);
        assert_eq!(
            TLS_SIGPIPE_GUARD_COUNTS.with(|counts| counts.get().1) - before,
            peers.len()
        );
        for peer in &mut peers {
            assert_closed(peer, true);
        }
    }
}
