#[cfg(target_arch = "aarch64")]
use super::json_escape_map_neon;
use super::{find_quote_or_escape, json_hex4, json_needs_escape};
#[cfg(target_arch = "x86_64")]
use super::{json_escape_mask_avx2, json_escape_mask_sse2};

/// Decode the escape at the beginning of a raw JSON fragment. A complete
/// surrogate pair consumes twelve source bytes and produces a single scalar.
fn json_escape_scalar(raw: &[u8]) -> Option<(char, usize)> {
    if raw.first() != Some(&b'\\') {
        return None;
    }
    let scalar = match *raw.get(1)? {
        b'"' => '"',
        b'\\' => '\\',
        b'/' => '/',
        b'b' => '\u{8}',
        b'f' => '\u{c}',
        b'n' => '\n',
        b'r' => '\r',
        b't' => '\t',
        b'u' => {
            let hi = json_hex4(raw, 2)?;
            if (0xd800..=0xdbff).contains(&hi) {
                if raw.get(6..8) != Some(b"\\u") {
                    return None;
                }
                let lo = json_hex4(raw, 8)?;
                if !(0xdc00..=0xdfff).contains(&lo) {
                    return None;
                }
                let cp = 0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00);
                return Some((char::from_u32(cp)?, 12));
            }
            return Some((char::from_u32(hi)?, 6));
        }
        _ => return None,
    };
    Some((scalar, 2))
}

/// Walk validated UTF-8 as raw JSON string content. Count, copy and key
/// comparison share one escape grammar. Clean runs never need a staging buffer.
fn json_unescape_chunks(
    mut raw: &[u8],
    mut emit: impl FnMut(&[u8]) -> Option<()>,
) -> Option<usize> {
    let mut decoded = 0;
    while let Some(at) = first_json_special(raw) {
        let (clean, rest) = raw.split_at(at);
        if !clean.is_empty() {
            emit(clean)?;
        }
        let (scalar, consumed) = json_escape_scalar(rest)?;
        let mut buf = [0; 4];
        emit(scalar.encode_utf8(&mut buf).as_bytes())?;
        // Each decoded segment is no longer than the source segment it consumes.
        decoded += at + scalar.len_utf8();
        raw = &rest[consumed..];
    }
    if !raw.is_empty() {
        emit(raw)?;
    }
    Some(decoded + raw.len())
}

pub(super) fn json_unescaped_len(raw: &[u8]) -> Option<usize> {
    json_unescape_chunks(raw, |_| Some(()))
}

pub(super) fn json_unescape_to(raw: &[u8], dst: &mut [u8]) -> Option<usize> {
    let mut offset: usize = 0;
    let len = json_unescape_chunks(raw, |bytes| {
        let end = offset.checked_add(bytes.len())?;
        dst.get_mut(offset..end)?.copy_from_slice(bytes);
        offset = end;
        Some(())
    })?;
    (len == dst.len()).then_some(len)
}

pub(super) fn json_string_equals(raw: &[u8], mut target: &[u8]) -> bool {
    json_unescape_chunks(raw, |bytes| {
        target = target.strip_prefix(bytes)?;
        Some(())
    })
    .is_some()
        && target.is_empty()
}

