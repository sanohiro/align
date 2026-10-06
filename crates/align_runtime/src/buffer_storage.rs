//! Owned byte storage whose shared publications retain writable pointer provenance.

use core::ops::{Deref, DerefMut};
use std::alloc::{Layout, alloc, dealloc, realloc};

pub(super) struct BufferStorage {
    writable: *mut u8,
    len: usize,
    capacity: usize,
    alignment: usize,
}

// SAFETY: moving the owner transfers the allocation and its exact layout together.
unsafe impl Send for BufferStorage {}
// SAFETY: safe shared access only reads initialized bytes or copies a raw pointer. Dereferencing that
// pointer for writing requires the native caller's exclusive byte access and live-storage proof.
unsafe impl Sync for BufferStorage {}

impl From<Vec<u8>> for BufferStorage {
    fn from(bytes: Vec<u8>) -> Self {
        let mut bytes = core::mem::ManuallyDrop::new(bytes);
        let writable = bytes.as_mut_ptr();
        let owner = Self {
            writable,
            len: bytes.len(),
            capacity: bytes.capacity(),
            alignment: 1,
        };
        #[cfg(test)]
        if owner.capacity != 0 {
            record(true, writable.addr(), owner.capacity, 1);
        }
        owner
    }
}

impl Deref for BufferStorage {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for BufferStorage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: this exclusive owner contains exactly len initialized bytes.
        unsafe { core::slice::from_raw_parts_mut(self.writable, self.len) }
    }
}

impl Drop for BufferStorage {
    fn drop(&mut self) {
        if self.capacity != 0 {
            #[cfg(test)]
            record(false, self.writable.addr(), self.capacity, self.alignment);
            // SAFETY: every acquisition records its exact valid Layout; Vec<u8> adoption uses
            // capacity bytes and alignment 1, never the allocator's incidental over-alignment.
            unsafe { dealloc(self.writable, self.layout()) };
        }
    }
}

impl BufferStorage {
    pub(super) fn new(alignment: usize) -> Self {
        if !alignment.is_power_of_two() || alignment > (1 << 29) {
            crate::panic_abort("buffer alignment must be a power of two between 1 and 536870912");
        }
        Self {
            writable: core::ptr::without_provenance_mut(alignment),
            len: 0,
            capacity: 0,
            alignment,
        }
    }

    fn layout(&self) -> Layout {
        // SAFETY: acquisition checked Layout formation; growth changes these fields atomically.
        unsafe { Layout::from_size_align_unchecked(self.capacity, self.alignment) }
    }

    pub(super) fn as_slice(&self) -> &[u8] {
        // SAFETY: even the empty sentinel is non-null and u8-aligned; len bytes are initialized.
        unsafe { core::slice::from_raw_parts(self.writable, self.len) }
    }

