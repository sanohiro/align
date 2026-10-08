//! Scalar admission oracle and publication owners for percent/form escape decoding.
use super::*;

fn oracle(input: &[u8], form: bool) -> Option<Vec<u8>> {
    fn digit(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let mut bytes = input.iter().copied();
    let mut output = Vec::new();
    while let Some(byte) = bytes.next() {
        output.push(match byte {
            b'%' => digit(bytes.next()?)? * 16 + digit(bytes.next()?)?,
            b'+' if form => b' ',
            _ => byte,
        });
    }
    Some(output)
}

fn decoders() -> [fn(&[u8]) -> Option<Vec<u8>>; 2] {
    [percent_decode_impl, form_decode_impl]
}

#[test]
fn every_escape_suffix_matches_scalar_admission() {
    let prefix = 128;
    let mut input = vec![b'a'; prefix];
    input.extend_from_slice(b"%00+tail");
    for high in 0..=u8::MAX {
        for low in 0..=u8::MAX {
            input[prefix + 1] = high;
            input[prefix + 2] = low;
            for (form, decode) in decoders().into_iter().enumerate() {
                assert_eq!(
                    decode(&input),
                    oracle(&input, form != 0),
                    "suffix={high},{low}, form={form}"
                );
            }
        }
    }
}

#[test]
fn ordinary_bytes_and_escapes_preserve_binary_values() {
    let mut cases = vec![
        vec![],
        b"+%2b%2B%25%00%ff%FF%80%7f".to_vec(),
        (0..=u8::MAX).filter(|byte| *byte != b'%').collect(),
        "日本+%E8%AA%9e🦀".as_bytes().to_vec(),
        b"a%20b+%2B".repeat(8192),
        vec![b'a'; 65536],
    ];
    for prefix in (0..=80).chain([127, 128, 129, 255, 256, 257, 4095, 4096, 4097]) {
        for tail in [
            b"".as_slice(),
            b"%",
            b"%0",
            b"%0G",
            b"%g0",
            b"%%00",
            b"%+0",
            b"%00+%ff",
            b"+++",
            b"%252B",
            b"z%",
        ] {
            let mut input = vec![b'z'; prefix];
            input.extend_from_slice(tail);
            cases.push(input);
        }
    }
    for input in cases {
        for (form, decode) in decoders().into_iter().enumerate() {
            assert_eq!(
                decode(&input),
                oracle(&input, form != 0),
                "length={}, form={form}",
                input.len()
            );
        }
    }
}

#[test]
fn ordinary_run_boundaries_resume_exact_escape_and_plus_transitions() {
    for length in (0..=65).chain([127, 128, 129, 255, 256, 257, 4095, 4096, 4097]) {
        for prefix in [b"".as_slice(), b"%00", b"+", b"%2b+"] {
            for tail in [b"".as_slice(), b"%ff", b"+%00+", b"%", b"%0", b"%G0"] {
                let mut input = prefix.to_vec();
                input.extend((0..length).map(|index| [b'a', 0, 0xff, b'~'][index % 4]));
                input.extend_from_slice(tail);
                input.extend_from_slice(b"ordinary+%25tail");
                for (form, decode) in decoders().into_iter().enumerate() {
                    assert_eq!(decode(&input), oracle(&input, form != 0),
                        "run={length}, prefix={prefix:?}, tail={tail:?}, form={form}");
                }
            }
        }
    }
}

struct BufferOwner(*mut Buffer);
impl Drop for BufferOwner {
    fn drop(&mut self) {
        unsafe { align_rt_buffer_free(self.0) };
    }
}

type Decode = unsafe extern "C" fn(*const u8, i64, *mut *mut Buffer) -> i32;

#[test]
fn native_publication_preserves_capacity_independence_and_failure_slots() {
    let decoders: [Decode; 2] = [align_rt_percent_decode, align_rt_form_decode];
    for (form, decode) in decoders.into_iter().enumerate() {
        for initial in [
            b"".as_slice(),
            b"plain+text",
            b"a%00%ff+%2b",
            b"%",
            b"before%0g",
            b"after%00%",
        ] {
            let mut input = initial.to_vec();
            let expected = oracle(&input, form != 0);
            let mut output = BufferOwner(core::ptr::null_mut());
            let status = unsafe {
                decode(
                    input.as_ptr(),
                    i64::try_from(input.len()).unwrap(),
                    &mut output.0,
                )
            };
            input.fill(b'!');
            if let Some(expected) = expected {
                assert_eq!(status, 0);
                assert!(!output.0.is_null());
                let buffer = unsafe { &*output.0 };
                assert_eq!(buffer.cap, expected.len());
                assert_eq!(buffer.len, expected.len());
                assert_eq!(&buffer.data[..buffer.len], expected.as_slice());
            } else {
                assert_eq!(status, AL_INVALID);
                assert!(output.0.is_null());
            }
        }
    }
}

#[cfg(feature = "alloc-count")]
#[test]
fn allocation_requests_remain_one_input_sized_reservation() {
    let Some(_completion) = crate::allocation_test::enter() else {
        return;
    };
    for input in [
        vec![],
        vec![b'a'; 4096],
        b"%20+%ff".repeat(1024),
        b"a%0g".repeat(1024),
    ] {
        for (form, decode) in decoders().into_iter().enumerate() {
            let expected = oracle(&input, form != 0);
            let count_before = global_alloc_count();
            let bytes_before = global_alloc_bytes();
            let output = decode(std::hint::black_box(&input));
            let count = global_alloc_count() - count_before;
            let bytes = global_alloc_bytes() - bytes_before;
            assert_eq!(output, expected);
            assert_eq!(count, u64::from(!input.is_empty()));
            assert_eq!(bytes, input.len());
        }
    }
}
