//! Ordinary path-based directory operations; no retained authority or recursive work.
use super::{AL_INVALID, io_error_to_status};

fn extent(path: *const u8, len: i64) -> Result<usize, i32> {
    let length = usize::try_from(len)
        .ok()
        .filter(|n| *n > 0 && *n <= isize::MAX.unsigned_abs())
        .ok_or(AL_INVALID)?;
    if path.is_null() || path.addr().checked_add(length).is_none() {
        return Err(AL_INVALID);
    }
    Ok(length)
}
unsafe fn text<'a>(path: *const u8, length: usize) -> Result<&'a str, i32> {
    // The caller certified the address extent and supplies a live immutable allocation.
    let bytes = unsafe { core::slice::from_raw_parts(path, length) };
    if bytes.contains(&0) {
        return Err(AL_INVALID);
    }
    core::str::from_utf8(bytes).map_err(|_| AL_INVALID)
}

/// Create exactly one directory, using ordinary OS path resolution and umask.
/// # Safety
/// The positive path range must be readable and immutable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_create_dir(path: *const u8, len: i64) -> i32 {
    let result = (|| {
        let length = extent(path, len)?;
        let path = unsafe { text(path, length) }?;
        std::fs::create_dir(path).map_err(|error| io_error_to_status(&error))
    })();
    result.err().unwrap_or(0)
}

/// Observe followed metadata. False means a successfully observed non-directory.
/// # Safety
/// Path must be readable and immutable; out must be a disjoint exclusive writable byte.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_is_dir(path: *const u8, len: i64, out: *mut u8) -> i32 {
    if out.is_null() || out.addr().checked_add(1).is_none() {
        return AL_INVALID;
    }
    let length = match extent(path, len) {
        Ok(length) => length,
        Err(error) => return error,
    };
    if out.addr() >= path.addr() && out.addr() - path.addr() < length {
        return AL_INVALID;
    }
    unsafe { out.write(0) };
    let result = (|| {
        let path = unsafe { text(path, length) }?;
        std::fs::metadata(path).map_err(|error| io_error_to_status(&error))
    })();
    match result {
        Ok(metadata) => {
            unsafe { out.write(u8::from(metadata.is_dir())) };
            0
        }
        Err(status) => status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_paths_before_io() {
        for (pointer, len) in [
            (core::ptr::null(), 1),
            (b"x".as_ptr(), -1),
            (b"x".as_ptr(), 0),
            (usize::MAX as *const u8, 1),
        ] {
            assert_eq!(unsafe { align_rt_fs_create_dir(pointer, len) }, AL_INVALID);
            let mut output = 99;
            assert_eq!(
                unsafe { align_rt_fs_is_dir(pointer, len, &mut output) },
                AL_INVALID
            );
            assert_eq!(output, 99);
        }
        for bytes in [b"x\0y".as_slice(), b"\xff"] {
            let length = i64::try_from(bytes.len()).unwrap_or(0);
            assert_eq!(
                unsafe { align_rt_fs_create_dir(bytes.as_ptr(), length) },
                AL_INVALID
            );
            let mut output = 99;
            assert_eq!(
                unsafe { align_rt_fs_is_dir(bytes.as_ptr(), length, &mut output) },
                AL_INVALID
            );
            assert_eq!(output, 0);
        }
    }
    #[test]
    fn query_out_contract() {
        let mut output = 99;
        assert_eq!(
            unsafe { align_rt_fs_is_dir(core::ptr::null(), 1, &mut output) },
            AL_INVALID
        );
        assert_eq!(output, 99);
        let mut overlapping = *b"path";
        assert_eq!(
            unsafe { align_rt_fs_is_dir(overlapping.as_ptr(), 4, overlapping.as_mut_ptr()) },
            AL_INVALID
        );
        assert_eq!(overlapping, *b"path");
        assert_eq!(
            unsafe { align_rt_fs_is_dir(b"x\0y".as_ptr(), 3, &mut output) },
            AL_INVALID
        );
        assert_eq!(output, 0);
        assert_eq!(
            unsafe { align_rt_fs_is_dir(b".".as_ptr(), 1, core::ptr::null_mut()) },
            AL_INVALID
        );
        assert_eq!(
            unsafe { align_rt_fs_is_dir(b".".as_ptr(), 1, usize::MAX as *mut u8) },
            AL_INVALID
        );
    }
}
