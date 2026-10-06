//! Independent admission, byte parity, publication and allocation owners for hex decoding.
use super::*;

fn oracle(input: &[u8]) -> Option<Vec<u8>> {
    if !input.len().is_multiple_of(2) {
        return None;
    }
    let mut result = Vec::new();
    let mut high = None;
    for &byte in input {
        let value = match byte {
            b'0'..=b'9' => byte - b'0',
            b'A'..=b'F' => byte - b'A' + 10,
            b'a'..=b'f' => byte - b'a' + 10,
            _ => return None,
        };
        if let Some(high) = high.take() {
            result.push(high * 16 + value);
        } else {
            high = Some(value);
        }
    }
    Some(result)
}

fn encoded(raw: &[u8], case: usize) -> Vec<u8> {
    let mut result = Vec::new();
    for (index, &byte) in raw.iter().enumerate() {
        let alphabet = if case == 0 || (case == 2 && index.is_multiple_of(2)) {
            b"0123456789abcdef"
        } else {
            b"0123456789ABCDEF"
        };
        result.push(alphabet[usize::from(byte / 16)]);
        result.push(alphabet[usize::from(byte % 16)]);
    }
    result
}

#[test]
fn hex_group_every_byte_pair_matches_independent_oracle() {
    let mut accepted = 0;
    for high in 0..=u8::MAX {
        for low in 0..=u8::MAX {
            let input = [high, low];
            let expected = oracle(&input);
            accepted += usize::from(expected.is_some());
            assert_eq!(hex_decode_impl(&input), expected, "input={input:?}");
        }
    }
    assert_eq!(accepted, 22 * 22);
}

#[test]
fn hex_group_every_symbol_position_and_tail_matches_oracle() {
    // Includes a second complete group and all three nonempty pair tails.
    for length in 0..=23 {
        let mut input = b"09aFBcD178eE23456789abcdef"[..length].to_vec();
        assert_eq!(hex_decode_impl(&input), oracle(&input));
        for index in 0..length {
            let original = input[index];
            for byte in 0..=u8::MAX {
                input[index] = byte;
                assert_eq!(hex_decode_impl(&input), oracle(&input), "input={input:?}");
            }
            input[index] = original;
        }
    }
}

#[test]
fn hex_group_binary_lengths_and_cases_preserve_all_bytes() {
    for length in (0..=96).chain([1023, 1024, 1025, 65_536]) {
        let mut seed = 0x531ca762u32;
        let raw = (0..length)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                seed.to_be_bytes()[0]
            })
            .collect::<Vec<_>>();
        for case in 0..3 {
            let input = encoded(&raw, case);
            assert_eq!(oracle(&input).as_ref(), Some(&raw));
            assert_eq!(hex_decode_impl(&input).as_ref(), Some(&raw));
        }
    }
}

struct BufferOwner(*mut Buffer);
impl Drop for BufferOwner {
    fn drop(&mut self) {
        unsafe { align_rt_buffer_free(self.0) };
    }
}

#[test]
fn hex_group_native_publication_is_atomic_and_independent() {
    for raw in [vec![], vec![0, 0xff, 0x80, 0x7f], (0..=u8::MAX).collect()] {
        let mut input = encoded(&raw, 2);
        let mut output = BufferOwner(core::ptr::null_mut());
        assert_eq!(
            unsafe {
                align_rt_hex_decode(
                    input.as_ptr(),
                    i64::try_from(input.len()).unwrap(),
                    &mut output.0,
                )
            },
            0
        );
        input.fill(b'!');
        assert!(!output.0.is_null());
        let buffer = unsafe { &*output.0 };
        assert_eq!(&buffer.data[..buffer.len], raw.as_slice());
    }
    for length in [2, 4, 6, 8, 10, 12, 14, 16, 18, 1024] {
        let mut input = vec![b'0'; length];
        for index in [0, length / 2, length - 1] {
            input[index] = 0xff;
            let mut output = BufferOwner(core::ptr::null_mut());
            assert_eq!(
                unsafe {
                    align_rt_hex_decode(
                        input.as_ptr(),
                        i64::try_from(length).unwrap(),
                        &mut output.0,
                    )
                },
                AL_INVALID
            );
            assert!(output.0.is_null());
            input[index] = b'0';
        }
        input.push(b'0');
        let mut output = BufferOwner(core::ptr::null_mut());
        assert_eq!(
            unsafe {
                align_rt_hex_decode(
                    input.as_ptr(),
                    i64::try_from(input.len()).unwrap(),
                    &mut output.0,
                )
            },
            AL_INVALID
        );
        assert!(output.0.is_null());
    }
}

#[cfg(feature = "alloc-count")]
#[test]
fn hex_group_allocates_exact_nonempty_payload_once_and_rejects_odd_before_reserve() {
    let before = global_alloc_count();
    let witness = std::hint::black_box(vec![1u8; 4096]);
    assert!(
        global_alloc_count() > before,
        "counter must observe Rust allocations"
    );
    drop(witness);
    for length in [0, 1, 2, 3, 4, 7, 8, 9, 31, 32, 1024, 65_536] {
        let raw = (0..=u8::MAX).cycle().take(length).collect::<Vec<_>>();
        let mut input = encoded(&raw, 2);
        let count_before = global_alloc_count();
        let bytes_before = global_alloc_bytes();
        let output = hex_decode_impl(std::hint::black_box(&input));
        let allocations = global_alloc_count() - count_before;
        let bytes = global_alloc_bytes() - bytes_before;
        assert_eq!(output.as_ref(), Some(&raw));
        assert_eq!(allocations, u64::from(length != 0));
        assert_eq!(bytes, length);

        input.push(b'0');
        let count_before = global_alloc_count();
        let bytes_before = global_alloc_bytes();
        let output = hex_decode_impl(std::hint::black_box(&input));
        let allocations = global_alloc_count() - count_before;
        let bytes = global_alloc_bytes() - bytes_before;
        assert!(output.is_none());
        assert_eq!((allocations, bytes), (0, 0));
    }
}
