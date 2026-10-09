//! Private page-policy ownership for explicitly opted-in buffers (plan161).
#[cfg(target_os = "linux")]
use core::ptr::NonNull;

pub(super) const THRESHOLD: usize = 2 * 1024 * 1024;

/// Exact mapping provenance. No hinted extent is returned to the global allocator.
pub(super) struct Mapping {
    #[cfg(target_os = "linux")]
    base: NonNull<u8>,
    #[cfg(target_os = "linux")]
    span: usize,
}

// SAFETY: the mapping is exclusively owned and transfers with its buffer.
unsafe impl Send for Mapping {}
// SAFETY: shared access to the owner does not expose mutable access to its bytes.
unsafe impl Sync for Mapping {}

impl Drop for Mapping {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            // SAFETY: base/span are the original live mapping, never a payload interior.
            #[cfg(test)]
            if REFUSE_RELEASE.get() {
                crate::panic_abort("buffer mapping release failed");
            }
            if unsafe { libc::munmap(self.base.as_ptr().cast(), self.span) } != 0 {
                crate::panic_abort("buffer mapping release failed");
            }
            #[cfg(test)]
            observe(Event::Release(self.base.as_ptr().addr(), self.span));
        }
    }
}

fn page_size() -> Option<usize> {
    #[cfg(test)]
    if UNSUPPORTED.get() {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
        page.is_power_of_two().then_some(page)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub(super) fn eligible(capacity: usize) -> bool {
    capacity >= THRESHOLD && page_size().is_some()
}

#[cfg(any(target_os = "linux", test))]
fn span(capacity: usize, alignment: usize, page: usize) -> Option<usize> {
    if capacity == 0 || !alignment.is_power_of_two() || !page.is_power_of_two() {
        return None;
    }
    let bytes = capacity.checked_add(alignment.checked_sub(1)?)?;
    let rounded = bytes.checked_add(page.checked_sub(1)?)? & !(page - 1);
    (rounded <= isize::MAX.unsigned_abs()).then_some(rounded)
}

#[cfg(any(target_os = "linux", test))]
fn interior(address: usize, capacity: usize, page: usize) -> Option<(usize, usize)> {
    if !page.is_power_of_two() {
        return None;
    }
    let start = address.checked_add(page.checked_sub(1)?)? & !(page - 1);
    let end = address.checked_add(capacity)? & !(page - 1);
    (end > start).then(|| (start, end - start))
}

pub(super) fn allocate(capacity: usize, alignment: usize) -> Result<(Mapping, *mut u8), ()> {
    #[cfg(target_os = "linux")]
    {
        #[cfg(test)]
        if REFUSE_MAPPING.get() {
            return Err(());
        }
        let page = page_size().ok_or(())?;
        let span = span(capacity, alignment, page).ok_or(())?;
        // SAFETY: a fresh anonymous private mapping owns span initialized zero bytes. Alignment
        // padding remains within this owner and is never exposed as extra readable capacity.
        let raw = unsafe {
            libc::mmap(
                core::ptr::null_mut(),
                span,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            return Err(());
        }
        let Some(base) = NonNull::new(raw.cast::<u8>()) else {
            // A valid zero-address mapping cannot satisfy the nonnull buffer contract.
            if unsafe { libc::munmap(raw, span) } != 0 {
                crate::panic_abort("buffer mapping release failed");
            }
            return Err(());
        };
        let owner = Mapping { base, span };
        let offset = (alignment - base.as_ptr().addr() % alignment) % alignment;
        // SAFETY: checked span includes alignment-1 padding and capacity bytes after this offset.
        let payload = unsafe { base.as_ptr().add(offset) };
        #[cfg(test)]
        observe(Event::Acquire(
            base.as_ptr().addr(),
            span,
            payload.addr(),
            capacity,
        ));
        if let Some((start, length)) = interior(payload.addr(), capacity, page) {
            #[cfg(test)]
            let refused = REFUSE_ADVICE.get();
            #[cfg(not(test))]
            let refused = false;
            // SAFETY: advise only complete pages of this owned payload. Refusal leaves the same
            // initialized mapping live; neither success nor refusal proves physical huge backing.
            let accepted = !refused
                && unsafe {
                    libc::madvise(
                        payload.add(start - payload.addr()).cast(),
                        length,
                        libc::MADV_HUGEPAGE,
                    )
                } == 0;
            #[cfg(test)]
            observe(Event::Advice(start, length, accepted));
            #[cfg(not(test))]
            let _ = accepted;
        }
        Ok((owner, payload))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (capacity, alignment);
        Err(())
    }
}

#[cfg(all(test, target_os = "linux"))]
#[derive(Debug, Clone, Copy)]
pub(super) enum Event {
    Acquire(usize, usize, usize, usize),
    Advice(usize, usize, bool),
    Release(usize, usize),
}
#[cfg(test)]
std::thread_local! {
    #[cfg(target_os = "linux")]
    pub(super) static REFUSE_MAPPING: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    #[cfg(target_os = "linux")]
    pub(super) static REFUSE_RELEASE: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    pub(super) static UNSUPPORTED: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    #[cfg(target_os = "linux")]
    pub(super) static REFUSE_ADVICE: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    #[cfg(target_os = "linux")]
    static EVENTS: core::cell::RefCell<Option<Vec<Event>>> = const { core::cell::RefCell::new(None) };
}
#[cfg(all(test, target_os = "linux"))]
fn observe(event: Event) {
    EVENTS.with_borrow_mut(|events| {
        if let Some(events) = events {
            events.push(event);
        }
    });
}
#[cfg(all(test, target_os = "linux"))]
pub(super) fn observe_events(refuse: bool, operation: impl FnOnce()) -> Vec<Event> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            REFUSE_ADVICE.set(false);
            REFUSE_MAPPING.set(false);
            REFUSE_RELEASE.set(false);
            UNSUPPORTED.set(false);
            EVENTS.with_borrow_mut(|events| *events = None);
        }
    }
    EVENTS.with_borrow_mut(|events| {
        assert!(events.is_none());
        *events = Some(Vec::with_capacity(64));
    });
    REFUSE_ADVICE.set(refuse);
    let _reset = Reset;
    operation();
    EVENTS.with_borrow_mut(|events| events.take().expect("active mapping observer"))
}

