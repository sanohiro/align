//! R156: public caller limits, measured payload growth and atomic publication.
use super::*;
use std::cell::Cell;

#[derive(Clone, Copy, Default)]
struct Growth {
    calls: usize,
    requested: usize,
    capacity: usize,
}
thread_local! {
    static GROWTH: Cell<Option<Growth>> = const { Cell::new(None) };
}
pub(super) fn observe_growth(requested: usize, capacity: usize) {
    GROWTH.with(|cell| {
        if let Some(mut growth) = cell.get() {
            growth.calls += 1;
            growth.requested = growth.requested.max(requested);
            growth.capacity = growth.capacity.max(capacity);
            cell.set(Some(growth));
        }
    });
}
struct GrowthOwner;
impl GrowthOwner {
    fn start() -> Self {
        GROWTH.with(|cell| {
            assert!(cell.get().is_none());
            cell.set(Some(Growth::default()));
        });
        Self
    }
    fn finish(self) -> Result<Growth, String> {
        GROWTH.with(|cell| {
            cell.get()
                .ok_or_else(|| "growth observation inactive".into())
        })
    }
}
impl Drop for GrowthOwner {
    fn drop(&mut self) {
        GROWTH.with(|cell| cell.set(None));
    }
}
struct Output(*mut Buffer);
impl Drop for Output {
    fn drop(&mut self) {
        unsafe { align_rt_buffer_free(self.0) };
    }
}
type Decode = unsafe extern "C" fn(*const u8, i64, i64, *mut *mut Buffer) -> i32;
const DECODERS: [Decode; 4] = [
    align_rt_compress_gzip_decompress,
    align_rt_compress_zlib_decompress,
    align_rt_compress_deflate_raw_decompress,
    align_rt_compress_zstd_decompress,
];
fn encode(codec: usize, data: &[u8]) -> Result<Vec<u8>, String> {
    match codec {
        0 => gzip_deflate(data, 6),
        1 => zlib_deflate(data, 6, DeflateFormat::Zlib, zlib_input_chunk()),
        2 => zlib_deflate(data, 6, DeflateFormat::Raw, zlib_input_chunk()),
        _ => zstd_compress_impl(data, 6),
    }
    .map_err(|status| format!("encode {codec}: {status}"))
}
fn decoded(
    codec: usize,
    data: &[u8],
    cap: usize,
    expected: Option<&[u8]>,
) -> Result<Growth, String> {
    let length = i64::try_from(data.len()).map_err(|error| error.to_string())?;
    let limit = i64::try_from(cap).map_err(|error| error.to_string())?;
    let mut output = Output(core::ptr::null_mut());
    let observation = GrowthOwner::start();
    let status = unsafe { DECODERS[codec](data.as_ptr(), length, limit, &mut output.0) };
    let growth = observation.finish()?;
    assert!(
        growth.requested <= cap,
        "codec={codec} cap={cap} requested={}",
        growth.requested
    );
    // Vec's allocator may round its physical allocation, but its exposed capacity and every
    // requested layout on this runtime stay within the caller's payload limit.
    assert!(
        growth.capacity <= cap,
        "codec={codec} cap={cap} capacity={}",
        growth.capacity
    );
    if let Some(expected) = expected {
        assert_eq!(status, 0, "codec={codec} cap={cap}");
        assert!(!output.0.is_null());
        let buffer = unsafe { &*output.0 };
        assert_eq!(&buffer.data[..buffer.len], expected);
    } else {
        assert_eq!(status, AL_INVALID, "codec={codec} cap={cap}");
        assert!(output.0.is_null(), "partial output escaped");
    }
    Ok(growth)
}

#[test]
fn caller_limits_bound_actual_output_growth_and_publication() -> Result<(), String> {
    for codec in 0..DECODERS.len() {
        for length in [0, 1, 31, 65_535, 65_536, 65_537, 262_145] {
            let payload = vec![0x61; length];
            let compressed = encode(codec, &payload)?;
            for cap in [0, 1, length.saturating_sub(1), length, length + 1, 1 << 30] {
                let expected = (length <= cap).then_some(payload.as_slice());
                let growth = decoded(codec, &compressed, cap, expected)?;
                if cap == 0 {
                    assert_eq!(growth.calls, 0);
                }
                if length > 0 && cap > 0 {
                    assert!(growth.calls > 0, "inactive growth probe");
                }
                if length > 65_536 && cap >= length {
                    assert!(growth.calls > 1);
                }
                if length == 1 && cap == 1 << 30 {
                    assert!(
                        growth.requested <= GZIP_OUT_CHUNK,
                        "allocated full declared limit"
                    );
                }
            }
        }
        let compressed = encode(codec, b"abc")?;
        for cap in [0, 2, 3, 4] {
            decoded(codec, &[], cap, None)?;
            decoded(codec, &compressed[..compressed.len() - 1], cap, None)?;
            decoded(codec, &[compressed.as_slice(), &[0]].concat(), cap, None)?;
        }
    }
    Ok(())
}

#[test]
fn caller_limit_covers_all_members_and_metadata_tails() -> Result<(), String> {
    for codec in [0, 3] {
        let first = encode(codec, b"ab")?;
        let second = encode(codec, b"c")?;
        let empty = encode(codec, b"")?;
        let joined = [first.as_slice(), second.as_slice(), empty.as_slice()].concat();
        decoded(codec, &joined, 2, None)?;
        decoded(codec, &joined, 3, Some(b"abc"))?;
        decoded(codec, &joined, 4, Some(b"abc"))?;
        decoded(codec, &empty.repeat(2), 0, Some(b""))?;
        decoded(
            codec,
            &[joined.as_slice(), first.as_slice()].concat(),
            3,
            None,
        )?;
        decoded(
            codec,
            &[joined.as_slice(), &empty[..empty.len() - 1]].concat(),
            3,
            None,
        )?;
        if codec == 3 {
            let skip = [0x50, 0x2a, 0x4d, 0x18, 1, 0, 0, 0, 42];
            decoded(codec, &skip, 0, Some(b""))?;
            decoded(
                codec,
                &[joined.as_slice(), skip.as_slice()].concat(),
                3,
                Some(b"abc"),
            )?;
        }
    }
    Ok(())
}
