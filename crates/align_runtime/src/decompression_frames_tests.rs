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
        inflate_run_with_chunk(stream.0.as_mut(), data, cap, chunk, true, &[])
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

type NativeDecode = unsafe extern "C" fn(*const u8, i64, i64, *mut *mut Buffer) -> i32;
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
            let status = unsafe { decode(input.as_ptr(), size, 1 << 30, &mut output.0) };
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

type PresetCodec = unsafe extern "C" fn(*const u8, i64, i64, *const u8, i64, *mut *mut Buffer) -> i32;

fn preset_call(op: PresetCodec, input: &[u8], number: i64, dictionary: &[u8]) -> Result<Vec<u8>, i32> {
    let sentinel = core::ptr::dangling_mut();
    let mut output = sentinel;
    let status = unsafe { op(input.as_ptr(), input.len() as i64, number, dictionary.as_ptr(), dictionary.len() as i64, &mut output) };
    if status != 0 {
        assert!(output.is_null(), "failure must clear a previously non-null result slot");
        return Err(status);
    }
    assert!(!output.is_null() && output != sentinel);
    let owner = BufferOwner(output);
    let value = unsafe { &*owner.0 };
    Ok(value.data[..value.len].to_vec())
}

// Reference frames from Python zlib.compressobj(zdict=...) and ZSTD_compress_usingDict.
// ZDICT_trainFromBuffer produced this 256-byte dictionary from 200 weather records.
const TRAINED_DICTIONARY: &str = concat!(
    "37a430ec08ef185b1610f06c1c000000bc90524a29656a43efbdf7de7bef1d73",
    "07000020c00d0700000004000406404b844e4901d0a3d013510c000080400000",
    "b802080000188101180000000000f46fcb1b0000000000000000000000000000",
    "000100000004000000080000000a73746174696f6e3d3030323b2074656d7065",
    "7261747572653d3035313b2068756d69646974793d3032313b2077696e643d0a",
    "73746174696f6e3d3031333b2074656d70657261747572653d3034353b206875",
    "6d69646974793d3031353b2077696e643d0a73746174696f6e3d3031313b2074",
    "656d70657261747572653d3030393b2068756d69646974793d3037393b207769",
);
const TRAINED_FRAME: &str = "28b52ffd2308ef185b69d50000606f62736572767c7c7c00ff0a040063145112dc41155915141c";
const RAW_ZSTD_FRAME: &str = "28b52ffd20693d0000000100c6a40a08";
const WEATHER: &[u8] = b"humidity|pressure|wind|observation|temperature|station|\0\xff\n";

