use super::*;

struct ScriptedWrite {
    output: Vec<u8>,
    payload: *const u8,
    payload_len: usize,
    saw_payload: bool,
    first_limit: usize,
    interrupt: bool,
    failure: Option<SocketCallOutcome>,
    calls: usize,
}

impl HttpStreamWriteOps for ScriptedWrite {
    fn send_parts(&mut self, _fd: i32, parts: &[libc::iovec], flags: i32) -> SocketCallOutcome {
        assert_eq!(
            flags,
            if cfg!(target_os = "linux") {
                libc::MSG_NOSIGNAL
            } else {
                0
            }
        );
        self.calls += 1;
        for part in parts {
            let base = part.iov_base as usize;
            let payload = self.payload as usize;
            if self.payload_len > 0 && base >= payload && base < payload + self.payload_len {
                assert!(part.iov_len <= payload + self.payload_len - base);
                self.saw_payload = true;
            }
        }
        if self.interrupt {
            self.interrupt = false;
            return SocketCallOutcome {
                result: -1,
                errno: libc::EINTR,
            };
        }
        if let Some(failure) = self.failure.take() {
            return failure;
        }
        let total: usize = parts.iter().map(|part| part.iov_len).sum();
        let count = total.min(self.first_limit);
        self.first_limit = usize::MAX;
        let mut left = count;
        for part in parts {
            let take = left.min(part.iov_len);
            if take > 0 {
                let bytes =
                    unsafe { core::slice::from_raw_parts(part.iov_base.cast::<u8>(), take) };
                self.output.extend_from_slice(bytes);
            }
            left -= take;
        }
        SocketCallOutcome {
            result: isize::try_from(count).expect("small fixture"),
            errno: 0,
        }
    }
}

fn script(payload: &[u8], capacity: usize, first_limit: usize) -> ScriptedWrite {
    ScriptedWrite {
        output: Vec::with_capacity(capacity),
        payload: payload.as_ptr(),
        payload_len: payload.len(),
        saw_payload: false,
        first_limit,
        interrupt: true,
        failure: None,
        calls: 0,
    }
}

#[test]
fn http_stream_vectored_every_partial_prefix_preserves_wire_and_source_pointer() {
    for framed in [false, true] {
        for pending in [false, true] {
            for event in [false, true] {
                for data in [b"".as_slice(), b"a\0\xff\xe3\x81\x82z"] {
                    if !event && data.is_empty() {
                        continue;
                    }
                    let prefix = if event { b"data: ".as_slice() } else { &[] };
                    let suffix = if event { b"\n\n".as_slice() } else { &[] };
                    let mut expected = if pending {
                        b"HEAD\r\n\r\n".to_vec()
                    } else {
                        Vec::new()
                    };
                    let payload_len = prefix.len() + data.len() + suffix.len();
                    if framed {
                        expected.extend_from_slice(format!("{payload_len:x}\r\n").as_bytes());
                    }
                    expected.extend_from_slice(prefix);
                    expected.extend_from_slice(data);
                    expected.extend_from_slice(suffix);
                    if framed {
                        expected.extend_from_slice(b"\r\n");
                    }
                    for first_limit in 1..=expected.len() {
                        let mut stream = HttpStream {
                            fd: -1,
                            framed,
                            poisoned: false,
                            pending_head: pending.then(|| b"HEAD\r\n\r\n".to_vec()),
                        };
                        let mut ops = script(data, expected.len(), first_limit);
                        #[cfg(feature = "alloc-count")]
                        let allocations = global_alloc_count();
                        let status = http_stream_send_parts_with(
                            &mut stream,
                            data,
                            event,
                            &mut ops,
                        );
                        #[cfg(feature = "alloc-count")]
                        assert_eq!(
                            global_alloc_count(),
                            allocations,
                            "no writer or framing allocation"
                        );
                        assert_eq!(status, 0);
                        assert_eq!(
                            ops.output, expected,
                            "{framed}/{pending}/{event}/{first_limit}"
                        );
                        assert_eq!(
                            ops.saw_payload,
                            !data.is_empty(),
                            "native vectors borrow the actual caller allocation"
                        );
                        assert!(stream.pending_head.is_none());
                        assert!(!stream.poisoned);
                        assert_eq!(ops.calls, if first_limit == expected.len() { 2 } else { 3 });
                    }
                }
            }
        }
    }
}

