//! Encode the existing validated tape without numeric conversion or decoded-string scratch.
use super::*;

fn write_node(
    buf: &mut BuilderBuf,
    input: &[u8],
    nodes: &[DocNode],
    index: usize,
    depth: u32,
) -> Option<usize> {
    if buf.failed_limit() || depth > 128 {
        return None;
    }
    let node = nodes.get(index)?;
    let next = usize::try_from(node.next).ok()?;
    let first = index.checked_add(1)?;
    if next < first || next > nodes.len() {
        return None;
    }
    match node.kind {
        DOC_OBJECT | DOC_ARRAY => {
            let object = node.kind == DOC_OBJECT;
            buf.push(if object { b'{' } else { b'[' });
            let mut child = first;
            for ordinal in 0..node.a {
                if buf.failed_limit() || child >= next {
                    return None;
                }
                if ordinal != 0 {
                    buf.push(b',');
                }
                if object {
                    let key = nodes.get(child)?;
                    if key.kind != DOC_STR {
                        return None;
                    }
                    // Keys do not increase JSON value depth, matching DocBuilder::object.
                    child = write_node(buf, input, nodes, child, depth)?;
                    if child >= next {
                        return None;
                    }
                    buf.push(b':');
                }
                child = write_node(buf, input, nodes, child, depth + 1)?;
                if child > next {
                    return None;
                }
            }
            if child != next {
                return None;
            }
            buf.push(if object { b'}' } else { b']' });
        }
        DOC_STR | DOC_NUMBER => {
            if next != first {
                return None;
            }
            let start = usize::try_from(node.a).ok()?;
            let len = usize::try_from(node.b).ok()?;
            let raw = input.get(start..start.checked_add(len)?)?;
            let string = node.kind == DOC_STR;
            // Reserve the complete token first: a large escaped string under a tiny cap never
            // allocates decoded bytes or visits its body. The parser already validated it.
            if !buf.reserve(len.checked_add(if string { 2 } else { 0 })?) {
                return None;
            }
            if string {
                buf.push(b'"');
            }
            buf.extend_from_slice(raw);
            if string {
                buf.push(b'"');
            }
        }
        DOC_BOOL => {
            if next != first || node.a > 1 {
                return None;
            }
            buf.extend_from_slice(if node.a == 0 { b"false" } else { b"true" });
        }
        DOC_NULL => {
            if next != first {
                return None;
            }
            buf.extend_from_slice(b"null");
        }
        _ => return None,
    }
    (!buf.failed_limit()).then_some(next)
}

/// Append one validated document subtree to the existing JSON builder. Missing or invalid
/// metadata sets sticky failure; finalization owns error publication and output cleanup.
/// Original string escapes and number tokens are preserved; only structural whitespace is lost.
///
/// # Safety
/// `builder` is null or an exclusive live JSON builder. A nonnull `tape` is a live parser-produced
/// header whose initialized node/input ranges remain readable, disjoint from the builder and its
/// output, through this call. No tape/input pointer is retained. Negative nodes denote Missing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_json_encode_doc(
    builder: *mut Builder,
    tape: *const DocTape,
    node: i64,
) {
    if builder.is_null() {
        return;
    }
    let buf = &mut unsafe { &mut *builder }.buf;
    // A negative bound must not even inspect a document header.
    if buf.failed_limit() {
        return;
    }
    let result = (|| {
        if tape.is_null() {
            return None;
        }
        let index = usize::try_from(node).ok()?;
        let tape = unsafe { &*tape };
        let max = usize::try_from(isize::MAX).ok()?;
        if tape.input_ptr.is_null()
            || tape.nodes.is_null()
            || tape.input_len > max
            || tape.nodes_len > max / core::mem::size_of::<DocNode>()
        {
            return None;
        }
        // SAFETY: the ABI requires initialized readable ranges; representable extents and
        // nonnull pointers were checked above. Node/span indexing below stays checked.
        let input = unsafe { core::slice::from_raw_parts(tape.input_ptr, tape.input_len) };
        let nodes = unsafe { core::slice::from_raw_parts(tape.nodes, tape.nodes_len) };
        write_node(buf, input, nodes, index, 0)
    })();
    if result.is_none() {
        buf.encode_failed = true;
    }
}

const _: unsafe extern "C" fn(*mut Builder, *const DocTape, i64) = align_rt_json_encode_doc;

