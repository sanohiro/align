use super::*;
use std::{net::TcpStream, time::Instant};

unsafe extern "C" {
    fn SSL_session_reused(ssl: *const c_void) -> c_int;
    fn SSL_CTX_set_session_id_context(ctx: *mut c_void, sid: *const u8, len: u32) -> c_int;
    fn SSL_CTX_set_options(ctx: *mut c_void, options: u64) -> u64;
    fn SSL_CTX_set_timeout(ctx: *mut c_void, seconds: c_long) -> c_long;
    fn SSL_set_num_tickets(ssl: *mut c_void, count: usize) -> c_int;
}

const TLS12: c_long = 0x0303;
const TLS13: c_long = 0x0304;

#[derive(Clone, Copy)]
enum Exchange {
    Body,
    Bodyless,
    Truncated,
    FreshContext,
    Untrusted,
    NoTicket,
    Sse,
    Overlap,
}

#[derive(Debug, Default)]
struct Seen {
    handshake: bool,
    resumed: bool,
    request: bool,
}
struct Server {
    url: String,
    worker: thread::JoinHandle<Vec<Seen>>,
}

fn context(version: c_long, id_only: bool, chain: &str, key: &str, timeout: c_long) -> TlsCtx {
    let ctx = TlsCtx(crate::tests::tls_server_ctx(chain, key));
    unsafe {
        if timeout > 0 {
            SSL_CTX_set_timeout(ctx.0, timeout);
        }
        for command in [123, 124] {
            assert_eq!(
                crate::SSL_CTX_ctrl(ctx.0, command, version, ptr::null_mut()),
                1
            );
        }
        assert_eq!(
            SSL_CTX_set_session_id_context(ctx.0, b"align".as_ptr(), 5),
            1
        );
        if id_only {
            SSL_CTX_set_options(ctx.0, 1 << 14);
        }
    }
    ctx
}

fn accept(listener: &TcpListener) -> TcpStream {
    let mut poll = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    assert_eq!(
        unsafe { libc::poll(&mut poll, 1, 5000) },
        1,
        "missing fresh connection"
    );
    let (tcp, _) = listener.accept().unwrap();
    tcp.set_nodelay(true).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    tcp.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
    tcp
}

fn server(version: c_long, id_only: bool, script: Vec<Exchange>) -> Server {
    configured_server(
        version,
        id_only,
        script,
        concat(&["server", "intermediate"]),
        fixture("server_key"),
        0,
    )
}
fn configured_server(
    version: c_long,
    id_only: bool,
    script: Vec<Exchange>,
    chain: String,
    key: &'static str,
    timeout: c_long,
) -> Server {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("https://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let mut ctx = context(version, id_only, &chain, key, timeout);
        let mut observations = Vec::new();
        let _signals = crate::SigpipeBlock::new();
        let mut queued = None;
        for step in script {
            if matches!(step, Exchange::FreshContext | Exchange::Untrusted) {
                ctx = if matches!(step, Exchange::Untrusted) {
                    context(
                        version,
                        id_only,
                        fixture("client_b"),
                        fixture("client_b_key"),
                        timeout,
                    )
                } else {
                    context(version, id_only, &chain, key, timeout)
                };
            }
            let tcp = queued.take().unwrap_or_else(|| accept(&listener));
            if matches!(step, Exchange::Overlap) {
                let other = accept(&listener);
                for socket in [&tcp, &other] {
                    assert_eq!(socket.peek(&mut [0]).unwrap(), 1);
                }
                queued = Some(other);
            }
            unsafe {
                let ssl = SSL_new(ctx.0);
                assert!(!ssl.is_null());
                assert_eq!(SSL_set_fd(ssl, tcp.as_raw_fd()), 1);
                if matches!(step, Exchange::NoTicket) {
                    assert_eq!(SSL_set_num_tickets(ssl, 0), 1);
                }
                let mut observation = Seen::default();
                if SSL_accept(ssl) == 1 {
                    observation.handshake = true;
                    observation.resumed = SSL_session_reused(ssl) != 0;
                    let mut request = [0u8; 512];
                    let n = SSL_read(ssl, request.as_mut_ptr().cast(), 512);
                    observation.request = n > 0;
                    if observation.request {
                        let data: &[u8] = match step {
                            Exchange::Truncated => b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nx",
                            Exchange::Bodyless => b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
                            Exchange::Sse => b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 9\r\nConnection: close\r\n\r\ndata: x\n\n",
                            _ => b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody",
                        };
                        let len = c_int::try_from(data.len()).unwrap();
                        assert_eq!(SSL_write(ssl, data.as_ptr().cast(), len), len);
                    }
                    crate::SSL_shutdown(ssl);
                }
                SSL_free(ssl);
                observations.push(observation);
            }
        }
        observations
    });
    Server { url, worker }
}

