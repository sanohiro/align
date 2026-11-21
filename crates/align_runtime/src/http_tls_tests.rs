use super::*;
use crate::{
    AL_TIMEOUT, AlignStr, SSL_accept, SSL_free, SSL_new, SSL_read, SSL_set_fd, SSL_write,
    align_rt_http_client_free, align_rt_http_get_many, align_rt_http_resp_free,
    http_client_perform, http_client_perform_stream, http_get_request,
};
use std::{
    ffi::{c_int, c_long},
    net::TcpListener,
    os::fd::AsRawFd,
    thread,
    time::Duration,
};

fn fixture(name: &str) -> &'static str {
    include_str!("../tests/fixtures/scoped_https.pem.txt")
        .split_once(&format!("[{name}]\n"))
        .expect("named TLS fixture")
        .1
        .split("\n\n")
        .next()
        .unwrap()
        .trim_end()
}
fn concat(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| fixture(name))
        .collect::<Vec<_>>()
        .join("\n")
}
fn der(name: &str) -> Vec<u8> {
    decode_list(
        fixture(name).as_bytes(),
        b"-----BEGIN CERTIFICATE-----",
        b"-----END CERTIFICATE-----",
        false,
    )
    .unwrap()[0]
        .as_slice()
        .to_vec()
}

struct Client(*mut HttpClient);
impl Drop for Client {
    fn drop(&mut self) {
        unsafe { align_rt_http_client_free(self.0) };
    }
}
fn native(parts: [(*const u8, i64); 5], out: *mut *mut HttpClient) -> i32 {
    native_mode(parts, 0, out)
}
fn native_mode(parts: [(*const u8, i64); 5], resume: i32, out: *mut *mut HttpClient) -> i32 {
    let [(a, al), (b, bl), (c, cl), (d, dl), (e, el)] = parts;
    unsafe { align_rt_http_tls_client_new(a, al, b, bl, c, cl, d, dl, e, el, resume, out) }
}
fn client(fields: [&str; 5]) -> Result<Client, i32> {
    client_mode(fields, 0)
}
fn client_mode(fields: [&str; 5], resume: i32) -> Result<Client, i32> {
    let mut ptr = ptr::dangling_mut();
    let status = native_mode(
        fields.map(|s| (s.as_ptr(), i64::try_from(s.len()).unwrap())),
        resume,
        &mut ptr,
    );
    if status != 0 {
        assert!(ptr.is_null());
        Err(status)
    } else {
        assert!(!ptr.is_null());
        Ok(Client(ptr))
    }
}
fn get(client: &Client, url: &str) -> i32 {
    let mut response = ptr::null_mut();
    let code = unsafe { http_client_perform(client.0, http_get_request(url), &mut response, None) };
    if code == 0 {
        assert!(!response.is_null());
        unsafe { align_rt_http_resp_free(response) };
    } else {
        assert!(response.is_null());
    }
    code
}
fn batch(client: &Client, urls: &[&str]) -> i32 {
    let views: Vec<_> = urls
        .iter()
        .map(|url| AlignStr {
            ptr: url.as_ptr(),
            len: url.len() as i64,
        })
        .collect();
    let mut out = AlignStr {
        ptr: ptr::null(),
        len: 0,
    };
    let status = unsafe {
        align_rt_http_get_many(client.0, views.as_ptr(), views.len() as i64, 3, &mut out)
    };
    if status == 0 {
        unsafe { crate::align_rt_free_response_array(out.ptr.cast_mut(), out.len) };
    } else {
        assert_eq!(out.len, 0);
        assert!(out.ptr.is_null());
    }
    status
}

unsafe extern "C" {
    fn SSL_CTX_set_verify(ctx: *mut c_void, mode: c_int, callback: *const c_void);
    fn SSL_CTX_get_cert_store(ctx: *mut c_void) -> *mut c_void;
    fn X509_STORE_add_cert(store: *mut c_void, cert: *mut c_void) -> c_int;
    fn X509_free(cert: *mut c_void);
    fn d2i_X509(cert: *mut *mut c_void, input: *mut *const u8, len: c_long) -> *mut c_void;
    fn i2d_X509(cert: *const c_void, out: *mut *mut u8) -> c_int;
    fn SSL_get0_peer_certificate(ssl: *const c_void) -> *mut c_void;
    fn SSL_get_peer_cert_chain(ssl: *const c_void) -> *mut c_void;
    fn OPENSSL_sk_num(stack: *const c_void) -> c_int;
    fn OPENSSL_sk_value(stack: *const c_void, index: c_int) -> *mut c_void;
}