#[cfg(test)]
mod tests {
    use super::*;
    struct Document {
        arena: *mut Arena,
        handle: DocHandle,
    }
    impl Document {
        fn parse(input: &str) -> Self {
            let arena = align_rt_arena_begin();
            let mut handle = DocHandle {
                tape: core::ptr::null(),
                node: 0,
            };
            assert_eq!(
                unsafe {
                    align_rt_json_doc_parse(
                        input.as_ptr(),
                        i64::try_from(input.len()).unwrap(),
                        arena,
                        &mut handle,
                    )
                },
                0
            );
            Self { arena, handle }
        }
    }
    impl Drop for Document {
        fn drop(&mut self) {
            unsafe { align_rt_arena_end(self.arena) };
        }
    }
    #[repr(align(16))]
    struct Header([u8; 64]);

    fn encode(handle: DocHandle, limit: Option<i64>, expected: Option<&str>) {
        let mut header = Header([0; 64]);
        let b = unsafe {
            align_rt_json_builder_init(
                header.0.as_mut_ptr(),
                i32::from(limit.is_some()),
                limit.unwrap_or(0),
            )
        };
        unsafe { align_rt_json_encode_doc(b, handle.tape, handle.node) };
        let capacity = unsafe { (*b).buf.cap };
        if let Some(limit) = limit {
            assert!(capacity <= usize::try_from(limit).unwrap_or(0));
        }
        let original = unsafe { (*b).buf.ptr };
        let mut output = AlignStr {
            ptr: core::ptr::null(),
            len: -1,
        };
        let status = unsafe { align_rt_json_builder_finish(b, &mut output) };
        match expected {
            Some(expected) => {
                assert_eq!(status, 0);
                assert_eq!(
                    unsafe { safe_slice(output.ptr, output.len) },
                    expected.as_bytes()
                );
                assert_eq!(output.ptr, original, "output must transfer without a copy");
                unsafe { align_rt_free(output.ptr.cast_mut()) };
            }
            None => {
                assert_eq!(status, AL_INVALID);
                assert!(output.ptr.is_null());
                assert_eq!(output.len, 0);
            }
        }
    }

