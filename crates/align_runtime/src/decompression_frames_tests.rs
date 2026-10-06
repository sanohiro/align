//! Complete-input codec framing, aggregate limits and atomic publication.
use super::*;

type Decode = fn(&[u8]) -> Result<Vec<u8>, i32>;

// Independently generated/decoded with Python gzip(mtime=0) and zstd CLI --check.
// The tuples decode to empty, "left", "right", and binary 00 ff 80 41.
const GZIP: [&str; 4] = [
    "1f8b08000000000002ff03000000000000000000",
    "1f8b08000000000002ffcb494d2b010068e7677a04000000",
    "1f8b08000000000002ff2bca4ccf2801001475cab405000000",
    "1f8b08000000000002ff63f8dfe00800bc0284a504000000",
];
const ZSTD: [&str; 4] = [
    "28b52ffd240001000099e9d851",
    "28b52ffd04582100006c656674c89be648",
    "28b52ffd04582900007269676874923eb3be",
    "28b52ffd045821000000ff804115bb5a99",
];
// Independently authored v0.7 raw-block frame containing "A". Homebrew libzstd's
// legacy decoder successfully decoded it before these admission guards were added.
const LEGACY_ZSTD: &str = "27b52ffd200140000141c00000";

fn bytes(text: &str) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 {
        return Err("odd fixture length".into());
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let hi = char::from(pair[0])
                .to_digit(16)
                .ok_or("fixture high nibble")?;
            let lo = char::from(pair[1])
                .to_digit(16)
                .ok_or("fixture low nibble")?;
            u8::try_from(hi * 16 + lo).map_err(|error| error.to_string())
        })
        .collect()
}

fn frames(values: &[&str; 4]) -> Result<Vec<Vec<u8>>, String> {
    values.iter().map(|value| bytes(value)).collect()
}

#[test]
fn decompress_concatenation_preserves_every_payload_and_empty_member() -> Result<(), String> {
    for (decode, golden) in [(gzip_inflate as Decode, GZIP), (zstd_decompress_impl, ZSTD)] {
        let values = frames(&golden)?;
        for (indices, expected) in [
            (vec![0], vec![]),
            (vec![1], b"left".to_vec()),
            (vec![1, 2], b"leftright".to_vec()),
            (vec![0, 1, 0, 2, 0], b"leftright".to_vec()),
            (vec![0; 32], vec![]),
            (vec![1; 32], b"left".repeat(32)),
            (
                vec![3, 1, 3],
                [b"\0\xff\x80A".as_slice(), b"left", b"\0\xff\x80A"].concat(),
            ),
        ] {
            let input = indices
                .iter()
                .flat_map(|&index| values[index].iter().copied())
                .collect::<Vec<_>>();
            assert_eq!(decode(&input), Ok(expected), "members={indices:?}");
        }
    }
    Ok(())
}

#[test]
fn decompress_rejects_every_invalid_suffix_and_late_checksum() -> Result<(), String> {
    for (decode, golden, other) in [
        (gzip_inflate as Decode, GZIP, ZSTD),
        (zstd_decompress_impl, ZSTD, GZIP),
    ] {
        let values = frames(&golden)?;
        assert_eq!(decode(&[]), Err(AL_INVALID));
        for tail in [
            vec![0],
            vec![0xbe],
            b"not a frame".to_vec(),
            bytes(other[1])?,
        ] {
            assert_eq!(
                decode(&[values[1].as_slice(), tail.as_slice()].concat()),
                Err(AL_INVALID)
            );
        }
        for length in 1..values[2].len() {
            assert_eq!(
                decode(&[values[1].as_slice(), &values[2][..length]].concat()),
                Err(AL_INVALID),
                "suffix length={length}"
            );
        }
        for offset in [10, values[2].len() - 1] {
            let mut damaged = values[2].clone();
            damaged[offset] ^= 0x80;
            assert_eq!(
                decode(&[values[1].as_slice(), damaged.as_slice()].concat()),
                Err(AL_INVALID),
                "damage offset={offset}"
            );
        }
    }
    Ok(())
}

