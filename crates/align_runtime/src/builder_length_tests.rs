//! Initialized-count observations across native header representations.
use super::*;

#[repr(align(16))]
struct Header([u8; 64]);
struct TextOwner(*mut Builder, bool);
impl Drop for TextOwner {
    fn drop(&mut self) {
        unsafe {
            if self.1 { align_rt_builder_free_stack(self.0) } else { align_rt_builder_free(self.0) }
        }
    }
}
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
    fn drop(&mut self) { unsafe { align_rt_arena_end(self.0) } }
}

fn observe_text(owner: &TextOwner, expected: i64) {
    let b = unsafe { &*owner.0 };
    let before = (b.buf.ptr, b.buf.len, b.buf.cap, b.buf.limit, b.buf.encode_failed, b.arena);
    #[cfg(feature = "alloc-count")]
    let allocations = global_alloc_count();
    let actual = unsafe { align_rt_builder_len(std::hint::black_box(owner.0)) };
    #[cfg(feature = "alloc-count")]
    let after_allocations = global_alloc_count();
    assert_eq!(actual, expected);
    assert_eq!((b.buf.ptr, b.buf.len, b.buf.cap, b.buf.limit, b.buf.encode_failed, b.arena), before);
    #[cfg(feature = "alloc-count")]
    assert_eq!(after_allocations, allocations, "text observation allocated");
}

fn observe_array(owner: &ArrayOwner, expected: i64) {
    let b = unsafe { &*owner.0 };
    let before = (b.data, b.len, b.cap, b.elem_size, b.arena, b.head, b.tail, b.elem_align);
    #[cfg(feature = "alloc-count")]
    let allocations = global_alloc_count();
    let actual = unsafe { align_rt_array_builder_len(std::hint::black_box(owner.0)) };
    #[cfg(feature = "alloc-count")]
    let after_allocations = global_alloc_count();
    assert_eq!(actual, expected);
    assert_eq!((b.data, b.len, b.cap, b.elem_size, b.arena, b.head, b.tail, b.elem_align), before);
    #[cfg(feature = "alloc-count")]
    assert_eq!(after_allocations, allocations, "array observation allocated");
}

#[test]
fn builder_lengths_observe_initialized_prefix() {
    // Pin the exported native signatures independently of LLVM's declarations.
    let _: unsafe extern "C" fn(*mut Builder) -> i64 = align_rt_builder_len;
    let _: unsafe extern "C" fn(*mut ArrayBuilder) -> i64 = align_rt_array_builder_len;
    assert_eq!(unsafe { align_rt_builder_len(core::ptr::null_mut()) }, 0);
    assert_eq!(unsafe { align_rt_array_builder_len(core::ptr::null_mut()) }, 0);
    #[cfg(feature = "alloc-count")]
    {
        let before = global_alloc_count();
        let witness = std::hint::black_box(vec![std::hint::black_box(1u8); 32]);
        assert!(global_alloc_count() > before, "allocation observer must be active");
        drop(witness);
    }
    for stack in [false, true] {
        for capacity in [0, 1, 64] {
            let mut storage = Header([0; 64]);
            let owner = TextOwner(if stack {
                unsafe { align_rt_builder_init_stack(storage.0.as_mut_ptr(), core::ptr::null_mut(), capacity) }
            } else { align_rt_builder_new(core::ptr::null_mut(), capacity) }, stack);
            observe_text(&owner, 0);
            unsafe { align_rt_builder_write(owner.0, "あ\0".as_ptr(), 4) };
            observe_text(&owner, 4);
            unsafe { align_rt_builder_write_int(owner.0, -42) };
            observe_text(&owner, 7);
            unsafe { align_rt_builder_write_bool(owner.0, 1) };
            observe_text(&owner, 11);
            unsafe { align_rt_builder_write_char(owner.0, u32::from('界')) };
            observe_text(&owner, 14);
            unsafe { align_rt_builder_write_f64(owner.0, 1.5) };
            observe_text(&owner, 17);
            unsafe { align_rt_builder_write_f32(owner.0, 2.0) };
            observe_text(&owner, 20);
            let expected = "あ\0-42true界1.52.0".as_bytes();
            let actual = unsafe { core::slice::from_raw_parts((*owner.0).buf.ptr, (*owner.0).buf.len) };
            assert_eq!(actual, expected);
        }
        let mut storage = Header([0; 64]);
        let owner = ArrayOwner(if stack {
            unsafe { align_rt_array_builder_init_stack(storage.0.as_mut_ptr(), 8, 1) }
        } else { align_rt_array_builder_new(8, 1) }, stack);
        observe_array(&owner, 0);
        for value in 0..19u64 {
            unsafe { align_rt_array_builder_push(owner.0, value) };
            observe_array(&owner, i64::try_from(value + 1).unwrap());
        }
        let suffix = [20u64, 21];
        unsafe { align_rt_array_builder_append(owner.0, suffix.as_ptr().cast(), 2) };
        observe_array(&owner, 21);
        assert_eq!(unsafe { *(*owner.0).data.cast::<u64>().add(20) }, 21);
    }
    let arena = RegionOwner(align_rt_arena_begin());
    for stride in [0, 8] {
        let owner = ArrayOwner(unsafe { align_rt_array_builder_new_in(arena.0, stride, 8, 1) }, false);
        observe_array(&owner, 0);
        for value in 0..19u64 {
            unsafe { align_rt_array_builder_push_bytes(owner.0, (&value as *const u64).cast()) };
            observe_array(&owner, i64::try_from(value + 1).unwrap());
        }
        if stride != 0 {
            assert_ne!(unsafe { (*owner.0).head }, unsafe { (*owner.0).tail }, "must observe multiple chunks");
        } else {
            let maximum = usize::try_from(i64::MAX).unwrap();
            // Zero-width elements require no initialized backing or chunks. Seed the
            // producer's total count rather than performing an unbounded push loop.
            unsafe { (*owner.0).len = maximum };
            observe_array(&owner, i64::MAX);
        }
    }
}

#[cfg(unix)]
#[test]
#[ignore = "isolated allocation-size hard-error probe"]
fn builder_length_overflow_probe() {
    let Some(mode) = std::env::var_os("ALIGN_BUILDER_LENGTH_OVERFLOW") else { return };
    let too_large = usize::try_from(i64::MAX).unwrap().checked_add(1).unwrap();
    match mode.to_str().unwrap() {
        "stall" => loop { std::thread::park(); },
        "conversion" => { initialized_builder_len(too_large); }
        "array" => {
            let arena = RegionOwner(align_rt_arena_begin());
            let owner = ArrayOwner(unsafe { align_rt_array_builder_new_in(arena.0, 0, 8, 0) }, false);
            unsafe { (*owner.0).len = too_large; (*owner.0).cap = usize::MAX; }
            unsafe { align_rt_array_builder_len(owner.0) };
        }
        _ => panic!("unknown overflow probe"),
    }
    panic!("unrepresentable initialized count returned");
}

#[cfg(unix)]
#[test]
fn builder_length_overflow_is_allocation_size_failure() {
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
    for mode in ["conversion", "array", "stall"] {
        let process = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "builder_length_tests::builder_length_overflow_probe", "--nocapture"])
            .env("ALIGN_BUILDER_LENGTH_OVERFLOW", mode)
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
        assert!(error.contains("align: panic: allocation size overflow"), "{mode}: {error}");
    }
}
