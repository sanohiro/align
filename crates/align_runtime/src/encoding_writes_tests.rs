//! Exact initialized output and fail-closed private escaped-output destinations.
use super::*;
use core::mem::MaybeUninit;

type Writer = fn(&[u8], &mut [MaybeUninit<u8>]);

fn escaped_prefix_inputs() -> Vec<Vec<u8>> {
    let mut inputs = Vec::new();
    for length in [511, 512, 513, 639, 640, 641, 4096] {
        for seed in [b"a".as_slice(), b"/", b" ", b" A0~"] {
            for byte in [b'+', b'%', b'/', 0, 255] {
                for position in [0, 127, 128, 129, length / 2, length - 1] {
                    let mut input: Vec<u8> = seed.iter().copied().cycle().take(length).collect();
                    input[position] = byte;
                    inputs.push(input);
                }
            }
        }
    }
    inputs
}

#[test]
fn percent_form_lengths_cover_every_byte_at_reduction_boundaries() {
    for byte in 0u8..=255 {
        for length in [0, 1, 15, 16, 127, 128, 129, 255, 256, 257, 4096] {
            let input = vec![byte; length];
            for (kind, actual) in [
                (0, percent_encoded_len(&input)),
                (1, percent_encoded_len_with_slash(&input, true)),
                (3, form_encoded_len(&input)),
            ] {
                assert_eq!(actual, Some(reference(&input, kind).len()),
                    "kind={kind}, byte={byte}, length={length}");
            }
        }
    }
}

fn reference(input: &[u8], kind: usize) -> Vec<u8> {
    let mut output = Vec::new();
    for &byte in input {
        if kind == 2 {
            match byte {
                b'&' => output.extend_from_slice(b"&amp;"),
                b'<' => output.extend_from_slice(b"&lt;"),
                b'>' => output.extend_from_slice(b"&gt;"),
                b'"' => output.extend_from_slice(b"&quot;"),
                b'\'' => output.extend_from_slice(b"&#39;"),
                _ => output.push(byte),
            }
        } else if byte.is_ascii_alphanumeric()
            || b"-._~".contains(&byte)
            || (kind == 1 && byte == b'/')
        {
            output.push(byte);
        } else if kind == 3 && byte == b' ' {
            output.push(b'+');
        } else {
            output.extend_from_slice(format!("%{byte:02X}").as_bytes());
        }
    }
    output
}

#[test]
fn html_discriminator_search_has_bounded_work_for_every_entity_group() {
    for length in [0, 1, 255, 256, 257, 65536] {
        let input = vec![b'a'; length];
        let mut visited = 0;
        let first = html_literal_run(&input, |window| {
            assert!(window.len() <= 256);
            visited += window.len();
            html_entity_in_window(window)
        });
        assert_eq!(first, length);
        assert_eq!(visited, length);
    }
    for &entity in b"&<>\"'" {
        for prefix in [0, 1, 15, 16, 17, 63, 64, 255, 256, 257, 1024, 65536] {
            let mut input = vec![b'a'; prefix];
            input.push(entity);
            input.extend_from_slice(&[b'z'; 65536]);
            let mut visited = 0;
            let first = html_literal_run(&input, |window| {
                assert!(window.len() <= 256, "unbounded discriminator search");
                visited += window.len();
                html_entity_in_window(window)
            });
            assert_eq!(first, prefix);
            // Each window is searched at most twice; total work is at most twice
            // the ordinary prefix plus one fixed window, regardless of the suffix.
            assert!(visited <= prefix + 256, "entity={entity}, prefix={prefix}, visited={visited}");
        }
        let mut repeated = vec![b'a'; 16];
        repeated.push(entity);
        let input = repeated.repeat(4096);
        let mut cursor = 0;
        let mut visited = 0;
        while cursor < input.len() {
            let copied = html_literal_run(&input[cursor..], |window| {
                assert!(window.len() <= 256);
                visited += window.len();
                html_entity_in_window(window)
            });
            assert_eq!(copied, 16);
            cursor += copied + 1;
        }
        assert!(visited <= 4096 * 256);
    }
}

