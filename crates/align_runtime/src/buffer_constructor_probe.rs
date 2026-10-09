//! Scoped native construction observations; absent from production builds.
use super::*;
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HeaderEvent(bool, usize, usize, usize);
thread_local! {
    static REFUSE_HEADER: Cell<bool> = const { Cell::new(false) };
    static HEADERS: RefCell<Option<Vec<HeaderEvent>>> = const { RefCell::new(None) };
}
pub(super) fn refuse_header() -> bool {
    REFUSE_HEADER.replace(false)
}
pub(super) fn header_event(acquire: bool, pointer: *mut Buffer) {
    HEADERS.with_borrow_mut(|events| {
        if let Some(events) = events {
            let layout = std::alloc::Layout::new::<Buffer>();
            events.push(HeaderEvent(
                acquire,
                pointer.addr(),
                layout.size(),
                layout.align(),
            ));
        }
    });
}

struct Owner(*mut Buffer);
impl Drop for Owner {
    fn drop(&mut self) {
        unsafe { align_rt_buffer_free(self.0) };
    }
}
fn construct(filled: bool, count: i64, alignment: i64, out: &mut *mut Buffer) -> i32 {
    unsafe {
        if filled {
            align_rt_buffer_try_filled(count, 0xa5, alignment, 0, out)
        } else {
            align_rt_buffer_try_new(count, alignment, 0, out)
        }
    }
}
fn observe(
    payload: bool,
    header: bool,
    action: impl FnOnce(),
) -> (
    Vec<buffer_storage::AllocationEvent>,
    Vec<HeaderEvent>,
    u8,
    bool,
) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            HEADERS.with_borrow_mut(|events| *events = None);
            REFUSE_HEADER.set(false);
            buffer_storage::ALLOCATION_PROBE.set(0);
        }
    }
    HEADERS.with_borrow_mut(|events| {
        assert!(events.is_none());
        *events = Some(Vec::with_capacity(128));
    });
    let _reset = Reset;
    assert!(!REFUSE_HEADER.replace(header));
    assert_eq!(
        buffer_storage::ALLOCATION_PROBE.replace(u8::from(payload)),
        0
    );
    let payloads = buffer_storage::observe_allocations(action);
    let headers = HEADERS.with_borrow_mut(|events| events.take().unwrap());
    (
        payloads,
        headers,
        buffer_storage::ALLOCATION_PROBE.get(),
        REFUSE_HEADER.get(),
    )
}
fn assert_retired(events: &[buffer_storage::AllocationEvent]) {
    let mut live = std::collections::HashSet::new();
    for buffer_storage::AllocationEvent(acquire, pointer, size, alignment) in events {
        let key = (*pointer, *size, *alignment);
        assert!(*size > 0);
        assert_eq!(pointer % alignment, 0);
        if *acquire {
            assert!(live.insert(key));
        } else {
            assert!(live.remove(&key));
        }
    }
    assert!(live.is_empty(), "unretired payloads {live:?}");
}
fn assert_header_retired(events: &[HeaderEvent], expected: usize) {
    assert_eq!(events.len(), expected * 2);
    for pair in events.chunks_exact(2) {
        let HeaderEvent(acquire, pointer, size, alignment) = pair[0];
        assert!(acquire);
        assert_eq!(pointer % alignment, 0);
        assert_eq!(size, std::mem::size_of::<Buffer>());
        assert_eq!(alignment, std::mem::align_of::<Buffer>());
        assert_eq!(pair[1], HeaderEvent(false, pointer, size, alignment));
    }
}

#[test]
fn zero_filled_payloads_keep_layout_initialization_and_failure_ownership() -> Result<(), core::num::TryFromIntError> {
    for shift in 0..=29 {
        let alignment = 1_i64 << shift;
        let expected_alignment = usize::try_from(alignment)?;
        for length in [0, 17] {
            let expected_length = usize::try_from(length)?;
            for fallible in [false, true] {
                let (payloads, headers, _, _) = observe(false, false, || {
                    let owner = if fallible {
                        let mut out = core::ptr::null_mut();
                        assert_eq!(unsafe { align_rt_buffer_try_filled(length, 0, alignment, 0, &mut out) }, 0);
                        Owner(out)
                    } else { Owner(align_rt_buffer_filled(length, 0, alignment, 0)) };
                    let buffer = unsafe { &mut *owner.0 };
                    assert_eq!((buffer.len, buffer.cap, buffer.data.len()), (expected_length, expected_length, expected_length));
                    assert!(buffer.data.iter().all(|byte| *byte == 0));
                    assert_eq!(buffer.data.writable_ptr().addr() % expected_alignment, 0);
                    buffer.data.fill(0xa5);
                });
                assert_eq!(payloads.len(), if length == 0 { 0 } else { 2 });
                assert_retired(&payloads);
                if fallible { assert_header_retired(&headers, 1); }
            }
        }
    }
    for (payload, header) in [(true, false), (false, true), (true, true)] {
        let (payloads, headers, _, _) = observe(payload, header, || {
            let mut out = core::ptr::dangling_mut();
            assert_eq!(unsafe { align_rt_buffer_try_filled(17, 0, 64, 0, &mut out) }, AL_CODE + libc::ENOMEM);
            assert!(out.is_null());
        });
        assert_eq!(payloads.len(), if payload { 0 } else { 2 });
        assert_retired(&payloads);
        assert!(headers.is_empty());
    }
    Ok(())
}

