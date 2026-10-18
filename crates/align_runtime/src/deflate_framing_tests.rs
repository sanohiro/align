//! Plan226: independent framing bytes, bounded windows/caps and atomic output.
use super::*;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
// Python 3 zlib.compressobj(level=6, wbits=15/-15), independently decoded by
// zlib.decompress. Payloads: empty, left, right, binary 00 ff 80 41.
const ZLIB: [&str; 4] = [
    "789c030000000001",
    "789ccb494d2b0100042301ac",
    "789c2bca4ccf280100065c021f",
    "789c63f8dfe00800044201c1",
];
const RAW: [&str; 4] = ["0300", "cb494d2b0100", "2bca4ccf280100", "63f8dfe00800"];
const DICTIONARY: &str = "78bb371c0691cb46e32ba00b0000dfe90d41";
// Valid 2x1 RGB8 PNG, two independently CRC32-framed IDAT chunks. The unfiltered
// scanline is 00 ff 00 00 00 ff 00 (red, green). No PNG support enters production.
const PNG: &str = "89504e470d0a1a0a0000000d49484452000000020000000108020000007b40e8dd0000000549444154789c63f8cfef976d360000000a49444154c0c0f09f010007ff01ffda58c1390000000049454e44ae426082";
fn bytes(hex: &str) -> TestResult<Vec<u8>> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
        .collect()
}
fn decode(data: &[u8], format: DeflateFormat, cap: usize, chunk: usize) -> Result<Vec<u8>, i32> {
    zlib_inflate(data, format, cap, chunk)
}

#[test]
fn framing_round_trip_sizes_levels_and_windows() -> TestResult {
    let large = (0usize..131081)
        .map(|index| u8::try_from((index * 73 + index / 11) % 256))
        .collect::<Result<Vec<_>, _>>()?;
    for format in [DeflateFormat::Zlib, DeflateFormat::Raw] {
        for level in 0..=9 {
            for data in [
                b"".as_slice(),
                b"small\0\xff",
                &[0xff; 257],
                large.as_slice(),
            ] {
                for chunk in [17, 4096, zlib_input_chunk()] {
                    let encoded = zlib_deflate(data, level, format, chunk)
                        .map_err(|s| format!("compress status {s}"))?;
                    assert!(!encoded.is_empty());
                    assert_eq!(
                        decode(&encoded, format, data.len(), chunk),
                        Ok(data.to_vec()),
                        "{format:?} level={level} chunk={chunk} len={}",
                        data.len()
                    );
                }
            }
            let encoded = zlib_deflate(b"tiny window", level, format, 1)
                .map_err(|s| format!("compress status {s}"))?;
            assert_eq!(decode(&encoded, format, 11, 1), Ok(b"tiny window".to_vec()));
        }
    }
    Ok(())
}

