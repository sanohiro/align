use super::safe_slice;

/// For a small Unicode set, search the leading bytes together. On a shared
/// prefix that does not match a whole scalar, switch to full substring searches
/// so a long run of near misses does not repeatedly enter the byte dispatcher.
fn find_small_scalar_set(haystack: &[u8], members: &[&[u8]]) -> Option<usize> {
    let candidate = match members {
        [a] => return memchr::memmem::find(haystack, a),
        [a, b] => memchr::memchr2(a[0], b[0], haystack)?,
        [a, b, c] => memchr::memchr3(a[0], b[0], c[0], haystack)?,
        _ => return None,
    };
    let suffix = &haystack[candidate..];
    if members.iter().any(|member| suffix.starts_with(member)) {
        return Some(candidate);
    }
    let rest = &suffix[1..];
    let mut best = None;
    for member in members {
        // A complete UTF-8 scalar cannot straddle another scalar's start.
        if let Some(at) = memchr::memmem::find(&rest[..best.unwrap_or(rest.len())], member) {
            best = Some(at);
        }
    }
    best.map(|at| candidate + 1 + at)
}

fn leading_scalar(bytes: &[u8]) -> Option<&[u8]> {
    let width = match *bytes.first()? {
        0..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    bytes.get(..width)
}

fn scalar_in_set(scalar: &[u8], set: &[u8]) -> bool {
    let Some((&first, tail)) = scalar.split_first() else {
        return false;
    };
    let Some(at) = memchr::memchr(first, set) else {
        return false;
    };
    let suffix = &set[at + 1..];
    suffix.starts_with(tail) || memchr::memmem::find(suffix, scalar).is_some()
}

fn find_in_large_scalar_set(haystack: &[u8], set: &[u8]) -> Option<usize> {
    // Short receivers can finish without building an index for a long delimiter set.
    let mut offset = 0;
    let mut previous_miss = &[][..];
    while offset < haystack.len().min(8) {
        let scalar = leading_scalar(&haystack[offset..])?;
        if scalar != previous_miss && scalar_in_set(scalar, set) {
            return Some(offset);
        }
        previous_miss = scalar;
        offset += scalar.len();
    }
    if offset == haystack.len() {
        return None;
    }

    // Index leading bytes once. ASCII candidates immediately prove a match;
    // multibyte candidates must match a complete scalar in the delimiter set.
    let mut starts = [false; 256];
    let mut short = [0; 3];
    let mut distinct = 0;
    for &byte in set {
        if byte & 0xc0 == 0x80 || starts[usize::from(byte)] {
            continue;
        }
        starts[usize::from(byte)] = true;
        if let Some(slot) = short.get_mut(distinct) {
            *slot = byte;
        }
        distinct += 1;
    }
    while offset < haystack.len() {
        let rest = &haystack[offset..];
        let candidate = match distinct {
            0 => return None,
            1 => memchr::memchr(short[0], rest)?,
            2 => memchr::memchr2(short[0], short[1], rest)?,
            3 => memchr::memchr3(short[0], short[1], short[2], rest)?,
            _ => rest.iter().position(|&b| starts[usize::from(b)])?,
        };
        offset += candidate;
        if haystack[offset].is_ascii() {
            return Some(offset);
        }
        let scalar = leading_scalar(&haystack[offset..])?;
        if scalar != previous_miss && scalar_in_set(scalar, set) {
            return Some(offset);
        }
        previous_miss = scalar;
        offset += scalar.len();
    }
    None
}

/// Probe a bounded prefix before preparing the full delimiter set. In valid UTF-8
/// a scalar cannot begin inside another scalar. A match in a byte prefix is
/// therefore conclusive, even when the prefix ends inside a later scalar.
#[inline]
fn probe_first_scalar<const N: usize>(text: &[u8], set: &[u8]) -> Option<i64> {
    let scalar = text.first_chunk::<N>()?;
    let prefix = &set[..set.len().min(12)];
    let hit = if N == 1 {
        prefix.contains(&scalar[0])
    } else {
        prefix.windows(N).any(|window| window == scalar)
    };
    if hit {
        return Some(0);
    }
    if text.len() != N {
        return None;
    }
    if prefix.len() == set.len() {
        return Some(-1);
    }
    // Recheck the N-1 possible starts that straddle the end of the probe.
    let rest = &set[prefix.len() - (N - 1)..];
    Some(if scalar_in_set(scalar, rest) { 0 } else { -1 })
}

/// First UTF-8 byte offset occupied by any scalar in the delimiter set, or -1.
/// Null, nonpositive and unrepresentable ranges are empty.
///
/// # Safety
/// Admitted positive ranges must describe live readable UTF-8 for this call.
/// The views may overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_find_any(
    hptr: *const u8,
    hlen: i64,
    sptr: *const u8,
    slen: i64,
) -> i64 {
    let haystack = unsafe { safe_slice(hptr, hlen) };
    let set = unsafe { safe_slice(sptr, slen) };
    if haystack.is_empty() || set.is_empty() {
        return -1;
    }
    let first = match haystack[0] {
        0..=0x7f => probe_first_scalar::<1>(haystack, set),
        0xc2..=0xdf => probe_first_scalar::<2>(haystack, set),
        0xe0..=0xef => probe_first_scalar::<3>(haystack, set),
        0xf0..=0xf4 => probe_first_scalar::<4>(haystack, set),
        _ => None,
    };
    if let Some(answer) = first {
        return answer;
    }
    let index = if set.len() > 12 {
        find_in_large_scalar_set(haystack, set)
    } else if set.is_ascii() {
        match set {
            [a] => memchr::memchr(*a, haystack),
            [a, b] => memchr::memchr2(*a, *b, haystack),
            [a, b, c] => memchr::memchr3(*a, *b, *c, haystack),
            _ => {
                let mut members = [false; 256];
                for &byte in set {
                    members[usize::from(byte)] = true;
                }
                haystack.iter().position(|&byte| members[usize::from(byte)])
            }
        }
    } else {
        let Ok(delimiters) = std::str::from_utf8(set) else {
            return -1;
        };
        let mut chars = delimiters.char_indices();
        let mut members = [&[][..]; 3];
        let mut count = 0;
        for (slot, (start, ch)) in members.iter_mut().zip(&mut chars) {
            *slot = &set[start..start + ch.len_utf8()];
            count += 1;
        }
        if chars.next().is_none() {
            find_small_scalar_set(haystack, &members[..count])
        } else {
            find_in_large_scalar_set(haystack, set)
        }
    };
    index.and_then(|at| i64::try_from(at).ok()).unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn str_find_any_checks_complete_scalars_across_the_delimiter_prefix() {
        for (target, neighbor) in [
            ('\0', '!'),
            (':', ';'),
            ('é', 'ê'),
            ('界', '京'),
            ('🚀', '🚁'),
        ] {
            for padding in 0..=17 {
                for member in [target, neighbor] {
                    let set = format!("{}{member}{neighbor}", "x".repeat(padding));
                    for haystack in [
                        target.to_string(),
                        format!("{target}rest"),
                        format!("a{target}"),
                    ] {
                        let expected = haystack
                            .char_indices()
                            .find(|(_, c)| set.contains(*c))
                            .map_or(-1, |(at, _)| i64::try_from(at).unwrap());
                        let got = unsafe {
                            align_rt_str_find_any(
                                haystack.as_ptr(),
                                haystack.len() as i64,
                                set.as_ptr(),
                                set.len() as i64,
                            )
                        };
                        assert_eq!(got, expected, "haystack={haystack:?}, set={set:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn str_find_any_large_sets_confirm_leads_and_repeated_misses() {
        for alphabet in ["é", "é界", "é界🚀", "!é界🚀", ":é界🚀\0", "abcde", "界京"]
        {
            let set = alphabet.repeat(16);
            for text in [
                "x",
                "yyyyyyyyyyyyyy",
                "êêê",
                "êêêé",
                "🚁🚁🚀",
                "xあ😄界",
                "éê",
                "x\0",
                "xb",
                "x!",
                "x:",
                "ø京",
                "ы界",
                "x🚀",
            ] {
                for text in [text.to_owned(), format!("yyyyyyyyyyyyyyyyy{text}")] {
                    let expected = text
                        .char_indices()
                        .find(|(_, c)| set.contains(*c))
                        .map_or(-1, |(at, _)| i64::try_from(at).unwrap());
                    let got = unsafe {
                        align_rt_str_find_any(
                            text.as_ptr(),
                            text.len() as i64,
                            set.as_ptr(),
                            set.len() as i64,
                        )
                    };
                    assert_eq!(got, expected, "haystack={text:?}, set={set:?}");
                }
            }
        }
    }
}
