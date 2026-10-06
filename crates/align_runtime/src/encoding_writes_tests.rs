//! Exact initialized output and fail-closed private escaped-output destinations.
use super::*;
use core::mem::MaybeUninit;

type Writer = fn(&[u8], &mut [MaybeUninit<u8>]);

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
fn encoding_writers_initialize_exact_destinations_and_reject_mismatches() {
    let writers: [Writer; 4] = [
        percent_encode_into,
        |input, output| percent_encode_into_with_slash(input, output, true),
        html_escape_into,
        form_encode_into,
    ];
    for (kind, writer) in writers.into_iter().enumerate() {
        let mut cases = vec![
            vec![],
            (0u8..=127).collect(),
            b"&<>\"'&amp;&#39; /%+?-._~".to_vec(),
            "日本語é🦀".as_bytes().to_vec(),
            b"<& a/b >".repeat(1024),
        ];
        for length in [
            1, 2, 3, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129, 255, 256, 257,
        ] {
            cases.push(vec![b'&'; length]);
        }
        if kind != 2 {
            cases.push((0u8..=255).collect());
            cases.push((0u8..=255).rev().collect());
        }
        for input in cases {
            let expected = reference(&input, kind);
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
