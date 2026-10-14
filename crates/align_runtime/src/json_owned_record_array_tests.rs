//! Plan188: each V3 value class through the indexed owned-array root.
use super::*;

fn field(name: &'static [u8], tag: i32, offset: i64) -> JsonField {
    JsonField {
        name_ptr: name.as_ptr(),
        name_len: i64::try_from(name.len()).unwrap(),
        tag,
        offset,
        sub: core::ptr::null(),
        opt_tag: -1,
    }
}

#[test]
fn json_owned_record_array_v3_value_and_cleanup_matrix() {
    let Some(_isolated) = crate::allocation_test::enter() else {
        return;
    };
    align_rt_requested_live_reset();
    let inner = [field(b"text", 0x810, 0)];
    let sub = JsonSubTable {
        descs: inner.as_ptr(),
        n_fields: 1,
        store_size: 16,
        phf: core::ptr::null(),
        phf_len: 0,
        phf_seed: 0,
    };
    // Payload bytes use an independent scalar oracle; aggregate leaves are checked below.
    let mut cases: Vec<(i32, &str, Vec<u8>, i64)> = vec![
        (0x101, "true", vec![1], 0),
        (0x204, "1.5", 1.5f32.to_le_bytes().to_vec(), 0),
        (0x208, "-2.25", (-2.25f64).to_le_bytes().to_vec(), 0),
        (0x810, "\"\\u304c\\u0000\"", vec![], 1),
        (0x910, "[\"a\",\"\"]", vec![], 2),
        (0x400, "{\"text\":\"nested\"}", vec![], 1),
        (0x510, "[{\"text\":\"nested\"}]", vec![], 2),
        ((8 << 24) | (1 << 16) | 0x710, "[-2,3]", vec![], 1),
        ((4 << 24) | (2 << 20) | 0x710, "[1.5,-2.25]", vec![], 1),
        ((1 << 24) | (1 << 20) | 0x710, "[true,false]", vec![], 1),
    ];
    for width in [1usize, 2, 4, 8] {
        cases.push((
            i32::try_from(width).unwrap(),
            "7",
            7u64.to_le_bytes()[..width].to_vec(),
            0,
        ));
        cases.push((
            (1 << 16) | i32::try_from(width).unwrap(),
            "-2",
            (-2i64).to_le_bytes()[..width].to_vec(),
            0,
        ));
    }
    for (tag, json, expected, payload_allocations) in cases {
        for optional in [false, true] {
            let mut payload = field(b"value", tag, 16);
            if matches!((tag >> 8) & 255, 4 | 5) {
                payload.sub = &sub;
            }
            if optional {
                payload.opt_tag = 32;
            }
            let descs = [
                field(b"lead", 0x810, 0),
                payload,
                field(b"end", 0x10008, 40),
            ];
            let row = format!("{{\"lead\":\"owner\",\"value\":{json},\"end\":7}}");
            let reordered = format!("{{\"end\":7,\"value\":{json},\"lead\":\"owner\"}}");
            let none = "{\"lead\":\"owner\",\"value\":null,\"end\":7}";
            let missing = "{\"lead\":\"owner\",\"end\":7}";
            for (source, valid, null_payload, rows) in [
                ("[]".to_string(), true, false, 0),
                (format!("[{row},{row},{reordered}]"), true, false, 3),
                (format!("[{none},{missing}]"), optional, true, 2),
                (
                    format!("[{row},{}]", row.replace("\"end\":7", "\"end\":false")),
                    false,
                    false,
                    2,
                ),
                (
                    format!("[{row},{}]", row.replace(",\"end\":7", "")),
                    false,
                    false,
                    2,
                ),
            ] {
                let before = (align_rt_alloc_count(), align_rt_free_count());
                let sentinel = core::ptr::NonNull::<u8>::dangling().as_ptr().cast_const();
                let mut out = AlignStr {
                    ptr: sentinel,
                    len: 73,
                };
                let status = unsafe {
                    align_rt_json_decode_struct_array(
                        source.as_ptr(),
                        i64::try_from(source.len()).unwrap(),
                        descs.as_ptr(),
                        3,
                        48,
                        &mut out,
                        core::ptr::null(),
                        0,
                        0,
                        core::ptr::null_mut(),
                    )
                };
                assert_eq!(
                    status,
                    if valid { 0 } else { 1 },
                    "{tag:x}/{optional}: {source}"
                );
                if valid {
                    assert_eq!(out.len, rows);
                    let allocated = align_rt_alloc_count() - before.0;
                    let per_row = if null_payload {
                        1
                    } else {
                        1 + payload_allocations
                    };
                    assert_eq!(
                        allocated,
                        if rows == 0 { 0 } else { 1 + rows * per_row },
                        "{tag:x}/{optional}: allocation shape"
                    );
                    for row in 0..usize::try_from(rows).unwrap() {
                        let base = unsafe { out.ptr.add(row * 48) };
                        let text = |offset: usize| unsafe {
                            let value = base.add(offset).cast::<AlignStr>().read_unaligned();
                            safe_slice(value.ptr, value.len)
                        };
                        assert_eq!(text(0), b"owner");
                        assert_eq!(unsafe { base.add(40).cast::<i64>().read_unaligned() }, 7);
                        if optional {
                            assert_eq!(unsafe { *base.add(32) }, u8::from(!null_payload));
                        }
                        if !null_payload {
                            if !expected.is_empty() {
                                assert_eq!(
                                    unsafe {
                                        std::slice::from_raw_parts(base.add(16), expected.len())
                                    },
                                    expected
                                );
                            } else {
                                let value =
                                    unsafe { base.add(16).cast::<AlignStr>().read_unaligned() };
                                match (tag >> 8) & 255 {
                                    8 => assert_eq!(text(16), "が\0".as_bytes()),
                                    4 => assert_eq!(text(16), b"nested"),
                                    5 | 9 => {
                                        assert_eq!(value.len, if tag == 0x910 { 2 } else { 1 });
                                        let first = unsafe {
                                            value.ptr.cast::<AlignStr>().read_unaligned()
                                        };
                                        assert_eq!(
                                            unsafe { safe_slice(first.ptr, first.len) },
                                            if tag == 0x910 {
                                                b"a".as_slice()
                                            } else {
                                                b"nested".as_slice()
                                            }
                                        );
                                        if tag == 0x910 {
                                            let last = unsafe {
                                                value
                                                    .ptr
                                                    .add(16)
                                                    .cast::<AlignStr>()
                                                    .read_unaligned()
                                            };
                                            assert!(last.ptr.is_null());
                                            assert_eq!(last.len, 0);
                                        }
                                    }
                                    7 => {
                                        assert_eq!(value.len, 2);
                                        let bytes: Vec<u8> = match (tag >> 20) & 15 {
                                            0 => [-2i64, 3]
                                                .into_iter()
                                                .flat_map(i64::to_le_bytes)
                                                .collect(),
                                            2 => [1.5f32, -2.25]
                                                .into_iter()
                                                .flat_map(f32::to_le_bytes)
                                                .collect(),
                                            _ => vec![1, 0],
                                        };
                                        assert_eq!(
                                            unsafe {
                                                std::slice::from_raw_parts(value.ptr, bytes.len())
                                            },
                                            bytes
                                        );
                                    }
                                    _ => panic!("fixture tag"),
                                }
                            }
                        }
                        unsafe { drop_decoded_owned(base.cast_mut(), &descs, None) };
                        let freed = align_rt_free_count();
                        unsafe { drop_decoded_owned(base.cast_mut(), &descs, None) };
                        assert_eq!(align_rt_free_count(), freed, "source nulling");
                    }
                    unsafe { align_rt_free(out.ptr.cast_mut()) };
                } else {
                    assert_eq!(out.ptr, sentinel);
                    assert_eq!(out.len, 73);
                    assert!(
                        align_rt_alloc_count() > before.0,
                        "partial prefix must allocate"
                    );
                }
                assert_eq!(
                    align_rt_alloc_count() - before.0,
                    align_rt_free_count() - before.1
                );
                assert_eq!(
                    align_rt_requested_live_bytes(),
                    0,
                    "{tag:x}/{optional}: {source}"
                );
            }
        }
    }
}
