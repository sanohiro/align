//! Valid-input reuse, malformed suffixes and exact owned output for replacement decoding.
use super::*;

struct Owned(AlignStr);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { align_rt_free(self.0.ptr.cast_mut()) };
    }
}

fn decode(input: &[u8]) -> Owned {
    Owned(unsafe {
        align_rt_utf8_decode_lossy(input.as_ptr(), i64::try_from(input.len()).unwrap())
    })
}

fn assert_output(output: &Owned, expected: &[u8]) {
    assert_eq!(output.0.len, i64::try_from(expected.len()).unwrap());
    if expected.is_empty() {
        assert!(output.0.ptr.is_null());
    } else {
        assert!(!output.0.ptr.is_null());
        let actual = unsafe { core::slice::from_raw_parts(output.0.ptr, expected.len()) };
        assert_eq!(actual, expected);
    }
}

#[test]
fn valid_and_mixed_boundaries_publish_independent_exact_text() {
    // Literal replacements are valid input, distinct from replacements inserted for bad bytes.
    let valid = "\0A¢€日本🦀�".as_bytes();
    let tails: &[(&[u8], &[u8])] = &[
        (b"", b""),
        (valid, valid),
        (b"\xff", "�".as_bytes()),
        (b"\xe1\x80", "�".as_bytes()),
        (b"\xe1\x80A", "�A".as_bytes()),
        (b"\xed\xa0\x80", "���".as_bytes()),
        (b"\xf4\x90\x80\x80", "����".as_bytes()),
        (b"\xc2\xa2\xff\xf0\x9f\x92\xa9", "¢�💩".as_bytes()),
    ];
    for length in (0..=80).chain([127, 128, 129, 255, 256, 257, 4095, 4096, 4097, 65536]) {
        for (tail, replacement) in tails {
            for before in [false, true] {
                let mut input = vec![b'a'; length];
                let mut expected = input.clone();
                if before {
                    input.splice(0..0, tail.iter().copied());
                    expected.splice(0..0, replacement.iter().copied());
                } else {
                    input.extend_from_slice(tail);
                    expected.extend_from_slice(replacement);
                }
                let output = decode(&input);
                if !input.is_empty() {
                    assert_ne!(output.0.ptr, input.as_ptr());
                }
                input.fill(b'!');
                assert_output(&output, &expected);
            }
        }
    }
    let input = valid.repeat(8192);
    assert_output(&decode(&input), &input);
}

#[cfg(feature = "alloc-count")]
#[test]
fn exact_payload_allocation_and_drop_are_unchanged() {
    let Some(_completion) = crate::allocation_test::enter() else {
        return;
    };
    for (input, expected) in [
        (vec![], vec![]),
        (vec![b'a'; 65536], vec![b'a'; 65536]),
        (
            "日本\0🦀�".as_bytes().repeat(4096),
            "日本\0🦀�".as_bytes().repeat(4096),
        ),
        (b"a\xe1\x80".repeat(4096), "a�".as_bytes().repeat(4096)),
        (b"\xff".repeat(4096), "�".as_bytes().repeat(4096)),
    ] {
        align_rt_requested_live_reset();
        let counts = (align_rt_alloc_count(), align_rt_free_count());
        let output = decode(std::hint::black_box(&input));
        assert_eq!(
            align_rt_alloc_count() - counts.0,
            i64::from(!expected.is_empty())
        );
        assert_eq!(
            align_rt_requested_live_bytes(),
            i64::try_from(expected.len()).unwrap()
        );
        assert_eq!(
            align_rt_requested_live_peak(),
            i64::try_from(expected.len()).unwrap()
        );
        assert_output(&output, &expected);
        drop(output);
        assert_eq!(
            align_rt_free_count() - counts.1,
            i64::from(!expected.is_empty())
        );
        assert_eq!(align_rt_requested_live_bytes(), 0);
    }
}
