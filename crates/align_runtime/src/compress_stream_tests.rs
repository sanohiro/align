//! File-backed windows, partial output, independent fixtures and format configuration.
use super::*;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::os::fd::{AsRawFd, FromRawFd};

type Transform = unsafe extern "C" fn(*mut Reader, *mut Writer, i64, *const u8, i64) -> i64;

fn file() -> std::io::Result<File> {
    let mut path = std::env::temp_dir()
        .join("align-codec-XXXXXX")
        .into_os_string()
        .into_encoded_bytes();
    path.push(0);
    let fd = unsafe { libc::mkstemp(path.as_mut_ptr().cast()) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    if unsafe { libc::unlink(path.as_ptr().cast()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

fn run(
    op: Transform,
    input: &[u8],
    first_window: usize,
    number: i64,
    dictionary: &[u8],
    buffered: bool,
) -> (i64, Vec<u8>) {
    let mut source = file().unwrap();
    source.write_all(&input[first_window..]).unwrap();
    source.rewind().unwrap();
    let mut target = file().unwrap();
    let mut reader = Reader::unbuffered(source.as_raw_fd(), false);
    if first_window > 0 {
        reader.buffered = true;
        reader.buf.reserve_exact(READ_LINE_CHUNK);
        reader.buf.extend_from_slice(&input[..first_window]);
        reader.filled = first_window;
    }
    let mut writer = Writer::generic_fd(target.as_raw_fd(), false, buffered);
    if buffered {
        writer.buf.push(b'*');
    }
    let status = unsafe {
        op(
            &mut reader,
            &mut writer,
            number,
            dictionary.as_ptr(),
            dictionary.len().try_into().unwrap(),
        )
    };
    if buffered {
        // The caller's prefix stays ahead of codec bytes; small outputs remain buffered until flush.
        if status >= 0 && status < 1000 {
            assert_eq!(target.stream_position().unwrap(), 0);
        }
        assert_eq!(unsafe { align_rt_io_writer_flush(&mut writer) }, 0);
    }
    target.rewind().unwrap();
    let mut output = Vec::new();
    target.read_to_end(&mut output).unwrap();
    if buffered {
        assert_eq!(output.remove(0), b'*');
    }
    (status, output)
}

#[test]
fn stream_independent_frames_at_every_split_and_limit() {
    for (decoder, reference, concatenated) in [
        (
            align_rt_compress_gzip_decompress_to as Transform,
            GZIP[1],
            true,
        ),
        (
            align_rt_compress_zlib_decompress_to,
            "789ccb494d2b0100042301ac",
            false,
        ),
        (
            align_rt_compress_deflate_raw_decompress_to,
            "cb494d2b0100",
            false,
        ),
        (align_rt_compress_zstd_decompress_to, ZSTD[1], true),
    ] {
        let frame = bytes(reference).unwrap();
        let mut input = frame.clone();
        input.extend_from_slice(&frame);
        for split in 0..=input.len() {
            let (status, output) = run(decoder, &input, split, 8, &[], true);
            assert_eq!(
                (status, output.as_slice()),
                if concatenated {
                    (8, &b"leftleft"[..])
                } else {
                    (-i64::from(AL_INVALID), &b"left"[..])
                },
                "split {split}"
            );
        }
        for split in 0..=frame.len() {
            assert_eq!(
                run(decoder, &frame, split, 4, &[], false),
                (4, b"left".to_vec())
            );
            assert_eq!(
                run(decoder, &frame, split, 3, &[], false),
                (-i64::from(AL_INVALID), b"lef".to_vec())
            );
        }
        for length in 0..frame.len() {
            assert!(run(decoder, &frame[..length], 0, 4, &[], false).0 < 0);
        }
        let mut bad = frame.clone();
        bad.push(0);
        assert_eq!(
            run(decoder, &bad, frame.len(), 4, &[], false),
            (-i64::from(AL_INVALID), b"left".to_vec())
        );
    }
    // A bad checksum in a later member cannot retract the completed first member.
    for (decoder, reference) in [
        (align_rt_compress_gzip_decompress_to as Transform, GZIP),
        (align_rt_compress_zstd_decompress_to, ZSTD),
    ] {
        let first = bytes(reference[1]).unwrap();
        let mut corrupt = bytes(reference[2]).unwrap();
        let footer = corrupt.len() - if reference == GZIP { 8 } else { 4 };
        corrupt[footer] ^= 0x40;
        let pair = [first.as_slice(), &corrupt].concat();
        let (status, output) = run(decoder, &pair, first.len(), 99, &[], true);
        assert_eq!(status, -i64::from(AL_INVALID));
        assert!(output.starts_with(b"left"));
    }
    let empty = bytes(ZSTD[0]).unwrap();
    let skip = skippable(0x5f, b"discard").unwrap();
    let mixed = [empty.as_slice(), &skip, &empty].concat();
    for split in 0..=mixed.len() {
        assert_eq!(
            run(
                align_rt_compress_zstd_decompress_to,
                &mixed,
                split,
                0,
                &[],
                true
            ),
            (0, vec![])
        );
    }
    assert!(
        run(
            align_rt_compress_zstd_decompress_to,
            &bytes(LEGACY_ZSTD).unwrap(),
            2,
            9,
            &[],
            false
        )
        .0 < 0
    );
}

#[test]
fn stream_encoders_and_dictionary_interoperate_with_independent_decoders() {
    let trained = bytes(TRAINED_DICTIONARY).unwrap();
    let message = b"observation|temperature|station|\0\xff\n".repeat(3);
    for (format, encode, decode, dictionary, reference) in [
        (
            Some(DeflateFormat::Gzip),
            align_rt_compress_gzip_compress_to as Transform,
            align_rt_compress_gzip_decompress_to as Transform,
            &[][..],
            "",
        ),
        (
            Some(DeflateFormat::Zlib),
            align_rt_compress_zlib_compress_to,
            align_rt_compress_zlib_decompress_to,
            WEATHER,
            "78f9e59918e6a39b1200414f2c8c",
        ),
        (
            Some(DeflateFormat::Raw),
            align_rt_compress_deflate_raw_compress_to,
            align_rt_compress_deflate_raw_decompress_to,
            WEATHER,
            "a39b1200",
        ),
        (
            None,
            align_rt_compress_zstd_compress_to,
            align_rt_compress_zstd_decompress_to,
            &trained[..],
            TRAINED_FRAME,
        ),
        (
            None,
            align_rt_compress_zstd_compress_to,
            align_rt_compress_zstd_decompress_to,
            WEATHER,
            RAW_ZSTD_FRAME,
        ),
    ] {
        if !reference.is_empty() {
            let vector = bytes(reference).unwrap();
            for split in 0..=vector.len() {
                assert_eq!(
                    run(decode, &vector, split, 105, dictionary, true),
                    (105, message.clone())
                );
            }
        }
        // Include compressible input larger than several output windows and incompressible windows.
        let mut seed = 1u32;
        let binary: Vec<u8> = (0..3 * READ_LINE_CHUNK + 17)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed.to_be_bytes()[0]
            })
            .collect();
        for value in [
            &[][..],
            &message,
            &vec![0x5b; READ_LINE_CHUNK * 5 + 3],
            &binary,
        ] {
            let (count, packed) = run(encode, value, value.len().min(5), 6, dictionary, false);
            assert_eq!(usize::try_from(count).unwrap(), packed.len());
            let decoded = match format {
                Some(kind) => {
                    zlib_inflate_with_dictionary(&packed, kind, value.len(), 11, dictionary)
                }
                None => zstd_decompress_with_dictionary(&packed, value.len(), dictionary),
            }
            .unwrap();
            assert_eq!(decoded, value);
            let cap = i64::try_from(value.len()).unwrap();
            assert_eq!(
                run(decode, &packed, 1, cap, dictionary, false),
                (cap, value.to_vec())
            );
        }
    }
}

#[test]
fn stream_read_fault_drains_output_and_setup_failure_preserves_cursor() {
    let clear = vec![42; READ_LINE_CHUNK * 5];
    let packed = zstd_compress_impl(&clear, 2).unwrap();
    let mut reader = Reader::unbuffered(-1, false);
    reader.buffered = true;
    reader.buf = Vec::with_capacity(READ_LINE_CHUNK);
    reader.buf.extend_from_slice(&packed);
    reader.filled = packed.len();
    let mut target = file().unwrap();
    let mut writer = Writer::generic_fd(target.as_raw_fd(), false, false);
    let malformed = [0x37, 0xa4, 0x30, 0xec, 1, 2, 3, 4];
    for (op, number, ptr, len) in [
        (
            align_rt_compress_zstd_decompress_to as Transform,
            18,
            malformed.as_ptr(),
            8,
        ),
        (align_rt_compress_gzip_compress_to, 3, malformed.as_ptr(), 1),
        (
            align_rt_compress_zlib_compress_to,
            3,
            malformed.as_ptr(),
            i64::from(u32::MAX) + 1,
        ),
        (
            align_rt_compress_zstd_decompress_to,
            18,
            std::ptr::null(),
            1,
        ),
    ] {
        assert_eq!(
            unsafe { op(&mut reader, &mut writer, number, ptr, len) },
            -i64::from(AL_INVALID)
        );
        assert_eq!(reader.start, 0);
        assert_eq!(target.stream_position().unwrap(), 0);
    }
    assert_eq!(
        unsafe {
            align_rt_compress_zstd_decompress_to(
                &mut reader,
                &mut writer,
                i64::MAX,
                std::ptr::null(),
                0,
            )
        },
        -i64::from(io_read_write_status(&std::io::Error::from_raw_os_error(
            libc::EBADF
        )))
    );
    target.rewind().unwrap();
    let mut delivered = Vec::new();
    target.read_to_end(&mut delivered).unwrap();
    assert_eq!(delivered, clear);
}
#[cfg(feature = "alloc-count")]
#[test]
fn stream_wrapper_payload_allocation_is_independent_of_total_size() {
    for buffered in [false, true] {
        for size in [1, 16 * 1024 * 1024] {
            let mut input = file().unwrap();
            input.set_len(size).unwrap();
            input.rewind().unwrap();
            let output = std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/null")
                .unwrap();
            let mut writer = Writer::generic_fd(output.as_raw_fd(), false, false);
            let mut reader = Reader::unbuffered(input.as_raw_fd(), false);
            if buffered {
                reader.buffered = true;
                reader.buf.reserve_exact(READ_LINE_CHUNK);
            }
            let before = global_alloc_bytes();
            let count = unsafe {
                align_rt_compress_gzip_compress_to(
                    &mut reader,
                    &mut writer,
                    1,
                    core::ptr::null(),
                    0,
                )
            };
            let bytes = global_alloc_bytes() - before;
            assert!(count > 0);
            assert_eq!(
                bytes,
                if buffered {
                    READ_LINE_CHUNK
                } else {
                    2 * READ_LINE_CHUNK
                }
            );
        }
    }
}