#[test]
fn http_stream_vectored_zero_error_and_impossible_count_poison_without_retry() {
    for outcome in [
        SocketCallOutcome {
            result: 0,
            errno: libc::EINTR,
        },
        SocketCallOutcome {
            result: -1,
            errno: libc::EPIPE,
        },
        SocketCallOutcome {
            result: isize::MAX,
            errno: 0,
        },
    ] {
        let mut stream = HttpStream {
            fd: -1,
            framed: true,
            poisoned: false,
            pending_head: Some(b"HEAD".to_vec()),
        };
        let mut ops = script(b"x", 32, usize::MAX);
        ops.interrupt = false;
        ops.failure = Some(outcome);
        let status = http_stream_send_parts_with(&mut stream, b"x", false, &mut ops);
        assert_eq!(
            status,
            if outcome.result == 0 {
                AL_CODE
            } else if outcome.result > 0 {
                AL_INVALID
            } else {
                io_read_write_status(&std::io::Error::from_raw_os_error(libc::EPIPE))
            }
        );
        assert!(stream.poisoned);
        assert!(stream.pending_head.is_none());
        assert_eq!(ops.calls, 1);
        assert_eq!(
            http_stream_send_parts_with(&mut stream, b"x", false, &mut ops),
            AL_INVALID
        );
        assert_eq!(ops.calls, 1);
        assert_eq!(
            unsafe { align_rt_http_stream_send(&mut stream, core::ptr::null(), 0) },
            0
        );
        assert_eq!(
            unsafe { align_rt_http_stream_send_event(&mut stream, core::ptr::null(), 0) },
            AL_INVALID
        );
    }
}

#[test]
fn http_stream_vectored_raw_extent_validation_precedes_head_commit() {
    let mut stream = HttpStream {
        fd: -1,
        framed: true,
        poisoned: false,
        pending_head: Some(b"HEAD".to_vec()),
    };
    let invalid = core::ptr::without_provenance::<u8>(usize::MAX - 1);
    for event in [false, true] {
        let send = if event {
            align_rt_http_stream_send_event
        } else {
            align_rt_http_stream_send
        };
        assert_eq!(unsafe { send(&mut stream, invalid, 3) }, AL_INVALID);
        assert!(stream.pending_head.is_some());
        assert!(!stream.poisoned);
    }
    for (ptr, len) in [(core::ptr::null(), 123), (invalid, -1), (invalid, 0)] {
        assert_eq!(
            unsafe { align_rt_http_stream_send(&mut stream, ptr, len) },
            0
        );
        assert!(stream.pending_head.is_some());
    }
}

#[test]
fn http_stream_vectored_native_binary_octets_and_buffer_reuse() {
    use std::io::Read;
    use std::os::fd::IntoRawFd;
    for framed in [false, true] {
        let (writer, mut reader) = std::os::unix::net::UnixStream::pair().expect("socket pair");
        reader
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("bounded reader");
        let fd = http_prepare_accepted_fd(
            writer.into_raw_fd(),
            NATIVE_SOCKET_SIGPIPE_POLICY,
            &mut NativeSocketWriteOps,
        )
        .unwrap_or_else(|_| panic!("SIGPIPE setup failed"));
        let mut stream = HttpStream {
            fd,
            framed,
            poisoned: false,
            pending_head: Some(b"HEAD\r\n\r\n".to_vec()),
        };
        let mut data: Vec<u8> = (u8::MIN..=u8::MAX).collect();
        assert_eq!(
            unsafe { align_rt_http_stream_send(&mut stream, data.as_ptr(), 256) },
            0
        );
        data.fill(42);
        assert_eq!(
            unsafe { align_rt_http_stream_send(&mut stream, data.as_ptr(), 256) },
            0
        );
        drop(stream);
        let mut actual = Vec::new();
        reader.read_to_end(&mut actual).expect("complete bytes");
        let mut expected = b"HEAD\r\n\r\n".to_vec();
        for body in [(u8::MIN..=u8::MAX).collect::<Vec<_>>(), data] {
            if framed {
                expected.extend_from_slice(b"100\r\n");
            }
            expected.extend_from_slice(&body);
            if framed {
                expected.extend_from_slice(b"\r\n");
            }
        }
        assert_eq!(actual, expected);
    }
}
