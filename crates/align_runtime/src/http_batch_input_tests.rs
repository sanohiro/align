//! Scoped HTTP batches borrow valid input bytes and still execute every selected exchange.

use super::*;

fn view(bytes: &[u8]) -> AlignStr {
    AlignStr {
        ptr: bytes.as_ptr(),
        len: i64::try_from(bytes.len()).unwrap(),
    }
}

#[test]
fn batch_url_storage_preserves_bytes_and_native_normalization() {
    let owner = "prefixhttp://example.test/日本語\0suffix".to_owned();
    let bytes = &owner.as_bytes()[6..];
    let invalid = b"http://example.test/\xff\xc0x";
    let views = [
        view(bytes),
        view(bytes),
        view(&bytes[..20]),
        view(b""),
        view(invalid),
        AlignStr {
            ptr: core::ptr::null(),
            len: 7,
        },
        AlignStr {
            ptr: core::ptr::null(),
            len: 0,
        },
        AlignStr {
            ptr: bytes.as_ptr(),
            len: -1,
        },
    ];
    let urls = unsafe { http_get_many_urls(&views) };
    for (i, expected) in [bytes, bytes, &bytes[..20], b""].into_iter().enumerate() {
        assert!(matches!(urls[i], std::borrow::Cow::Borrowed(_)));
        assert_eq!(urls[i].as_bytes(), expected);
        assert_eq!(urls[i].as_ptr(), expected.as_ptr());
    }
    assert!(matches!(urls[4], std::borrow::Cow::Owned(_)));
    assert_eq!(urls[4], "http://example.test/\u{fffd}\u{fffd}x");
    for normalized in &urls[5..] {
        assert_eq!(normalized, "");
    }
    assert_eq!(owner, "prefixhttp://example.test/日本語\0suffix");
}

#[cfg(feature = "alloc-count")]
#[test]
fn batch_url_preparation_allocates_only_the_view_vector() {
    let before = global_alloc_count();
    let witness = std::hint::black_box(vec![std::hint::black_box(7_u8); 32]);
    assert!(global_alloc_count() > before, "allocator counter is active");
    drop(witness);
    for n in [0_usize, 1, 8, 64, 1024] {
        for length in [32_usize, 2048] {
            let prefix = "http://example.test/";
            let input = format!("{prefix}{}", "u".repeat(length - prefix.len()));
            let views: Vec<_> = (0..n).map(|_| view(input.as_bytes())).collect();
            let before = global_alloc_count();
            let urls = unsafe { http_get_many_urls(std::hint::black_box(&views)) };
            let allocations = global_alloc_count() - before;
            assert_eq!(allocations, u64::from(n != 0), "n={n}, length={length}");
            for url in &urls {
                let before = global_alloc_count();
                let request = std::hint::black_box(http_get_request(std::hint::black_box(url)));
                let allocations = global_alloc_count() - before;
                assert_eq!(allocations, 0);
                assert_eq!(request.url.as_ptr(), input.as_ptr());
                assert_eq!(request.method, "GET");
                assert!(request.body.is_empty() && request.headers.is_empty());
                assert!(!request.body_present);
                assert_eq!(
                    (request.timeout_ns, request.max_response_body_bytes),
                    (0, 0)
                );
            }
        }
    }
}

struct ResponseBatch(AlignStr);
impl Drop for ResponseBatch {
    fn drop(&mut self) {
        unsafe { align_rt_free_response_array(self.0.ptr.cast_mut(), self.0.len) };
    }
}

#[test]
fn batch_invalid_url_still_runs_live_peers_in_both_input_orders() {
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};
    for invalid in [b"not-a-url".as_slice(), b"http://example.test/\0", b""] {
        for invalid_first in [false, true] {
            for workers in [1, 2] {
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                listener.set_nonblocking(true).unwrap();
                let url = format!(
                    "http://127.0.0.1:{}/live",
                    listener.local_addr().unwrap().port()
                );
                std::thread::scope(|scope| {
                    let peer = scope.spawn(move || {
                        let deadline = Instant::now() + Duration::from_secs(3);
                        let mut socket = loop {
                            assert!(Instant::now() < deadline, "valid peer was skipped");
                            match listener.accept() {
                                Ok((socket, _)) => break socket,
                                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {
                                    std::thread::sleep(Duration::from_millis(1));
                                }
                                Err(e) => panic!("accept: {e}"),
                            }
                        };
                        socket.set_nonblocking(false).unwrap();
                        socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                        socket.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                        let mut request = [0_u8; 2048];
                        let mut used = 0;
                        while !request[..used].ends_with(b"\r\n\r\n") {
                            assert!(Instant::now() < deadline && used < request.len());
                            match socket.read(&mut request[used..]) {
                                Ok(0) => panic!("request ended before its head"),
                                Ok(n) => used += n,
                                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                                Err(e) => panic!("read: {e}"),
                            }
                        }
                        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").unwrap();
                        assert!(request[..used].starts_with(b"GET /live HTTP/1.1\r\n"));
                    });
                    let mut views = [view(url.as_bytes()), view(invalid)];
                    if invalid_first {
                        views.swap(0, 1);
                    }
                    let mut output = ResponseBatch(AlignStr {
                        ptr: core::ptr::null(),
                        len: 0,
                    });
                    let status = unsafe {
                        align_rt_http_get_many(
                            core::ptr::null_mut(),
                            views.as_ptr(),
                            2,
                            workers,
                            &mut output.0,
                        )
                    };
                    assert_eq!(status, AL_INVALID);
                    assert!(output.0.ptr.is_null() && output.0.len == 0);
                    peer.join().unwrap();
                });
            }
        }
    }
}
