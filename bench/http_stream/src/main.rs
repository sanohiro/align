//! Production-profile stream timing; allocation measurement is a separate build/process.
use align_runtime::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

#[cfg(feature = "count-alloc")]
mod allocation {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    thread_local! { static COUNT: Cell<Option<u64>> = const { Cell::new(None) }; }
    struct Counter;
    unsafe impl GlobalAlloc for Counter {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let _ = COUNT.try_with(|count| {
                if let Some(n) = count.get() {
                    count.set(Some(n + 1));
                }
            });
            unsafe { System.alloc(layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let _ = COUNT.try_with(|count| {
                if let Some(n) = count.get() {
                    count.set(Some(n + 1));
                }
            });
            unsafe { System.alloc_zeroed(layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
            let _ = COUNT.try_with(|count| {
                if let Some(n) = count.get() {
                    count.set(Some(n + 1));
                }
            });
            unsafe { System.realloc(ptr, layout, new) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    #[global_allocator]
    static ALLOCATOR: Counter = Counter;
    pub fn begin() {
        COUNT.with(|count| count.set(Some(0)));
    }
    pub fn end() -> u64 {
        COUNT.with(|count| count.replace(None).expect("measurement active"))
    }
}

struct Server(*mut HttpServer);
impl Drop for Server {
    fn drop(&mut self) {
        unsafe { align_rt_http_server_free(self.0) }
    }
}
struct Context(*mut HttpRequestCtx);
impl Drop for Context {
    fn drop(&mut self) {
        unsafe { align_rt_http_ctx_free(self.0) }
    }
}
struct Stream(*mut HttpStream);
impl Drop for Stream {
    fn drop(&mut self) {
        unsafe { align_rt_http_stream_free(self.0) }
    }
}

fn server() -> (Server, u16) {
    // Native HTTP deliberately excludes port zero. Retry the release-and-bind race rather than
    // assuming an observed ephemeral port remains ours.
    for _ in 0..32 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral listener");
        let port = listener.local_addr().expect("local address").port();
        drop(listener);
        let mut handle = std::ptr::null_mut();
        let status =
            unsafe { align_rt_http_serve(b"127.0.0.1".as_ptr(), 9, i64::from(port), &mut handle) };
        if status == 0 {
            return (Server(handle), port);
        }
        assert!(handle.is_null(), "failed acquisition publishes no server");
    }
    panic!("could not exclusively bind a benchmark listener");
}

struct Reader {
    thread: Option<std::thread::JoinHandle<(usize, Option<u128>, u128)>>,
    stop: TcpStream,
}
impl Reader {
    fn join(mut self) -> (usize, Option<u128>, u128) {
        self.thread
            .take()
            .expect("reader owner")
            .join()
            .expect("reader success")
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        if let Some(reader) = self.thread.take() {
            let _ = self.stop.shutdown(std::net::Shutdown::Both);
            let _ = reader.join();
        }
    }
}

fn run_case(framed: bool, event: bool, payload: &[u8], iterations: usize) {
    let (server, port) = server();
    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("bounded peer");
    client
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("bounded request");
    client.set_nodelay(true).expect("client nodelay");
    client
        .write_all(if framed {
            b"GET / HTTP/1.1\r\nHost: bench\r\n\r\n"
        } else {
            b"GET / HTTP/1.0\r\nHost: bench\r\n\r\n"
        })
        .expect("request");
    let mut ctx = std::ptr::null_mut();
    assert_eq!(unsafe { align_rt_http_accept(server.0, &mut ctx) }, 0);
    let ctx = Context(ctx);
    let rb = align_rt_http_response_new(200);
    let mut stream = std::ptr::null_mut();
    assert_eq!(
        unsafe { align_rt_http_respond_stream(ctx.0, rb, &mut stream) },
        0
    );
    let mut stream = Stream(stream);
    #[cfg(feature = "write-budget")]
    assert_eq!(
        unsafe { align_rt_http_stream_write_timeout_ns(stream.0, 1_000_000_000) },
        0
    );
    let wire_payload = payload.len() + if event { 8 } else { 0 };
    let framing_len = if framed && wire_payload > 0 {
        format!("{wire_payload:x}").len() + 4
    } else {
        0
    };
    let first_prefix = if framed && wire_payload > 0 {
        framing_len - 2
    } else {
        0
    };
    let (start_tx, start_rx) = std::sync::mpsc::sync_channel::<Instant>(1);
    let stop = client.try_clone().expect("reader cleanup owner");
    let reader = Reader {
        stop,
        thread: Some(std::thread::spawn(move || {
            let start = start_rx.recv().expect("start mark");
            let mut scratch = [0_u8; 32768];
            let mut header = Vec::with_capacity(512);
            let mut head_end = None;
            let mut first_payload = None;
            let mut received = 0_usize;
            loop {
                let n = client.read(&mut scratch).expect("bounded response read");
                if n == 0 {
                    break;
                }
                let arrival = start.elapsed().as_nanos();
                received += n;
                if head_end.is_none() {
                    header.extend_from_slice(&scratch[..n.min(512 - header.len())]);
                    head_end = header
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|end| end + 4);
                    assert!(
                        head_end.is_some() || header.len() < 512,
                        "bounded standard response head"
                    );
                }
                if wire_payload > 0
                    && first_payload.is_none()
                    && head_end.is_some_and(|end| received > end + first_prefix)
                {
                    first_payload = Some(arrival);
                }
            }
            let head_end = head_end.expect("complete head");
            let frames = if wire_payload > 0 {
                (wire_payload + framing_len) * (iterations + 1)
            } else {
                0
            };
            let terminator = if framed { 5 } else { 0 };
            assert_eq!(
                received,
                head_end + frames + terminator,
                "exact received wire count"
            );
            (received, first_payload, start.elapsed().as_nanos())
        })),
    };
    let send = if event {
        align_rt_http_stream_send_event
    } else {
        align_rt_http_stream_send
    };
    let length = i64::try_from(payload.len()).expect("payload width");
    let mut latencies = Vec::with_capacity(iterations);
    start_tx.send(Instant::now()).expect("reader start");
    #[cfg(feature = "count-alloc")]
    allocation::begin();
    let first_start = Instant::now();
    assert_eq!(unsafe { send(stream.0, payload.as_ptr(), length) }, 0);
    let first_ns = first_start.elapsed().as_nanos();
    for _ in 0..iterations {
        let start = Instant::now();
        assert_eq!(unsafe { send(stream.0, payload.as_ptr(), length) }, 0);
        latencies.push(start.elapsed().as_nanos());
    }
    #[cfg(feature = "count-alloc")]
    let allocations = allocation::end();
    #[cfg(not(feature = "count-alloc"))]
    let allocations = 0;
    let owned = std::mem::replace(&mut stream.0, std::ptr::null_mut());
    assert_eq!(unsafe { align_rt_http_stream_finish(owned) }, 0);
    let (bytes, first_payload_ns, elapsed) = reader.join();
    latencies.sort_unstable();
    let p50 = latencies[iterations / 2];
    let p95 = latencies[iterations * 95 / 100];
    let mode = if cfg!(feature = "count-alloc") {
        "allocation"
    } else {
        "timing"
    };
    println!(
        "{mode},{framed},{event},{},{iterations},{first_ns},{p50},{p95},{},{bytes},{elapsed},{allocations}",
        payload.len(),
        first_payload_ns.map_or_else(|| "NA".to_owned(), |ns| ns.to_string())
    );
}

fn main() {
    println!(
        "mode,http11,event,payload_bytes,steady_sends,first_send_ns,p50_ns,p95_ns,first_payload_ns,received_bytes,elapsed_ns,allocation_calls"
    );
    let payloads = [
        Vec::new(),
        b"x".to_vec(),
        "あ".as_bytes().to_vec(),
        vec![42; 4096],
        vec![42; 262144],
    ];
    for framed in [false, true] {
        for event in [false, true] {
            for payload in &payloads {
                let iterations = if payload.len() >= 262144 { 128 } else { 4096 };
                run_case(framed, event, payload, iterations);
            }
        }
    }
}