#[test]
fn preset_codecs_read_independent_frames_and_publish_atomically() -> Result<(), String> {
    let expected = b"observation|temperature|station|\0\xff\n".repeat(3);
    let trained = bytes(TRAINED_DICTIONARY)?;
    for (encode, decode, frame, dictionary) in [
        (align_rt_compress_zlib_compress_dict as PresetCodec, align_rt_compress_zlib_decompress_dict as PresetCodec, "78f9e59918e6a39b1200414f2c8c", WEATHER),
        (align_rt_compress_deflate_raw_compress_dict, align_rt_compress_deflate_raw_decompress_dict, "a39b1200", WEATHER),
        (align_rt_compress_zstd_compress_dict, align_rt_compress_zstd_decompress_dict, RAW_ZSTD_FRAME, WEATHER),
        (align_rt_compress_zstd_compress_dict, align_rt_compress_zstd_decompress_dict, TRAINED_FRAME, trained.as_slice()),
    ] {
        let frame = bytes(frame)?;
        let n = expected.len() as i64;
        assert_eq!(preset_call(decode, &frame, n, dictionary), Ok(expected.clone()));
        assert_eq!(preset_call(decode, &frame, n - 1, dictionary), Err(AL_INVALID));
        assert_eq!(preset_call(decode, &frame[..frame.len() - 1], n, dictionary), Err(AL_INVALID));
        for payload in [expected.as_slice(), &[]] {
            for dict in [dictionary, &[]] {
                let compressed = preset_call(encode, payload, 6, dict).map_err(|s| s.to_string())?;
                assert_eq!(preset_call(decode, &compressed, payload.len() as i64, dict), Ok(payload.to_vec()));
            }
        }
        let trailing = [frame.as_slice(), &[0x60]].concat();
        assert_eq!(preset_call(decode, &trailing, n, dictionary), Err(AL_INVALID));
    }
    let frame = bytes("78f9e59918e6a39b1200414f2c8c")?;
    for missing in [&[][..], b"wrong dictionary here"] {
        assert_eq!(preset_call(align_rt_compress_zlib_decompress_dict, &frame, 105, missing), Err(AL_INVALID));
    }
    let raw = bytes("a39b1200")?;
    let wrong = vec![b'X'; WEATHER.len()];
    assert_eq!(preset_call(align_rt_compress_deflate_raw_decompress_dict, &raw, 105, &wrong), Ok(vec![b'X'; 105]));
    let ordinary = bytes("789ccb494d2b0100042301ac")?;
    assert_eq!(preset_call(align_rt_compress_zlib_decompress_dict, &ordinary, 4, WEATHER), Ok(b"left".to_vec()));
    for chunk in [1, 2, 4] {
        assert_eq!(zlib_inflate_with_dictionary(&frame, DeflateFormat::Zlib, 105, chunk, WEATHER), Ok(expected.clone()));
    }
    Ok(())
}

#[test]
fn preset_zstd_validates_dictionary_before_frames_and_keeps_dictionary_across_frames() -> Result<(), String> {
    let decode = align_rt_compress_zstd_decompress_dict;
    let encode = align_rt_compress_zstd_compress_dict;
    let dictionary = bytes(TRAINED_DICTIONARY)?;
    let frame = bytes(TRAINED_FRAME)?;
    let skip = bytes("502a4d1800000000")?;
    for length in [8, 16, 63] {
        assert_eq!(preset_call(decode, &skip, 0, &dictionary[..length]), Err(AL_INVALID));
        assert_eq!(preset_call(encode, b"abc", 3, &dictionary[..length]), Err(AL_INVALID));
    }
    assert_eq!(preset_call(decode, &skip, 0, &dictionary[..7]), Ok(vec![]));
    for dict in [b"1234567".as_slice(), b"12345678"] {
        let compressed = preset_call(encode, b"12345678", 6, dict).map_err(|s| s.to_string())?;
        assert_eq!(preset_call(decode, &compressed, 8, dict), Ok(b"12345678".to_vec()));
    }
    let payload = b"observation|temperature|station|\0\xff\n".repeat(3);
    let joined = [frame.as_slice(), &skip, &frame].concat();
    assert_eq!(preset_call(decode, &joined, 210, &dictionary), Ok(payload.repeat(2)));
    let suffix = [frame.as_slice(), &skip].concat();
    assert_eq!(preset_call(decode, &suffix, 105, &dictionary), Ok(payload.clone()));
    assert_eq!(preset_call(decode, &frame, 105, WEATHER), Err(AL_INVALID));
    let mut wrong_id = frame.clone();
    wrong_id[5] ^= 1;
    assert_eq!(preset_call(decode, &[frame.as_slice(), &wrong_id].concat(), 105, &dictionary), Err(AL_INVALID));
    Ok(())
}

#[test]
fn preset_libz_rejects_unrepresentable_dictionary_before_access() {
    for codec in [align_rt_compress_zlib_compress_dict as PresetCodec, align_rt_compress_zlib_decompress_dict,
        align_rt_compress_deflate_raw_compress_dict, align_rt_compress_deflate_raw_decompress_dict] {
        let mut output = core::ptr::dangling_mut();
        let status = unsafe { codec(core::ptr::null(), 0, 6, core::ptr::dangling(), i64::from(u32::MAX) + 1, &mut output) };
        assert_eq!(status, AL_INVALID);
        assert!(output.is_null());
    }
}