    #[test]
    fn json_doc_encode_token_and_bound_matrix() {
        for (input, expected) in [
            (" { } ", "{}"),
            (" [ ] ", "[]"),
            (" null ", "null"),
            (" true ", "true"),
            ("false", "false"),
            ("-0", "-0"),
            ("1.2300E+009", "1.2300E+009"),
            ("1e99999", "1e99999"),
            ("18446744073709551616", "18446744073709551616"),
            (
                r#" "a\u0000\uD83D\uDE00\/\"\\\b\f\n\r\t日本語" "#,
                r#""a\u0000\uD83D\uDE00\/\"\\\b\f\n\r\t日本語""#,
            ),
            (
                r#" { "x" : [ null, false, {"":-1e-9999} ], "x": 2, "\u0078":3 } "#,
                r#"{"x":[null,false,{"":-1e-9999}],"x":2,"\u0078":3}"#,
            ),
            (
                r#" { "type":"object", "properties": {"cmd": {"type":"string","enum":["a b", ""]}}, "required": ["cmd"], "maximum":999999999999999999999 } "#,
                r#"{"type":"object","properties":{"cmd":{"type":"string","enum":["a b",""]}},"required":["cmd"],"maximum":999999999999999999999}"#,
            ),
        ] {
            let document = Document::parse(input);
            let length = i64::try_from(expected.len()).unwrap();
            for limit in [
                None,
                Some(length),
                Some(length + 1),
                Some(-1),
                Some(0),
                Some(length - 1),
            ] {
                encode(
                    document.handle,
                    limit,
                    limit.is_none_or(|cap| cap >= length).then_some(expected),
                );
            }
            encode(document.handle, None, Some(expected)); // failed writes never poison input
        }
        let document = Document::parse(r#"{"first":0,"selected":["\u0000",1e999,true],"last":9}"#);
        let mut selected = document.handle;
        unsafe {
            align_rt_json_doc_get(
                selected.tape,
                selected.node,
                b"selected".as_ptr(),
                8,
                &mut selected,
            )
        };
        encode(selected, None, Some(r#"["\u0000",1e999,true]"#));
        unsafe { align_rt_json_doc_at(selected.tape, selected.node, 1, &mut selected) };
        encode(selected, Some(5), Some("1e999"));
        for node in [-1, i64::MAX] {
            encode(
                DocHandle {
                    node,
                    ..document.handle
                },
                None,
                None,
            );
        }
        encode(
            DocHandle {
                tape: core::ptr::null(),
                node: 0,
            },
            None,
            None,
        );
        // Negative limits and sticky failure precede even tape dereferencing.
        encode(
            DocHandle {
                tape: core::ptr::NonNull::dangling().as_ptr(),
                node: 0,
            },
            Some(-1),
            None,
        );
    }

    #[test]
    fn json_doc_encode_depth_metadata_and_early_stop() {
        let deep = format!("{}0{}", "[".repeat(128), "]".repeat(128));
        let document = Document::parse(&deep);
        encode(document.handle, Some(257), Some(&deep));
        encode(document.handle, Some(1), None);
        let escaped = format!("\"{}\"", "\\u0000".repeat(100_000));
        let document = Document::parse(&escaped);
        encode(document.handle, Some(8), None);
        let wide = format!("[{}0]", "0,".repeat(100_000));
        let document = Document::parse(&wide);
        encode(document.handle, Some(8), None);

        let input = b"0";
        for node in [
            DocNode {
                kind: DOC_NUMBER,
                next: 0,
                a: 0,
                b: 1,
            },
            DocNode {
                kind: DOC_NUMBER,
                next: 2,
                a: 0,
                b: 1,
            },
            DocNode {
                kind: DOC_NUMBER,
                next: 1,
                a: u32::MAX,
                b: 1,
            },
            DocNode {
                kind: DOC_NUMBER,
                next: 1,
                a: 0,
                b: u32::MAX,
            },
            DocNode {
                kind: DOC_ARRAY,
                next: 1,
                a: 1,
                b: 0,
            },
            DocNode {
                kind: DOC_OBJECT,
                next: 1,
                a: u32::MAX,
                b: 0,
            },
            DocNode {
                kind: DOC_BOOL,
                next: 1,
                a: 2,
                b: 0,
            },
            DocNode {
                kind: u32::MAX,
                next: 1,
                a: 0,
                b: 0,
            },
        ] {
            let tape = DocTape {
                input_ptr: input.as_ptr(),
                input_len: 1,
                nodes: &node,
                nodes_len: 1,
                arena: core::ptr::null_mut(),
            };
            encode(
                DocHandle {
                    tape: &tape,
                    node: 0,
                },
                None,
                None,
            );
        }
        // An unreadable second node is never inspected once the first token exceeds the cap.
        let nodes = [
            DocNode {
                kind: DOC_ARRAY,
                next: 3,
                a: 2,
                b: 0,
            },
            DocNode {
                kind: DOC_NUMBER,
                next: 2,
                a: 0,
                b: 1,
            },
            DocNode {
                kind: u32::MAX,
                next: 3,
                a: u32::MAX,
                b: u32::MAX,
            },
        ];
        let mut buf = BuilderBuf::new_bounded(1);
        assert!(write_node(&mut buf, input, &nodes, 0, 0).is_none());
        assert!(buf.failed_limit());
        assert_eq!(buf.as_slice(), b"[");
    }

    #[test]
    #[cfg(feature = "alloc-count")]
    fn json_doc_encode_output_allocation_and_cleanup() {
        let Some(_isolated) = crate::allocation_test::enter() else {
            return;
        };
        let input = format!(
            "{{\"schema\":[{},1e9999]}}",
            "0,".repeat(5000).trim_end_matches(',')
        );
        let document = Document::parse(&input);
        let before = global_alloc_count();
        encode(document.handle, Some(8), None);
        assert_eq!(
            global_alloc_count(),
            before,
            "serialization uses no Rust scratch allocation"
        );
        // Parse allocations predate this observer. Each failed/successful output is independent.
        for _ in 0..8 {
            for limit in [0, 1, 8, 64, i64::try_from(input.len()).unwrap()] {
                align_rt_requested_live_reset();
                encode(
                    document.handle,
                    Some(limit),
                    (limit == i64::try_from(input.len()).unwrap()).then_some(input.as_str()),
                );
                assert_eq!(align_rt_requested_live_bytes(), 0);
                assert!(
                    align_rt_requested_live_peak() <= limit * 2,
                    "peak includes old/new realloc overlap"
                );
                if limit > 0 {
                    assert!(
                        align_rt_requested_live_peak() > 0,
                        "allocation observation is active"
                    );
                }
            }
        }
    }
}
