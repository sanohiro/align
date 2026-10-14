use super::*;
use std::collections::HashSet;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn check(input: &str, expected: &str) -> TestResult {
    let output = normalize(input).map_err(|code| format!("NFC refused {input:?}: {code}"))?;
    let _owner = Allocation(output.ptr.cast_mut());
    let len = usize::try_from(output.len)?;
    let actual = if len == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(output.ptr, len) }
    };
    assert_eq!(actual, expected.as_bytes(), "input {input:?}");
    if !input.is_empty() {
        assert_ne!(output.ptr, input.as_ptr());
    }
    Ok(())
}

fn text(field: &str) -> Result<String, Box<dyn std::error::Error>> {
    field
        .split_whitespace()
        .map(|value| {
            char::from_u32(u32::from_str_radix(value, 16)?)
                .ok_or_else(|| "non-scalar fixture".into())
        })
        .collect()
}

#[test]
fn unicode_nfc_conformance() -> TestResult {
    let data = include_str!("../../data/unicode-16.0.0/NormalizationTest.txt");
    let mut part = "";
    let mut part1 = HashSet::new();
    let mut rows = 0;
    for line in data.lines() {
        let line = line.split('#').next().ok_or("missing fixture line")?.trim();
        if line.starts_with('@') {
            part = line;
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let columns = line
            .split(';')
            .take(5)
            .map(text)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(columns.len(), 5);
        for (column, expected) in [(0, 1), (1, 1), (2, 1), (3, 3), (4, 3)] {
            check(&columns[column], &columns[expected])?;
        }
        if part == "@Part1" {
            assert_eq!(columns[0].chars().count(), 1);
            part1.insert(columns[0].chars().next().ok_or("empty Part1")?);
        }
        rows += 1;
    }
    assert_eq!(rows, 19965);
    assert!(!part1.is_empty());
    let mut unchanged = 0;
    for code in 0..=0x10ffff {
        if let Some(value) = char::from_u32(code)
            && !part1.contains(&value)
        {
            let mut bytes = [0u8; 4];
            let input = value.encode_utf8(&mut bytes);
            check(input, input)?;
            unchanged += 1;
        }
    }
    assert_eq!(unchanged + part1.len(), 0x110000 - 0x800);
    Ok(())
}

#[test]
fn unicode_nfc_data_identity() {
    assert_eq!(crate::unicode_data::VERSION, "16.0.0");
    assert!(
        include_str!("../../../../apps/unicode/pkg/unicode.align")
            .contains("pub fn version() -> str = \"16.0.0\"")
    );
    assert_eq!(
        (CCC.len(), DECOMPOSITION.len(), COMPOSITION.len()),
        (934, 2081, 961)
    );
    assert!(CCC.windows(2).all(|w| w[0].0 < w[1].0));
    assert!(CCC.iter().all(|entry| entry.1 > 0));
    assert!(DECOMPOSITION.windows(2).all(|w| w[0].0 < w[1].0));
    assert!(
        DECOMPOSITION
            .iter()
            .all(|entry| !entry.1.is_empty() && entry.1.len() <= 4)
    );
    assert!(
        COMPOSITION
            .windows(2)
            .all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1))
    );
    assert_eq!(extent(usize::MAX, 2), None);
    assert_eq!(extent(usize::MAX / 2 + 1, 1), None);
    assert_eq!(extent(usize::MAX / 2, 1), Some(usize::MAX / 2));
    assert_eq!(extent(0, mem::size_of::<Unit>()), Some(0));
}

#[test]
fn unicode_nfc_long_and_boundary_text() -> TestResult {
    for (input, expected) in [
        ("", ""),
        ("abc\0\u{10ffff}\u{0378}", "abc\0\u{10ffff}\u{0378}"),
        ("か\u{3099}くせい", "がくせい"),
        ("e\u{0301}", "é"),
        ("A\u{030a}", "Å"),
        ("각", "각"),
        ("Å", "Å"),
        ("\u{0344}", "\u{0308}\u{0301}"),
        ("a\u{0305}\u{0301}", "a\u{0305}\u{0301}"),
    ] {
        check(input, expected)?;
    }
    let plain = "x".repeat(10000);
    check(&plain, &plain)?;
    let input = format!("x{}{}", "\u{0315}".repeat(9000), "\u{0300}".repeat(9000));
    let expected = format!("x{}{}", "\u{0300}".repeat(9000), "\u{0315}".repeat(9000));
    check(&input, &expected)?;
    Ok(())
}