#[test]
fn single_stream_completion_and_reference_vectors() -> TestResult {
    for (format, goldens) in [(DeflateFormat::Zlib, ZLIB), (DeflateFormat::Raw, RAW)] {
        for (hex, expected) in
            goldens
                .into_iter()
                .zip([b"".as_slice(), b"left", b"right", b"\0\xff\x80A"])
        {
            let input = bytes(hex)?;
            for chunk in [1, 2, 3, 7, zlib_input_chunk()] {
                assert_eq!(decode(&input, format, 1024, chunk), Ok(expected.to_vec()));
                for length in 0..input.len() {
                    assert_eq!(
                        decode(&input[..length], format, 1024, chunk),
                        Err(AL_INVALID),
                        "{format:?} truncation={length}"
                    );
                }
                for suffix in [
                    vec![0],
                    vec![255],
                    b"garbage".to_vec(),
                    bytes(goldens[0])?,
                    bytes(goldens[2])?,
                ] {
                    assert_eq!(
                        decode(&[input.as_slice(), &suffix].concat(), format, 1024, chunk),
                        Err(AL_INVALID)
                    );
                }
            }
        }
    }
    for chunk in [1, 2, 7, zlib_input_chunk()] {
        let mut bad = bytes(ZLIB[1])?;
        *bad.last_mut().ok_or("empty fixture")? ^= 1;
        assert_eq!(decode(&bad, DeflateFormat::Zlib, 4, chunk), Err(AL_INVALID));
        assert_eq!(
            decode(&bytes(DICTIONARY)?, DeflateFormat::Zlib, 1024, chunk),
            Err(AL_INVALID)
        );
    }
    // Wrong-framing rejection is for these reference bytes, not a magical raw-format detector.
    let fixtures = [
        bytes("1f8b08000000000002ffcb494d2b010068e7677a04000000")?,
        bytes(ZLIB[1])?,
        bytes(RAW[1])?,
        bytes("28b52ffd04582100006c656674c89be648")?,
    ];
    for (encoded_kind, input) in fixtures.iter().enumerate() {
        for decoder_kind in 0..4 {
            let decoded = match decoder_kind {
                0 => gzip_inflate(input),
                1 => decode(input, DeflateFormat::Zlib, 1024, 1),
                2 => decode(input, DeflateFormat::Raw, 1024, 1),
                _ => zstd_decompress_impl(input),
            };
            assert_eq!(
                decoded,
                if decoder_kind == encoded_kind {
                    Ok(b"left".to_vec())
                } else {
                    Err(AL_INVALID)
                }
            );
        }
    }
    // Unused bits in the final raw byte belong to that byte, not a trailing stream.
    assert_eq!(decode(&[3, 0xfc], DeflateFormat::Raw, 0, 1), Ok(vec![]));
    let png = bytes(PNG)?;
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut position = 8;
    let mut idat = Vec::new();
    let mut chunks = 0;
    while position < png.len() {
        let length = usize::try_from(u32::from_be_bytes(png[position..position + 4].try_into()?))?;
        if &png[position + 4..position + 8] == b"IDAT" {
            idat.extend_from_slice(&png[position + 8..position + 8 + length]);
            chunks += 1;
        }
        position += length + 12;
    }
    assert_eq!(chunks, 2);
    assert_eq!(
        decode(&idat, DeflateFormat::Zlib, 7, 1),
        Ok(vec![0, 255, 0, 0, 0, 255, 0])
    );
    Ok(())
}

type NativeDecode = unsafe extern "C" fn(*const u8, i64, *mut *mut Buffer) -> i32;
struct BufferOwner(*mut Buffer);
impl Drop for BufferOwner {
    fn drop(&mut self) {
        unsafe { align_rt_buffer_free(self.0) };
    }
}
#[test]
fn inclusive_cap_and_atomic_publication() -> TestResult {
    for (format, native) in [
        (
            DeflateFormat::Zlib,
            align_rt_compress_zlib_decompress as NativeDecode,
        ),
        (
            DeflateFormat::Raw,
            align_rt_compress_deflate_raw_decompress as NativeDecode,
        ),
    ] {
        for length in [0, 1, 31, 32, 33, 255, 256, 257] {
            let data = vec![0x61; length];
            let encoded =
                zlib_deflate(&data, 6, format, 7).map_err(|s| format!("compress status {s}"))?;
            for cap in [length.saturating_sub(1), length, length + 1] {
                for chunk in [1, 7, zlib_input_chunk()] {
                    assert_eq!(
                        decode(&encoded, format, cap, chunk),
                        if length <= cap {
                            Ok(data.clone())
                        } else {
                            Err(AL_INVALID)
                        }
                    );
                }
            }
        }
        let mut encoded = zlib_deflate(b"independent", 6, format, 1)
            .map_err(|s| format!("compress status {s}"))?;
        let size = i64::try_from(encoded.len())?;
        let mut output = BufferOwner(core::ptr::null_mut());
        assert_eq!(unsafe { native(encoded.as_ptr(), size, &mut output.0) }, 0);
        assert!(!output.0.is_null());
        encoded.fill(0xcc);
        drop(encoded);
        let buffer = unsafe { &*output.0 };
        assert_eq!(&buffer.data[..buffer.len], b"independent");
        let encoded = zlib_deflate(b"reject suffix", 6, format, 1)
            .map_err(|s| format!("compress status {s}"))?;
        for input in [
            vec![],
            encoded[..encoded.len() - 1].to_vec(),
            [encoded.as_slice(), &[0]].concat(),
        ] {
            // An old caller-owned handle remains separately owned; failure must clear the out slot.
            let mut published = output.0;
            assert_eq!(
                unsafe { native(input.as_ptr(), i64::try_from(input.len())?, &mut published) },
                AL_INVALID
            );
            assert!(published.is_null());
        }
        assert_eq!(
            unsafe {
                native(
                    encoded.as_ptr(),
                    i64::try_from(encoded.len())?,
                    core::ptr::null_mut(),
                )
            },
            AL_INVALID
        );
    }
    Ok(())
}
