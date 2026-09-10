//! Incremental SHA-256 owns one EVP context and never retains update inputs.

use crate::{AlignStr, align_rt_alloc, panic_abort};
use core::ffi::{c_int, c_uint, c_void};

unsafe extern "C" {
    fn EVP_MD_CTX_new() -> *mut c_void;
    fn EVP_MD_CTX_free(context: *mut c_void);
    fn EVP_sha256() -> *const c_void;
    fn EVP_DigestInit_ex(context: *mut c_void, kind: *const c_void, engine: *mut c_void) -> c_int;
    fn EVP_DigestUpdate(context: *mut c_void, data: *const c_void, len: usize) -> c_int;
    fn EVP_DigestFinal_ex(context: *mut c_void, output: *mut u8, len: *mut c_uint) -> c_int;
}

const MAX_BYTES: u64 = (1_u64 << 61) - 1;

#[cfg(test)]
thread_local! {
    static LIVE_CONTEXTS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    static FAIL_PROVIDER: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

struct Digest {
    context: *mut c_void,
    bytes: u64,
}

impl Drop for Digest {
    fn drop(&mut self) {
        // SAFETY: this shell exclusively owns the context acquired by its constructor.
        unsafe { EVP_MD_CTX_free(self.context) };
        #[cfg(test)]
        LIVE_CONTEXTS.with(|live| live.set(live.get() - 1));
    }
}

fn require_handle(handle: *mut c_void) -> *mut Digest {
    if handle.is_null()
        || !handle
            .addr()
            .is_multiple_of(core::mem::align_of::<Digest>())
    {
        panic_abort("crypto.digest: invalid owner");
    }
    handle.cast()
}

fn next_length(current: u64, added: usize) -> Option<u64> {
    current
        .checked_add(u64::try_from(added).ok()?)
        .filter(|n| *n <= MAX_BYTES)
}

/// Create one independently owned SHA-256 context.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_crypto_digest_new() -> *mut c_void {
    let digest = create_digest().unwrap_or_else(|error| panic_abort(error));
    Box::into_raw(digest).cast()
}

fn create_digest() -> Result<Box<Digest>, &'static str> {
    // SAFETY: EVP takes no caller pointers and returns an owned context or null.
    let context = unsafe { EVP_MD_CTX_new() };
    if context.is_null() {
        return Err("crypto.digest: context allocation failed");
    }
    let digest = Box::new(Digest { context, bytes: 0 });
    #[cfg(test)]
    LIVE_CONTEXTS.with(|live| live.set(live.get() + 1));
    #[cfg(test)]
    if FAIL_PROVIDER.with(core::cell::Cell::get) {
        return Err("crypto.digest: initialization failed");
    }
    // SAFETY: context is live and exclusively owned; EVP_sha256 is the fixed algorithm descriptor.
    if unsafe { EVP_DigestInit_ex(context, EVP_sha256(), core::ptr::null_mut()) } != 1 {
        drop(digest);
        return Err("crypto.digest: initialization failed");
    }
    Ok(digest)
}

/// Borrow an exclusive digest and a nonretained byte input.
///
/// # Safety
/// `handle` identifies one live exclusively accessible digest allocation. For positive `len`,
/// `data` names that many readable bytes for the call. Input must not overlap the shell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_crypto_digest_update(
    handle: *mut c_void,
    data: *const u8,
    len: i64,
) {
    let handle = require_handle(handle);
    let Some(len) = usize::try_from(len)
        .ok()
        .filter(|n| isize::try_from(*n).is_ok())
    else {
        panic_abort("crypto.digest: invalid byte length");
    };
    let Some(end) = data.addr().checked_add(len) else {
        panic_abort("crypto.digest: invalid byte extent");
    };
    let Some(owner_end) = handle.addr().checked_add(core::mem::size_of::<Digest>()) else {
        panic_abort("crypto.digest: invalid owner extent");
    };
    if len > 0 && (data.is_null() || (data.addr() < owner_end && handle.addr() < end)) {
        panic_abort("crypto.digest: invalid byte input");
    }
    // SAFETY: the live exclusive allocation is the ABI precondition; aliases were rejected above.
    let digest = unsafe { &mut *handle };
    let Some(total) = next_length(digest.bytes, len) else {
        panic_abort("crypto.digest: message length exceeds SHA-256 limit");
    };
    if len > 0 {
        // SAFETY: the ABI guarantees the checked input extent remains readable; EVP retains none.
        if unsafe { EVP_DigestUpdate(digest.context, data.cast(), len) } != 1 {
            panic_abort("crypto.digest: update failed");
        }
    }
    digest.bytes = total;
}