/// Run in the existing bounded, single-test child so unrelated allocator traffic cannot race
/// the released address. The real kernel must accept exact reuse with no old huge-page flag.
#[cfg(all(test, target_os = "linux"))]
pub(super) fn verify_retired_mapping_reuse() {
    let events = observe_events(false, || {
        drop(
            crate::buffer_storage::BufferStorage::try_filled(THRESHOLD, 0, 64, 1)
                .expect("hinted owner"),
        );
    });
    let Event::Acquire(base, span, _, _) = events[0] else {
        panic!("mapping acquisition");
    };
    let raw = unsafe {
        libc::mmap(
            core::ptr::without_provenance_mut(base),
            span,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED_NOREPLACE,
            -1,
            0,
        )
    };
    assert_ne!(raw, libc::MAP_FAILED, "original mapping was not released");
    let ordinary = Mapping {
        base: NonNull::new(raw.cast()).expect("reused nonnull address"),
        span,
    };
    assert_eq!(raw.addr(), base);
    let maps = std::fs::read_to_string("/proc/self/smaps").expect("mapping flags");
    let mut selected = false;
    let mut found = false;
    for line in maps.lines() {
        if let Some((begin, end)) = line
            .split_whitespace()
            .next()
            .and_then(|word| word.split_once('-'))
            && let (Ok(begin), Ok(end)) = (
                usize::from_str_radix(begin, 16),
                usize::from_str_radix(end, 16),
            )
        {
            selected = begin <= base && base < end;
        }
        if selected && line.starts_with("VmFlags:") {
            assert!(
                !line.split_whitespace().any(|flag| flag == "hg"),
                "fresh mapping inherited hint"
            );
            found = true;
        }
    }
    assert!(found, "kernel mapping flags unavailable");
    drop(ordinary);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer_storage::BufferStorage;

    #[test]
    fn buffer_page_policy_checked_extent_arithmetic() {
        assert_eq!(span(4096, 1, 4096), Some(4096));
        assert_eq!(span(4097, 64, 4096), Some(8192));
        for (capacity, alignment, page) in [
            (0, 1, 4096),
            (1, 0, 4096),
            (1, 3, 4096),
            (1, 1, 3),
            (usize::MAX, 1, 4096),
            (isize::MAX.unsigned_abs(), 1 << 29, 4096),
        ] {
            assert_eq!(span(capacity, alignment, page), None);
        }
        assert_eq!(interior(1, 8191, 4096), Some((4096, 4096)));
        assert_eq!(interior(4096, 8193, 4096), Some((4096, 8192)));
        assert_eq!(interior(1, 4095, 4096), None);
        assert_eq!(interior(usize::MAX - 3, 8, 4096), None);
    }

    #[test]
    fn buffer_page_policy_portable_alignment_and_growth() -> Result<(), ()> {
        for pages in [0, 1] {
            for alignment in [1, 64, 4096, 16384] {
                let mut storage = BufferStorage::try_filled(THRESHOLD, 0xa5, alignment, pages)?;
                assert_eq!(storage.as_ptr().addr() % alignment, 0);
                assert_eq!(storage.alignment(), alignment);
                assert!(storage.iter().all(|byte| *byte == 0xa5));
                storage.push(7);
                assert_eq!(storage.as_ptr().addr() % alignment, 0);
                assert!(storage[..THRESHOLD].iter().all(|byte| *byte == 0xa5));
                assert_eq!(storage[THRESHOLD], 7);
            }
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn retired(events: &[Event]) -> usize {
        let mut live = std::collections::BTreeMap::new();
        let mut count = 0;
        for event in events {
            match *event {
                Event::Acquire(base, span, payload, capacity) => {
                    assert!(live.insert(base, (span, payload, capacity)).is_none());
                    assert!(payload >= base && payload + capacity <= base + span);
                    count += 1;
                }
                Event::Advice(start, length, _) => {
                    let page = page_size().expect("Linux page size");
                    assert_eq!(start % page, 0);
                    assert_eq!(length % page, 0);
                    assert!(length > 0);
                    assert!(live.values().any(|(_, payload, capacity)| start >= *payload
                        && start + length <= payload + capacity));
                }
                Event::Release(base, span) => {
                    let old = live.remove(&base).expect("exact live mapping");
                    assert_eq!(old.0, span);
                }
            }
        }
        assert!(live.is_empty());
        count
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn buffer_page_policy_mapping_lifecycle_and_advice_refusal() {
        for refused in [false, true] {
            for value in [0, 0xa5] {
                for alignment in [1, 64, 4096, 16384, 1 << 22, 1 << 29] {
                    let events = observe_events(refused, || {
                        let mut storage =
                            BufferStorage::try_filled(THRESHOLD + 1, value, alignment, 1)
                                .expect("mapping");
                        assert_eq!(storage.as_ptr().addr() % alignment, 0);
                        assert!(storage.iter().all(|byte| *byte == value));
                        let pointer = storage.as_ptr();
                        assert!(storage.try_reserve_exact(0).is_ok());
                        assert_eq!(storage.as_ptr(), pointer);
                        REFUSE_MAPPING.set(true);
                        assert!(storage.try_reserve_exact(1).is_err());
                        assert_eq!(
                            (storage.as_ptr(), storage.len(), storage.capacity()),
                            (pointer, THRESHOLD + 1, THRESHOLD + 1)
                        );
                        assert!(storage.iter().all(|byte| *byte == value));
                        REFUSE_MAPPING.set(false);
                        storage.push(7);
                        assert_ne!(storage.as_ptr(), pointer);
                        assert_eq!(storage.as_ptr().addr() % alignment, 0);
                        assert!(storage[..THRESHOLD + 1].iter().all(|byte| *byte == value));
                        assert_eq!(storage[THRESHOLD + 1], 7);
                        let moved = storage;
                        drop(moved);
                    });
                    assert_eq!(retired(&events), 2);
                    assert_eq!(
                        events
                            .iter()
                            .filter(|e| matches!(e, Event::Advice(..)))
                            .count(),
                        2
                    );
                    if refused {
                        assert!(
                            events
                                .iter()
                                .all(|e| !matches!(e, Event::Advice(_, _, true)))
                        );
                    }
                }
            }
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn buffer_page_policy_threshold_default_and_unsupported() {
        for capacity in [0, 1, THRESHOLD - 1, THRESHOLD, THRESHOLD + 1] {
            for pages in [0, 1] {
                for unsupported in [false, true] {
                    let events = observe_events(false, || {
                        UNSUPPORTED.set(unsupported);
                        let storage =
                            BufferStorage::try_filled(capacity, 0, 64, pages).expect("payload");
                        assert!(storage.iter().all(|byte| *byte == 0));
                        assert_eq!(storage.as_ptr().addr() % 64, 0);
                    });
                    let expected = usize::from(pages == 1 && capacity >= THRESHOLD && !unsupported);
                    assert_eq!(retired(&events), expected);
                }
            }
        }
        let events = observe_events(false, || {
            let mut storage = BufferStorage::try_filled(17, 5, 64, 1).expect("global payload");
            storage.resize(THRESHOLD, 0);
            assert_eq!(&storage[..17], &[5; 17]);
            assert!(storage[17..].iter().all(|byte| *byte == 0));
            let pointer = storage.as_ptr();
            storage.clear();
            storage.extend_from_slice(b"reuse");
            assert_eq!(storage.as_ptr(), pointer);
            assert_eq!(storage.as_slice(), b"reuse");
            let adopted = BufferStorage::from(vec![1; THRESHOLD]);
            assert!(!adopted.prefers_huge());
        });
        assert_eq!(retired(&events), 1);
    }
}
