//! Native selection reads initialized bytes without changing storage.
use super::*;

#[repr(align(16))]
struct Header([u8; 64]);
struct ArrayOwner(*mut ArrayBuilder, bool);
impl Drop for ArrayOwner {
    fn drop(&mut self) {
        unsafe {
            if self.1 { align_rt_array_builder_free_stack(self.0) } else { align_rt_array_builder_free(self.0) }
        }
    }
}
struct RegionOwner(*mut Arena);
impl Drop for RegionOwner {
    fn drop(&mut self) { if !self.0.is_null() { unsafe { align_rt_arena_end(self.0) } } }
}

fn observe(owner: &ArrayOwner, index: i64, expected: &[u8]) {
    let b = unsafe { &*owner.0 };
    let state = (b.data, b.len, b.cap, b.elem_size, b.arena, b.head, b.tail, b.elem_align);
    let mut chunks = Vec::new();
    let mut chunk = b.head;
    while !chunk.is_null() {
        let c = unsafe { &*chunk };
        chunks.push((chunk, c.next, c.data, c.len, c.cap));
        chunk = c.next;
    }
    #[cfg(feature = "alloc-count")]
    let allocations = global_alloc_count();
    #[cfg(feature = "alloc-count")]
    let native_events = RAW_STORAGE_EVENTS.with(core::cell::Cell::get);
    #[cfg(feature = "alloc-count")]
    let frees = GLOBAL_DEALLOC_CALLS.with(core::cell::Cell::get);
    let pointer = unsafe { align_rt_array_builder_at(std::hint::black_box(owner.0), index) };
    #[cfg(feature = "alloc-count")]
    assert_eq!(global_alloc_count(), allocations, "element selection allocated");
    #[cfg(feature = "alloc-count")]
    assert_eq!(RAW_STORAGE_EVENTS.with(core::cell::Cell::get), native_events,
        "element selection allocated, reallocated or freed native storage");
    #[cfg(feature = "alloc-count")]
    assert_eq!(GLOBAL_DEALLOC_CALLS.with(core::cell::Cell::get), frees, "element selection freed Rust storage");
    if expected.is_empty() { assert!(pointer.is_null()); }
    else { assert_eq!(unsafe { core::slice::from_raw_parts(pointer, expected.len()) }, expected); }
    assert_eq!((b.data, b.len, b.cap, b.elem_size, b.arena, b.head, b.tail, b.elem_align), state);
    for (pointer, next, data, len, cap) in chunks {
        let c = unsafe { &*pointer };
        assert_eq!((c.next, c.data, c.len, c.cap), (next, data, len, cap));
    }
}