/// Consume one digest and return the ordinary 32-byte owned array representation.
///
/// # Safety
/// `handle` is a live uniquely owned digest and is never used again after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_crypto_digest_finish(handle: *mut c_void) -> AlignStr {
    let handle = require_handle(handle);
    // SAFETY: this call consumes the unique allocation originally published by new.
    let digest = unsafe { Box::from_raw(handle) };
    let bytes = finish_digest(digest).unwrap_or_else(|error| panic_abort(error));
    let output = align_rt_alloc(32);
    // SAFETY: the normal allocator supplies 32 fresh bytes and stack output has exactly 32 valid bytes.
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), output, 32) };
    AlignStr {
        ptr: output,
        len: 32,
    }
}

fn finish_digest(digest: Box<Digest>) -> Result<[u8; 64], &'static str> {
    #[cfg(test)]
    if FAIL_PROVIDER.with(core::cell::Cell::get) {
        return Err("crypto.digest: finalization failed");
    }
    let mut bytes = [0_u8; 64];
    let mut len = 0;
    // SAFETY: the unique EVP context owns SHA-256 state; its bounded digest fits this output.
    let status = unsafe { EVP_DigestFinal_ex(digest.context, bytes.as_mut_ptr(), &mut len) };
    drop(digest);
    if status != 1 || len != 32 {
        return Err("crypto.digest: finalization failed");
    }
    Ok(bytes)
}

