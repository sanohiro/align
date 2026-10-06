//! Argument text admission and borrowed-byte ownership, with isolated allocation counters.
use super::*;
use std::ffi::{CString, OsStr};
use std::io::ErrorKind;
use std::time::{Duration, Instant};

struct ArgsChild {
    child: Option<std::process::Child>,
    deadline: Instant,
}
impl ArgsChild {
    fn tick(&self) {
        assert!(
            Instant::now() + Duration::from_secs(5) < self.deadline,
            "argument child exceeded work deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    fn wait(&mut self) {
        loop {
            self.tick();
            match self.child.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    self.child.take();
                    assert!(status.success(), "argument child: {status}");
                    return;
                }
                Ok(None) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("poll argument child: {error}"),
            }
        }
    }
}
impl Drop for ArgsChild {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // The exact-filter child uses native argument buffers only and has no descendants.
        loop {
            match child.kill() {
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => break,
            }
        }
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => {
                    eprintln!("argument child did not reap before deadline");
                    return;
                }
            }
        }
    }
}

struct Owned(AlignStr);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { align_rt_free(self.0.ptr.cast_mut()) };
    }
}
fn vacant() -> AlignStr {
    AlignStr {
        ptr: core::ptr::null(),
        len: 0,
    }
}

#[test]
fn admission_borrows_bytes_and_allocates_only_after_complete_validation() {
    const CHILD: &str = "ALIGN_ARGV_TEXT_ADMISSION_CHILD";
    if std::env::var_os(CHILD).as_deref() != Some(OsStr::new("1")) {
        let mut child = ArgsChild {
            child: Some(std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "args_tests::admission_borrows_bytes_and_allocates_only_after_complete_validation", "--nocapture"])
                .env(CHILD, "1")
                .stdin(std::process::Stdio::null())
                .spawn().expect("spawn argument owner")),
            deadline: Instant::now() + Duration::from_secs(20),
        };
        child.wait();
        return;
    }
    #[cfg(feature = "alloc-count")]
    {
        assert!(!REQUESTED_LIVE_PROBE.lock().unwrap().active);
        let witness = align_rt_alloc(1);
        unsafe { align_rt_free(witness) };
    }
    let mut values = vec![
        Vec::new(),
        b"program".to_vec(),
        "日本語🦀".as_bytes().to_vec(),
        vec![0xff],
        vec![0x80],
        vec![0xc0, 0x80],
        vec![0xed, 0xa0, 0x80],
        vec![0xc3],
        vec![0xf4, 0x90, 0x80, 0x80],
        vec![0xe2, 0x28, 0xa1],
    ];
    let mut late = vec![b'x'; 4096];
    late.push(0xff);
    values.push(late);
    let ordinary = CString::new("ordinary").unwrap();
    for value in values {
        let text = CString::new(value.clone()).unwrap();
        for ordinal in 0..3 {
            let mut pointers = [ordinary.as_ptr().cast::<u8>(); 3];
            pointers[ordinal] = text.as_ptr().cast();
            let valid = std::str::from_utf8(&value).is_ok();
            #[cfg(feature = "alloc-count")]
            let (allocated, freed) = (align_rt_alloc_count(), align_rt_free_count());
            let mut output = Owned(vacant());
            let status = unsafe { align_rt_args_build(3, pointers.as_ptr(), &mut output.0) };
            assert_eq!(
                status,
                if valid { 0 } else { AL_INVALID },
                "ordinal {ordinal}: {value:?}"
            );
            if valid {
                assert_eq!(output.0.len, 3);
                let views =
                    unsafe { core::slice::from_raw_parts(output.0.ptr.cast::<AlignStr>(), 3) };
                for (index, view) in views.iter().enumerate() {
                    assert_eq!(
                        view.ptr, pointers[index],
                        "argument bytes must remain borrowed"
                    );
                    let expected = if index == ordinal {
                        value.as_slice()
                    } else {
                        b"ordinary"
                    };
                    assert_eq!(unsafe { bytes_view(view.ptr, view.len) }, expected);
                }
            } else {
                assert!(output.0.ptr.is_null() && output.0.len == 0);
            }
            #[cfg(feature = "alloc-count")]
            assert_eq!(align_rt_alloc_count() - allocated, i64::from(valid));
            drop(output);
            #[cfg(feature = "alloc-count")]
            assert_eq!(align_rt_free_count() - freed, i64::from(valid));
            assert_eq!(text.to_bytes(), value, "Drop must preserve borrowed text");
        }
    }
    // A repeated native pointer stays a repeated borrowed view, with one outer allocation.
    let aliases = [ordinary.as_ptr().cast::<u8>(); 2];
    let mut output = Owned(vacant());
    assert_eq!(
        unsafe { align_rt_args_build(2, aliases.as_ptr(), &mut output.0) },
        0
    );
    let views = unsafe { core::slice::from_raw_parts(output.0.ptr.cast::<AlignStr>(), 2) };
    assert_eq!(views[0].ptr, views[1].ptr);
    drop(output);

    let null_entry = [core::ptr::null()];
    for (argc, argv, expected) in [
        (0, core::ptr::null(), 0),
        (0, null_entry.as_ptr(), 0),
        (-1, core::ptr::null(), AL_INVALID),
        (i32::MIN, null_entry.as_ptr(), AL_INVALID),
        (1, core::ptr::null(), AL_INVALID),
        (i32::MAX, core::ptr::null(), AL_INVALID),
        (1, null_entry.as_ptr(), AL_INVALID),
    ] {
        // Non-owned poison makes canonicalization observable without fabricating an allocation.
        let mut slot = AlignStr {
            ptr: ordinary.as_ptr().cast(),
            len: 99,
        };
        #[cfg(feature = "alloc-count")]
        let allocated = align_rt_alloc_count();
        assert_eq!(
            unsafe { align_rt_args_build(argc, argv, &mut slot) },
            expected
        );
        assert!(slot.ptr.is_null() && slot.len == 0);
        #[cfg(feature = "alloc-count")]
        assert_eq!(align_rt_alloc_count(), allocated);
    }
    for ordinal in 0..3 {
        let mut pointers = [ordinary.as_ptr().cast::<u8>(); 3];
        pointers[ordinal] = core::ptr::null();
        let mut output = Owned(vacant());
        #[cfg(feature = "alloc-count")]
        let allocated = align_rt_alloc_count();
        assert_eq!(
            unsafe { align_rt_args_build(3, pointers.as_ptr(), &mut output.0) },
            AL_INVALID
        );
        assert!(output.0.ptr.is_null() && output.0.len == 0);
        #[cfg(feature = "alloc-count")]
        assert_eq!(align_rt_alloc_count(), allocated);
    }
    assert_eq!(
        unsafe { align_rt_args_build(0, core::ptr::null(), core::ptr::null_mut()) },
        AL_INVALID
    );
    assert_eq!(
        unsafe { align_rt_args_build(1, core::ptr::null(), core::ptr::null_mut()) },
        AL_INVALID
    );
}