#[test]
fn array_builder_reads_initialized_prefix() {
    let _: unsafe extern "C" fn(*const ArrayBuilder, i64) -> *const u8 = align_rt_array_builder_at;
    #[cfg(feature = "alloc-count")]
    {
        let before = global_alloc_count();
        let witness = std::hint::black_box(vec![std::hint::black_box(1u8); 32]);
        assert!(global_alloc_count() > before, "allocation observer must be active");
        let frees = GLOBAL_DEALLOC_CALLS.with(core::cell::Cell::get);
        drop(witness);
        assert!(GLOBAL_DEALLOC_CALLS.with(core::cell::Cell::get) > frees);
        let before = RAW_STORAGE_EVENTS.with(core::cell::Cell::get);
        let witness = align_rt_alloc(8);
        unsafe { align_rt_free(witness) };
        assert_eq!(RAW_STORAGE_EVENTS.with(core::cell::Cell::get), before + 2);
    }
    for region in [false, true] {
        for stack in [false, true] {
            if region && stack { continue; }
            for stride in [0, 1, 2, 4, 8, 16, 32] {
                for capacity in [0, 1, 64] {
                    let arena = RegionOwner(if region { align_rt_arena_begin() } else { core::ptr::null_mut() });
                    let mut header = Header([0; 64]);
                    let owner = ArrayOwner(if region {
                        unsafe { align_rt_array_builder_new_in(arena.0, stride, 8, capacity) }
                    } else if stack {
                        unsafe { align_rt_array_builder_init_stack(header.0.as_mut_ptr(), stride, capacity) }
                    } else { align_rt_array_builder_new(stride, capacity) }, stack);
                    let bytes = usize::try_from(stride).unwrap();
                    assert_eq!(unsafe { (*owner.0).elem_size }, bytes);
                    #[cfg(feature = "alloc-count")]
                    let payload_events = RAW_STORAGE_EVENTS.with(core::cell::Cell::get);
                    let mut expected = Vec::new();
                    for index in 0..35u8 {
                        let row = vec![index; bytes];
                        unsafe { align_rt_array_builder_push_bytes(owner.0, row.as_ptr()) };
                        expected.push(row);
                        for selected in [0, usize::from(index) / 2, usize::from(index)] {
                            observe(&owner, i64::try_from(selected).unwrap(), &expected[selected]);
                        }
                    }
                    if region && stride != 0 && capacity < 35 {
                        assert_ne!(unsafe { (*owner.0).head }, unsafe { (*owner.0).tail });
                    }
                    let suffix = vec![91u8; bytes * 2];
                    if region || stride <= 8 {
                        unsafe { align_rt_array_builder_append(owner.0, suffix.as_ptr(), 2) };
                    } else {
                        for row in suffix.chunks_exact(bytes) {
                            unsafe { align_rt_array_builder_push_bytes(owner.0, row.as_ptr()) };
                        }
                    }
                    observe(&owner, 34, &expected[34]);
                    observe(&owner, 36, &suffix[..bytes]);
                    let builder = owner.0;
                    let frozen = if stack { unsafe { align_rt_array_builder_build_stack(builder) } }
                        else { unsafe { align_rt_array_builder_build(builder) } };
                    std::mem::forget(owner);
                    assert_eq!(frozen.len, 37);
                    if !region && bytes == 0 {
                        assert!(frozen.ptr.is_null());
                        #[cfg(feature = "alloc-count")]
                        assert_eq!(RAW_STORAGE_EVENTS.with(core::cell::Cell::get), payload_events,
                            "zero-stride heap builder allocated payload storage");
                    }
                    if bytes != 0 {
                        assert_eq!(unsafe { core::slice::from_raw_parts(frozen.ptr.add(34 * bytes), bytes) }, expected[34]);
                        assert_eq!(unsafe { core::slice::from_raw_parts(frozen.ptr.add(36 * bytes), bytes) }, &suffix[..bytes]);
                    }
                    if !region { unsafe { align_rt_free(frozen.ptr as *mut u8) }; }
                    if !region && bytes == 0 {
                        let abandoned = ArrayOwner(if stack {
                            unsafe { align_rt_array_builder_init_stack(header.0.as_mut_ptr(), 0, capacity) }
                        } else { align_rt_array_builder_new(0, capacity) }, stack);
                        unsafe { align_rt_array_builder_push_bytes(abandoned.0, core::ptr::NonNull::<u8>::dangling().as_ptr()) };
                        assert!(unsafe { (*abandoned.0).data.is_null() });
                        drop(abandoned);
                        #[cfg(feature = "alloc-count")]
                        assert_eq!(RAW_STORAGE_EVENTS.with(core::cell::Cell::get), payload_events + 2,
                            "only the two null payload frees may reach the native storage seam");
                    }
                }
            }
        }
    }
}