fn skippable(tag: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
    let mut frame = vec![tag, 0x2a, 0x4d, 0x18];
    frame.extend_from_slice(
        &u32::try_from(payload.len())
            .map_err(|e| e.to_string())?
            .to_le_bytes(),
    );
    frame.extend_from_slice(payload);
    Ok(frame)
}

#[test]
fn zstd_skippable_frames_and_legacy_rejection_are_independent_of_native_flags() -> Result<(), String>
{
    let ordinary = bytes(ZSTD[1])?;
    for tag in 0x50..=0x5f {
        for payload in [b"".as_slice(), b"metadata\0\xff"] {
            let skip = skippable(tag, payload)?;
            assert_eq!(zstd_decompress_impl(&skip), Ok(vec![]));
            assert_eq!(
                zstd_decompress_impl(
                    &[
                        skip.as_slice(),
                        ordinary.as_slice(),
                        skip.as_slice(),
                        ordinary.as_slice()
                    ]
                    .concat()
                ),
                Ok(b"leftleft".to_vec())
            );
            for length in 1..skip.len() {
                assert_eq!(
                    zstd_decompress_impl(&[ordinary.as_slice(), &skip[..length]].concat()),
                    Err(AL_INVALID)
                );
            }
        }
    }
    let legacy = bytes(LEGACY_ZSTD)?;
    assert_eq!(zstd_decompress_impl(&legacy), Err(AL_INVALID));
    assert_eq!(
        zstd_decompress_impl(&[ordinary.as_slice(), legacy.as_slice()].concat()),
        Err(AL_INVALID)
    );
    Ok(())
}

struct InflateOwner(Box<ZStream>);
impl InflateOwner {
    fn new() -> Result<Self, i32> {
        // zlib retains the stream address; box it before initialization and never move its pointee.
        let mut stream = Box::new(ZStream::zeroed());
        let status = unsafe {
            inflateInit2_(
                stream.as_mut(),
                GZIP_WINDOW_BITS,
                ZLIB_VERSION.as_ptr().cast(),
                c_int::try_from(core::mem::size_of::<ZStream>()).map_err(|_| AL_CODE)?,
            )
        };
        if status != Z_OK {
            return Err(zlib_error_to_status(status));
        }
        Ok(Self(stream))
    }
}
impl Drop for InflateOwner {
    fn drop(&mut self) {
        unsafe { inflateEnd(self.0.as_mut()) };
    }
}

struct ZstdOwner(*mut c_void);
impl ZstdOwner {
    fn new() -> Result<Self, i32> {
        let stream = Self(unsafe { ZSTD_createDStream() });
        if stream.0.is_null() {
            return Err(AL_CODE);
        }
        let status = unsafe { ZSTD_initDStream(stream.0) };
        if unsafe { ZSTD_isError(status) } != 0 {
            return Err(zstd_error_code(status));
        }
        Ok(stream)
    }
}
impl Drop for ZstdOwner {
    fn drop(&mut self) {
        unsafe { ZSTD_freeDStream(self.0) };
    }
}

fn limited(data: &[u8], cap: usize, gzip_chunk: Option<usize>) -> Result<Vec<u8>, i32> {
    if let Some(chunk) = gzip_chunk {
        let mut stream = InflateOwner::new()?;
        inflate_run_with_chunk(stream.0.as_mut(), data, cap, chunk)
    } else {
        let stream = ZstdOwner::new()?;
        zstd_decompress_stream(stream.0, data, cap)
    }
}

