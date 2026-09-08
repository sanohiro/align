//! Allocation-free UTC calendar engine for the exact named wire-format ledger.
use crate::{AL_INVALID, AlignStr, owned_str_exact};
use core::fmt::Write;

const SECOND: i128 = 1_000_000_000;
const DAY: i128 = 86_400 * SECOND;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

fn resolution(kind: i32) -> Option<i128> {
    match kind {
        0 => Some(1),
        1 => Some(1_000_000),
        2 | 3 => Some(SECOND),
        4 => Some(DAY),
        _ => None,
    }
}

fn year_start(year: i64) -> i64 {
    let previous = year - 1;
    previous * 365 + previous.div_euclid(4) - previous.div_euclid(100) + previous.div_euclid(400)
        - 719_162
}

fn month_days(year: i64, month: i64) -> Option<i64> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 => Some(if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            29
        } else {
            28
        }),
        _ => None,
    }
}

fn civil_days(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=month_days(year, month)?).contains(&day) {
        return None;
    }
    let mut days = year_start(year) + day - 1;
    for m in 1..month {
        days += month_days(year, m)?;
    }
    Some(days)
}

fn civil_date(days: i64) -> Option<(i64, i64, i64)> {
    let (mut lo, mut hi) = (0, 10_000);
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if year_start(mid) <= days {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let mut day = days - year_start(lo);
    for month in 1..=12 {
        let count = month_days(lo, month)?;
        if day < count {
            return Some((lo, month, day + 1));
        }
        day -= count;
    }
    None
}

struct Text {
    bytes: [u8; 32],
    len: usize,
}
impl Write for Text {
    fn write_str(&mut self, value: &str) -> core::fmt::Result {
        let end = self.len.checked_add(value.len()).ok_or(core::fmt::Error)?;
        self.bytes
            .get_mut(self.len..end)
            .ok_or(core::fmt::Error)?
            .copy_from_slice(value.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn format(ns: i64, kind: i32) -> Option<Text> {
    let precision = resolution(kind)?;
    let rounded = i128::from(ns).div_euclid(precision) * precision;
    i64::try_from(rounded).ok()?;
    let days = i64::try_from(rounded.div_euclid(DAY)).ok()?;
    let (year, month, day) = civil_date(days)?;
    let within = rounded.rem_euclid(DAY);
    let hour = within / (3600 * SECOND);
    let minute = (within / (60 * SECOND)) % 60;
    let second = (within / SECOND) % 60;
    let fraction = within % SECOND;
    let mut out = Text {
        bytes: [0; 32],
        len: 0,
    };
    match kind {
        0 | 1 => {
            write!(
                out,
                "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}"
            )
            .ok()?;
            if kind == 1 {
                write!(out, ".{:03}", fraction / 1_000_000).ok()?;
            } else if fraction != 0 {
                let start = out.len;
                write!(out, ".{fraction:09}").ok()?;
                while out.len > start && out.bytes.get(out.len - 1) == Some(&b'0') {
                    out.len -= 1;
                }
            }
            out.write_str("Z").ok()?;
        }
        2 => {
            let weekday = WEEKDAYS.get(usize::try_from((days + 4).rem_euclid(7)).ok()?)?;
            let month_name = MONTHS.get(usize::try_from(month - 1).ok()?)?;
            write!(
                out,
                "{weekday}, {day:02} {month_name} {year:04} {hour:02}:{minute:02}:{second:02} GMT"
            )
            .ok()?;
        }
        3 => write!(
            out,
            "{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z"
        )
        .ok()?,
        4 => write!(out, "{year:04}{month:02}{day:02}").ok()?,
        _ => return None,
    }
    Some(out)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl Parser<'_> {
    fn digits(&mut self, width: usize) -> Option<i64> {
        let end = self.pos.checked_add(width)?;
        let mut value = 0;
        for &b in self.bytes.get(self.pos..end)? {
            if !b.is_ascii_digit() {
                return None;
            }
            value = value * 10 + i64::from(b - b'0');
        }
        self.pos = end;
        Some(value)
    }
    fn literal(&mut self, expected: &[u8]) -> Option<()> {
        let end = self.pos.checked_add(expected.len())?;
        if self.bytes.get(self.pos..end)? != expected {
            return None;
        }
        self.pos = end;
        Some(())
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
}

fn parse(bytes: &[u8], kind: i32) -> Option<i64> {
    resolution(kind)?;
    if bytes.len() > 35 || !bytes.is_ascii() {
        return None;
    }
    let mut p = Parser { bytes, pos: 0 };
    let (year, month, day);
    let mut weekday = None;
    if kind == 2 {
        let name = bytes.get(..3)?;
        weekday = Some(WEEKDAYS.iter().position(|s| s.as_bytes() == name)?);
        p.pos = 3;
        p.literal(b", ")?;
        day = p.digits(2)?;
        p.literal(b" ")?;
        let name = bytes.get(p.pos..p.pos + 3)?;
        month = i64::try_from(MONTHS.iter().position(|s| s.as_bytes() == name)?).ok()? + 1;
        p.pos += 3;
        p.literal(b" ")?;
        year = p.digits(4)?;
        p.literal(b" ")?;
    } else {
        year = p.digits(4)?;
        if kind < 2 {
            p.literal(b"-")?;
        }
        month = p.digits(2)?;
        if kind < 2 {
            p.literal(b"-")?;
        }
        day = p.digits(2)?;
        if kind != 4 {
            if kind < 2 && p.peek() == Some(b't') {
                p.literal(b"t")?;
            } else {
                p.literal(b"T")?;
            }
        }
    }
    let days = civil_days(year, month, day)?;
    if weekday.is_some_and(|w| i64::try_from(w).ok() != Some((days + 4).rem_euclid(7))) {
        return None;
    }
    let mut value = i128::from(days) * DAY;
    if kind != 4 {
        let hour = p.digits(2)?;
        if kind < 3 {
            p.literal(b":")?;
        }
        let minute = p.digits(2)?;
        if kind < 3 {
            p.literal(b":")?;
        }
        let second = p.digits(2)?;
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        value += i128::from(hour * 3600 + minute * 60 + second) * SECOND;
        if kind < 2 {
            let mut digits = 0;
            if p.peek() == Some(b'.') {
                p.literal(b".")?;
                let mut fraction = 0;
                while p.peek().is_some_and(|b| b.is_ascii_digit()) {
                    if digits == 9 {
                        return None;
                    }
                    fraction = fraction * 10 + p.digits(1)?;
                    digits += 1;
                }
                if digits == 0 {
                    return None;
                }
                for _ in digits..9 {
                    fraction *= 10;
                }
                value += i128::from(fraction);
            }
            if kind == 1 && digits != 3 {
                return None;
            }
            match p.peek()? {
                b'Z' | b'z' => p.pos += 1,
                sign @ (b'+' | b'-') => {
                    p.pos += 1;
                    let hour = p.digits(2)?;
                    p.literal(b":")?;
                    let minute = p.digits(2)?;
                    if hour > 23 || minute > 59 || (sign == b'-' && hour == 0 && minute == 0) {
                        return None;
                    }
                    let offset = i128::from(hour * 3600 + minute * 60) * SECOND;
                    value -= if sign == b'+' { offset } else { -offset };
                }
                _ => return None,
            }
        } else if kind == 2 {
            p.literal(b" GMT")?;
        } else {
            p.literal(b"Z")?;
        }
    }
    if p.pos != bytes.len() {
        return None;
    }
    i64::try_from(value).ok()
}

fn output_range<T>(out: *mut T) -> Option<(usize, usize)> {
    let start = out.addr();
    if out.is_null() || !start.is_multiple_of(core::mem::align_of::<T>()) {
        return None;
    }
    Some((start, start.checked_add(core::mem::size_of::<T>())?))
}

/// Format one signed-nanosecond instant. Invalid metadata/precision returns EINVAL.
///
/// # Safety
/// A non-null aligned non-wrapping `out` must name writable AlignStr storage.
/// No live owned string may be overwritten. Output is zeroed before semantic rejection.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_time_format(out: *mut AlignStr, ns: i64, kind: i32) -> i32 {
    if output_range(out).is_none() {
        return AL_INVALID;
    }
    unsafe {
        out.write(AlignStr {
            ptr: core::ptr::null(),
            len: 0,
        });
    }
    let Some(text) = format(ns, kind) else {
        return AL_INVALID;
    };
    let owned = unsafe {
        owned_str_exact(text.len, |dst| {
            for (target, &byte) in dst.iter_mut().zip(&text.bytes[..text.len]) {
                target.write(byte);
            }
        })
    };
    unsafe {
        out.write(owned);
    }
    0
}

/// Parse a complete bounded wire timestamp. Invalid input returns EINVAL and zero output.
///
/// # Safety
/// A non-null aligned non-wrapping `out` names writable i64 storage. A structurally valid
/// disjoint nonempty `input` span must name initialized readable bytes. Overlap is rejected
/// after clearing output, before reading input; raw pointers are not converted to references
/// until this preflight completes. No input reference may forbid writing the output span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_time_parse(
    out: *mut i64,
    input: *const u8,
    len: i64,
    kind: i32,
) -> i32 {
    let Some(out_range) = output_range(out) else {
        return AL_INVALID;
    };
    unsafe {
        out.write(0);
    }
    if resolution(kind).is_none() {
        return AL_INVALID;
    }
    let Ok(len) = usize::try_from(len) else {
        return AL_INVALID;
    };
    if len == 0 || len > 35 || input.is_null() {
        return AL_INVALID;
    }
    let Some(end) = input.addr().checked_add(len) else {
        return AL_INVALID;
    };
    if input.addr() < out_range.1 && out_range.0 < end {
        return AL_INVALID;
    }
    let bytes = unsafe { core::slice::from_raw_parts(input, len) };
    let Some(value) = parse(bytes, kind) else {
        return AL_INVALID;
    };
    unsafe {
        out.write(value);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_golden_vectors_and_precision_endpoints() {
        let cases = [
            (0, "1970-01-01T00:00:00Z"),
            (-1, "1969-12-31T23:59:59.999999999Z"),
            (951_782_400_123_456_789, "2000-02-29T00:00:00.123456789Z"),
            (i64::MIN, "1677-09-21T00:12:43.145224192Z"),
            (i64::MAX, "2262-04-11T23:47:16.854775807Z"),
        ];
        for (ns, expected) in cases {
            let text = format(ns, 0).unwrap();
            assert_eq!(&text.bytes[..text.len], expected.as_bytes());
            assert_eq!(parse(expected.as_bytes(), 0), Some(ns));
        }
        for kind in 0..5 {
            let precision = resolution(kind).unwrap();
            let first = i128::from(i64::MIN).div_euclid(precision) * precision + precision;
            let first = i64::try_from(first).unwrap();
            for ns in [i64::MIN, first - 1, first, -1, 0, 1, i64::MAX] {
                let expected = i64::try_from(i128::from(ns).div_euclid(precision) * precision).ok();
                let text = format(ns, kind);
                assert_eq!(
                    text.as_ref().map(|t| parse(&t.bytes[..t.len], kind)),
                    expected.map(Some),
                    "{kind} {ns}"
                );
            }
        }
    }

    #[test]
    fn parser_grammar_and_calendar_matrix() {
        for (input, kind, expected) in [
            ("1970-01-01t01:00:00+01:00", 0, 0),
            ("1969-12-31T23:00:00-01:00", 0, 0),
            ("1970-01-01T00:00:00.000z", 1, 0),
            ("Thu, 01 Jan 1970 00:00:00 GMT", 2, 0),
            ("19700101T000000Z", 3, 0),
            ("19700101", 4, 0),
            ("1677-09-21T01:12:43.145224192+01:00", 0, i64::MIN),
            ("2262-04-12T00:47:16.854775807+01:00", 0, i64::MAX),
        ] {
            assert_eq!(parse(input.as_bytes(), kind), Some(expected), "{input}");
            for index in 0..input.len() {
                let mut changed = input.as_bytes().to_vec();
                changed[index] = 0;
                assert_eq!(parse(&changed, kind), None, "NUL {kind} {index}");
                changed[index] = 255;
                assert_eq!(parse(&changed, kind), None, "nonASCII {kind} {index}");
            }
            assert_eq!(parse(format!("{input} ").as_bytes(), kind), None);
        }
        for input in [
            "1900-02-29T00:00:00Z",
            "2100-02-29T00:00:00Z",
            "2000-02-30T00:00:00Z",
            "2000-00-01T00:00:00Z",
            "2000-13-01T00:00:00Z",
            "2000-01-00T00:00:00Z",
            "2000-01-01T24:00:00Z",
            "2000-01-01T00:60:00Z",
            "2000-01-01T00:00:60Z",
            "1970-01-01T00:00:00-00:00",
            "1970-01-01T00:00:00+24:00",
            "1970-01-01T00:00:00+00:60",
            "1970-01-01T00:00:00.Z",
            "1970-01-01T00:00:00.0000000000Z",
            "1970-1-01T00:00:00Z",
            "1677-09-21T00:12:43.145224191Z",
            "2262-04-11T23:47:16.854775808Z",
        ] {
            assert_eq!(parse(input.as_bytes(), 0), None, "{input}");
        }
        assert!(parse(b"2000-02-29T00:00:00Z", 0).is_some());
        assert_eq!(parse(b"Fri, 01 Jan 1970 00:00:00 GMT", 2), None);
        for input in [
            "1970-01-01T00:00:00Z",
            "1970-01-01T00:00:00.00Z",
            "1970-01-01T00:00:00.0000Z",
        ] {
            assert_eq!(parse(input.as_bytes(), 1), None);
        }
    }

    #[test]
    fn grammar_widths_and_fraction_lengths_are_closed() {
        for kind in 0..5 {
            let text = format(951_782_400_123_456_789, kind).unwrap();
            let bytes = &text.bytes[..text.len];
            for cut in 0..bytes.len() {
                assert_eq!(
                    parse(&bytes[..cut], kind),
                    None,
                    "truncated kind {kind} at {cut}"
                );
            }
            let mut extended = bytes.to_vec();
            extended.push(b'Z');
            assert_eq!(parse(&extended, kind), None);
        }
        for width in 0..=10 {
            let text = format!("1970-01-01T00:00:00.{}Z", "1".repeat(width));
            assert_eq!(
                parse(text.as_bytes(), 0).is_some(),
                (1..=9).contains(&width)
            );
            assert_eq!(parse(text.as_bytes(), 1).is_some(), width == 3);
        }
        for (kind, expected) in [
            (0, "2000-02-29T00:00:00.123456789Z"),
            (1, "2000-02-29T00:00:00.123Z"),
            (2, "Tue, 29 Feb 2000 00:00:00 GMT"),
            (3, "20000229T000000Z"),
            (4, "20000229"),
        ] {
            let text = format(951_782_400_123_456_789, kind).unwrap();
            assert_eq!(&text.bytes[..text.len], expected.as_bytes());
        }
    }

    #[test]
    fn percent_path_all_bytes_match_independent_oracle() {
        let data: Vec<u8> = (0..=255).collect();
        let expected: String = data
            .iter()
            .map(|&byte| {
                if b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~/"
                    .contains(&byte)
                {
                    char::from(byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect();
        unsafe {
            let text = crate::align_rt_percent_encode_path(data.as_ptr(), 256);
            assert_eq!(
                core::slice::from_raw_parts(text.ptr, usize::try_from(text.len).unwrap()),
                expected.as_bytes()
            );
            crate::align_rt_free(text.ptr.cast_mut());
        }
    }

    #[test]
    fn ffi_preflight_and_zero_on_failure() {
        unsafe {
            let mut out = 99_i64;
            for kind in [-1, 5, i32::MAX] {
                assert_eq!(
                    align_rt_time_parse(&mut out, b"19700101".as_ptr(), 8, kind),
                    AL_INVALID
                );
                assert_eq!(out, 0);
            }
            for len in [-1, 0, 36, i64::MAX] {
                out = 99;
                assert_eq!(
                    align_rt_time_parse(&mut out, core::ptr::null(), len, 4),
                    AL_INVALID
                );
                assert_eq!(out, 0);
            }
            let ptr = &mut out as *mut i64;
            assert_eq!(align_rt_time_parse(ptr, ptr.cast(), 8, 4), AL_INVALID);
            assert_eq!(out, 0);
            assert_eq!(
                align_rt_time_parse(core::ptr::null_mut(), core::ptr::null(), 0, 0),
                AL_INVALID
            );
            assert_eq!(
                align_rt_time_parse(
                    core::ptr::without_provenance_mut(1),
                    core::ptr::null(),
                    0,
                    0
                ),
                AL_INVALID
            );
            assert_eq!(
                align_rt_time_parse(
                    &mut out,
                    core::ptr::without_provenance(usize::MAX - 3),
                    8,
                    4
                ),
                AL_INVALID
            );
            let mut text = AlignStr {
                ptr: core::ptr::without_provenance(1),
                len: 1,
            };
            for kind in [-1, 1, 2, 3, 4, 5] {
                assert_eq!(align_rt_time_format(&mut text, i64::MIN, kind), AL_INVALID);
                assert!(text.ptr.is_null());
                assert_eq!(text.len, 0);
            }
            assert_eq!(
                align_rt_time_format(core::ptr::null_mut(), 0, 0),
                AL_INVALID
            );
            assert_eq!(
                align_rt_time_format(core::ptr::without_provenance_mut(1), 0, 0),
                AL_INVALID
            );
            assert_eq!(align_rt_time_format(&mut text, 0, 0), 0);
            assert_eq!(
                core::slice::from_raw_parts(text.ptr, usize::try_from(text.len).unwrap()),
                b"1970-01-01T00:00:00Z"
            );
            crate::align_rt_free(text.ptr.cast_mut());
        }
    }
}