/// Find a quote, backslash or raw C0 byte. Encoding and decoding use the same
/// byte classifier; only complete vectors are loaded.
#[inline]
fn first_json_special(bytes: &[u8]) -> Option<usize> {
    let head = bytes.len().min(4);
    if let Some(at) = bytes[..head].iter().copied().position(json_needs_escape) {
        return Some(at);
    }
    let rest = &bytes[head..];
    #[cfg(target_arch = "x86_64")]
    if rest.len() >= 32 && is_x86_feature_detected!("avx2") {
        return unsafe { first_json_special_avx2(rest) }.map(|at| head + at);
    }
    #[cfg(target_arch = "x86_64")]
    let at = scan_json_blocks(
        rest,
        |block: &[u8; 16]| u64::from(unsafe { json_escape_mask_sse2(block.as_ptr()) }),
        1,
    );
    #[cfg(target_arch = "aarch64")]
    let at = scan_json_blocks(
        rest,
        |block: &[u8; 16]| unsafe { json_escape_map_neon(block.as_ptr()) },
        4,
    );
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    let at = rest.iter().copied().position(json_needs_escape);
    at.map(|at| head + at)
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[inline(always)]
fn scan_json_blocks<const N: usize>(
    bytes: &[u8],
    mask: impl Fn(&[u8; N]) -> u64,
    bits_per_byte: u32,
) -> Option<usize> {
    let (blocks, tail) = bytes.as_chunks::<N>();
    for (i, block) in blocks.iter().enumerate() {
        let hit = mask(block);
        if hit != 0 {
            return Some(i * N + usize::try_from(hit.trailing_zeros() / bits_per_byte).ok()?);
        }
    }
    tail.iter()
        .copied()
        .position(json_needs_escape)
        .map(|at| blocks.len() * N + at)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn first_json_special_avx2(bytes: &[u8]) -> Option<usize> {
    scan_json_blocks(
        bytes,
        |block: &[u8; 32]| u64::from(unsafe { json_escape_mask_avx2(block.as_ptr()) }),
        1,
    )
}

pub(super) struct JsonStringBody<'a> {
    pub(super) raw: &'a [u8],
    pub(super) escaped_len: Option<usize>,
}

/// Validate the body and return the position following its closing quote.
pub(super) fn json_string_span(src: &[u8], start: usize) -> Option<(JsonStringBody<'_>, usize)> {
    if src.get(start) != Some(&b'"') {
        return None;
    }
    let body = start + 1;
    let mut pos = body + first_json_special(&src[body..])?;
    if src[pos] == b'"' {
        return Some((
            JsonStringBody {
                raw: &src[body..pos],
                escaped_len: None,
            },
            pos + 1,
        ));
    }
    if src[pos] != b'\\' {
        return None;
    }
    let first_escape = pos;
    loop {
        pos = pos.checked_add(2)?;
        pos += find_quote_or_escape(src.get(pos..)?)?;
        if src[pos] == b'"' {
            let raw = &src[body..pos];
            let len = (first_escape - body) + json_unescaped_len(&src[first_escape..pos])?;
            return Some((
                JsonStringBody {
                    raw,
                    escaped_len: Some(len),
                },
                pos + 1,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_scan_finds_the_first_special_byte_across_all_lanes() {
        // Short slices, full AVX2/NEON/SSE2 blocks, unaligned starts and partial tails.
        let mut storage = [b'x'; 171];
        for shift in [0, 1, 5, 15] {
            for n in [0, 1, 3, 4, 15, 16, 17, 31, 32, 33, 63, 64, 65, 129] {
                let body = &mut storage[shift..shift + n];
                assert_eq!(first_json_special(body), None);
                for at in 0..n {
                    for byte in [0, 1, 0x1f, 0x20, b'"', b'\\', 0x7f, 0x80, 0xff] {
                        body[at] = byte;
                        let expected = body.iter().position(|&b| b < 32 || b == b'"' || b == b'\\');
                        assert_eq!(
                            first_json_special(body),
                            expected,
                            "shift={shift} len={n} at={at} byte={byte}"
                        );
                        body[at] = b'x';
                    }
                }
            }
        }
    }

    #[test]
    fn json_string_span_rejects_late_errors_after_a_clean_chunk_or_escape() {
        for n in [0, 4, 15, 16, 31, 32, 63, 64, 127] {
            let prefix = format!("\"{}", "a".repeat(n));
            for suffix in ["\u{1f}\"", "\\n\u{1f}\"", "\\uD800\\u0041\"", "\\", ""] {
                assert!(json_string_span(format!("{prefix}{suffix}").as_bytes(), 0).is_none());
            }
            let doc = format!("{prefix}\\uD83D\\uDE42\\\"z\"!");
            let (body, next) = json_string_span(doc.as_bytes(), 0).unwrap();
            assert_eq!(body.escaped_len, Some(n + 6));
            assert_eq!(body.raw.as_ptr(), doc.as_bytes()[1..].as_ptr());
            assert_eq!(&doc[next..], "!");
        }
    }

    #[test]
    fn json_string_chunks_share_decoded_length_bytes_and_key_equality() {
        let prefix = "界".repeat(29);
        for (escape, decoded) in [
            (r#"\""#, "\""),
            (r"\\", "\\"),
            (r"\/", "/"),
            (r"\b", "\u{8}"),
            (r"\f", "\u{c}"),
            (r"\n", "\n"),
            (r"\r", "\r"),
            (r"\t", "\t"),
            (r"\u0000", "\0"),
            (r"\u6771", "東"),
            (r"\uD83D\uDE42", "🙂"),
        ] {
            let raw = format!("{prefix}{escape}{prefix}{escape}");
            let expected = format!("{prefix}{decoded}{prefix}{decoded}");
            let mut out = vec![0; expected.len()];
            assert_eq!(json_unescaped_len(raw.as_bytes()), Some(out.len()));
            assert_eq!(
                json_unescape_to(raw.as_bytes(), &mut out),
                Some(expected.len())
            );
            assert_eq!(out, expected.as_bytes());
            assert!(json_string_equals(raw.as_bytes(), &out));
            assert!(!json_string_equals(raw.as_bytes(), &out[..out.len() - 1]));
            out.push(0);
            assert!(!json_string_equals(raw.as_bytes(), &out));
        }
    }
}
