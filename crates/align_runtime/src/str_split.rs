use super::{
    AlignStr, align_rt_alloc, align_rt_alloc_size_fail, align_rt_realloc, safe_len, str_subview,
    validate_utf8,
};

unsafe fn split_input<'a>(ptr: *const u8, len: i64) -> Option<&'a str> {
    let n = safe_len(len).ok()?;
    if ptr.is_null() {
        return (n == 0).then_some("");
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, n) };
    if bytes.is_ascii() || validate_utf8(bytes) {
        // Either predicate proves the whole input is valid UTF-8.
        Some(unsafe { std::str::from_utf8_unchecked(bytes) })
    } else {
        None
    }
}

fn split_header_bytes(capacity: usize) -> i64 {
    capacity
        .checked_mul(size_of::<AlignStr>())
        .and_then(|bytes| isize::try_from(bytes).ok())
        .and_then(|bytes| i64::try_from(bytes).ok())
        .unwrap_or_else(|| align_rt_alloc_size_fail())
}

fn split_scalar_fields(text: &str) -> AlignStr {
    let bytes = text.as_bytes();
    let ascii = bytes.is_ascii();
    let scalars = if ascii {
        bytes.len()
    } else {
        text.chars().count()
    };
    let count = scalars + 2;
    let out = align_rt_alloc(split_header_bytes(count)).cast::<AlignStr>();
    unsafe {
        out.write(str_subview(&bytes[..0]));
        if ascii {
            for i in 0..bytes.len() {
                out.add(i + 1).write(AlignStr {
                    ptr: bytes.as_ptr().add(i),
                    len: 1,
                });
            }
        } else {
            for (i, (offset, ch)) in text.char_indices().enumerate() {
                out.add(i + 1)
                    .write(str_subview(&bytes[offset..offset + ch.len_utf8()]));
            }
        }
        out.add(count - 1).write(str_subview(&bytes[bytes.len()..]));
    }
    AlignStr {
        ptr: out.cast(),
        len: count as i64,
    }
}

fn split_literal_fields(
    bytes: &[u8],
    width: usize,
    mut matches: impl Iterator<Item = usize>,
) -> AlignStr {
    let Some(first) = matches.next() else {
        let out = align_rt_alloc(split_header_bytes(1)).cast::<AlignStr>();
        unsafe { out.write(str_subview(bytes)) };
        return AlignStr {
            ptr: out.cast(),
            len: 1,
        };
    };
    // Nonoverlapping matches after the first need at least width remaining bytes each.
    let max_fields =
        (1 + (bytes.len() - first) / width).min(isize::MAX.unsigned_abs() / size_of::<AlignStr>());
    let mut capacity = max_fields.min(16);
    let mut out = align_rt_alloc(split_header_bytes(capacity)).cast::<AlignStr>();
    unsafe { out.write(str_subview(&bytes[..first])) };
    let mut count = 1;
    let mut start = first + width;
    for end in matches {
        // Reserve one additional cell for the trailing field, including an empty one.
        if count + 1 == capacity {
            if capacity == max_fields {
                align_rt_alloc_size_fail();
            }
            capacity = (capacity * 2).min(max_fields);
            out = unsafe {
                align_rt_realloc(out.cast(), split_header_bytes(capacity)).cast::<AlignStr>()
            };
        }
        unsafe { out.add(count).write(str_subview(&bytes[start..end])) };
        count += 1;
        start = end + width;
    }
    unsafe { out.add(count).write(str_subview(&bytes[start..])) };
    AlignStr {
        ptr: out.cast(),
        len: (count + 1) as i64,
    }
}

/// Compare both endpoint bytes at eight potential starts together. Skip long
/// runs without candidates using the vectorized byte search. Every reported
/// occurrence has been confirmed and starts after the previous match.
fn split_short_fields<const N: usize>(bytes: &[u8], needle: [u8; N]) -> AlignStr {
    let first = bytes.get(N - 1..).and_then(|suffix| {
        let at = memchr::memchr(needle[N - 1], suffix)?;
        if bytes[at..].first_chunk() == Some(&needle) {
            Some(at)
        } else {
            let rest = at + 1;
            memchr::memmem::find(&bytes[rest..], &needle).map(|offset| rest + offset)
        }
    });
    let mut scan = first.map_or(bytes.len(), |at| at + N);
    let mut next = scan;
    let mut base = scan;
    let mut mask = 0u64;
    let first_byte = u64::from_le_bytes([needle[0]; 8]);
    let last_byte = u64::from_le_bytes([needle[N - 1]; 8]);
    let matches = std::iter::from_fn(|| {
        loop {
            if mask != 0 {
                let at = base + usize::try_from(mask.trailing_zeros() / 8).ok()?;
                mask &= mask - 1;
                if at >= next && bytes[at..].first_chunk() == Some(&needle) {
                    next = at + N;
                    return Some(at);
                }
            } else if bytes.len() - scan >= N + 7 {
                base = scan;
                let a = u64::from_le_bytes(*bytes[base..].first_chunk()?);
                let b = u64::from_le_bytes(*bytes[base + N - 1..].first_chunk()?);
                let mismatch = (a ^ first_byte) | (b ^ last_byte);
                // Adding LOW cannot carry between bytes; only a zero mismatch leaves bit 7 clear.
                const LOW: u64 = 0x7f7f_7f7f_7f7f_7f7f;
                mask = !(((mismatch & LOW).wrapping_add(LOW)) | mismatch | LOW);
                scan += 8;
                if mask == 0 {
                    scan += memchr::memchr(needle[N - 1], bytes.get(scan + N - 1..)?)?;
                }
            } else {
                let start = scan.max(next);
                let at = start + bytes[start..].windows(N).position(|w| w == needle)?;
                scan = at + N;
                next = scan;
                return Some(at);
            }
        }
    });
    split_literal_fields(bytes, N, first.into_iter().chain(matches))
}

