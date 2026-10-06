//! Independent scalar oracle for the shared Base64 quantum decoder.
use super::*;

fn oracle(input: &[u8], url: bool) -> Option<Vec<u8>> {
    let end = input.iter().position(|c| *c == b'=').unwrap_or(input.len());
    let padding = input.len() - end;
    if padding > 2 || input[end..].iter().any(|c| *c != b'=') {
        return None;
    }
    let remainder = end % 4;
    if remainder == 1 || (padding > 0 && (remainder + padding != 4)) {
        return None;
    }
    let mut out = Vec::new();
    let mut accumulator = 0u32;
    let mut bits = 0u32;
    for &c in &input[..end] {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' if !url => 62,
            b'/' if !url => 63,
            b'-' if url => 62,
            b'_' if url => 63,
            _ => return None,
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((accumulator >> bits) & 255).ok()?);
        }
    }
    if accumulator & ((1 << bits) - 1) != 0 {
        return None;
    }
    Some(out)
}

fn encoded(raw: &[u8], url: bool, padded: bool) -> Vec<u8> {
    let alphabet = if url { BASE64_URL } else { BASE64_STD };
    let mut result = Vec::new();
    let mut accumulator = 0usize;
    let mut bits = 0usize;
    for &byte in raw {
        accumulator = (accumulator << 8) | usize::from(byte);
        bits += 8;
        loop {
            if bits < 6 {
                break;
            }
            bits -= 6;
            let digit = (accumulator >> bits) & 63;
            result.push(alphabet[digit]);
        }
    }
    if bits != 0 {
        let digit = (accumulator << (6 - bits)) & 63;
        result.push(alphabet[digit]);
    }
    if padded {
        while !result.len().is_multiple_of(4) {
            result.push(b'=');
        }
    }
    result
}

#[test]
fn base64_quantum_every_symbol_position_and_tail_matches_scalar_oracle() {
    for url in [false, true] {
        for body in [b"AAAA".as_slice(), b"AA", b"AAA", b"AAAAAA", b"AAAAAAA"] {
            for padded in [false, true] {
                let mut input = body.to_vec();
                if padded {
                    while !input.len().is_multiple_of(4) {
                        input.push(b'=');
                    }
                }
                for index in 0..input.len() {
                    let original = input[index];
                    for value in 0..=u8::MAX {
                        input[index] = value;
                        assert_eq!(
                            base64_decode_impl(&input, url),
                            oracle(&input, url),
                            "url={url} input={input:?}"
                        );
                    }
                    input[index] = original;
                }
            }
        }
        for input in [
            b"".as_slice(),
            b"=",
            b"==",
            b"===",
            b"A",
            b"AAAA=",
            b"AA===",
            b"A=AA",
            b"AAAA\n",
            b"AAAA AA",
        ] {
            assert_eq!(
                base64_decode_impl(input, url),
                oracle(input, url),
                "url={url} input={input:?}"
            );
        }
    }
}

#[test]
fn base64_quantum_binary_lengths_and_padding_preserve_all_bytes() {
    for url in [false, true] {
        for padded in [false, true] {
            for length in (0..=96).chain([1023, 1024, 1025, 65_536]) {
                let raw = (0..=u8::MAX).cycle().take(length).collect::<Vec<_>>();
                let input = encoded(&raw, url, padded);
                assert_eq!(oracle(&input, url).as_ref(), Some(&raw));
                assert_eq!(base64_decode_impl(&input, url).as_ref(), Some(&raw));
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

#[test]
fn base64_quantum_native_publication_is_atomic_and_independent() -> Result<(), String> {
    type Decode = unsafe extern "C" fn(*const u8, i64, *mut *mut Buffer) -> i32;
    for (url, decode) in [
        (false, align_rt_base64_decode as Decode),
        (true, align_rt_base64url_decode),
    ] {
        for raw in [vec![], vec![0, 0xff, 0x80, 0x7f], b"binary output".to_vec()] {
            let mut input = encoded(&raw, url, true);
            let size = i64::try_from(input.len()).map_err(|e| e.to_string())?;
            let mut output = BufferOwner(core::ptr::null_mut());
            assert_eq!(unsafe { decode(input.as_ptr(), size, &mut output.0) }, 0);
            input.fill(b'!');
            assert!(!output.0.is_null());
            let buffer = unsafe { &*output.0 };
            assert_eq!(&buffer.data[..buffer.len], raw.as_slice());
        }
        for suffix in [b"!AAA".as_slice(), b"AA!A", b"Zh", b"Zm9", b"Zg=", b"A"] {
            let input = [b"Zm9v".as_slice(), suffix].concat();
            let size = i64::try_from(input.len()).map_err(|e| e.to_string())?;
            let mut output = BufferOwner(core::ptr::null_mut());
            assert_eq!(
                unsafe { decode(input.as_ptr(), size, &mut output.0) },
                AL_INVALID
            );
            assert!(output.0.is_null());
        }
    }
    Ok(())
}

#[cfg(feature = "alloc-count")]
#[test]
fn base64_quantum_allocates_only_nonempty_output_once() {
    let before = global_alloc_count();
    let witness = std::hint::black_box(vec![1u8; 4096]);
    assert!(
        global_alloc_count() > before,
        "counter must observe Rust allocations"
    );
    drop(witness);
    for url in [false, true] {
        for input in [b"!AAA".as_slice(), b"=AAA", b"\xffAAA"] {
            let before = global_alloc_count();
            let output = base64_decode_impl(std::hint::black_box(input), url);
            let allocations = global_alloc_count() - before;
            assert!(output.is_none());
            assert_eq!(allocations, 0, "invalid first symbol allocated output");
        }
        for length in [0, 1, 2, 3, 4, 31, 32, 1024] {
            let raw = (0..=u8::MAX).cycle().take(length).collect::<Vec<_>>();
            let input = encoded(&raw, url, true);
            let before = global_alloc_count();
            let output = base64_decode_impl(std::hint::black_box(&input), url);
            let allocations = global_alloc_count() - before;
            assert_eq!(output.as_ref(), Some(&raw));
            assert_eq!(allocations, u64::from(length != 0));
        }
    }
}