    pub(super) fn as_ptr(&self) -> *const u8 {
        self.writable.cast_const()
    }
    pub(super) fn as_mut_ptr(&mut self) -> *mut u8 {
        self.writable
    }
    pub(super) fn capacity(&self) -> usize {
        self.capacity
    }
    pub(super) fn len(&self) -> usize {
        self.len
    }
    pub(super) fn clear(&mut self) {
        self.len = 0;
    }
    pub(super) fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }

    /// The caller must have initialized the prefix and proved len <= capacity.
    pub(super) unsafe fn set_len(&mut self, len: usize) {
        self.len = len;
    }

    pub(super) fn spare_capacity_mut(&mut self) -> &mut [core::mem::MaybeUninit<u8>] {
        // SAFETY: this is exclusively borrowed allocated storage after the initialized prefix.
        unsafe {
            core::slice::from_raw_parts_mut(
                self.writable.add(self.len).cast(),
                self.capacity - self.len,
            )
        }
    }

    fn grow(&mut self, required: usize, exact: bool) -> Result<(), ()> {
        if required <= self.capacity {
            return Ok(());
        }
        let required_layout = Layout::from_size_align(required, self.alignment).map_err(|_| ())?;
        let layout = if exact {
            required_layout
        } else {
            self.capacity
                .checked_mul(2)
                .and_then(|capacity| {
                    Layout::from_size_align(capacity.max(required).max(8), self.alignment).ok()
                })
                .unwrap_or(required_layout)
        };
        #[cfg(test)]
        let previous = (self.writable.addr(), self.capacity);
        #[cfg(test)]
        match ALLOCATION_PROBE.replace(0) {
            1 => return Err(()),
            2 => crate::panic_abort("buffer test allocation reached"),
            _ => {}
        }
        // SAFETY: the new nonzero layout is checked and retains the old alignment. Reallocation
        // transfers the initialized prefix on success and leaves the old allocation on failure.
        let replacement = unsafe {
            if self.capacity == 0 {
                alloc(layout)
            } else {
                realloc(self.writable, self.layout(), layout.size())
            }
        };
        if replacement.is_null() {
            return Err(());
        }
        // Publish the returned provenance even for in-place success, before test observers can
        // unwind. The old pointer is no longer an owner after a successful realloc.
        self.writable = replacement;
        self.capacity = layout.size();
        #[cfg(test)]
        {
            if previous.1 != 0 {
                record(false, previous.0, previous.1, self.alignment);
            }
            record(true, self.writable.addr(), self.capacity, self.alignment);
        }
        Ok(())
    }

    pub(super) fn try_reserve_exact(&mut self, additional: usize) -> Result<(), ()> {
        self.grow(self.len.checked_add(additional).ok_or(())?, true)
    }
    pub(super) fn try_reserve(&mut self, additional: usize) -> Result<(), ()> {
        self.grow(self.len.checked_add(additional).ok_or(())?, false)
    }
    fn reserve(&mut self, additional: usize) {
        if self.try_reserve(additional).is_err() {
            crate::panic_abort("buffer allocation failed");
        }
    }
    #[cfg(test)]
    pub(super) fn reserve_exact(&mut self, additional: usize) {
        if self.try_reserve_exact(additional).is_err() {
            crate::panic_abort("buffer allocation failed");
        }
    }
    pub(super) fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.reserve(bytes.len());
        // SAFETY: reserve proved the full suffix fits; a live slice cannot overlap an exclusive
        // owner. The native self-append entrypoint snapshots raw aliases before this operation.
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), self.writable.add(self.len), bytes.len())
        };
        self.len += bytes.len();
    }
    pub(super) fn push(&mut self, byte: u8) {
        self.reserve(1);
        // SAFETY: one reserved uninitialized byte follows the initialized prefix.
        unsafe { self.writable.add(self.len).write(byte) };
        self.len += 1;
    }
    pub(super) fn resize(&mut self, len: usize, value: u8) {
        if len > self.len {
            self.reserve(len - self.len);
            // SAFETY: reserve admitted the suffix; byte initialization cannot unwind.
            unsafe {
                self.writable
                    .add(self.len)
                    .write_bytes(value, len - self.len)
            };
        }
        self.len = len;
    }

    pub(super) fn writable_ptr(&self) -> *mut u8 {
        self.writable
    }

    // Every storage method preserves the raw acquisition pointer; R cannot borrow from input.
    pub(super) fn with_mut<R>(&mut self, operation: impl FnOnce(&mut Self) -> R) -> R {
        operation(self)
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AllocationEvent(pub bool, pub usize, pub usize, pub usize);
#[cfg(test)]
std::thread_local! {
    static EVENTS: core::cell::RefCell<Option<Vec<AllocationEvent>>> = const { core::cell::RefCell::new(None) };
    pub(super) static ALLOCATION_PROBE: core::cell::Cell<u8> = const { core::cell::Cell::new(0) };
}
#[cfg(test)]
fn record(acquire: bool, address: usize, size: usize, alignment: usize) {
    EVENTS.with_borrow_mut(|events| {
        if let Some(events) = events {
            events.push(AllocationEvent(acquire, address, size, alignment));
        }
    });
}
#[cfg(test)]
pub(super) fn allocation_event_count() -> usize {
    EVENTS.with_borrow(|events| {
        events
            .as_ref()
            .expect("active allocation observation")
            .len()
    })
}
#[cfg(test)]
pub(super) fn observe_allocations(action: impl FnOnce()) -> Vec<AllocationEvent> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            EVENTS.with_borrow_mut(|events| {
                *events = None;
            });
        }
    }
    EVENTS.with_borrow_mut(|events| {
        assert!(events.is_none());
        *events = Some(Vec::with_capacity(256));
    });
    let _reset = Reset;
    action();
    EVENTS.with_borrow_mut(|events| events.take().unwrap())
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
    fn aligned_storage_growth_and_adoption_match_every_allocation_layout() {
        use super::{AllocationEvent, observe_allocations};
        let events = observe_allocations(|| {
            for alignment in [1, 2, 8, 64, 4096, 16384] {
                let mut storage = BufferStorage::new(alignment);
                assert_eq!(storage.writable_ptr().addr() % alignment, 0);
                storage.try_reserve_exact(3).unwrap();
                assert_eq!(storage.capacity(), 3);
                storage.extend_from_slice(b"abc");
                storage.resize(8193, 0x5a);
                assert_eq!(storage.writable_ptr().addr() % alignment, 0);
                assert_eq!(&storage[..3], b"abc");
                assert!(storage[3..].iter().all(|byte| *byte == 0x5a));
                let before = (storage.writable_ptr(), storage.capacity(), storage.len());
                assert!(storage.try_reserve_exact(usize::MAX).is_err());
                assert_eq!(
                    before,
                    (storage.writable_ptr(), storage.capacity(), storage.len())
                );
                super::ALLOCATION_PROBE.set(1);
                assert!(storage.try_reserve_exact(16384).is_err());
                assert_eq!(
                    super::ALLOCATION_PROBE.get(),
                    0,
                    "allocation failure witness was consumed"
                );
                assert_eq!(
                    before,
                    (storage.writable_ptr(), storage.capacity(), storage.len())
                );
                assert_eq!(&storage[..3], b"abc");
                assert!(storage[3..].iter().all(|byte| *byte == 0x5a));
                storage.clear();
                assert_eq!(storage.writable_ptr().addr() % alignment, 0);
            }
            // Exercise every admitted alignment without asking an allocator to reserve enormous
            // alignment padding. Real allocation/growth above includes both supported page sizes.
            for power in 0..=29 {
                let storage = BufferStorage::new(1 << power);
                assert_eq!(storage.writable_ptr().addr() % (1 << power), 0);
                assert_eq!(storage.len(), 0);
            }
            let mut bytes = Vec::with_capacity(97);
            bytes.extend_from_slice(b"adopted");
            let ptr = bytes.as_ptr();
            let storage = BufferStorage::from(bytes);
            assert_eq!(ptr, storage.as_ptr());
            assert_eq!(storage.as_slice(), b"adopted");
        });
        assert_eq!(events.iter().filter(|event| event.0).count(), 13);
        let mut live = std::collections::BTreeMap::new();
        for AllocationEvent(acquire, pointer, size, alignment) in events {
            assert_ne!(pointer, 0);
            assert_eq!(pointer % alignment, 0);
            if acquire {
                assert!(live.insert(pointer, (size, alignment)).is_none());
            } else {
                assert_eq!(live.remove(&pointer), Some((size, alignment)));
            }
        }
        assert!(live.is_empty());
    }

    #[test]
    fn buffer_storage_refreshes_every_exclusive_transition() {
        let mut storage = BufferStorage::from(vec![1, 2, 3]);
        assert_current(&storage);
        let previous = storage.writable_ptr();
        storage.with_mut(|bytes| {
            // Both allocations are live while constructing the replacement, so their addresses
            // differ even if a growth allocator could otherwise extend a buffer in place.
            *bytes = vec![4; 1024].into();
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
                *bytes = vec![2; 128].into();
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
            let owner = Owner(align_rt_buffer_new(capacity, 1));
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
        let owner = Owner(align_rt_buffer_new(8, 1));
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
