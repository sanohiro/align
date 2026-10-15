//! Plan183: direct ABI ownership, exact tags and complete-root validation.
use super::*;

#[cfg(feature = "alloc-count")]
#[test]
fn json_string_root_grammar_and_cleanup() -> Result<(), Box<dyn std::error::Error>> {
    let Some(_isolated) = crate::allocation_test::enter() else {
        return Ok(());
    };
    align_rt_requested_live_reset();
    for (source, expected) in [
        (br#""""#.as_slice(), b"".as_slice()),
        (b" \n\"plain\"\t", b"plain"),
        ("\"が\"".as_bytes(), "が".as_bytes()),
        (br#""\"\\\/\b\f\n\r\t""#, b"\"\\/\x08\x0c\n\r\t"),
        (br#""\u0000""#, b"\0"),
        (br#""\uD83D\uDE00""#, "😀".as_bytes()),
        (br#""\u304c""#, "が".as_bytes()),
    ] {
        let mut input = source.to_vec();
        let mut out = AlignStr {
            ptr: core::ptr::null(),
            len: 0,
        };
        let (a, f) = (align_rt_alloc_count(), align_rt_free_count());
        assert_eq!(
            unsafe {
                align_rt_json_decode_scalar(
                    input.as_ptr(),
                    i64::try_from(input.len())?,
                    0x810,
                    (&mut out as *mut AlignStr).cast(),
                )
            },
            0
        );
        input.fill(b'x');
        drop(input);
        assert_eq!(unsafe { safe_slice(out.ptr, out.len) }, expected);
        assert_eq!(out.ptr.is_null(), expected.is_empty());
        assert_eq!(align_rt_alloc_count() - a, i64::from(!expected.is_empty()));
        assert_eq!(
            align_rt_requested_live_bytes(),
            i64::try_from(expected.len())?
        );
        unsafe { align_rt_free(out.ptr.cast_mut()) };
        assert_eq!(align_rt_free_count() - f, i64::from(!expected.is_empty()));
        assert_eq!(align_rt_requested_live_bytes(), 0);
    }
    for (source, allocations) in [
        (b"".as_slice(), 0),
        (b"null", 0),
        (b"true", 0),
        (b"false", 0),
        (b"1", 0),
        (b"1.5", 0),
        (b"[]", 0),
        (b"{}", 0),
        (br#""unterminated"#, 0),
        (br#""\x""#, 0),
        (br#""\u12""#, 0),
        (br#""\ud800""#, 0),
        (br#""\udc00""#, 0),
        (br#""\udc00\ud800""#, 0),
        (br#""\ud800\u0041""#, 0),
        (b"\"raw\n\"", 0),
        (b"\"\0\"", 0),
        (b"\"\xff\"", 0),
        (b"\"a\"\xff", 0),
        (br#""a" x"#, 1),
        (br#""a""b""#, 1),
        (br#""a"[]"#, 1),
        (br#""\u304c" {}"#, 1),
        (br#""" x"#, 0),
        (b"\x0b\"a\"", 0),
        (b"\"a\"\x0b", 1),
    ] {
        let marker = b"sentinel";
        let mut out = AlignStr {
            ptr: marker.as_ptr(),
            len: 8,
        };
        let (a, f) = (align_rt_alloc_count(), align_rt_free_count());
        assert_eq!(
            unsafe {
                align_rt_json_decode_scalar(
                    source.as_ptr(),
                    i64::try_from(source.len())?,
                    0x810,
                    (&mut out as *mut AlignStr).cast(),
                )
            },
            1,
            "{source:?}"
        );
        assert_eq!((out.ptr, out.len), (marker.as_ptr(), 8), "{source:?}");
        assert_eq!(align_rt_alloc_count() - a, allocations, "{source:?}");
        assert_eq!(align_rt_free_count() - f, allocations, "{source:?}");
        assert_eq!(align_rt_requested_live_bytes(), 0, "{source:?}");
    }
    for tag in [
        -1,
        0,
        3,
        0x108,
        0x201,
        0x310,
        0x800,
        0x808,
        0x910,
        0x10810,
        0x100101,
        i32::MAX,
    ] {
        for source in [b"".as_slice(), b"1", br#""a""#] {
            let mut out = [0xfeed_u64; 2];
            let before = align_rt_alloc_count();
            assert_eq!(
                unsafe {
                    align_rt_json_decode_scalar(
                        source.as_ptr(),
                        i64::try_from(source.len())?,
                        tag,
                        out.as_mut_ptr().cast(),
                    )
                },
                1
            );
            assert_eq!(out, [0xfeed; 2]);
            assert_eq!(align_rt_alloc_count(), before);
        }
    }
    for tag in [
        1, 2, 4, 8, 0x10001, 0x10002, 0x10004, 0x10008, 0x101, 0x204, 0x208,
    ] {
        let input = if tag == 0x101 {
            b"true".as_slice()
        } else {
            b"1"
        };
        let mut out = [0_u64; 2];
        let before = align_rt_alloc_count();
        assert_eq!(
            unsafe {
                align_rt_json_decode_scalar(
                    input.as_ptr(),
                    i64::try_from(input.len())?,
                    tag,
                    out.as_mut_ptr().cast(),
                )
            },
            0
        );
        assert_ne!(out[0], 0);
        assert_eq!(align_rt_alloc_count(), before);
    }
    Ok(())
}

#[cfg(feature = "alloc-count")]
#[test]
fn json_string_array_root_grammar_and_cleanup() -> Result<(), Box<dyn std::error::Error>> {
    let Some(_isolated) = crate::allocation_test::enter() else {
        return Ok(());
    };
    align_rt_requested_live_reset();
    let success: &[(&[u8], &[&[u8]])] = &[
        (b"[]", &[]),
        (b" \n[\"\"]\t", &[b""]),
        (
            "[\"が\",\"voice\",\"が\"]".as_bytes(),
            &["が".as_bytes(), b"voice", "が".as_bytes()],
        ),
        (
            br#"["\"\\\/\b\f\n\r\t","\u0000","\uD83D\uDE00","\u304c"]"#,
            &[
                b"\"\\/\x08\x0c\n\r\t",
                b"\0",
                "😀".as_bytes(),
                "が".as_bytes(),
            ],
        ),
    ];
    for (source, expected) in success {
        let mut input = source.to_vec();
        let mut output = AlignStr {
            ptr: core::ptr::null(),
            len: 0,
        };
        let (a, f) = (align_rt_alloc_count(), align_rt_free_count());
        assert_eq!(
            unsafe {
                align_rt_json_decode_array(
                    input.as_ptr(),
                    i64::try_from(input.len())?,
                    0x810,
                    &mut output,
                )
            },
            0
        );
        input.fill(b'x');
        drop(input);
        assert_eq!(output.len, i64::try_from(expected.len())?);
        let allocation_count = i64::from(!expected.is_empty())
            + i64::try_from(expected.iter().filter(|v| !v.is_empty()).count())?;
        assert_eq!(align_rt_alloc_count() - a, allocation_count);
        let headers = unsafe { safe_slice(output.ptr.cast::<AlignStr>(), output.len) };
        for (header, bytes) in headers.iter().zip(*expected) {
            assert_eq!(unsafe { safe_slice(header.ptr, header.len) }, *bytes);
        }
        // Independent identical values must still have separate owned payloads.
        if headers.len() == 3 {
            assert_ne!(headers[0].ptr, headers[2].ptr);
        }
        for header in headers {
            unsafe { align_rt_free(header.ptr.cast_mut()) };
        }
        unsafe { align_rt_free(output.ptr.cast_mut()) };
        assert_eq!(align_rt_free_count() - f, allocation_count);
        assert_eq!(align_rt_requested_live_bytes(), 0);
    }
    let failures: &[(&[u8], i64)] = &[
        (b"", 0),
        (b"null", 0),
        (b"{}", 0),
        (b"[", 0),
        (br#"[7]"#, 0),
        (br#"["a",7]"#, 1),
        (br#"["a","b",null]"#, 2),
        (br#"["a",[]]"#, 1),
        (br#"["a",{}]"#, 1),
        (br#"["a" "b"]"#, 1),
        (br#"["a",]"#, 1),
        (br#"["a",,"b"]"#, 1),
        (br#"["a""#, 1),
        (br#"["a"] x"#, 1),
        (br#"["a"][]"#, 1),
        (br#"["a","\x"]"#, 1),
        (br#"["a","\u12"]"#, 1),
        (br#"["a","\ud800"]"#, 1),
        (br#"["a","\udc00"]"#, 1),
        (br#"["a","\udc00\ud800"]"#, 1),
        (br#"["a","\ud800\u0041"]"#, 1),
        (b"[\"a\",\"raw\n\"]", 1),
        (b"[\"a\",\"\0\"]", 1),
        (b"[\"a\",\"\xff\"]", 0),
        (b"[\"a\"]\xff", 0),
        (br#"["a","unterminated"#, 1),
        (b"\x0b[]", 0),
    ];
    for (source, allocations) in failures {
        // An unchanged nonzero sentinel distinguishes no publication from zeroing on failure.
        let marker = b"sentinel";
        let mut output = AlignStr {
            ptr: marker.as_ptr(),
            len: 8,
        };
        let (a, f) = (align_rt_alloc_count(), align_rt_free_count());
        assert_eq!(
            unsafe {
                align_rt_json_decode_array(
                    source.as_ptr(),
                    i64::try_from(source.len())?,
                    0x810,
                    &mut output,
                )
            },
            1,
            "{source:?}"
        );
        assert_eq!((output.ptr, output.len), (marker.as_ptr(), 8));
        assert_eq!(align_rt_alloc_count() - a, *allocations, "{source:?}");
        assert_eq!(align_rt_free_count() - f, *allocations, "{source:?}");
        assert_eq!(align_rt_requested_live_bytes(), 0);
    }
    // Full private tag validation cannot be skipped by an empty container.
    for tag in [
        -1,
        0,
        3,
        0x108,
        0x201,
        0x310,
        0x800,
        0x808,
        0x910,
        0x10810,
        0x100101,
        i32::MAX,
    ] {
        for source in [b"[]".as_slice(), br#"["a"]"#] {
            let mut output = AlignStr {
                ptr: core::ptr::null(),
                len: 0,
            };
            let before = align_rt_alloc_count();
            assert_eq!(
                unsafe {
                    align_rt_json_decode_array(
                        source.as_ptr(),
                        i64::try_from(source.len())?,
                        tag,
                        &mut output,
                    )
                },
                1
            );
            assert!(output.ptr.is_null() && output.len == 0);
            assert_eq!(align_rt_alloc_count(), before);
        }
    }
    for tag in [
        1, 2, 4, 8, 0x10001, 0x10002, 0x10004, 0x10008, 0x101, 0x204, 0x208, 0x810,
    ] {
        let mut output = AlignStr {
            ptr: core::ptr::null(),
            len: 0,
        };
        assert_eq!(
            unsafe { align_rt_json_decode_array(b"[]".as_ptr(), 2, tag, &mut output) },
            0
        );
        assert!(output.ptr.is_null() && output.len == 0);
    }
    assert_eq!(align_rt_requested_live_bytes(), 0);
    Ok(())
}
