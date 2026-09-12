//! Owned byte storage whose shared publications retain writable pointer provenance.

use core::ops::Deref;

pub(super) struct BufferStorage {
    bytes: Vec<u8>,
    writable: *mut u8,
}

// SAFETY: moving the owner transfers its Vec and the pointer into that allocation together.
unsafe impl Send for BufferStorage {}
// SAFETY: safe shared access only reads the Vec or copies a raw pointer. Dereferencing that
// pointer for writing requires the native caller's exclusive byte access and live-storage proof.
unsafe impl Sync for BufferStorage {}

impl From<Vec<u8>> for BufferStorage {
    fn from(mut bytes: Vec<u8>) -> Self {
        let writable = bytes.as_mut_ptr();
        Self { bytes, writable }
    }
}

impl Deref for BufferStorage {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.bytes
    }
}

impl BufferStorage {
    pub(super) fn writable_ptr(&self) -> *mut u8 {
        self.writable
    }

    // No DerefMut: all reallocations and mutable byte references must finish before refresh.
    // The private guard cannot be forgotten by a caller; R cannot borrow from the closure input.
    pub(super) fn with_mut<R>(&mut self, operation: impl FnOnce(&mut Vec<u8>) -> R) -> R {
        struct Refresh<'a>(&'a mut BufferStorage);
        impl Drop for Refresh<'_> {
            fn drop(&mut self) {
                self.0.writable = self.0.bytes.as_mut_ptr();
            }
        }
        let guard = Refresh(self);
        operation(&mut guard.0.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::BufferStorage;
    use crate::{
        AlignStr, Buffer, align_rt_buffer_append, align_rt_buffer_bytes, align_rt_buffer_free,
        align_rt_buffer_new,
    };

    struct Owner(*mut Buffer);

    impl Drop for Owner {
        fn drop(&mut self) {
            unsafe { align_rt_buffer_free(self.0) };
        }
    }

    fn view(buffer: *mut Buffer) -> AlignStr {
        let mut result = AlignStr {
            ptr: core::ptr::null(),
            len: -1,
        };
        unsafe { align_rt_buffer_bytes(buffer, &mut result) };
        result
    }

    fn assert_current(storage: &BufferStorage) {
        assert_eq!(storage.writable_ptr().cast_const(), storage.as_ptr());
    }

    #[test]
    fn buffer_storage_refreshes_every_exclusive_transition() {
        let mut storage = BufferStorage::from(vec![1, 2, 3]);
        assert_current(&storage);
        let previous = storage.writable_ptr();
        storage.with_mut(|bytes| {
            // Both allocations are live while constructing the replacement, so their addresses
            // differ even if a growth allocator could otherwise extend a buffer in place.
            *bytes = vec![4; 1024];
        });
        assert_current(&storage); // Check before dereferencing: omitted refresh fails safely.
        assert_ne!(storage.writable_ptr(), previous);
        unsafe { storage.writable_ptr().write(9) };
        assert_eq!(storage[0], 9);
        storage.with_mut(|bytes| bytes.reserve(4096));
        assert_current(&storage);
        assert!(
            storage
                .with_mut(|bytes| bytes.try_reserve(usize::MAX))
                .is_err()
        );
        assert_current(&storage);
        storage.with_mut(|bytes| bytes.truncate(1));
        assert_current(&storage);
        storage.with_mut(|bytes| bytes.clear());
        assert_current(&storage);
        let raw = storage.with_mut(|bytes| bytes.as_mut_ptr());
        unsafe { raw.write(7) };
        storage.with_mut(|bytes| unsafe { bytes.set_len(1) });
        assert_current(&storage);
        assert_eq!(storage.as_slice(), &[7]);
    }

    #[test]
    fn buffer_storage_refreshes_during_unwind() {
        let mut storage = BufferStorage::from(vec![1]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            storage.with_mut(|bytes| {
                *bytes = vec![2; 128];
                panic!("exercise the refresh guard");
            });
        }));
        assert!(result.is_err());
        assert_current(&storage);
        unsafe { storage.writable_ptr().write(3) };
        assert_eq!(storage[0], 3);
    }

    #[test]
    fn buffer_byte_views_preserve_writable_aliases() {
        for capacity in [0, 8] {
            let owner = Owner(align_rt_buffer_new(capacity));
            assert_eq!(view(owner.0).len, 0);
            unsafe { align_rt_buffer_append(owner.0, b"abcd".as_ptr(), 4) };
            let first = view(owner.0);
            let second = view(owner.0);
            assert_eq!(first.ptr, second.ptr);
            assert_eq!(first.len, 4);
            unsafe {
                first.ptr.cast_mut().write(b'A');
                second.ptr.add(1).cast_mut().write(b'B');
                first.ptr.add(2).cast_mut().write(b'C');
            }
            assert_eq!(
                unsafe { core::slice::from_raw_parts(second.ptr, 4) },
                b"ABCd"
            );
            // A self-append must snapshot before the mutation guard can grow storage.
            unsafe { align_rt_buffer_append(owner.0, first.ptr, first.len) };
            let appended = view(owner.0);
            assert_eq!(appended.len, 8);
            assert_eq!(
                unsafe { core::slice::from_raw_parts(appended.ptr, 8) },
                b"ABCdABCd"
            );
            unsafe { align_rt_buffer_bytes(owner.0, core::ptr::null_mut()) };
            assert_eq!(view(owner.0).ptr, appended.ptr);
        }
        let empty = view(core::ptr::null_mut());
        assert!(empty.ptr.is_null());
        assert_eq!(empty.len, 0);
        unsafe { align_rt_buffer_bytes(core::ptr::null_mut(), core::ptr::null_mut()) };
    }

    #[test]
    fn buffer_byte_views_allow_shared_publication() {
        let owner = Owner(align_rt_buffer_new(8));
        unsafe { align_rt_buffer_append(owner.0, b"shared".as_ptr(), 6) };
        let buffer = unsafe { &*owner.0 };
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(move || {
                    for _ in 0..128 {
                        let published = view((buffer as *const Buffer).cast_mut());
                        assert_eq!(published.len, 6);
                        assert_eq!(published.ptr, buffer.data.as_ptr());
                        assert_eq!(
                            unsafe { core::slice::from_raw_parts(published.ptr, 6) },
                            b"shared"
                        );
                    }
                });
            }
        });
    }
}