fn private(url: &str, enabled: bool) -> Client {
    client_mode([url, fixture("root"), "", "", ""], i32::from(enabled)).unwrap()
}
fn hits(server: Server) -> Vec<bool> {
    server
        .worker
        .join()
        .unwrap()
        .into_iter()
        .map(|seen| {
            assert!(seen.handshake && seen.request, "{seen:?}");
            seen.resumed
        })
        .collect()
}
fn network() -> std::sync::MutexGuard<'static, ()> {
    crate::tests::GET_MANY_SERVER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn resumption_flag_and_private_trust_admission() {
    let origin = "https://localhost";
    for flag in [-1, 2, 256] {
        assert_eq!(
            client_mode([origin, fixture("root"), "", "", ""], flag).err(),
            Some(AL_INVALID)
        );
    }
    assert_eq!(
        client_mode([origin, "", "", "", ""], 1).err(),
        Some(AL_INVALID)
    );
    assert_eq!(
        client_mode([origin, fixture("root"), "", "", fixture("root_crl")], 1).err(),
        Some(AL_INVALID)
    );
}

#[test]
fn resumes_verified_fresh_connections_in_both_protocols() {
    let _network = network();
    for (version, id_only) in [(TLS12, false), (TLS12, true), (TLS13, false)] {
        let peer = server(version, id_only, vec![Exchange::Body; 5]);
        let first = private(&peer.url, true);
        let other = private(&peer.url, true);
        let disabled = private(&peer.url, false);
        for client in [&first, &other, &first, &disabled, &disabled] {
            assert_eq!(get(client, &peer.url), 0, "TLS version {version}");
        }
        assert_eq!(
            hits(peer),
            [false, false, true, false, false],
            "{version} id_only={id_only}"
        );
    }
}

#[test]
fn failed_exchanges_do_not_publish_and_declined_offers_verify_the_new_peer() {
    let _network = network();
    let peer = server(
        TLS13,
        false,
        vec![
            Exchange::Truncated,
            Exchange::NoTicket,
            Exchange::Bodyless,
            Exchange::FreshContext,
            Exchange::Untrusted,
        ],
    );
    let c = private(&peer.url, true);
    assert_ne!(get(&c, &peer.url), 0);
    for _ in 0..3 {
        assert_eq!(get(&c, &peer.url), 0);
    }
    assert_eq!(get(&c, &peer.url), AL_DENIED);
    let observed = peer.worker.join().unwrap();
    assert!(observed[0..4].iter().all(|s| s.request && !s.resumed));
    assert!(!observed[4].request);
}

#[test]
fn single_use_ticket_requires_new_issuance_after_a_hit() {
    let _network = network();
    let peer = server(
        TLS13,
        false,
        vec![Exchange::Body, Exchange::NoTicket, Exchange::Body],
    );
    let c = private(&peer.url, true);
    for _ in 0..3 {
        assert_eq!(get(&c, &peer.url), 0);
    }
    assert_eq!(hits(peer), [false, true, false]);
}