#[test]
fn decompress_aggregate_cap_is_inclusive_across_members_and_input_windows() -> Result<(), String> {
    for chunk in [Some(1), Some(2), Some(7), Some(31), Some(65_536), None] {
        let values = frames(if chunk.is_some() { &GZIP } else { &ZSTD })?;
        let pair = [values[1].as_slice(), values[2].as_slice()].concat();
        for cap in [8, 9, 10] {
            let expected = if cap < 9 {
                Err(AL_INVALID)
            } else {
                Ok(b"leftright".to_vec())
            };
            assert_eq!(
                limited(&pair, cap, chunk),
                expected,
                "cap={cap}, chunk={chunk:?}"
            );
        }
        let empty = values[0].repeat(3);
        assert_eq!(limited(&empty, 0, chunk), Ok(vec![]));
        assert_eq!(limited(&values[1], 0, chunk), Err(AL_INVALID));
        assert_eq!(
            limited(&[pair.as_slice(), empty.as_slice()].concat(), 9, chunk),
            Ok(b"leftright".to_vec())
        );
        assert_eq!(
            limited(&[pair.as_slice(), values[1].as_slice()].concat(), 9, chunk),
            Err(AL_INVALID)
        );
        for suffix in [vec![0], values[0][..values[0].len() - 1].to_vec()] {
            assert_eq!(
                limited(&[pair.as_slice(), suffix.as_slice()].concat(), 9, chunk),
                Err(AL_INVALID)
            );
        }
        if chunk.is_none() {
            let skip = skippable(0x5f, b"metadata")?;
            assert_eq!(limited(&skip, 0, chunk), Ok(vec![]));
            assert_eq!(
                limited(
                    &[pair.as_slice(), skip.as_slice(), empty.as_slice()].concat(),
                    9,
                    chunk
                ),
                Ok(b"leftright".to_vec())
            );
        }
        // Multiple output growths and a final buffered-output flush must not look like no progress.
        let plain = vec![b'A'; 100_003];
        let encoded = if chunk.is_some() {
            gzip_deflate(&plain, 6)
        } else {
            zstd_compress_impl(&plain, 3)
        }
        .map_err(|code| format!("compression status {code}"))?;
        assert_eq!(limited(&encoded, plain.len() - 1, chunk), Err(AL_INVALID));
        assert_eq!(limited(&encoded, plain.len(), chunk), Ok(plain));
    }
    assert_eq!(limited(&bytes(GZIP[0])?, 0, Some(0)), Err(AL_INVALID));
    if let Some(oversized) = usize::try_from(c_uint::MAX)
        .ok()
        .and_then(|n| n.checked_add(1))
    {
        assert_eq!(
            limited(&bytes(GZIP[0])?, 0, Some(oversized)),
            Err(AL_INVALID)
        );
    }
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
fn decompress_publishes_independent_complete_output_or_null() -> Result<(), String> {
    for (decode, golden) in [
        (align_rt_compress_gzip_decompress as NativeDecode, GZIP),
        (align_rt_compress_zstd_decompress as NativeDecode, ZSTD),
    ] {
        let values = frames(&golden)?;
        for (mut input, expected) in [
            (values[0].clone(), Some(b"".as_slice())),
            (
                [values[1].as_slice(), values[2].as_slice()].concat(),
                Some(b"leftright"),
            ),
            ([values[1].as_slice(), &[0]].concat(), None),
            (
                [values[1].as_slice(), &values[2][..values[2].len() - 1]].concat(),
                None,
            ),
        ] {
            let mut output = BufferOwner(core::ptr::null_mut());
            let size = i64::try_from(input.len()).map_err(|error| error.to_string())?;
            let status = unsafe { decode(input.as_ptr(), size, &mut output.0) };
            input.fill(0xcc);
            if let Some(expected) = expected {
                assert_eq!(status, 0);
                assert!(!output.0.is_null());
                let buffer = unsafe { &*output.0 };
                assert_eq!(&buffer.data[..buffer.len], expected);
            } else {
                assert_eq!(status, AL_INVALID);
                assert!(output.0.is_null(), "late error published partial output");
            }
        }
    }
    Ok(())
}
