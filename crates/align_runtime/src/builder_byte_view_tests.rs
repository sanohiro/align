//! The getter observes the initialized payload across all native header forms.
use super::*;

#[repr(align(16))]
struct Header([u8; 64]);
struct Owner(*mut Builder, bool);
impl Drop for Owner {
    fn drop(&mut self) {
        unsafe {
            if self.1 { align_rt_builder_free_stack(self.0) } else { align_rt_builder_free(self.0) }
        }
    }
}
struct Region(*mut Arena);
impl Drop for Region {
    fn drop(&mut self) { if !self.0.is_null() { unsafe { align_rt_arena_end(self.0) } } }
}

fn observe(owner: &Owner, expected: &[u8]) {
    let mut output = AlignStr { ptr: core::ptr::null(), len: -1 };
    #[cfg(feature = "alloc-count")]
    let before = (global_alloc_count(), GLOBAL_DEALLOC_CALLS.with(core::cell::Cell::get));
    unsafe { align_rt_builder_bytes(std::hint::black_box(owner.0), &mut output) };
    #[cfg(feature = "alloc-count")]
    let after = (global_alloc_count(), GLOBAL_DEALLOC_CALLS.with(core::cell::Cell::get));
    assert!(!output.ptr.is_null());
    assert_eq!(output.len, i64::try_from(expected.len()).unwrap());
    assert_eq!(unsafe { core::slice::from_raw_parts(output.ptr, expected.len()) }, expected);
    if !expected.is_empty() { assert_eq!(output.ptr, unsafe { (*owner.0).buf.as_ptr() }); }
    #[cfg(feature = "alloc-count")]
    assert_eq!(after, before, "the getter must not allocate, reallocate or free");
}

#[test]
fn builder_bytes_initialized_prefix_without_allocation() {
    let _: unsafe extern "C" fn(*const Builder, *mut AlignStr) = align_rt_builder_bytes;
    for stack in [false, true] {
        for arena in [false, true] {
            let region = Region(if arena { align_rt_arena_begin() } else { core::ptr::null_mut() });
            let mut header = Header([0; 64]);
            let owner = Owner(if stack {
                unsafe { align_rt_builder_init_stack(header.0.as_mut_ptr(), region.0, 1) }
            } else { align_rt_builder_new(region.0, 1) }, stack);
            observe(&owner, b"");
            unsafe {
                align_rt_builder_write(owner.0, "あ\0".as_ptr(), 4);
                align_rt_builder_write_int(owner.0, -42);
                align_rt_builder_write_bool(owner.0, 1);
                align_rt_builder_write_char(owner.0, u32::from('界'));
                align_rt_builder_write_f64(owner.0, 1.5);
            }
            observe(&owner, "あ\0-42true界1.5".as_bytes());
            let growth = vec![b'x'; 2048];
            unsafe { align_rt_builder_write(owner.0, growth.as_ptr(), i64::try_from(growth.len()).unwrap()) };
            let mut expected = "あ\0-42true界1.5".as_bytes().to_vec();
            expected.extend_from_slice(&growth);
            observe(&owner, &expected);
        }
    }
}