fn clock(client: &Client, samples: &[(i64, i64)]) {
    let policy = unsafe { &*client.0 }.policy.as_ref().unwrap();
    let mut times = policy.session_clock.lock().unwrap();
    assert!(times.is_empty());
    times.extend(
        samples
            .iter()
            .map(|&(wall, monotonic)| SessionClock { wall, monotonic }),
    );
}
fn at(client: &Client, seconds: i64, nanos: i64) {
    clock(client, &[(seconds, nanos); 3]);
}
fn wall() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .try_into()
        .unwrap()
}

#[test]
fn resumed_authentication_expires_at_original_age_and_rechecks_after_handshake() {
    let _network = network();
    let start = wall() + 5;
    let peer = server(TLS13, false, vec![Exchange::Body; 4]);
    let c = private(&peer.url, true);
    for (seconds, monotonic) in [
        (start, 0),
        (start + 89, 89_999_999_999),
        (start + 90, 90_000_000_000),
        (start - 20, 90_000_000_001),
    ] {
        at(&c, seconds, monotonic);
        assert_eq!(get(&c, &peer.url), 0);
    }
    assert_eq!(hits(peer), [false, true, false, false]);
    let peer = server(TLS13, false, vec![Exchange::Body; 2]);
    let c = private(&peer.url, true);
    at(&c, start, 0);
    assert_eq!(get(&c, &peer.url), 0);
    clock(
        &c,
        &[(start + 89, 89_000_000_000), (start + 90, 90_000_000_000)],
    );
    assert_eq!(get(&c, &peer.url), AL_DENIED);
    let seen = peer.worker.join().unwrap();
    assert!(seen[1].resumed && !seen[1].request, "{seen:?}");
}

#[test]
fn concurrent_batch_claims_each_issuance_at_most_once() {
    let _network = network();
    let peer = server(
        TLS13,
        false,
        vec![Exchange::Body, Exchange::Overlap, Exchange::Body],
    );
    let c = private(&peer.url, true);
    assert_eq!(get(&c, &peer.url), 0);
    assert_eq!(batch(&c, &[&peer.url, &peer.url]), 0);
    let chosen = hits(peer);
    assert!(!chosen[0]);
    assert_eq!(chosen[1..].iter().filter(|&&hit| hit).count(), 1);
}

#[test]
fn completed_raw_and_sse_streams_publish_but_abandoned_streams_do_not() {
    let _network = network();
    let peer = server(
        TLS13,
        false,
        vec![
            Exchange::Body,
            Exchange::Body,
            Exchange::Sse,
            Exchange::Body,
        ],
    );
    let c = private(&peer.url, true);
    let stream =
        || unsafe { http_client_perform_stream(c.0, http_get_request(&peer.url)) }.unwrap();
    drop(stream());
    let mut raw = stream();
    let mut storage = [std::mem::MaybeUninit::uninit(); 32];
    assert_eq!(raw.read_into(&mut storage).unwrap(), 4);
    drop(raw);
    let mut events = stream();
    events.sse = Some(crate::HttpSseState::new(true));
    assert!(events.next_sse(&mut storage, 32).unwrap().is_some());
    assert!(events.next_sse(&mut storage, 32).unwrap().is_none());
    drop(events);
    assert_eq!(get(&c, &peer.url), 0);
    assert_eq!(hits(peer), [false, false, true, true]);
}

#[test]
#[ignore = "local handshake measurement on distinct loopback TCP connections"]
fn measure_full_and_resumed_handshakes() {
    let _network = network();
    for version in [TLS12, TLS13] {
        for enabled in [false, true] {
            let measured = 40u32;
            let peer = server(
                version,
                false,
                vec![Exchange::Body; usize::try_from(measured + 1).unwrap()],
            );
            let c = private(&peer.url, enabled);
            assert_eq!(get(&c, &peer.url), 0);
            let timings: Vec<_> = (0..measured)
                .map(|_| {
                    let start = Instant::now();
                    assert_eq!(get(&c, &peer.url), 0);
                    start.elapsed()
                })
                .collect();
            let chosen = hits(peer);
            let mut groups = [Vec::new(), Vec::new()];
            for (elapsed, &hit) in timings.into_iter().zip(&chosen[1..]) {
                groups[usize::from(hit)].push(elapsed);
            }
            assert_eq!(!groups[1].is_empty(), enabled);
            for (resumed, samples) in groups.iter_mut().enumerate() {
                samples.sort_unstable();
                if let Some(median) = samples.get(samples.len() / 2) {
                    eprintln!(
                        "TLS {version:#x} enabled={enabled} resumed={resumed} n={} median={median:?}",
                        samples.len()
                    );
                }
            }
        }
    }
}

