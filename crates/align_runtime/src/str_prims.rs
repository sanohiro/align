//! Fast-path string primitives (`memcmp`-class) shared as ONE source, compiled TWICE.
//!
//! These four `align_rt_str_*` functions plus their sole private helper [`safe_slice`] are the
//! guarded set for the M14 Slice-2 runtime-bitcode LTO (`docs/impl/07-roadmap.md`). They live in
//! their own file so the driver's `build.rs` can compile exactly them — dependency-free (slice ops
//! only, no `align_hash`/`memchr`) — to a standalone LLVM-bitcode artifact that `alignc --rt-lto`
//! links into the program module, exposing the constant-length `x == "literal"` fast path to the
//! optimizer (probe: `str_eq` 2.1×). The identical file is also compiled into
//! `libalign_runtime.a` (`mod str_prims; pub use str_prims::*;` in `lib.rs`), so there is one
//! source of truth: the staticlib path and the bitcode path can never drift.
//!
//! `str_cmp` deliberately stays in `lib.rs` — the probe measured it *regressing* under a post-link
//! reoptimization (~0.72×), so it must remain an opaque call resolved from the `.a`, never a body
//! visible to the merged module's inliner.

/// Construct a `&[T]` from an FFI pointer and `i64` length, returning an empty slice if `len <= 0`,
/// `ptr` is null, or the total byte size would exceed `isize::MAX`. The single memory-safety guard
/// shared by every `memcmp`-class primitive here (moved verbatim from `lib.rs`).
///
/// # Safety
/// When it returns a non-empty slice, `ptr`/`len` must describe a valid, initialized `[T]` range
/// that stays valid for the borrow.
#[inline(always)]
pub unsafe fn safe_slice<'a, T>(ptr: *const T, len: i64) -> &'a [T] {
    let Ok(n) = isize::try_from(len) else { return &[] };
    if n <= 0 || ptr.is_null() { return &[] }
    let n = n as usize;
    let size = std::mem::size_of::<T>();
    if size > 0 && n > isize::MAX as usize / size { return &[] }
    unsafe { std::slice::from_raw_parts(ptr, n) }
}

/// Byte-equality of two `str` views (M5). Returns 1 if equal, else 0.
///
/// # Safety
/// Both `ptr`/`len` pairs must describe valid byte ranges for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_eq(a: *const u8, alen: i64, b: *const u8, blen: i64) -> i32 {
    // `alen < 0` guards the FFI boundary the same way as `eq_ignore_case`: an equal-and-negative
    // length would otherwise wrap to a huge `usize` in `from_raw_parts` (UB). Real lengths are >= 0.
    if alen != blen || alen < 0 {
        return 0;
    }
    // Same view, or both empty: equal without touching memory. This also avoids
    // `from_raw_parts` on a (possibly null) pointer of a zero-length view, which is UB.
    if a == b || alen == 0 {
        return 1;
    }
    let (x, y) = unsafe {
        (
            safe_slice(a, alen),
            safe_slice(b, blen),
        )
    };
    (x == y) as i32
}

/// `s.eq_ignore_ascii_case(other)` (M5, `core.string`) — 1 if the two byte runs are equal with ASCII
/// letters compared case-insensitively (non-ASCII bytes compare exactly), else 0. Not Unicode
/// case-folding (that stays package-level). Backed by `[u8]::eq_ignore_ascii_case`.
///
/// # Safety
/// Both `ptr`/`len` pairs must describe valid byte ranges for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_eq_ignore_case(aptr: *const u8, alen: i64, bptr: *const u8, blen: i64) -> i32 {
    // `alen < 0` guards the FFI boundary: a (never-expected) negative length would otherwise survive
    // the `alen != blen` check when both are equal-and-negative, then wrap to a huge `usize` in
    // `from_raw_parts` (UB). Real `str` lengths are always >= 0; this is pure defense in depth.
    if alen != blen || alen < 0 {
        return 0;
    }
    if aptr == bptr || alen == 0 {
        return 1;
    }
    let (a, b) = unsafe {
        (
            safe_slice(aptr, alen),
            safe_slice(bptr, blen),
        )
    };
    a.eq_ignore_ascii_case(b) as i32
}

/// `s.starts_with(prefix)` (M5, `core.string`) — 1 if `s` begins with `prefix`'s bytes, else 0.
///
/// # Safety
/// Both `ptr`/`len` pairs must describe valid byte ranges for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_starts_with(hptr: *const u8, hlen: i64, pptr: *const u8, plen: i64) -> i32 {
    if plen <= 0 {
        return 1;
    }
    if plen > hlen {
        return 0;
    }
    let (hay, pre) = unsafe {
        (
            safe_slice(hptr, plen),
            safe_slice(pptr, plen),
        )
    };
    (hay == pre) as i32
}

/// `s.ends_with(suffix)` (M5, `core.string`) — 1 if `s` ends with `suffix`'s bytes, else 0.
///
/// # Safety
/// Both `ptr`/`len` pairs must describe valid byte ranges for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_ends_with(hptr: *const u8, hlen: i64, sptr: *const u8, slen: i64) -> i32 {
    if slen <= 0 {
        return 1;
    }
    if slen > hlen {
        return 0;
    }
    // Compare the trailing `slen` bytes of the haystack against the suffix.
    let tail = unsafe { hptr.add((hlen - slen) as usize) };
    let (hay, suf) = unsafe {
        (
            safe_slice(tail, slen),
            safe_slice(sptr, slen),
        )
    };
    (hay == suf) as i32
}