/// Release a digest slot, including a null moved-from slot.
///
/// # Safety
/// A non-null handle is a uniquely owned live digest and is never used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_crypto_digest_free(handle: *mut c_void) {
    if !handle.is_null() {
        // SAFETY: the ABI transfers the unique allocation; Digest::drop releases its EVP context.
        drop(unsafe { Box::from_raw(require_handle(handle)) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_failed_provider_releases_unpublished_and_consumed_owners() {
        let baseline = LIVE_CONTEXTS.with(core::cell::Cell::get);
        FAIL_PROVIDER.with(|fail| fail.set(true));
        assert!(create_digest().is_err());
        assert_eq!(LIVE_CONTEXTS.with(core::cell::Cell::get), baseline);
        FAIL_PROVIDER.with(|fail| fail.set(false));
        let digest = create_digest().expect("live context");
        assert_eq!(LIVE_CONTEXTS.with(core::cell::Cell::get), baseline + 1);
        FAIL_PROVIDER.with(|fail| fail.set(true));
        assert!(finish_digest(digest).is_err());
        FAIL_PROVIDER.with(|fail| fail.set(false));
        assert_eq!(LIVE_CONTEXTS.with(core::cell::Cell::get), baseline);
    }

    #[test]
    #[ignore = "local fixed-chunk storage measurement; not a correctness gate"]
    fn digest_storage_measurement() {
        let owner = align_rt_crypto_digest_new();
        let chunk = [0_u8; 65536];
        for total_mib in [1, 16, 64] {
            // SAFETY: owner and chunk stay live and exclusive for the complete measurement.
            unsafe {
                while (*owner.cast::<Digest>()).bytes < total_mib * 1024 * 1024 {
                    align_rt_crypto_digest_update(owner, chunk.as_ptr(), 65536);
                }
            }
            let rss = std::fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|status| {
                    status
                        .lines()
                        .find(|line| line.starts_with("VmRSS:"))
                        .map(str::to_owned)
                });
            eprintln!(
                "digest total_mib={total_mib} shell_bytes={} live_contexts={} rss={rss:?}",
                core::mem::size_of::<Digest>(),
                LIVE_CONTEXTS.with(core::cell::Cell::get)
            );
        }
        // SAFETY: this is the sole owner and no later operation uses it.
        unsafe {
            align_rt_crypto_digest_free(owner);
        }
    }

    #[test]
    fn digest_fixed_context_many_updates_and_drop() {
        let baseline = LIVE_CONTEXTS.with(core::cell::Cell::get);
        let owner = align_rt_crypto_digest_new();
        let unfinished = align_rt_crypto_digest_new();
        assert_eq!(LIVE_CONTEXTS.with(core::cell::Cell::get), baseline + 2);
        let chunk = [b'a'; 1000];
        // SAFETY: owners remain exclusively held here; the chunk is live for every update.
        unsafe {
            let context = (*owner.cast::<Digest>()).context;
            for _ in 0..1000 {
                align_rt_crypto_digest_update(owner, chunk.as_ptr(), 1000);
                assert_eq!((*owner.cast::<Digest>()).context, context);
                assert_eq!(LIVE_CONTEXTS.with(core::cell::Cell::get), baseline + 2);
            }
            let result = align_rt_crypto_digest_finish(owner);
            assert_eq!(result.len, 32);
            assert_eq!(
                core::slice::from_raw_parts(result.ptr, 32),
                &[
                    0xcd, 0xc7, 0x6e, 0x5c, 0x99, 0x14, 0xfb, 0x92, 0x81, 0xa1, 0xc7, 0xe2, 0x84,
                    0xd7, 0x3e, 0x67, 0xf1, 0x80, 0x9a, 0x48, 0xa4, 0x97, 0x20, 0x0e, 0x04, 0x6d,
                    0x39, 0xcc, 0xc7, 0x11, 0x2c, 0xd0,
                ]
            );
            crate::align_rt_free(result.ptr.cast_mut());
            assert_eq!(LIVE_CONTEXTS.with(core::cell::Cell::get), baseline + 1);
            align_rt_crypto_digest_free(unfinished);
        }
        assert_eq!(LIVE_CONTEXTS.with(core::cell::Cell::get), baseline);
    }

    #[test]
    fn digest_invalid_native_inputs_abort() {
        const CHILD: &str = "ALIGN_DIGEST_NATIVE_ABORT_CASE";
        if let Ok(case) = std::env::var(CHILD) {
            let owner = align_rt_crypto_digest_new();
            // SAFETY: malformed extents are specifically rejected before dereference or EVP.
            unsafe {
                match case.as_str() {
                    "negative" => align_rt_crypto_digest_update(owner, b"a".as_ptr(), -1),
                    "null" => align_rt_crypto_digest_update(owner, core::ptr::null(), 1),
                    "overlap" => align_rt_crypto_digest_update(owner, owner.cast(), 1),
                    "extent" => align_rt_crypto_digest_update(owner, usize::MAX as *const u8, 2),
                    "overflow" => {
                        (*owner.cast::<Digest>()).bytes = MAX_BYTES;
                        align_rt_crypto_digest_update(owner, b"a".as_ptr(), 1);
                    }
                    _ => panic!("unknown child case"),
                }
            }
            panic!("invalid digest input returned");
        }
        for (case, message) in [
            ("negative", "invalid byte length"),
            ("null", "invalid byte input"),
            ("overlap", "invalid byte input"),
            ("extent", "invalid byte extent"),
            ("overflow", "message length exceeds SHA-256 limit"),
        ] {
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    "crypto_digest::tests::digest_invalid_native_inputs_abort",
                    "--nocapture",
                ])
                .env(CHILD, case)
                .output()
                .expect("native abort child");
            assert!(!output.status.success(), "{case}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(message),
                "{case}: {output:?}"
            );
        }
    }

    #[test]
    fn digest_length_limit_is_checked_before_update() {
        assert_eq!(next_length(MAX_BYTES, 0), Some(MAX_BYTES));
        assert_eq!(next_length(MAX_BYTES - 1, 1), Some(MAX_BYTES));
        assert_eq!(next_length(MAX_BYTES, 1), None);
        assert_eq!(next_length(u64::MAX, 1), None);
    }

    #[test]
    fn digest_native_vectors_and_empty_view() {
        let owner = align_rt_crypto_digest_new();
        // SAFETY: the unique owner is live; literal inputs stay live, and finish transfers ownership.
        let result = unsafe {
            align_rt_crypto_digest_update(owner, core::ptr::null(), 0);
            align_rt_crypto_digest_update(owner, b"a".as_ptr(), 1);
            align_rt_crypto_digest_update(owner, b"bc".as_ptr(), 2);
            align_rt_crypto_digest_finish(owner)
        };
        // SAFETY: finish returned exactly 32 owned bytes, read before releasing the normal allocation.
        let bytes = unsafe { core::slice::from_raw_parts(result.ptr, 32) };
        assert_eq!(
            bytes,
            &[
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad
            ]
        );
        // SAFETY: result has one normal allocator owner; a null digest slot is explicitly accepted.
        unsafe {
            crate::align_rt_free(result.ptr.cast_mut());
            align_rt_crypto_digest_free(core::ptr::null_mut());
        }
    }
}