#[test]
fn fallible_buffer_admission_and_failures() {
    for filled in [false, true] {
        for count in [i64::MIN, -1, 0, 1, i64::MAX] {
            for alignment in [i64::MIN, -1, 0, 3, (1 << 29) + 1, 1 << 30, i64::MAX] {
                let (payloads, headers, payload_pending, header_pending) =
                    observe(true, true, || {
                        let mut out = core::ptr::dangling_mut::<Buffer>();
                        assert_eq!(construct(filled, count, alignment, &mut out), AL_INVALID);
                        assert!(out.is_null());
                    });
                assert!(payloads.is_empty() && headers.is_empty());
                assert_eq!((payload_pending, header_pending), (1, true));
            }
        }
        for count in [i64::MIN, -1, i64::MAX] {
            // i64::MAX with alignment 64 exceeds Layout's rounded isize bound on 64-bit.
            let (payloads, headers, payload_pending, header_pending) = observe(true, true, || {
                let mut out = core::ptr::dangling_mut::<Buffer>();
                assert_eq!(construct(filled, count, 64, &mut out), AL_INVALID);
                assert!(out.is_null());
            });
            assert!(payloads.is_empty() && headers.is_empty());
            assert_eq!((payload_pending, header_pending), (1, true));
        }
        for alignment in [1, 2, 64, 4096] {
            for count in [0, 17] {
                for (payload, header) in [(true, false), (false, true), (true, true)] {
                    let fails = count != 0 && payload || header;
                    let (payloads, headers, payload_pending, header_pending) =
                        observe(payload, header, || {
                            let mut out = core::ptr::dangling_mut::<Buffer>();
                            let status = construct(filled, count, alignment, &mut out);
                            if fails {
                                assert_eq!(status, AL_CODE + libc::ENOMEM);
                                assert!(out.is_null());
                            } else {
                                let owner = Owner(out);
                                assert_eq!(status, 0);
                                assert!(!owner.0.is_null());
                            }
                        });
                    let allocated_payload = count != 0 && !payload;
                    assert_eq!(payloads.len(), if allocated_payload { 2 } else { 0 });
                    assert_retired(&payloads);
                    assert_header_retired(&headers, usize::from(!fails));
                    assert_eq!(payload_pending, u8::from(payload && count == 0));
                    assert_eq!(header_pending, header && count != 0 && payload);
                }
            }
        }
    }
    let (payloads, headers, payload_pending, header_pending) = observe(true, true, || unsafe {
        assert_eq!(
            align_rt_buffer_try_new(1, 1, 0, core::ptr::null_mut()),
            AL_INVALID
        );
        assert_eq!(
            align_rt_buffer_try_filled(1, 0, 1, 0, core::ptr::null_mut()),
            AL_INVALID
        );
    });
    assert!(payloads.is_empty() && headers.is_empty());
    assert_eq!((payload_pending, header_pending), (1, true));
}

