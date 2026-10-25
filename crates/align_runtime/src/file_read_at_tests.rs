//! Plan244: native initialized window admission, bounded mutation and allocation parity.
use super::*;
use std::cell::Cell;
use std::io::{Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;

#[test]
fn file_read_at_native_matrix() -> Result<(), Box<dyn std::error::Error>> {
    let _: unsafe extern "C" fn(*const RwFile, i64, *mut u8, i64) -> i64 = align_rt_io_file_read_at;
    let dir = tests::FileFixtureDir::new("read-at");
    let path = dir.0.join("input");
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)?;
    file.write_all(b"01234567")?;
    file.seek(SeekFrom::Start(3))?;
    let handle = RwFile {
        fd: file.as_raw_fd(),
    };
    for (offset, length, expected) in [
        (2, 4, 4),
        (6, 4, 2),
        (8, 4, 0),
        (0, 0, 0),
        (-1, 0, -i64::from(AL_INVALID)),
    ] {
        let mut bytes = *b"ABCDEFGH";
        #[cfg(feature = "alloc-count")]
        let before = (global_alloc_count(), GLOBAL_DEALLOC_CALLS.with(Cell::get));
        let count =
            unsafe { align_rt_io_file_read_at(&handle, offset, bytes.as_mut_ptr().add(2), length) };
        #[cfg(feature = "alloc-count")]
        assert_eq!(
            (global_alloc_count(), GLOBAL_DEALLOC_CALLS.with(Cell::get)),
            before
        );
        assert_eq!(count, expected);
        let mut wanted = *b"ABCDEFGH";
        if count > 0 {
            let count = usize::try_from(count)?;
            let start = usize::try_from(offset)?;
            wanted[2..2 + count].copy_from_slice(&b"01234567"[start..start + count]);
        }
        assert_eq!(bytes, wanted);
        assert_eq!(file.stream_position()?, 3);
    }
    let mut calls = 0;
    let mut bytes = [9; 8];
    let count = unsafe {
        file_read_at_with(
            &handle,
            0,
            bytes.as_mut_ptr().add(2),
            4,
            |fd, destination, length, offset| {
                assert_eq!((fd, length, offset), (handle.fd, 4, 0));
                calls += 1;
                if calls == 1 {
                    return Err(std::io::Error::from_raw_os_error(libc::EINTR));
                }
                destination.write(3);
                Ok(1)
            },
        )
    };
    assert_eq!((count, calls, bytes), (1, 2, [9, 9, 3, 9, 9, 9, 9, 9]));
    let count = unsafe {
        file_read_at_with(
            &handle,
            0,
            bytes.as_mut_ptr().add(2),
            4,
            |_, destination, _, _| {
                // Native errors have no rollback promise, but may not escape the supplied window.
                destination.write(4);
                Err(std::io::Error::from_raw_os_error(libc::EIO))
            },
        )
    };
    assert_eq!(count, -i64::from(AL_CODE + libc::EIO));
    assert_eq!(bytes, [9, 9, 4, 9, 9, 9, 9, 9]);
    assert_eq!(
        unsafe { file_read_at_with(&handle, 0, bytes.as_mut_ptr(), 4, |_, _, _, _| Ok(5)) },
        -i64::from(AL_INVALID)
    );
    Ok(())
}

#[test]
fn file_read_at_admission_precedence() {
    let mut handle = RwFile { fd: 7 };
    let file = &mut handle as *mut RwFile;
    let mut bytes = [99; 8];
    let byte = bytes.as_mut_ptr();
    let overflow = usize::MAX & !(core::mem::align_of::<RwFile>() - 1);
    for (source, offset, destination, length) in [
        (core::ptr::null(), 0, byte, 0),
        (
            file.cast::<u8>().wrapping_add(1).cast_const().cast(),
            0,
            byte,
            0,
        ),
        (
            core::ptr::without_provenance::<RwFile>(overflow),
            0,
            byte,
            0,
        ),
        (file.cast_const(), -1, byte, 0),
        (file.cast_const(), 0, byte, -1),
        (file.cast_const(), 0, core::ptr::null_mut(), 1),
        (
            file.cast_const(),
            0,
            core::ptr::without_provenance_mut::<u8>(usize::MAX),
            1,
        ),
        (file.cast_const(), 0, file.cast::<u8>(), 1),
        (
            file.cast_const(),
            0,
            file.cast::<u8>()
                .wrapping_add(core::mem::size_of::<RwFile>() - 1),
            1,
        ),
    ] {
        let calls = Cell::new(0);
        let count = unsafe {
            file_read_at_with(source, offset, destination, length, |_, _, _, _| {
                calls.set(calls.get() + 1);
                Ok(0)
            })
        };
        assert_eq!(
            (count, calls.get(), bytes, handle.fd),
            (-i64::from(AL_INVALID), 0, [99; 8], 7)
        );
    }
    handle.fd = -1;
    assert_eq!(
        unsafe {
            file_read_at_with(&handle, 0, byte, 0, |_, _, _, _| {
                panic!("negative fd reached pread")
            })
        },
        -i64::from(AL_INVALID)
    );
    handle.fd = 7;
    assert_eq!(
        unsafe {
            file_read_at_with(&handle, 0, core::ptr::null_mut(), 0, |_, _, _, _| {
                panic!("empty window reached pread")
            })
        },
        0
    );
}