fn dated(name: &str) -> &'static str {
    include_str!("../tests/fixtures/session_validity.pem.txt")
        .split_once(&format!("[{name}]\n"))
        .unwrap()
        .1
        .split("\n\n")
        .next()
        .unwrap()
        .trim_end()
}
#[test]
fn earliest_peer_path_or_client_certificate_bounds_the_offer() {
    let _network = network();
    let start = wall() + 5;
    // TLS 1.2 ID-only has no ticket lifetime hint. The native session outlives these test dates.
    let timeout = 2_000_000_000;
    for (mid, leaf, identity, expiry) in [
        ("mid", "server", false, 3_155_760_000), // 2070-01-01, root
        ("mid_short", "server", false, 2_840_140_800), // 2060-01-01, intermediate
        ("mid", "server_short", false, 2_524_608_000), // 2050-01-01, leaf
        ("mid", "server", true, 2_208_988_800),  // 2040-01-01, client
    ] {
        for (delta, expected) in [(-1, true), (0, false)] {
            let peer = configured_server(
                TLS12,
                true,
                vec![Exchange::Body; 2],
                format!("{}\n{}", dated(leaf), dated(mid)),
                dated("server_key"),
                timeout,
            );
            let (local, key) = if identity {
                (dated("client_short"), dated("client_key"))
            } else {
                ("", "")
            };
            let c = client_mode([&peer.url, dated("root"), local, key, ""], 1).unwrap();
            unsafe {
                SSL_CTX_set_timeout((*c.0).policy.as_ref().unwrap().ctx.0, timeout);
            }
            at(&c, start, 0);
            assert_eq!(get(&c, &peer.url), 0);
            at(&c, expiry + delta, 1);
            assert_eq!(get(&c, &peer.url), if expected { 0 } else { AL_DENIED });
            let observations = peer.worker.join().unwrap();
            let last = &observations[1];
            assert!(last.handshake);
            assert_eq!(
                (last.resumed, last.request),
                (expected, expected),
                "{mid} {leaf} identity={identity} delta={delta}"
            );
        }
    }
}

#[test]
fn native_session_lifetime_and_server_ticket_hint_bound_reuse() {
    let _network = network();
    for (version, id_only, native, hint) in [(TLS12, true, 30, 3600), (TLS13, false, 3600, 30)] {
        let peer = configured_server(
            version,
            id_only,
            vec![Exchange::Body; 3],
            concat(&["server", "intermediate"]),
            fixture("server_key"),
            hint,
        );
        let c = private(&peer.url, true);
        unsafe {
            SSL_CTX_set_timeout((*c.0).policy.as_ref().unwrap().ctx.0, native);
        }
        let now = wall() + 5;
        at(&c, now, 0);
        assert_eq!(get(&c, &peer.url), 0);
        at(&c, now + 35, 1);
        assert_eq!(get(&c, &peer.url), 0);
        at(&c, now + 36, 2);
        assert_eq!(get(&c, &peer.url), 0);
        assert_eq!(hits(peer), [false, false, false]);
    }
}