#[test]
fn encoding_writers_initialize_exact_destinations_and_reject_mismatches() {
    let writers: [Writer; 4] = [
        percent_encode_expanded_into::<false>,
        percent_encode_expanded_into::<true>,
        html_escape_into,
        form_encode_expanded_into,
    ];
    let lengths: [fn(&[u8]) -> Option<usize>; 4] = [
        percent_encoded_len,
        |input| percent_encoded_len_with_slash(input, true),
        html_escaped_len,
        form_encoded_len,
    ];
    for (kind, writer) in writers.into_iter().enumerate() {
        let mut cases = vec![
            vec![],
            (0u8..=127).collect(),
            b"&<>\"'&amp;&#39; /%+?-._~".to_vec(),
            "日本語é🦀".as_bytes().to_vec(),
            b"<& a/b >".repeat(1024),
        ];
        if kind != 2 {
            cases.extend(escaped_prefix_inputs());
        }
        for length in [
            1, 2, 3, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129, 255, 256, 257,
        ] {
            for byte in b"&<>\"'" {
                cases.push(vec![*byte; length]);
            }
            if kind == 2 {
                cases.push(b"&<>\"'a".iter().copied().cycle().take(length).collect());
            } else {
                cases.push((0u8..=255).cycle().take(length).collect());
            }
        }
        if kind != 2 {
            cases.push((0u8..=255).collect());
            cases.push((0u8..=255).rev().collect());
        } else {
            for &entity in b"&<>\"'" {
                let mut run = vec![b'a'; 16];
                run.push(entity);
                cases.push(run.repeat(256));
            }
            for length in (0..=84).chain([127, 128, 129, 255, 256, 257, 4095, 4096, 4097]) {
                for &entity in b"&<>\"'" {
                    let mut input = vec![entity];
                    input.extend(std::iter::repeat_n(b'a', length));
                    input.push(entity);
                    input.extend_from_slice("日本\0".as_bytes());
                    input.extend(std::iter::repeat_n(b'z', length));
                    input.extend_from_slice(b"&<>\"'");
                    cases.push(input);
                }
            }
        }
        for input in cases {
            let expected = reference(&input, kind);
            assert_eq!(lengths[kind](&input), Some(expected.len()), "kind={kind}, input={input:?}");
            for sentinel in [0xa5, 0x5a] {
                let mut output = vec![MaybeUninit::new(sentinel); expected.len()];
                writer(&input, &mut output);
                // Every test slot was initialized before the call, even if a faulty writer skips
                // one. Distinct sentinels discriminate skipped writes without an uninitialized read.
                let actual = output
                    .iter()
                    .map(|byte| unsafe { byte.assume_init() })
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "kind={kind}, input={input:?}");
            }
            for length in [expected.len().checked_sub(1), Some(expected.len() + 1)]
                .into_iter()
                .flatten()
            {
                let mut output = vec![MaybeUninit::new(0xa5); length];
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    writer(&input, &mut output)
                }));
                assert!(
                    result.is_err(),
                    "kind={kind}, input bytes={}, output={length}, exact={}",
                    input.len(),
                    expected.len()
                );
            }
        }
    }
}

#[test]
fn public_encoding_entries_preserve_exact_owned_output() {
    #[cfg(feature = "alloc-count")]
    let Some(_isolated) = crate::allocation_test::enter() else { return; };
    struct Owned(AlignStr);
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { align_rt_free(self.0.ptr.cast_mut()) };
        }
    }
    let encoders: [unsafe extern "C" fn(*const u8, i64) -> AlignStr; 4] = [
        align_rt_percent_encode,
        align_rt_percent_encode_path,
        align_rt_html_escape,
        align_rt_form_encode,
    ];
    let mut inputs = vec![Vec::new(), (0u8..=127).collect()];
    inputs.extend(escaped_prefix_inputs());
    for byte in 0u8..=255 {
        for length in [1, 129] {
            inputs.push(vec![byte; length]);
        }
    }
    for seed in [
        b"Alpha09-._~".as_slice(), b"a/b//./../", b" ", b"&<>\"'", b"\0",
        "日本語é🦀".as_bytes(),
    ] {
        inputs.push(seed.to_vec());
        inputs.push(seed.repeat(1024));
    }
    for length in [0, 1, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129, 255, 256, 257, 4096] {
        let input: Vec<u8> = b" Az09-._~ ".iter().copied().cycle().take(length).collect();
        inputs.push(input.clone());
        if length != 0 {
            for byte in [b'+', b'%', b'/', 0, 255] {
                let mut expanded = input.clone();
                expanded[length / 2] = byte;
                inputs.push(expanded);
            }
        }
    }
    for (kind, encode) in encoders.into_iter().enumerate() {
        for input in &inputs {
            if kind == 2 && std::str::from_utf8(input).is_err() {
                continue; // Invalid HTML text has its separate abort/admission owner.
            }
            let expected = reference(input, kind);
            #[cfg(feature = "alloc-count")]
            let counts = (align_rt_alloc_count(), align_rt_free_count());
            let pointer = if input.is_empty() { core::ptr::null() } else { input.as_ptr() };
            let output = Owned(unsafe { encode(pointer, i64::try_from(input.len()).unwrap()) });
            assert_eq!(usize::try_from(output.0.len).unwrap(), expected.len(), "kind={kind}");
            if expected.is_empty() {
                assert!(output.0.ptr.is_null());
            } else {
                assert!(!output.0.ptr.is_null());
                assert_ne!(output.0.ptr, input.as_ptr(), "output must own independent storage");
                let actual = unsafe { core::slice::from_raw_parts(output.0.ptr, expected.len()) };
                assert_eq!(actual, expected, "kind={kind}, input={input:?}");
            }
            #[cfg(feature = "alloc-count")]
            assert_eq!(align_rt_alloc_count()-counts.0, i64::from(!expected.is_empty()));
            drop(output);
            #[cfg(feature = "alloc-count")]
            assert_eq!(align_rt_free_count()-counts.1, i64::from(!expected.is_empty()));
            assert_eq!(reference(input, kind), expected, "input survives output Drop");
        }
    }
}