#[test]
fn unicode_nfc_native_admission() -> TestResult {
    let mut output = AlignStr {
        ptr: ptr::dangling(),
        len: 99,
    };
    let out = &mut output as *mut AlignStr;
    unsafe {
        assert_eq!(
            align_rt_unicode_nfc(ptr::null_mut(), ptr::null(), -1),
            AL_INVALID
        );
        assert_eq!(
            align_rt_unicode_nfc(1usize as *mut AlignStr, ptr::null(), -1),
            AL_INVALID
        );
        let wrapping = usize::MAX & !(mem::align_of::<AlignStr>() - 1);
        assert_eq!(
            align_rt_unicode_nfc(wrapping as *mut AlignStr, ptr::null(), -1),
            AL_INVALID
        );
        for (input, length) in [
            (ptr::null(), -1),
            (ptr::null(), 1),
            (usize::MAX as *const u8, i64::MAX),
            (usize::MAX as *const u8, 1),
            (out.cast(), 1),
        ] {
            output.ptr = ptr::dangling();
            output.len = 99;
            assert_eq!(align_rt_unicode_nfc(out, input, length), AL_INVALID);
            assert!(output.ptr.is_null());
            assert_eq!(output.len, 0);
        }
        for bytes in [
            &b"\xff"[..],
            &b"\xc0\x80"[..],
            &b"\xed\xa0\x80"[..],
            &b"\xf4\x90\x80\x80"[..],
        ] {
            assert_eq!(
                align_rt_unicode_nfc(out, bytes.as_ptr(), i64::try_from(bytes.len())?),
                AL_INVALID
            );
            assert!(output.ptr.is_null());
            assert_eq!(output.len, 0);
        }
        assert_eq!(align_rt_unicode_nfc(out, ptr::null(), 0), 0);
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);
        assert_eq!(align_rt_unicode_nfc(out, "e\u{301}".as_ptr(), 3), 0);
        let _guard = Allocation(output.ptr.cast_mut());
        assert_eq!(
            slice::from_raw_parts(output.ptr, usize::try_from(output.len)?),
            "é".as_bytes()
        );
    }
    Ok(())
}

#[cfg(feature = "alloc-count")]
#[test]
fn unicode_nfc_allocation_lifecycle() -> TestResult {
    use crate::{
        OWNED_ALLOC_FAIL_AFTER, align_rt_alloc_count, align_rt_free_count,
        align_rt_requested_live_bytes, align_rt_requested_live_reset,
    };
    use core::sync::atomic::Ordering::Relaxed;
    let Some(_completion) = crate::allocation_test::enter() else {
        return Ok(());
    };
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            OWNED_ALLOC_FAIL_AFTER.store(-1, Relaxed);
        }
    }
    let _reset = Reset;
    for failure in [0, 1, -1] {
        align_rt_requested_live_reset();
        let (alloc, free) = (align_rt_alloc_count(), align_rt_free_count());
        OWNED_ALLOC_FAIL_AFTER.store(failure, Relaxed);
        let input = *b"e\xcc\x81";
        let mut output = AlignStr {
            ptr: ptr::dangling(),
            len: 99,
        };
        let result = unsafe { align_rt_unicode_nfc(&mut output, input.as_ptr(), 3) };
        OWNED_ALLOC_FAIL_AFTER.store(-1, Relaxed);
        assert_eq!(input, *b"e\xcc\x81");
        if failure >= 0 {
            assert_eq!(result, AL_CODE + libc::ENOMEM);
            assert!(output.ptr.is_null());
            assert_eq!(output.len, 0);
            assert_eq!(align_rt_alloc_count() - alloc, failure);
            assert_eq!(align_rt_free_count() - free, failure);
        } else {
            assert_eq!(result, 0);
            assert_eq!(align_rt_requested_live_bytes(), 2);
            assert_eq!(align_rt_alloc_count() - alloc, 2);
            assert_eq!(align_rt_free_count() - free, 1);
            unsafe { align_rt_free(output.ptr.cast_mut()) };
            assert_eq!(align_rt_free_count() - free, 2);
        }
        assert_eq!(align_rt_requested_live_bytes(), 0);
    }
    for bytes in [&b""[..], &b"\xff"[..]] {
        OWNED_ALLOC_FAIL_AFTER.store(0, Relaxed);
        let mut output = AlignStr {
            ptr: ptr::dangling(),
            len: 99,
        };
        let result = unsafe {
            align_rt_unicode_nfc(&mut output, bytes.as_ptr(), i64::try_from(bytes.len())?)
        };
        assert_eq!(result, if bytes.is_empty() { 0 } else { AL_INVALID });
        assert_eq!(OWNED_ALLOC_FAIL_AFTER.load(Relaxed), 0);
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);
    }
    Ok(())
}
