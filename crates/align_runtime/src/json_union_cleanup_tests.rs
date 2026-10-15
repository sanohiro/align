//! Plan202: selected-arm ownership and failure publication use the emitted overlapping layout.
use super::*;

fn field(name: &'static [u8], tag: i32, offset: i64, sub: *const JsonSubTable) -> JsonField {
    JsonField {
        name_ptr: name.as_ptr(),
        name_len: i64::try_from(name.len()).unwrap(),
        tag,
        offset,
        sub,
        opt_tag: -1,
    }
}
fn table(fields: &[JsonField], size: i64) -> JsonSubTable {
    JsonSubTable {
        descs: fields.as_ptr(),
        n_fields: i64::try_from(fields.len()).unwrap(),
        store_size: size,
        phf: core::ptr::null(),
        phf_len: 0,
        phf_seed: 0,
    }
}

#[test]
fn selected_union_cleanup_matrix() {
    let Some(_isolated) = crate::allocation_test::enter() else {
        return;
    };
    let part_fields = [field(b"value", 8, 0, core::ptr::null())];
    let part = table(&part_fields, 8);
    let row_fields = [
        field(b"text", (3 << 8) | 16, 0, core::ptr::null()),
        field(
            b"values",
            (7 << 8) | 16 | (1 << 16) | (8 << 24),
            16,
            core::ptr::null(),
        ),
        field(b"required", 8, 32, core::ptr::null()),
    ];
    let row = table(&row_fields, 40);
    // Every arm shares offset 8, exactly as the LLVM tagged-union storage does.
    for record_first in [false, true] {
        let mut arms = [
            field(b"", (5 << 8) | 16, 8, &part),
            field(b"", 4 << 8, 8, &row),
        ];
        if record_first {
            arms.swap(0, 1);
        }
        let record_index = if record_first { 0 } else { 1 };
        let classes = [-1, -1, -1, record_index, 1 - record_index];
        let tags = [0, 1];
        let union = JsonUnion {
            arms: arms.as_ptr(),
            class_to_arm: classes.as_ptr(),
            enum_tags: tags.as_ptr(),
            n_arms: 2,
            store_size: 48,
        };
        let outer_fields = [
            field(b"choice", 6 << 8, 0, (&union as *const JsonUnion).cast()),
            field(b"tail", 8, 48, core::ptr::null()),
        ];
        let outer = table(&outer_fields, 56);
        let array_field = field(b"", (5 << 8) | 16, 0, &outer);
        for arena_text in [false, true] {
            let arena = if arena_text {
                align_rt_arena_begin()
            } else {
                core::ptr::null_mut()
            };
            for (payload, complete) in [
                (r#"{"text":"borrowed","values":[1,2],"required":3}"#, true),
                (r#"{"text":"borrowed","values":[1,2]}"#, false),
                (r#"{"text":"borrowed"}"#, false),
                (r#"[{"value":1},{"value":2}]"#, true),
            ] {
                for placement in 0..3 {
                    for later_error in [false, true] {
                        let source = match placement {
                            0 => format!("{payload}{}", if later_error { " trailing" } else { "" }),
                            1 => format!(
                                "{{\"choice\":{payload}{}}}",
                                if later_error { "" } else { ",\"tail\":4" }
                            ),
                            _ => format!(
                                "[{{\"choice\":{payload},\"tail\":4}},{{\"choice\":{payload}{}}}]",
                                if later_error { "" } else { ",\"tail\":5" }
                            ),
                        };
                        align_rt_requested_live_reset();
                        let (allocs, frees) = (align_rt_alloc_count(), align_rt_free_count());
                        let mut out = [0u8; 56];
                        let mut array = AlignStr {
                            ptr: core::ptr::null(),
                            len: 0,
                        };
                        let len = i64::try_from(source.len()).unwrap();
                        let status = unsafe {
                            match placement {
                                0 => align_rt_json_decode_union(
                                    source.as_ptr(),
                                    len,
                                    &union,
                                    out.as_mut_ptr(),
                                    arena,
                                ),
                                1 => align_rt_json_decode(
                                    source.as_ptr(),
                                    len,
                                    outer_fields.as_ptr(),
                                    2,
                                    out.as_mut_ptr(),
                                    56,
                                    core::ptr::null(),
                                    0,
                                    0,
                                    arena,
                                ),
                                _ => align_rt_json_decode_struct_array(
                                    source.as_ptr(),
                                    len,
                                    outer_fields.as_ptr(),
                                    2,
                                    56,
                                    &mut array,
                                    core::ptr::null(),
                                    0,
                                    0,
                                    arena,
                                ),
                            }
                        };
                        let label = format!(
                            "record_first={record_first}/arena={arena_text}/placement={placement}/later={later_error}/{payload}"
                        );
                        assert_eq!(status == 0, complete && !later_error, "{label}");
                        if status == 0 {
                            assert!(
                                align_rt_requested_live_bytes() > 0,
                                "counter activation: {label}"
                            );
                        }
                        if status != 0 {
                            assert_eq!(
                                align_rt_requested_live_bytes(),
                                0,
                                "error leaked before caller cleanup: {label}"
                            );
                            assert_eq!(
                                align_rt_alloc_count() - allocs,
                                align_rt_free_count() - frees,
                                "{label}"
                            );
                        }
                        // Repeat cleanup after both successful and partial/error construction.
                        // The first pass on failure may already have run inside multiple parents.
                        for _ in 0..2 {
                            unsafe {
                                match placement {
                                    0 => drop_decoded_union(out.as_mut_ptr(), &union),
                                    1 => drop_decoded_owned(out.as_mut_ptr(), &outer_fields, None),
                                    _ => drop_decoded_owned(
                                        (&mut array as *mut AlignStr).cast(),
                                        core::slice::from_ref(&array_field),
                                        None,
                                    ),
                                }
                            };
                        }
                        assert_eq!(align_rt_requested_live_bytes(), 0, "{label}");
                        assert_eq!(
                            align_rt_alloc_count() - allocs,
                            align_rt_free_count() - frees,
                            "{label}"
                        );
                    }
                }
            }
            if !arena.is_null() {
                unsafe { align_rt_arena_end(arena) };
            }
        }
    }
}
