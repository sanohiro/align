use super::*;
use core::mem::MaybeUninit;

fn dispatch(
    state: &mut HttpSseState,
    source: &[u8],
    output: &mut [MaybeUninit<u8>],
) -> HttpSsePublished {
    let capacity = output.len();
    state.prepare_call(capacity).unwrap();
    let mut event = None;
    for &byte in source {
        assert!(event.is_none(), "fixture ends at the first event boundary");
        if let HttpSseFeed::Event(value) = state.feed_source_byte(byte, output, capacity).unwrap() {
            event = Some(value);
        }
    }
    event.expect("one fixture event")
}

fn published<'a>(output: &'a [MaybeUninit<u8>], event: &HttpSsePublished) -> &'a [u8] {
    let prefix = output.get(..event.total).unwrap();
    // SAFETY: successful publication initializes exactly this prefix.
    unsafe { core::slice::from_raw_parts(prefix.as_ptr().cast(), prefix.len()) }
}

/// Manual local parser-storage measurement, including native work acquisition and Drop.
/// Select one of 12 cases with ALIGN_SSE_PREFIX_CASE; use the release test binary.
/// This excludes network/transport and makes no whole-client throughput claim.
#[test]
#[ignore]
fn preparation_and_dispatch_probe() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    let selected: usize = std::env::var("ALIGN_SSE_PREFIX_CASE")
        .unwrap()
        .parse()
        .unwrap();
    assert!(selected < 12);
    let capacity = [16, 1024, 65536][selected / 4];
    let reuse = selected % 4 >= 2;
    let lossy = selected % 2 == 1;
    let source: &[u8] = if lossy {
        b"id: \xff\n\ndata: x\n\n"
    } else {
        b"id: first\n\ndata: x\n\n"
    };
    let expected: &[u8] = if lossy {
        b"messagex\xef\xbf\xbd"
    } else {
        b"messagexfirst"
    };
    let mut output = vec![MaybeUninit::uninit(); capacity];
    let mut retained = HttpSseState::new(true);
    let mut one = || {
        let mut fresh = HttpSseState::new(true);
        let state = if reuse { &mut retained } else { &mut fresh };
        let event = dispatch(black_box(state), black_box(source), black_box(&mut output));
        assert_eq!(published(&output, &event), expected);
        assert_eq!(event.event_len, 7);
        assert_eq!(event.data_len, 1);
        assert_eq!(event.id_len, expected.len() - 8);
        black_box(event.total)
    };
    let warm = Instant::now();
    while warm.elapsed() < Duration::from_millis(100) {
        black_box(one());
    }
    let calls: u64 = if reuse { 50_000 } else { 10_000 };
    for trial in 0..7 {
        let mut bytes = 0usize;
        let start = Instant::now();
        for _ in 0..calls {
            bytes += one();
        }
        let elapsed = start.elapsed();
        assert_eq!(bytes, usize::try_from(calls).unwrap() * expected.len());
        println!(
            "sse-prefix,{selected},{capacity},{reuse},{lossy},{trial},{calls},{bytes},{:.3}",
            elapsed.as_nanos() as f64 / calls as f64
        );
    }
}

#[test]
fn prefix_growth_clear_and_reuse() {
    for (length, capacity) in [(0, 0), (0, 8), (1, 1), (3, 8), (17, 64), (257, 1024)] {
        let mut bytes = HttpSseBytes::new();
        assert!(bytes.as_slice().is_empty());
        bytes.grow_exact(capacity);
        assert_eq!(bytes.capacity(), capacity);
        let expected: Vec<u8> = (0..length)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        for &byte in &expected {
            bytes.push(byte).unwrap();
        }
        // A recognizable spare tail discriminates prefix publication from capacity exposure.
        for slot in &mut bytes.storage[length..] {
            slot.write(0xa5);
        }
        assert_eq!(bytes.as_slice(), expected);
        let old = bytes.storage.as_ptr();
        for request in [0, length, capacity] {
            bytes.grow_exact(request);
            assert_eq!(bytes.storage.as_ptr(), old);
            assert_eq!(bytes.as_slice(), expected);
        }
        let next_capacity = capacity * 2 + 1;
        bytes.grow_exact(next_capacity);
        assert_eq!(bytes.capacity(), next_capacity);
        if capacity != 0 {
            assert_ne!(
                bytes.storage.as_ptr(),
                old,
                "old owner lives through acquisition/copy"
            );
        }
        for slot in &mut bytes.storage[length..] {
            slot.write(0xcc);
        }
        assert_eq!(bytes.as_slice(), expected);
        bytes.clear();
        assert!(bytes.as_slice().is_empty());
        for _ in 0..next_capacity {
            bytes.push(0x34).unwrap();
        }
        assert_eq!(bytes.push(0xff), Err(AL_INVALID));
        assert_eq!(bytes.as_slice(), vec![0x34; next_capacity]);
    }
}