#[test]
fn ticket_survives_producing_connection_drop_after_another_worker_offers_it() {
    let _network = network();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("https://{}", listener.local_addr().unwrap());
    let c = private(&url, true);
    let (offered, hello) = std::sync::mpsc::channel();
    let (retired, release) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let _signals = crate::SigpipeBlock::new();
        let ctx = context(
            TLS13,
            false,
            &concat(&["server", "intermediate"]),
            fixture("server_key"),
            0,
        );
        let first = accept(&listener);
        unsafe {
            let session = |socket: &TcpStream| {
                let ssl = SSL_new(ctx.0);
                assert!(!ssl.is_null());
                assert_eq!(SSL_set_fd(ssl, socket.as_raw_fd()), 1);
                assert_eq!(SSL_accept(ssl), 1);
                ssl
            };
            let reply = |ssl, wire: &[u8]| {
                let mut bytes = [0u8; 512];
                assert!(SSL_read(ssl, bytes.as_mut_ptr().cast(), 512) > 0);
                let n = c_int::try_from(wire.len()).unwrap();
                assert_eq!(SSL_write(ssl, wire.as_ptr().cast(), n), n);
            };
            let a = session(&first);
            reply(a, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            reply(a, b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n");
            let second = accept(&listener);
            assert_eq!(second.peek(&mut [0]).unwrap(), 1);
            offered.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(5)).unwrap();
            SSL_free(a);
            let b = session(&second);
            let reused = SSL_session_reused(b) != 0;
            reply(b, b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
            crate::SSL_shutdown(b);
            SSL_free(b);
            reused
        }
    });
    assert_eq!(get(&c, &url), 0);
    let stream = unsafe { http_client_perform_stream(c.0, http_get_request(&url)) }.unwrap();
    let shared = unsafe { &*c.0 };
    thread::scope(|scope| {
        let request = scope.spawn(|| {
            let mut response = ptr::null_mut();
            let status = unsafe {
                http_client_perform(
                    ptr::from_ref(shared).cast_mut(),
                    http_get_request(&url),
                    &mut response,
                    None,
                )
            };
            assert_eq!(status, 0);
            unsafe { align_rt_http_resp_free(response) };
        });
        hello.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(stream);
        retired.send(()).unwrap();
        request.join().unwrap();
    });
    assert!(worker.join().unwrap());
}

#[test]
fn session_lifetime_hint_follows_the_negotiated_protocol() {
    unsafe extern "C" {
        fn align_tls_session_bounds(
            protocol: c_int,
            ticket: c_int,
            issued: i64,
            lifetime: c_long,
            hint: std::ffi::c_ulong,
            until: *mut i64,
        ) -> c_int;
    }
    for (version, ticket, hint, expected) in [
        (0x0303, 0, 0, Some(160)),
        (0x0303, 1, 0, Some(160)),
        (0x0303, 1, 20, Some(120)),
        (0x0304, 1, 0, None),
        (0x0304, 0, 20, None),
        (0x0304, 1, 20, Some(120)),
        (0x0304, 1, 120, Some(160)),
    ] {
        let mut until = -1;
        let ok =
            unsafe { align_tls_session_bounds(version, ticket, 100, 60, hint, &mut until) } != 0;
        assert_eq!(
            ok.then_some(until),
            expected,
            "{version} ticket={ticket} hint={hint}"
        );
    }
}

#[test]
fn full_and_fallback_handshakes_recheck_the_peer_before_sending_http() {
    let _network = network();
    let now = wall() + 5;
    let expires = 2_524_608_000; // server_short, 2050-01-01
    for fallback in [false, true] {
        let script = if fallback {
            vec![Exchange::Body, Exchange::FreshContext]
        } else {
            vec![Exchange::Body]
        };
        let peer = configured_server(
            TLS13,
            false,
            script,
            format!("{}\n{}", dated("server_short"), dated("mid")),
            dated("server_key"),
            0,
        );
        let c = client_mode([&peer.url, dated("root"), "", "", ""], 1).unwrap();
        if fallback {
            at(&c, now, 0);
            assert_eq!(get(&c, &peer.url), 0);
        }
        clock(&c, &[(now, 1), (expires, 2)]);
        assert_eq!(get(&c, &peer.url), AL_DENIED);
        let observed = peer.worker.join().unwrap();
        let last = observed.last().unwrap();
        assert!(
            last.handshake && !last.resumed && !last.request,
            "{observed:?}"
        );
    }
}