#[cfg(unix)]
#[test]
#[ignore = "isolated builder-read hard-error probe"]
fn array_builder_read_failure_probe() {
    let Some(mode) = std::env::var_os("ALIGN_BUILDER_READ_FAILURE") else { return };
    let mode = mode.to_str().unwrap();
    let (storage, mode) = mode.split_once(':').unwrap_or(("region", mode));
    if mode == "stall" { loop { std::thread::park(); } }
    if mode == "null" { unsafe { align_rt_array_builder_at(core::ptr::null(), 0) }; }
    let arena = RegionOwner(if storage == "region" { align_rt_arena_begin() } else { core::ptr::null_mut() });
    let mut header = Header([0; 64]);
    let stride = if mode == "stride-overflow" { -1 } else { 0 };
    let owner = ArrayOwner(match storage {
        "heap" => align_rt_array_builder_new(stride, 64),
        "stack" => unsafe { align_rt_array_builder_init_stack(header.0.as_mut_ptr(), stride, 64) },
        _ => unsafe { align_rt_array_builder_new_in(arena.0, stride, 8, 64) },
    }, storage == "stack");
    if matches!(mode, "push-overflow" | "append-overflow") {
        unsafe { (*owner.0).len = isize::MAX.unsigned_abs(); (*owner.0).cap = isize::MAX.unsigned_abs(); }
        let source = core::ptr::NonNull::<u8>::dangling().as_ptr();
        if mode == "push-overflow" { unsafe { align_rt_array_builder_push_bytes(owner.0, source) }; }
        else { unsafe { align_rt_array_builder_append(owner.0, source, 1) }; }
        panic!("zero-stride growth overflow returned");
    }
    if mode == "overflow" {
        unsafe { (*owner.0).len = usize::try_from(i64::MAX).unwrap() + 1; (*owner.0).cap = usize::MAX; }
    } else if mode != "empty" {
        unsafe { align_rt_array_builder_push_bytes(owner.0, core::ptr::NonNull::<u8>::dangling().as_ptr()) };
    }
    let index = if matches!(mode, "negative" | "overflow") { -1 } else { 1 };
    unsafe { align_rt_array_builder_at(owner.0, index) };
    panic!("invalid builder read returned");
}

#[cfg(unix)]
#[test]
fn array_builder_read_failures() {
    use std::os::unix::process::ExitStatusExt;
    use std::time::{Duration, Instant};
    struct Child { process: std::process::Child, deadline: Instant }
    impl Child {
        fn poll(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
            loop {
                match self.process.try_wait() {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < self.deadline => {}
                    result => return result,
                }
            }
        }
        fn stop(&mut self) -> bool {
            if matches!(self.poll(), Ok(Some(_))) { return true; }
            loop {
                match self.process.kill() {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < self.deadline => {}
                    _ => break,
                }
            }
            while Instant::now() < self.deadline {
                if matches!(self.poll(), Ok(Some(_))) { return true; }
                std::thread::sleep(Duration::from_millis(2));
            }
            false
        }
    }
    impl Drop for Child {
        fn drop(&mut self) {
            if !self.stop() { eprintln!("builder overflow probe could not be reaped"); }
        }
    }
    for mode in ["null", "empty", "negative", "high", "overflow", "stall",
        "heap:empty", "heap:negative", "heap:high", "stack:empty", "stack:negative", "stack:high",
        "heap:push-overflow", "heap:append-overflow", "stack:push-overflow", "stack:append-overflow",
        "region:push-overflow", "region:append-overflow", "heap:stride-overflow", "stack:stride-overflow"] {
        let process = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "array_builder_read_tests::array_builder_read_failure_probe", "--nocapture"])
            .env("ALIGN_BUILDER_READ_FAILURE", mode)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped())
            .spawn().unwrap();
        let budget = if mode == "stall" { Duration::from_millis(600) } else { Duration::from_secs(10) };
        let mut child = Child { process, deadline: Instant::now() + budget };
        let work_deadline = child.deadline - Duration::from_millis(400);
        let status = loop {
            if let Some(status) = child.poll().unwrap() { break Some(status); }
            if Instant::now() >= work_deadline { break None; }
            std::thread::sleep(Duration::from_millis(2));
        };
        if mode == "stall" {
            assert!(status.is_none(), "stalled probe unexpectedly terminated");
            assert!(child.stop(), "stalled probe was not killed and reaped before deadline");
            continue;
        }
        let status = status.expect("overflow probe exceeded its work deadline");
        let mut error = String::new();
        std::io::Read::read_to_string(&mut child.process.stderr.take().unwrap(), &mut error).unwrap();
        assert_eq!(status.signal(), Some(libc::SIGABRT), "{mode}: {error}");
        assert!(error.contains(if mode.ends_with("overflow") { "align: panic: allocation size overflow" } else { "index out of bounds" }), "{mode}: {error}");
    }
}