#[test]
fn committed_id_prefix_and_event_publication() {
    let mut state = HttpSseState::new(true);
    for (source, capacity, expected, id) in [
        (
            b"id:abcdefgh\n\ndata:1\n\n".as_slice(),
            64,
            b"message1abcdefgh".as_slice(),
            b"abcdefgh".as_slice(),
        ),
        (b"id:x\n\ndata:2\n\n", 16, b"message2x", b"x"),
        (b"data:3\n\n", 16, b"message3x", b"x"),
        (b"id:\n\ndata:4\n\n", 16, b"message4", b""),
        (
            b"id:\xff\n\ndata:5\n\n",
            16,
            b"message5\xef\xbf\xbd",
            b"\xef\xbf\xbd",
        ),
        (
            b"id:ignored\0\n\ndata:6\n\n",
            16,
            b"message6\xef\xbf\xbd",
            b"\xef\xbf\xbd",
        ),
        (
            b"id:published\ndata:7\n\n",
            32,
            b"message7published",
            b"published",
        ),
        (b"id:z\ndata:8\n\n", 16, b"message8z", b"z"),
    ] {
        for slot in &mut state.block.storage[state.block.len..] {
            slot.write(0xa5);
        }
        for slot in &mut state.committed_id.storage[state.committed_id.len..] {
            slot.write(0xcc);
        }
        let mut output = vec![MaybeUninit::new(0xdd); capacity];
        let event = dispatch(&mut state, source, &mut output);
        assert_eq!(published(&output, &event), expected);
        assert_eq!(state.committed_id.as_slice(), id);
        assert_eq!(event.event_len, 7);
        assert_eq!(event.data_len, 1);
        assert_eq!(event.id_len, id.len());
        assert!(state.block.as_slice().is_empty());
        // These slots were initialized by this fixture, so inspect only to prove nonpublication.
        for slot in &output[event.total..] {
            assert_eq!(unsafe { slot.assume_init() }, 0xdd);
        }
    }
    let old_id = state.committed_id.as_slice().to_vec();
    let mut output = [MaybeUninit::new(0xdd); 8];
    state.prepare_call(output.len()).unwrap();
    let mut failure = None;
    for &byte in b"id:pending\ndata:x\n\n" {
        match state.feed_source_byte(byte, &mut output, 8) {
            Ok(HttpSseFeed::Continue) => {}
            Ok(HttpSseFeed::Event(_)) => panic!("oversized event must not publish"),
            Err(status) => {
                failure = Some(status);
                break;
            }
        }
    }
    assert_eq!(failure, Some(AL_HTTP_BODY_LIMIT));
    assert_eq!(state.committed_id.as_slice(), old_id);
    state.discard_pending();
    assert!(state.block.as_slice().is_empty());
    assert_eq!(state.committed_id.as_slice(), old_id);
}

#[cfg(feature = "alloc-count")]
#[test]
fn growth_allocates_without_zeroing_spare_capacity() {
    let Some(_isolated) = crate::allocation_test::enter() else {
        return;
    };
    let zeroed_before = global_zeroed_alloc_bytes();
    let layout = std::alloc::Layout::from_size_align(37, 1).unwrap();
    // Keep the actual allocator call opaque: a constant zero-filled fixture can otherwise be
    // optimized away, including the allocation whose counter is the positive witness.
    let allocate =
        std::hint::black_box(std::alloc::alloc_zeroed as unsafe fn(std::alloc::Layout) -> *mut u8);
    let ptr = unsafe { allocate(std::hint::black_box(layout)) };
    assert!(!ptr.is_null());
    assert_eq!(
        global_zeroed_alloc_bytes() - zeroed_before,
        layout.size(),
        "real zeroed-allocation witness"
    );
    assert!(
        unsafe { core::slice::from_raw_parts(ptr, layout.size()) }
            .iter()
            .all(|&byte| byte == 0)
    );
    unsafe { std::alloc::dealloc(ptr, layout) };

    let mut bytes = HttpSseBytes::new();
    for capacity in [
        1,
        8,
        17,
        HTTP_MAX_SSE_METADATA + 16,
        HTTP_MAX_SSE_METADATA + 65536,
    ] {
        let before = (
            global_alloc_count(),
            global_alloc_bytes(),
            global_zeroed_alloc_bytes(),
        );
        bytes.grow_exact(capacity);
        let after = (
            global_alloc_count(),
            global_alloc_bytes(),
            global_zeroed_alloc_bytes(),
        );
        assert_eq!(after.0 - before.0, 1, "one exact allocation");
        assert_eq!(
            after.1 - before.1,
            capacity,
            "requested retained bytes unchanged"
        );
        assert_eq!(
            after.2 - before.2,
            0,
            "spare capacity must not request zero initialization"
        );
        assert_eq!(bytes.capacity(), capacity);
        bytes.clear();
        bytes.push(42).unwrap();
        let before = (
            global_alloc_count(),
            global_alloc_bytes(),
            global_zeroed_alloc_bytes(),
        );
        bytes.grow_exact(capacity);
        bytes.grow_exact(0);
        assert_eq!(
            (
                global_alloc_count(),
                global_alloc_bytes(),
                global_zeroed_alloc_bytes()
            ),
            before
        );
        assert_eq!(bytes.as_slice(), &[42]);
    }
}
