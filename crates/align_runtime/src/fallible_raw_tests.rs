use super::*;

struct RefuseAllocation;
impl RefuseAllocation {
    fn enter() -> Self {
        RAW_TRY_ALLOC_FAIL.with(|slot| assert!(!slot.replace(true), "nested allocator refusal"));
        Self
    }
}
impl Drop for RefuseAllocation {
    fn drop(&mut self) {
        RAW_TRY_ALLOC_FAIL.with(|slot| slot.set(false));
    }
}

#[test]
fn fallible_raw_admission_refusal_retry_and_free() {
    let dirty = core::ptr::dangling_mut::<u8>();
    let mut output = dirty;
    unsafe {
        assert_eq!(align_rt_raw_try_alloc(-1, core::ptr::null_mut()), AL_INVALID);
        let refusal = RefuseAllocation::enter();
        #[cfg(feature = "alloc-count")]
        let before = RAW_STORAGE_EVENTS.with(|events| events.get());
        assert_eq!(align_rt_raw_try_alloc(-1, &mut output), AL_INVALID);
        assert!(output.is_null());
        output = dirty;
        assert_eq!(align_rt_raw_try_alloc(0, &mut output), 0);
        assert!(output.is_null());
        #[cfg(feature = "alloc-count")]
        assert_eq!(RAW_STORAGE_EVENTS.with(|events| events.get()), before);
        output = dirty;
        assert_eq!(align_rt_raw_try_alloc(37, &mut output), AL_CODE + 12);
        assert!(output.is_null());
        #[cfg(feature = "alloc-count")]
        assert_eq!(RAW_STORAGE_EVENTS.with(|events| events.get()), before + 1);
        drop(refusal);
        for size in [1, 7, 8, 37, 104, 152, 5120] {
            assert_eq!(align_rt_raw_try_alloc(size, &mut output), 0);
            assert!(!output.is_null());
            assert_eq!(output.addr() % 8, 0);
            output.write_bytes(0xa5, usize::try_from(size).unwrap());
            assert_eq!(output.read(), 0xa5);
            assert_eq!(output.add(usize::try_from(size - 1).unwrap()).read(), 0xa5);
            align_rt_free(output);
        }
        // A representable but enormous input is tested under refusal, never sent to malloc.
        let _refusal = RefuseAllocation::enter();
        assert_eq!(align_rt_raw_try_alloc(i64::MAX, &mut output),
            if usize::BITS < 64 { AL_INVALID } else { AL_CODE + 12 });
        assert!(output.is_null());
    }
}

#[test]
fn fallible_raw_refusal_is_thread_local_and_restored() {
    let refusal = RefuseAllocation::enter();
    std::thread::spawn(|| unsafe {
        let mut output = core::ptr::null_mut();
        assert_eq!(align_rt_raw_try_alloc(16, &mut output), 0);
        assert!(!output.is_null());
        align_rt_free(output);
    }).join().unwrap();
    drop(refusal);
    assert!(!RAW_TRY_ALLOC_FAIL.with(|slot| slot.get()));
}