#[test]
fn fallible_buffer_layout_and_window() {
    for filled in [false, true] {
        for shift in 0..=29 {
            let alignment = 1_i64 << shift;
            let (payloads, headers, _, _) = observe(false, false, || {
                let mut out = core::ptr::null_mut();
                let status = construct(filled, 0, alignment, &mut out);
                let owner = Owner(out);
                assert_eq!(status, 0);
                let buffer = unsafe { &*owner.0 };
                assert_eq!((buffer.len, buffer.cap), (0, 0));
                assert_eq!(
                    buffer.data.writable_ptr().addr() % usize::try_from(alignment).unwrap(),
                    0
                );
            });
            assert!(payloads.is_empty());
            assert_header_retired(&headers, 1);
        }
        for alignment in [1, 2, 64, 4096] {
            let (payloads, headers, _, _) = observe(false, false, || {
                let mut out = core::ptr::null_mut();
                let status = construct(filled, 32, alignment, &mut out);
                let owner = Owner(out);
                assert_eq!(status, 0);
                let buffer = unsafe { &mut *owner.0 };
                assert_eq!((buffer.len, buffer.cap), (if filled { 32 } else { 0 }, 32));
                assert!(buffer.data.iter().all(|byte| *byte == 0xa5));
                let address = buffer.data.writable_ptr();
                assert_eq!(address.addr() % usize::try_from(alignment).unwrap(), 0);
                assert_eq!(
                    unsafe {
                        file_pread_into_with(7, buffer, 0, 3, 0, |_, target, length, _| {
                            assert_eq!(length, 3);
                            core::ptr::copy_nonoverlapping(b"abc".as_ptr(), target, 3);
                            Ok(3)
                        })
                    },
                    3
                );
                assert_eq!(buffer.data.writable_ptr(), address);
                assert_eq!(buffer.cap, 32);
                assert_eq!(&buffer.data[..3], b"abc");
                if filled {
                    assert!(buffer.data[3..].iter().all(|byte| *byte == 0xa5));
                }
                assert_eq!(
                    buffer_storage::allocation_event_count(),
                    1,
                    "bounded read acquires nothing"
                );
                unsafe {
                    align_rt_buffer_append_filled(owner.0, 80, 7);
                }
                let buffer = unsafe { &*owner.0 };
                assert_eq!(
                    buffer.data.writable_ptr().addr() % usize::try_from(alignment).unwrap(),
                    0
                );
                assert_eq!(&buffer.data[..3], b"abc");
                assert!(buffer.data[buffer.len - 80..].iter().all(|byte| *byte == 7));
            });
            assert_eq!(
                payloads.iter().filter(|event| event.0).count(),
                2,
                "one construction, one growth"
            );
            assert_retired(&payloads);
            assert_header_retired(&headers, 1);
        }
    }
}


#[test]
fn buffer_page_policy_native_admission_and_header_refusal() {
    for filled in [false, true] {
        for policy in [i32::MIN, -1, 2, i32::MAX] {
            for alignment in [-1, 0, 3, 64] {
                for count in [-1, 0, 1, i64::MAX] {
                    let (payloads, headers, payload_pending, header_pending) = observe(true, true, || {
                        let mut out = core::ptr::dangling_mut();
                        let status = unsafe {
                            if filled { align_rt_buffer_try_filled(count, 0, alignment, policy, &mut out) }
                            else { align_rt_buffer_try_new(count, alignment, policy, &mut out) }
                        };
                        assert_eq!(status, AL_INVALID);
                        assert!(out.is_null());
                    });
                    assert!(payloads.is_empty() && headers.is_empty());
                    assert_eq!((payload_pending, header_pending), (1, true));
                }
            }
        }
        #[cfg(target_os = "linux")]
        for refuse_payload in [false, true] {
            let mappings = buffer_pages::observe_events(true, || {
                buffer_pages::REFUSE_MAPPING.set(refuse_payload);
                let (_, headers, _, _) = observe(false, true, || {
                    let mut out = core::ptr::dangling_mut();
                    let status = unsafe {
                        if filled { align_rt_buffer_try_filled(2 << 20, 0, 64, 1, &mut out) }
                        else { align_rt_buffer_try_new(2 << 20, 64, 1, &mut out) }
                    };
                    assert_eq!(status, AL_CODE + libc::ENOMEM);
                    assert!(out.is_null());
                });
                assert!(headers.is_empty());
            });
            if refuse_payload { assert!(mappings.is_empty()); }
            else {
                assert_eq!(mappings.len(), 3);
                let buffer_pages::Event::Acquire(base, span, _, _) = mappings[0] else { panic!("acquisition") };
                assert!(matches!(mappings[1], buffer_pages::Event::Advice(_, _, false)));
                assert!(matches!(mappings[2], buffer_pages::Event::Release(address, length) if address == base && length == span));
            }
        }
    }
    unsafe {
        assert_eq!(align_rt_buffer_try_new(-1, 0, 2, core::ptr::null_mut()), AL_INVALID);
        assert_eq!(align_rt_buffer_try_filled(-1, 0, 0, 2, core::ptr::null_mut()), AL_INVALID);
    }
    #[cfg(target_os = "linux")]
    buffer_pages::observe_events(false, || {
        buffer_pages::REFUSE_MAPPING.set(true);
        let owner = Owner(align_rt_buffer_new(2 << 20, 64, 1));
        assert_eq!(unsafe { align_rt_buffer_capacity(owner.0) }, 0);
    });
}
