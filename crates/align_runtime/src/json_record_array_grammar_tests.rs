//! Plan187: full indexed-array grammar, exact selected values and publication.
use super::*;

struct ArenaOwner(*mut Arena);
impl Drop for ArenaOwner {
    fn drop(&mut self) {
        unsafe { align_rt_arena_end(self.0) };
    }
}

fn fields() -> [JsonField; 1] {
    [JsonField {
        name_ptr: b"id".as_ptr(), name_len: 2, tag: 0x10008, offset: 0,
        sub: core::ptr::null(), opt_tag: -1,
    }]
}

fn observe(source: &[u8], soa: bool) -> (i32, Vec<i64>, bool) {
    let arena = ArenaOwner(align_rt_arena_begin());
    let sentinel = core::ptr::NonNull::<u8>::dangling().as_ptr().cast_const();
    let mut out = AlignStr { ptr: sentinel, len: 73 };
    let descs = fields();
    let len = i64::try_from(source.len()).expect("bounded fixture");
    let status = unsafe {
        if soa {
            align_rt_json_decode_soa(source.as_ptr(), len, descs.as_ptr(), 1, arena.0,
                                     &mut out, core::ptr::null(), 0, 0)
        } else {
            align_rt_json_decode_struct_array(source.as_ptr(), len, descs.as_ptr(), 1,
                                              8, &mut out, core::ptr::null(), 0, 0,
                                              core::ptr::null_mut())
        }
    };
    let untouched = out.ptr == sentinel && out.len == 73;
    let values = if status == 0 {
        unsafe { safe_slice(out.ptr.cast::<i64>(), out.len) }.to_vec()
    } else { Vec::new() };
    if !soa && out.ptr != sentinel {
        unsafe { align_rt_free(out.ptr.cast_mut()) };
    }
    (status, values, untouched)
}

#[test]
fn indexed_json_record_array_grammar_matrix() {
    let valid: &[(&[u8], &[i64])] = &[
        (b"[]", &[]),
        (b" \t[ { \"id\" : 1 } , {\"id\":-2} ]\r\n", &[1, -2]),
        (br#"[{"id":1,"skip":1e100000}]"#, &[1]),
        (br#"[{"id":1,"skip":{"x":[null,true,false,{},[],"\uD83D\uDE00"]}}]"#, &[1]),
        (br#"[{"\u0069d":1},{"id":2}]"#, &[1, 2]),
        (br#"[{"id":1,"x":0,"x":1},{"x":{},"id":2,"x":[]}]"#, &[1, 2]),
    ];
    let invalid: &[&[u8]] = &[
        b"", b"{}", b"null", b"true", b"42", b"[", b"]", b"[][]", b"[]x", b"x[]",
        b"[42]", b"[true]", b"[false]", b"[null]", b"[\"text\"]", b"[[]]",
        b"[,]", b"[{},]", b"[{},,{}]", b"[{}{}]", b"[{}", b"[{]}",
        br#"[{"id":1},,{"id":2}]"#, br#"[{"id":1},]"#, br#"[{"id":1} {"id":2}]"#,
        br#"[{"id":1,"skip":bogus}]"#, br#"[{"id":1,"skip":truefalse}]"#,
        br#"[{"id":1,"skip":nullx}]"#, br#"[{"id":1,"skip":01}]"#,
        br#"[{"id":1,"skip":1e+}]"#, br#"[{"id":1,"skip":1.}]"#,
        br#"[{"id":1,"skip":[1,]}]"#, br#"[{"id":1,"skip":{"x":0,}}]"#,
        br#"[{"id":1,"skip":{"x" 0}}]"#, br#"[{"id":1,"skip":{"x"::0}}]"#,
        br#"[{"id":1,"skip":{"x":}}]"#, br#"[{"id":1,"skip":{x:0}}]"#,
        br#"[{"id":1,"skip":"\uD800"}]"#, br#"[{"id":1,"skip":"\x00"}]"#,
        b"[{\"id\":1,\"skip\":\"\xff\"}]", b"[{\"id\":1,\"skip\":\"\n\"}]",
        // Valid syntax still requires exact schema and selected-value consumption.
        b"[{}]", br#"[{"id":1.5}]"#, br#"[{"id":1e2}]"#,
        br#"[{"id":null}]"#, br#"[{"id":true}]"#, br#"[{"id":1,"id":2}]"#,
        br#"[{"id":9223372036854775808}]"#,
        br#"[{"id":1,"skip":0},{"id":2,"skip":bogus}]"#,
        br#"[{"id":1,"skip":0},{"id":2,"id":3}]"#,
        br#"[{"id":1},{"id":2.5}]"#, br#"[{"id":1},{"id":2e3}]"#,
        br#"[{"id":1},{"id":2}] trailing"#,
    ];
    for soa in [false, true] {
        for &(source, expected) in valid {
            let (status, values, _) = observe(source, soa);
            assert_eq!(status, 0, "soa={soa}: {source:?}");
            assert_eq!(values, expected, "soa={soa}: {source:?}");
        }
        for source in invalid {
            let (status, _, untouched) = observe(source, soa);
            assert_eq!(status, 1, "soa={soa}: {source:?}");
            assert!(untouched, "error published output: soa={soa}: {source:?}");
        }
        // No new recursive-stack limit for deeply nested projected data.
        let nested = format!("[{{\"id\":7,\"skip\":{}0{}}}]", "[".repeat(4096), "]".repeat(4096));
        assert_eq!(observe(nested.as_bytes(), soa).0, 0);
        let malformed = nested.replacen('0', "0,", 1);
        assert_eq!(observe(malformed.as_bytes(), soa).0, 1);
    }
}

#[cfg(feature = "alloc-count")]
#[test]
fn indexed_json_record_array_grammar_and_schema_cleanup() {
    let Some(_isolated) = crate::allocation_test::enter() else { return; };
    align_rt_requested_live_reset();
    let descs = [
        JsonField { name_ptr: b"text".as_ptr(), name_len: 4, tag: 0x810,
                    offset: 0, sub: core::ptr::null(), opt_tag: -1 },
        JsonField { offset: 16, ..fields()[0] },
    ];
    let cases: &[(&[u8], bool)] = &[
        (br#"[{"text":"a","id":1},]"#, false),
        (br#"[{"text":"a","id":1},{"text":"b","id":bogus}]"#, false),
        (br#"[{"text":"a","id":1},{"text":"b","id":"bad"}]"#, true),
        (br#"[{"text":"a","id":1},{"text":"b","id":1.5}]"#, true),
        (br#"[{"text":"a","id":1},{"text":"b","missing":2}]"#, true),
    ];
    for &(source, allocated) in cases {
        let before = (align_rt_alloc_count(), align_rt_free_count());
        let mut out = AlignStr { ptr: core::ptr::null(), len: 0 };
        let status = unsafe {
            align_rt_json_decode_struct_array(source.as_ptr(), i64::try_from(source.len()).expect("bounded fixture"),
                descs.as_ptr(), 2, 24, &mut out, core::ptr::null(), 0, 0, core::ptr::null_mut())
        };
        assert_eq!(status, 1);
        assert!(out.ptr.is_null());
        assert_eq!(out.len, 0);
        let allocations = align_rt_alloc_count() - before.0;
        assert_eq!(allocations > 0, allocated);
        assert_eq!(allocations, align_rt_free_count() - before.1);
        assert_eq!(align_rt_requested_live_bytes(), 0);
    }
}