unsafe fn presented_chain(ssl: *mut c_void) -> Vec<Vec<u8>> {
    let cert = unsafe { SSL_get0_peer_certificate(ssl) };
    if cert.is_null() {
        return Vec::new();
    }
    let chain = unsafe { SSL_get_peer_cert_chain(ssl) };
    std::iter::once(cert)
        .chain((0..unsafe { OPENSSL_sk_num(chain) }).map(|i| unsafe { OPENSSL_sk_value(chain, i) }))
        .map(|x| {
            let size = unsafe { i2d_X509(x, ptr::null_mut()) };
            assert!(size > 0);
            let mut bytes = vec![0; usize::try_from(size).unwrap()];
            let mut p = bytes.as_mut_ptr();
            assert_eq!(unsafe { i2d_X509(x, &mut p) }, size);
            bytes
        })
        .collect()
}

struct Peer {
    url: String,
    handle: thread::JoinHandle<Vec<Option<Vec<Vec<u8>>>>>,
}
fn serve(accepted: usize, trust: &[&str], keep_alive: bool) -> Peer {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!(
        "https://127.0.0.1:{}",
        listener.local_addr().unwrap().port()
    );
    let authorities: Vec<_> = trust.iter().map(|s| der(s)).collect();
    let handle = thread::spawn(move || {
        let chain = concat(&["server", "intermediate"]);
        let ctx = TlsCtx(crate::tests::tls_server_ctx(&chain, fixture("server_key")));
        if !authorities.is_empty() {
            unsafe { SSL_CTX_set_verify(ctx.0, 3, ptr::null()) };
            for auth in authorities {
                let mut p = auth.as_ptr();
                unsafe {
                    let cert = d2i_X509(ptr::null_mut(), &mut p, auth.len() as c_long);
                    assert!(!cert.is_null());
                    assert_eq!(X509_STORE_add_cert(SSL_CTX_get_cert_store(ctx.0), cert), 1);
                    X509_free(cert);
                }
            }
        }
        let mut observations = Vec::new();
        for _ in 0..accepted {
            let mut poll = libc::pollfd {
                fd: listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            assert_eq!(
                unsafe { libc::poll(&mut poll, 1, 5_000) },
                1,
                "expected a fresh TLS connection"
            );
            let (tcp, _) = listener.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            tcp.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
            unsafe {
                let _signals = crate::SigpipeBlock::new();
                let ssl = SSL_new(ctx.0);
                assert!(!ssl.is_null());
                assert_eq!(SSL_set_fd(ssl, tcp.as_raw_fd()), 1);
                if SSL_accept(ssl) == 1 {
                    let mut bytes = [0u8; 1024];
                    let n = SSL_read(ssl, bytes.as_mut_ptr().cast(), 1024);
                    if n > 0 {
                        let response = if keep_alive {
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".as_slice()
                        } else {
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        };
                        assert_eq!(
                            SSL_write(ssl, response.as_ptr().cast(), response.len() as c_int),
                            response.len() as c_int
                        );
                        observations.push(Some(presented_chain(ssl)));
                    } else {
                        observations.push(None);
                    }
                } else {
                    observations.push(None);
                }
                SSL_free(ssl);
            }
        }
        observations
    });
    Peer { url, handle }
}

#[test]
fn scoped_origin_and_descriptor_validation() {
    let origin = "https://LOCALHOST:443/";
    for invalid in [
        "http://a",
        "https://user@a",
        "https://a?",
        "https://a#q",
        "https://a/x",
        "https://a:",
        "https://a:+1",
        "https://a:0",
        "https://a:65536",
        "https://1.2.3",
        "https://0x7f.1",
        "https://[127.0.0.1]",
        "https://::1",
        "https://[fe80::1%2]",
        "https://a.",
        "https://-a",
        "https://a..b",
        "https://あ",
        "https://a\0",
    ] {
        assert_eq!(
            client([invalid, "", "", "", ""]).err(),
            Some(AL_INVALID),
            "{invalid:?}"
        );
    }
    let owned = client([origin, "", "", "", ""]).unwrap();
    let approved = http_get_request("https://localhost/path?q=1");
    let dest = |req| destination(Some(unsafe { &*owned.0 }), req);
    assert_eq!(dest(approved).unwrap().1, "localhost");
    assert_eq!(
        dest(http_get_request("http://localhost/")).err(),
        Some(AL_DENIED)
    );
    assert_eq!(
        dest(http_get_request("https://localhost:444/")).err(),
        Some(AL_DENIED)
    );
    for (name, value) in [
        ("Host", "localhost"),
        ("content-length", "0"),
        ("Transfer-Encoding", "chunked"),
        ("bad name", "x"),
        ("x", "\u{07}"),
    ] {
        let headers = [(name.to_owned(), value.to_owned())];
        assert_eq!(
            destination(
                Some(unsafe { &*owned.0 }),
                HttpRequestView {
                    headers: &headers,
                    ..http_get_request("https://other")
                }
            )
            .err(),
            Some(AL_INVALID)
        );
    }
    let normal = [
        (origin.as_ptr(), origin.len() as i64),
        (ptr::null(), 0),
        (ptr::null(), 0),
        (ptr::null(), 0),
        (ptr::null(), 0),
    ];
    assert_eq!(native(normal, ptr::null_mut()), AL_INVALID);
    for i in 0..5 {
        for invalid in [
            (ptr::null(), 1),
            (ptr::null(), -1),
            (ptr::dangling(), (MAX_PEM + 1) as i64),
            ((usize::MAX - 1) as *const u8, 8),
        ] {
            let mut parts = normal;
            parts[i] = invalid;
            let mut output = ptr::dangling_mut();
            assert_eq!(native(parts, &mut output), AL_INVALID);
            assert!(output.is_null());
        }
    }
}

#[test]
fn scoped_pem_bounds_chain_and_key() {
    let (ca, mid, leaf, key, crl) = (
        fixture("root"),
        fixture("intermediate"),
        fixture("client"),
        fixture("client_key"),
        fixture("mid_crl"),
    );
    let joined = |a: &str, b: &str| format!("{a}\r\n \t{b}\n");
    let many = std::iter::repeat_n(ca, 65).collect::<Vec<_>>().join("\n");
    let chain = joined(leaf, mid);
    let origins = "https://[::1]";
    for (trust, certs, pk, revoked) in [
        ("", "", "", crl),
        (mid, "", "", ""),
        (leaf, "", "", ""),
        (&joined(ca, ca), "", "", ""),
        (&joined(ca, "bad"), "", "", ""),
        (&many, "", "", ""),
        (ca, leaf, "", ""),
        (ca, "", key, ""),
        (ca, &joined(leaf, ca), key, ""),
        (ca, &joined(leaf, mid), fixture("client_b_key"), ""),
        (ca, &joined(leaf, mid), &joined(key, key), ""),
        (ca, &joined(mid, leaf), key, ""),
        (ca, &joined(leaf, leaf), key, ""),
        (ca, leaf, ca, ""),
        (ca, leaf, "-----BEGIN ENCRYPTED PRIVATE KEY-----", ""),
        (ca, "", "", &joined(crl, crl)),
        (ca, "", "", fixture("delta")),
        (" \t\r\n", "", "", ""),
    ] {
        assert_eq!(
            client([origins, trust, certs, pk, revoked]).err(),
            Some(AL_INVALID)
        );
    }
    assert!(client([origins, ca, &chain, key, crl]).is_ok());
    let parse = |s: &[u8]| decode_list(s, b"BEGIN", b"END", true);
    for bad in [
        &b"BEGIN\nYR==\nEND"[..],
        b"BEGIN\nYQ===\nEND",
        b"BEGIN\nY Q=\nEND",
        b"BEGIN\nYQ==\nEND\0",
        b"BEGIN\n\nEND",
    ] {
        assert!(parse(bad).is_err());
    }
    let ok = parse(b" \tBEGIN\r\nY\nQ\r\n=\n=\nEND ").unwrap();
    assert_eq!(ok[0].as_slice(), b"a");
}

#[test]
fn scoped_real_handshakes_and_wire_identity() {
    let _network = crate::tests::GET_MANY_SERVER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ca = fixture("root");
    let chain = concat(&["client", "intermediate"]);
    for (sent, key, receiver, expected) in [
        (
            chain.as_str(),
            fixture("client_key"),
            vec!["root"],
            Some(vec![der("client"), der("intermediate")]),
        ),
        (
            fixture("client"),
            fixture("client_key"),
            vec!["root", "intermediate"],
            Some(vec![der("client")]),
        ),
        ("", "", vec!["root"], None),
        (
            fixture("client_b"),
            fixture("client_b_key"),
            vec!["root"],
            None,
        ),
    ] {
        let peer = serve(1, &receiver, false);
        let config = client([&peer.url, ca, sent, key, ""]).unwrap();
        let status = get(&config, &peer.url);
        assert_eq!(status == 0, expected.is_some(), "TLS status {status}");
        assert_eq!(peer.handle.join().unwrap(), vec![expected]);
    }
    for trust in [fixture("other_root"), ""] {
        let peer = serve(1, &[], false);
        let c = client([&peer.url, trust, "", "", ""]).unwrap();
        assert_eq!(get(&c, &peer.url), AL_DENIED);
        assert_eq!(peer.handle.join().unwrap(), vec![None]);
    }
}

#[test]
fn offline_crls_cover_chain_and_disable_live_reuse() {
    let _network = crate::tests::GET_MANY_SERVER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ca = fixture("root");
    for rows in [
        vec!["root_crl"],
        vec!["mid_crl"],
        vec!["root_crl", "bad_sig"],
        vec!["root_crl", "wrong_issuer"],
        vec!["root_crl", "revoked_leaf"],
        vec!["revoked_mid", "mid_crl"],
    ] {
        let peer = serve(1, &[], false);
        let c = client([&peer.url, ca, "", "", &concat(&rows)]).unwrap();
        assert_eq!(get(&c, &peer.url), AL_DENIED, "{rows:?}");
        assert_eq!(peer.handle.join().unwrap(), vec![None]);
    }
    let peer = serve(2, &[], true);
    let c = client([&peer.url, ca, "", "", &concat(&["root_crl", "mid_crl"])]).unwrap();
    assert_eq!(get(&c, &peer.url), 0);
    drop(unsafe { http_client_perform_stream(c.0, http_get_request(&peer.url)) }.unwrap());
    assert!(unsafe { &*c.0 }.idle.lock().unwrap().is_empty());
    let obs = peer.handle.join().unwrap();
    assert_eq!(obs, vec![Some(vec![]), Some(vec![])]);
    unsafe { &*c.0 }
        .policy
        .as_ref()
        .unwrap()
        .verification_time
        .store(3_787_000_000, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(get(&c, &peer.url), AL_DENIED);
    for bad in ["future", "expired"] {
        let c = client([&peer.url, ca, "", "", &concat(&["root_crl", bad])]).unwrap();
        assert_eq!(get(&c, &peer.url), AL_DENIED);
    }
}

#[test]
fn scoped_batch_isolation_and_error_order() {
    let _network = crate::tests::GET_MANY_SERVER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ca = fixture("root");
    let first = serve(1, &["root"], false);
    let other = serve(1, &["other_root"], false);
    thread::scope(|scope| {
        for (peer, neighbor, certs, key) in [
            (
                &first,
                &other,
                concat(&["client", "intermediate"]),
                fixture("client_key"),
            ),
            (
                &other,
                &first,
                fixture("client_b").to_string(),
                fixture("client_b_key"),
            ),
        ] {
            scope.spawn(move || {
                let c = client([&peer.url, ca, &certs, key, ""]).unwrap();
                assert_eq!(
                    batch(&c, &[&neighbor.url, "https://bad host/", &peer.url]),
                    AL_DENIED
                );
                assert_eq!(batch(&c, &["https://bad host/", &neighbor.url]), AL_INVALID);
            });
        }
    });
    assert_eq!(
        first.handle.join().unwrap(),
        vec![Some(vec![der("client"), der("intermediate")])]
    );
    assert_eq!(
        other.handle.join().unwrap(),
        vec![Some(vec![der("client_b")])]
    );
}

#[test]
fn failed_pem_then_handshake_timeout() {
    let _network = crate::tests::GET_MANY_SERVER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(
        client([
            "https://localhost",
            fixture("root"),
            fixture("client"),
            fixture("client_b_key"),
            ""
        ])
        .is_err()
    );
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!(
        "https://127.0.0.1:{}",
        listener.local_addr().unwrap().port()
    );
    let c = client([&url, fixture("root"), "", "", ""]).unwrap();
    unsafe { crate::align_rt_http_client_timeout(c.0, 50_000_000) };
    assert_eq!(get(&c, &url), AL_TIMEOUT);
}

#[test]
fn scoped_constructor_unwinds_partially_installed_credentials() {
    use crate::crypto_asymmetric::{
        allocation_failpoint_triggered, probe, set_allocation_failpoint,
    };
    let chain = concat(&["client", "intermediate"]);
    let crls = concat(&["root_crl", "mid_crl"]);
    let inputs = [
        "https://localhost",
        fixture("root"),
        &chain,
        fixture("client_key"),
        &crls,
    ];
    assert_eq!(probe::reset(), 0);
    for after in 0..128 {
        set_allocation_failpoint(Some(after));
        let result = client(inputs);
        let failed = allocation_failpoint_triggered();
        set_allocation_failpoint(None);
        assert_eq!(probe::live_sensitive(), 0);
        if failed {
            assert_eq!(result.err(), Some(AL_CODE), "allocation {after}");
        } else {
            assert!(result.is_ok());
            assert!(after > 5 && probe::sensitive_cleanses() > 0);
            return;
        }
    }
    panic!("allocation sweep never reached successful construction");
}

#[test]
fn scoped_certificate_name_and_exact_der_consumption() {
    let _network = crate::tests::GET_MANY_SERVER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    use crate::tests::{TLS_TEST_CA, TLS_WRONG_CERT, TLS_WRONG_KEY, b64_enc, tls_serve};
    for host in ["127.0.0.1", "localhost"] {
        let (port, peer) = tls_serve(TLS_WRONG_CERT, TLS_WRONG_KEY, Vec::new(), 1, true);
        let url = format!("https://{host}:{port}");
        let configured = client([&url, TLS_TEST_CA, "", "", ""]).unwrap();
        assert_eq!(get(&configured, &url), AL_DENIED);
        assert_eq!(peer.join().unwrap(), 1);
    }
    let mut certificate = der("root");
    certificate.push(0);
    let digits = String::from_utf8(b64_enc(&certificate, false)).unwrap();
    let trailing = format!("-----BEGIN CERTIFICATE-----\n{digits}\n-----END CERTIFICATE-----");
    assert_eq!(
        client(["https://localhost", &trailing, "", "", ""]).err(),
        Some(AL_INVALID)
    );
}

#[test]
fn scoped_destination_precedes_pool_checkout_and_reconnect() {
    let _network = crate::tests::GET_MANY_SERVER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // The peer closes after writing a keepalive response, leaving the client one stale socket.
    let peer = serve(2, &[], true);
    let config = client([&peer.url, fixture("root"), "", "", ""]).unwrap();
    assert_eq!(get(&config, &peer.url), 0);
    let denied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    denied.set_nonblocking(true).unwrap();
    let addr = denied.local_addr().unwrap();
    for url in [format!("http://{addr}/"), format!("https://{addr}/")] {
        assert_eq!(get(&config, &url), AL_DENIED);
        assert_eq!(
            unsafe { http_client_perform_stream(config.0, http_get_request(&url)) }.err(),
            Some(AL_DENIED)
        );
        assert_eq!(
            denied.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
    assert!(!unsafe { &*config.0 }.idle.lock().unwrap().is_empty());
    assert_eq!(get(&config, &peer.url), 0);
    drop(config);
    assert_eq!(
        peer.handle.join().unwrap(),
        vec![Some(vec![]), Some(vec![])]
    );
}

#[path = "http_tls_session_tests.rs"]
mod session_tests;