/// Allocate contiguous borrowed field headers for a literal UTF-8 split.
///
/// # Safety
/// Each non-null input must cover its declared readable extent. The source bytes
/// must remain valid for every use of the returned fields. Free the header pointer once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_split(
    source: *const u8,
    source_len: i64,
    separator: *const u8,
    separator_len: i64,
) -> AlignStr {
    let empty = AlignStr {
        ptr: core::ptr::null(),
        len: 0,
    };
    let (Some(source), Some(separator)) = (unsafe { split_input(source, source_len) }, unsafe {
        split_input(separator, separator_len)
    }) else {
        return empty;
    };
    let bytes = source.as_bytes();
    match separator.as_bytes() {
        [] => split_scalar_fields(source),
        [byte] => split_literal_fields(bytes, 1, memchr::memchr_iter(*byte, bytes)),
        &[a, b] => split_short_fields(bytes, [a, b]),
        &[a, b, c] => split_short_fields(bytes, [a, b, c]),
        &[a, b, c, d] => split_short_fields(bytes, [a, b, c, d]),
        needle if needle.len() >= 32 => split_literal_fields(
            bytes,
            needle.len(),
            source.match_indices(separator).map(|(at, _)| at),
        ),
        needle => split_literal_fields(
            bytes,
            needle.len(),
            memchr::memmem::find_iter(bytes, needle),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::super::align_rt_free;
    use super::*;

    #[test]
    fn str_split_matches_every_reference_header_through_growth() {
        for pattern in [
            "a",
            ":",
            "::",
            "aba",
            "aaab",
            "abca",
            "界",
            "x\0",
            "😊",
            "123456789012345678901234567890123",
        ] {
            for repeats in [0, 1, 2, 7, 15, 16, 17, 31, 32, 33, 129] {
                let text = format!("{0}β{0}{0}尾{0}", pattern.repeat(repeats));
                for sep in ["", pattern, ":", "界", "ab", "axca", "abba", "absent"] {
                    let expected: Vec<_> = text.split(sep).collect();
                    let result = unsafe {
                        align_rt_str_split(
                            text.as_ptr(),
                            text.len() as i64,
                            sep.as_ptr(),
                            sep.len() as i64,
                        )
                    };
                    assert_eq!(result.len, expected.len() as i64);
                    let fields = unsafe {
                        std::slice::from_raw_parts(result.ptr.cast::<AlignStr>(), expected.len())
                    };
                    for (got, want) in fields.iter().zip(expected) {
                        assert_eq!((got.ptr, got.len), (want.as_ptr(), want.len() as i64));
                    }
                    unsafe { align_rt_free(result.ptr.cast_mut()) };
                }
            }
        }
        let ascii = "ascii".repeat(33);
        let result =
            unsafe { align_rt_str_split(ascii.as_ptr(), ascii.len() as i64, std::ptr::null(), 0) };
        let fields =
            unsafe { std::slice::from_raw_parts(result.ptr.cast::<AlignStr>(), ascii.len() + 2) };
        for (got, want) in fields.iter().zip(ascii.split("")) {
            assert_eq!((got.ptr, got.len), (want.as_ptr(), want.len() as i64));
        }
        unsafe { align_rt_free(result.ptr.cast_mut()) };
        let empty_input = ascii.as_ptr().wrapping_add(5);
        let result = unsafe { align_rt_str_split(empty_input, 0, empty_input, 0) };
        assert_eq!(result.len, 2);
        let fields = unsafe { std::slice::from_raw_parts(result.ptr.cast::<AlignStr>(), 2) };
        assert!(fields.iter().all(|f| f.len == 0 && f.ptr == empty_input));
        unsafe { align_rt_free(result.ptr.cast_mut()) };
        let mut invalid = vec![b':'; 128];
        invalid.push(0xff);
        let result = unsafe { align_rt_str_split(invalid.as_ptr(), 129, invalid.as_ptr(), 1) };
        assert!(result.ptr.is_null() && result.len == 0);
    }
}
