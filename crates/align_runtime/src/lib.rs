//! ABI sketch of the minimal runtime (`docs/impl/06-runtime-std.md`).
//!
//! No GC. Holds only "the minimum the language requires" such as arena / parallelism
//! / panic / mutable buffers. Lifetimes and free points are already settled by the
//! compiler (MIR); the runtime allocates/frees exactly as told.
//!
//! M1 wires the first real entry point: the builtin `print` lowers to a call to
//! [`align_rt_print_i64`]. Formatting lives here (not in codegen) so it can later be
//! swapped for a SIMD itoa without touching the compiler (`open-questions.md` Future).

/// Builtin `print` for integers: write the decimal value + newline to stdout.
///
/// M1 widens every integer argument to `i64` in codegen and routes it here. `bool`,
/// strings, and a no-newline variant arrive with `std.io` (M5). The C ABI (`extern "C"`
/// + no mangling) is what the generated `call` targets.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_print_i64(x: i64) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    // A failed write to a closed pipe is ignored here (EPIPE handling is a std.io concern).
    // The generated `main` returns straight to crt0, so std's atexit flush never runs;
    // an explicit flush keeps output from being lost when stdout is block-buffered (a
    // file/pipe redirect). Same for every other `print` variant below.
    let _ = writeln!(out, "{x}").and_then(|()| out.flush());
}

/// Builtin `print` for strings: write the bytes + a newline to stdout. `str` is a
/// `{ ptr, len }` view (`docs/impl/06-runtime-std.md` §2).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_print_str(ptr: *const u8, len: i64) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    // An empty owned `string` (from `str.clone()` / `builder().to_string()`) carries a *null*
    // pointer with `len == 0`; `from_raw_parts(null, 0)` is UB, so emit just the newline. `try_from`
    // avoids a truncating `len as usize` (a heap OOB) on a 32-bit target.
    if len > 0 && let Ok(n) = safe_len(len) {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, n) };
        let _ = out.write_all(bytes);
    }
    let _ = out.write_all(b"\n").and_then(|()| out.flush());
}

/// Builtin `print` for booleans: write `true`/`false` + a newline.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_print_bool(v: i32) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    // Write the constant bytes directly (no formatting machinery).
    let _ = out
        .write_all(if v != 0 { &b"true\n"[..] } else { &b"false\n"[..] })
        .and_then(|()| out.flush());
}

/// Builtin `print` for a `char` (a Unicode scalar value): write its UTF-8 + a newline.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_print_char(c: u32) {
    use std::io::Write;
    let ch = char::from_u32(c).unwrap_or('\u{FFFD}');
    let mut tmp = [0u8; 4];
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(ch.encode_utf8(&mut tmp).as_bytes());
    let _ = out.write_all(b"\n").and_then(|()| out.flush());
}

/// Append a float's shortest round-trip decimal (Rust's `Display`), ensuring it reads as a
/// float: if the rendering has no `.`/exponent and isn't `inf`/`NaN`, a `.0` is appended.
/// Generic over `Display` so the value is written straight into `buf` (no temporary `String`).
fn push_float<T: std::fmt::Display>(buf: &mut Vec<u8>, x: T) {
    use std::io::Write;
    let start = buf.len();
    let _ = write!(buf, "{x}");
    let looks_float = buf[start..].iter().any(|&b| matches!(b, b'.' | b'e' | b'E') || b.is_ascii_alphabetic());
    if !looks_float {
        buf.extend_from_slice(b".0");
    }
}

/// Builtin `print` for `f64`: shortest round-trip decimal + a newline.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_print_f64(x: f64) {
    use std::io::Write;
    let mut line = Vec::with_capacity(32);
    push_float(&mut line, x);
    line.push(b'\n');
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(&line).and_then(|()| out.flush());
}

/// Builtin `print` for `f32`: shortest round-trip decimal + a newline.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_print_f32(x: f32) {
    use std::io::Write;
    let mut line = Vec::with_capacity(32);
    push_float(&mut line, x);
    line.push(b'\n');
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(&line).and_then(|()| out.flush());
}

/// A `str` view passed/returned across the ABI: `{ ptr, len }` (`06-runtime-std.md` §2).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AlignStr {
    pub ptr: *const u8,
    pub len: i64,
}

/// `str.clone()` — deep-copy the bytes of a `str` view into a fresh heap buffer, returning an
/// owned `string` `{ptr,len}` (MMv2 slice 7). The buffer comes from [`align_rt_alloc`] and is
/// freed by the generated code's `Drop` of the owning slot. An empty clone owns no buffer (null
/// ptr), so its `free(null)` drop is a harmless no-op.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.

// Helper to safely construct a slice from an FFI pointer and i64 length.
// Returns an empty slice if len <= 0, ptr is null, or len exceeds isize::MAX.
#[inline(always)]
fn safe_len(len: i64) -> Result<usize, ()> {
    usize::try_from(len).map_err(|_| ()).and_then(|x| if x <= isize::MAX as usize { Ok(x) } else { Err(()) })
}

#[inline(always)]

unsafe fn safe_slice<'a, T>(ptr: *const T, len: i64) -> &'a [T] {
    let Ok(n) = isize::try_from(len) else { return &[] };
    if n <= 0 || ptr.is_null() { return &[] }
    let n = n as usize;
    let size = std::mem::size_of::<T>();
    if size > 0 && n > isize::MAX as usize / size { return &[] }
    unsafe { std::slice::from_raw_parts(ptr, n) }
}


#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_clone(ptr: *const u8, len: i64) -> AlignStr {
    if len <= 0 {
        return AlignStr { ptr: core::ptr::null(), len: 0 };
    }
    // Validate `len` fits a `usize` before allocating/copying. On a 32-bit target an unchecked
    // `len as usize` would truncate, so `align_rt_alloc` would size a tiny buffer while we return
    // the full `len` — a heap out-of-bounds. After this check, both the alloc and the copy are
    // exact (on 64-bit the check never fires).
    let n = match safe_len(len) {
        Ok(n) => n,
        Err(_) => panic_abort("string length exceeds addressable memory"),
    };
    let dst = align_rt_alloc(len);
    unsafe { core::ptr::copy_nonoverlapping(ptr, dst, n) };
    AlignStr { ptr: dst, len }
}

/// `fs.read_file(path)` — read the whole file at `path` (a `str`, `ptr`/`len`) into a freshly
/// heap-allocated owned `string`, writing its `{ptr,len}` to `out`. The buffer comes from
/// [`align_rt_alloc`] (so the generated `Drop` frees it). Returns 0 on success, 1 on any I/O error
/// (or a non-UTF-8 path), `AL_INVALID` if the file's **content** is not valid UTF-8 (a `str`/`string`
/// is always UTF-8, draft §7/§12 — binary files are read via `reader.read(buffer)`), leaving `out` as
/// the caller-zeroed `{null,0}`. An empty file yields a null buffer with len 0 (no allocation). The
/// first `std.fs` surface (`06-runtime-std.md`).
///
/// # Safety
/// `path` must describe a valid byte range; `out` must point to a writable `{ptr,len}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_read_file(path: *const u8, path_len: i64, out: *mut AlignStr) -> i32 {
    // `out` is the caller's `{ptr,len}` slot; the generated code always passes a valid one, but
    // guard the raw pointer at the FFI boundary rather than dereferencing a null below.
    if out.is_null() {
        return 1;
    }
    // `from_raw_parts` is UB on a null pointer even with len 0 — guard an empty/owned path. Use
    // `try_from` so a `path_len` that doesn't fit `usize` (only possible on a 32-bit target) is a
    // clean error, not a truncating `as` cast (a heap out-of-bounds).
    let path_bytes: &[u8] = if path_len <= 0 || path.is_null() {
        &[]
    } else {
        let Ok(n) = safe_len(path_len) else {
            return 1;
        };
        unsafe { std::slice::from_raw_parts(path, n) }
    };
    let Ok(path_str) = std::str::from_utf8(path_bytes) else {
        return 1;
    };
    use std::io::Read;
    // Fast path: a regular file with a known nonzero length — allocate the owned buffer once and
    // read straight into it, skipping the `std::fs::read` Vec and the second copy into the runtime
    // allocator (~1.8× on a 128 MiB file; `work/io_perf_probe.rs`). Special / streaming files
    // (length 0, `/proc`, pipes, char devices) and any file that shrinks or grows under us fall
    // back to the copy path below — which re-opens by path, so a partial read here is harmless.
    if let Ok((mut file, meta)) = std::fs::File::open(path_str).and_then(|f| f.metadata().map(|m| (f, m))) {
        let flen = meta.len();
        // Regular files only (`is_file`), nonzero length (skips empty / size-unknown special
        // files). `isize::try_from` is the single guard that keeps the rest sound on every
        // target: a positive `isize` fits both `usize` (the slice len) and `i64` (the alloc
        // size) losslessly, and is `<= isize::MAX` so `from_raw_parts_mut` is not UB. A larger
        // file (only reachable on a 32-bit target) just takes the fallback path.
        if meta.is_file() && flen > 0
            && let Ok(len_z) = isize::try_from(flen) {
                let len_i = len_z as i64;
                let len_u = len_z as usize;
                let dst = align_rt_alloc(len_i);
                let buf = unsafe { core::slice::from_raw_parts_mut(dst, len_u) };
                // `read_exact` fills the whole buffer (a shorter file errors). On success one
                // more read must hit EOF — otherwise the file grew past the snapshot and the
                // buffer would silently truncate, so fall back. Any failure frees and falls back.
                if file.read_exact(buf).is_ok() && matches!(file.read(&mut [0u8; 1]), Ok(0)) {
                    // A `str`/`string` is always valid UTF-8 (draft §7/§12); binary content is read via
                    // `reader.read(buffer)`. Invalid → `Error.Invalid` (no fallback: re-reading yields
                    // the same bytes). `buf`'s borrow of `dst` ends here, so the free below is sound.
                    if !validate_utf8(buf) {
                        unsafe { align_rt_free(dst) };
                        return AL_INVALID;
                    }
                    unsafe { *out = AlignStr { ptr: dst, len: len_i } };
                    return 0;
                }
                unsafe { align_rt_free(dst) };
            }
    }
    // Fallback (empty / special / changed file): read into a Vec, then copy into the owned buffer.
    let data = match std::fs::read(path_str) {
        Ok(d) => d,
        Err(_) => return 1,
    };
    // A `str`/`string` is always valid UTF-8 (draft §7/§12); reject binary content before it becomes
    // a `str` (binary reads use `reader.read(buffer)`). `Error.Invalid` (== `AL_INVALID`).
    if !validate_utf8(&data) {
        return AL_INVALID;
    }
    let len = data.len() as i64;
    // Copy into the runtime's own allocator so the generated `Drop` (which calls `free`) owns it.
    let dst = align_rt_alloc(len);
    if len > 0 {
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len()) };
    }
    unsafe { *out = AlignStr { ptr: dst, len } };
    0
}

/// `fs.write_file(path, data)` — create/truncate `path` (a `str` view) and write all of `data` (a
/// `str`/`bytes` view: `{ptr,len}`), then close. Returns `0` on success, else the errno mapped
/// through [`io_error_to_status`]. An empty `data` still create+truncates the file (an empty file).
///
/// # Safety
/// `path`/`data` must describe valid byte ranges for their lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_write_file(path: *const u8, path_len: i64, data: *const u8, data_len: i64) -> i32 {
    let Some(path_str) = (unsafe { path_from_view(path, path_len) }) else {
        return AL_INVALID;
    };
    // `from_raw_parts` is UB on a null pointer even for len 0 — guard an empty/owned data view.
    let bytes: &[u8] = if data_len <= 0 || data.is_null() {
        &[]
    } else {
        let Ok(n) = safe_len(data_len) else { return AL_INVALID };
        unsafe { std::slice::from_raw_parts(data, n) }
    };
    use std::io::Write;
    match std::fs::File::create(&path_str).and_then(|mut f| f.write_all(bytes)) {
        Ok(()) => 0,
        Err(e) => io_error_to_status(&e),
    }
}

/// `fs.write_file(path, builder)` — the `builder`-source form (writes the builder's accumulated
/// bytes; the builder is borrowed, not consumed), mirroring [`align_rt_io_writer_write_builder`].
///
/// # Safety
/// `path` must describe a valid byte range; `b` must be a valid `Builder` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_write_file_builder(path: *const u8, path_len: i64, b: *mut Builder) -> i32 {
    if b.is_null() {
        return AL_INVALID;
    }
    let b = unsafe { &*b };
    let (ptr, len) = (b.buf.as_ptr(), b.buf.len() as i64);
    unsafe { align_rt_fs_write_file(path, path_len, ptr, len) }
}

/// `fs.exists(path)` — `1` if `path` exists, else `0`. Per `draft.md` §18.2 this folds *every* error
/// (a `stat` failure — not found, a permission error on a path component, a bad path) to `0` ("does
/// not exist"), so it returns a plain `bool`, never a `Result`. Uses `stat` (follows symlinks), so
/// "stat failure = does not exist" as specified.
///
/// # Safety
/// `path` must describe a valid byte range for its length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_exists(path: *const u8, path_len: i64) -> i32 {
    let Some(path_str) = (unsafe { path_from_view(path, path_len) }) else {
        return 0;
    };
    i32::from(std::fs::metadata(&path_str).is_ok())
}

/// `fs.remove(path)` — delete the file at `path`. Returns `0` on success, else the mapped errno.
/// Files only (v1) — `remove_file`, not a recursive/directory remove.
///
/// # Safety
/// `path` must describe a valid byte range for its length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_remove(path: *const u8, path_len: i64) -> i32 {
    let Some(path_str) = (unsafe { path_from_view(path, path_len) }) else {
        return AL_INVALID;
    };
    match std::fs::remove_file(&path_str) {
        Ok(()) => 0,
        Err(e) => io_error_to_status(&e),
    }
}

/// `fs.read_dir(path)` — the entry names of directory `path` as an owned `array<string>` written to
/// `out` (`{ptr,len}`: a heap buffer of `len` `AlignStr` headers, each owning its own name buffer).
/// Entries are bare names (no path prefix), in OS order (`.`/`..` excluded — Rust's `read_dir`
/// omits them; the caller sorts if a deterministic order is wanted, per `draft.md` §18.2). An entry
/// whose name is **not valid UTF-8 is excluded** (a `string` is always UTF-8, draft §7/§12; such a
/// file is unreachable through a `str` path regardless), so the result may be shorter than the
/// on-disk entry count. Returns `0` on success, else the mapped errno (leaving `out` = `{null,0}`). The whole array is `Drop`-freed
/// by [`align_rt_free_string_array`] (each name buffer, then the header).
///
/// # Safety
/// `path` must describe a valid byte range; `out` must point to a writable `{ptr,len}` slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_read_dir(path: *const u8, path_len: i64, out: *mut AlignStr) -> i32 {
    if out.is_null() {
        return AL_INVALID;
    }
    unsafe { *out = AlignStr { ptr: core::ptr::null(), len: 0 } };
    let Some(path_str) = (unsafe { path_from_view(path, path_len) }) else {
        return AL_INVALID;
    };
    let rd = match std::fs::read_dir(&path_str) {
        Ok(rd) => rd,
        Err(e) => return io_error_to_status(&e),
    };
    // Collect every entry name first (a mid-iteration error maps like any other, leaving `out` empty).
    let mut names: Vec<Vec<u8>> = Vec::new();
    for entry in rd {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => return io_error_to_status(&e),
        };
        // A `string` is always valid UTF-8 (draft §7/§12), so an entry whose name is not valid UTF-8
        // cannot be represented — it is **excluded** from the listing (draft §18.2). Excluding just the
        // broken name keeps enumeration usable for the rest of the directory; such a file is
        // unreachable through a `str` path anyway. The bare file name, as raw bytes.
        let name = entry.file_name();
        let bytes = name.as_encoded_bytes();
        if !validate_utf8(bytes) {
            continue;
        }
        names.push(bytes.to_vec());
    }
    let n = names.len();
    if n == 0 {
        return 0; // empty directory → {null,0}
    }
    // The header buffer: `n` `AlignStr` entries. `checked_mul` guards a 32-bit size overflow (which
    // would otherwise under-allocate and heap-overflow the store loop below).
    let Some(hdr_bytes) = n.checked_mul(core::mem::size_of::<AlignStr>()).and_then(|b| i64::try_from(b).ok()) else {
        return AL_INVALID;
    };
    let hdr = align_rt_alloc(hdr_bytes) as *mut AlignStr;
    for (i, name) in names.into_iter().enumerate() {
        let len = name.len() as i64;
        let dst = align_rt_alloc(len); // null for an empty name (len 0) — a harmless free at Drop
        if len > 0 {
            unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), dst, name.len()) };
        }
        unsafe { *hdr.add(i) = AlignStr { ptr: dst, len } };
    }
    unsafe { *out = AlignStr { ptr: hdr as *const u8, len: n as i64 } };
    0
}

/// Free an owned `array<string>` (`fs.read_dir`): free each element's name buffer, then the header
/// buffer. Null-safe (a moved-out / never-initialised `{null,0}` frees nothing). This is the deep
/// `Drop` for `array<string>` — unlike a scalar `array<T>` (one buffer), each element owns a buffer.
///
/// # Safety
/// `ptr` must be null or a header buffer from [`align_rt_fs_read_dir`] of `len` `AlignStr` entries
/// (each entry's `ptr` an [`align_rt_alloc`] buffer or null), not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_free_string_array(ptr: *mut u8, len: i64) {
    if ptr.is_null() {
        return;
    }
    if let Ok(n) = safe_len(len) {
        let hdr = ptr as *mut AlignStr;
        for i in 0..n {
            let entry = unsafe { *hdr.add(i) };
            unsafe { align_rt_free(entry.ptr as *mut u8) };
        }
    }
    unsafe { align_rt_free(ptr) };
}

/// Read the whole file at `path` into a fresh **arena** allocation, writing a `{ptr,len}` view to
/// `out` — the [`align_rt_fs_read_file_view`] fallback for special / zero-length files. Returns `0`
/// or a mapped errno; an empty file yields `{null,0}`. Unlike `fs.read_file` (heap-owned,
/// `Drop`-freed), this buffer is arena-owned (bulk-freed at arena end), so the returned view follows
/// the same region rule as the mmap path — no separate `Drop`.
///
/// # Safety
/// `arena` must be a valid arena handle; `out` must point to a writable `{ptr,len}` slot.
unsafe fn read_file_view_into_arena(path: &str, arena: *mut Arena, out: *mut AlignStr) -> i32 {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => return io_error_to_status(&e),
    };
    if data.is_empty() {
        return 0; // already {null,0}
    }
    // A `str` is always valid UTF-8 (draft §7/§12) — reject binary content (read via
    // `reader.read(buffer)`) before it becomes an arena-owned `str` view. `Error.Invalid`.
    if !validate_utf8(&data) {
        return AL_INVALID;
    }
    let Ok(len_z) = isize::try_from(data.len()) else { return AL_INVALID };
    let dst = unsafe { align_rt_arena_alloc(arena, len_z as i64, 1) };
    if dst.is_null() {
        return AL_INVALID;
    }
    unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len()) };
    unsafe { *out = AlignStr { ptr: dst as *const u8, len: len_z as i64 } };
    0
}

/// `fs.read_file_view(path)` — memory-map the regular file at `path` read-only into `arena`, writing
/// the `{ptr,len}` view to `out`. Returns `0` on success, else the errno mapped through
/// [`io_error_to_status`] (leaving `out` = `{null,0}`). The mapping is registered on `arena` and
/// `munmap`ped when the arena ends (`draft.md` §18.2 region rule), so the returned `str` lives
/// exactly as long as the arena — no separate `Drop`, and a small returned view cannot pin the
/// mapping past the arena.
///
/// Guardrails (`open-questions.md` "Transparent zero-copy I/O", the mmap bullet):
/// - **Regular files only.** `fstat` (via the fd's `metadata`) gates to a regular file; a character
///   device / FIFO / `/proc` file (whose `st_size` is 0 or a lie) is *not* mmap'd — it takes the
///   owned-copy fallback ([`read_file_view_into_arena`]), which reads the real bytes into arena
///   memory. That changes the cost class (a copy, not a zero-copy view) but is correct — the
///   deliberate v1 behavior (recorded), preferring correctness over a broken zero-copy on files
///   whose size can't be trusted.
/// - **Zero-length files** are never `mmap`ped (`mmap` of length 0 is `EINVAL`); the fallback reads
///   them, yielding an empty `{null,0}` view.
/// - **Truncation after mapping (SIGBUS):** the mapping length is fixed at `mmap` time from the
///   `fstat` size. If another process truncates the file afterward, touching the lost pages raises
///   `SIGBUS` — a known v1 limitation, not UB. We deliberately install **no** `SIGBUS` handler: a
///   process-global signal handler is exactly the hidden global side effect Align forbids ("nothing
///   hidden"), and per-mapping recovery needs `sigsetjmp`/`siglongjmp` machinery out of v1 scope.
///   Concurrent truncation of a mapped file is the caller's contract to avoid.
///
/// # Safety
/// `path` must describe a valid byte range; `arena` must be a valid arena handle; `out` must point
/// to a writable `{ptr,len}` slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_read_file_view(path: *const u8, path_len: i64, arena: *mut Arena, out: *mut AlignStr) -> i32 {
    if out.is_null() {
        return AL_INVALID;
    }
    unsafe { *out = AlignStr { ptr: core::ptr::null(), len: 0 } };
    // A view must be arena-owned (sema requires an enclosing arena; codegen always passes a real
    // handle). Guard the FFI boundary rather than deref a null arena.
    if arena.is_null() {
        return AL_INVALID;
    }
    let Some(path_str) = (unsafe { path_from_view(path, path_len) }) else {
        return AL_INVALID;
    };
    use std::os::fd::AsRawFd;
    let (file, meta) = match std::fs::File::open(&path_str).and_then(|f| f.metadata().map(|m| (f, m))) {
        Ok(fm) => fm,
        Err(e) => return io_error_to_status(&e),
    };
    // A regular file with a nonzero size that fits `isize` (so it fits both the `usize` map length
    // and the `i64` view len) takes the mmap fast path; a special / zero-length / oversized file
    // falls back to an owned arena copy (correctness over zero-copy).
    if meta.is_file() && meta.len() > 0
        && let Ok(len_z) = isize::try_from(meta.len()) {
            let len_u = len_z as usize;
            let addr = unsafe {
                mmap(core::ptr::null_mut(), len_u, PROT_READ, MAP_PRIVATE, file.as_raw_fd(), 0)
            };
            if addr != MAP_FAILED && !addr.is_null() {
                // A `str` is always valid UTF-8 (draft §7/§12). Validate the mapped bytes before the
                // view escapes; invalid → `munmap` immediately (it was never registered on the arena)
                // and fail with `Error.Invalid`. Binary files take the `reader.read(buffer)` path.
                let mapped = unsafe { core::slice::from_raw_parts(addr as *const u8, len_u) };
                if !validate_utf8(mapped) {
                    unsafe { munmap(addr, len_u) };
                    return AL_INVALID;
                }
                // Register on the arena for bulk `munmap` at arena end (every exit path).
                unsafe { (*arena).maps.push((addr, len_u)) };
                unsafe { *out = AlignStr { ptr: addr as *const u8, len: len_z as i64 } };
                // The fd can close now — the mapping keeps the file alive on its own (POSIX).
                return 0;
            }
            // mmap failed (rare — e.g. a filesystem that can't map): fall through to the copy path.
        }
    // Fallback: read the true contents into arena memory (special files, /proc, zero-length, or a
    // failed mmap). A directory errors here (`std::fs::read` on a dir → mapped errno). Re-reads by
    // path, so dropping `file` first is fine.
    drop(file);
    unsafe { read_file_view_into_arena(&path_str, arena, out) }
}

/// Build the `args: array<str>` value for `main` from the C `argc`/`argv`. Returns the owned
/// `array<str>` as an `{ptr, len}`: a freshly [`align_rt_alloc`]'d buffer of `argc` `AlignStr`
/// (`{ptr,len}`) entries, each a zero-copy view of one argv string (length via `strlen`). The
/// element string bytes are argv's (process-lifetime, not freed); only the `AlignStr` buffer is
/// owned, freed by the generated `Drop` of the `args` local at `main` exit. `argc <= 0` → an empty
/// `{null,0}` array.
///
/// # Safety
/// `argv` must point to `argc` valid, NUL-terminated C strings (the platform `main` contract).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_args_build(argc: i32, argv: *const *const u8) -> AlignStr {
    if argc <= 0 || argv.is_null() {
        return AlignStr { ptr: core::ptr::null(), len: 0 };
    }
    let n = argc as usize;
    // Buffer of `n` AlignStr entries; sized in bytes for `align_rt_alloc` (a `c_void*`-granular
    // bump/heap allocator). The element views point into argv, so the buffer is the only owned part.
    // `checked_mul` guards a 32-bit `usize` overflow (`n` up to `i32::MAX` × the entry size), which
    // would otherwise under-allocate and then heap-overflow the store loop below.
    let bytes = n
        .checked_mul(core::mem::size_of::<AlignStr>())
        .and_then(|b| i64::try_from(b).ok())
        .unwrap_or_else(|| panic_abort("arguments buffer size overflow"));
    let buf = align_rt_alloc(bytes) as *mut AlignStr;
    for i in 0..n {
        let cstr = unsafe { *argv.add(i) };
        let len = if cstr.is_null() {
            0
        } else {
            // strlen: scan to the NUL.
            let mut l = 0usize;
            while unsafe { *cstr.add(l) } != 0 {
                l += 1;
            }
            l as i64
        };
        unsafe { *buf.add(i) = AlignStr { ptr: cstr, len } };
    }
    AlignStr { ptr: buf as *const u8, len: argc as i64 }
}

/// `chunks(n)`: split the `{src, src_len}` view (element size `elem_size` bytes, `src_len` =
/// element count) into length-`n` sub-slices — the last may be shorter — returning an owned
/// `{ chunk_buf, count }` array of slice headers (`draft.md` §11). Each header `{ ptr, len }`
/// points into `src` (a borrow, not freed); only the header buffer is owned (freed by the
/// generated `Drop`). `n <= 0` / empty source → `{ null, 0 }`.
///
/// # Safety
/// `src` must point to `src_len` elements of `elem_size` bytes for the call's duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_chunks(src: *const u8, src_len: i64, n: i64, elem_size: i64) -> AlignStr {
    if n <= 0 || src_len <= 0 || src.is_null() {
        return AlignStr { ptr: core::ptr::null(), len: 0 };
    }
    let count = (src_len - 1) / n + 1; // ceil(src_len / n), overflow-free (src_len > 0, n > 0)
    // Header buffer of `count` `AlignStr` entries. `try_from` + `checked_mul` guard a 32-bit
    // `usize` truncation/overflow (a huge `count` would otherwise under-allocate and heap-overflow).
    let count_usize = safe_len(count).unwrap_or_else(|_| panic_abort("chunks count overflow"));
    let bytes = count_usize
        .checked_mul(core::mem::size_of::<AlignStr>())
        .and_then(|b| i64::try_from(b).ok())
        .unwrap_or_else(|| panic_abort("chunks buffer size overflow"));
    let buf = align_rt_alloc(bytes) as *mut AlignStr;
    for i in 0..count {
        let start = i * n; // element offset of this chunk
        let len = core::cmp::min(n, src_len - start);
        let offset = start.checked_mul(elem_size)
            .and_then(|o| isize::try_from(o).ok())
            .map(|o| o as usize)
            .unwrap_or_else(|| panic_abort("chunks offset overflow"));
        let ptr = unsafe { src.add(offset) };
        unsafe { *buf.add(usize::try_from(i).unwrap()) = AlignStr { ptr, len } };
    }
    AlignStr { ptr: buf as *const u8, len: count }
}

/// A process-lifetime worker pool for data-parallel runtime ops. The first parallel call lazily
/// spawns `available_parallelism` workers that park on a job queue; subsequent calls submit jobs
/// instead of spawning fresh OS threads (raw per-call `thread::spawn` — ~tens of µs each — dominated
/// small `par_map`s). Workers are detached and die when the process exits.
type ParJob = Box<dyn FnOnce() + Send + 'static>;

struct ParPool {
    queue: std::sync::Mutex<std::collections::VecDeque<ParJob>>,
    available: std::sync::Condvar,
}

impl ParPool {
    fn submit(&'static self, job: ParJob) {
        self.queue.lock().unwrap().push_back(job);
        self.available.notify_one();
    }
}

/// The global pool (lazily created). Returns its worker count too (= the parallelism degree).
fn par_pool() -> (&'static ParPool, usize) {
    static POOL: std::sync::OnceLock<(&'static ParPool, usize)> = std::sync::OnceLock::new();
    *POOL.get_or_init(|| {
        let n = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(1);
        let p: &'static ParPool = Box::leak(Box::new(ParPool {
            queue: std::sync::Mutex::new(std::collections::VecDeque::new()),
            available: std::sync::Condvar::new(),
        }));
        for _ in 0..n {
            std::thread::spawn(move || loop {
                let job = {
                    let mut q = p.queue.lock().unwrap();
                    loop {
                        match q.pop_front() {
                            Some(j) => break j,
                            None => q = p.available.wait(q).unwrap(),
                        }
                    }
                };
                job(); // run outside the lock so other workers can keep pulling
            });
        }
        (p, n)
    })
}

/// `par_map`: allocate an output buffer of `count` elements (`out_stride` bytes each) and apply
/// `thunk` to each of `count` input elements — reading element `i` from `in_buf + i*in_stride`,
/// writing its result to `out + i*out_stride` — splitting the work into contiguous, **disjoint**
/// output ranges across the available threads. No synchronization is needed: the language
/// guarantees `thunk` (a Pure function) shares no mutable state and the ranges never overlap
/// (`draft.md` §11). Returns the owned output buffer (freed by the generated `Drop`). `count <= 0`
/// → null. The buffer size uses `checked_mul` (a huge `count` would otherwise under-allocate and
/// then heap-overflow the write loop).
///
/// # Safety
/// `in_buf` must point to `count` elements of `in_stride` bytes for the call; `thunk` reads one
/// input element and writes one output element.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_par_map(
    in_buf: *const u8,
    count: i64,
    in_stride: i64,
    out_stride: i64,
    thunk: extern "C" fn(*const u8, *mut u8),
) -> *mut u8 {
    if count <= 0 {
        return core::ptr::null_mut();
    }
    let count = safe_len(count).unwrap_or_else(|_| panic_abort("par_map count overflow"));
    let Ok(in_stride) = safe_len(in_stride) else { return core::ptr::null_mut() };
    let Ok(out_stride) = safe_len(out_stride) else { return core::ptr::null_mut() };
    let bytes = count
        .checked_mul(out_stride)
        .and_then(|b| i64::try_from(b).ok())
        .unwrap_or_else(|| panic_abort("par_map output size overflow"));
    
    // Check input size overflow
    let _in_bytes = count
        .checked_mul(in_stride)
        .and_then(|b| isize::try_from(b).ok())
        .unwrap_or_else(|| panic_abort("par_map input size overflow"));

    let out_buf = align_rt_alloc(bytes);
    let in_addr = in_buf as usize;
    let out_addr = out_buf as usize;
    // Run `[start, end)` of the map on this thread (buffers passed as `usize` so the closures are
    // `Send` — raw pointers are not; the ranges are disjoint, so this is race-free).
    let run = move |start: usize, end: usize| {
        for i in start..end {
            let ip = in_addr.checked_add(i.checked_mul(in_stride).unwrap()).unwrap() as *const u8;
            let op = out_addr.checked_add(i.checked_mul(out_stride).unwrap()).unwrap() as *mut u8;
            thunk(ip, op);
        }
    };

    let (pool, workers) = par_pool();
    // Don't parallelize trivially-small work: a chunk must be at least `PAR_MIN_CHUNK` elements, so
    // tiny maps (where the pool round-trip would dwarf the work) fall to the single-chunk caller
    // path. Keep chunks coarse because each element still crosses the indirect thunk boundary.
    const PAR_MIN_CHUNK: usize = 32768;
    let per = count.div_ceil(workers).max(PAR_MIN_CHUNK);
    let nchunks = count.div_ceil(per); // ≤ workers, every chunk non-empty
    // Single-chunk fast path: run on the caller, no pool round-trip.
    if nchunks <= 1 {
        run(0, count);
        return out_buf;
    }
    // Submit chunks 1.. to the pool and run chunk 0 on the caller, then wait for the submitted ones.
    // The barrier is `(remaining count, first panic payload)`: each worker decrements + signals; the
    // caller waits to 0. A worker job is wrapped in `catch_unwind` so a panic in it can't kill the
    // pool worker or leave the barrier stuck (a deadlock) — it's recorded and re-raised on the caller
    // (Align thunks abort rather than unwind, so this is defensive, but a stuck pool is unacceptable).
    type PanicBox = Box<dyn std::any::Any + Send + 'static>;
    // (remaining chunk count, first panic payload) guarded by a mutex, signaled via the condvar.
    type Barrier = std::sync::Arc<(std::sync::Mutex<(usize, Option<PanicBox>)>, std::sync::Condvar)>;
    let remaining: Barrier =
        std::sync::Arc::new((std::sync::Mutex::new((nchunks - 1, None)), std::sync::Condvar::new()));
    for t in 1..nchunks {
        let start = t * per;
        let end = (start + per).min(count);
        let remaining = remaining.clone();
        pool.submit(Box::new(move || {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(start, end)));
            let (m, cv) = &*remaining;
            let mut st = m.lock().unwrap();
            st.0 -= 1;
            if let Err(p) = res
                && st.1.is_none() {
                    st.1 = Some(p);
                }
            cv.notify_all();
        }));
    }
    run(0, per.min(count));
    let (m, cv) = &*remaining;
    let mut st = m.lock().unwrap();
    while st.0 > 0 {
        st = cv.wait(st).unwrap();
    }
    if let Some(p) = st.1.take() {
        std::panic::resume_unwind(p);
    }
    out_buf
}

/// An append-oriented string builder (`06-runtime-std.md` §7), backing `template`
/// desugaring. M5: heap-backed; the finished buffer is leaked (no ownership/free yet —
/// arena-tied builders come later).
pub struct Builder {
    buf: Vec<u8>,
    /// Where the finished bytes live: an arena (bulk-freed) or null (leaked).
    arena: *mut Arena,
}

/// Open a builder. If `arena` is non-null, the finished string is allocated in that arena (freed in
/// bulk at the block's end); otherwise it is leaked (no owner yet). `capacity` (bytes) pre-sizes the
/// backing buffer so appends don't reallocate as it grows; 0 = default (empty).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_new(arena: *mut Arena, capacity: i64) -> *mut Builder {
    // `try_reserve` (not `with_capacity`) so a bogus/huge user capacity can't abort the process on
    // OOM — an over-large reservation just fails silently and the buffer grows on demand instead.
    let mut buf = Vec::new();
    if let Ok(cap) = safe_len(capacity) {
        let _ = buf.try_reserve(cap);
    }
    Box::into_raw(Box::new(Builder { buf, arena }))
}

/// Append raw bytes (a static template part or a `str` value).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write(b: *mut Builder, ptr: *const u8, len: i64) {
    if b.is_null() { return; }
    let b = unsafe { &mut *b };
    b.buf.extend_from_slice(unsafe { safe_slice(ptr, len) });
}

fn builder_push_i64(buf: &mut Vec<u8>, v: i64) {
    if (-999..=999).contains(&v) {
        // Format into a stack buffer (max = sign + 3 digits) and append in one `extend_from_slice`,
        // so the hot path pays a single capacity check / length update rather than one per digit.
        let mut n = v;
        let mut tmp = [0u8; 4];
        let mut len = 0;
        if n < 0 {
            tmp[0] = b'-';
            len = 1;
            n = -n;
        }
        let n = n as u16;
        if n >= 100 {
            tmp[len] = b'0' + (n / 100) as u8;
            tmp[len + 1] = b'0' + ((n / 10) % 10) as u8;
            tmp[len + 2] = b'0' + (n % 10) as u8;
            len += 3;
        } else if n >= 10 {
            tmp[len] = b'0' + (n / 10) as u8;
            tmp[len + 1] = b'0' + (n % 10) as u8;
            len += 2;
        } else {
            tmp[len] = b'0' + n as u8;
            len += 1;
        }
        buf.extend_from_slice(&tmp[..len]);
        return;
    }
    // 20 = the widest i64 decimal (`-9223372036854775808`). Emit digits back-to-front from the
    // value's magnitude computed via `n % 10` / `unsigned_abs` (works for `i64::MIN` — never negates).
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    // Work on the unsigned magnitude (`unsigned_abs` handles `i64::MIN`): the loop then uses unsigned
    // div/mod, which LLVM lowers to a multiply+shift — no signed-division sign corrections per digit.
    let mut n = v.unsigned_abs();
    loop {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if v < 0 {
        i -= 1;
        tmp[i] = b'-';
    }
    buf.extend_from_slice(&tmp[i..]);
}

/// Append a decimal integer. Hand-rolled itoa straight into the buffer — no generic `write!`
/// formatter (runtime format-string parsing + trait dispatch), the builder's hot path.
///
/// # Safety
/// `b` must be a valid `Builder` pointer for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write_int(b: *mut Builder, v: i64) {
    let b = unsafe { &mut *b };
    builder_push_i64(&mut b.buf, v);
}

/// Append `prefix + decimal integer + suffix` in one runtime call. This is the batched form of the
/// common generated pattern `b.write("..."); b.write_int(x); b.write("...")`.
///
/// # Safety
/// `p1`/`l1` and `p2`/`l2` must describe valid byte ranges when their lengths are positive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write_str_int_str(b: *mut Builder, p1: *const u8, l1: i64, v: i64, p2: *const u8, l2: i64) {
    if b.is_null() { return; }
    let b = unsafe { &mut *b };
    b.buf.extend_from_slice(unsafe { safe_slice(p1, l1) });
    builder_push_i64(&mut b.buf, v);
    b.buf.extend_from_slice(unsafe { safe_slice(p2, l2) });
}

/// Append `true`/`false`.
///
/// # Safety
/// `b` must be a valid `Builder` pointer for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write_bool(b: *mut Builder, v: i32) {
    let b = unsafe { &mut *b };
    b.buf.extend_from_slice(if v != 0 { &b"true"[..] } else { &b"false"[..] });
}

/// Append a `char`'s UTF-8 encoding.
///
/// # Safety
/// `b` must be a valid `Builder` pointer for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write_char(b: *mut Builder, c: u32) {
    let b = unsafe { &mut *b };
    let ch = char::from_u32(c).unwrap_or('\u{FFFD}');
    let mut tmp = [0u8; 4];
    b.buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
}

/// Append an `f64`'s shortest round-trip decimal.
///
/// # Safety
/// `b` must be a valid `Builder` pointer for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write_f64(b: *mut Builder, x: f64) {
    let b = unsafe { &mut *b };
    push_float(&mut b.buf, x);
}

/// Append an `f32`'s shortest round-trip decimal.
///
/// # Safety
/// `b` must be a valid `Builder` pointer for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write_f32(b: *mut Builder, x: f32) {
    let b = unsafe { &mut *b };
    push_float(&mut b.buf, x);
}

/// Append a `str` as a JSON string literal: a leading/trailing `"` with the content
/// escaped per RFC 8259 (`"`, `\`, and the C0 control set; the rest passes through as
/// UTF-8). Backs `json.encode` for `str` fields.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write_json_str(b: *mut Builder, ptr: *const u8, len: i64) {
    let b = unsafe { &mut *b };
    b.buf.push(b'"');
    if len > 0 {
        let bytes = unsafe { safe_slice(ptr, len) };
        // Only `"`, `\`, and the C0 control set need escaping; every other byte (including all
        // multi-byte UTF-8 continuations) passes through verbatim. Copy each clean run in bulk
        // (`extend_from_slice`) instead of pushing byte by byte — ~1.9–3× on typical text
        // (`work/json_str_simd_probe.rs`), with byte-identical output.
        let mut start = 0;
        for (i, &c) in bytes.iter().enumerate() {
            if c == b'"' || c == b'\\' || c < 0x20 {
                if start < i {
                    // Skip an empty copy when escapes are adjacent (e.g. `\r\n`).
                    b.buf.extend_from_slice(&bytes[start..i]);
                }
                write_json_escape(&mut b.buf, c);
                start = i + 1;
            }
        }
        if start < bytes.len() {
            b.buf.extend_from_slice(&bytes[start..]);
        }
    }
    b.buf.push(b'"');
}

/// Append the JSON escape for one byte that needs escaping (`"`, `\`, or a C0 control), per
/// RFC 8259 — the short forms where defined, else `\u00XX`. Caller guarantees `c` needs escaping.
#[inline]
fn write_json_escape(buf: &mut Vec<u8>, c: u8) {
    match c {
        b'"' => buf.extend_from_slice(b"\\\""),
        b'\\' => buf.extend_from_slice(b"\\\\"),
        0x08 => buf.extend_from_slice(b"\\b"),
        0x0c => buf.extend_from_slice(b"\\f"),
        b'\n' => buf.extend_from_slice(b"\\n"),
        b'\r' => buf.extend_from_slice(b"\\r"),
        b'\t' => buf.extend_from_slice(b"\\t"),
        c => {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            buf.extend_from_slice(b"\\u00");
            buf.push(HEX[(c >> 4) as usize]);
            buf.push(HEX[(c & 0xf) as usize]);
        }
    }
}

/// One field descriptor for `json.decode` (matches the codegen layout):
/// `{ name_ptr, name_len, tag, offset }`. `tag` packs `(signed << 16) | (kind << 8) | byte-width`:
/// kind 0 = int, 1 = bool, 2 = float, 3 = str; the byte-width is 1/2/4/8 for scalars (16 for a
/// `str` view); bit 16 is the int sign flag (1 = signed, 0 = unsigned), only meaningful for kind 0.
/// The sign flag lets the decoder range-check a parsed integer before writing (see [`int_in_range`]).
#[repr(C)]
pub struct JsonField {
    pub name_ptr: *const u8,
    pub name_len: i64,
    pub tag: i32,
    pub offset: i64,
}

/// Whether the parsed `i64` value `v` fits the target integer field of `w` bytes with signedness
/// `signed` — the range-check that keeps `json.decode` from silently truncating/sign-wrapping
/// out-of-range input (`{"n": 300}` into `u8`, `{"n": -1}` into `u32`, `{"n": 200}` into `i8`).
/// `w` is 1/2/4/8 (caller-validated). A `w == 8` field spans the whole `i64`, so it always fits.
/// **`u64` note:** a width-8 *unsigned* (`u64`) field never reaches this function — the decode sites
/// route it through [`JsonParser::integer_field`] → [`JsonParser::integer_unsigned`], which parses the
/// full `[0, u64::MAX]` range directly (the `i64` [`JsonParser::integer`] path can't represent
/// `(i64::MAX, u64::MAX]`). This function still handles `(w >= 8, unsigned)` defensively (any `v >= 0`
/// fits), so it remains correct if ever called that way.
#[inline]
fn int_in_range(v: i64, w: usize, signed: bool) -> bool {
    // Defense in depth: a zero width is caller-validated as unreachable, but guard it so the
    // `bits - 1` below can never underflow (no width fits a zero-byte field anyway).
    if w == 0 {
        return false;
    }
    if signed {
        if w >= 8 {
            return true;
        }
        let bits = (w as u32) * 8;
        let min = -(1i64 << (bits - 1));
        let max = (1i64 << (bits - 1)) - 1;
        v >= min && v <= max
    } else {
        if v < 0 {
            return false;
        }
        if w >= 8 {
            return true;
        }
        let bits = (w as u32) * 8;
        let max = (1i64 << bits) - 1;
        v <= max
    }
}

/// Parse the JSON object in `input` into the zeroed struct at `out` (size `out_size`),
/// writing each known field per its descriptor. Returns 0 on success, nonzero on a parse
/// error or a missing/duplicate field. M5 cut: a flat object of integer / boolean values.
///
/// # Safety
/// `input`/`fields`/`out` must describe valid ranges for the call; `out` must have room for
/// the largest `offset + width`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_json_decode(
    input: *const u8,
    input_len: i64,
    fields: *const JsonField,
    n_fields: i64,
    out: *mut u8,
    out_size: i64,
    phf: *const i32,
    phf_len: i64,
    phf_seed: i64,
) -> i32 {
    let src: &[u8] = unsafe { safe_slice(input, input_len) };
    // A `str` field decoded from JSON is a zero-copy `{ptr,len}` view into `src`, so validating the
    // whole input once guarantees every `str` field is valid UTF-8 (draft §7/§12; the same one-shot
    // check simdjson does). Invalid → a decode error, before any view is handed out.
    if !validate_utf8(src) {
        return 1;
    }
    let descs: &[JsonField] = unsafe { safe_slice(fields, n_fields) };
    let phf = unsafe { phf_slice(phf, phf_len) };

    let mut p = JsonParser { src, pos: 0 };
    let ok = (|| -> Option<()> {
        unsafe { parse_object(&mut p, descs, out, out_size, phf, phf_seed as u64)? };
        p.ws();
        // Trailing garbage after the object is an error.
        if p.pos != src.len() {
            return None;
        }
        Some(())
    })();
    if ok.is_some() {
        0
    } else {
        1
    }
}

/// Tracks which declared fields `parse_object` has seen, to reject duplicates and require all of
/// them — without a per-object heap allocation in the common case. A struct of <= 64 fields uses a
/// `u64` bitmask; a wider one falls back to a `Vec<bool>` (MMv2 slice 8d perf).
enum SeenSet {
    Mask(u64),
    Big(Vec<bool>),
}

impl SeenSet {
    fn new(n: usize) -> Self {
        if n <= 64 {
            SeenSet::Mask(0)
        } else {
            SeenSet::Big(vec![false; n])
        }
    }

    /// Mark field `i` as seen; returns false if it was already seen (a duplicate). `i` is a valid
    /// descriptor index (`< n`), so for `Mask` it is `< 64` and the shift never overflows.
    fn mark(&mut self, i: usize) -> bool {
        match self {
            SeenSet::Mask(m) => {
                let bit = 1u64 << i;
                if *m & bit != 0 {
                    return false;
                }
                *m |= bit;
                true
            }
            SeenSet::Big(v) => {
                if v[i] {
                    return false;
                }
                v[i] = true;
                true
            }
        }
    }

    /// Whether all `n` declared fields have been marked.
    fn all_seen(&self, n: usize) -> bool {
        match self {
            // `Mask` is only used for `n <= 64`; `n == 64` needs `!0` (a `1 << 64` shift is UB).
            SeenSet::Mask(m) => *m == if n >= 64 { !0u64 } else { (1u64 << n) - 1 },
            SeenSet::Big(v) => v.iter().all(|&s| s),
        }
    }
}

/// The canonical `wyhash`, seeded — the hash behind the compile-time perfect-hash field dispatch.
/// Codegen's `build_phf` and this runtime probe **both** call `align_hash::wyhash`, so the slot a
/// field name maps to is byte-identical on the two ends by construction (see `align_codegen_llvm`'s
/// `phf_hash`). A thin wrapper keeps the call sites reading intent-first (`json_phf_hash(key, seed)`).
#[inline]
fn json_phf_hash(bytes: &[u8], seed: u64) -> u64 {
    align_hash::wyhash(bytes, seed)
}

/// Rebuild the perfect-hash slot table from the C ABI `(ptr, len)`: `None` (→ linear scan) when the
/// pointer is null or the length is non-positive, else the `[i32]` slot → index table.
///
/// # Safety
/// `(ptr, len)` must describe a valid `i32` range when `len > 0`.
unsafe fn phf_slice<'a>(ptr: *const i32, len: i64) -> Option<&'a [i32]> {
    if len <= 0 || ptr.is_null() {
        None
    } else {
        Some(unsafe { safe_slice(ptr, len) })
    }
}

/// Resolve `key` to a field-descriptor index, or `None` for an unknown key (which the caller skips).
/// With a perfect-hash table `phf` (slot → index, `-1` = empty; length is a power of two) this is
/// O(1): hash the key to a slot, then **one** name comparison confirms it (an unknown key can hash
/// into an occupied slot, so the compare is required for soundness). Without a table it falls back
/// to a linear scan (e.g. an empty struct, or when codegen found no collision-free table).
///
/// # Safety
/// Each descriptor's `name_ptr`/`name_len` must describe a valid byte range.
unsafe fn find_field(descs: &[JsonField], key: &[u8], phf: Option<&[i32]>, phf_seed: u64) -> Option<usize> {
    let name_of = |d: &JsonField| unsafe { std::slice::from_raw_parts(d.name_ptr, d.name_len.max(0) as usize) };
    match phf {
        Some(table) if !table.is_empty() => {
            // `table.len()` is a power of two (codegen guarantees it), so `& (len-1)` is the modulo.
            let slot = (json_phf_hash(key, phf_seed) & (table.len() as u64 - 1)) as usize;
            let idx = table[slot];
            if idx < 0 {
                return None;
            }
            let i = idx as usize;
            (i < descs.len() && name_of(&descs[i]) == key).then_some(i)
        }
        _ => descs.iter().position(|d| name_of(d) == key),
    }
}

/// Parse one JSON object from `p` into the (caller-zeroed) struct at `out` (`out_size` bytes) per
/// the field `descs`, leaving `p` positioned just past the closing `}`. Returns `None` on a parse
/// error, a missing or duplicate declared field, or an out-of-range descriptor. Shared by the
/// single-struct decode and the `array<Struct>` AoS decode (MMv2 slice 8d).
///
/// # Safety
/// `out` must point to `out_size` writable, already-zeroed bytes; each descriptor's `name_ptr`
/// must describe a valid byte range. `str` fields write a `{ptr,len}` view into `p`'s input.
unsafe fn parse_object(
    p: &mut JsonParser,
    descs: &[JsonField],
    out: *mut u8,
    out_size: i64,
    phf: Option<&[i32]>,
    phf_seed: u64,
) -> Option<()> {
    // `parse_object` runs once per array element (slice 8d), so avoid a per-object heap allocation:
    // a struct of <= 64 fields tracks "seen" in a `u64` bitmask, falling back to a `Vec` only for
    // a wider struct (which essentially never occurs).
    let mut seen = SeenSet::new(descs.len());
    p.ws();
    p.expect(b'{')?;
    p.ws();
    if p.peek() == Some(b'}') {
        p.pos += 1;
    } else {
        loop {
            p.ws();
            let key = p.string()?;
            p.ws();
            p.expect(b':')?;
            p.ws();
            // Find the matching field descriptor (unknown keys are skipped). O(1) via the
            // compile-time perfect-hash table when present, else a linear scan.
            let idx = unsafe { find_field(descs, key, phf, phf_seed) };
            match idx {
                Some(i) => {
                    if !seen.mark(i) {
                        return None; // duplicate field
                    }
                    let d = &descs[i];
                    // tag = (signed << 16) | (kind << 8) | byte-width. kind: 0 = int, 1 = bool,
                    // 2 = float, 3 = str; bit 16 = int sign flag.
                    let kind = (d.tag >> 8) & 0xff;
                    let width = (d.tag & 0xff) as i64;
                    // Defense in depth: never write outside the out struct, even if a
                    // descriptor offset/width were wrong (checked_add avoids i64 overflow).
                    if d.offset < 0 || d.offset.checked_add(width).is_none_or(|end| end > out_size) {
                        return None;
                    }
                    let off = d.offset as usize;
                    let w = width as usize;
                    match kind {
                        1 => {
                            if w != 1 {
                                return None;
                            }
                            let v = p.boolean()?;
                            unsafe { *out.add(off) = v as u8 };
                        }
                        2 => {
                            if w != 4 && w != 8 {
                                return None;
                            }
                            let v = p.number()?;
                            // Write the float repr at the field width (f32 / f64). `bytes` is a local
                            // (stack) array from `to_le_bytes()`, `out` is a distinct heap/arena
                            // buffer — the two never alias, so a straight-line bulk copy is sound.
                            if w == 4 {
                                let bytes = (v as f32).to_le_bytes();
                                unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.add(off), bytes.len()) };
                            } else {
                                let bytes = v.to_le_bytes();
                                unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.add(off), bytes.len()) };
                            }
                        }
                        3 => {
                            // str: a zero-copy `{ptr,len}` view into the input buffer.
                            // `string()` borrows the input and rejects escapes, so its
                            // pointer is the absolute address of the content within `src`.
                            if w != 16 {
                                return None;
                            }
                            let s = p.string()?;
                            let ptr_bytes = (s.as_ptr() as usize as u64).to_le_bytes();
                            let len_bytes = (s.len() as i64).to_le_bytes();
                            // Two disjoint 8-byte fields of the 16-byte `{ptr,len}` slot; `ptr_bytes`/
                            // `len_bytes` are local arrays, `out` is a distinct heap/arena buffer.
                            unsafe {
                                std::ptr::copy_nonoverlapping(ptr_bytes.as_ptr(), out.add(off), 8);
                                std::ptr::copy_nonoverlapping(len_bytes.as_ptr(), out.add(off + 8), 8);
                            }
                        }
                        _ => {
                            if w != 1 && w != 2 && w != 4 && w != 8 {
                                return None;
                            }
                            // Parse + range-check per the field's (width, sign); a `u64` field takes
                            // the full-range unsigned path. Rejects out-of-range instead of silently
                            // writing the low `w` bytes and truncating/sign-wrapping.
                            let v = p.integer_field(w, (d.tag & 0x1_0000) != 0)?;
                            let bytes = v.to_le_bytes();
                            // `w <= 8 == bytes.len()` (checked above); `bytes` is a local array, `out`
                            // a distinct heap/arena buffer.
                            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.add(off), w) };
                        }
                    }
                }
                None => p.skip_value()?,
            }
            p.ws();
            match p.peek() {
                Some(b',') => {
                    p.pos += 1;
                    continue;
                }
                Some(b'}') => {
                    p.pos += 1;
                    break;
                }
                _ => return None,
            }
        }
    }
    // All declared fields must be present.
    if seen.all_seen(descs.len()) {
        Some(())
    } else {
        None
    }
}

/// The declared field's name bytes (for key verification + dispatch).
#[inline]
unsafe fn field_name<'a>(d: &JsonField) -> &'a [u8] {
    if d.name_ptr.is_null() || d.name_len <= 0 {
        &[]
    } else {
        unsafe { safe_slice(d.name_ptr, d.name_len) }
    }
}

/// Skip JSON whitespace from `i` (a value may not start immediately after `:` in pretty JSON).
#[inline]
fn skip_ws_at(src: &[u8], mut i: usize) -> usize {
    while i < src.len() && matches!(src[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// Where a decoded field value is written. Abstracts the AoS (`eptr + d.offset`) and the SoA
/// direct-fill (`base + col_off + row*width`) destinations so the index-driven decode
/// ([`write_field_indexed`], [`json_speculate`], [`json_fallback`]) is single-sourced. Generic, so
/// each impl monomorphizes — the AoS hot path keeps the exact code it had before.
trait FieldDst {
    /// Resolve the writable destination for field index `fi` (descriptor `d`, byte `width`), or
    /// `None` if the field does not fit the layout. The returned pointer addresses `width` bytes.
    ///
    /// # Safety
    /// The returned pointer must be valid for `width` writable bytes.
    unsafe fn field_ptr(&self, fi: usize, d: &JsonField, width: i64) -> Option<*mut u8>;
}

/// AoS element slot of `esz` bytes; field `d` lands at `eptr + d.offset`.
struct AosDst {
    eptr: *mut u8,
    esz: i64,
}

impl FieldDst for AosDst {
    #[inline]
    unsafe fn field_ptr(&self, _fi: usize, d: &JsonField, width: i64) -> Option<*mut u8> {
        if d.offset < 0 || d.offset.checked_add(width).is_none_or(|end| end > self.esz) {
            return None;
        }
        Some(unsafe { self.eptr.add(d.offset as usize) })
    }
}

/// SoA direct fill: field `fi`'s column starts at `base + cols[fi].0` (the `align_up` column offset
/// for the known row count) with element stride `cols[fi].1`; row `row` lands at `+ row*stride`.
struct SoaDst<'a> {
    base: *mut u8,
    row: usize,
    cols: &'a [(usize, usize)],
}

impl FieldDst for SoaDst<'_> {
    #[inline]
    unsafe fn field_ptr(&self, fi: usize, _d: &JsonField, width: i64) -> Option<*mut u8> {
        let (off, stride) = self.cols[fi];
        // The column stride must equal the field's declared width, or the schema and layout disagree.
        if stride as i64 != width {
            return None;
        }
        Some(unsafe { self.base.add(off + self.row * stride) })
    }
}

/// The decode-time constants shared by [`json_speculate`]/[`json_fallback`]: the field descriptors,
/// the write destination, and the (optional) perfect-hash table used by `find_field`. Grouping these
/// keeps both functions under the argument-count lint without losing any parameter.
struct DecodeCtx<'a, D: FieldDst> {
    descs: &'a [JsonField],
    dst: &'a D,
    phf: Option<&'a [i32]>,
    phf_seed: u64,
}

/// Write field `d`'s value into `dst` during the index-driven (Mison-style) decode. `k` is the
/// colon's position in the structural index `idx`; a `str` value's delimiter quotes are
/// `idx[k+1]`/`idx[k+2]`, a scalar is parsed from the first non-space byte after the `:`. `fi` is the
/// field's descriptor index (used by SoA to pick its column). Matches [`parse_object`]'s per-kind
/// writes exactly.
///
/// # Safety
/// `dst` must resolve to `width` writable bytes for the field.
#[inline]
unsafe fn write_field_indexed<D: FieldDst>(src: &[u8], idx: &[u32], k: usize, fi: usize, d: &JsonField, dst: &D) -> Option<()> {
    let kind = (d.tag >> 8) & 0xff;
    let width = (d.tag & 0xff) as i64;
    let p = unsafe { dst.field_ptr(fi, d, width)? };
    let w = width as usize;
    let colon = idx[k] as usize;
    let mut vp = JsonParser { src, pos: skip_ws_at(src, colon + 1) };
    if kind == 3 {
        // str: a zero-copy `{ptr,len}` view. The lean index holds no value quotes, so scan the
        // string from the raw bytes via `string()` (which borrows the input and rejects escapes).
        if w != 16 {
            return None;
        }
        let s = vp.string()?;
        let pb = (s.as_ptr() as usize as u64).to_le_bytes();
        let lb = (s.len() as i64).to_le_bytes();
        // Two disjoint 8-byte fields of the 16-byte `{ptr,len}` slot; `pb`/`lb` are local arrays,
        // `p` is the field's own destination (never aliases a local).
        unsafe {
            std::ptr::copy_nonoverlapping(pb.as_ptr(), p, 8);
            std::ptr::copy_nonoverlapping(lb.as_ptr(), p.add(8), 8);
        }
        return Some(());
    }
    match kind {
        1 => {
            if w != 1 {
                return None;
            }
            let v = vp.boolean()?;
            unsafe { *p = v as u8 };
        }
        2 => {
            if w != 4 && w != 8 {
                return None;
            }
            let v = vp.number()?;
            // `b` is a local array from `to_le_bytes()`, `p` the field's own destination — disjoint.
            if w == 4 {
                let b = (v as f32).to_le_bytes();
                unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
            } else {
                let b = v.to_le_bytes();
                unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
            }
        }
        _ => {
            if w != 1 && w != 2 && w != 4 && w != 8 {
                return None;
            }
            // Parse + range-check per the field's (width, sign); `u64` uses the full-range path —
            // see `parse_object` / `JsonParser::integer_field`.
            let v = vp.integer_field(w, (d.tag & 0x1_0000) != 0)?;
            let b = v.to_le_bytes();
            // `w <= 8 == b.len()` (checked above); `b` is a local array, `p` a distinct destination.
            unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), p, w) };
        }
    }
    Some(())
}

/// Mison **speculation** fast path: the record's colon count matched the learned pattern, so for each
/// declared field at its learned ordinal, **verify** the key (a byte compare) and write the value —
/// no `find_field` hashing, and the unqueried fields' *values* are never parsed. Returns `false` on
/// any key mismatch (the caller then falls back); a partial write is harmless (the fallback overwrites
/// the slot or errors). `rec_cols[o]` is the index position of the record's o-th colon; `pat_field[o]`
/// is the declared field at ordinal `o`, or `-1` for an unqueried position.
///
/// **Duplicate-key soundness (the strict `json.decode` contract, `docs/open-questions.md`):** at a
/// queried ordinal a duplicated declared field displaces some declared field and trips its key verify
/// → fallback → error, so those are already caught. The one gap was an *unqueried* position: the
/// pattern learned it from an undeclared key, but a later record can put a **declared** field name
/// there — a duplicate of the field already written at its own ordinal — which the strict contract
/// must reject. So an unqueried position is not skipped blindly: its key is delimited and checked
/// against the declared set, and on a declared hit (or a key that can't be cleanly delimited, which
/// the fallback also rejects) speculation returns `false` so [`json_fallback`] surfaces it as a decode
/// error. The projection win is preserved — an ordinary undeclared key delimits cleanly and
/// `find_field` returns `None` (one PHF probe into an empty/mismatched slot), so the fast path
/// continues without parsing that field's value.
///
/// # Safety
/// `dst` must resolve to writable bytes for every written field.
unsafe fn json_speculate<D: FieldDst>(
    src: &[u8],
    idx: &[u32],
    rec_cols: &[usize],
    pat_field: &[i32],
    ctx: &DecodeCtx<D>,
) -> bool {
    for (o, &k) in rec_cols.iter().enumerate() {
        let fi = pat_field[o];
        if fi < 0 {
            // An unqueried position (projection): still confirm its key is a plain, *undeclared* key,
            // or fall back so the strict duplicate/missing/malformed contract is enforced there.
            match key_before_colon(src, idx[k] as usize) {
                Some(key) if unsafe { find_field(ctx.descs, key, ctx.phf, ctx.phf_seed) }.is_none() => continue,
                _ => return false,
            }
        }
        let d = &ctx.descs[fi as usize];
        if !key_matches_before_colon(src, idx[k] as usize, unsafe { field_name(d) }) {
            return false; // structure drifted from the pattern — fall back
        }
        if unsafe { write_field_indexed(src, idx, k, fi as usize, d, ctx.dst) }.is_none() {
            return false;
        }
    }
    true
}

/// Mison fallback / learn path: scan the record's colons with `find_field` (enforcing unknown-skip,
/// duplicate-reject, all-declared-present), writing each declared field, and **(re)learn** the
/// pattern into `pat_field` (ordinal → declared field, `-1` for unqueried) for subsequent speculation.
/// Returns `None` on a duplicate or a missing declared field (the strict decode semantics).
///
/// # Safety
/// `dst` must resolve to writable bytes for every written field.
unsafe fn json_fallback<D: FieldDst>(
    src: &[u8],
    idx: &[u32],
    rec_cols: &[usize],
    ctx: &DecodeCtx<D>,
    seen: &mut SeenSet,
    pat_field: &mut Vec<i32>,
) -> Option<()> {
    *seen = SeenSet::new(ctx.descs.len());
    pat_field.clear();
    pat_field.resize(rec_cols.len(), -1);
    for (o, &k) in rec_cols.iter().enumerate() {
        let Some(key) = key_before_colon(src, idx[k] as usize) else {
            return None; // a `:` not preceded by a `"..."` key — malformed object
        };
        if let Some(fi) = unsafe { find_field(ctx.descs, key, ctx.phf, ctx.phf_seed) } {
            if !seen.mark(fi) {
                return None; // duplicate field
            }
            pat_field[o] = fi as i32;
            unsafe { write_field_indexed(src, idx, k, fi, &ctx.descs[fi], ctx.dst)? };
        }
    }
    if seen.all_seen(ctx.descs.len()) {
        Some(())
    } else {
        None // a declared field is missing
    }
}

/// Parse the JSON array of objects in `input` into a freshly heap-allocated owned `array<Struct>`
/// (AoS), writing the materialized `{ptr, len}` (len = element count) to `out` (MMv2 slice 8d,
/// the draft.md §19 headline). Each object is decoded by [`parse_object`] per the `fields`
/// descriptors; `str` fields are zero-copy `{ptr,len}` views into `input`, so the result is
/// owned (the buffer is freed by the generated `Drop`) yet borrows `input` for its string content
/// — the compiler region-ties it to `input`. Returns 0 on success, 1 on a parse error (leaving
/// `out` as the caller-zeroed `{null,0}`). An empty array allocates nothing (null buffer).
///
/// # Safety
/// `input`/`fields` must describe valid ranges; `elem_size` is the struct stride in bytes; `out`
/// must point to a writable `{ptr,len}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_json_decode_struct_array(
    input: *const u8,
    input_len: i64,
    fields: *const JsonField,
    n_fields: i64,
    elem_size: i64,
    out: *mut AlignStr,
    phf: *const i32,
    phf_len: i64,
    phf_seed: i64,
) -> i32 {
    let src: &[u8] = unsafe { safe_slice(input, input_len) };
    // Validate the whole input once — every decoded `str` field is a zero-copy view into `src`, so
    // this covers all of them (draft §7/§12). Invalid UTF-8 → a decode error.
    if !validate_utf8(src) {
        return 1;
    }
    let descs: &[JsonField] = unsafe { safe_slice(fields, n_fields) };
    let phf = unsafe { phf_slice(phf, phf_len) };
    let phf_seed = phf_seed as u64;
    let esz = elem_size.max(0) as usize;
    let mut buf: Vec<u8> = Vec::new();
    let mut count: i64 = 0;

    // Stage 1: index the whole input once (SIMD); stage 2 is a Mison-style speculative walk.
    // Pre-reserve so per-element growth never reallocates; decoded `str` fields point into `src`,
    // not `buf`, so a reallocation would not invalidate them anyway.
    let mut idx: Vec<u32> = Vec::new();
    json_decode_index(src, &mut idx);
    if esz > 0 {
        buf.reserve((src.len() / 24).saturating_mul(esz).min(1 << 30));
    }

    // Bracket depth tracks structure (1 = the array, 2 = a record, 3+ = a nested value). A record's
    // colon index-positions accumulate in `rec_cols`; at its `}` the record decodes by **speculation**
    // (jump to each declared field's learned colon ordinal, verify the key — no `find_field`) when its
    // colon count matches the learned `pat`, else by the `find_field` fallback (which also relearns).
    let mut pat_ncol: i64 = -1;
    let mut pat_field: Vec<i32> = Vec::new();
    let mut rec_cols: Vec<usize> = Vec::new();
    let mut seen = SeenSet::new(descs.len());
    let mut eoff = 0usize;
    let mut depth: i32 = 0;
    let mut started = false;
    let mut array_close = 0usize;
    let ok = (|| -> Option<()> {
        for k in 0..idx.len() {
            let pos = idx[k] as usize;
            match src[pos] {
                b'[' => {
                    depth += 1;
                    if depth == 1 {
                        started = true;
                    }
                }
                b'{' => {
                    depth += 1;
                    if depth == 2 {
                        eoff = buf.len();
                        buf.resize(eoff + esz, 0);
                        rec_cols.clear();
                    }
                }
                b':' if depth == 2 => rec_cols.push(k),
                b'}' => {
                    if depth == 2 {
                        let eptr = unsafe { buf.as_mut_ptr().add(eoff) };
                        let dst = AosDst { eptr, esz: esz as i64 };
                        let ctx = DecodeCtx { descs, dst: &dst, phf, phf_seed };
                        let spec = pat_ncol == rec_cols.len() as i64
                            && unsafe { json_speculate(src, &idx, &rec_cols, &pat_field, &ctx) };
                        if !spec {
                            unsafe { json_fallback(src, &idx, &rec_cols, &ctx, &mut seen, &mut pat_field)? };
                            pat_ncol = rec_cols.len() as i64;
                        }
                        count += 1;
                    }
                    depth -= 1;
                }
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        // The top-level array has closed; any further structural token is trailing
                        // garbage (e.g. a second `[...]`), so reject it directly.
                        if k != idx.len() - 1 {
                            return None;
                        }
                        array_close = pos;
                    }
                }
                _ => {}
            }
        }
        if !started || depth != 0 {
            return None;
        }
        // No non-whitespace may follow the array's closing `]` (catches non-structural junk like `]x`).
        if src[array_close + 1..].iter().any(|&b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r')) {
            return None;
        }
        Some(())
    })();
    if ok.is_none() {
        return 1;
    }

    // Materialize into a fresh heap buffer (owned; freed by the generated `Drop`). An empty array
    // allocates nothing — `align_rt_alloc(0)` returns null and `free(null)` is a no-op.
    let total = buf.len() as i64;
    let dst = align_rt_alloc(total);
    if total > 0 {
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), dst, buf.len()) };
    }
    unsafe { *out = AlignStr { ptr: dst, len: count } };
    0
}

/// Column-major layout for a `soa<Struct>` of `n` rows, given each field's byte `width` in field
/// order. Returns `(cols, total_bytes, max_align)` where `cols[j] = (byte_offset, width)` for
/// column `j`, or `None` if the byte size overflows `usize` (a pathological row count × width on a
/// 32-bit target — checked because `n` comes from untrusted input). **MUST match codegen's
/// `soa_column_offset` / `SoaAlloc` in `align_codegen_llvm`** (`start_0 = 0`,
/// `start_j = align_up(start_{j-1} + n*size_{j-1}, size_j)`), or a direct-filled column would land
/// where a later `IndexColumn` scan does not read it.
/// A `soa<Struct>` column layout: `(cols, total_bytes, max_align)`, see [`soa_layout`].
type SoaLayout = (Vec<(usize, usize)>, usize, usize);

fn soa_layout(widths: &[usize], n: usize) -> Option<SoaLayout> {
    let mut cols = Vec::with_capacity(widths.len());
    let mut off = 0usize;
    for (j, &w) in widths.iter().enumerate() {
        if j > 0 && w > 1 {
            // align_up(off, w) — each column starts at its field's alignment (= its width).
            let mask = w - 1;
            off = off.checked_add(mask)? & !mask;
        }
        cols.push((off, w));
        off = off.checked_add(n.checked_mul(w)?)?; // advance past this column's `n` elements
    }
    let max_align = widths.iter().copied().max().unwrap_or(1);
    Some((cols, off, max_align))
}

/// `json.decode(input)` straight into a column-major `soa<Struct>` (the direct-fill rail) — no AoS
/// intermediate, no transpose. Two passes over the SIMD structural index: pass 1 counts records (so
/// the column offsets, which depend on the row count, can be computed) and validates the array
/// structure; pass 2 decodes each record's values directly into its columns via the shared
/// [`json_speculate`]/[`json_fallback`] writers with a [`SoaDst`]. The column buffer is bump-allocated
/// from `arena` (so the result is region-tied and bulk-freed, like the transpose path it replaces).
/// Writes the soa `{ptr, len}` view (len = row count) to `out`. Returns 0 on success, 1 on a parse
/// error (leaving `out` as the caller-zeroed `{null,0}`). An empty array allocates nothing.
///
/// # Safety
/// `input`/`fields` must describe valid ranges; `arena` must be a live arena handle; `out` must point
/// to a writable `{ptr,len}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_json_decode_soa(
    input: *const u8,
    input_len: i64,
    fields: *const JsonField,
    n_fields: i64,
    arena: *mut Arena,
    out: *mut AlignStr,
    phf: *const i32,
    phf_len: i64,
    phf_seed: i64,
) -> i32 {
    // The arena (for the column buffer) and `out` (for the soa view) are dereferenced below; a null
    // here is a caller bug, but fail closed rather than risk UB on untrusted-input-driven sizes.
    if arena.is_null() || out.is_null() {
        return 1;
    }
    let src: &[u8] = unsafe { safe_slice(input, input_len) };
    // Validate the whole input once — every decoded `str` column entry is a zero-copy view into
    // `src`, so this covers all of them (draft §7/§12). Invalid UTF-8 → a decode error.
    if !validate_utf8(src) {
        return 1;
    }
    let descs: &[JsonField] = unsafe { safe_slice(fields, n_fields) };
    let phf = unsafe { phf_slice(phf, phf_len) };
    let phf_seed = phf_seed as u64;

    let mut idx: Vec<u32> = Vec::new();
    json_decode_index(src, &mut idx);

    // Pass 1: validate the array structure and count records (no value parsing). The bracket-depth
    // logic mirrors the AoS walk; a depth-2 `{` opens a record.
    let mut depth: i32 = 0;
    let mut started = false;
    let mut n_rows: usize = 0;
    let mut array_close = 0usize;
    let count_ok = (|| -> Option<()> {
        for k in 0..idx.len() {
            let pos = idx[k] as usize;
            match src[pos] {
                b'[' => {
                    depth += 1;
                    if depth == 1 {
                        started = true;
                    }
                }
                b'{' => {
                    depth += 1;
                    if depth == 2 {
                        n_rows += 1;
                    }
                }
                b'}' => depth -= 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        if k != idx.len() - 1 {
                            return None; // trailing structural token after the array closed
                        }
                        array_close = pos;
                    }
                }
                _ => {}
            }
        }
        if !started || depth != 0 {
            return None;
        }
        if src[array_close + 1..].iter().any(|&b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r')) {
            return None;
        }
        Some(())
    })();
    if count_ok.is_none() {
        return 1;
    }

    // An empty array allocates nothing — the soa view is `{null, 0}`.
    if n_rows == 0 {
        unsafe { *out = AlignStr { ptr: core::ptr::null_mut(), len: 0 } };
        return 0;
    }

    // Lay out the columns for `n_rows` rows and bump-allocate the buffer from the arena.
    let widths: Vec<usize> = descs.iter().map(|d| (d.tag & 0xff) as usize).collect();
    let Some((cols, total, max_align)) = soa_layout(&widths, n_rows) else {
        return 1; // byte size overflowed usize — reject rather than under-allocate
    };
    let Ok(total_i64) = i64::try_from(total) else {
        return 1; // size doesn't fit the i64 arena ABI
    };
    let base = unsafe { align_rt_arena_alloc(arena, total_i64, max_align.max(1) as i64) };
    // The arena hands back zeroed chunks, but a missing field must still read 0 — zero defensively
    // (cheap relative to the parse) so a partial record leaves declared columns at 0, like the AoS
    // path's per-element `buf.resize(.., 0)`.
    if !base.is_null() && total > 0 {
        unsafe { core::ptr::write_bytes(base, 0, total) };
    }

    // Pass 2: decode each record's values directly into its columns.
    let mut pat_ncol: i64 = -1;
    let mut pat_field: Vec<i32> = Vec::new();
    let mut rec_cols: Vec<usize> = Vec::new();
    let mut seen = SeenSet::new(descs.len());
    let mut row: usize = 0;
    let mut depth: i32 = 0;
    let fill_ok = (|| -> Option<()> {
        for k in 0..idx.len() {
            let pos = idx[k] as usize;
            match src[pos] {
                b'[' => depth += 1,
                b'{' => {
                    depth += 1;
                    if depth == 2 {
                        rec_cols.clear();
                    }
                }
                b':' if depth == 2 => rec_cols.push(k),
                b'}' => {
                    if depth == 2 {
                        let dst = SoaDst { base, row, cols: &cols };
                        let ctx = DecodeCtx { descs, dst: &dst, phf, phf_seed };
                        let spec = pat_ncol == rec_cols.len() as i64
                            && unsafe { json_speculate(src, &idx, &rec_cols, &pat_field, &ctx) };
                        if !spec {
                            unsafe { json_fallback(src, &idx, &rec_cols, &ctx, &mut seen, &mut pat_field)? };
                            pat_ncol = rec_cols.len() as i64;
                        }
                        row += 1;
                    }
                    depth -= 1;
                }
                b']' => depth -= 1,
                _ => {}
            }
        }
        Some(())
    })();
    if fill_ok.is_none() {
        return 1;
    }
    unsafe { *out = AlignStr { ptr: base, len: row as i64 } };
    0
}

/// Parse the JSON array in `input` into a freshly heap-allocated owned `array<T>`, writing the
/// materialized `{ptr, len}` (len = element count) to `out` (MMv2 slice 8c). `elem_tag` is the
/// element encoding `(signed << 16) | (kind << 8) | byte-width` (kind 0 = int, 1 = bool, 2 = float;
/// bit 16 = int sign flag), matching the struct-field tags. Elements are *copied* into the new
/// buffer (not borrowed), so the result is
/// owned and freed by the generated `Drop`. Returns 0 on success, 1 on a parse error (leaving
/// `out` as the caller-zeroed `{null,0}`). An empty array allocates nothing (null buffer).
///
/// # Safety
/// `input` must describe a valid byte range; `out` must point to a writable `{ptr,len}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_json_decode_array(
    input: *const u8,
    input_len: i64,
    elem_tag: i32,
    out: *mut AlignStr,
) -> i32 {
    let src: &[u8] = unsafe { safe_slice(input, input_len) };
    // A JSON `array<str>` element is a zero-copy view into `src`; validate the whole input once so
    // every element is valid UTF-8 (draft §7/§12). Invalid UTF-8 → a decode error. (A scalar-element
    // array carries no `str`, but the invariant that a decoded `str` is UTF-8 is uniform, so the
    // one-shot check stays at every `json.decode` entry.)
    if !validate_utf8(src) {
        return 1;
    }
    let kind = (elem_tag >> 8) & 0xff;
    let width = (elem_tag & 0xff) as usize;
    let signed = (elem_tag & 0x1_0000) != 0;
    let mut bytes: Vec<u8> = Vec::new();
    let mut count: i64 = 0;

    let mut p = JsonParser { src, pos: 0 };
    let ok = (|| -> Option<()> {
        p.ws();
        p.expect(b'[')?;
        p.ws();
        if p.peek() == Some(b']') {
            p.pos += 1;
        } else {
            loop {
                p.ws();
                match kind {
                    1 => {
                        // bool — always one byte.
                        if width != 1 {
                            return None;
                        }
                        bytes.push(p.boolean()? as u8);
                    }
                    2 => {
                        // float — f32 (4) or f64 (8).
                        let v = p.number()?;
                        match width {
                            4 => bytes.extend_from_slice(&(v as f32).to_le_bytes()),
                            8 => bytes.extend_from_slice(&v.to_le_bytes()),
                            _ => return None,
                        }
                    }
                    _ => {
                        // int — write the low `width` bytes of the i64 (two's-complement LE),
                        // matching how struct int fields are written.
                        if !matches!(width, 1 | 2 | 4 | 8) {
                            return None;
                        }
                        // Parse + range-check per the element's (width, sign); `u64` uses the
                        // full-range path — see `parse_object` / `JsonParser::integer_field`. Rejects
                        // out-of-range instead of silently truncating/sign-wrapping.
                        let v = p.integer_field(width, signed)?;
                        bytes.extend_from_slice(&v.to_le_bytes()[..width]);
                    }
                }
                count += 1;
                p.ws();
                match p.peek() {
                    Some(b',') => {
                        p.pos += 1;
                        continue;
                    }
                    Some(b']') => {
                        p.pos += 1;
                        break;
                    }
                    _ => return None,
                }
            }
        }
        p.ws();
        // Trailing garbage after the array is an error.
        if p.pos != src.len() {
            return None;
        }
        Some(())
    })();
    if ok.is_none() {
        return 1;
    }

    // Materialize into a fresh heap buffer (owned; freed by the generated `Drop`). An empty
    // array allocates nothing — `align_rt_alloc(0)` returns null and `free(null)` is a no-op.
    let total = bytes.len() as i64;
    let dst = align_rt_alloc(total);
    if total > 0 {
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len()) };
    }
    unsafe { *out = AlignStr { ptr: dst, len: count } };
    0
}

/// Column-oriented grouped sum (`group_by` first slice): for each `i`, accumulate `vals[i]` into the
/// bucket for `keys[i]`, then emit the distinct keys and their sums into `out_keys`/`out_vals`
/// (caller-provided, capacity `cap`). Returns the group count, or `-1` if it would exceed `cap`
/// (the caller sizes `cap = len`, an upper bound, so that never trips on valid input).
///
/// Mechanism: an internal open-addressing (linear-probe) table sized to the next power of two ≥ 2·len
/// — a primitive-key, no-boxing, cache-tight aggregate, the lever vs Rust's generic `HashMap`. Sums
/// wrap on overflow (Align's defined two's-complement wrap). Output order is table order (groups are
/// unordered). The keys/values are read sequentially (soa columns). The table is a heap `Vec` (one
/// allocation per call, amortized over all `len` elements) — keeping this primitive self-contained
/// and unit-testable; allocating it in the caller's arena (to drop even that one `malloc` when
/// `group_by` runs in a hot loop) is a recorded refinement once the language wiring threads an arena.
///
/// Slot index for a key: Fibonacci multiply + an XOR-fold so the low bits used by `& mask` are
/// well-distributed at any (power-of-two) table size.
#[inline]
fn group_slot(k: i64, mask: usize) -> usize {
    let h = (k as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    (h ^ (h >> 29)) as usize & mask
}

/// Generic column-oriented group-aggregate over i64 keys. `per_row(i)` is the value folded for row
/// `i` (`vals[i]` for sum/min/max; `1` for count); `combine(acc, v)` folds it into the per-group
/// accumulator (the first row of a group seeds the accumulator with its value). Emits the distinct
/// keys + their accumulators into `out_keys`/`out_vals`. Returns the group count, or -1 if it would
/// exceed `cap`. Monomorphized per op so `per_row`/`combine` inline (no per-element branch).
///
/// Mechanism: two strategies, picked by the key range (one O(n) min/max pre-scan decides).
/// - **Dense path** — when the keys span a tight integer range (`max - min < n`, so a direct-indexed
///   accumulator never exceeds the key column itself), aggregate by `acc[key - min]`: no hashing, no
///   probing, keys emitted already sorted. This is the big win on dense-id columns (enum / category /
///   small-int keys) — direct indexing beats a hash table by ~an order of magnitude (`bench/group_by`).
/// - **Hash path** — otherwise, an open-addressing (linear-probe) table that grows to track the live
///   group count (doubling past a 0.75 load) — a primitive-key, no-boxing, cache-tight aggregate, the
///   lever vs Rust's generic `HashMap`. Three dense parallel arrays (key / acc / used) probe-scan well
///   (a naive interleaved-slot layout measured *worse*; `docs/open-questions.md`).
///
/// The `max - min < n` guard keeps the dense array bounded by the input (so a sparse-but-wide key set
/// — e.g. a few keys at the extremes of a huge range — falls back to the hash table rather than
/// allocating a giant mostly-empty array); the pre-scan bails early the moment the span exceeds `n`.
///
/// # Safety
/// `out_keys`/`out_vals` must each be valid for `cap` `i64` writes (they're written for the emitted
/// group count, which is `≤ cap`).
unsafe fn group_agg_i64(
    keys: &[i64],
    out_keys: *mut i64,
    out_vals: *mut i64,
    cap: i64,
    per_row: impl Fn(usize) -> i64,
    combine: impl Fn(i64, i64) -> i64,
) -> i64 {
    let n = keys.len();
    if n == 0 {
        return 0;
    }
    // The output is the same regardless of strategy, so reject an invalid output up front — before
    // any pre-scan or table/accumulator allocation (a null out always returns -1, never a count).
    if cap < 0 || out_keys.is_null() || out_vals.is_null() {
        return -1;
    }

    // Pre-scan for the key range, bailing out of the dense path the instant the span reaches `n`
    // (so a sparse key set pays only a partial scan before falling through to the hash table). The
    // span is only checked when `kmin`/`kmax` actually move — a key already inside `[kmin, kmax]`
    // costs nothing. `i128` is required here: before density is established the span can exceed
    // `i64` (e.g. keys at both `i64::MIN` and `i64::MAX`).
    let mut kmin = keys[0];
    let mut kmax = keys[0];
    let limit = n as i128; // dense requires span + 1 <= n, i.e. span < n.
    let mut dense = true;
    for &k in &keys[1..] {
        if k < kmin {
            kmin = k;
            if (kmax as i128 - kmin as i128) >= limit {
                dense = false;
                break;
            }
        } else if k > kmax {
            kmax = k;
            if (kmax as i128 - kmin as i128) >= limit {
                dense = false;
                break;
            }
        }
    }

    if dense {
        // span = kmax - kmin < n, so `slots` fits and the accumulator is no larger than the keys.
        // Density guarantees `kmin <= k <= kmax` with span < n, so `k - kmin` is in `[0, n)` — it
        // never overflows `i64`, so the hot loop stays in `i64` (no `i128` per element).
        let slots = (kmax - kmin) as usize + 1;
        let mut acc = vec![0i64; slots];
        let mut occ = vec![false; slots];
        let mut count: usize = 0;
        for (i, &k) in keys.iter().enumerate() {
            let idx = (k - kmin) as usize;
            let v = per_row(i);
            unsafe {
                if *occ.get_unchecked(idx) {
                    *acc.get_unchecked_mut(idx) = combine(*acc.get_unchecked(idx), v);
                } else {
                    *occ.get_unchecked_mut(idx) = true;
                    *acc.get_unchecked_mut(idx) = v;
                    count += 1;
                    if count > cap as usize {
                        return -1;
                    }
                }
            }
        }
        let out_keys = unsafe { std::slice::from_raw_parts_mut(out_keys, count) };
        let out_vals = unsafe { std::slice::from_raw_parts_mut(out_vals, count) };
        let mut g = 0;
        for s in 0..slots {
            unsafe {
                if *occ.get_unchecked(s) {
                    out_keys[g] = kmin + s as i64; // kmin + span = kmax, so this never overflows i64.
                    out_vals[g] = *acc.get_unchecked(s);
                    g += 1;
                }
            }
        }
        return count as i64;
    }

    let mut tsize = 16usize;
    let mut mask = tsize - 1;
    let mut tkey = vec![0i64; tsize];
    let mut tacc = vec![0i64; tsize];
    let mut occ = vec![false; tsize];
    let mut count: usize = 0;

    for (i, &k) in keys.iter().enumerate() {
        let v = per_row(i);
        let mut slot = group_slot(k, mask);
        loop {
            unsafe {
                if !*occ.get_unchecked(slot) {
                    *occ.get_unchecked_mut(slot) = true;
                    *tkey.get_unchecked_mut(slot) = k;
                    *tacc.get_unchecked_mut(slot) = v;
                    count += 1;
                    if count > cap as usize {
                        return -1;
                    }
                    if count > tsize / 4 * 3 {
                        let ns = tsize.checked_mul(2).unwrap_or_else(|| panic_abort("group_agg table overflow"));
                        let nm = ns - 1;
                        let mut nk = vec![0i64; ns];
                        let mut na = vec![0i64; ns];
                        let mut no = vec![false; ns];
                        for s in 0..tsize {
                            if *occ.get_unchecked(s) {
                                let mut t = group_slot(*tkey.get_unchecked(s), nm);
                                while *no.get_unchecked(t) {
                                    t = (t + 1) & nm;
                                }
                                *no.get_unchecked_mut(t) = true;
                                *nk.get_unchecked_mut(t) = *tkey.get_unchecked(s);
                                *na.get_unchecked_mut(t) = *tacc.get_unchecked(s);
                            }
                        }
                        tkey = nk;
                        tacc = na;
                        occ = no;
                        tsize = ns;
                        mask = nm;
                    }
                    break;
                }
                if *tkey.get_unchecked(slot) == k {
                    *tacc.get_unchecked_mut(slot) = combine(*tacc.get_unchecked(slot), v);
                    break;
                }
            }
            slot = (slot + 1) & mask;
        }
    }

    let out_keys = unsafe { std::slice::from_raw_parts_mut(out_keys, count) };
    let out_vals = unsafe { std::slice::from_raw_parts_mut(out_vals, count) };
    let mut g = 0;
    for slot in 0..tsize {
        if occ[slot] {
            out_keys[g] = tkey[slot];
            out_vals[g] = tacc[slot];
            g += 1;
        }
    }
    count as i64
}

/// `keys`/`vals` as `&[i64]` of `len`, or empty slices when degenerate (null / non-positive). The
/// sum/min/max wrappers need `keys` and `vals` the same length.
unsafe fn group_io<'a>(keys: *const i64, vals: *const i64, len: i64) -> (&'a [i64], &'a [i64]) {
    let keys_slice = unsafe { safe_slice(keys, len) };
    let vals_slice = unsafe { safe_slice(vals, len) };
    if keys_slice.is_empty() || vals_slice.is_empty() || keys_slice.len() != vals_slice.len() {
        (&[], &[])
    } else {
        (keys_slice, vals_slice)
    }
}

/// `group_by(.key).sum(.value)` — per-group sum. Wraps + sums in row order.
///
/// # Safety
/// `keys`/`vals` must be valid for `len` `i64`s; `out_keys`/`out_vals` for `cap` `i64`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_sum_i64(keys: *const i64, vals: *const i64, len: i64, out_keys: *mut i64, out_vals: *mut i64, cap: i64) -> i64 {
    let (keys, vals) = unsafe { group_io(keys, vals, len) };
    unsafe {
        group_agg_i64(keys, out_keys, out_vals, cap, |i| {
            // SAFETY: `group_agg_i64` only calls `per_row` with indices in `0..keys.len()`.
            // Since `keys` and `vals` have the same length (guaranteed by `group_io`), `i` is always in-bounds for `vals`.
            unsafe { *vals.get_unchecked(i) }
        }, |a, b| a.wrapping_add(b))
    }
}

/// `group_by(.key).min(.value)` — per-group minimum.
///
/// # Safety
/// Same as [`align_rt_group_sum_i64`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_min_i64(keys: *const i64, vals: *const i64, len: i64, out_keys: *mut i64, out_vals: *mut i64, cap: i64) -> i64 {
    let (keys, vals) = unsafe { group_io(keys, vals, len) };
    unsafe {
        group_agg_i64(keys, out_keys, out_vals, cap, |i| {
            // SAFETY: `group_agg_i64` only calls `per_row` with indices in `0..keys.len()`.
            // Since `keys` and `vals` have the same length (guaranteed by `group_io`), `i` is always in-bounds for `vals`.
            unsafe { *vals.get_unchecked(i) }
        }, |a, b| a.min(b))
    }
}

/// `group_by(.key).max(.value)` — per-group maximum.
///
/// # Safety
/// Same as [`align_rt_group_sum_i64`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_max_i64(keys: *const i64, vals: *const i64, len: i64, out_keys: *mut i64, out_vals: *mut i64, cap: i64) -> i64 {
    let (keys, vals) = unsafe { group_io(keys, vals, len) };
    unsafe {
        group_agg_i64(keys, out_keys, out_vals, cap, |i| {
            // SAFETY: `group_agg_i64` only calls `per_row` with indices in `0..keys.len()`.
            // Since `keys` and `vals` have the same length (guaranteed by `group_io`), `i` is always in-bounds for `vals`.
            unsafe { *vals.get_unchecked(i) }
        }, |a, b| a.max(b))
    }
}

/// `group_by(.key).count()` — per-group row count (no value column).
///
/// # Safety
/// `keys` must be valid for `len` `i64`s; `out_keys`/`out_vals` for `cap` `i64`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_count_i64(keys: *const i64, len: i64, out_keys: *mut i64, out_vals: *mut i64, cap: i64) -> i64 {
    let Ok(len_u) = safe_len(len) else { return 0 };
    let keys: &[i64] = if len_u == 0 || keys.is_null() || len_u > isize::MAX as usize / 8 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(keys, len_u) }
    };
    unsafe { group_agg_i64(keys, out_keys, out_vals, cap, |_| 1, |a, b| a.wrapping_add(b)) }
}

/// Generic str-keyed group-aggregate over an AoS `array<Struct>` — the **dictionary-id rail**.
///
/// The struct array is `base` (a `[%Struct]` buffer), `n` rows of `stride` bytes; each row's `str`
/// key is an `AlignStr` (`{ptr,len}`) at `key_off`. We **intern** each distinct key to a dense id
/// while scanning (a hash over the key *bytes* assigning ids `0, 1, 2, …`), recording the first
/// occurrence's view as the group's representative; `value_at(i)` is the row `i` value to fold
/// (`vals[i]` for sum/min/max, `1` for count) and `combine(acc, v)` folds it into the per-group
/// accumulator (the first row of a group seeds it). The values aggregate by id into a dense `Vec`,
/// so the per-row work after interning is direct-index, not hashing — versus a `HashMap<&str, Acc>`
/// which hashes + probes a string for *every* step. Emits the representative key views into
/// `out_keys` (`AlignStr`s borrowing the source) and the per-group accumulators into `out_vals`;
/// returns the group count, or -1 if it exceeds `cap`. Monomorphized per op so the closures inline.
///
/// # Safety
/// `base` must point to `n` valid `stride`-byte rows, each holding a valid `AlignStr` at `key_off`;
/// `value_at` must read only within those rows. `out_keys`/`out_vals` must be valid for `cap`
/// `AlignStr` / `i64` writes.
/// A byte-slice map key **pre-hashed with the one canonical `wyhash`** — the string-interning maps in
/// `group_by`/`dict_encode` key on this, so their hashing converges on the same `wyhash` as the
/// `hash64` builtin and the JSON PHF (no separate FxHash). `Hash` just replays the precomputed hash
/// into the (identity) hasher, so each key is `wyhash`'d exactly once (at construction), not restreamed
/// on every probe; `Eq` still compares the *bytes*, so equal keys collide correctly.
#[derive(Clone, Copy)]
struct WyKey<'a> {
    bytes: &'a [u8],
    hash: u64,
}
impl<'a> WyKey<'a> {
    #[inline]
    fn new(bytes: &'a [u8]) -> Self {
        WyKey { bytes, hash: wyhash(bytes, WY_SEED) }
    }
}
impl PartialEq for WyKey<'_> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}
impl Eq for WyKey<'_> {}
impl std::hash::Hash for WyKey<'_> {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        h.write_u64(self.hash);
    }
}

/// Pass-through hasher for the already-`wyhash`'d [`WyKey`]: the key's `Hash` feeds one `write_u64`
/// (the precomputed hash), which is returned as-is. wyhash already avalanches, so the map needs no
/// further mixing. `write` is never reached (a `WyKey` only ever calls `write_u64`).
#[derive(Default)]
struct IdentityHasher(u64);
impl std::hash::Hasher for IdentityHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = i;
    }
    #[inline]
    fn write(&mut self, _: &[u8]) {
        // Unreachable: `WyKey::hash` only ever calls `write_u64`. Kept a no-op (not `unreachable!`)
        // so a future non-`WyKey` key can't turn a wrong-hasher mistake into a runtime panic in the
        // linked user binary — such a bug would surface as a test failure, never a crash in the field.
        debug_assert!(false, "IdentityHasher only accepts pre-hashed WyKey (write_u64)");
    }
}

type WyBuildHasher = std::hash::BuildHasherDefault<IdentityHasher>;

// core.hash — Align's one canonical non-cryptographic hash lives in the shared `align_hash` crate
// (wyhash final v3). The `hash64`/`hash128` builtins, the `group_by`/`dict_encode` interning
// (`WyKey`, above), and the JSON perfect-hash probe (`json_phf_hash`, below) all route through the
// same `align_hash::wyhash` — so codegen and runtime cannot compute a different hash for the same
// bytes. NOT cryptographic (not DoS-resistant, not a stable on-disk/wire format).
use align_hash::{WY_SECRET, WY_SEED, wyhash};

/// `core.hash.hash64(data)` — 64-bit non-crypto hash of a byte view (`str` / `slice<u8>`).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call (`len == 0` ⇒ `ptr` unused).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_hash64(ptr: *const u8, len: i64) -> u64 {
    let bytes: &[u8] = unsafe { safe_slice(ptr, len) };
    wyhash(bytes, WY_SEED)
}

/// 128-bit non-crypto hash result — two `u64` lanes (Align has no `u128`; this maps to the
/// `(u64, u64)` tuple the `hash128` builtin returns). By-value `#[repr(C)]`, like [`AlignStr`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AlignHash128 {
    pub lo: u64,
    pub hi: u64,
}

/// `core.hash.hash128(data)` — 128-bit non-crypto hash of a byte view. `lo` is the same value as
/// [`align_rt_hash64`] (so `hash128(x).0 == hash64(x)`); `hi` is a decorrelated second wyhash pass
/// seeded from `lo`.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call (`len == 0` ⇒ `ptr` unused).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_hash128(ptr: *const u8, len: i64) -> AlignHash128 {
    let bytes: &[u8] = unsafe { safe_slice(ptr, len) };
    let lo = wyhash(bytes, WY_SEED);
    let hi = wyhash(bytes, lo ^ WY_SECRET[2]);
    AlignHash128 { lo, hi }
}

#[allow(dead_code)]
const OP_SUM: i64 = 0;
const OP_MIN: i64 = 1;
const OP_MAX: i64 = 2;
const OP_COUNT: i64 = 3;

#[inline(always)]
unsafe fn read_key_slice<'a>(row: *const u8, key_off: usize) -> (&'a [u8], AlignStr) {
    let ks = unsafe { (row.add(key_off) as *const AlignStr).read_unaligned() };
    let bytes = if ks.ptr.is_null() || ks.len <= 0 {
        &[]
    } else {
        unsafe { safe_slice(ks.ptr, ks.len) }
    };
    (bytes, ks)
}

/// Core of the str-key group-by: intern each row's `str` key to a dense id in **first-occurrence
/// order** (a `HashMap<&[u8], id>`), accumulate `value_at(i)` per id with `combine`, then emit the
/// distinct keys (first-occurrence view as representative) + their accumulators. Key and value are
/// **index closures** over `0..n`, so the same core serves both a strided AoS record
/// (`align_rt_group_*_str`, key+value in one row) and two separate contiguous soa columns
/// (`align_rt_group_*_str_cols`). Returns the group count, or -1 on a null/cap error. `out_keys` /
/// `out_vals` must hold at least `cap` elements. (Callers validate their own input pointers + `n`.)
unsafe fn group_agg_str<'a>(
    n: usize,
    out_keys: *mut AlignStr,
    out_vals: *mut i64,
    cap: i64,
    key_at: impl Fn(usize) -> (&'a [u8], AlignStr),
    value_at: impl Fn(usize) -> i64,
    combine: impl Fn(i64, i64) -> i64,
) -> i64 {
    use std::collections::HashMap;
    if cap < 0 || out_keys.is_null() || out_vals.is_null() {
        return -1;
    }
    // Reserve up front to avoid the early grow-and-rehash churn; the group count is unknown, so cap
    // at a sane starting size (n is the worst case = all-distinct, but don't over-reserve for huge n).
    let initial = n.min(cap as usize).min(1024);
    let mut ids: HashMap<WyKey, usize, WyBuildHasher> = HashMap::with_capacity_and_hasher(initial, WyBuildHasher::default());
    let mut acc: Vec<i64> = Vec::with_capacity(initial);
    let mut reprs: Vec<AlignStr> = Vec::with_capacity(initial);
    for i in 0..n {
        let (bytes, ks) = key_at(i);
        let v = value_at(i);
        let key = WyKey::new(bytes);
        match ids.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => {
                let id = *e.get();
                unsafe { *acc.get_unchecked_mut(id) = combine(*acc.get_unchecked(id), v) };
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                let id = acc.len();
                if id >= cap as usize {
                    return -1;
                }
                e.insert(id);
                acc.push(v);
                reprs.push(ks);
            }
        }
    }
    let count = acc.len();
    let out_keys = unsafe { std::slice::from_raw_parts_mut(out_keys, count) };
    let out_vals = unsafe { std::slice::from_raw_parts_mut(out_vals, count) };
    out_keys.copy_from_slice(&reprs);
    out_vals.copy_from_slice(&acc);
    count as i64
}

/// Read the i64 value at `base + i*stride + val_off` — the AoS value-column index closure.
#[inline(always)]
unsafe fn aos_value_at(base: *const u8, stride: usize, val_off: usize) -> impl Fn(usize) -> i64 {
    move |i| unsafe { (base.add(i * stride).add(val_off) as *const i64).read_unaligned() }
}

/// The AoS key-column index closure: the `AlignStr` at `base + i*stride + key_off` (the strided
/// record's key field). The sibling of [`soa_key_at`] for the strided-record layout.
#[inline(always)]
unsafe fn aos_key_at<'a>(base: *const u8, stride: usize, key_off: usize) -> impl Fn(usize) -> (&'a [u8], AlignStr) {
    move |i| unsafe { read_key_slice(base.add(i * stride), key_off) }
}

/// `group_by(.str_key).sum(.i64_value)` over an AoS `array<Struct>` (key + value in one strided row).
///
/// # Safety
/// `base` addresses `n` records of `stride` bytes; `key_off`/`val_off` are valid field offsets.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_sum_str(base: *const u8, n: i64, stride: i64, key_off: i64, val_off: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    if n <= 0 || base.is_null() {
        return 0;
    }
    let (Ok(n), Ok(stride), Ok(key_off), Ok(val_off)) = (safe_len(n), safe_len(stride), safe_len(key_off), safe_len(val_off)) else { return 0 };
    unsafe {
        group_agg_str(n, out_keys, out_vals, cap, aos_key_at(base, stride, key_off), aos_value_at(base, stride, val_off), |a, b| a.wrapping_add(b))
    }
}

/// `group_by(.str_key).min(.i64_value)` over an AoS array — per-group minimum.
///
/// # Safety
/// See [`align_rt_group_sum_str`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_min_str(base: *const u8, n: i64, stride: i64, key_off: i64, val_off: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    if n <= 0 || base.is_null() {
        return 0;
    }
    let (Ok(n), Ok(stride), Ok(key_off), Ok(val_off)) = (safe_len(n), safe_len(stride), safe_len(key_off), safe_len(val_off)) else { return 0 };
    unsafe {
        group_agg_str(n, out_keys, out_vals, cap, aos_key_at(base, stride, key_off), aos_value_at(base, stride, val_off), |a, b| a.min(b))
    }
}

/// `group_by(.str_key).max(.i64_value)` over an AoS array — per-group maximum.
///
/// # Safety
/// See [`align_rt_group_sum_str`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_max_str(base: *const u8, n: i64, stride: i64, key_off: i64, val_off: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    if n <= 0 || base.is_null() {
        return 0;
    }
    let (Ok(n), Ok(stride), Ok(key_off), Ok(val_off)) = (safe_len(n), safe_len(stride), safe_len(key_off), safe_len(val_off)) else { return 0 };
    unsafe {
        group_agg_str(n, out_keys, out_vals, cap, aos_key_at(base, stride, key_off), aos_value_at(base, stride, val_off), |a, b| a.max(b))
    }
}

/// `group_by(.str_key).count()` over an AoS array — per-group row count (no value column).
///
/// # Safety
/// See [`align_rt_group_sum_str`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_count_str(base: *const u8, n: i64, stride: i64, key_off: i64, _val_off: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    if n <= 0 || base.is_null() {
        return 0;
    }
    let (Ok(n), Ok(stride), Ok(key_off)) = (safe_len(n), safe_len(stride), safe_len(key_off)) else { return 0 };
    unsafe { group_agg_str(n, out_keys, out_vals, cap, aos_key_at(base, stride, key_off), |_| 1, |a, b| a.wrapping_add(b)) }
}

// ---- soa two-contiguous-column variants: `key_col` is an `AlignStr` column, `val_col` an i64 column
// (separate buffers, not a strided record). `n` is the row count. Count ignores `val_col`. ----

/// The soa key-column index closure: the i-th `AlignStr` at `key_col + i*16` (`read_key_slice` with
/// offset 0 reads the `AlignStr` at the given address).
#[inline(always)]
unsafe fn soa_key_at<'a>(key_col: *const AlignStr) -> impl Fn(usize) -> (&'a [u8], AlignStr) {
    move |i| unsafe { read_key_slice(key_col.add(i) as *const u8, 0) }
}

/// The soa value-column index closure: the i-th i64 at `val_col + i*8`.
#[inline(always)]
unsafe fn soa_value_at(val_col: *const i64) -> impl Fn(usize) -> i64 {
    move |i| unsafe { val_col.add(i).read_unaligned() }
}

/// `group_by(.str_key).sum(.i64_value)` over a `soa<Struct>` — key and value are SEPARATE contiguous
/// columns (`key_col`: `AlignStr` elements, `val_col`: `i64`), the columnar counterpart of
/// [`align_rt_group_sum_str`].
///
/// # Safety
/// `key_col` addresses `n` `AlignStr`s, `val_col` `n` `i64`s; `out_keys`/`out_vals` hold ≥`cap`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_sum_str_cols(key_col: *const AlignStr, val_col: *const i64, n: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    if n <= 0 || key_col.is_null() || val_col.is_null() {
        return 0;
    }
    let Ok(n) = safe_len(n) else { return 0 };
    unsafe { group_agg_str(n, out_keys, out_vals, cap, soa_key_at(key_col), soa_value_at(val_col), |a, b| a.wrapping_add(b)) }
}

/// `group_by(.str_key).min(.i64_value)` over a soa — per-group minimum (two-column form).
///
/// # Safety
/// See [`align_rt_group_sum_str_cols`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_min_str_cols(key_col: *const AlignStr, val_col: *const i64, n: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    if n <= 0 || key_col.is_null() || val_col.is_null() {
        return 0;
    }
    let Ok(n) = safe_len(n) else { return 0 };
    unsafe { group_agg_str(n, out_keys, out_vals, cap, soa_key_at(key_col), soa_value_at(val_col), |a, b| a.min(b)) }
}

/// `group_by(.str_key).max(.i64_value)` over a soa — per-group maximum (two-column form).
///
/// # Safety
/// See [`align_rt_group_sum_str_cols`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_max_str_cols(key_col: *const AlignStr, val_col: *const i64, n: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    if n <= 0 || key_col.is_null() || val_col.is_null() {
        return 0;
    }
    let Ok(n) = safe_len(n) else { return 0 };
    unsafe { group_agg_str(n, out_keys, out_vals, cap, soa_key_at(key_col), soa_value_at(val_col), |a, b| a.max(b)) }
}

/// `group_by(.str_key).count()` over a soa — per-group row count (two-column form; `val_col` unused).
///
/// # Safety
/// See [`align_rt_group_sum_str_cols`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_count_str_cols(key_col: *const AlignStr, _val_col: *const i64, n: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    if n <= 0 || key_col.is_null() {
        return 0;
    }
    let Ok(n) = safe_len(n) else { return 0 };
    unsafe { group_agg_str(n, out_keys, out_vals, cap, soa_key_at(key_col), |_| 1, |a, b| a.wrapping_add(b)) }
}

/// One aggregate of a fused multi-aggregate str group-by ([`align_rt_group_multi_str`]). `op` is
/// `0`=sum, `1`=min, `2`=max, `3`=count; `val_off` is the i64 value field's byte offset in the row
/// (ignored for `count`); `out_vals` is this aggregate's i64 output column.
#[repr(C)]
pub struct GroupMultiSpec {
    pub val_off: i64,
    pub op: i64,
    pub out_vals: *mut i64,
}

/// `group_by(.str_key).agg(sum(.a), max(.b), count(), …)` over an AoS `array<Struct>` — the fused
/// multi-aggregate str rail. **One pass**: intern each row's `str` key once (a `HashMap<&[u8], id>`),
/// then fold every aggregate in `specs` into its own dense per-group column — the idiomatic-fast-Rust
/// `HashMap<&str,[i64;K]>` shape, vs running one whole str group-by per aggregate. Emits the
/// representative key views into `out_keys` (borrowing the source) and each aggregate `j`'s per-group
/// result into `specs[j].out_vals`; returns the group count, or -1 if it exceeds `cap`. Empty / null
/// input aggregates nothing (returns 0).
///
/// # Safety
/// `base` points to `n` `stride`-byte rows, each with a valid `AlignStr` at `key_off` and a valid
/// `i64` at every `specs[j].val_off` (for non-`count` ops). `out_keys` must be valid for `cap`
/// `AlignStr` writes; each `specs[j].out_vals` for `cap` `i64` writes. `specs`/`k` describe a valid
/// `GroupMultiSpec` range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_multi_str(
    base: *const u8,
    n: i64,
    stride: i64,
    key_off: i64,
    specs: *const GroupMultiSpec,
    k: i64,
    out_keys: *mut AlignStr,
    cap: i64,
) -> i64 {
    use std::collections::HashMap;
    if n <= 0 || base.is_null() {
        return 0;
    }
    if cap < 0 || k < 0 || out_keys.is_null() || (k > 0 && specs.is_null()) {
        return -1;
    }
    let Ok(n) = safe_len(n) else { return 0 };
    let Ok(k) = safe_len(k) else { return -1 };
    let (Ok(stride), Ok(key_off)) = (safe_len(stride), safe_len(key_off)) else { return -1 };
    let specs: &[GroupMultiSpec] = if k == 0 { &[] } else { unsafe { std::slice::from_raw_parts(specs, k) } };
    // Per aggregate: the value reader (a row pointer → i64; `count` reads `1`) and the combine op. The
    // combine is selected once per aggregate (not per row) so the inner fold is a small fixed match.
    let ops: Vec<i64> = specs.iter().map(|s| s.op).collect();
    let val_offs: Vec<usize> = specs.iter().map(|s| usize::try_from(s.val_off).unwrap_or(0)).collect();

    let initial = n.min(usize::try_from(cap).unwrap_or(0)).min(1024);
    let mut ids: HashMap<WyKey, usize, WyBuildHasher> = HashMap::with_capacity_and_hasher(initial, WyBuildHasher::default());
    // Accumulators, row-major per group: `acc[id*k + j]`. One contiguous buffer keeps a group's K
    // accumulators adjacent (cache-friendly on the update).
    let mut acc: Vec<i64> = Vec::with_capacity(initial.saturating_mul(k));
    let mut reprs: Vec<AlignStr> = Vec::with_capacity(initial);
    // Read aggregate `j`'s value for the row at `row` (`count` contributes 1). `j < k` and
    // `ops`/`val_offs` have length `k`, so the unchecked index is in bounds — eliminating a bounds
    // check on every K-way per-row fold.
    let read = |row: *const u8, j: usize| -> i64 {
        if unsafe { *ops.get_unchecked(j) } == OP_COUNT {
            1
        } else {
            unsafe { (row.add(*val_offs.get_unchecked(j)) as *const i64).read_unaligned() }
        }
    };
    // Fold value `v` into accumulator `cur` per aggregate `j`'s op (`j < k`, see `read`).
    let combine = |cur: i64, v: i64, j: usize| -> i64 {
        match unsafe { *ops.get_unchecked(j) } {
            OP_MIN => cur.min(v),
            OP_MAX => cur.max(v),
            // sum (OP_SUM) and count (OP_COUNT) both add (count's `v` is 1).
            _ => cur.wrapping_add(v),
        }
    };
    for i in 0..n {
        let row = unsafe { base.add(i * stride) };
        let (bytes, ks) = unsafe { read_key_slice(row, key_off) };
        let key = WyKey::new(bytes);
        match ids.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => {
                let id = *e.get();
                // `id < reprs.len()` and `acc.len() == reprs.len() * k`, so `g + j < acc.len()`.
                let g = id * k;
                for j in 0..k {
                    unsafe {
                        let slot = acc.get_unchecked_mut(g + j);
                        *slot = combine(*slot, read(row, j), j);
                    }
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                let id = reprs.len();
                // Bail early if the group count would exceed the caller's output capacity, before
                // growing the tables further (cap = the row count in generated code, so unreachable
                // there, but keeps the function safe for any caller).
                if id >= cap as usize {
                    return -1;
                }
                e.insert(id);
                reprs.push(ks);
                // Seed each accumulator with the group's first row value.
                for j in 0..k {
                    acc.push(read(row, j));
                }
            }
        }
    }
    let count = reprs.len();
    let out_keys = unsafe { std::slice::from_raw_parts_mut(out_keys, count) };
    out_keys.copy_from_slice(&reprs);
    // Scatter each aggregate's accumulators into its output column.
    for (j, spec) in specs.iter().enumerate() {
        // A null output column would make `from_raw_parts_mut` UB even at len 0 — fail closed.
        if spec.out_vals.is_null() {
            return -1;
        }
        let out = unsafe { std::slice::from_raw_parts_mut(spec.out_vals, count) };
        for (g, slot) in out.iter_mut().enumerate() {
            // `g < count` and `g * k + j < count * k = acc.len()`.
            *slot = unsafe { *acc.get_unchecked(g * k + j) };
        }
    }
    count as i64
}

/// **Dictionary-encode** a strided `str` column over an AoS `array<Struct>` — the A2 reuse rail.
/// Assigns each distinct key a **dense id** in first-occurrence order: `out_ids[i]` = the id of row
/// `i`, and `out_dict[id]` = that id's representative `str` view (borrowing the source). Returns the
/// dictionary size (distinct count), or -1 if it exceeds `cap` (the `out_dict` capacity). The id
/// column can then be aggregated by the dense-id `align_rt_group_*_i64` **repeatedly**, with results
/// labeled back to strings via `out_dict` — so the string interning is paid **once** and reused
/// across many group-bys (vs re-interning per group-by, the A1 cost). The empty / null input encodes
/// nothing (returns 0).
///
/// # Safety
/// `base` points to `n` `stride`-byte rows, each with a valid `AlignStr` at `key_off`. `out_ids` must
/// be valid for `n` `i64` writes; `out_dict` for `cap` `AlignStr` writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_dict_encode_str(base: *const u8, n: i64, stride: i64, key_off: i64, out_ids: *mut i64, out_dict: *mut AlignStr, cap: i64) -> i64 {
    use std::collections::HashMap;
    if n <= 0 || base.is_null() {
        return 0;
    }
    // Fail fast on invalid output up front — before any O(n) work or partial mutation of `out_ids`.
    if out_ids.is_null() || out_dict.is_null() || cap < 0 {
        return -1;
    }
    let Ok(n) = safe_len(n) else { return 0 };
    let (Ok(stride), Ok(key_off)) = (safe_len(stride), safe_len(key_off)) else { return -1 };
    let out_ids = unsafe { std::slice::from_raw_parts_mut(out_ids, n) };
    let initial = n.min(usize::try_from(cap).unwrap_or(0)).min(1024);
    let mut ids: HashMap<WyKey, i64, WyBuildHasher> = HashMap::with_capacity_and_hasher(initial, WyBuildHasher::default());
    let mut reprs: Vec<AlignStr> = Vec::with_capacity(initial);
    for (i, out_id) in out_ids.iter_mut().enumerate() {
        let row = unsafe { base.add(i * stride) };
        let (bytes, ks) = unsafe { read_key_slice(row, key_off) };
        let key = WyKey::new(bytes);
        let id = match ids.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => *e.get(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let id = reprs.len() as i64;
                // The dictionary would exceed `out_dict`'s capacity — abort early (don't grow the
                // table for a result we can't return).
                if id >= cap {
                    return -1;
                }
                e.insert(id);
                reprs.push(ks);
                id
            }
        };
        *out_id = id;
    }
    let count = reprs.len(); // <= cap (the loop aborts past it) and out_dict is non-null (checked up front)
    let out_dict = unsafe { std::slice::from_raw_parts_mut(out_dict, count) };
    out_dict.copy_from_slice(&reprs);
    count as i64
}

/// Label a column of dictionary ids back to `str` views — the A2 result step. `ids[i]` (a dense id
/// from [`align_rt_dict_encode_str`]) is mapped to `out[i] = dict[ids[i]]`. After a dense-id
/// `group_by` on the encoded id column produces `(distinct_ids, aggregates)`, this turns the distinct
/// ids back into the `(array<str>, …)` result keys. An id out of `dict_len` range yields an empty
/// `str` (defensive; shouldn't happen for ids produced by `dict_encode`).
///
/// # Safety
/// `ids` valid for `n` `i64`s; `dict` for `dict_len` `AlignStr`s; `out` for `n` `AlignStr` writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_dict_lookup(ids: *const i64, n: i64, dict: *const AlignStr, dict_len: i64, out: *mut AlignStr) {
    if n <= 0 || ids.is_null() || out.is_null() {
        return;
    }
    // `usize::try_from` (not `as usize`) so an out-of-range id can't truncate into a false in-bounds
    // hit (Align is 64-bit, but a public C-ABI entry shouldn't depend on that).
    let Ok(n) = safe_len(n) else { return };
    let ids = unsafe { std::slice::from_raw_parts(ids, n) };
    let out = unsafe { std::slice::from_raw_parts_mut(out, n) };
    let Ok(dict_len) = safe_len(dict_len) else { return };
    let dict: &[AlignStr] = unsafe { safe_slice(dict, dict_len as i64) };
    let empty = AlignStr { ptr: core::ptr::NonNull::dangling().as_ptr(), len: 0 };
    for (slot, &id) in out.iter_mut().zip(ids.iter()) {
        *slot = usize::try_from(id).ok().and_then(|idx| dict.get(idx).copied()).unwrap_or(empty);
    }
}

/// Gather a strided `i64` column out of an AoS struct array into a contiguous buffer — the value
/// projection for the A2 reuse rail. `out[i]` = the `i64` at byte `off` of row `i` (`base + i*stride`).
/// After `dict_encode` yields a dense-id column, a `group_by(.key).<agg>(.value)` gathers the chosen
/// `.value` column here, then runs the contiguous-input `align_rt_group_*_i64` over `(ids, out)`. The
/// empty / null input writes nothing.
///
/// # Safety
/// `base` points to `n` `stride`-byte rows, each with a valid `i64` at `off`; `out` is valid for `n`
/// `i64` writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_gather_i64(base: *const u8, n: i64, stride: i64, off: i64, out: *mut i64) {
    // A negative `stride`/`off` is meaningless and would wrap to a huge value through `as usize`
    // (an out-of-bounds read); reject it defensively, like `align_rt_dict_lookup`'s `try_from` guards.
    let Ok(n_u) = safe_len(n) else { return };
    let Ok(stride_u) = safe_len(stride) else { return };
    let Ok(off_u) = safe_len(off) else { return };
    if n_u == 0 || base.is_null() || out.is_null() || n_u > isize::MAX as usize / 8 {
        return;
    }
    let out = unsafe { std::slice::from_raw_parts_mut(out, n_u) };
    for (i, slot) in out.iter_mut().enumerate() {
        let row = unsafe { base.add(i.wrapping_mul(stride_u)) };
        *slot = unsafe { (row.add(off_u) as *const i64).read_unaligned() };
    }
}

/// Find the first `"` or `\` in `hay` (the two bytes that bound or interrupt a JSON string body),
/// returning its index, or `None` if neither occurs.
///
/// A scalar prefix scan handles the common short string (field names, small values) with no SIMD
/// setup cost; only when the body runs past the prefix does it escalate to a runtime-dispatched
/// `memchr2` (AVX2/NEON via the `memchr` crate). On long string bodies the SIMD scan is several×
/// to ~30× faster than the byte-at-a-time loop (`work/json_str_simd_probe.rs`), while the prefix
/// keeps short keys from regressing.
fn find_quote_or_escape(hay: &[u8]) -> Option<usize> {
    const PREFIX: usize = 16;
    let head = hay.len().min(PREFIX);
    for (i, &c) in hay[..head].iter().enumerate() {
        if c == b'"' || c == b'\\' {
            return Some(i);
        }
    }
    if hay.len() <= PREFIX {
        return None;
    }
    memchr::memchr2(b'"', b'\\', &hay[PREFIX..]).map(|i| i + PREFIX)
}

// ===================== JSON SIMD index (simdjson-style stage 1) =====================
//
// Stage 1 of the two-stage JSON decoder: scan the input and emit the byte positions of the
// structural punctuation that lies OUTSIDE string literals. String interiors are masked (a `:`
// inside `"a:b"` is not structure) via a carry-less-multiply prefix-XOR of the quote bitmap, and
// `\"` / `\\` escapes are handled (an escaped quote is not a delimiter). Stage 2 (the decoder) walks
// this index instead of stepping byte-by-byte.
//
// The live indexer is the **lean** `json_decode_index` below — it emits only `{ } [ ] :` (no quotes,
// no commas), ~⅓ the tokens for object-heavy input, and keys / string values are recovered by a short
// raw-byte scan-back. The fast paths are AVX2 (x86_64, carry-less-multiply prefix-XOR) and NEON
// (aarch64, baseline shift-XOR), each runtime-safe with a scalar reference / fallback that is also the
// oracle the SIMD is differentially tested against. (The earlier quote+comma `json_structural_index`
// was removed once the lean index superseded it — it never had a live consumer.)
//
// `prefix_xor` (AVX2) / `prefix_xor_portable` (NEON) and `find_escaped` are the shared bit-twiddling
// helpers used by both SIMD index paths.

/// prefix-XOR of a 64-bit mask: bit i = XOR of bits 0..=i. Via carry-less multiply by all-ones — the
/// classic simdjson trick for turning a quote bitmap into an "inside string" mask.
#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "pclmulqdq,sse2")]
unsafe fn prefix_xor(bitmask: u64) -> u64 {
    use core::arch::x86_64::*;
    let m = _mm_set_epi64x(0, bitmask as i64);
    let ones = _mm_set1_epi8(-1);
    _mm_cvtsi128_si64(_mm_clmulepi64_si128(m, ones, 0)) as u64
}

/// The escaped-character mask for a 64-bit `backslash` bitmap (simdjson's odd/even backslash-run
/// analysis). A character is "escaped" iff it is preceded by an odd-length run of backslashes.
/// `prev_escaped` carries a pending escape across the 64-byte block boundary (0 or 1).
/// Pure bitwise — shared by the AVX2 and NEON indexers (no architecture intrinsics).
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[inline]
fn find_escaped(backslash: u64, prev_escaped: &mut u64) -> u64 {
    let pe = *prev_escaped;
    // A backslash already consumed as the *escaped* char of a prior run can't start a new one.
    let backslash = backslash & !pe;
    let follows_escape = (backslash << 1) | pe;
    const EVEN: u64 = 0x5555_5555_5555_5555;
    let odd_starts = backslash & !EVEN & !follows_escape;
    let (seq_even, carry) = odd_starts.overflowing_add(backslash);
    *prev_escaped = carry as u64;
    let invert = seq_even << 1;
    (EVEN ^ invert) & follows_escape
}

/// A **lean** decode index for the two-stage `array<Struct>` decoder: emits only `{ } [ ] :`
/// (structure + field separators) outside strings — **not** quotes or commas. The decoder navigates
/// records by the braces and fields by the colons, and recovers keys / `str` values by a short scan
/// of the raw bytes around the colon (so the quotes need not be in the index). Emitting only colons
/// (~⅓ the tokens of a quote+comma index) is the win — the index size dominates the decode
/// (`docs/open-questions.md` "JSON two-stage SIMD decode" autopsy), so the smaller index is faster.
#[cfg_attr(not(test), allow(dead_code))]
fn json_decode_index_scalar(src: &[u8], out: &mut Vec<u32>) {
    out.clear();
    out.reserve(src.len() / 12);
    let mut in_string = false;
    let mut esc = false;
    for (i, &b) in src.iter().enumerate() {
        if b == b'"' && !esc {
            in_string = !in_string;
        } else if !in_string && matches!(b, b'{' | b'}' | b'[' | b']' | b':') {
            out.push(i as u32);
        }
        esc = b == b'\\' && !esc;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,pclmulqdq")]
unsafe fn json_decode_index_avx2(src: &[u8], out: &mut Vec<u32>) {
    use core::arch::x86_64::*;
    out.clear();
    out.reserve(src.len() / 12);
    let n = src.len();
    let eqm = |v: __m256i, c: u8| -> u32 { _mm256_movemask_epi8(_mm256_cmpeq_epi8(v, _mm256_set1_epi8(c as i8))) as u32 };
    // Structure + field separators only — no `,`, and quotes are masking-only (never emitted).
    let op_bits = |v: __m256i| -> u32 { eqm(v, b'{') | eqm(v, b'}') | eqm(v, b'[') | eqm(v, b']') | eqm(v, b':') };

    let mut string_carry: u64 = 0;
    let mut escape_carry: u64 = 0;
    let mut i = 0usize;
    while i + 64 <= n {
        let p = unsafe { src.as_ptr().add(i) };
        let lo = unsafe { _mm256_loadu_si256(p as *const __m256i) };
        let hi = unsafe { _mm256_loadu_si256(p.add(32) as *const __m256i) };
        let quote = (eqm(lo, b'"') as u64) | ((eqm(hi, b'"') as u64) << 32);
        let backslash = (eqm(lo, b'\\') as u64) | ((eqm(hi, b'\\') as u64) << 32);
        let op = (op_bits(lo) as u64) | ((op_bits(hi) as u64) << 32);

        let escaped = find_escaped(backslash, &mut escape_carry);
        let real_quote = quote & !escaped;
        let in_string = unsafe { prefix_xor(real_quote) } ^ string_carry;
        string_carry = (in_string as i64 >> 63) as u64;

        let mut bits = op & !in_string;
        while bits != 0 {
            out.push(i as u32 + bits.trailing_zeros());
            bits &= bits - 1;
        }
        i += 64;
    }
    let mut in_string = string_carry & 1 == 1;
    let mut esc = escape_carry & 1 == 1;
    while i < n {
        let b = unsafe { *src.get_unchecked(i) };
        if b == b'"' && !esc {
            in_string = !in_string;
        } else if !in_string && matches!(b, b'{' | b'}' | b'[' | b']' | b':') {
            out.push(i as u32);
        }
        esc = b == b'\\' && !esc;
        i += 1;
    }
}

/// Inclusive prefix-XOR of a 64-bit mask (bit i = parity of bits 0..=i), via the Kogge-Stone
/// shift-XOR ladder — six dependent `u64` ops, pure baseline (no PMULL). The AVX2 path uses
/// `pclmulqdq` for the same result; the NEON path uses this because PMULL (`vmull_p64`) is the
/// optional `aes` crypto feature, not ARMv8-A baseline, and the prefix-XOR is not the hot cost
/// (the per-byte movemask dominates) — so a branch-free baseline ladder is the cleaner choice.
#[cfg(target_arch = "aarch64")]
#[inline]
fn prefix_xor_portable(mut x: u64) -> u64 {
    x ^= x << 1;
    x ^= x << 2;
    x ^= x << 4;
    x ^= x << 8;
    x ^= x << 16;
    x ^= x << 32;
    x
}

/// NEON decode index — the aarch64 counterpart to [`json_decode_index_avx2`], emitting the same
/// lean `{ } [ ] :` positions. NEON is ARMv8-A baseline, so this needs no runtime feature detection.
/// It processes 64 bytes per block as four 16-byte vectors, builds a 16-bit movemask per vector
/// (bit-weight AND + across-lane `vaddv`), combines them into the same 64-bit masks the AVX2 path
/// uses, then shares [`find_escaped`] and uses [`prefix_xor_portable`] in place of `pclmulqdq`.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn json_decode_index_neon(src: &[u8], out: &mut Vec<u32>) {
    use core::arch::aarch64::*;
    out.clear();
    out.reserve(src.len() / 12);
    let n = src.len();

    // Per-lane bit weights 1,2,4,…,128 (repeated over the two 8-lane halves): AND a 0x00/0xFF compare
    // mask with these, then `vaddv` each half → one byte whose bit i is set iff lane i matched. A
    // local `static` guarantees a single `.rodata` instance (a `const` could be re-materialized on
    // the stack per call; `static` + `as_ptr()` cannot).
    static WEIGHTS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
    let bit_weights: uint8x16_t = unsafe { vld1q_u8(WEIGHTS.as_ptr()) };
    // Closures inherit the fn's `neon` feature, so the pure (memory-free) intrinsics are callable
    // without `unsafe` (only the pointer loads below are `unsafe`).
    let movemask16 = |cmp: uint8x16_t| -> u32 {
        let masked = vandq_u8(cmp, bit_weights);
        (vaddv_u8(vget_low_u8(masked)) as u32) | ((vaddv_u8(vget_high_u8(masked)) as u32) << 8)
    };
    let eqm = |v: uint8x16_t, c: u8| -> u32 { movemask16(vceqq_u8(v, vdupq_n_u8(c))) };
    // Structure + field separators only — no `,`, and quotes are masking-only (never emitted). OR the
    // five 0x00/0xFF compare masks lane-wise *before* the movemask → one `vaddv` pair per vector
    // instead of five (the across-lane add is the cost); identical result, since a lane is set iff it
    // matched any of the five.
    let op_bits = |v: uint8x16_t| -> u32 {
        let m = vorrq_u8(
            vorrq_u8(vorrq_u8(vceqq_u8(v, vdupq_n_u8(b'{')), vceqq_u8(v, vdupq_n_u8(b'}'))), vorrq_u8(vceqq_u8(v, vdupq_n_u8(b'[')), vceqq_u8(v, vdupq_n_u8(b']')))),
            vceqq_u8(v, vdupq_n_u8(b':')),
        );
        movemask16(m)
    };

    let mut string_carry: u64 = 0;
    let mut escape_carry: u64 = 0;
    let mut i = 0usize;
    while i + 64 <= n {
        let p = unsafe { src.as_ptr().add(i) };
        let v0 = unsafe { vld1q_u8(p) };
        let v1 = unsafe { vld1q_u8(p.add(16)) };
        let v2 = unsafe { vld1q_u8(p.add(32)) };
        let v3 = unsafe { vld1q_u8(p.add(48)) };
        // Combine four 16-bit movemasks into one 64-bit mask, mirroring the AVX2 lo/hi packing.
        let quote = (eqm(v0, b'"') as u64) | ((eqm(v1, b'"') as u64) << 16) | ((eqm(v2, b'"') as u64) << 32) | ((eqm(v3, b'"') as u64) << 48);
        let backslash = (eqm(v0, b'\\') as u64) | ((eqm(v1, b'\\') as u64) << 16) | ((eqm(v2, b'\\') as u64) << 32) | ((eqm(v3, b'\\') as u64) << 48);
        let op = (op_bits(v0) as u64) | ((op_bits(v1) as u64) << 16) | ((op_bits(v2) as u64) << 32) | ((op_bits(v3) as u64) << 48);

        let escaped = find_escaped(backslash, &mut escape_carry);
        let real_quote = quote & !escaped;
        let in_string = prefix_xor_portable(real_quote) ^ string_carry;
        string_carry = (in_string as i64 >> 63) as u64;

        let mut bits = op & !in_string;
        while bits != 0 {
            out.push(i as u32 + bits.trailing_zeros());
            bits &= bits - 1;
        }
        i += 64;
    }
    // Scalar tail, continuing the carried state — identical semantics to `json_decode_index_scalar`.
    let mut in_string = string_carry & 1 == 1;
    let mut esc = escape_carry & 1 == 1;
    while i < n {
        let b = unsafe { *src.get_unchecked(i) };
        if b == b'"' && !esc {
            in_string = !in_string;
        } else if !in_string && matches!(b, b'{' | b'}' | b'[' | b']' | b':') {
            out.push(i as u32);
        }
        esc = b == b'\\' && !esc;
        i += 1;
    }
}

/// Fill `out` with the lean decode-index positions (`{ } [ ] :`). Runtime-dispatched: AVX2 +
/// `pclmulqdq` on x86_64 when present, the always-baseline NEON path on aarch64, else the scalar
/// reference.
fn json_decode_index(src: &[u8], out: &mut Vec<u32>) {
    if src.len() > u32::MAX as usize {
        panic_abort("JSON input exceeds the 4 GiB decode-index limit");
    }
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("pclmulqdq") {
            unsafe { json_decode_index_avx2(src, out) };
            return;
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // NEON is ARMv8-A baseline — unconditionally available on every aarch64 target.
        unsafe { json_decode_index_neon(src, out) };
    }
    #[cfg(not(target_arch = "aarch64"))]
    json_decode_index_scalar(src, out);
}

// ── UTF-8 validation (draft §7/§12: a `str`/`string` is always valid UTF-8) ──────────────────────
// Every I/O surface that hands out a `str` — `fs.read_file`, `fs.read_file_view` (mmap + fallback),
// `json.decode` (a decoded `str` field is a zero-copy view into the input, so validating the whole
// input once covers every field, exactly as simdjson does) — runs its bytes through here first;
// invalid → the operation fails rather than producing a `str` that breaks the invariant. Binary reads
// take the `reader.read(buffer)` path instead (`bytes`/`buffer` carry no UTF-8 invariant, draft §18.2).
//
// Algorithm: Lemire's range/lookup table method (simdjson `utf8_lookup4`), with AVX2 (x86_64) / NEON
// (aarch64) / scalar paths. The scalar path is `std::str::from_utf8` — the correctness reference and
// the oracle the SIMD paths are differentially tested against (same discipline as the decode index).

/// Scalar reference validator (and the SIMD oracle): `bytes` is well-formed UTF-8. On aarch64 the
/// NEON path is baseline (always taken), so this is only the dispatch fallback / test oracle there.
#[inline]
#[cfg_attr(not(test), allow(dead_code))]
fn validate_utf8_scalar(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

// simdjson `utf8_lookup4` error-class bits (a lead+continuation pattern that sets any of these under
// the three-table AND is malformed). `TOO_LARGE_1000` and `OVERLONG_4` deliberately share bit 6 —
// they never co-occur in one lane, per the algorithm.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
mod utf8_tbl {
    pub const TOO_SHORT: u8 = 1 << 0; // 11______ not followed by 10______
    pub const TOO_LONG: u8 = 1 << 1; // 0_______ or 10______ following 10______
    pub const OVERLONG_3: u8 = 1 << 2; // 11100000 100_____
    pub const TOO_LARGE: u8 = 1 << 3; // > U+10FFFF
    pub const SURROGATE: u8 = 1 << 4; // U+D800..U+DFFF
    pub const OVERLONG_2: u8 = 1 << 5; // 1100000_ 10______
    pub const TOO_LARGE_1000: u8 = 1 << 6;
    pub const OVERLONG_4: u8 = 1 << 6; // 11110000 1000____
    pub const TWO_CONTS: u8 = 1 << 7; // 10______ 10______
    pub const CARRY: u8 = TOO_SHORT | TOO_LONG | TWO_CONTS;

    /// Lookup by the high nibble of the lead byte (`prev1 >> 4`).
    pub const B1H: [u8; 16] = [
        TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG, // 0_ ASCII
        TWO_CONTS, TWO_CONTS, TWO_CONTS, TWO_CONTS, // 10 continuation
        TOO_SHORT | OVERLONG_2,                     // 1100 two-byte lead (C0/C1 overlong)
        TOO_SHORT,                                  // 1101 two-byte lead
        TOO_SHORT | OVERLONG_3 | SURROGATE,         // 1110 three-byte lead
        TOO_SHORT | TOO_LARGE | TOO_LARGE_1000 | OVERLONG_4, // 1111 four-byte lead
    ];
    /// Lookup by the low nibble of the lead byte (`prev1 & 0x0F`).
    pub const B1L: [u8; 16] = [
        CARRY | OVERLONG_2 | OVERLONG_3 | OVERLONG_4, // 0 (C0/E0/F0)
        CARRY | OVERLONG_2,                           // 1 (C1)
        CARRY,                                        // 2
        CARRY,                                        // 3
        CARRY | TOO_LARGE,                            // 4 (F4)
        CARRY | TOO_LARGE | TOO_LARGE_1000,           // 5
        CARRY | TOO_LARGE | TOO_LARGE_1000,           // 6
        CARRY | TOO_LARGE | TOO_LARGE_1000,           // 7
        CARRY | TOO_LARGE | TOO_LARGE_1000,           // 8
        CARRY | TOO_LARGE | TOO_LARGE_1000,           // 9
        CARRY | TOO_LARGE | TOO_LARGE_1000,           // a
        CARRY | TOO_LARGE | TOO_LARGE_1000,           // b
        CARRY | TOO_LARGE | TOO_LARGE_1000,           // c
        CARRY | TOO_LARGE | TOO_LARGE_1000 | SURROGATE, // d (ED lead → surrogate range)
        CARRY | TOO_LARGE | TOO_LARGE_1000,           // e
        CARRY | TOO_LARGE | TOO_LARGE_1000,           // f
    ];
    /// Lookup by the high nibble of the current (second) byte (`input >> 4`).
    pub const B2H: [u8; 16] = [
        TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT, // 0_ ASCII
        TOO_LONG | OVERLONG_2 | TWO_CONTS | OVERLONG_3 | TOO_LARGE_1000 | OVERLONG_4, // 1000
        TOO_LONG | OVERLONG_2 | TWO_CONTS | OVERLONG_3 | TOO_LARGE,                   // 1001
        TOO_LONG | OVERLONG_2 | TWO_CONTS | SURROGATE | TOO_LARGE,                    // 1010
        TOO_LONG | OVERLONG_2 | TWO_CONTS | SURROGATE | TOO_LARGE,                    // 1011
        TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT, // 11 not a continuation
    ];
}

/// AVX2 UTF-8 validator — 32-byte blocks, Lemire's `utf8_lookup4`. Carries the last block's tail
/// (`prev_input`) so sequences straddling a block boundary are checked, and `prev_incomplete` so a
/// lead byte in the final block with no room for its continuations is an error. A wholly-ASCII block
/// takes the fast path (only the carried incompleteness matters). The `< 32`-byte tail is validated
/// in a zero-padded block — the zero padding is ASCII, so an unfinished lead there is caught as
/// `TOO_SHORT`. Bytewise-equal to [`validate_utf8_scalar`] (differentially fuzzed).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn validate_utf8_avx2(input: &[u8]) -> bool {
    use core::arch::x86_64::*;
    let b1h = _mm256_broadcastsi128_si256(unsafe { _mm_loadu_si128(utf8_tbl::B1H.as_ptr() as *const __m128i) });
    let b1l = _mm256_broadcastsi128_si256(unsafe { _mm_loadu_si128(utf8_tbl::B1L.as_ptr() as *const __m128i) });
    let b2h = _mm256_broadcastsi128_si256(unsafe { _mm_loadu_si128(utf8_tbl::B2H.as_ptr() as *const __m128i) });
    let low_mask = _mm256_set1_epi8(0x0f);
    // `is_incomplete` cap: a lead byte in the last three lanes with no room for its continuations is
    // > the cap → nonzero after the saturating subtract. Lanes 0..28 cap at 0xFF (never flagged).
    let inc_max = {
        let mut m = [0xffu8; 32];
        m[29] = 0xf0 - 1; // a 4-byte lead needs 3 more bytes
        m[30] = 0xe0 - 1; // a 3-byte lead needs 2 more
        m[31] = 0xc0 - 1; // any lead needs at least 1 more
        unsafe { _mm256_loadu_si256(m.as_ptr() as *const __m256i) }
    };
    // High nibble of each byte (`v >> 4`): a 16-bit shift then mask keeps each byte's top nibble.
    let high_nib = |v: __m256i| _mm256_and_si256(_mm256_srli_epi16(v, 4), low_mask);
    // Per-block special-case + multibyte-length error bits (0 where valid).
    let block_err = |cur: __m256i, prev: __m256i| -> __m256i {
        let shifted = _mm256_permute2x128_si256(prev, cur, 0x21);
        let prev1 = _mm256_alignr_epi8(cur, shifted, 15);
        let prev2 = _mm256_alignr_epi8(cur, shifted, 14);
        let prev3 = _mm256_alignr_epi8(cur, shifted, 13);
        let sc = _mm256_and_si256(
            _mm256_and_si256(
                _mm256_shuffle_epi8(b1h, high_nib(prev1)),
                _mm256_shuffle_epi8(b1l, _mm256_and_si256(prev1, low_mask)),
            ),
            _mm256_shuffle_epi8(b2h, high_nib(cur)),
        );
        // This byte must be a 2nd/3rd continuation iff prev2 is a 3+-byte lead (>= 0xE0) or prev3 is a
        // 4-byte lead (>= 0xF0). Saturating-subtract so only those set bit 0x80; XOR against `sc`
        // (whose 0x80 = TWO_CONTS marks an actual continuation-after-continuation) → 0 iff consistent.
        let is_third = _mm256_subs_epu8(prev2, _mm256_set1_epi8(0x60)); // 0xE0 - 0x80
        let is_fourth = _mm256_subs_epu8(prev3, _mm256_set1_epi8(0x70)); // 0xF0 - 0x80
        let must23_80 = _mm256_and_si256(_mm256_or_si256(is_third, is_fourth), _mm256_set1_epi8(0x80u8 as i8));
        _mm256_xor_si256(must23_80, sc)
    };

    let n = input.len();
    let ptr = input.as_ptr();
    let mut err = _mm256_setzero_si256();
    let mut prev_input = _mm256_setzero_si256();
    let mut prev_incomplete = _mm256_setzero_si256();
    let mut i = 0usize;
    while i + 32 <= n {
        let cur = unsafe { _mm256_loadu_si256(ptr.add(i) as *const __m256i) };
        if _mm256_movemask_epi8(cur) == 0 {
            // All ASCII: only a lead spilling from the previous block can be an error here.
            err = _mm256_or_si256(err, prev_incomplete);
            prev_incomplete = _mm256_setzero_si256();
        } else {
            err = _mm256_or_si256(err, block_err(cur, prev_input));
            prev_incomplete = _mm256_subs_epu8(cur, inc_max);
        }
        prev_input = cur;
        i += 32;
    }
    if i < n {
        let mut buf = [0u8; 32];
        unsafe { core::ptr::copy_nonoverlapping(ptr.add(i), buf.as_mut_ptr(), n - i) };
        let cur = unsafe { _mm256_loadu_si256(buf.as_ptr() as *const __m256i) };
        err = _mm256_or_si256(err, block_err(cur, prev_input));
        prev_incomplete = _mm256_subs_epu8(cur, inc_max);
    }
    err = _mm256_or_si256(err, prev_incomplete);
    _mm256_testz_si256(err, err) == 1
}

/// NEON UTF-8 validator — the aarch64 counterpart to [`validate_utf8_avx2`], 16-byte blocks. NEON is
/// ARMv8-A baseline (no runtime detection). `vqtbl1q_u8` does the 16-entry table lookup directly (no
/// lane duplication). Same carry / tail / incompleteness logic; bytewise-equal to the scalar oracle.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn validate_utf8_neon(input: &[u8]) -> bool {
    use core::arch::aarch64::*;
    let b1h = unsafe { vld1q_u8(utf8_tbl::B1H.as_ptr()) };
    let b1l = unsafe { vld1q_u8(utf8_tbl::B1L.as_ptr()) };
    let b2h = unsafe { vld1q_u8(utf8_tbl::B2H.as_ptr()) };
    let low_mask = vdupq_n_u8(0x0f);
    let inc_max = {
        let mut m = [0xffu8; 16];
        m[13] = 0xf0 - 1;
        m[14] = 0xe0 - 1;
        m[15] = 0xc0 - 1;
        unsafe { vld1q_u8(m.as_ptr()) }
    };
    let block_err = |cur: uint8x16_t, prev: uint8x16_t| -> uint8x16_t {
        let prev1 = vextq_u8(prev, cur, 15);
        let prev2 = vextq_u8(prev, cur, 14);
        let prev3 = vextq_u8(prev, cur, 13);
        let sc = vandq_u8(
            vandq_u8(vqtbl1q_u8(b1h, vshrq_n_u8(prev1, 4)), vqtbl1q_u8(b1l, vandq_u8(prev1, low_mask))),
            vqtbl1q_u8(b2h, vshrq_n_u8(cur, 4)),
        );
        let is_third = vqsubq_u8(prev2, vdupq_n_u8(0x60));
        let is_fourth = vqsubq_u8(prev3, vdupq_n_u8(0x70));
        let must23_80 = vandq_u8(vorrq_u8(is_third, is_fourth), vdupq_n_u8(0x80));
        veorq_u8(must23_80, sc)
    };

    let n = input.len();
    let ptr = input.as_ptr();
    let mut err = vdupq_n_u8(0);
    let mut prev_input = vdupq_n_u8(0);
    let mut prev_incomplete = vdupq_n_u8(0);
    let mut i = 0usize;
    while i + 16 <= n {
        let cur = unsafe { vld1q_u8(ptr.add(i)) };
        if vmaxvq_u8(cur) < 0x80 {
            err = vorrq_u8(err, prev_incomplete);
            prev_incomplete = vdupq_n_u8(0);
        } else {
            err = vorrq_u8(err, block_err(cur, prev_input));
            prev_incomplete = vqsubq_u8(cur, inc_max);
        }
        prev_input = cur;
        i += 16;
    }
    if i < n {
        let mut buf = [0u8; 16];
        unsafe { core::ptr::copy_nonoverlapping(ptr.add(i), buf.as_mut_ptr(), n - i) };
        let cur = unsafe { vld1q_u8(buf.as_ptr()) };
        err = vorrq_u8(err, block_err(cur, prev_input));
        prev_incomplete = vqsubq_u8(cur, inc_max);
    }
    err = vorrq_u8(err, prev_incomplete);
    vmaxvq_u8(err) == 0
}

/// Validate that `bytes` is well-formed UTF-8 (draft §7/§12). Runtime-dispatched: AVX2 on x86_64 when
/// present, baseline NEON on aarch64, else the scalar reference — every path returns the same answer.
fn validate_utf8(bytes: &[u8]) -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            return unsafe { validate_utf8_avx2(bytes) };
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        return unsafe { validate_utf8_neon(bytes) };
    }
    #[cfg(not(target_arch = "aarch64"))]
    validate_utf8_scalar(bytes)
}

/// The key `"..."` immediately before the colon at byte position `cpos`, scanned from the raw bytes
/// (the lean index holds no quotes). Skips whitespace, then the closing quote, back to the opening
/// quote. `None` if the bytes there are not a plain `"..."` key — including an **escaped** key (any
/// `\` in or escaping it), which is rejected (matching [`JsonParser::string`], so the record errors)
/// rather than risk a wrong/shorter parse silently matching a declared field.
#[inline]
fn key_before_colon(src: &[u8], cpos: usize) -> Option<&[u8]> {
    let mut e = cpos;
    while e > 0 && matches!(src[e - 1], b' ' | b'\t' | b'\n' | b'\r') {
        e -= 1;
    }
    if e == 0 || src[e - 1] != b'"' {
        return None;
    }
    let close = e - 1;
    let mut s = close;
    while s > 0 && src[s - 1] != b'"' {
        s -= 1;
    }
    if s == 0 {
        return None;
    }
    // If the opening quote we stopped at is itself escaped (`\"`), the scan-back found a wrong,
    // shorter key that could silently match a declared field — reject it (so the record errors).
    // This O(1) check covers the dangerous case. A backslash *inside* an otherwise-plain key
    // (`"a\\b"`) is not separately rejected here (it just won't match any declared name → treated
    // as unknown — a narrow relaxation vs `string()`'s strict reject, traded for not scanning every
    // key on the hot path).
    if s >= 2 && src[s - 2] == b'\\' {
        return None;
    }
    Some(&src[s..close])
}

/// Speculation-path key verify: is the key right before the colon at `cpos` exactly `"name"`?
/// Unlike [`key_before_colon`] (which *delimits* an unknown key by scanning back to its opening
/// quote, then the caller compares), speculation already knows the expected `name`, so the opening
/// quote's position is computed from `name.len()` — no backward key scan — and the bytes are matched
/// against `name` directly. This collapses the two hottest verify costs (the scan-back loop and the
/// generic `memcmp` dispatch) into a bounded, inlinable check. Same soundness as the original: returns
/// `true` only when the bytes are `"<name>"` (closing quote at the trimmed end, matching key bytes, a
/// non-escaped opening quote). On any drift it returns `false` → the caller falls back.
#[inline]
fn key_matches_before_colon(src: &[u8], cpos: usize, name: &[u8]) -> bool {
    // Skip whitespace between the key string and the colon (`"k" :` is legal).
    let mut e = cpos;
    while e > 0 && matches!(src[e - 1], b' ' | b'\t' | b'\n' | b'\r') {
        e -= 1;
    }
    let nl = name.len();
    // Need room for `"` + name + `"`; the closing quote sits at `e-1`, the key at `[ks..e-1]`, the
    // opening quote at `ks-1`. `e >= nl + 2` keeps every index below in bounds (`ks >= 1`).
    if e < nl + 2 || src[e - 1] != b'"' {
        return false;
    }
    let ks = e - 1 - nl; // key start
    if src[ks - 1] != b'"' {
        return false; // no opening quote where a `"name"` key would put it (length drift)
    }
    if ks >= 2 && src[ks - 2] == b'\\' {
        return false; // an escaped opening quote `\"` — reject (matches key_before_colon)
    }
    // Equal-length compare against the known `name`; short and bounded, so it inlines (no memcmp call).
    src[ks..e - 1] == *name
}

/// A minimal JSON scanner over a byte slice (just what `json.decode` needs: objects with
/// integer / boolean values; strings only as keys).
struct JsonParser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }
    fn expect(&mut self, c: u8) -> Option<()> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Some(())
        } else {
            None
        }
    }
    /// Read a `"..."` string key (no escapes for the M5 cut). Borrows the input (`&'a`), so
    /// it does not hold `self`, and the parser can keep advancing after. The body is located with
    /// [`find_quote_or_escape`] (a SIMD scan for long strings).
    fn string(&mut self) -> Option<&'a [u8]> {
        self.expect(b'"')?;
        let start = self.pos;
        let rest = &self.src[self.pos..];
        let off = find_quote_or_escape(rest)?;
        if rest[off] == b'\\' {
            return None; // escapes in keys unsupported (M5 cut)
        }
        self.pos += off; // at the closing quote
        let s = &self.src[start..self.pos];
        self.pos += 1; // consume `"`
        Some(s)
    }
    fn integer(&mut self) -> Option<i64> {
        let neg = self.peek() == Some(b'-');
        if neg {
            self.pos += 1;
        }
        let digits = self.pos;
        // Accumulate digits directly in one pass (no UTF-8 validation, no generic `parse`) — the
        // hot path for integer fields. Accumulate as a *negative* magnitude so `i64::MIN` stays
        // representable; `checked_*` rejects overflow (matching the old `parse::<i64>()` reject).
        let mut v: i64 = 0;
        while let Some(c @ b'0'..=b'9') = self.peek() {
            match v.checked_mul(10).and_then(|x| x.checked_sub((c - b'0') as i64)) {
                Some(nv) => {
                    v = nv;
                    self.pos += 1;
                }
                // Overflow: consume the rest of the digits (so the parser ends up past the whole
                // number, matching the old `parse` behavior) and then reject. No branch is added to
                // the success path — this arm is the cold error edge.
                None => {
                    while matches!(self.peek(), Some(b'0'..=b'9')) {
                        self.pos += 1;
                    }
                    return None;
                }
            }
        }
        if self.pos == digits {
            return None;
        }
        if neg { Some(v) } else { v.checked_neg() }
    }
    /// Parse a non-negative JSON integer into the **full `u64` range** (unsigned accumulate +
    /// `checked_*`). Used only for a width-8 *unsigned* (`u64`) field, where `[0, u64::MAX]` must all
    /// be accepted — the `i64` [`integer`] path caps at `i64::MAX`, so a value in `(i64::MAX, u64::MAX]`
    /// (e.g. `i64::MAX + 1`) is representable here but not there. A leading `-` (a `u64` rejects every
    /// negative) or an overflow past `u64::MAX` is rejected (`None`); either way the cursor is left
    /// past the whole number token (matching [`integer`]), so a failed parse aborts the record cleanly.
    fn integer_unsigned(&mut self) -> Option<u64> {
        // A `u64` field rejects any negative. Consume a leading `-` and its digits (so the cursor
        // ends past the whole number token, matching `integer`'s overflow arm) and then reject.
        if self.peek() == Some(b'-') {
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            return None;
        }
        let digits = self.pos;
        let mut v: u64 = 0;
        while let Some(c @ b'0'..=b'9') = self.peek() {
            match v.checked_mul(10).and_then(|x| x.checked_add((c - b'0') as u64)) {
                Some(nv) => {
                    v = nv;
                    self.pos += 1;
                }
                // Overflow past `u64::MAX`: consume the rest of the digits (so the cursor ends past
                // the whole number, matching `integer`) and reject. Cold error edge.
                None => {
                    while matches!(self.peek(), Some(b'0'..=b'9')) {
                        self.pos += 1;
                    }
                    return None;
                }
            }
        }
        if self.pos == digits {
            return None;
        }
        Some(v)
    }
    /// Parse a JSON integer for a target field of `w` bytes and signedness `signed`, returning the
    /// little-endian bit pattern to write as a `u64` (the caller stores its low `w` bytes). An
    /// out-of-range value is rejected (`None`). A width-8 *unsigned* (`u64`) field routes to
    /// [`integer_unsigned`] so the full `[0, u64::MAX]` range is accepted; every other width and every
    /// signed field reuses the `i64` [`integer`] path + [`int_in_range`] (preserving the negative /
    /// overflow / `i64::MIN`-edge handling), then reinterprets the `i64` as its `u64` bit pattern
    /// (`i64 as u64` — the low `w` bytes are identical two's-complement). Single-sourced so the three
    /// integer write sites (`parse_object` / `write_field_indexed` / `decode_array`) stay consistent.
    #[inline]
    fn integer_field(&mut self, w: usize, signed: bool) -> Option<u64> {
        if !signed && w == 8 {
            self.integer_unsigned()
        } else {
            let v = self.integer()?;
            if !int_in_range(v, w, signed) {
                return None;
            }
            Some(v as u64)
        }
    }
    /// Advance the cursor over a JSON number token (`-?int(.digits)?([eE][+-]?digits)?`),
    /// returning its byte span, or `None` (restoring the cursor) when the cursor is not at a valid
    /// number. Enforces the JSON grammar's mandatory digits — at least one in the integer part, and
    /// at least one after a `.` or in an exponent — so malformed forms (`-`, `.5`, `1.`, `1e`,
    /// `1e+`) are rejected rather than half-consumed. Shared by [`number`] (which parses the span)
    /// and [`skip_number`] (which discards it), so the two accept exactly the same language.
    fn number_span(&mut self) -> Option<&'a [u8]> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        // Integer part: at least one digit (else a lone `-`, or a leading `.`).
        let int_start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == int_start {
            self.pos = start;
            return None;
        }
        // Fraction: a `.` must be followed by at least one digit (rejects `1.`).
        if self.peek() == Some(b'.') {
            self.pos += 1;
            let frac_start = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            if self.pos == frac_start {
                self.pos = start;
                return None;
            }
        }
        // Exponent: `[eE][+-]?` must be followed by at least one digit (rejects `1e`, `1e+`).
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let exp_start = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            if self.pos == exp_start {
                self.pos = start;
                return None;
            }
        }
        Some(&self.src[start..self.pos])
    }
    /// Read a JSON number as `f64`.
    fn number(&mut self) -> Option<f64> {
        let span = self.number_span()?;
        std::str::from_utf8(span).ok()?.parse::<f64>().ok()
    }
    /// Skip a JSON number **lexically** — advance over the token without parsing it to `f64`.
    /// Used by [`skip_value`] for unknown numeric fields, whose value is discarded; lexical skip
    /// is ~3x faster than a full parse (verified by `work/skip_number_probe.rs`).
    fn skip_number(&mut self) -> Option<()> {
        self.number_span().map(|_| ())
    }
    fn boolean(&mut self) -> Option<bool> {
        if self.src[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Some(true)
        } else if self.src[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Some(false)
        } else {
            None
        }
    }
    /// Skip a `"..."` string, honoring `\` escapes so an embedded `\"` does not end it early
    /// (unlike [`string`], which is for zero-copy *keys* and rejects escapes). Used only to
    /// discard an unknown value, so the escape bytes need not be decoded — just stepped over.
    /// Each clean run is found with [`find_quote_or_escape`] (a SIMD scan for long strings).
    fn skip_string(&mut self) -> Option<()> {
        self.expect(b'"')?;
        loop {
            let off = find_quote_or_escape(&self.src[self.pos..])?;
            self.pos += off;
            match self.src[self.pos] {
                b'"' => {
                    self.pos += 1;
                    return Some(());
                }
                // `\`: step over the backslash and the escaped byte (`\"`, `\\`, the `u` of
                // `\uXXXX` — the four hex digits are then skipped as ordinary clean bytes).
                _ => {
                    self.pos += 1;
                    self.peek()?;
                    self.pos += 1;
                }
            }
        }
    }
    fn skip_null(&mut self) -> Option<()> {
        if self.src[self.pos..].starts_with(b"null") {
            self.pos += 4;
            Some(())
        } else {
            None
        }
    }
    /// Skip a value of an unknown key — number / string / bool / null / nested object / nested
    /// array — so a narrow struct decodes from JSON carrying fields it does not declare (the
    /// "declare only what you need" projection rail). Recursion is depth-bounded so adversarially
    /// nested input is rejected rather than overflowing the stack.
    fn skip_value(&mut self) -> Option<()> {
        self.skip_value_depth(0)
    }
    fn skip_value_depth(&mut self, depth: u32) -> Option<()> {
        // Bound nesting so a pathological `[[[[…` cannot exhaust the native stack.
        const MAX_DEPTH: u32 = 128;
        if depth > MAX_DEPTH {
            return None;
        }
        match self.peek() {
            Some(b't' | b'f') => self.boolean().map(|_| ()),
            Some(b'n') => self.skip_null(),
            Some(b'-' | b'0'..=b'9') => self.skip_number(),
            Some(b'"') => self.skip_string(),
            Some(b'{') => self.skip_object(depth),
            Some(b'[') => self.skip_array(depth),
            _ => None,
        }
    }
    /// Skip a `{ "key": value, ... }` object, discarding every member (keys via [`skip_string`],
    /// values recursively). Mirrors the whitespace handling of the real object parser.
    fn skip_object(&mut self, depth: u32) -> Option<()> {
        self.expect(b'{')?;
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Some(());
        }
        loop {
            self.ws();
            self.skip_string()?; // member key
            self.ws();
            self.expect(b':')?;
            self.ws();
            self.skip_value_depth(depth + 1)?; // member value
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Some(());
                }
                _ => return None,
            }
        }
    }
    /// Skip a `[ value, ... ]` array, discarding every element recursively.
    fn skip_array(&mut self, depth: u32) -> Option<()> {
        self.expect(b'[')?;
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Some(());
        }
        loop {
            self.ws();
            self.skip_value_depth(depth + 1)?; // element
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Some(());
                }
                _ => return None,
            }
        }
    }
}

/// Finish the builder, returning a `str` view over the (leaked) contents and freeing
/// the builder object.
///
/// # Safety
/// `b` must be a non-null pointer returned by [`align_rt_builder_new`] and not yet finished/freed;
/// this call consumes it (frees the `Builder` object), so `b` must not be used again afterward.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_finish(b: *mut Builder) -> AlignStr {
    let b = unsafe { Box::from_raw(b) };
    let len = b.buf.len() as i64;
    if len == 0 {
        // Empty: no allocation needed; a dangling non-null ptr is valid for a 0-len view.
        AlignStr { ptr: std::ptr::NonNull::dangling().as_ptr(), len: 0 }
    } else if b.arena.is_null() {
        // No arena: leak the buffer so the view stays valid (process-lifetime).
        let ptr = Box::leak(b.buf.into_boxed_slice()).as_ptr();
        AlignStr { ptr, len }
    } else {
        // Copy into the arena so the view is freed with it (no leak).
        let arena = unsafe { &mut *b.arena };
        let dst = arena.alloc(b.buf.len(), 1);
        unsafe { std::ptr::copy_nonoverlapping(b.buf.as_ptr(), dst, b.buf.len()) };
        AlignStr { ptr: dst, len }
    }
}

/// Finish a surface `builder` (`b.to_string()`), returning an **owned** `string` `{ptr,len}`
/// (MMv2 slice 7c) and freeing the builder object. The bytes are copied into a fresh
/// [`align_rt_alloc`] buffer, freed by the generated code's `Drop` of the owning slot — so the
/// finished string outlives the builder and any arena. An empty result owns no buffer (null
/// ptr), so its `free(null)` drop is a harmless no-op.
///
/// # Safety
/// `b` must be a non-null pointer returned by [`align_rt_builder_new`] and not yet finished/freed;
/// this call consumes it (frees the `Builder` object), so `b` must not be used again afterward.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_into_string(b: *mut Builder) -> AlignStr {
    let b = unsafe { Box::from_raw(b) };
    let len = b.buf.len() as i64;
    if len == 0 {
        return AlignStr { ptr: core::ptr::null(), len: 0 };
    }
    let dst = align_rt_alloc(len);
    unsafe { core::ptr::copy_nonoverlapping(b.buf.as_ptr(), dst, b.buf.len()) };
    AlignStr { ptr: dst, len }
}

/// Free a `builder` object that was never finished (`to_string()` not called) — the `Drop` of an
/// owned builder slot (MMv2 slice 7c). Null-safe: a builder slot nulled on move (its value was
/// consumed by `to_string()`) drops to a no-op.
///
/// # Safety
/// `b` must be null or a pointer returned by [`align_rt_builder_new`] and not yet finished/freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_free(b: *mut Builder) {
    if !b.is_null() {
        drop(unsafe { Box::from_raw(b) });
    }
}

// ---------------------------------------------------------------------------------------------
// std.io / std.fs — `reader` / `writer` (own an fd, `Drop` closes it) + `buffer` (owned bytes).
// The one fixed errno→`Error` mapping table (`draft.md` §18.2): every std fn maps a failing
// syscall's errno through `io_error_to_status`, and MIR rebuilds the builtin `Error` from the
// returned status — see `make_error_from_status` (`align_mir`). The status encoding is shared:
//   0            success
//   AL_NOT_FOUND success->Error.NotFound   (ENOENT)
//   AL_INVALID   ->Error.Invalid           (EINVAL)
//   AL_DENIED    ->Error.Denied            (EACCES / EPERM)
//   >= AL_CODE   ->Error.Code(status - AL_CODE)   (anything else — the raw errno)
// The three category sentinels sit below `AL_CODE` so they never collide with an encoded errno.
// ---------------------------------------------------------------------------------------------

const AL_NOT_FOUND: i32 = 1;
const AL_INVALID: i32 = 2;
const AL_DENIED: i32 = 3;
/// Base offset for `Error.Code(errno)`: an encoded status is `AL_CODE + errno`, kept above the
/// three category sentinels so a small errno (e.g. `ESRCH` = 3) can never look like a category.
const AL_CODE: i32 = 4;

/// The one fixed errno→`Error` table (`draft.md` §18.2). Uses `std::io::ErrorKind` so the mapping
/// is portable (the kernel errno numbers differ per platform): `NotFound`←ENOENT, `PermissionDenied`
/// ←EACCES/EPERM, `InvalidInput`←EINVAL; anything else carries its raw errno as `Error.Code`.
fn io_error_to_status(e: &std::io::Error) -> i32 {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::NotFound => AL_NOT_FOUND,
        ErrorKind::PermissionDenied => AL_DENIED,
        ErrorKind::InvalidInput => AL_INVALID,
        // `AL_CODE + errno`. `saturating_add` keeps a pathological errno from wrapping into a
        // category sentinel; a missing errno (`None`) degrades to `Code(0)`.
        _ => AL_CODE.saturating_add(e.raw_os_error().unwrap_or(0)),
    }
}

/// 64 KiB — large enough to amortize the syscall over many small writes, small enough to stay in
/// cache and bound a buffered writer's memory to O(buffer).
const BUF_WRITER_CAP: usize = 64 * 1024;

/// Write all of `bytes` to `fd`, looping over partial writes and retrying `EINTR`. Returns `0` on
/// success, else the errno mapped through [`io_error_to_status`]. An empty slice succeeds without a
/// syscall.
fn write_all_fd(fd: i32, mut bytes: &[u8]) -> i32 {
    while !bytes.is_empty() {
        let n = unsafe { write(fd, bytes.as_ptr() as *const core::ffi::c_void, bytes.len()) };
        if n > 0 {
            bytes = &bytes[n as usize..];
        } else {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue; // interrupted before writing: retry
            }
            // A genuine error, or a 0-byte write (treat as failure rather than spin). A 0-byte
            // write leaves errno unset, so `io_error_to_status` yields `Code(0)` — a distinct
            // non-success status.
            return io_error_to_status(&e);
        }
    }
    0
}

/// Copy a `str` view's bytes (`ptr`/`len`) into an owned UTF-8 path `String`. `None` for a
/// length that doesn't fit `usize` (a 32-bit target) or non-UTF-8 bytes.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range when `len > 0`.
unsafe fn path_from_view(ptr: *const u8, len: i64) -> Option<String> {
    let bytes: &[u8] = if len <= 0 || ptr.is_null() {
        &[]
    } else {
        let n = safe_len(len).ok()?;
        unsafe { std::slice::from_raw_parts(ptr, n) }
    };
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

// --- reader -----------------------------------------------------------------------------------

/// A `reader` (`std.io`) — a Move handle owning a file descriptor; `Drop` (`align_rt_io_reader_free`)
/// closes it iff `owns_fd` (a `fs.open` file, not a borrowed `io.stdin`).
pub struct Reader {
    fd: i32,
    owns_fd: bool,
}

/// `io.stdin` — a `reader` over fd 0. Borrows the fd (does not close it on `Drop`).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_io_reader_stdin() -> *mut Reader {
    Box::into_raw(Box::new(Reader { fd: 0, owns_fd: false }))
}

/// `fs.open(path)` — open `path` (a `str` view) for reading, writing the owned `reader` handle to
/// `out`. Returns `0` on success, else the errno mapped through [`io_error_to_status`] (leaving
/// `*out` null). The fd is owned — `Drop` closes it.
///
/// # Safety
/// `path`/`path_len` must describe a valid byte range; `out` must point to a writable slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_reader_open(path: *const u8, path_len: i64, out: *mut *mut Reader) -> i32 {
    if out.is_null() {
        return AL_INVALID;
    }
    unsafe { *out = core::ptr::null_mut() };
    let Some(path_str) = (unsafe { path_from_view(path, path_len) }) else {
        return AL_INVALID;
    };
    use std::os::fd::IntoRawFd;
    match std::fs::File::open(&path_str) {
        Ok(f) => {
            unsafe { *out = Box::into_raw(Box::new(Reader { fd: f.into_raw_fd(), owns_fd: true })) };
            0
        }
        Err(e) => io_error_to_status(&e),
    }
}

/// `r.read(b: mut buffer)` — read up to `b`'s capacity from the reader's fd into `b`, overwriting
/// `b`'s length. Returns the number of bytes read (`0` = EOF) on success, or `-(status)` where
/// `status` is the errno mapped through [`io_error_to_status`] (always `>= 1`, so an error is a
/// distinct negative value). Retries `EINTR`.
///
/// # Safety
/// `r` and `b` must be valid handles for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_reader_read(r: *mut Reader, b: *mut Buffer) -> i64 {
    if r.is_null() || b.is_null() {
        return -(AL_INVALID as i64);
    }
    let r = unsafe { &*r };
    let b = unsafe { &mut *b };
    if b.cap == 0 {
        b.len = 0;
        return 0;
    }
    // Ensure the backing storage spans the full capacity (read fills up to `cap`).
    if b.data.len() != b.cap {
        b.data.resize(b.cap, 0);
    }
    loop {
        let n = unsafe { read(r.fd, b.data.as_mut_ptr() as *mut core::ffi::c_void, b.cap) };
        if n >= 0 {
            b.len = n as usize;
            return n as i64;
        }
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        b.len = 0;
        return -(io_error_to_status(&e) as i64);
    }
}

/// Free a `reader`, closing its fd first iff owned. Null-safe (a never-initialised owned slot
/// drops harmlessly).
///
/// # Safety
/// `r` must be null or a pointer from [`align_rt_io_reader_open`] / [`align_rt_io_reader_stdin`],
/// not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_reader_free(r: *mut Reader) {
    if r.is_null() {
        return;
    }
    let r = unsafe { Box::from_raw(r) };
    if r.owns_fd {
        unsafe { close(r.fd) };
    }
}

// --- writer -----------------------------------------------------------------------------------

/// A `writer` (`std.io`) — one Move type for every write sink (`io.stdout`, `io.stderr`,
/// `io.stdout.buffered()`, `fs.create`): it owns an fd and, when `buffered`, an O(buffer)
/// accumulator that reaches the fd only on a full buffer / explicit `flush` / `Drop`. `Drop`
/// (`align_rt_io_writer_free`) flushes best-effort, then closes the fd iff `owns_fd`.
pub struct Writer {
    fd: i32,
    owns_fd: bool,
    buffered: bool,
    buf: Vec<u8>,
}

impl Writer {
    /// Flush the accumulator to the fd, clearing it on success. Returns the write status.
    fn flush_buf(&mut self) -> i32 {
        if self.buf.is_empty() {
            return 0;
        }
        let s = write_all_fd(self.fd, &self.buf);
        self.buf.clear(); // drop bytes regardless; the status reports any loss
        s
    }
}

/// `io.stdout` / `io.stderr` / `io.stdout.buffered()` — a `writer` over a standard-stream fd
/// (1 = stdout, 2 = stderr), `buffered != 0` selecting the O(buffer) accumulator. The fd is
/// borrowed (never closed on `Drop`).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_io_writer_std(fd: i32, buffered: i32) -> *mut Writer {
    let buffered = buffered != 0;
    let buf = if buffered { Vec::with_capacity(BUF_WRITER_CAP) } else { Vec::new() };
    Box::into_raw(Box::new(Writer { fd, owns_fd: false, buffered, buf }))
}

/// `fs.create(path)` — create/truncate `path` (a `str` view) for writing, writing the owned
/// `writer` handle to `out`. Buffered (the file sink amortizes syscalls). Returns `0` on success,
/// else the errno mapped through [`io_error_to_status`] (leaving `*out` null). The fd is owned —
/// `Drop` flushes then closes it.
///
/// # Safety
/// `path`/`path_len` must describe a valid byte range; `out` must point to a writable slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_writer_create(path: *const u8, path_len: i64, out: *mut *mut Writer) -> i32 {
    if out.is_null() {
        return AL_INVALID;
    }
    unsafe { *out = core::ptr::null_mut() };
    let Some(path_str) = (unsafe { path_from_view(path, path_len) }) else {
        return AL_INVALID;
    };
    use std::os::fd::IntoRawFd;
    match std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&path_str) {
        Ok(f) => {
            unsafe {
                *out = Box::into_raw(Box::new(Writer {
                    fd: f.into_raw_fd(),
                    owns_fd: true,
                    buffered: true,
                    buf: Vec::with_capacity(BUF_WRITER_CAP),
                }))
            };
            0
        }
        Err(e) => io_error_to_status(&e),
    }
}

/// `w.write(bytes)` — append `ptr`/`len` bytes to the writer. An unbuffered writer streams straight
/// to the fd; a buffered one accumulates, flushing when the buffer would overflow (a chunk at least
/// the buffer's size is written straight through, no double copy). Returns `0` on success, else the
/// errno mapped through [`io_error_to_status`].
///
/// # Safety
/// `w` must be a valid `Writer` pointer; `ptr`/`len` must describe a valid byte range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_writer_write(w: *mut Writer, ptr: *const u8, len: i64) -> i32 {
    if w.is_null() {
        return AL_INVALID;
    }
    if len <= 0 || ptr.is_null() {
        return 0; // nothing to write — success
    }
    let w = unsafe { &mut *w };
    let Ok(n) = safe_len(len) else { return AL_INVALID };
    let bytes = unsafe { std::slice::from_raw_parts(ptr, n) };
    if !w.buffered {
        return write_all_fd(w.fd, bytes);
    }
    if w.buf.len() + n > BUF_WRITER_CAP {
        let s = w.flush_buf();
        if s != 0 {
            return s;
        }
        if n >= BUF_WRITER_CAP {
            return write_all_fd(w.fd, bytes);
        }
    }
    w.buf.extend_from_slice(bytes);
    0
}

/// `w.write(b)` for a `builder` — append the builder's accumulated bytes (a borrow; the builder is
/// not consumed). Returns the same status as [`align_rt_io_writer_write`].
///
/// # Safety
/// `w` must be a valid `Writer`; `b` must be a valid `Builder` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_writer_write_builder(w: *mut Writer, b: *mut Builder) -> i32 {
    if b.is_null() {
        return AL_INVALID;
    }
    let b = unsafe { &*b };
    let (ptr, len) = (b.buf.as_ptr(), b.buf.len() as i64);
    unsafe { align_rt_io_writer_write(w, ptr, len) }
}

/// `w.flush()` — write any buffered bytes to the fd. Returns `0` on success, else the mapped errno.
///
/// # Safety
/// `w` must be a valid `Writer` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_writer_flush(w: *mut Writer) -> i32 {
    if w.is_null() {
        return AL_INVALID;
    }
    unsafe { (*w).flush_buf() }
}

/// Free a `writer`, flushing any buffered bytes best-effort first (errors are not observable here —
/// use an explicit `flush()?` to handle them), then closing the fd iff owned. Null-safe.
///
/// # Safety
/// `w` must be null or a pointer from a `writer` constructor, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_writer_free(w: *mut Writer) {
    if w.is_null() {
        return;
    }
    let mut w = unsafe { Box::from_raw(w) };
    let _ = w.flush_buf();
    if w.owns_fd {
        unsafe { close(w.fd) };
    }
}

/// `io.copy(r, w)` — stream all of `r` into `w` through a fixed 64 KiB buffer (memory is
/// O(buffer), never O(file size)), returning the number of bytes transferred, or `-(status)` on
/// error (the errno mapped through [`io_error_to_status`], sign-encoded like
/// [`align_rt_io_reader_read`]). Both handles are **borrowed** — neither fd is closed here, so the
/// caller's `reader`/`writer` remain usable afterward. Retries `EINTR` on read; the write side
/// (partial writes, `EINTR`, buffering) is shared with [`align_rt_io_writer_write`]. Buffered bytes
/// left in `w` flush on its `flush()` / `Drop`, like any other `w.write` (this does not force a
/// flush — that stays the caller's one way, `w.flush()`).
///
/// v1 is this portable fixed-buffer loop (the reference implementation). A Linux `sendfile` /
/// `splice` fast path (file → pipe/socket) would dispatch on the fd kinds at the marked point
/// below — post-M9 (`docs/open-questions.md` "Transparent zero-copy I/O"), validated against this
/// loop and without changing the signature.
///
/// # Safety
/// `r` must be a valid `Reader` pointer and `w` a valid `Writer` pointer for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_copy(r: *mut Reader, w: *mut Writer) -> i64 {
    if r.is_null() || w.is_null() {
        return -(AL_INVALID as i64);
    }
    let rfd = unsafe { (*r).fd };
    // A fixed 64 KiB transfer buffer (matches `BUF_WRITER_CAP`) — the point is O(buffer) memory,
    // independent of the file size. `try_reserve` so a hostile/OOM environment fails softly
    // (EINVAL) instead of aborting the process.
    let mut buf: Vec<u8> = Vec::new();
    if buf.try_reserve_exact(BUF_WRITER_CAP).is_err() {
        return -(AL_INVALID as i64);
    }
    buf.resize(BUF_WRITER_CAP, 0);

    // Fast-path dispatch site (post-M9): on Linux, if `rfd` is a regular file and `w`'s fd is a
    // pipe/socket, a `sendfile`/`splice` loop would replace the read+write below — same result,
    // same O(buffer) bound, no signature change. v1 always takes the portable loop.
    let mut total: i64 = 0;
    loop {
        let n = unsafe { read(rfd, buf.as_mut_ptr() as *mut core::ffi::c_void, BUF_WRITER_CAP) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue; // interrupted before reading: retry
            }
            return -(io_error_to_status(&e) as i64);
        }
        if n == 0 {
            break; // EOF
        }
        // Route the chunk through the writer so buffering + partial-write + EINTR handling is the
        // one shared implementation.
        let s = unsafe { align_rt_io_writer_write(w, buf.as_ptr(), n as i64) };
        if s != 0 {
            return -(s as i64);
        }
        total = total.saturating_add(n as i64);
    }
    total
}

// --- buffer -----------------------------------------------------------------------------------

/// A `buffer` (`core.buffer`) — an owned, growable byte container (the byte analog of `Vec<u8>`),
/// the caller-owned sink a `reader.read` fills. `cap` is the read window; `len` is how many bytes
/// the last read produced (`.bytes()` views `data[..len]`). A Move type, `Drop`-freed.
pub struct Buffer {
    data: Vec<u8>,
    cap: usize,
    len: usize,
}

/// `buffer(cap)` — open an owned byte buffer whose read window is `cap` bytes (`<= 0` → empty).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_buffer_new(cap: i64) -> *mut Buffer {
    let requested = safe_len(cap).unwrap_or(0);
    let mut data = Vec::new();
    // `try_reserve` so a bogus/huge capacity fails softly instead of aborting on OOM. The read
    // window is capped to what was actually reserved, so `reader.read`'s later `resize(cap)` can
    // never trigger a new (infallible, abort-on-OOM) allocation — a huge `buffer(cap)` degrades to
    // an empty window rather than crashing the process.
    let cap = match data.try_reserve_exact(requested) {
        Ok(()) => requested,
        Err(_) => 0,
    };
    Box::into_raw(Box::new(Buffer { data, cap, len: 0 }))
}

/// `b.bytes()` — a `slice<u8>` view of the buffer's current contents (`data[..len]`), written to
/// `out` as a `{ptr,len}`. The view borrows the buffer (region-tracked; must not outlive it).
///
/// # Safety
/// `b` must be a valid `Buffer`; `out` must point to a writable `{ptr,len}` slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_buffer_bytes(b: *mut Buffer, out: *mut AlignStr) {
    if out.is_null() {
        return;
    }
    if b.is_null() {
        unsafe { *out = AlignStr { ptr: core::ptr::null(), len: 0 } };
        return;
    }
    let b = unsafe { &*b };
    unsafe { *out = AlignStr { ptr: b.data.as_ptr(), len: b.len as i64 } };
}

/// `b.len()` — the number of bytes the buffer currently holds (the last read's count).
///
/// # Safety
/// `b` must be a valid `Buffer` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_buffer_len(b: *mut Buffer) -> i64 {
    if b.is_null() {
        return 0;
    }
    unsafe { (*b).len as i64 }
}

/// Free a `buffer` (its heap storage). Null-safe.
///
/// # Safety
/// `b` must be null or a pointer from [`align_rt_buffer_new`], not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_buffer_free(b: *mut Buffer) {
    if !b.is_null() {
        drop(unsafe { Box::from_raw(b) });
    }
}

// ---------------------------------------------------------------------------------------------
// std.encoding (M10 Slice 1) — Base64 (standard + URL-safe), hex, and UTF-8 validation. Pure
// functions over `bytes`/`str`: encode returns an owned `string` (a fresh `align_rt_alloc` buffer,
// freed by the generated `Drop`); decode returns an owned `buffer` handle (Box, freed by
// `align_rt_buffer_free`) or fails with `AL_INVALID` -> `Error.Invalid`. v1 is a scalar reference
// implementation (correctness before speed); a Lemire-class SIMD Base64 is a later optimization
// behind these same symbols (`draft.md` §18.2, `open-questions.md` #342), never a signature change.
// ---------------------------------------------------------------------------------------------

/// The standard Base64 alphabet (RFC 4648 §4): `A-Za-z0-9+/`.
const BASE64_STD: [u8; 64] = *b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
/// The URL/filename-safe Base64 alphabet (RFC 4648 §5): `+`->`-`, `/`->`_`.
const BASE64_URL: [u8; 64] = *b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The reverse decode table for a Base64 `alphabet`: symbol byte -> 6-bit value, `0xFF` = not in
/// this alphabet. Built once at compile time (a `const fn`), so a decode is a pure table lookup with
/// no per-call setup.
const fn base64_decode_table(alphabet: &[u8; 64]) -> [u8; 256] {
    let mut table = [0xFFu8; 256];
    let mut i = 0;
    while i < 64 {
        table[alphabet[i] as usize] = i as u8;
        i += 1;
    }
    table
}

/// Compile-time reverse tables for the two alphabets (built once, not per `base64_decode_impl` call).
static BASE64_STD_TABLE: [u8; 256] = base64_decode_table(&BASE64_STD);
static BASE64_URL_TABLE: [u8; 256] = base64_decode_table(&BASE64_URL);

/// Copy an owned byte vector into a freshly `align_rt_alloc`'d `string` `{ptr,len}` (the generated
/// `Drop` frees the buffer via `align_rt_free`). An empty result owns no buffer (null ptr, len 0),
/// so its `free(null)` drop is a harmless no-op — same convention as `align_rt_str_clone`.
fn owned_str_from_vec(v: &[u8]) -> AlignStr {
    let n = v.len();
    if n == 0 {
        return AlignStr { ptr: core::ptr::null(), len: 0 };
    }
    let dst = align_rt_alloc(n as i64);
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), dst, n) };
    AlignStr { ptr: dst, len: n as i64 }
}

/// Wrap a decoded byte vector into an owned `buffer` handle (`cap == len == v.len()`, so `.bytes()`
/// views all of it and `.len()` is its length). Freed by `align_rt_buffer_free` like every `buffer`.
fn buffer_from_vec(v: Vec<u8>) -> *mut Buffer {
    let n = v.len();
    Box::into_raw(Box::new(Buffer { data: v, cap: n, len: n }))
}

/// Encode `data` into `out` using `alphabet`; append `=` padding to a whole 4-char group iff `pad`
/// (standard Base64 pads; URL-safe does not — `draft.md` §18.2). Pure, allocation-only.
fn base64_encode_into(data: &[u8], alphabet: &[u8; 64], pad: bool, out: &mut Vec<u8>) {
    out.reserve(data.len().div_ceil(3) * 4);
    let mut chunks = data.chunks_exact(3);
    for c in &mut chunks {
        let n = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
        out.push(alphabet[(n >> 18 & 63) as usize]);
        out.push(alphabet[(n >> 12 & 63) as usize]);
        out.push(alphabet[(n >> 6 & 63) as usize]);
        out.push(alphabet[(n & 63) as usize]);
    }
    let rem = chunks.remainder();
    match rem.len() {
        1 => {
            let n = (rem[0] as u32) << 16;
            out.push(alphabet[(n >> 18 & 63) as usize]);
            out.push(alphabet[(n >> 12 & 63) as usize]);
            if pad {
                out.push(b'=');
                out.push(b'=');
            }
        }
        2 => {
            let n = (rem[0] as u32) << 16 | (rem[1] as u32) << 8;
            out.push(alphabet[(n >> 18 & 63) as usize]);
            out.push(alphabet[(n >> 12 & 63) as usize]);
            out.push(alphabet[(n >> 6 & 63) as usize]);
            if pad {
                out.push(b'=');
            }
        }
        _ => {}
    }
}

/// Decode a Base64 `input` (accepting `url`-safe or standard alphabet by the flag). `None` on any
/// invalid input: a symbol outside the chosen alphabet, a stray `=` before the trailing padding,
/// a length whose non-pad remainder is 1 (impossible group), inconsistent padding, or non-zero
/// trailing bits (non-canonical). Padding is optional when absent; when present it must complete a
/// 4-char group. Scalar reference implementation.
fn base64_decode_impl(input: &[u8], url: bool) -> Option<Vec<u8>> {
    // The reverse table is built once at compile time (see `base64_decode_table`), so a decode is a
    // pure table lookup — no per-call setup.
    let table = if url { &BASE64_URL_TABLE } else { &BASE64_STD_TABLE };
    // Split off trailing `=` padding (at most 2). A `=` anywhere before the padding is rejected
    // below (it maps to 0xFF in the table since it is not an alphabet symbol).
    let mut end = input.len();
    let mut pads = 0usize;
    while end > 0 && input[end - 1] == b'=' {
        end -= 1;
        pads += 1;
    }
    if pads > 2 {
        return None;
    }
    let content = &input[..end];
    let rem = content.len() % 4;
    if rem == 1 {
        return None; // a lone trailing symbol carries < 8 bits — no valid encoding produces it.
    }
    // When padding is present it must bring the group to a multiple of 4 (RFC 4648); when absent,
    // an unpadded (URL-safe or stripped) input is accepted as-is.
    if pads > 0 && !(content.len() + pads).is_multiple_of(4) {
        return None;
    }
    let mut out: Vec<u8> = Vec::with_capacity(content.len() / 4 * 3 + 2);
    let mut acc: u32 = 0;
    let mut nbits: u32 = 0;
    for &c in content {
        let v = table[c as usize];
        if v == 0xFF {
            return None; // outside the alphabet (includes a mid-string `=`).
        }
        acc = (acc << 6) | v as u32;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((acc >> nbits) as u8);
        }
    }
    // Any leftover bits must be zero — a canonical encoding never sets the discarded padding bits.
    if nbits > 0 && (acc & ((1 << nbits) - 1)) != 0 {
        return None;
    }
    Some(out)
}

/// A single hex digit's value, accepting both cases; `None` for a non-hex byte.
fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// `encoding.base64_encode(data)` — standard alphabet + padding. Returns an owned `string`.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_base64_encode(ptr: *const u8, len: i64) -> AlignStr {
    let data = unsafe { bytes_view(ptr, len) };
    let mut out = Vec::new();
    base64_encode_into(data, &BASE64_STD, true, &mut out);
    owned_str_from_vec(&out)
}

/// `encoding.base64url_encode(data)` — URL-safe alphabet, no padding. Returns an owned `string`.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_base64url_encode(ptr: *const u8, len: i64) -> AlignStr {
    let data = unsafe { bytes_view(ptr, len) };
    let mut out = Vec::new();
    base64_encode_into(data, &BASE64_URL, false, &mut out);
    owned_str_from_vec(&out)
}

/// `encoding.hex_encode(data)` — lower-case hex. Returns an owned `string`.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_hex_encode(ptr: *const u8, len: i64) -> AlignStr {
    const HEX: [u8; 16] = *b"0123456789abcdef";
    let data = unsafe { bytes_view(ptr, len) };
    let mut out = Vec::with_capacity(data.len() * 2);
    for &b in data {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 15) as usize]);
    }
    owned_str_from_vec(&out)
}

/// `encoding.base64_decode(s)` — standard alphabet. Writes an owned `buffer` handle to `*out` and
/// returns `0`, or `AL_INVALID` (`Error.Invalid`) on invalid input (leaving `*out` null).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range; `out` must point to a writable handle slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_base64_decode(ptr: *const u8, len: i64, out: *mut *mut Buffer) -> i32 {
    unsafe { decode_into(base64_decode_impl(bytes_view(ptr, len), false), out) }
}

/// `encoding.base64url_decode(s)` — URL-safe alphabet. Same contract as [`align_rt_base64_decode`].
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range; `out` must point to a writable handle slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_base64url_decode(ptr: *const u8, len: i64, out: *mut *mut Buffer) -> i32 {
    unsafe { decode_into(base64_decode_impl(bytes_view(ptr, len), true), out) }
}

/// `encoding.hex_decode(s)` — accepts both cases; odd length / non-hex byte -> `AL_INVALID`.
/// Same out-slot contract as [`align_rt_base64_decode`].
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range; `out` must point to a writable handle slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_hex_decode(ptr: *const u8, len: i64, out: *mut *mut Buffer) -> i32 {
    let input = unsafe { bytes_view(ptr, len) };
    let decoded = if input.len() % 2 != 0 {
        None
    } else {
        let mut v = Vec::with_capacity(input.len() / 2);
        let mut ok = true;
        let mut i = 0;
        while i < input.len() {
            match (hex_val(input[i]), hex_val(input[i + 1])) {
                (Some(hi), Some(lo)) => v.push(hi << 4 | lo),
                _ => {
                    ok = false;
                    break;
                }
            }
            i += 2;
        }
        if ok { Some(v) } else { None }
    };
    unsafe { decode_into(decoded, out) }
}

/// Shared tail of the three decoders: on `Some(v)` publish an owned `buffer` handle and return `0`;
/// on `None` leave `*out` null and return `AL_INVALID` (`Error.Invalid`).
///
/// # Safety
/// `out` must point to a writable handle slot (or be null, handled here).
unsafe fn decode_into(decoded: Option<Vec<u8>>, out: *mut *mut Buffer) -> i32 {
    if out.is_null() {
        return AL_INVALID;
    }
    unsafe { *out = core::ptr::null_mut() };
    match decoded {
        Some(v) => {
            unsafe { *out = buffer_from_vec(v) };
            0
        }
        None => AL_INVALID,
    }
}

/// `encoding.utf8_valid(b)` — whether `b`'s bytes are valid UTF-8 (a thin wrapper over the shared
/// SIMD/scalar [`validate_utf8`], the same validator used at every str-returning I/O boundary).
/// Returns `1` if valid, `0` otherwise. Lets a caller check `bytes` before turning them into a `str`.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_utf8_valid(ptr: *const u8, len: i64) -> i32 {
    if validate_utf8(unsafe { bytes_view(ptr, len) }) { 1 } else { 0 }
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

/// `s.contains(needle)` (M5, `core.string`) — 1 if `needle`'s bytes occur in `s`, else 0. An
/// empty needle is always present. Backed by `memchr::memmem` (its own AVX2/NEON dispatch), the
/// byte-oriented scan the spec mandates over a `chars()` walk.
///
/// # Safety
/// Both `ptr`/`len` pairs must describe valid byte ranges for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_contains(hptr: *const u8, hlen: i64, nptr: *const u8, nlen: i64) -> i32 {
    if nlen <= 0 {
        return 1;
    }
    if nlen > hlen {
        return 0;
    }
    let (hay, needle) = unsafe {
        (
            safe_slice(hptr, hlen),
            safe_slice(nptr, nlen),
        )
    };
    memchr::memmem::find(hay, needle).is_some() as i32
}

/// `s.find(needle)` (M5, `core.string`) — the byte index of `needle`'s first occurrence in `s`, or
/// `-1` if absent (codegen turns the sentinel into `Option<i64>`: `None` for `-1`, else `Some(i)`).
/// An empty needle is found at index 0. Backed by `memchr::memmem`.
///
/// # Safety
/// Both `ptr`/`len` pairs must describe valid byte ranges for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_find(hptr: *const u8, hlen: i64, nptr: *const u8, nlen: i64) -> i64 {
    if nlen <= 0 {
        return 0;
    }
    if nlen > hlen {
        return -1;
    }
    let (hay, needle) = unsafe {
        (
            safe_slice(hptr, hlen),
            safe_slice(nptr, nlen),
        )
    };
    match memchr::memmem::find(hay, needle) {
        Some(i) => i as i64,
        None => -1,
    }
}

/// `s.rfind(needle)` (M5, `core.string`) — the byte index of `needle`'s **last** occurrence in `s`,
/// or `-1` if absent (codegen turns the sentinel into `Option<i64>`). An empty needle matches at the
/// end (`hlen`). Backed by `memchr::memmem::rfind`.
///
/// # Safety
/// Both `ptr`/`len` pairs must describe valid byte ranges for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_rfind(hptr: *const u8, hlen: i64, nptr: *const u8, nlen: i64) -> i64 {
    if nlen <= 0 {
        return hlen;
    }
    if nlen > hlen {
        return -1;
    }
    let (hay, needle) = unsafe {
        (
            safe_slice(hptr, hlen),
            safe_slice(nptr, nlen),
        )
    };
    match memchr::memmem::rfind(hay, needle) {
        Some(i) => i as i64,
        None => -1,
    }
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

/// Wrap a `&[u8]` sub-slice of an already-valid `str` buffer as an `AlignStr` view. The slice
/// aliases the original bytes (no allocation); an empty slice may carry a dangling/one-past pointer,
/// which is fine for a zero-length view (the generated code never dereferences it).
#[inline]
fn str_subview(s: &[u8]) -> AlignStr {
    AlignStr { ptr: s.as_ptr(), len: s.len() as i64 }
}

/// `s.trim()` (M5, `core.string`) — return a borrowed sub-`str` `{ptr,len}` of `s` with leading
/// and trailing ASCII whitespace removed. No allocation: the result points into the same buffer.
/// The whitespace set is Rust's `[u8]::trim_ascii` (space, `\t`, `\n`, `\x0c`, `\r` — the WHATWG
/// ASCII-whitespace set; vertical tab `\x0b` is *not* included). Unicode whitespace is deliberately
/// out of core (package-level).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_trim(ptr: *const u8, len: i64) -> AlignStr {
    if len <= 0 {
        return AlignStr { ptr, len: 0 };
    }
    let bytes = unsafe { safe_slice(ptr, len) };
    str_subview(bytes.trim_ascii())
}

/// `s.trim_start()` (M5, `core.string`) — borrowed sub-`str` with leading ASCII whitespace removed
/// (same set as [`align_rt_str_trim`]).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_trim_start(ptr: *const u8, len: i64) -> AlignStr {
    if len <= 0 {
        return AlignStr { ptr, len: 0 };
    }
    let bytes = unsafe { safe_slice(ptr, len) };
    str_subview(bytes.trim_ascii_start())
}

/// `s.trim_end()` (M5, `core.string`) — borrowed sub-`str` with trailing ASCII whitespace removed
/// (same set as [`align_rt_str_trim`]).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_trim_end(ptr: *const u8, len: i64) -> AlignStr {
    if len <= 0 {
        return AlignStr { ptr, len: 0 };
    }
    let bytes = unsafe { safe_slice(ptr, len) };
    str_subview(bytes.trim_ascii_end())
}

// ---------------------------------------------------------------------------------------------
// std.path / std.env / std.time (M9 Slice 4). `path.*` are pure lexical byte operations (UTF-8 is
// the representation, as for every other `str`, but the vocabulary — `/`, `.`, `..` — is ASCII, so
// the ops are byte-level and never split a multi-byte scalar). `env.*`/`time.*` touch process /
// OS state (Impure). Every returned owned `string` comes from `align_rt_alloc` and is freed by the
// generated `Drop`; the `path.base`/`dir`/`ext` results are borrowed sub-views of the input.
// ---------------------------------------------------------------------------------------------

/// View a `{ptr,len}` argument as a byte slice, tolerating the empty/null view (`{null,0}`).
///
/// # Safety
/// For a non-empty view, `ptr`/`len` must describe a valid byte range for the call.
#[inline]
unsafe fn bytes_view<'a>(ptr: *const u8, len: i64) -> &'a [u8] {
    // Null / non-positive → empty (never `from_raw_parts(null, 0)`, which is UB). `usize::try_from`
    // rather than `len as usize` so a length that doesn't fit a `usize` (only reachable on a 32-bit
    // target) yields empty instead of a truncated, out-of-bounds view.
    match safe_len(len) {
        Ok(n) => unsafe { safe_slice(ptr, n as i64) },
        _ => &[],
    }
}

/// `path.base(p)` — the final path component as a **borrowed** sub-`str` `{ptr,len}` of `p` (no
/// allocation; aliases `p`'s bytes). Trailing `/` are stripped; an all-`/` path yields `/`; an empty
/// path yields an empty view. View-safe (always a substring of `p`).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_path_base(ptr: *const u8, len: i64) -> AlignStr {
    let b = unsafe { bytes_view(ptr, len) };
    if b.is_empty() {
        return AlignStr { ptr, len: 0 };
    }
    let mut end = b.len();
    while end > 0 && b[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        // All slashes → "/" (a 1-byte view of a separator).
        return str_subview(&b[0..1]);
    }
    let mut start = end;
    while start > 0 && b[start - 1] != b'/' {
        start -= 1;
    }
    str_subview(&b[start..end])
}

/// `path.dir(p)` — everything before the final component as a **borrowed** sub-`str` `{ptr,len}` of
/// `p`. Trailing separators are cleaned; a path with no separator yields an **empty** view (not `.`,
/// since the result must be a substring of `p`); separators that reach the root yield `/`. View-safe.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_path_dir(ptr: *const u8, len: i64) -> AlignStr {
    let b = unsafe { bytes_view(ptr, len) };
    if b.is_empty() {
        return AlignStr { ptr, len: 0 };
    }
    let mut end = b.len();
    while end > 0 && b[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        // All slashes → dir is "/".
        return str_subview(&b[0..1]);
    }
    let mut start = end;
    while start > 0 && b[start - 1] != b'/' {
        start -= 1;
    }
    if start == 0 {
        // No directory separator → empty view.
        return AlignStr { ptr, len: 0 };
    }
    // Strip the separator(s) before the base component.
    let mut dend = start;
    while dend > 0 && b[dend - 1] == b'/' {
        dend -= 1;
    }
    if dend == 0 {
        // The separators reach the root → "/".
        return str_subview(&b[0..1]);
    }
    str_subview(&b[0..dend])
}

/// `path.ext(p)` — the extension of the final component (from the last `.` to the end, including
/// the dot) as a **borrowed** sub-`str` `{ptr,len}` of `p`; empty when there is no `.`, or when the
/// only `.` starts the component (a dotfile like `.bashrc`). View-safe (always a suffix of the base).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_path_ext(ptr: *const u8, len: i64) -> AlignStr {
    let b = unsafe { bytes_view(ptr, len) };
    if b.is_empty() {
        return AlignStr { ptr, len: 0 };
    }
    let mut end = b.len();
    while end > 0 && b[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        return AlignStr { ptr, len: 0 };
    }
    let mut start = end;
    while start > 0 && b[start - 1] != b'/' {
        start -= 1;
    }
    let mut i = end;
    while i > start {
        i -= 1;
        if b[i] == b'.' {
            if i == start {
                // Leading dot → dotfile, no extension.
                return AlignStr { ptr, len: 0 };
            }
            return str_subview(&b[i..end]);
        }
    }
    AlignStr { ptr, len: 0 }
}

/// `path.join(a, b)` — join two fragments with a single `/` separator into a freshly heap-allocated
/// owned `string` `{ptr,len}`. An empty fragment yields a clone of the other; otherwise `a`'s
/// trailing `/` and `b`'s leading `/` are collapsed to exactly one separator.
///
/// # Safety
/// Each `{ptr,len}` pair must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_path_join(aptr: *const u8, alen: i64, bptr: *const u8, blen: i64) -> AlignStr {
    let a = unsafe { bytes_view(aptr, alen) };
    let b = unsafe { bytes_view(bptr, blen) };
    if a.is_empty() {
        return unsafe { align_rt_str_clone(bptr, blen) };
    }
    if b.is_empty() {
        return unsafe { align_rt_str_clone(aptr, alen) };
    }
    let mut ae = a.len();
    while ae > 0 && a[ae - 1] == b'/' {
        ae -= 1;
    }
    let mut bs = 0;
    while bs < b.len() && b[bs] == b'/' {
        bs += 1;
    }
    let asl = &a[0..ae];
    let bsl = &b[bs..];
    let total = asl.len() + 1 + bsl.len();
    let dst = align_rt_alloc(total as i64);
    unsafe {
        core::ptr::copy_nonoverlapping(asl.as_ptr(), dst, asl.len());
        *dst.add(asl.len()) = b'/';
        core::ptr::copy_nonoverlapping(bsl.as_ptr(), dst.add(asl.len() + 1), bsl.len());
    }
    AlignStr { ptr: dst, len: total as i64 }
}

/// `path.normalize(p)` — lexically resolve `.` / `..` / redundant `/` into a freshly heap-allocated
/// owned `string` `{ptr,len}`. Pure string manipulation (POSIX vocabulary only) — **no** symlink
/// resolution or filesystem access. A relative result that collapses to nothing is `.`; an absolute
/// one that collapses to nothing is `/`; a leading `..` is preserved on a relative path, and dropped
/// past the root on an absolute one.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_path_normalize(ptr: *const u8, len: i64) -> AlignStr {
    let b = unsafe { bytes_view(ptr, len) };
    let absolute = !b.is_empty() && b[0] == b'/';
    let mut comps: Vec<&[u8]> = Vec::new();
    for comp in b.split(|&c| c == b'/') {
        if comp.is_empty() || comp == b"." {
            continue;
        }
        if comp == b".." {
            if absolute {
                comps.pop(); // Can't go above the root; empty → no-op.
            } else if matches!(comps.last(), Some(&last) if last == b"..") || comps.is_empty() {
                comps.push(comp); // Preserve a leading `..` on a relative path.
            } else {
                comps.pop();
            }
        } else {
            comps.push(comp);
        }
    }
    let mut out: Vec<u8> = Vec::new();
    if absolute {
        out.push(b'/');
    }
    for (i, c) in comps.iter().enumerate() {
        if i > 0 {
            out.push(b'/');
        }
        out.extend_from_slice(c);
    }
    if out.is_empty() {
        out.push(b'.'); // A relative path that collapsed to nothing → ".".
    }
    let n = out.len();
    let dst = align_rt_alloc(n as i64);
    unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), dst, n) };
    AlignStr { ptr: dst, len: n as i64 }
}

/// `env.get(name)` — write the owned `string` `{ptr,len}` value of environment variable `name` into
/// `*out` (or `{null,0}` if unset / the name is invalid), returning `1` if set, `0` if not. The
/// value is copied out (owned) — the environment is volatile, so a view would dangle after a later
/// `env.set`. A present-but-empty value is `1` with a `{null,0}` (empty owned) string — distinct
/// from absent (`0`).
///
/// # Safety
/// `nptr`/`nlen` must describe a valid byte range, and `out` a valid `*mut AlignStr`, for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_env_get(nptr: *const u8, nlen: i64, out: *mut AlignStr) -> i32 {
    unsafe { *out = AlignStr { ptr: core::ptr::null(), len: 0 } };
    let name = unsafe { bytes_view(nptr, nlen) };
    // `getenv` needs a NUL-terminated name; an empty name or an interior NUL can never name a
    // variable, so treat it as absent.
    if name.is_empty() || name.contains(&0) {
        return 0;
    }
    let mut c = Vec::with_capacity(name.len() + 1);
    c.extend_from_slice(name);
    c.push(0);
    let v = unsafe { getenv(c.as_ptr()) };
    if v.is_null() {
        return 0;
    }
    // `strlen(v)`, then copy the bytes into an owned buffer immediately (before any later `setenv`
    // could invalidate the returned pointer).
    let mut n = 0usize;
    while unsafe { *v.add(n) } != 0 {
        n += 1;
    }
    unsafe { *out = align_rt_str_clone(v, n as i64) };
    1
}

/// `env.set(name, value)` — `setenv(name, value, overwrite=1)`. Returns `0` on success, else the
/// errno mapped through [`io_error_to_status`]. `name` must be non-empty and contain no `=` or NUL,
/// and `value` no NUL (POSIX `EINVAL` otherwise → `AL_INVALID`). **v1 concurrency:** `setenv` is not
/// thread-safe (POSIX), so a concurrent `env.set` from another `task_group` task is undefined; v1
/// documents this rather than serializing (no hidden global lock — "nothing hidden").
///
/// # Safety
/// Each `{ptr,len}` pair must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_env_set(nptr: *const u8, nlen: i64, vptr: *const u8, vlen: i64) -> i32 {
    let name = unsafe { bytes_view(nptr, nlen) };
    let value = unsafe { bytes_view(vptr, vlen) };
    if name.is_empty() || name.contains(&0) || name.contains(&b'=') || value.contains(&0) {
        return AL_INVALID;
    }
    let mut cn = Vec::with_capacity(name.len() + 1);
    cn.extend_from_slice(name);
    cn.push(0);
    let mut cv = Vec::with_capacity(value.len() + 1);
    cv.extend_from_slice(value);
    cv.push(0);
    let r = unsafe { setenv(cn.as_ptr(), cv.as_ptr(), 1) };
    if r == 0 {
        0
    } else {
        io_error_to_status(&std::io::Error::last_os_error())
    }
}

/// `time.now()` — wall-clock time as UNIX-epoch nanoseconds (`CLOCK_REALTIME` via `SystemTime`). A
/// clock set before the epoch yields a negative count. (`i128` ns is truncated to `i64` — good until
/// ~year 2262.)
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_time_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i64,
        Err(e) => -(e.duration().as_nanos() as i64),
    }
}

/// `time.instant()` — a monotonic-clock reading in nanoseconds (`CLOCK_MONOTONIC` via `Instant`),
/// measured from the first call in this process. Guaranteed non-decreasing (unlike `time.now`), so
/// it is the correct clock for measuring elapsed intervals.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_time_instant() -> i64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static BASE: OnceLock<Instant> = OnceLock::new();
    let base = BASE.get_or_init(Instant::now);
    base.elapsed().as_nanos() as i64
}

/// `time.sleep(ns)` — suspend the calling thread for `ns` nanoseconds. A non-positive `ns` is a
/// no-op. `std::thread::sleep` retries `EINTR` internally with the remaining time (POSIX
/// `nanosleep` resume semantics), so a signal never shortens the sleep.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_time_sleep(ns: i64) {
    if ns <= 0 {
        return;
    }
    std::thread::sleep(std::time::Duration::from_nanos(ns as u64));
}

/// Report an `Err` returned from `main` (`docs/impl/06-runtime-std.md` §9). M2's `Error`
/// is an i32 code; the original code is reported, and the returned value is the process
/// exit code — clamped to a nonzero `u8` so a failure never looks like success (exit 0)
/// and never wraps past the 8-bit Unix exit range. The eventual Error sum type will
/// carry a message/category here.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_report_error(code: i32) -> i32 {
    eprintln!("error: code {code}");
    code.clamp(1, 255)
}

/// Immediate abort called on arithmetic traps / invariant violations (`draft.md` §5).
/// Normally not called since overflow defaults to wrap.
pub fn panic_abort(msg: &str) -> ! {
    eprintln!("align: panic: {msg}");
    std::process::abort();
}

/// Out-of-bounds array index: report `index`/`len` and abort. Codegen emits the bounds check
/// (`0 <= index < len`) inline and calls this only on the failing path (the settled panic model:
/// a memory-safety violation in ordinary code is a hard error, never silent UB).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_bounds_fail(index: i64, len: i64) -> ! {
    eprintln!("align: panic: index out of bounds: the len is {len} but the index is {index}");
    std::process::abort();
}

/// `map_into` destination/source length mismatch: report both lengths and abort. `map_into`
/// writes each source element into the caller's `out`/`mut` slice, so the two must have the same
/// length; codegen emits the `dst.len() == src.len()` check inline and calls this on mismatch (the
/// settled panic model — a length violation is a hard error, never a silent partial/overrun write).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_len_mismatch_fail(dst_len: i64, src_len: i64) -> ! {
    eprintln!("align: panic: map_into length mismatch: the destination slice length is {dst_len} but the source length is {src_len}");
    std::process::abort();
}

/// Out-of-bounds range slice (`xs[start..end]`): report the whole `start..end` against `len` and
/// abort. Unlike an element index, a range has three ways to fail — a negative `start`, an inverted
/// `start > end`, or an over-length `end` — so all three values are reported together, avoiding the
/// `(index, len)` form's ambiguity (e.g. for an inverted range both bounds are individually valid).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_range_fail(start: i64, end: i64, len: i64) -> ! {
    eprintln!("align: panic: slice range out of bounds: {start}..{end} is not within length {len}");
    std::process::abort();
}

/// Integer division (or remainder) by zero: report and abort. Codegen emits the `divisor == 0`
/// check inline and calls this only on the failing path (the settled "division by zero is never
/// silent, always an error" decision — a raw `sdiv`/`udiv` by zero is LLVM UB, so it must be
/// guarded). The signed `INT_MIN / -1` overflow is handled inline (it wraps, matching defined
/// two's-complement overflow) and does not reach here.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_div_fail() -> ! {
    eprintln!("align: panic: division by zero");
    std::process::abort();
}

/// A bump allocator (`docs/impl/06-runtime-std.md` §3). Memory is carved from a list of
/// fixed-size chunks; individual allocations are never freed — the whole arena is
/// released at once by [`align_rt_arena_end`]. Chunk buffers are heap-stable (the outer
/// `Vec` growing never moves an inner buffer), so returned pointers stay valid until end.
pub struct Arena {
    chunks: Vec<Vec<u8>>,
    /// Byte offset into the last chunk.
    off: usize,
    /// `mmap` views registered by `fs.read_file_view` — `(addr, len)` pairs `munmap`ped in bulk when
    /// the arena ends/resets. Distinct from `chunks` (owned bump memory `free`d by dropping the
    /// `Vec`s): a mapping is kernel-owned and must be released with `munmap`, not `free`. Binding the
    /// mapping's lifetime to the arena (per the `draft.md` §18.2 region rule) is what guarantees a
    /// small returned `str` view cannot pin a huge mapping past the arena — release runs on *every*
    /// arena exit (block end, `return`, `?`), since they all lower to `ArenaEnd` → `align_rt_arena_end`.
    maps: Vec<(*mut core::ffi::c_void, usize)>,
}

impl Arena {
    /// Release every registered `mmap` view (`fs.read_file_view`). Called from both
    /// [`align_rt_arena_reset`] and [`align_rt_arena_end`] so a view is never leaked on any arena exit.
    fn unmap_all(&mut self) {
        for (addr, len) in self.maps.drain(..) {
            // `len` is the mapping length passed to `mmap`; `munmap` on a valid `(addr,len)` cannot
            // fail here (the pair came straight from a successful `mmap`), so the return is ignored.
            unsafe { munmap(addr, len) };
        }
    }
}

const CHUNK: usize = 64 * 1024;

impl Arena {
    fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        // The bit-trick below requires a power-of-two alignment; normalize so a future
        // ABI passing odd alignments stays correct.
        let align = align.max(1).next_power_of_two();
        let need = size.max(1);
        // Align against the chunk's *absolute* base address, not the chunk-relative
        // offset: a `Vec<u8>` buffer is only guaranteed 1-byte aligned, so a multiple of
        // `align` measured from the chunk start need not be an aligned address. Returning
        // an unaligned pointer is UB for the typed loads/stores codegen emits.
        let aligned_off = |base: usize, off: usize| -> usize {
            let addr = (base + off + align - 1) & !(align - 1);
            addr - base
        };
        if let Some(chunk) = self.chunks.last_mut() {
            let off = aligned_off(chunk.as_ptr() as usize, self.off);
            if off + need <= chunk.len() {
                let ptr = unsafe { chunk.as_mut_ptr().add(off) };
                self.off = off + need;
                return ptr;
            }
        }
        // A fresh chunk: size it so an aligned `need` always fits (+align worst case).
        self.chunks.push(vec![0u8; CHUNK.max(need + align)]);
        let chunk = self.chunks.last_mut().unwrap();
        let off = aligned_off(chunk.as_ptr() as usize, 0);
        let ptr = unsafe { chunk.as_mut_ptr().add(off) };
        self.off = off + need;
        ptr
    }
}

/// Open a new arena. The returned handle is freed by [`align_rt_arena_end`].
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_arena_begin() -> *mut Arena {
    Box::into_raw(Box::new(Arena { chunks: Vec::new(), off: 0, maps: Vec::new() }))
}

/// Bump-allocate `size` bytes (with `align`) from the arena.
///
/// # Safety
/// `arena` must be null or a valid pointer returned by [`align_rt_arena_begin`] and not yet ended.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_arena_alloc(arena: *mut Arena, size: i64, align: i64) -> *mut u8 {
    // Null-safe like every other runtime entry point: a null arena handle must not be dereferenced.
    if arena.is_null() {
        return core::ptr::null_mut();
    }
    // Validate `size`/`align` fit a `usize` before bump-allocating, matching every other runtime
    // FFI boundary: a raw `as usize` cast would turn a negative input into a huge value, so
    // `off + need` (`Arena::alloc`) could wrap in a release build. `align` must also be a nonzero
    // power of two (`is_power_of_two()` is false for 0, so this subsumes the nonzero check) —
    // `Arena::alloc`'s aligned-address bit-trick assumes it. Not reachable today (codegen always
    // passes a sound value), but guard it rather than trust the caller.
    let (Ok(size), Some(align)) =
        (safe_len(size), safe_len(align).ok().filter(|&a| a.is_power_of_two()))
    else {
        return core::ptr::null_mut();
    };
    let arena = unsafe { &mut *arena };
    arena.alloc(size, align)
}

/// Bulk-release every allocation, keeping the arena for reuse.
///
/// # Safety
/// `arena` must be a non-null, valid pointer returned by [`align_rt_arena_begin`] and not yet
/// ended; no allocation from it may still be in use afterward (they are all released).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_arena_reset(arena: *mut Arena) {
    let arena = unsafe { &mut *arena };
    arena.unmap_all();
    arena.chunks.clear();
    arena.off = 0;
}

/// Release every allocation and the arena itself.
///
/// # Safety
/// `arena` must be a non-null pointer returned by [`align_rt_arena_begin`] and not yet ended; this
/// call consumes it (frees the `Arena` object), so `arena` must not be used again afterward.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_arena_end(arena: *mut Arena) {
    let mut arena = unsafe { Box::from_raw(arena) };
    // Release every `fs.read_file_view` mapping before the arena's own memory is dropped. This is the
    // munmap path for *all* arena exits — the block end, an early `return`, and `?` all lower to
    // `ArenaEnd(handle)` (`align_mir::emit_exit_cleanup`), which codegen lowers to this call.
    arena.unmap_all();
    drop(arena);
}

// `task_group` runtime (slice ④b). A `TaskGroup` owns a region (arena) holding each spawned
// task's environment + result slot, plus the deferred task list. ④b-1 runs the tasks
// sequentially at `wait`; ④b-2 will spawn a thread per task and join at `wait` (the per-task
// trampoline, env, and slot are already heap-stable in the region, so that is the only change).
struct TgTask {
    /// `tramp(thunk, env, slot, err_slot) -> i32` — runs the spawned closure. On `Ok` it writes the
    /// result into `slot` and returns `0`; on `Err` it writes the full `Error` value into `err_slot`
    /// and returns `1` (surfaced by `wait()?`). A non-fallible task always returns `0`.
    tramp: extern "C" fn(*const u8, *mut u8, *mut u8, *mut u8) -> i32,
    /// The closure's function pointer (env-ABI `fn(env) -> R`), passed through to the trampoline.
    thunk: *const u8,
    /// The task's environment (capture snapshot) — a fresh region allocation per `spawn`.
    env: *mut u8,
    /// The task's result slot (a region allocation sized for `R`).
    slot: *mut u8,
    /// The task's error slot (a region allocation sized for `Error`), or null for a non-fallible
    /// task. `wait()?` reads the first errored task's `err_slot`.
    err_slot: *mut u8,
}

pub struct TaskGroup {
    arena: Arena,
    tasks: Vec<TgTask>,
}

/// Open a `task_group`. Freed by [`align_rt_tg_end`].
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_tg_begin() -> *mut TaskGroup {
    Box::into_raw(Box::new(TaskGroup { arena: Arena { chunks: Vec::new(), off: 0, maps: Vec::new() }, tasks: Vec::new() }))
}

/// Bump-allocate `size` bytes (with `align`) from the task group's region (envs + result slots).
///
/// # Safety
/// `tg` must be a non-null, valid pointer returned by [`align_rt_tg_begin`] and not yet ended.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_tg_alloc(tg: *mut TaskGroup, size: i64, align: i64) -> *mut u8 {
    if tg.is_null() {
        return core::ptr::null_mut();
    }
    let Ok(size_u) = safe_len(size) else { return core::ptr::null_mut() };
    let Ok(align_u) = safe_len(align) else { return core::ptr::null_mut() };
    unsafe { &mut *tg }.arena.alloc(size_u, align_u)
}

/// Register a deferred task (its trampoline + closure pointer + env + result slot).
///
/// # Safety
/// `tg` must be a non-null, valid pointer returned by [`align_rt_tg_begin`] and not yet ended.
/// `thunk`/`env`/`slot`/`err_slot` (`err_slot` may be null for a non-fallible task) must be valid
/// for `tramp` to read/write as documented on [`TgTask`], and must stay valid until the group's
/// `wait()` (they are read only during the task's run, before `tg_end` frees the region).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_tg_register(
    tg: *mut TaskGroup,
    tramp: extern "C" fn(*const u8, *mut u8, *mut u8, *mut u8) -> i32,
    thunk: *const u8,
    env: *mut u8,
    slot: *mut u8,
    err_slot: *mut u8,
) {
    unsafe { &mut *tg }.tasks.push(TgTask { tramp, thunk, env, slot, err_slot });
}

/// A task's data, made `Send` so it can move into a worker thread. Safe by construction (slice
/// ④b): each task's `env`/`slot` are a fresh, private region allocation — no task shares them, the
/// `env` is only read (its capture snapshot) and the `slot` only written, and the region outlives
/// the join (`wait` happens before `tg_end`). `get()` reads a slot only after the join (④c).
struct TgRun {
    tramp: extern "C" fn(*const u8, *mut u8, *mut u8, *mut u8) -> i32,
    thunk: *const u8,
    env: *mut u8,
    slot: *mut u8,
    err_slot: *mut u8,
}
unsafe impl Send for TgRun {}

/// A `task_group`'s registered tasks, shared across the runners of one `wait()`. Each index is
/// **claimed exactly once** (an atomic fetch-add hands each index to a single runner), and every
/// task's `env`/`slot`/`err_slot` is a fresh, private, disjoint region allocation that no other
/// task touches (`env` read-only, `slot`/`err_slot` write-only) — so concurrent immutable reads of
/// the list plus disjoint per-task writes are race-free. That is why this is `Send + Sync` despite
/// holding raw pointers.
struct TgTasks(Vec<TgTask>);
unsafe impl Send for TgTasks {}
unsafe impl Sync for TgTasks {}

/// The join barrier shared by every runner of one `wait()`: how many tasks have finished, the
/// first panic payload, and the first errored task's `err_slot`. "First" is by **lowest task
/// index** (deterministic, unlike thread-completion order), so a re-run gives the same error.
struct TgBarrier {
    /// Tasks that have completed (ran to a return or panicked). The caller waits for this to reach
    /// the task count before returning, so no task is still live when `tg_end` frees the region.
    done: usize,
    /// First panic (lowest index) — re-raised on the caller so a worker panic is never swallowed.
    panic: Option<(usize, Box<dyn std::any::Any + Send + 'static>)>,
    /// First errored task (lowest index): `(index, err_slot address)`. Stored as a `usize` address
    /// because a raw pointer is not `Send`; converted back to `*mut u8` on return.
    err: Option<(usize, usize)>,
}

/// Run every registered task and join them all before returning, reusing the process-lifetime
/// [`ParPool`] (like `align_rt_par_map`) instead of spawning one OS thread per task.
///
/// **Work-claiming, caller-participating design.** The tasks live in a shared claim-once list with
/// an atomic cursor. A *runner* loops: claim the next index, run that task (under `catch_unwind`),
/// record its outcome, repeat until the list is drained. `wait()` dispatches up to
/// `min(workers, n-1)` runners onto the pool **and runs the same claim loop on the calling thread
/// itself**, then blocks until every task has finished.
///
/// **Nesting / deadlock analysis (the crux).** A spawned closure is lifted to an ordinary function,
/// so its body may open its own `task_group` — i.e. a pool worker can re-enter `tg_wait`. With a
/// *finite* pool, a naive "submit all, then wait" scheme deadlocks: nested waits on busy workers
/// would wait for jobs that no free worker can pick up. The caller-participates claim loop removes
/// that hazard: **the calling thread of every `wait()` drains its own group to completion by
/// itself if no pool worker is free.** So each nesting level always makes forward progress on its
/// own thread — even with zero idle workers, an N-deep nest just runs sequentially, one level per
/// blocked thread. No `wait()` can ever wait on the pool for its *own* group's tasks. (This is why
/// the tasks are shared and claimed atomically rather than moved into per-task jobs.) Runner jobs
/// that a worker only picks up *after* the group has drained find the cursor past the end and exit
/// without touching any task — they never dereference the freed region.
///
/// A worker panic is caught, recorded, and re-raised on the caller (never swallowed — that would
/// falsely report success and then read an unwritten slot). All tasks are guaranteed finished
/// before this returns, so the region stays valid until `tg_end` (the join precedes the free).
///
/// # Safety
/// `tg` must be a non-null, valid pointer returned by [`align_rt_tg_begin`] and not yet ended.
/// Every task registered via [`align_rt_tg_register`] must still have its `env`/`slot`/`err_slot`
/// valid for the duration of this call (per its own safety contract).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_tg_wait(tg: *mut TaskGroup) -> *mut u8 {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    let tg = unsafe { &mut *tg };
    let tasks = std::mem::take(&mut tg.tasks);
    let n = tasks.len();
    if n == 0 {
        return std::ptr::null_mut();
    }

    let tasks = Arc::new(TgTasks(tasks));
    let cursor = Arc::new(AtomicUsize::new(0));
    let barrier: Arc<(Mutex<TgBarrier>, Condvar)> =
        Arc::new((Mutex::new(TgBarrier { done: 0, panic: None, err: None }), Condvar::new()));

    // One runner: claim indices until the list is drained, running each claimed task and recording
    // its outcome. Cloned per pool worker and also run on the caller. (Closures capturing only
    // `Clone` values — here three `Arc`s — are themselves `Clone` in edition 2021.)
    let run_all = {
        let tasks = tasks.clone();
        let cursor = cursor.clone();
        let barrier = barrier.clone();
        move || loop {
            let i = cursor.fetch_add(1, Ordering::Relaxed);
            if i >= n {
                break;
            }
            let t = &tasks.0[i];
            // Copy the raw fields into a `Send` unit so `catch_unwind`'s closure captures them as a
            // whole (edition-2021 disjoint capture would otherwise grab the non-`Send` raw fields).
            let run = TgRun { tramp: t.tramp, thunk: t.thunk, env: t.env, slot: t.slot, err_slot: t.err_slot };
            let es = t.err_slot as usize;
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let run = run;
                (run.tramp)(run.thunk, run.env, run.slot, run.err_slot)
            }));
            let (m, cv) = &*barrier;
            let mut st = m.lock().unwrap();
            st.done += 1;
            match res {
                Ok(errored) => {
                    if errored != 0 && st.err.is_none_or(|(j, _)| i < j) {
                        st.err = Some((i, es));
                    }
                }
                Err(p) => {
                    if st.panic.as_ref().is_none_or(|(j, _)| i < *j) {
                        st.panic = Some((i, p));
                    }
                }
            }
            cv.notify_all();
        }
    };

    let (pool, workers) = par_pool();
    // Dispatch helper runners onto the pool (bounded by the pool size and by `n-1` — the caller is
    // itself a runner), then run the claim loop on the caller. See the deadlock analysis above:
    // even if every submitted helper is starved by busy workers, the caller drains the group.
    for _ in 0..workers.min(n - 1) {
        pool.submit(Box::new(run_all.clone()));
    }
    run_all();

    // Block until every task has finished. The caller may have run them all itself (no worker was
    // free), or workers hold some — either way the region must not be freed until all are done.
    let (m, cv) = &*barrier;
    let mut st = m.lock().unwrap();
    while st.done < n {
        st = cv.wait(st).unwrap();
    }
    if let Some((_, p)) = st.panic.take() {
        drop(st);
        std::panic::resume_unwind(p);
    }
    st.err.map_or(std::ptr::null_mut(), |(_, addr)| addr as *mut u8)
}

/// Release the task group's region and the handle.
///
/// # Safety
/// `tg` must be null or a pointer returned by [`align_rt_tg_begin`] and not yet ended, and every
/// task must have already been joined (via [`align_rt_tg_wait`]) so none is still live. This call
/// consumes `tg` (frees the `TaskGroup` object), so `tg` must not be used again afterward.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_tg_end(tg: *mut TaskGroup) {
    if !tg.is_null() {
        drop(unsafe { Box::from_raw(tg) });
    }
}

// Free-standing heap allocation for owned collections (`array<T>` produced by `.to_array()`
// outside an arena). Backed by the C allocator so `free` needs no size/layout — the buffer
// may be over-allocated (map/where never grow) and is freed whole. `free(null)` is a no-op,
// so a never-initialised (null) owned slot drops harmlessly (MMv2 slice 4).
unsafe extern "C" {
    fn malloc(size: usize) -> *mut core::ffi::c_void;
    fn free(ptr: *mut core::ffi::c_void);
    // POSIX `write(2)` — a `writer` streams straight to its fd, bypassing the `std::io::Stdout`
    // lock + line-buffering that `print` pays per call.
    fn write(fd: i32, buf: *const core::ffi::c_void, count: usize) -> isize;
    // POSIX `read(2)` — a `reader` fills a caller-owned `buffer` straight from its fd.
    fn read(fd: i32, buf: *mut core::ffi::c_void, count: usize) -> isize;
    // POSIX `close(2)` — a file-backed `reader`/`writer` closes the fd it owns at `Drop`.
    fn close(fd: i32) -> i32;
    // POSIX `mmap(2)` / `munmap(2)` — `fs.read_file_view` maps a regular file read-only into the
    // enclosing arena's address space; the mapping is `munmap`ped when the arena ends.
    fn mmap(addr: *mut core::ffi::c_void, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut core::ffi::c_void;
    fn munmap(addr: *mut core::ffi::c_void, len: usize) -> i32;
    // POSIX `getenv(3)` / `setenv(3)` — `std.env`. `getenv` returns a pointer to the value's
    // NUL-terminated bytes (or null if unset); `setenv` returns 0 on success, -1 (with `errno` set)
    // on failure. Both are used only from the calling thread; concurrent `env.set` is documented
    // UB (POSIX — `setenv` is not thread-safe against `getenv`/`setenv` in other threads).
    fn getenv(name: *const u8) -> *const u8;
    fn setenv(name: *const u8, value: *const u8, overwrite: i32) -> i32;
    // OS CSPRNG seed for `rand.seed()`; never raw `RDRAND`/`RNDR` (outside the baseline, `SIGILL`
    // on older silicon — `docs/open-questions.md` #342). Per-OS symbol (the C entry differs):
    //   Linux — `getrandom(2)` (glibc ≥ 2.25 / musl): fills `buf` with `buflen` bytes, returns the
    //   byte count or -1 (with `errno`). `flags` = 0 (block until the pool is initialized, then
    //   never fails short of `EINTR`).
    #[cfg(target_os = "linux")]
    fn getrandom(buf: *mut core::ffi::c_void, buflen: usize, flags: u32) -> isize;
    //   macOS — `getentropy(2)`: fills `buf` with `buflen` (≤ 256) bytes, returns 0 on success or
    //   -1 (with `errno`). No `getrandom` symbol exists on macOS, so a bare `getrandom` extern would
    //   be a link error there.
    #[cfg(target_os = "macos")]
    fn getentropy(buf: *mut core::ffi::c_void, buflen: usize) -> i32;
}

// `mmap` protection / flags — the portable POSIX constants (identical on Linux and macOS).
const PROT_READ: i32 = 0x1;
const MAP_PRIVATE: i32 = 0x2;
/// `mmap` failure sentinel — `(void*)-1`, not null.
const MAP_FAILED: *mut core::ffi::c_void = usize::MAX as *mut core::ffi::c_void;

/// Allocate `size` bytes on the heap (C `malloc`). Returns null for `size <= 0` (an empty
/// buffer). On OOM (`malloc` returns null for a positive request) we fail fast and abort,
/// rather than hand back a null the generated code would dereference on the first store.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_alloc(size: i64) -> *mut u8 {
    if size <= 0 {
        return core::ptr::null_mut();
    }
    let ptr = unsafe { malloc(size as usize) as *mut u8 };
    if ptr.is_null() {
        panic_abort("out of memory");
    }
    ptr
}

/// Free a heap buffer from [`align_rt_alloc`]. Null-safe (a no-op), so dropping an owned
/// value whose slot was never initialised is harmless.
///
/// # Safety
/// `ptr` must be null or a pointer previously returned by [`align_rt_alloc`] and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_free(ptr: *mut u8) {
    unsafe { free(ptr as *mut core::ffi::c_void) }
}

// ── std.rand — Xoshiro256++ ────────────────────────────────────────────────────────────────────
//
// A non-cryptographic PRNG (`draft.md` §18.2). `rng` is a Copy 256-bit state (`[4 x i64]`); the
// generated code passes a pointer to that slot and the runtime advances it in place. Seeding is
// SplitMix64 (deterministic) or `getrandom` (OS). `range`/`shuffle`/`sample` draw bias-free indices
// via Lemire's nearly-divisionless bounded generator.

/// One Xoshiro256++ step: advance `s` and return the next 64-bit output.
#[inline]
fn xoshiro_next(s: &mut [u64; 4]) -> u64 {
    let result = (s[0].wrapping_add(s[3])).rotate_left(23).wrapping_add(s[0]);
    let t = s[1] << 17;
    s[2] ^= s[0];
    s[3] ^= s[1];
    s[1] ^= s[2];
    s[0] ^= s[3];
    s[2] ^= t;
    s[3] = s[3].rotate_left(45);
    result
}

/// Expand a 64-bit seed into a full 256-bit Xoshiro256++ state via SplitMix64 (the author-
/// recommended seeding). Guard the all-zero state (a fixed point for xoshiro) — SplitMix64 makes it
/// astronomically unlikely, but never rely on that.
fn splitmix64_state(seed: u64) -> [u64; 4] {
    let mut x = seed;
    let mut nextword = || {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let s = [nextword(), nextword(), nextword(), nextword()];
    if s == [0, 0, 0, 0] { [1, 0, 0, 0] } else { s }
}

/// A uniform `u64` in `[0, range)` (`range > 0`), bias-free via Lemire's nearly-divisionless method
/// (at most one rejection in the rare biased tail). Advances `s`.
fn bounded(s: &mut [u64; 4], range: u64) -> u64 {
    let mut m = (xoshiro_next(s) as u128) * (range as u128);
    let mut low = m as u64;
    if low < range {
        // Reject the `(2^64) mod range` biased low residues so every value in `[0, range)` is
        // equally likely; `range.wrapping_neg() % range` == `(2^64 - range) % range` == `2^64 % range`.
        let threshold = range.wrapping_neg() % range;
        while low < threshold {
            m = (xoshiro_next(s) as u128) * (range as u128);
            low = m as u64;
        }
    }
    (m >> 64) as u64
}

/// `rand.seed_with(s)` — deterministic seed. Writes the 256-bit state into `state` (a `[4 x i64]`).
///
/// # Safety
/// `state` must point to a writable `[u64; 4]` (the caller's `rng` slot).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_rng_seed_with(state: *mut u64, seed: i64) {
    let s = splitmix64_state(seed as u64);
    unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), state, 4) };
}

/// Fill `buf` with OS CSPRNG entropy — the per-OS seed source (Linux `getrandom`, macOS
/// `getentropy`). A failure is rare (the pool is initialized at boot) and **aborts** — seeding is
/// not a fallible user-facing operation. On a platform with neither symbol this is a hard abort at
/// runtime (the rest of `align_runtime` is Linux-only today anyway; the cfg keeps `rand` buildable).
fn fill_os_entropy(buf: &mut [u8; 32]) {
    #[cfg(target_os = "linux")]
    {
        // `getrandom(2)`: loop over short reads / `EINTR` until the 32 bytes are filled.
        let mut filled = 0usize;
        while filled < buf.len() {
            let n = unsafe {
                getrandom(buf.as_mut_ptr().add(filled) as *mut core::ffi::c_void, buf.len() - filled, 0)
            };
            if n < 0 {
                // A signal interrupted the fill → resume; any other error is unrecoverable.
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                panic_abort("rand.seed: getrandom failed");
            }
            if n == 0 {
                panic_abort("rand.seed: getrandom returned no bytes");
            }
            filled += n as usize;
        }
    }
    #[cfg(target_os = "macos")]
    {
        // `getentropy(2)` fills the whole buffer in one call (32 ≤ its 256-byte limit).
        let rc = unsafe { getentropy(buf.as_mut_ptr() as *mut core::ffi::c_void, buf.len()) };
        if rc != 0 {
            panic_abort("rand.seed: getentropy failed");
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = buf;
        panic_abort("rand.seed: OS seeding unsupported on this platform");
    }
}

/// `rand.seed()` — OS-seeded. Fills the 256-bit state from the OS CSPRNG (see [`fill_os_entropy`]).
/// A seed failure is rare and **aborts** rather than surface a `Result` — the settled design:
/// seeding is not a fallible user-facing operation.
///
/// # Safety
/// `state` must point to a writable `[u64; 4]` (the caller's `rng` slot).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_rng_seed_os(state: *mut u64) {
    let mut buf = [0u8; 32];
    fill_os_entropy(&mut buf);
    let mut s = [0u64; 4];
    for (i, word) in s.iter_mut().enumerate() {
        *word = u64::from_le_bytes(buf[i * 8..i * 8 + 8].try_into().unwrap());
    }
    if s == [0, 0, 0, 0] {
        s[0] = 1;
    }
    unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), state, 4) };
}

/// `r.next()` — advance the rng and return the next `i64`.
///
/// # Safety
/// `state` must point to a valid, seeded `[u64; 4]` (the caller's `rng` slot).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_rng_next(state: *mut u64) -> i64 {
    let s = unsafe { &mut *(state as *mut [u64; 4]) };
    xoshiro_next(s) as i64
}

/// `r.range(lo, hi)` — a uniform `i64` in `[lo, hi)` (bias-free). `lo >= hi` is a programmer error
/// (an empty range — nothing to draw) and aborts at runtime, like an out-of-bounds index.
///
/// # Safety
/// `state` must point to a valid, seeded `[u64; 4]`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_rng_range(state: *mut u64, lo: i64, hi: i64) -> i64 {
    if lo >= hi {
        eprintln!("align: panic: rand.range: empty range [{lo}, {hi}) — lo must be < hi");
        std::process::abort();
    }
    let s = unsafe { &mut *(state as *mut [u64; 4]) };
    // The width fits a `u64` (an i64 span is at most `2^64 - 1`); compute in `i128` to avoid the
    // `hi - lo` overflow when the span crosses zero (e.g. `i64::MIN`..`i64::MAX`).
    let width = (hi as i128 - lo as i128) as u64;
    let draw = bounded(s, width);
    (lo as i128 + draw as i128) as i64
}

/// `r.shuffle(out xs)` — Fisher-Yates shuffle the slice in place. `ptr`/`len` describe the slice;
/// `elem_size` is one element's byte width. Advances the rng. A `len <= 1` slice is left unchanged.
///
/// # Safety
/// `state` must point to a valid `[u64; 4]`; `ptr`/`len`/`elem_size` must describe a writable slice
/// of `len` elements of `elem_size` bytes each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_rng_shuffle(state: *mut u64, ptr: *mut u8, len: i64, elem_size: i64) {
    if len <= 1 || elem_size <= 0 || ptr.is_null() {
        return;
    }
    // `len`/`elem_size` describe an in-memory slice, so they fit `usize`; validate before any index
    // math (a truncating `as usize` on a 32-bit target would corrupt the offsets).
    let (Ok(n), Ok(es)) = (safe_len(len), safe_len(elem_size)) else {
        return;
    };
    let s = unsafe { &mut *(state as *mut [u64; 4]) };
    // i from n-1 down to 1: pick j uniformly in [0, i], swap elements i and j byte-wise.
    let mut i = n - 1;
    while i >= 1 {
        let j = bounded(s, (i + 1) as u64) as usize; // [0, i]
        if j != i {
            let a = unsafe { ptr.add(i * es) };
            let b = unsafe { ptr.add(j * es) };
            // Distinct indices → non-overlapping element ranges.
            unsafe { core::ptr::swap_nonoverlapping(a, b, es) };
        }
        i -= 1;
    }
}

/// `r.sample(xs, k)` — draw `k` elements of the slice (`src`/`src_len`, element size `elem_size`)
/// without replacement, into a fresh owned `array<T>` returned as `{ptr, len}` (buffer from
/// [`align_rt_alloc`], freed by the bound local's `Drop`). `k < 0` or `k > src_len` aborts — it is
/// impossible to draw that many distinct items. Advances the rng.
///
/// v1 uses a full `0..n` index permutation (O(n) scratch) partially shuffled to its first `k` —
/// correctness before speed; an O(k) Floyd's-sample is a later optimization behind this signature.
///
/// # Safety
/// `state` must point to a valid `[u64; 4]`; `src`/`src_len`/`elem_size` must describe a readable
/// slice of `src_len` elements of `elem_size` bytes each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_rng_sample(
    state: *mut u64,
    src: *const u8,
    src_len: i64,
    k: i64,
    elem_size: i64,
) -> AlignStr {
    if k < 0 || k > src_len {
        eprintln!("align: panic: rand.sample: cannot draw {k} distinct items from a slice of length {src_len}");
        std::process::abort();
    }
    if k == 0 || elem_size <= 0 || src.is_null() {
        // An empty draw owns no buffer (its `free(null)` drop is a no-op).
        return AlignStr { ptr: core::ptr::null(), len: 0 };
    }
    // All three describe / index an in-memory slice, so they fit `usize` (guard the truncating
    // `as usize` on a 32-bit target); `k` is already validated to `0..=src_len` above.
    let (Ok(es), Ok(n), Ok(kk)) = (safe_len(elem_size), safe_len(src_len), safe_len(k)) else {
        return AlignStr { ptr: core::ptr::null(), len: 0 };
    };
    let out_bytes = kk
        .checked_mul(es)
        .and_then(|b| i64::try_from(b).ok())
        .unwrap_or_else(|| panic_abort("rand.sample: output size overflow"));
    let out = align_rt_alloc(out_bytes); // kk > 0 → non-null (or aborts on OOM)
    let s = unsafe { &mut *(state as *mut [u64; 4]) };
    // Partial Fisher-Yates: select the first `kk` of a shuffled permutation of `0..n`, copying each
    // chosen source element into the output in draw order.
    let mut idx: Vec<usize> = (0..n).collect();
    for i in 0..kk {
        // Pick uniformly from the not-yet-selected suffix [i, n).
        let j = i + bounded(s, (n - i) as u64) as usize;
        idx.swap(i, j);
        let srcp = unsafe { src.add(idx[i] * es) };
        let dstp = unsafe { out.add(i * es) };
        unsafe { core::ptr::copy_nonoverlapping(srcp, dstp, es) };
    }
    AlignStr { ptr: out as *const u8, len: kk as i64 }
}

// ---------------------------------------------------------------------------------------------
// std.cli (M10 Slice 3) — a flag-registration parser over `main(args: array<str>)`'s `array<str>`
// (the one argv source). Pure in-language (no syscalls — argv is already captured). A `cli command`
// (`CliCommand`) is a Move handle owning its registered-flag table; `c.parse(args)` **borrows** it
// (so `c.usage()` stays callable after, including on the `Err` path) and yields an owned `cli parsed`
// (`CliParsed`) — the resolved name→value map. Both are `Box`ed and freed by the generated `Drop`.
// The three `get_*` are total after a successful parse: an unregistered name / wrong kind is a
// **programmer error** and aborts (like an OOB index), never a silent default. v1 argv grammar:
// `--name` (bool), `--name value`, `--name=value` (str/i64); `args[0]` is the program name, skipped.
// ---------------------------------------------------------------------------------------------

/// Which value a registered `std.cli` flag carries.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CliFlagKind {
    Bool,
    Str,
    I64,
}

/// A registered flag's default (a `bool` flag always defaults to `false`, so it carries no payload).
enum CliDefault {
    Bool,
    Str(String),
    I64(i64),
}

/// One registered flag: its `--name`, its kind, and its default.
struct CliFlag {
    name: String,
    kind: CliFlagKind,
    default: CliDefault,
}

/// A `cli command` — the flag-registration builder. Owns its flag table (each entry holds owned
/// `String`s), freed by [`align_rt_cli_command_free`].
pub struct CliCommand {
    name: String,
    flags: Vec<CliFlag>,
}

/// A resolved flag value after a successful parse.
enum CliValue {
    Bool(bool),
    Str(String),
    I64(i64),
}

/// A `cli parsed` — the resolved name→value map (one entry per registered flag, defaults filled in).
/// Owns its `String`s (`get_str` returns a zero-copy view into them), freed by
/// [`align_rt_cli_parsed_free`].
pub struct CliParsed {
    values: Vec<(String, CliValue)>,
}

/// Read a `str` `{ptr,len}` into an owned `String`. A `str` is UTF-8 by the language invariant;
/// `from_utf8_lossy` is used defensively (never aborts) so a non-UTF-8 view degrades rather than
/// crashing at registration time.
fn cli_str_owned(ptr: *const u8, len: i64) -> String {
    String::from_utf8_lossy(unsafe { bytes_view(ptr, len) }).into_owned()
}

/// Abort the process on a `get_*` programmer error (unregistered name, or a kind mismatch) — the
/// settled #345 policy: Align has no comptime, so a `get_*` cannot be checked against the runtime
/// flag set; it aborts like an OOB index, never a silent default. Mirrors [`align_rt_rng_range`]'s
/// `lo >= hi` abort.
fn cli_get_abort(what: &str, name: &[u8], detail: &str) -> ! {
    eprintln!("align: panic: cli.{what}: {detail} for flag '{}'", String::from_utf8_lossy(name));
    std::process::abort();
}

/// `cli.command(name)` — allocate a `cli command` handle named `name`.
///
/// # Safety
/// `name_ptr`/`name_len` must describe a valid byte range (or be `{null, <=0}`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_cli_command_new(name_ptr: *const u8, name_len: i64) -> *mut CliCommand {
    Box::into_raw(Box::new(CliCommand { name: cli_str_owned(name_ptr, name_len), flags: Vec::new() }))
}

/// `c.flag_bool(name)` — register a bool flag (default `false`). Null-safe on `cmd`.
///
/// # Safety
/// `cmd` must be a valid `CliCommand` (or null); `name_ptr`/`name_len` a valid byte range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_cli_flag_bool(cmd: *mut CliCommand, name_ptr: *const u8, name_len: i64) {
    if cmd.is_null() {
        return;
    }
    let c = unsafe { &mut *cmd };
    c.flags.push(CliFlag { name: cli_str_owned(name_ptr, name_len), kind: CliFlagKind::Bool, default: CliDefault::Bool });
}

/// `c.flag_str(name, default)` — register a `str` flag with a default. Null-safe on `cmd`.
///
/// # Safety
/// `cmd` must be a valid `CliCommand` (or null); the two byte ranges must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_cli_flag_str(cmd: *mut CliCommand, name_ptr: *const u8, name_len: i64, def_ptr: *const u8, def_len: i64) {
    if cmd.is_null() {
        return;
    }
    let c = unsafe { &mut *cmd };
    c.flags.push(CliFlag {
        name: cli_str_owned(name_ptr, name_len),
        kind: CliFlagKind::Str,
        default: CliDefault::Str(cli_str_owned(def_ptr, def_len)),
    });
}

/// `c.flag_i64(name, default)` — register an `i64` flag with a default. Null-safe on `cmd`.
///
/// # Safety
/// `cmd` must be a valid `CliCommand` (or null); `name_ptr`/`name_len` a valid byte range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_cli_flag_i64(cmd: *mut CliCommand, name_ptr: *const u8, name_len: i64, def: i64) {
    if cmd.is_null() {
        return;
    }
    let c = unsafe { &mut *cmd };
    c.flags.push(CliFlag { name: cli_str_owned(name_ptr, name_len), kind: CliFlagKind::I64, default: CliDefault::I64(def) });
}

/// Set (or replace) a resolved value in the parse accumulator, so a repeated `--flag` keeps the last
/// occurrence (the conventional last-wins rule).
fn cli_set_value(values: &mut Vec<(String, CliValue)>, name: &str, v: CliValue) {
    if let Some(slot) = values.iter_mut().find(|(nm, _)| nm == name) {
        slot.1 = v;
    } else {
        values.push((name.to_string(), v));
    }
}

/// Parse the value bytes `val` for the flag `f` into a [`CliValue`], or `None` on a malformed i64 /
/// non-UTF-8 str (→ `AL_INVALID`). A bool flag never reaches here (it takes no value).
fn cli_parse_value(f: &CliFlag, val: &[u8]) -> Option<CliValue> {
    let s = std::str::from_utf8(val).ok()?;
    match f.kind {
        CliFlagKind::Str => Some(CliValue::Str(s.to_string())),
        CliFlagKind::I64 => Some(CliValue::I64(s.parse::<i64>().ok()?)),
        CliFlagKind::Bool => None,
    }
}

/// `c.parse(args)` — tokenize the argv `array<str>` `{argv, argv_len}` (an `AlignStr` buffer) against
/// `cmd`'s flag table. `args[0]` is the program name (skipped). Writes an owned `cli parsed` handle to
/// `*out` and returns `0`, or `AL_INVALID` (`Error.Invalid`) — leaving `*out` null — on any input
/// error (unknown flag, missing value, malformed i64, wrong kind).
///
/// # Safety
/// `cmd` must be a valid `CliCommand` (or null); `argv`/`argv_len` must describe a valid `AlignStr`
/// buffer (each entry a valid `str` view); `out` must point to a writable handle slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_cli_parse(cmd: *mut CliCommand, argv: *const AlignStr, argv_len: i64, out: *mut *mut CliParsed) -> i32 {
    if out.is_null() {
        return AL_INVALID;
    }
    unsafe { *out = core::ptr::null_mut() };
    if cmd.is_null() {
        return AL_INVALID;
    }
    let c = unsafe { &*cmd };
    let Ok(n) = safe_len(argv_len) else { return 1 };
    let argv_slice: &[AlignStr] = unsafe { safe_slice(argv, n as i64) };
    let mut values: Vec<(String, CliValue)> = Vec::new();

    // Skip `argv[0]` (the program name — the `main(args)` convention).
    let mut i = 1usize;
    while i < argv_slice.len() {
        let tok = unsafe { bytes_view(argv_slice[i].ptr, argv_slice[i].len) };
        // v1 grammar: every token is a `--flag` form. A bare / short token is rejected.
        if tok.len() < 2 || &tok[..2] != b"--" {
            return AL_INVALID;
        }
        let body = &tok[2..];
        if let Some(eq) = body.iter().position(|&b| b == b'=') {
            // `--name=value` (str/i64 only — a bool takes no value).
            let name = &body[..eq];
            let val = &body[eq + 1..];
            let Some(f) = c.flags.iter().find(|f| f.name.as_bytes() == name) else {
                return AL_INVALID; // unknown flag
            };
            if f.kind == CliFlagKind::Bool {
                return AL_INVALID; // a bool flag takes no `=value`
            }
            let Some(v) = cli_parse_value(f, val) else {
                return AL_INVALID; // malformed i64 / non-UTF-8
            };
            cli_set_value(&mut values, &f.name, v);
            i += 1;
        } else {
            // `--name` (bool) or `--name value` (str/i64).
            let Some(f) = c.flags.iter().find(|f| f.name.as_bytes() == body) else {
                return AL_INVALID; // unknown flag
            };
            match f.kind {
                CliFlagKind::Bool => {
                    cli_set_value(&mut values, &f.name, CliValue::Bool(true));
                    i += 1;
                }
                CliFlagKind::Str | CliFlagKind::I64 => {
                    // The value is the next token.
                    if i + 1 >= argv_slice.len() {
                        return AL_INVALID; // missing value
                    }
                    let val = unsafe { bytes_view(argv_slice[i + 1].ptr, argv_slice[i + 1].len) };
                    let Some(v) = cli_parse_value(f, val) else {
                        return AL_INVALID;
                    };
                    cli_set_value(&mut values, &f.name, v);
                    i += 2;
                }
            }
        }
    }

    // Fill in the default for every registered flag not seen on the command line.
    for f in &c.flags {
        if values.iter().any(|(nm, _)| nm.as_bytes() == f.name.as_bytes()) {
            continue;
        }
        let v = match &f.default {
            CliDefault::Bool => CliValue::Bool(false),
            CliDefault::Str(s) => CliValue::Str(s.clone()),
            CliDefault::I64(x) => CliValue::I64(*x),
        };
        values.push((f.name.clone(), v));
    }

    unsafe { *out = Box::into_raw(Box::new(CliParsed { values })) };
    0
}

/// Look up flag `name` in a parsed handle, aborting (programmer error) if `parsed` is null or the
/// name was never registered. Returns the resolved value.
///
/// # Safety
/// `parsed` must be a valid `CliParsed` (or null); `name_ptr`/`name_len` a valid byte range.
unsafe fn cli_lookup<'a>(parsed: *const CliParsed, what: &str, name_ptr: *const u8, name_len: i64) -> (&'a CliValue, &'a [u8]) {
    let name = unsafe { bytes_view(name_ptr, name_len) };
    if parsed.is_null() {
        cli_get_abort(what, name, "the parsed result is null");
    }
    let p = unsafe { &*parsed };
    match p.values.iter().find(|(nm, _)| nm.as_bytes() == name) {
        Some((_, v)) => (v, name),
        None => cli_get_abort(what, name, "no such flag was registered"),
    }
}

/// `p.get_bool(name)` — `1`/`0`. Aborts on unregistered / wrong-kind.
///
/// # Safety
/// See [`cli_lookup`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_cli_get_bool(parsed: *const CliParsed, name_ptr: *const u8, name_len: i64) -> i32 {
    let (v, name) = unsafe { cli_lookup(parsed, "get_bool", name_ptr, name_len) };
    match v {
        CliValue::Bool(b) => *b as i32,
        _ => cli_get_abort("get_bool", name, "flag is not a bool"),
    }
}

/// `p.get_i64(name)`. Aborts on unregistered / wrong-kind.
///
/// # Safety
/// See [`cli_lookup`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_cli_get_i64(parsed: *const CliParsed, name_ptr: *const u8, name_len: i64) -> i64 {
    let (v, name) = unsafe { cli_lookup(parsed, "get_i64", name_ptr, name_len) };
    match v {
        CliValue::I64(x) => *x,
        _ => cli_get_abort("get_i64", name, "flag is not an i64"),
    }
}

/// `p.get_str(name)` — a `str` **view** into the parsed handle's owned storage (no copy;
/// region-bound to `parsed` in sema). Aborts on unregistered / wrong-kind.
///
/// # Safety
/// See [`cli_lookup`]. The returned view borrows `parsed`, which must outlive it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_cli_get_str(parsed: *const CliParsed, name_ptr: *const u8, name_len: i64) -> AlignStr {
    let (v, name) = unsafe { cli_lookup(parsed, "get_str", name_ptr, name_len) };
    match v {
        CliValue::Str(s) => AlignStr { ptr: s.as_ptr(), len: s.len() as i64 },
        _ => cli_get_abort("get_str", name, "flag is not a str"),
    }
}

/// `c.usage()` — render `cmd`'s flag table into a fresh owned `string` `{ptr,len}` (freed by the
/// bound local's `Drop` via `align_rt_free`). Null-`cmd` yields an empty string.
///
/// # Safety
/// `cmd` must be a valid `CliCommand` (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_cli_usage(cmd: *const CliCommand) -> AlignStr {
    if cmd.is_null() {
        return AlignStr { ptr: core::ptr::null(), len: 0 };
    }
    let c = unsafe { &*cmd };
    let mut s = String::new();
    s.push_str("usage: ");
    s.push_str(&c.name);
    s.push_str(" [flags]\n");
    for f in &c.flags {
        s.push_str("  --");
        s.push_str(&f.name);
        match &f.default {
            CliDefault::Bool => s.push_str("  (bool)\n"),
            CliDefault::Str(d) => {
                s.push_str("  (str, default: ");
                s.push_str(d);
                s.push_str(")\n");
            }
            CliDefault::I64(d) => {
                s.push_str("  (i64, default: ");
                s.push_str(&d.to_string());
                s.push_str(")\n");
            }
        }
    }
    owned_str_from_vec(s.as_bytes())
}

/// Free a `cli command` (its flag table). Null-safe.
///
/// # Safety
/// `cmd` must be null or a pointer from [`align_rt_cli_command_new`], not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_cli_command_free(cmd: *mut CliCommand) {
    if !cmd.is_null() {
        drop(unsafe { Box::from_raw(cmd) });
    }
}

/// Free a `cli parsed` (its value map). Null-safe.
///
/// # Safety
/// `parsed` must be null or a pointer from [`align_rt_cli_parse`], not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_cli_parsed_free(parsed: *mut CliParsed) {
    if !parsed.is_null() {
        drop(unsafe { Box::from_raw(parsed) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boundary lengths around the 4/8/16/48-byte branch cuts must not panic and must differ.
    #[test]
    fn align_rt_hash64_boundaries_and_determinism() {
        let mk = |n: usize| -> Vec<u8> { (0..n).map(|i| i as u8).collect() };
        let mut seen = std::collections::HashSet::new();
        for n in [0usize, 1, 3, 4, 7, 8, 15, 16, 17, 47, 48, 49, 96, 200] {
            let v = mk(n);
            let h = unsafe { align_rt_hash64(v.as_ptr(), v.len() as i64) };
            // determinism: same bytes → same hash
            assert_eq!(h, unsafe { align_rt_hash64(v.as_ptr(), v.len() as i64) });
            assert!(seen.insert(h), "unexpected collision at len {n}");
        }
        // null / non-positive len is the empty hash, no UB.
        assert_eq!(unsafe { align_rt_hash64(std::ptr::null(), 0) }, wyhash(b"", WY_SEED));
        assert_eq!(unsafe { align_rt_hash64(std::ptr::null(), -5) }, wyhash(b"", WY_SEED));
    }

    #[test]
    fn buffered_writer_accumulates_small_writes_without_flushing() {
        // Small writes stay buffered (no syscall, nothing reaches the fd): the buffer holds exactly
        // the concatenated bytes, and the writer records its target fd. The flush and large-chunk
        // pass-through paths are covered end-to-end (they necessarily touch a real fd). fd 2
        // (stderr) is used so the buffered bytes, if ever flushed, don't pollute the harness stdout.
        let w = align_rt_io_writer_std(2, 1);
        for part in [&b"hello "[..], b"world", b"!"] {
            assert_eq!(unsafe { align_rt_io_writer_write(w, part.as_ptr(), part.len() as i64) }, 0);
        }
        {
            let wr = unsafe { &mut *w };
            assert_eq!(wr.fd, 2, "writer targets the fd it was constructed with");
            assert_eq!(wr.buf, b"hello world!", "small writes accumulate, unflushed");
            wr.buf.clear(); // so the drop-flush below emits nothing
        }
        unsafe { align_rt_io_writer_free(w) };
    }

    #[test]
    fn errno_table_maps_categories_and_passes_through_codes() {
        // The one fixed errno→status table (`draft.md` §18.2). `ErrorKind` drives the three
        // categories portably; anything else carries its raw errno as `Code`.
        use std::io::{Error, ErrorKind};
        assert_eq!(io_error_to_status(&Error::from(ErrorKind::NotFound)), AL_NOT_FOUND);
        assert_eq!(io_error_to_status(&Error::from(ErrorKind::PermissionDenied)), AL_DENIED);
        assert_eq!(io_error_to_status(&Error::from(ErrorKind::InvalidInput)), AL_INVALID);
        // A raw errno with no dedicated `ErrorKind` (EIO = 5) passes through as `Code`, encoded
        // above the category sentinels so it can never look like one.
        assert_eq!(io_error_to_status(&Error::from_raw_os_error(5)), AL_CODE + 5);
    }

    #[test]
    fn reader_read_fills_buffer_and_reports_eof() {
        // A `buffer` over a temp file: the first read fills up to capacity, the second hits EOF (0).
        use std::io::Write;
        let mut path = std::env::temp_dir();
        path.push(format!("align_rt_reader_test_{}", std::process::id()));
        std::fs::File::create(&path).unwrap().write_all(b"hello").unwrap();
        let path_bytes = path.to_str().unwrap().as_bytes();

        let mut r: *mut Reader = std::ptr::null_mut();
        assert_eq!(unsafe { align_rt_io_reader_open(path_bytes.as_ptr(), path_bytes.len() as i64, &mut r) }, 0);
        let b = align_rt_buffer_new(3);
        // First read: up to 3 bytes ("hel").
        assert_eq!(unsafe { align_rt_io_reader_read(r, b) }, 3);
        assert_eq!(unsafe { align_rt_buffer_len(b) }, 3);
        let mut view = AlignStr { ptr: std::ptr::null(), len: 0 };
        unsafe { align_rt_buffer_bytes(b, &mut view) };
        assert_eq!(unsafe { safe_slice(view.ptr, view.len) }, b"hel");
        // Second read: "lo". Third: EOF.
        assert_eq!(unsafe { align_rt_io_reader_read(r, b) }, 2);
        assert_eq!(unsafe { align_rt_io_reader_read(r, b) }, 0);

        unsafe { align_rt_buffer_free(b) };
        unsafe { align_rt_io_reader_free(r) };
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn buffer_huge_capacity_degrades_to_empty_window_not_abort() {
        // A pathological capacity must fail softly (an empty read window), never abort the process
        // on an infallible allocation. `read` into it then yields 0 (nothing to fill).
        let b = align_rt_buffer_new(i64::MAX);
        let bref = unsafe { &*b };
        assert_eq!(bref.cap, 0, "an unreservable capacity degrades to a 0-byte window");
        assert_eq!(unsafe { align_rt_buffer_len(b) }, 0);
        unsafe { align_rt_buffer_free(b) };
        // A negative capacity is also an empty window (never a wrapping `as usize`).
        let b2 = align_rt_buffer_new(-5);
        assert_eq!(unsafe { &*b2 }.cap, 0);
        unsafe { align_rt_buffer_free(b2) };
    }

    // --- std.encoding (M10 Slice 1) ----------------------------------------------------------

    /// Encode via the internal encoder (the FFI shape is a thin `owned_str_from_vec` wrapper).
    fn b64_enc(data: &[u8], url: bool) -> Vec<u8> {
        let mut out = Vec::new();
        base64_encode_into(data, if url { &BASE64_URL } else { &BASE64_STD }, !url, &mut out);
        out
    }

    fn hex_enc(data: &[u8]) -> Vec<u8> {
        const HEX: [u8; 16] = *b"0123456789abcdef";
        let mut out = Vec::new();
        for &b in data {
            out.push(HEX[(b >> 4) as usize]);
            out.push(HEX[(b & 15) as usize]);
        }
        out
    }

    /// A hex decode mirroring the FFI path (odd length / non-hex byte -> `None`).
    fn hex_dec(input: &[u8]) -> Option<Vec<u8>> {
        if input.len() % 2 != 0 {
            return None;
        }
        let mut v = Vec::new();
        let mut i = 0;
        while i < input.len() {
            v.push(hex_val(input[i])? << 4 | hex_val(input[i + 1])?);
            i += 2;
        }
        Some(v)
    }

    #[test]
    fn base64_known_vectors() {
        // RFC 4648 §10 test vectors (standard alphabet + padding).
        for (input, want) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(b64_enc(input, false), want.as_bytes(), "encode {input:?}");
            assert_eq!(base64_decode_impl(want.as_bytes(), false).as_deref(), Some(input), "decode {want}");
        }
    }

    #[test]
    fn base64url_known_vectors_no_padding() {
        // URL-safe alphabet, no padding; `0xfbf0`/`0xffffff` exercise the `-`/`_` (62/63) symbols.
        for (input, want) in [(&[0xfb, 0xf0][..], "-_A"), (&[0xff, 0xff, 0xff][..], "____")] {
            assert_eq!(b64_enc(input, true), want.as_bytes(), "url encode {input:?}");
            assert_eq!(base64_decode_impl(want.as_bytes(), true).as_deref(), Some(input), "url decode {want}");
        }
        // The two alphabets are strict: a URL decode rejects `+`/`/`, a standard decode rejects `-`/`_`.
        assert_eq!(base64_decode_impl(b"+/A=", true), None);
        assert_eq!(base64_decode_impl(b"-_A", false), None);
    }

    #[test]
    fn encodings_round_trip_all_boundaries_and_binary() {
        // Empty, every 1/2/3-byte prefix boundary, and every single byte value 0..=255.
        let mut cases: Vec<Vec<u8>> = vec![vec![], vec![0], vec![0, 255], vec![1, 2, 3]];
        for b in 0u16..=255 {
            cases.push(vec![b as u8]);
        }
        // A full 0..=255 binary blob (all byte values, non-block-aligned length 256 -> 0 mod 3? 256
        // % 3 = 1, so it exercises the 1-byte residue too).
        cases.push((0u16..=255).map(|b| b as u8).collect());
        for data in &cases {
            for url in [false, true] {
                let enc = b64_enc(data, url);
                assert_eq!(base64_decode_impl(&enc, url).as_ref(), Some(data), "base64 round trip {data:?} url={url}");
            }
            let hx = hex_enc(data);
            assert_eq!(hex_dec(&hx).as_ref(), Some(data), "hex round trip {data:?}");
        }
    }

    #[test]
    fn base64_decode_rejects_invalid() {
        assert_eq!(base64_decode_impl(b"Zm9v!mFy", false), None, "bad symbol");
        assert_eq!(base64_decode_impl(b"Zm9vY", false), None, "residue-1 length");
        assert_eq!(base64_decode_impl(b"Zg=", false), None, "inconsistent padding (single = on residue 2)");
        assert_eq!(base64_decode_impl(b"Z===", false), None, "too much padding");
        assert_eq!(base64_decode_impl(b"Zm=9", false), None, "mid-string padding");
        // Non-canonical trailing bits: "Zh" carries a nonzero remainder ('h' = 33, low bits set).
        assert_eq!(base64_decode_impl(b"Zh", false), None, "non-canonical trailing bits");
        // The canonical residue-2/residue-3 forms decode fine (unpadded accepted).
        assert_eq!(base64_decode_impl(b"Zg", false).as_deref(), Some(&b"f"[..]));
        assert_eq!(base64_decode_impl(b"Zm8", false).as_deref(), Some(&b"fo"[..]));
    }

    #[test]
    fn hex_decode_rejects_invalid() {
        assert_eq!(hex_dec(b"abc"), None, "odd length");
        assert_eq!(hex_dec(b"zz"), None, "non-hex");
        assert_eq!(hex_dec(b"666F6F626172").as_deref(), Some(&b"foobar"[..]), "upper-case accepted");
        assert_eq!(hex_dec(b"666f6f626172").as_deref(), Some(&b"foobar"[..]), "lower-case accepted");
    }

    #[test]
    fn ffi_encode_returns_owned_string_and_decode_returns_buffer() {
        // FFI encode: a heap-owned `{ptr,len}` string, freed like any owned string.
        let s = unsafe { align_rt_base64_encode(b"foobar".as_ptr(), 6) };
        assert_eq!(unsafe { safe_slice(s.ptr, s.len) }, b"Zm9vYmFy");
        unsafe { align_rt_free(s.ptr as *mut u8) };
        // FFI decode: an owned `buffer` handle whose `.bytes()` is the decoded blob.
        let input = b"Zm9vYmFy";
        let mut buf: *mut Buffer = std::ptr::null_mut();
        assert_eq!(unsafe { align_rt_base64_decode(input.as_ptr(), input.len() as i64, &mut buf) }, 0);
        assert!(!buf.is_null());
        assert_eq!(unsafe { align_rt_buffer_len(buf) }, 6);
        let mut view = AlignStr { ptr: std::ptr::null(), len: 0 };
        unsafe { align_rt_buffer_bytes(buf, &mut view) };
        assert_eq!(unsafe { safe_slice(view.ptr, view.len) }, b"foobar");
        unsafe { align_rt_buffer_free(buf) };
        // FFI decode failure: null handle + AL_INVALID.
        let bad = b"Zm9v!mFy";
        let mut buf2: *mut Buffer = std::ptr::null_mut();
        assert_eq!(unsafe { align_rt_base64_decode(bad.as_ptr(), bad.len() as i64, &mut buf2) }, AL_INVALID);
        assert!(buf2.is_null(), "a failed decode leaves the out slot null");
    }

    #[test]
    fn ffi_utf8_valid_matches_validator() {
        let valid = b"Hello, world";
        assert_eq!(unsafe { align_rt_utf8_valid(valid.as_ptr(), valid.len() as i64) }, 1);
        // A lone 0xff is never a valid UTF-8 lead byte; a truncated 0xc3 sequence is incomplete.
        for bad in [&[0xffu8][..], &[0xc3][..], &[0x80]] {
            assert_eq!(unsafe { align_rt_utf8_valid(bad.as_ptr(), bad.len() as i64) }, 0, "invalid {bad:02x?}");
        }
        // Empty is valid; matches `std::str::from_utf8`.
        assert_eq!(unsafe { align_rt_utf8_valid(std::ptr::null(), 0) }, 1);
    }

    #[test]
    fn reader_open_missing_file_maps_to_not_found() {
        let path = b"/nonexistent/align/rt/path/xyzzy";
        let mut r: *mut Reader = std::ptr::null_mut();
        let s = unsafe { align_rt_io_reader_open(path.as_ptr(), path.len() as i64, &mut r) };
        assert_eq!(s, AL_NOT_FOUND);
        assert!(r.is_null(), "a failed open leaves the out handle null");
    }

    #[test]
    fn io_copy_is_byte_exact_and_does_not_consume_the_handles() {
        use std::io::Write;
        // A payload larger than the transfer buffer so the copy loop runs many times over a final
        // partial chunk.
        let content: Vec<u8> = (0..(BUF_WRITER_CAP * 2 + 777)).map(|i| (i * 37 + 5) as u8).collect();
        let mut src = std::env::temp_dir();
        src.push(format!("align_rt_iocopy_src_{}", std::process::id()));
        let mut dst = std::env::temp_dir();
        dst.push(format!("align_rt_iocopy_dst_{}", std::process::id()));
        std::fs::File::create(&src).unwrap().write_all(&content).unwrap();

        let src_bytes = src.to_str().unwrap().as_bytes();
        let dst_bytes = dst.to_str().unwrap().as_bytes();
        let mut r: *mut Reader = std::ptr::null_mut();
        let mut w: *mut Writer = std::ptr::null_mut();
        assert_eq!(unsafe { align_rt_io_reader_open(src_bytes.as_ptr(), src_bytes.len() as i64, &mut r) }, 0);
        assert_eq!(unsafe { align_rt_io_writer_create(dst_bytes.as_ptr(), dst_bytes.len() as i64, &mut w) }, 0);

        let n = unsafe { align_rt_io_copy(r, w) };
        assert_eq!(n, content.len() as i64, "io.copy returns the transferred byte count");

        // Non-consumption: both handles are still valid after the copy. The reader is now at EOF;
        // the writer can still append.
        let b = align_rt_buffer_new(16);
        assert_eq!(unsafe { align_rt_io_reader_read(r, b) }, 0, "the borrowed reader is at EOF, still usable");
        assert_eq!(unsafe { align_rt_io_writer_write(w, b"!".as_ptr(), 1) }, 0, "the borrowed writer still writes");

        unsafe { align_rt_buffer_free(b) };
        unsafe { align_rt_io_reader_free(r) };
        unsafe { align_rt_io_writer_free(w) }; // flush + close

        let mut expected = content.clone();
        expected.push(b'!');
        assert_eq!(std::fs::read(&dst).unwrap(), expected, "the copy is byte-exact (plus the appended byte)");
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    #[test]
    fn io_copy_null_handles_are_invalid_not_a_crash() {
        assert_eq!(unsafe { align_rt_io_copy(std::ptr::null_mut(), std::ptr::null_mut()) }, -(AL_INVALID as i64));
    }

    #[test]
    fn write_json_str_bulk_copy_matches_byte_by_byte_reference() {
        // The old per-byte implementation, used as the oracle: the bulk-copy rewrite must produce
        // byte-identical output for every input.
        fn reference(s: &[u8]) -> Vec<u8> {
            let mut out = vec![b'"'];
            for &c in s {
                match c {
                    b'"' => out.extend_from_slice(b"\\\""),
                    b'\\' => out.extend_from_slice(b"\\\\"),
                    0x08 => out.extend_from_slice(b"\\b"),
                    0x0c => out.extend_from_slice(b"\\f"),
                    b'\n' => out.extend_from_slice(b"\\n"),
                    b'\r' => out.extend_from_slice(b"\\r"),
                    b'\t' => out.extend_from_slice(b"\\t"),
                    c if c < 0x20 => {
                        const HEX: &[u8; 16] = b"0123456789abcdef";
                        out.extend_from_slice(b"\\u00");
                        out.push(HEX[(c >> 4) as usize]);
                        out.push(HEX[(c & 0xf) as usize]);
                    }
                    c => out.push(c),
                }
            }
            out.push(b'"');
            out
        }
        let encode = |s: &[u8]| -> Vec<u8> {
            let mut b = Builder { buf: Vec::new(), arena: core::ptr::null_mut() };
            unsafe { align_rt_builder_write_json_str(&mut b, s.as_ptr(), s.len() as i64) };
            b.buf
        };

        let mut cases: Vec<Vec<u8>> = vec![
            b"".to_vec(),
            b"plain ascii text".to_vec(),
            b"with \"quotes\" and \\ backslash".to_vec(),
            b"tabs\tnewlines\nand\rreturns".to_vec(),
            "UTF-8: \u{e9} \u{672c} \u{1f600} mixed".as_bytes().to_vec(),
            b"trailing quote\"".to_vec(),
            b"\"leading quote".to_vec(),
            vec![b'a'; 1000], // long clean run
        ];
        // Every C0 control byte (0x00..=0x1f), each surrounded by clean bytes.
        for c in 0u8..0x20 {
            cases.push(vec![b'x', c, b'y']);
        }
        // One string containing all control bytes in a row.
        cases.push((0u8..0x20).collect());

        for s in &cases {
            assert_eq!(encode(s), reference(s), "mismatch encoding {s:?}");
        }
    }

    #[test]
    fn find_quote_or_escape_prefix_and_simd_paths_agree() {
        // A trivial scalar reference: index of the first `"` or `\`, else None.
        let reference = |h: &[u8]| h.iter().position(|&c| c == b'"' || c == b'\\');

        // Cover the scalar prefix (< 16), the prefix boundary, and well past it (the memchr path),
        // with the delimiter at the start, middle, end, and absent — and both `"` and `\`.
        let bodies: &[&[u8]] = &[
            b"",
            b"a",
            b"short",
            b"created_at", // 10B: pure prefix scan, no match
            b"0123456789abcde",  // 15B, no match (just under PREFIX)
            b"0123456789abcdef",  // 16B == PREFIX, no match
            b"0123456789abcdefg", // 17B, no match → memchr tail returns None
            b"0123456789abcdef\"",  // delimiter exactly past the prefix → memchr finds it
            b"0123456789abcde\"",   // delimiter at the prefix's last byte (scalar)
            b"a long clean run of bytes with the quote way out here -> \" <- there",
            b"escape in the tail 0123456789abcdef\\xyz",
            b"\"", // immediate quote
            b"\\", // immediate backslash
        ];
        for h in bodies {
            assert_eq!(find_quote_or_escape(h), reference(h), "mismatch on {h:?}");
        }

        // A long (> prefix) all-clean body: None, and a long body with a deep delimiter: exact index.
        let long_clean = vec![b'x'; 10_000];
        assert_eq!(find_quote_or_escape(&long_clean), None);
        let mut long_hit = vec![b'y'; 5000];
        long_hit.push(b'"');
        long_hit.extend_from_slice(&[b'z'; 100]);
        assert_eq!(find_quote_or_escape(&long_hit), Some(5000));
    }

    #[test]
    fn group_sum_i64_aggregates_by_key() {
        // keys: 10,20,10,30,20,10 ; vals: 1,2,3,4,5,6 → {10:1+3+6=10, 20:2+5=7, 30:4}
        let keys = [10i64, 20, 10, 30, 20, 10];
        let vals = [1i64, 2, 3, 4, 5, 6];
        let mut ok = vec![0i64; keys.len()];
        let mut ov = vec![0i64; keys.len()];
        let n = unsafe {
            align_rt_group_sum_i64(keys.as_ptr(), vals.as_ptr(), keys.len() as i64, ok.as_mut_ptr(), ov.as_mut_ptr(), keys.len() as i64)
        };
        assert_eq!(n, 3, "three distinct keys");
        // Output order is table order; collect into a map to check regardless of order.
        let got: std::collections::HashMap<i64, i64> = ok[..3].iter().copied().zip(ov[..3].iter().copied()).collect();
        assert_eq!(got, std::collections::HashMap::from([(10, 10), (20, 7), (30, 4)]));

        // Empty input → zero groups.
        assert_eq!(unsafe { align_rt_group_sum_i64(keys.as_ptr(), vals.as_ptr(), 0, ok.as_mut_ptr(), ov.as_mut_ptr(), 0) }, 0);

        // A single key repeated → one group with the full sum (and many collisions exercise probing).
        let k1 = [7i64; 1000];
        let v1: Vec<i64> = (0..1000).collect();
        let (mut ok1, mut ov1) = (vec![0i64; 1000], vec![0i64; 1000]);
        let n1 = unsafe { align_rt_group_sum_i64(k1.as_ptr(), v1.as_ptr(), 1000, ok1.as_mut_ptr(), ov1.as_mut_ptr(), 1000) };
        assert_eq!((n1, ok1[0], ov1[0]), (1, 7, (0..1000).sum()));

        // Many distinct keys force several table doublings + rehashes — check every group survives.
        // 200 keys, each appearing twice (k and again), so group k sums to 2*k.
        let mut k2 = Vec::new();
        let mut v2 = Vec::new();
        for k in 0..200i64 {
            k2.push(k);
            v2.push(k);
            k2.push(k);
            v2.push(k);
        }
        let (mut ok2, mut ov2) = (vec![0i64; k2.len()], vec![0i64; k2.len()]);
        let n2 = unsafe { align_rt_group_sum_i64(k2.as_ptr(), v2.as_ptr(), k2.len() as i64, ok2.as_mut_ptr(), ov2.as_mut_ptr(), k2.len() as i64) } as usize;
        assert_eq!(n2, 200, "200 distinct keys after growth");
        let got2: std::collections::HashMap<i64, i64> = ok2[..n2].iter().copied().zip(ov2[..n2].iter().copied()).collect();
        for k in 0..200i64 {
            assert_eq!(got2.get(&k), Some(&(2 * k)), "group {k} after rehash");
        }
    }

    #[test]
    fn group_sum_dense_and_sparse_paths_agree() {
        // The dense path (tight key range) and the hash path (wide/sparse range) must produce the
        // same per-key aggregate. Drive both with the same logical data, only the key offset differs.
        let run = |keys: &[i64], vals: &[i64]| -> std::collections::HashMap<i64, i64> {
            let (mut ok, mut ov) = (vec![0i64; keys.len()], vec![0i64; keys.len()]);
            let n = unsafe {
                align_rt_group_sum_i64(keys.as_ptr(), vals.as_ptr(), keys.len() as i64, ok.as_mut_ptr(), ov.as_mut_ptr(), keys.len() as i64)
            } as usize;
            ok[..n].iter().copied().zip(ov[..n].iter().copied()).collect()
        };

        // Dense: 1000 rows, keys a contiguous 0..250 range → span (249) < n (1000) → dense path.
        let mut dk = Vec::new();
        let mut dv = Vec::new();
        for i in 0..1000i64 {
            dk.push(i % 250);
            dv.push(i);
        }
        let dense = run(&dk, &dv);
        assert_eq!(dense.len(), 250, "250 dense groups");
        // group g = keys ≡ g (mod 250): rows g, g+250, g+500, g+750 → sum 4g + 1500.
        for g in 0..250i64 {
            assert_eq!(dense[&g], 4 * g + 1500, "dense group {g}");
        }

        // Sparse: the SAME groups but keys spread far apart (× 1_000_000) → span ≫ n → hash path.
        // The aggregate per logical group must be identical (only the key labels are scaled).
        let sk: Vec<i64> = dk.iter().map(|k| k * 1_000_000).collect();
        let sparse = run(&sk, &dv);
        assert_eq!(sparse.len(), 250, "250 sparse groups");
        for g in 0..250i64 {
            assert_eq!(sparse[&(g * 1_000_000)], 4 * g + 1500, "sparse group {g}");
        }

        // Negative keys: dense indexing must offset by min, not assume a 0 base.
        let nk = [-3i64, -1, -3, -2, -1];
        let nv = [10i64, 20, 30, 40, 50];
        let neg = run(&nk, &nv);
        assert_eq!(neg, std::collections::HashMap::from([(-3, 40), (-1, 70), (-2, 40)]));
    }

    #[test]
    fn json_decode_index_simd_matches_scalar_oracle() {
        // The SIMD decode index (AVX2 on x86_64, NEON on aarch64) must emit byte-for-byte the same
        // lean `{ } [ ] :` positions as the scalar reference, including the string/escape masking and
        // the 64-byte block carry (the same oracle discipline).
        let check = |src: &[u8]| {
            let mut want = Vec::new();
            json_decode_index_scalar(src, &mut want);
            let mut got = Vec::new();
            json_decode_index(src, &mut got); // dispatch == oracle (it *is* the scalar path with no SIMD)
            assert_eq!(got, want, "dispatch mismatch on {:?}", String::from_utf8_lossy(src));
            #[cfg(target_arch = "x86_64")]
            if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("pclmulqdq") {
                let mut g2 = Vec::new();
                unsafe { json_decode_index_avx2(src, &mut g2) };
                assert_eq!(g2, want, "avx2 mismatch on {:?}", String::from_utf8_lossy(src));
            }
            #[cfg(target_arch = "aarch64")]
            {
                let mut g2 = Vec::new();
                unsafe { json_decode_index_neon(src, &mut g2) }; // NEON is baseline — always exercised
                assert_eq!(g2, want, "neon mismatch on {:?}", String::from_utf8_lossy(src));
            }
        };

        let cases: &[&[u8]] = &[
            b"",
            b"{}",
            b"[]",
            b"{\"a\":1}",
            b"[{\"active\":true,\"pay\":12},{\"active\":false,\"pay\":3}]",
            b"{\"s\":\"a,b:c{}[]\"}",          // structural chars INSIDE a string — must be ignored
            b"{\"s\":\"he said \\\"hi\\\"\"}", // escaped quotes \" inside the value
            b"{\"s\":\"back\\\\slash\"}",      // escaped backslash \\
            b"{\"a\":{\"b\":{\"c\":[1,2,3]}}}",
            b"{\"k\":\"\"}",
            b"\"\\\\\"",
        ];
        for c in cases {
            check(c);
        }

        // Block-boundary stress across the first two 64-byte seams (escape + string carries).
        for pad in 55..75usize {
            let mut s = Vec::new();
            s.push(b'{');
            s.push(b'"');
            s.extend(std::iter::repeat_n(b'x', pad));
            s.extend_from_slice(b"\\\"end");
            s.extend_from_slice(b"\":1}");
            check(&s);
            for run in 1..6usize {
                let mut t = Vec::new();
                t.extend_from_slice(b"{\"k\":\"");
                t.extend(std::iter::repeat_n(b'y', pad));
                t.extend(std::iter::repeat_n(b'\\', run));
                t.extend_from_slice(b"\"}");
                check(&t);
            }
        }

        // Pseudo-random fuzz over a JSON-ish alphabet (so strings/escapes actually form).
        let mut state: u64 = 0x0bad_c0de_dead_beef;
        let alpha = b"{}[]:,\"\\ ab12tfnul";
        for len in [16usize, 64, 65, 200, 1000, 4096] {
            for _ in 0..40 {
                let mut s = Vec::with_capacity(len);
                for _ in 0..len {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    s.push(alpha[((state >> 33) as usize) % alpha.len()]);
                }
                check(&s);
            }
        }
    }

    #[test]
    fn dict_encode_str_assigns_dense_ids_and_builds_the_dictionary() {
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Row {
            key: AlignStr,
        }
        let s = |b: &'static [u8]| AlignStr { ptr: b.as_ptr(), len: b.len() as i64 };
        // keys: a b a c b a → first-occurrence ids a=0, b=1, c=2; id column = [0,1,0,2,1,0].
        let rows = [Row { key: s(b"a") }, Row { key: s(b"b") }, Row { key: s(b"a") }, Row { key: s(b"c") }, Row { key: s(b"b") }, Row { key: s(b"a") }];
        let stride = std::mem::size_of::<Row>() as i64;
        let key_off = std::mem::offset_of!(Row, key) as i64;
        let mut out_ids = vec![0i64; rows.len()];
        let mut out_dict = vec![AlignStr { ptr: std::ptr::null(), len: 0 }; rows.len()];
        let count = unsafe {
            align_rt_dict_encode_str(rows.as_ptr() as *const u8, rows.len() as i64, stride, key_off, out_ids.as_mut_ptr(), out_dict.as_mut_ptr(), rows.len() as i64)
        };
        assert_eq!(count, 3, "three distinct keys");
        assert_eq!(out_ids, vec![0, 1, 0, 2, 1, 0], "dense ids in first-occurrence order");
        // The dictionary maps each id to its representative bytes.
        let dict: Vec<&[u8]> = (0..count as usize).map(|i| unsafe { std::slice::from_raw_parts(out_dict[i].ptr, out_dict[i].len as usize) }).collect();
        assert_eq!(dict, vec![&b"a"[..], &b"b"[..], &b"c"[..]]);

        // The id column re-labels through the dict back to the original keys (the reuse contract).
        for (i, r) in rows.iter().enumerate() {
            let orig = unsafe { safe_slice(r.key.ptr, r.key.len) };
            assert_eq!(dict[out_ids[i] as usize], orig, "row {i} round-trips via the dictionary");
        }

        // Empty / null inputs encode nothing.
        assert_eq!(unsafe { align_rt_dict_encode_str(rows.as_ptr() as *const u8, 0, stride, key_off, out_ids.as_mut_ptr(), out_dict.as_mut_ptr(), 0) }, 0);
        assert_eq!(unsafe { align_rt_dict_encode_str(std::ptr::null(), 6, stride, key_off, out_ids.as_mut_ptr(), out_dict.as_mut_ptr(), 6) }, 0);
    }

    #[test]
    fn group_multi_str_fuses_aggregates_in_one_pass() {
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Row {
            key: AlignStr,
            a: i64,
            b: i64,
        }
        let s = |b: &'static [u8]| AlignStr { ptr: b.as_ptr(), len: b.len() as i64 };
        // x:(a=10,b=1) y:(a=5,b=2) x:(a=7,b=3) → groups x (id0), y (id1).
        let rows = [
            Row { key: s(b"x"), a: 10, b: 1 },
            Row { key: s(b"y"), a: 5, b: 2 },
            Row { key: s(b"x"), a: 7, b: 3 },
        ];
        let stride = std::mem::size_of::<Row>() as i64;
        let key_off = std::mem::offset_of!(Row, key) as i64;
        let a_off = std::mem::offset_of!(Row, a) as i64;
        let b_off = std::mem::offset_of!(Row, b) as i64;
        let n = rows.len() as i64;
        let (mut sa, mut mb, mut cnt) = (vec![0i64; rows.len()], vec![0i64; rows.len()], vec![0i64; rows.len()]);
        // sum(.a) op=0, max(.b) op=2, count() op=3.
        let specs = [
            GroupMultiSpec { val_off: a_off, op: 0, out_vals: sa.as_mut_ptr() },
            GroupMultiSpec { val_off: b_off, op: 2, out_vals: mb.as_mut_ptr() },
            GroupMultiSpec { val_off: 0, op: 3, out_vals: cnt.as_mut_ptr() },
        ];
        let mut out_keys = vec![AlignStr { ptr: std::ptr::null(), len: 0 }; rows.len()];
        let count = unsafe {
            align_rt_group_multi_str(rows.as_ptr() as *const u8, n, stride, key_off, specs.as_ptr(), specs.len() as i64, out_keys.as_mut_ptr(), n)
        };
        assert_eq!(count, 2, "two distinct keys");
        let keys: Vec<&[u8]> = (0..count as usize).map(|i| unsafe { std::slice::from_raw_parts(out_keys[i].ptr, out_keys[i].len as usize) }).collect();
        assert_eq!(keys, vec![&b"x"[..], &b"y"[..]], "first-occurrence key order");
        assert_eq!(&sa[..2], &[17, 5], "sum(.a): x=10+7, y=5");
        assert_eq!(&mb[..2], &[3, 2], "max(.b): x=max(1,3), y=2");
        assert_eq!(&cnt[..2], &[2, 1], "count(): x=2, y=1");

        // Each fused column matches the equivalent single-aggregate str group-by (the contract).
        let mut single_sa = vec![0i64; rows.len()];
        let mut single_keys = vec![AlignStr { ptr: std::ptr::null(), len: 0 }; rows.len()];
        unsafe { align_rt_group_sum_str(rows.as_ptr() as *const u8, n, stride, key_off, a_off, single_keys.as_mut_ptr(), single_sa.as_mut_ptr(), n) };
        assert_eq!(&sa[..2], &single_sa[..2], "fused sum matches single-aggregate sum");

        // Empty / null inputs aggregate nothing.
        assert_eq!(
            unsafe { align_rt_group_multi_str(rows.as_ptr() as *const u8, 0, stride, key_off, specs.as_ptr(), specs.len() as i64, out_keys.as_mut_ptr(), 0) },
            0
        );
        assert_eq!(
            unsafe { align_rt_group_multi_str(std::ptr::null(), n, stride, key_off, specs.as_ptr(), specs.len() as i64, out_keys.as_mut_ptr(), n) },
            0
        );
    }

    #[test]
    fn gather_i64_projects_a_strided_column() {
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Row {
            key: AlignStr,
            val: i64,
        }
        let s = |b: &'static [u8]| AlignStr { ptr: b.as_ptr(), len: b.len() as i64 };
        let rows = [Row { key: s(b"a"), val: 10 }, Row { key: s(b"b"), val: 20 }, Row { key: s(b"a"), val: 5 }];
        let stride = std::mem::size_of::<Row>() as i64;
        let off = std::mem::offset_of!(Row, val) as i64;
        let mut out = vec![0i64; rows.len()];
        unsafe { align_rt_gather_i64(rows.as_ptr() as *const u8, rows.len() as i64, stride, off, out.as_mut_ptr()) };
        assert_eq!(out, vec![10, 20, 5]);
        // Empty / null / negative-stride / negative-offset inputs gather nothing (leave `out` untouched).
        let mut z = vec![-1i64; 3];
        unsafe { align_rt_gather_i64(rows.as_ptr() as *const u8, 0, stride, off, z.as_mut_ptr()) };
        unsafe { align_rt_gather_i64(std::ptr::null(), 3, stride, off, z.as_mut_ptr()) };
        unsafe { align_rt_gather_i64(rows.as_ptr() as *const u8, 3, -1, off, z.as_mut_ptr()) };
        unsafe { align_rt_gather_i64(rows.as_ptr() as *const u8, 3, stride, -1, z.as_mut_ptr()) };
        assert_eq!(z, vec![-1, -1, -1]);
    }

    #[test]
    fn a2_dict_encode_then_id_groupby_then_label_matches_string_groupby() {
        // The A2 reuse composition — dict_encode → dense-id group_by on the ids → dict_lookup label —
        // must produce the SAME (key → sum) as the one-shot A1 string-key group_by on the same data.
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Row {
            key: AlignStr,
            val: i64,
        }
        let s = |b: &'static [u8]| AlignStr { ptr: b.as_ptr(), len: b.len() as i64 };
        let rows = [
            Row { key: s(b"a"), val: 1 },
            Row { key: s(b"b"), val: 2 },
            Row { key: s(b"a"), val: 3 },
            Row { key: s(b"c"), val: 4 },
            Row { key: s(b"b"), val: 5 },
            Row { key: s(b"a"), val: 6 },
        ];
        let n = rows.len() as i64;
        let stride = std::mem::size_of::<Row>() as i64;
        let key_off = std::mem::offset_of!(Row, key) as i64;
        let val_off = std::mem::offset_of!(Row, val) as i64;
        let to_map = |keys: &[AlignStr], vals: &[i64]| -> std::collections::HashMap<Vec<u8>, i64> {
            keys.iter().zip(vals).map(|(k, &v)| (unsafe { safe_slice(k.ptr, k.len) }.to_vec(), v)).collect()
        };

        // A1 reference: one-shot string-key group sum.
        let (mut a1k, mut a1v) = (vec![AlignStr { ptr: std::ptr::null(), len: 0 }; 6], vec![0i64; 6]);
        let a1n = unsafe { align_rt_group_sum_str(rows.as_ptr() as *const u8, n, stride, key_off, val_off, a1k.as_mut_ptr(), a1v.as_mut_ptr(), n) } as usize;
        let a1 = to_map(&a1k[..a1n], &a1v[..a1n]);

        // A2: encode once → ids + dict.
        let mut ids = vec![0i64; 6];
        let mut dict = vec![AlignStr { ptr: std::ptr::null(), len: 0 }; 6];
        let dlen = unsafe { align_rt_dict_encode_str(rows.as_ptr() as *const u8, n, stride, key_off, ids.as_mut_ptr(), dict.as_mut_ptr(), n) };
        assert!(dlen > 0);
        // Project the value column (the encoded group_by reads values from the source struct array).
        let vals: Vec<i64> = rows.iter().map(|r| r.val).collect();
        // Reuse the dense-id i64 group_by on the (dense) ids.
        let (mut gk, mut gv) = (vec![0i64; 6], vec![0i64; 6]);
        let gn = unsafe { align_rt_group_sum_i64(ids.as_ptr(), vals.as_ptr(), n, gk.as_mut_ptr(), gv.as_mut_ptr(), n) } as usize;
        // Label the distinct ids back to str keys via the dictionary.
        let mut labels = vec![AlignStr { ptr: std::ptr::null(), len: 0 }; gn];
        unsafe { align_rt_dict_lookup(gk.as_ptr(), gn as i64, dict.as_ptr(), dlen, labels.as_mut_ptr()) };
        let a2 = to_map(&labels, &gv[..gn]);

        assert_eq!(a2, a1, "A2 (encode → id group_by → label) must equal A1 (string group_by)");
        assert_eq!(a2, std::collections::HashMap::from([(b"a".to_vec(), 10), (b"b".to_vec(), 7), (b"c".to_vec(), 4)]));
    }

    #[test]
    fn group_sum_str_interns_and_aggregates_by_string_key() {
        // An AoS row matching what codegen lays out: a `str` key view + an i64 value.
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Row {
            key: AlignStr,
            val: i64,
        }
        let s = |b: &'static [u8]| AlignStr { ptr: b.as_ptr(), len: b.len() as i64 };
        // keys: a a b c a b ; vals: 1 2 3 4 5 6 → {a:1+2+5=8, b:3+6=9, c:4}
        let rows = [
            Row { key: s(b"a"), val: 1 },
            Row { key: s(b"a"), val: 2 },
            Row { key: s(b"b"), val: 3 },
            Row { key: s(b"c"), val: 4 },
            Row { key: s(b"a"), val: 5 },
            Row { key: s(b"b"), val: 6 },
        ];
        let stride = std::mem::size_of::<Row>() as i64;
        let key_off = std::mem::offset_of!(Row, key) as i64;
        let val_off = std::mem::offset_of!(Row, val) as i64;
        type GroupStrFn = unsafe extern "C" fn(*const u8, i64, i64, i64, i64, *mut AlignStr, *mut i64, i64) -> i64;
        let collect = |f: GroupStrFn| -> std::collections::HashMap<&[u8], i64> {
            let (mut ok, mut ov) = (vec![AlignStr { ptr: std::ptr::null(), len: 0 }; rows.len()], vec![0i64; rows.len()]);
            let count = unsafe { f(rows.as_ptr() as *const u8, rows.len() as i64, stride, key_off, val_off, ok.as_mut_ptr(), ov.as_mut_ptr(), rows.len() as i64) } as usize;
            (0..count).map(|g| (unsafe { std::slice::from_raw_parts(ok[g].ptr, ok[g].len as usize) }, ov[g])).collect()
        };
        // sum {a:8, b:9, c:4}; min {a:1, b:3, c:4}; max {a:5, b:6, c:4}; count {a:3, b:2, c:1}.
        assert_eq!(collect(align_rt_group_sum_str), std::collections::HashMap::from([(&b"a"[..], 8), (&b"b"[..], 9), (&b"c"[..], 4)]));
        assert_eq!(collect(align_rt_group_min_str), std::collections::HashMap::from([(&b"a"[..], 1), (&b"b"[..], 3), (&b"c"[..], 4)]));
        assert_eq!(collect(align_rt_group_max_str), std::collections::HashMap::from([(&b"a"[..], 5), (&b"b"[..], 6), (&b"c"[..], 4)]));
        assert_eq!(collect(align_rt_group_count_str), std::collections::HashMap::from([(&b"a"[..], 3), (&b"b"[..], 2), (&b"c"[..], 1)]));
        let (mut ok, mut ov) = (vec![AlignStr { ptr: std::ptr::null(), len: 0 }; rows.len()], vec![0i64; rows.len()]);

        // Empty input → zero groups; a null base is also zero (degenerate, not -1).
        assert_eq!(unsafe { align_rt_group_sum_str(rows.as_ptr() as *const u8, 0, stride, key_off, val_off, ok.as_mut_ptr(), ov.as_mut_ptr(), 0) }, 0);
        assert_eq!(unsafe { align_rt_group_sum_str(std::ptr::null(), 6, stride, key_off, val_off, ok.as_mut_ptr(), ov.as_mut_ptr(), 6) }, 0);

        // An empty-string key is a real, distinct key (not skipped).
        let rows2 = [Row { key: s(b""), val: 10 }, Row { key: s(b"x"), val: 20 }, Row { key: s(b""), val: 5 }];
        let (mut ok2, mut ov2) = (vec![AlignStr { ptr: std::ptr::null(), len: 0 }; 3], vec![0i64; 3]);
        let c2 = unsafe {
            align_rt_group_sum_str(rows2.as_ptr() as *const u8, 3, stride, key_off, val_off, ok2.as_mut_ptr(), ov2.as_mut_ptr(), 3)
        } as usize;
        let got2: std::collections::HashMap<&[u8], i64> = (0..c2)
            .map(|g| (unsafe { std::slice::from_raw_parts(ok2[g].ptr, ok2[g].len as usize) }, ov2[g]))
            .collect();
        assert_eq!(got2, std::collections::HashMap::from([(&b""[..], 15), (&b"x"[..], 20)]));
    }

    #[test]
    fn group_str_cols_aggregates_two_separate_columns() {
        // The soa form: the key and value live in SEPARATE contiguous column buffers (not a strided
        // record). Same data as the AoS test → same groups: keys a a b c a b, vals 1 2 3 4 5 6.
        let s = |b: &'static [u8]| AlignStr { ptr: b.as_ptr(), len: b.len() as i64 };
        let key_col = [s(b"a"), s(b"a"), s(b"b"), s(b"c"), s(b"a"), s(b"b")];
        let val_col = [1i64, 2, 3, 4, 5, 6];
        let n = key_col.len() as i64;
        type ColsFn = unsafe extern "C" fn(*const AlignStr, *const i64, i64, *mut AlignStr, *mut i64, i64) -> i64;
        let collect = |f: ColsFn| -> std::collections::HashMap<&[u8], i64> {
            let (mut ok, mut ov) = (vec![AlignStr { ptr: std::ptr::null(), len: 0 }; key_col.len()], vec![0i64; key_col.len()]);
            let count = unsafe { f(key_col.as_ptr(), val_col.as_ptr(), n, ok.as_mut_ptr(), ov.as_mut_ptr(), n) } as usize;
            (0..count).map(|g| (unsafe { std::slice::from_raw_parts(ok[g].ptr, ok[g].len as usize) }, ov[g])).collect()
        };
        // sum {a:8, b:9, c:4}; min {a:1, b:3, c:4}; max {a:5, b:6, c:4}; count {a:3, b:2, c:1}.
        assert_eq!(collect(align_rt_group_sum_str_cols), std::collections::HashMap::from([(&b"a"[..], 8), (&b"b"[..], 9), (&b"c"[..], 4)]));
        assert_eq!(collect(align_rt_group_min_str_cols), std::collections::HashMap::from([(&b"a"[..], 1), (&b"b"[..], 3), (&b"c"[..], 4)]));
        assert_eq!(collect(align_rt_group_max_str_cols), std::collections::HashMap::from([(&b"a"[..], 5), (&b"b"[..], 6), (&b"c"[..], 4)]));
        // count passes a null value column (unused) — must not deref it.
        let (mut ok, mut ov) = (vec![AlignStr { ptr: std::ptr::null(), len: 0 }; key_col.len()], vec![0i64; key_col.len()]);
        let cc = unsafe { align_rt_group_count_str_cols(key_col.as_ptr(), std::ptr::null(), n, ok.as_mut_ptr(), ov.as_mut_ptr(), n) } as usize;
        let counts: std::collections::HashMap<&[u8], i64> = (0..cc).map(|g| (unsafe { std::slice::from_raw_parts(ok[g].ptr, ok[g].len as usize) }, ov[g])).collect();
        assert_eq!(counts, std::collections::HashMap::from([(&b"a"[..], 3), (&b"b"[..], 2), (&b"c"[..], 1)]));

        // Degenerate: empty input and a null key column both yield zero groups (not -1).
        assert_eq!(unsafe { align_rt_group_sum_str_cols(key_col.as_ptr(), val_col.as_ptr(), 0, ok.as_mut_ptr(), ov.as_mut_ptr(), 0) }, 0);
        assert_eq!(unsafe { align_rt_group_sum_str_cols(std::ptr::null(), val_col.as_ptr(), n, ok.as_mut_ptr(), ov.as_mut_ptr(), n) }, 0);
    }

    #[test]
    fn group_min_max_count_aggregate_by_key() {
        // keys: 10,20,10,30,20,10 ; vals: 1,2,3,4,5,6
        let keys = [10i64, 20, 10, 30, 20, 10];
        let vals = [1i64, 2, 3, 4, 5, 6];
        let collect = |f: unsafe extern "C" fn(*const i64, *const i64, i64, *mut i64, *mut i64, i64) -> i64| {
            let (mut ok, mut ov) = (vec![0i64; keys.len()], vec![0i64; keys.len()]);
            let n = unsafe { f(keys.as_ptr(), vals.as_ptr(), keys.len() as i64, ok.as_mut_ptr(), ov.as_mut_ptr(), keys.len() as i64) } as usize;
            ok[..n].iter().copied().zip(ov[..n].iter().copied()).collect::<std::collections::HashMap<i64, i64>>()
        };
        // min: {10:min(1,3,6)=1, 20:min(2,5)=2, 30:4}
        assert_eq!(collect(align_rt_group_min_i64), std::collections::HashMap::from([(10, 1), (20, 2), (30, 4)]));
        // max: {10:max(1,3,6)=6, 20:max(2,5)=5, 30:4}
        assert_eq!(collect(align_rt_group_max_i64), std::collections::HashMap::from([(10, 6), (20, 5), (30, 4)]));
        // count: {10:3, 20:2, 30:1} — no value column.
        let (mut ok, mut ov) = (vec![0i64; keys.len()], vec![0i64; keys.len()]);
        let n = unsafe { align_rt_group_count_i64(keys.as_ptr(), keys.len() as i64, ok.as_mut_ptr(), ov.as_mut_ptr(), keys.len() as i64) } as usize;
        let counts: std::collections::HashMap<i64, i64> = ok[..n].iter().copied().zip(ov[..n].iter().copied()).collect();
        assert_eq!(counts, std::collections::HashMap::from([(10, 3), (20, 2), (30, 1)]));
        // Negative values: min/max must track sign (not magnitude).
        let nk = [1i64, 1, 1];
        let nv = [-5i64, 3, -2];
        let (mut ok2, mut ov2) = (vec![0i64; 3], vec![0i64; 3]);
        let nn = unsafe { align_rt_group_min_i64(nk.as_ptr(), nv.as_ptr(), 3, ok2.as_mut_ptr(), ov2.as_mut_ptr(), 3) };
        assert_eq!((nn, ov2[0]), (1, -5));
    }

    #[test]
    fn json_number_parse_and_lexical_skip_agree_on_span() {
        // `skip_number` must advance the cursor over exactly the same token `number` parses, so
        // an unknown numeric field is skipped lexically without a float parse (work/ probe: ~3x).
        for s in ["0", "-1", "42", "3.14", "-0.5", "1e3", "6.022e23", "-2.5E-4", "1000000000000"] {
            let mut p = JsonParser { src: s.as_bytes(), pos: 0 };
            let parsed = p.number();
            let after_parse = p.pos;
            assert!(parsed.is_some(), "number() should parse {s:?}");

            let mut q = JsonParser { src: s.as_bytes(), pos: 0 };
            assert_eq!(q.skip_number(), Some(()), "skip_number() should accept {s:?}");
            assert_eq!(q.pos, after_parse, "skip and parse must consume the same span for {s:?}");
            assert_eq!(q.pos, s.len(), "whole token consumed for {s:?}");
        }

        // A trailing non-number byte bounds the token (number followed by `}`); only digits move.
        let mut p = JsonParser { src: b"12,3", pos: 0 };
        assert_eq!(p.skip_number(), Some(()));
        assert_eq!(p.pos, 2, "stops at the comma");

        // Not a valid JSON number: a lone `-`, a leading `.`, a dangling fraction (`1.`) or
        // exponent (`1e`, `1e+`) — each consumes nothing and fails, in BOTH `skip_number` and
        // `number` (so the two agree on the accepted language, the point of sharing `number_span`).
        for bad in ["-", ".5", "x", "1.", "1e", "1e+"] {
            let mut p = JsonParser { src: bad.as_bytes(), pos: 0 };
            assert_eq!(p.skip_number(), None, "{bad:?} is not a JSON number");
            assert_eq!(p.pos, 0, "cursor restored for {bad:?}");
            let mut q = JsonParser { src: bad.as_bytes(), pos: 0 };
            assert_eq!(q.number(), None, "number() also rejects {bad:?}");
        }
    }

    #[test]
    fn json_skip_value_handles_nested_objects_arrays_null_and_escapes() {
        // Each whole value is skipped end-to-end (cursor lands at EOF). Covers null, escaped
        // strings, nested objects/arrays, whitespace, and the `\"`/`\\` cases that a naive
        // string skip would terminate on early.
        for s in [
            "null",
            "true",
            "-12.5e3",
            r#""plain""#,
            r#""a\"b\\c\n""#,                 // escaped quote, backslash, newline
            r#""x\u00e9y""#,              // `\uXXXX` escape: stepped over, not decoded
            r#""é本""#,                       // multibyte UTF-8 literal content
            "{}",
            "[]",
            r#"{ "a": 1, "b": [1, 2, {"c": 3}], "d": null }"#,
            r#"[1, "x", true, null, {"k": [false]}, [[]]]"#,
            r#"{"s":"has } and ] and \" inside"}"#, // structural bytes inside a string must not end it
        ] {
            let mut p = JsonParser { src: s.as_bytes(), pos: 0 };
            assert_eq!(p.skip_value(), Some(()), "skip_value should accept {s:?}");
            assert_eq!(p.pos, s.len(), "whole value consumed for {s:?}");
        }

        // A skipped value bounded by a following member: `{"u":<obj>,"id":7}` — after skipping the
        // object value the cursor sits on the comma, ready for the next key.
        let s = r#"{"a":1},rest"#;
        let mut p = JsonParser { src: s.as_bytes(), pos: 0 };
        assert_eq!(p.skip_value(), Some(()));
        assert_eq!(&s.as_bytes()[p.pos..], b",rest", "stops at the object's close");

        // Malformed / truncated values fail (no panic, no run-off): unterminated string, object,
        // and array; a bare `}`/`]`; an unterminated escape.
        for bad in [r#""no end"#, "{", "[", "}", "]", r#""x\"#, r#"{"k":}"#, "[,]"] {
            let mut p = JsonParser { src: bad.as_bytes(), pos: 0 };
            assert_eq!(p.skip_value(), None, "{bad:?} must not skip cleanly");
        }

        // Depth guard: 200 nested arrays exceed MAX_DEPTH (128) → rejected, not a stack overflow.
        let deep = "[".repeat(200) + &"]".repeat(200);
        let mut p = JsonParser { src: deep.as_bytes(), pos: 0 };
        assert_eq!(p.skip_value(), None, "over-deep nesting is rejected");
        // Just within the limit skips fine.
        let ok_depth = "[".repeat(100) + &"]".repeat(100);
        let mut p = JsonParser { src: ok_depth.as_bytes(), pos: 0 };
        assert_eq!(p.skip_value(), Some(()), "depth 100 is within the limit");
    }

    #[test]
    fn fs_read_file_fast_path_and_fallbacks() {
        // Build a unique temp path so concurrent test runs don't collide (no Date/rand in the
        // crate, but the test binary's pid + a counter address suffice for uniqueness here).
        let dir = std::env::temp_dir();
        let uniq = format!("align-rt-readfile-{}-{:p}", std::process::id(), &dir as *const _);

        let read = |path: &str| -> (i32, Option<Vec<u8>>) {
            let mut out = AlignStr { ptr: core::ptr::null(), len: 0 };
            let rc = unsafe { align_rt_fs_read_file(path.as_ptr(), path.len() as i64, &mut out) };
            let bytes = if rc == 0 && out.len > 0 {
                let v = unsafe { safe_slice(out.ptr, out.len) }.to_vec();
                unsafe { align_rt_free(out.ptr as *mut u8) };
                Some(v)
            } else {
                // rc==0 with len 0 owns no buffer (null ptr) — nothing to free.
                if rc == 0 { Some(Vec::new()) } else { None }
            };
            (rc, bytes)
        };

        // Fast path: a regular file larger than one read buffer — the whole content comes back
        // intact (exercises `read_exact` filling the owned buffer + the EOF guard). `read_file`
        // returns a `string`, so the content is valid multibyte UTF-8 (draft §7/§12; whole units are
        // appended so no multibyte char is truncated) — binary is read via `reader.read(buffer)`.
        let big_path = dir.join(format!("{uniq}-big.bin"));
        let mut text = String::new();
        while text.len() < 100_000 {
            text.push_str("café 日本語 fast-path 😀 line\n");
        }
        let content = text.into_bytes();
        std::fs::write(&big_path, &content).expect("write big temp file");
        let (rc, got) = read(big_path.to_str().unwrap());
        assert_eq!(rc, 0);
        assert_eq!(got.as_deref(), Some(content.as_slice()), "fast path reads the whole file");

        // Empty file: `flen == 0` skips the fast path; the fallback yields an owned null/0 view.
        let empty_path = dir.join(format!("{uniq}-empty.bin"));
        std::fs::write(&empty_path, b"").expect("write empty temp file");
        assert_eq!(read(empty_path.to_str().unwrap()), (0, Some(Vec::new())));

        // Missing file: both `File::open` (fast path) and `std::fs::read` (fallback) fail → rc 1.
        let missing = dir.join(format!("{uniq}-missing.bin"));
        let _ = std::fs::remove_file(&missing);
        assert_eq!(read(missing.to_str().unwrap()).0, 1);

        std::fs::remove_file(&big_path).ok();
        std::fs::remove_file(&empty_path).ok();

        // Special file whose metadata length is 0 but which yields bytes on read (`/proc`): must
        // hit the fallback and return non-empty content. Linux-only.
        #[cfg(target_os = "linux")]
        {
            let (rc, got) = read("/proc/self/stat");
            assert_eq!(rc, 0, "/proc/self/stat readable via fallback");
            assert!(got.is_some_and(|b| !b.is_empty()), "/proc file returns content");
        }
    }

    #[test]
    fn builder_write_int_matches_format() {
        // The hand-rolled itoa must equal `format!("{v}")` across the i64 range incl. edges.
        for v in [0i64, 7, -1, 42, -123, i64::MAX, i64::MIN, 1000000000000, -9999] {
            let mut b = Builder { buf: Vec::new(), arena: std::ptr::null_mut() };
            unsafe { align_rt_builder_write_int(&mut b, v) };
            assert_eq!(String::from_utf8(b.buf).unwrap(), format!("{v}"), "write_int({v})");
        }
    }

    #[test]
    fn builder_write_str_int_str_matches_three_writes() {
        for v in [0i64, 7, -1, 42, -123, 999, -999, i64::MAX, i64::MIN] {
            let mut batched = Builder { buf: Vec::new(), arena: std::ptr::null_mut() };
            unsafe {
                align_rt_builder_write_str_int_str(&mut batched, b"item-".as_ptr(), 5, v, b"-status ".as_ptr(), 8);
            }

            let mut separate = Builder { buf: Vec::new(), arena: std::ptr::null_mut() };
            unsafe {
                align_rt_builder_write(&mut separate, b"item-".as_ptr(), 5);
                align_rt_builder_write_int(&mut separate, v);
                align_rt_builder_write(&mut separate, b"-status ".as_ptr(), 8);
            }

            assert_eq!(batched.buf, separate.buf, "batched writes match separate writes for {v}");
        }
    }

    #[test]
    fn integer_parses_edges() {
        // The hand-rolled single-pass `integer()` must match the old `parse::<i64>()`: full range
        // incl. `i64::MIN`, and reject overflow (return None) rather than wrap.
        let cases: &[(&[u8], Option<i64>)] = &[
            (b"0", Some(0)),
            (b"7", Some(7)),
            (b"-1", Some(-1)),
            (b"9223372036854775807", Some(i64::MAX)),
            (b"-9223372036854775808", Some(i64::MIN)),
            (b"9223372036854775808", None),  // i64::MAX + 1 → overflow, reject
            (b"-9223372036854775809", None), // i64::MIN - 1 → overflow, reject
            (b"x", None),                    // no digits
        ];
        for (input, want) in cases {
            let mut p = JsonParser { src: input, pos: 0 };
            assert_eq!(p.integer(), *want, "integer({:?})", std::str::from_utf8(input).unwrap());
        }
        // On overflow the parser still consumes the whole number (ends past all digits), so it
        // doesn't try to re-parse the tail as a new token.
        let mut p = JsonParser { src: b"9223372036854775808,", pos: 0 };
        assert_eq!(p.integer(), None);
        assert_eq!(p.peek(), Some(b','), "overflow should consume all 19 digits, leaving pos at ','");
    }

    #[test]
    fn integer_unsigned_parses_full_u64_range() {
        // The full-range unsigned parser for u64 fields: [0, u64::MAX] accepted, overflow + any
        // negative rejected, and (like `integer`) the cursor is left past the whole token on failure.
        let cases: &[(&[u8], Option<u64>)] = &[
            (b"0", Some(0)),
            (b"7", Some(7)),
            (b"9223372036854775807", Some(i64::MAX as u64)),
            (b"9223372036854775808", Some(1u64 << 63)), // i64::MAX + 1 — unrepresentable in i64
            (b"18446744073709551615", Some(u64::MAX)),
            (b"18446744073709551616", None), // u64::MAX + 1 → overflow, reject
            (b"-1", None),                   // a u64 has no negatives
            (b"x", None),                    // no digits
        ];
        for (input, want) in cases {
            let mut p = JsonParser { src: input, pos: 0 };
            assert_eq!(p.integer_unsigned(), *want, "integer_unsigned({:?})", std::str::from_utf8(input).unwrap());
        }
        // On overflow / negative the cursor ends past the whole token (so a failed parse aborts cleanly).
        let mut p = JsonParser { src: b"18446744073709551616,", pos: 0 };
        assert_eq!(p.integer_unsigned(), None);
        assert_eq!(p.peek(), Some(b','), "overflow consumes all digits");
        let mut p = JsonParser { src: b"-5,", pos: 0 };
        assert_eq!(p.integer_unsigned(), None);
        assert_eq!(p.peek(), Some(b','), "negative consumes the sign + digits");

        // `integer_field` routes width-8 unsigned to the full-range path, everything else to i64 +
        // range check. Spot-check the routing boundary (i64::MAX+1 into u64 vs i64, and truncation).
        let mut p = JsonParser { src: b"9223372036854775808", pos: 0 };
        assert_eq!(p.integer_field(8, false), Some(1u64 << 63), "i64::MAX+1 fits u64");
        let mut p = JsonParser { src: b"9223372036854775808", pos: 0 };
        assert_eq!(p.integer_field(8, true), None, "i64::MAX+1 overflows i64 (signed)");
        let mut p = JsonParser { src: b"256", pos: 0 };
        assert_eq!(p.integer_field(1, false), None, "256 out of u8 range");
        let mut p = JsonParser { src: b"-1", pos: 0 };
        assert_eq!(p.integer_field(8, false), None, "-1 out of u64 range");
        let mut p = JsonParser { src: b"-1", pos: 0 };
        assert_eq!(p.integer_field(8, true), Some(u64::MAX), "-1 into i64 = all-ones bit pattern");
    }

    #[test]
    fn int_in_range_covers_widths_and_signs() {
        // Unsigned: [0, 2^(8w)-1]; a w==8 field spans the whole i64 (only < 0 is rejected).
        assert!(int_in_range(0, 1, false));
        assert!(int_in_range(255, 1, false));
        assert!(!int_in_range(256, 1, false));
        assert!(!int_in_range(-1, 1, false));
        assert!(int_in_range(65535, 2, false));
        assert!(!int_in_range(65536, 2, false));
        assert!(int_in_range(4294967295, 4, false));
        assert!(!int_in_range(4294967296, 4, false));
        assert!(int_in_range(i64::MAX, 8, false));
        assert!(!int_in_range(-1, 8, false)); // u64 field rejects a negative
        // Signed: [-2^(8w-1), 2^(8w-1)-1]; a w==8 field is the whole i64.
        assert!(int_in_range(-128, 1, true));
        assert!(int_in_range(127, 1, true));
        assert!(!int_in_range(128, 1, true));
        assert!(!int_in_range(-129, 1, true));
        assert!(int_in_range(-32768, 2, true));
        assert!(int_in_range(32767, 2, true));
        assert!(!int_in_range(32768, 2, true));
        assert!(int_in_range(-2147483648, 4, true));
        assert!(int_in_range(2147483647, 4, true));
        assert!(!int_in_range(2147483648, 4, true));
        assert!(int_in_range(i64::MIN, 8, true));
        assert!(int_in_range(i64::MAX, 8, true));
    }

    #[test]
    fn json_decode_range_checks_integer_fields() {
        // Decode `{"n": <v>}` into a single integer field described by `tag` (int kind 0, so
        // `tag = (signed << 16) | byte-width`). Returns (status, first 8 out bytes).
        fn decode(json: &[u8], tag: i32, out_size: i64) -> (i32, [u8; 8]) {
            let name = b"n";
            let f = JsonField { name_ptr: name.as_ptr(), name_len: 1, tag, offset: 0 };
            let mut out = [0u8; 8];
            let rc = unsafe {
                align_rt_json_decode(
                    json.as_ptr(),
                    json.len() as i64,
                    &f,
                    1,
                    out.as_mut_ptr(),
                    out_size,
                    std::ptr::null(),
                    0,
                    0,
                )
            };
            (rc, out)
        }
        const U8: i32 = 1; // unsigned, width 1
        const U16: i32 = 2;
        const U32: i32 = 4;
        const U64: i32 = 8; // unsigned, width 8
        const I8: i32 = (1 << 16) | 1; // signed, width 1
        const I16: i32 = (1 << 16) | 2;
        const I32: i32 = (1 << 16) | 4;
        const I64: i32 = (1 << 16) | 8;

        // Out-of-range values must be rejected (status 1), not silently truncated/sign-wrapped.
        assert_eq!(decode(b"{\"n\": 300}", U8, 1).0, 1, "300 into u8 rejected");
        assert_eq!(decode(b"{\"n\": -1}", U32, 4).0, 1, "-1 into u32 rejected");
        assert_eq!(decode(b"{\"n\": 200}", I8, 1).0, 1, "200 into i8 rejected");
        assert_eq!(decode(b"{\"n\": 256}", U8, 1).0, 1, "256 into u8 rejected");
        assert_eq!(decode(b"{\"n\": 65536}", U16, 2).0, 1, "65536 into u16 rejected");
        assert_eq!(decode(b"{\"n\": 32768}", I16, 2).0, 1, "32768 into i16 rejected");
        assert_eq!(decode(b"{\"n\": 2147483648}", I32, 4).0, 1, "INT32_MAX+1 into i32 rejected");

        // In-range boundary values must decode (status 0) with the correct little-endian bytes.
        let (rc, out) = decode(b"{\"n\": 0}", U8, 1);
        assert_eq!((rc, out[0]), (0, 0), "u8 0 ok");
        let (rc, out) = decode(b"{\"n\": 255}", U8, 1);
        assert_eq!((rc, out[0]), (0, 255), "u8 255 ok");
        let (rc, out) = decode(b"{\"n\": -128}", I8, 1);
        assert_eq!((rc, out[0]), (0, 0x80), "i8 -128 ok");
        let (rc, out) = decode(b"{\"n\": 127}", I8, 1);
        assert_eq!((rc, out[0]), (0, 0x7f), "i8 127 ok");
        let (rc, out) = decode(b"{\"n\": 42}", U8, 1);
        assert_eq!((rc, out[0]), (0, 42), "u8 42 ok (regression)");
        let (rc, out) = decode(b"{\"n\": -9223372036854775808}", I64, 8);
        assert_eq!((rc, i64::from_le_bytes(out)), (0, i64::MIN), "i64::MIN ok");
        let (rc, out) = decode(b"{\"n\": 9223372036854775807}", I64, 8);
        assert_eq!((rc, i64::from_le_bytes(out)), (0, i64::MAX), "i64::MAX ok");

        // u64 fields accept the full [0, u64::MAX] range (the i64 parser capped at i64::MAX before).
        let (rc, out) = decode(b"{\"n\": 18446744073709551615}", U64, 8);
        assert_eq!((rc, u64::from_le_bytes(out)), (0, u64::MAX), "u64::MAX ok");
        let (rc, out) = decode(b"{\"n\": 9223372036854775808}", U64, 8);
        assert_eq!((rc, u64::from_le_bytes(out)), (0, 1u64 << 63), "i64::MAX+1 into u64 ok");
        let (rc, out) = decode(b"{\"n\": 0}", U64, 8);
        assert_eq!((rc, u64::from_le_bytes(out)), (0, 0), "u64 0 ok");
        // u64::MAX + 1 overflows the u64 parser → rejected.
        assert_eq!(decode(b"{\"n\": 18446744073709551616}", U64, 8).0, 1, "u64::MAX+1 rejected");
        // A negative into a u64 field is rejected (regression — a u64 has no negatives).
        assert_eq!(decode(b"{\"n\": -1}", U64, 8).0, 1, "-1 into u64 rejected");
    }

    #[test]
    fn json_decode_array_range_checks_integers() {
        // `align_rt_json_decode_array` shares the range check via the same tag encoding.
        fn decode(json: &[u8], tag: i32) -> i32 {
            let mut out = AlignStr { ptr: std::ptr::null_mut(), len: 0 };
            let rc = unsafe { align_rt_json_decode_array(json.as_ptr(), json.len() as i64, tag, &mut out) };
            if !out.ptr.is_null() {
                unsafe { align_rt_free(out.ptr as *mut u8) };
            }
            rc
        }
        const U8: i32 = 1;
        const I8: i32 = (1 << 16) | 1;
        const U64: i32 = 8;
        assert_eq!(decode(b"[1, 2, 300]", U8), 1, "300 in array<u8> rejected");
        assert_eq!(decode(b"[1, -1]", U8), 1, "-1 in array<u8> rejected");
        assert_eq!(decode(b"[200]", I8), 1, "200 in array<i8> rejected");
        assert_eq!(decode(b"[0, 255]", U8), 0, "in-range array<u8> ok");
        assert_eq!(decode(b"[-128, 127]", I8), 0, "in-range array<i8> ok");
        // array<u64>: the full range decodes; u64::MAX+1 overflows and a negative is rejected.
        assert_eq!(decode(b"[0, 9223372036854775808, 18446744073709551615]", U64), 0, "full-range array<u64> ok");
        assert_eq!(decode(b"[18446744073709551616]", U64), 1, "u64::MAX+1 in array<u64> rejected");
        assert_eq!(decode(b"[-1]", U64), 1, "-1 in array<u64> rejected");
    }

    #[test]
    fn json_decode_soa_u64_full_range() {
        // The indexed write site (`write_field_indexed`, reached via the SoA fill pass) must also
        // accept the full u64 range and reject overflow / negatives — the third of the three integer
        // write sites (Gate 1: same parse routing everywhere).
        let n = b"n";
        let descs = [JsonField { name_ptr: n.as_ptr(), name_len: 1, tag: 8, offset: 0 }]; // u64
        let read_u64 = |ptr: *const u8, off: usize| -> u64 {
            let mut b = [0u8; 8];
            // `ptr` is the decoded-buffer source, `b` a distinct local array — disjoint.
            unsafe { std::ptr::copy_nonoverlapping(ptr.add(off), b.as_mut_ptr(), 8) };
            u64::from_le_bytes(b)
        };
        // Two rows: i64::MAX+1 and u64::MAX — both representable only on the full-range path.
        let src = br#"[{"n":9223372036854775808},{"n":18446744073709551615}]"#;
        let arena = align_rt_arena_begin();
        let mut out = AlignStr { ptr: core::ptr::null_mut(), len: 0 };
        let rc = unsafe {
            align_rt_json_decode_soa(src.as_ptr(), src.len() as i64, descs.as_ptr(), 1, arena, &mut out, core::ptr::null(), 0, 0)
        };
        assert_eq!(rc, 0, "full-range u64 rows must decode");
        assert_eq!(out.len, 2, "two rows");
        let (cols, _, _) = soa_layout(&[8], 2).unwrap();
        assert_eq!(read_u64(out.ptr, cols[0].0), 1u64 << 63, "row 0 = i64::MAX+1");
        assert_eq!(read_u64(out.ptr, cols[0].0 + 8), u64::MAX, "row 1 = u64::MAX");
        // u64::MAX + 1 overflows → reject; a negative is rejected.
        let overflow = br#"[{"n":18446744073709551616}]"#;
        assert_eq!(
            unsafe { align_rt_json_decode_soa(overflow.as_ptr(), overflow.len() as i64, descs.as_ptr(), 1, arena, &mut out, core::ptr::null(), 0, 0) },
            1
        );
        let negative = br#"[{"n":-1}]"#;
        assert_eq!(
            unsafe { align_rt_json_decode_soa(negative.as_ptr(), negative.len() as i64, descs.as_ptr(), 1, arena, &mut out, core::ptr::null(), 0, 0) },
            1
        );
        unsafe { align_rt_arena_end(arena) };
    }

    #[test]
    fn phf_hash_matches_codegen() {
        // The same pinned value as `align_codegen_llvm`'s `phf_hash_is_pinned` test and
        // `align_hash`'s `phf_pinned_vector` — all three call the one `wyhash`, so this is now a
        // structural identity (a canary against an accidental algorithm/seed edit). If these ever
        // diverge, the compile-time perfect-hash table would route JSON keys to wrong slots.
        assert_eq!(json_phf_hash(b"score", 0), 0x1300_a50c_fadb_78d9);
    }

    #[test]
    fn find_field_via_phf_matches_linear_scan() {
        // Build descriptors for a few names, then check the PHF lookup agrees with a linear scan for
        // both known keys and an unknown key (which both must report as absent → skipped).
        let names = [b"id".as_slice(), b"score", b"age"];
        let descs: Vec<JsonField> =
            names.iter().map(|n| JsonField { name_ptr: n.as_ptr(), name_len: n.len() as i64, tag: 0, offset: 0 }).collect();
        // A hand-built collision-free table (m=4, seed found by scanning) — mirrors `build_phf`.
        let m = 4usize;
        let mut seed = 0u64;
        let slots = loop {
            let mut s = vec![-1i32; m];
            let mut ok = true;
            for (i, n) in names.iter().enumerate() {
                let slot = (json_phf_hash(n, seed) & (m as u64 - 1)) as usize;
                if s[slot] != -1 { ok = false; break; }
                s[slot] = i as i32;
            }
            if ok { break s; }
            seed += 1;
        };
        for (i, n) in names.iter().enumerate() {
            assert_eq!(unsafe { find_field(&descs, n, Some(&slots), seed) }, Some(i));
        }
        assert_eq!(unsafe { find_field(&descs, b"unknown", Some(&slots), seed) }, None);
        // And the linear-scan fallback (no table) agrees.
        assert_eq!(unsafe { find_field(&descs, b"age", None, 0) }, Some(2));
        assert_eq!(unsafe { find_field(&descs, b"nope", None, 0) }, None);
    }

    #[test]
    fn arena_alloc_is_stable_across_chunks() {
        let a = align_rt_arena_begin();
        // Many allocations spanning multiple chunks; earlier pointers stay writable.
        let first = unsafe { align_rt_arena_alloc(a, 8, 8) } as *mut i64;
        unsafe { *first = 42 };
        for _ in 0..50_000 {
            let p = unsafe { align_rt_arena_alloc(a, 8, 8) } as *mut i64;
            unsafe { *p = 1 };
        }
        assert_eq!(unsafe { *first }, 42, "earlier allocation must remain valid");
        unsafe { align_rt_arena_end(a) };
    }

    #[test]
    fn arena_alloc_rejects_negative_or_oversized_size_and_align() {
        // A negative `size`/`align` (or one that overflows `usize` on a hypothetical narrower
        // target) must not wrap into a huge `usize` via a raw `as` cast — it must return null
        // instead, like every other runtime FFI boundary that validates its size arguments.
        let a = align_rt_arena_begin();
        assert!(unsafe { align_rt_arena_alloc(a, -1, 8) }.is_null(), "negative size must yield null");
        assert!(unsafe { align_rt_arena_alloc(a, 8, -1) }.is_null(), "negative align must yield null");
        assert!(
            unsafe { align_rt_arena_alloc(a, i64::MIN, i64::MIN) }.is_null(),
            "i64::MIN size/align must yield null, not panic or wrap"
        );
        // A null arena handle must not be dereferenced.
        assert!(
            unsafe { align_rt_arena_alloc(core::ptr::null_mut(), 8, 8) }.is_null(),
            "a null arena handle must yield null"
        );
        // `align` must be a nonzero power of two — `Arena::alloc`'s aligned-address bit-trick is
        // UB otherwise.
        assert!(unsafe { align_rt_arena_alloc(a, 8, 0) }.is_null(), "align 0 must yield null");
        assert!(unsafe { align_rt_arena_alloc(a, 8, 3) }.is_null(), "a non-power-of-two align must yield null");
        // A normal allocation still works afterwards (the guard doesn't corrupt arena state).
        let p = unsafe { align_rt_arena_alloc(a, 8, 8) } as *mut i64;
        assert!(!p.is_null());
        unsafe { *p = 7 };
        assert_eq!(unsafe { *p }, 7);
        unsafe { align_rt_arena_end(a) };
    }

    #[test]
    fn soa_layout_matches_codegen_formula() {
        // start_0 = 0; start_j = align_up(start_{j-1} + n*size_{j-1}, size_j); total = end of last.
        // widths [1, 8] (bool, i64), n = 2: col0 @0, align_up(0+2,8)=8 → col1 @8, total = 8+16 = 24.
        let (cols, total, max_align) = soa_layout(&[1, 8], 2).unwrap();
        assert_eq!(cols, vec![(0, 1), (8, 8)]);
        assert_eq!(total, 24);
        assert_eq!(max_align, 8);
        // widths [8, 1, 8], n = 3: col0 @0, col1 @24 (1-byte, no align), col2 align_up(24+3,8)=32.
        let (cols, total, _) = soa_layout(&[8, 1, 8], 3).unwrap();
        assert_eq!(cols, vec![(0, 8), (24, 1), (32, 8)]);
        assert_eq!(total, 32 + 24);
        // A pathological row count × width overflows `usize` → None (no under-allocation).
        assert!(soa_layout(&[8], usize::MAX).is_none());
    }

    #[test]
    fn json_decode_soa_fills_columns() {
        // Two fields (`active: bool`, `pay: i64`) in declaration order; decode a 2-row array directly
        // into columns and verify each column holds the right values at the `soa_layout` offsets.
        let active = b"active";
        let pay = b"pay";
        let descs = [
            JsonField { name_ptr: active.as_ptr(), name_len: active.len() as i64, tag: (1 << 8) | 1, offset: 0 },
            JsonField { name_ptr: pay.as_ptr(), name_len: pay.len() as i64, tag: 8, offset: 0 },
        ];
        let src = br#"[{"active":true,"pay":10},{"active":false,"pay":20}]"#;
        let arena = align_rt_arena_begin();
        let mut out = AlignStr { ptr: core::ptr::null_mut(), len: 0 };
        let rc = unsafe {
            align_rt_json_decode_soa(src.as_ptr(), src.len() as i64, descs.as_ptr(), 2, arena, &mut out, core::ptr::null(), 0, 0)
        };
        assert_eq!(rc, 0, "valid input must decode");
        assert_eq!(out.len, 2, "two rows");
        let (cols, _, _) = soa_layout(&[1, 8], 2).unwrap();
        // active column (width 1) at cols[0].0: [true, false].
        assert_eq!(unsafe { *out.ptr.add(cols[0].0) }, 1);
        assert_eq!(unsafe { *out.ptr.add(cols[0].0 + 1) }, 0);
        // pay column (width 8) at cols[1].0: [10, 20] as little-endian i64.
        let read_i64 = |off: usize| -> i64 {
            let mut b = [0u8; 8];
            // `out.ptr` is the decoded-buffer source, `b` a distinct local array — disjoint.
            unsafe { std::ptr::copy_nonoverlapping(out.ptr.add(off), b.as_mut_ptr(), 8) };
            i64::from_le_bytes(b)
        };
        assert_eq!(read_i64(cols[1].0), 10);
        assert_eq!(read_i64(cols[1].0 + 8), 20);
        unsafe { align_rt_arena_end(arena) };
    }

    #[test]
    fn json_decode_soa_rejects_bad_input() {
        let active = b"active";
        let pay = b"pay";
        let descs = [
            JsonField { name_ptr: active.as_ptr(), name_len: active.len() as i64, tag: (1 << 8) | 1, offset: 0 },
            JsonField { name_ptr: pay.as_ptr(), name_len: pay.len() as i64, tag: 8, offset: 0 },
        ];
        let arena = align_rt_arena_begin();
        let mut out = AlignStr { ptr: core::ptr::null_mut(), len: 0 };
        // Missing a declared field (`pay`) → strict decode rejects in the fill pass.
        let missing = br#"[{"active":true}]"#;
        assert_eq!(
            unsafe { align_rt_json_decode_soa(missing.as_ptr(), missing.len() as i64, descs.as_ptr(), 2, arena, &mut out, core::ptr::null(), 0, 0) },
            1
        );
        // Unterminated array → structural validation rejects in the count pass.
        let unterminated = br#"[{"active":true,"pay":1}"#;
        assert_eq!(
            unsafe { align_rt_json_decode_soa(unterminated.as_ptr(), unterminated.len() as i64, descs.as_ptr(), 2, arena, &mut out, core::ptr::null(), 0, 0) },
            1
        );
        // Empty array → ok, zero rows, null buffer (allocates nothing).
        let empty = b"[]";
        let mut out2 = AlignStr { ptr: core::ptr::null_mut(), len: 7 };
        assert_eq!(
            unsafe { align_rt_json_decode_soa(empty.as_ptr(), empty.len() as i64, descs.as_ptr(), 2, arena, &mut out2, core::ptr::null(), 0, 0) },
            0
        );
        assert_eq!(out2.len, 0);
        assert!(out2.ptr.is_null());
        unsafe { align_rt_arena_end(arena) };
    }

    #[test]
    fn json_struct_array_speculative_duplicate_key_is_strict() {
        // The strict `json.decode` contract (docs/open-questions.md "JSON two-stage SIMD decode" /
        // "Duplicate-key semantics"): every declared field appears exactly once; a duplicate is a
        // decode `Err`, never a silent last-wins. The fallback path already enforced this; the gap was
        // the Mison *speculative* fast path, where a duplicate of a declared field landing at a colon
        // position the learned pattern treats as *unqueried* (a projected-away slot) was skipped and
        // never re-detected. All records here are decoded via `align_rt_json_decode_struct_array`,
        // whose second-and-later records take the speculative path when their colon count matches the
        // pattern learned from the first record.
        fn decode(src: &[u8], descs: &[JsonField], esz: i64) -> (i32, i64, Vec<u8>) {
            let mut out = AlignStr { ptr: core::ptr::null_mut(), len: 0 };
            let rc = unsafe {
                align_rt_json_decode_struct_array(
                    src.as_ptr(),
                    src.len() as i64,
                    descs.as_ptr(),
                    descs.len() as i64,
                    esz,
                    &mut out,
                    core::ptr::null(),
                    0,
                    0,
                )
            };
            let mut buf = Vec::new();
            if rc == 0 && !out.ptr.is_null() {
                for j in 0..(out.len as usize) * esz as usize {
                    buf.push(unsafe { *out.ptr.add(j) });
                }
            }
            if !out.ptr.is_null() {
                unsafe { align_rt_free(out.ptr as *mut u8) };
            }
            (rc, out.len, buf)
        }
        let read_i64 = |buf: &[u8], off: usize| -> i64 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[off..off + 8]);
            i64::from_le_bytes(b)
        };

        let a = b"a";
        let one = [JsonField { name_ptr: a.as_ptr(), name_len: 1, tag: 8, offset: 0 }]; // a: u64

        // REPRODUCTION: record 1 (`{"a":1,"x":9}`) learns the pattern `[a, <unqueried x>]` (colon
        // count 2). Record 2 (`{"a":1,"a":2}`) also has 2 colons, so speculation runs; its second
        // colon is a duplicate `a` in the slot the pattern learned as unqueried. Before the fix this
        // was silently accepted (rc 0, last-wins `a=2`); it must be a decode error.
        assert_eq!(
            decode(br#"[{"a":1,"x":9},{"a":1,"a":2}]"#, &one, 8).0,
            1,
            "duplicate of a declared field at an unqueried pattern position must error on the fast path"
        );

        // A duplicate at a *queried* pattern position stays rejected too (the key verify at that
        // ordinal fails → fallback → duplicate error). Two declared fields so both colons are queried.
        let ab = [
            JsonField { name_ptr: b"a".as_ptr(), name_len: 1, tag: 8, offset: 0 },
            JsonField { name_ptr: b"b".as_ptr(), name_len: 1, tag: 8, offset: 8 },
        ];
        assert_eq!(
            decode(br#"[{"a":1,"b":2},{"a":1,"a":2}]"#, &ab, 16).0,
            1,
            "duplicate at a queried position must error"
        );

        // REGRESSION (no duplicates, projection rail): declaring only `a` while each record carries a
        // *different* undeclared key still decodes — the speculative path continues on an undeclared
        // key (find_field → None), so structural variation among undeclared keys does not force a
        // fallback (fast-path usage is preserved). Values must be exactly [1, 2, 3].
        let (rc, n, buf) = decode(br#"[{"a":1,"x":9},{"a":2,"y":8},{"a":3,"z":7}]"#, &one, 8);
        assert_eq!((rc, n), (0, 3), "no-duplicate projection input decodes to three rows");
        assert_eq!([read_i64(&buf, 0), read_i64(&buf, 8), read_i64(&buf, 16)], [1, 2, 3]);

        // REGRESSION (full decode, no unqueried slots): every colon is a declared field, so there is
        // no added cost and duplicates are caught by the queried-position verify.
        let (rc, n, buf) = decode(br#"[{"a":1,"b":10},{"a":2,"b":20}]"#, &ab, 16);
        assert_eq!((rc, n), (0, 2), "full-decode input still decodes");
        assert_eq!([read_i64(&buf, 0), read_i64(&buf, 16)], [1, 2], "a column");
        assert_eq!([read_i64(&buf, 8), read_i64(&buf, 24)], [10, 20], "b column");
    }

    #[test]
    fn str_predicates_match_byte_semantics() {
        // Drive the FFI entry points exactly as codegen does ({ptr,len} pairs), and check each
        // against the equivalent Rust `&[u8]` predicate across a spread of cases incl. UTF-8,
        // empty needles, and over-long needles.
        let contains = |h: &[u8], n: &[u8]| unsafe {
            align_rt_str_contains(h.as_ptr(), h.len() as i64, n.as_ptr(), n.len() as i64)
        };
        let starts = |h: &[u8], n: &[u8]| unsafe {
            align_rt_str_starts_with(h.as_ptr(), h.len() as i64, n.as_ptr(), n.len() as i64)
        };
        let ends = |h: &[u8], n: &[u8]| unsafe {
            align_rt_str_ends_with(h.as_ptr(), h.len() as i64, n.as_ptr(), n.len() as i64)
        };
        let cases: &[(&[u8], &[u8])] = &[
            (b"hello, align", b"align"),
            (b"hello, align", b"hello"),
            (b"hello, align", b"xyz"),
            (b"abc", b""),          // empty needle: always present / prefix / suffix
            (b"", b""),             // both empty
            (b"abc", b"abcd"),      // needle longer than haystack
            (b"abc", b"abc"),       // whole-string
            ("café みかん".as_bytes(), "みかん".as_bytes()),
            ("café みかん".as_bytes(), "café".as_bytes()),
            ("café みかん".as_bytes(), "ん".as_bytes()),
        ];
        for (h, n) in cases {
            // Independent reference: an empty needle is always present; otherwise a sliding-window
            // scan (`h.windows` is empty when the needle is longer than the haystack → false).
            let expect_contains = n.is_empty() || h.windows(n.len()).any(|w| w == *n);
            assert_eq!(contains(h, n), expect_contains as i32, "contains({h:?}, {n:?})");
            assert_eq!(starts(h, n), h.starts_with(n) as i32, "starts_with({h:?}, {n:?})");
            assert_eq!(ends(h, n), h.ends_with(n) as i32, "ends_with({h:?}, {n:?})");

            // `find` returns the first index or -1; an empty needle is 0. Reference: a window scan.
            let find = unsafe {
                align_rt_str_find(h.as_ptr(), h.len() as i64, n.as_ptr(), n.len() as i64)
            };
            let expect_find: i64 = if n.is_empty() {
                0
            } else {
                h.windows(n.len()).position(|w| w == *n).map_or(-1, |i| i as i64)
            };
            assert_eq!(find, expect_find, "find({h:?}, {n:?})");

            // `rfind` is the last occurrence; an empty needle is the end (`hlen`).
            let rfind = unsafe {
                align_rt_str_rfind(h.as_ptr(), h.len() as i64, n.as_ptr(), n.len() as i64)
            };
            let expect_rfind: i64 = if n.is_empty() {
                h.len() as i64
            } else {
                h.windows(n.len()).rposition(|w| w == *n).map_or(-1, |i| i as i64)
            };
            assert_eq!(rfind, expect_rfind, "rfind({h:?}, {n:?})");
        }
    }

    /// Differential oracle for the generic `str` search ops, in the same discipline as the JSON
    /// structural-index SIMD tests (`json_decode_index_simd_matches_scalar_oracle`).
    ///
    /// `contains`/`find`/`rfind` are backed by `memchr::memmem`, whose substring search ships an
    /// AVX2 path (x86_64), a NEON path (aarch64 baseline), and a scalar fallback, selected by
    /// runtime feature detection — the same first-byte-scan-then-verify technique the JSON index
    /// uses, in its reference form. This locks whichever path the host CPU selects against an
    /// independent naive scalar oracle across the edges that break SIMD substring search: a needle
    /// straddling the 64-byte SIMD block boundary, prefilter decoys that must be verified and
    /// rejected before the real match, needle lengths 0/1/large, multibyte UTF-8, overlapping
    /// repeats, tail matches, a multi-KB haystack, and a deterministic randomized cross-check.
    /// `starts_with`/`ends_with` (deliberately scalar `==`/`memcmp`, bounded to the needle length —
    /// no worthwhile SIMD lever) ride along against the same corpus.
    #[test]
    fn str_search_simd_matches_scalar_oracle() {
        // FFI entry points, driven exactly as codegen does ({ptr,len} pairs).
        let contains = |h: &[u8], n: &[u8]| unsafe {
            align_rt_str_contains(h.as_ptr(), h.len() as i64, n.as_ptr(), n.len() as i64)
        };
        let find = |h: &[u8], n: &[u8]| unsafe {
            align_rt_str_find(h.as_ptr(), h.len() as i64, n.as_ptr(), n.len() as i64)
        };
        let rfind = |h: &[u8], n: &[u8]| unsafe {
            align_rt_str_rfind(h.as_ptr(), h.len() as i64, n.as_ptr(), n.len() as i64)
        };
        let starts = |h: &[u8], n: &[u8]| unsafe {
            align_rt_str_starts_with(h.as_ptr(), h.len() as i64, n.as_ptr(), n.len() as i64)
        };
        let ends = |h: &[u8], n: &[u8]| unsafe {
            align_rt_str_ends_with(h.as_ptr(), h.len() as i64, n.as_ptr(), n.len() as i64)
        };

        // Independent scalar oracles — a naive sliding-window scan, no memchr involved.
        fn oracle_find(h: &[u8], n: &[u8]) -> i64 {
            if n.is_empty() {
                return 0;
            }
            if n.len() > h.len() {
                return -1;
            }
            h.windows(n.len()).position(|w| w == n).map_or(-1, |i| i as i64)
        }
        fn oracle_rfind(h: &[u8], n: &[u8]) -> i64 {
            if n.is_empty() {
                return h.len() as i64;
            }
            if n.len() > h.len() {
                return -1;
            }
            h.windows(n.len()).rposition(|w| w == n).map_or(-1, |i| i as i64)
        }

        let check = |h: &[u8], n: &[u8]| {
            let ef = oracle_find(h, n);
            let er = oracle_rfind(h, n);
            assert_eq!(find(h, n), ef, "find(hlen {}, needle {:?})", h.len(), n);
            assert_eq!(rfind(h, n), er, "rfind(hlen {}, needle {:?})", h.len(), n);
            assert_eq!(
                contains(h, n),
                (n.is_empty() || ef >= 0) as i32,
                "contains(hlen {}, needle {:?})",
                h.len(),
                n
            );
            assert_eq!(starts(h, n), h.starts_with(n) as i32, "starts_with(hlen {}, needle {:?})", h.len(), n);
            assert_eq!(ends(h, n), h.ends_with(n) as i32, "ends_with(hlen {}, needle {:?})", h.len(), n);
        };

        // (1) Boundary-padding sweep: place a distinctive needle at every offset 40..96 of a
        // 160-byte buffer so it straddles the 64-byte SIMD block boundary, and scatter the needle's
        // first byte before it so the SIMD prefilter fires on non-matches and must verify+reject
        // them before reaching the real match. Needle bytes stay in A..=W (65..=87), never the '.'
        // (46) pad, so no window accidentally matches except the planted one.
        for needle_len in [1usize, 2, 3, 4, 7, 8, 15, 16, 17] {
            let needle: Vec<u8> = (0..needle_len).map(|k| b'A' + (k as u8 % 23)).collect();
            let mut buf = vec![b'.'; 160];
            for off in 40..96 {
                buf.fill(b'.'); // reset the reused buffer between offsets
                for j in (0..off).step_by(5) {
                    buf[j] = needle[0]; // prefilter decoy
                }
                buf[off..off + needle_len].copy_from_slice(&needle);
                check(&buf, &needle);
            }
        }

        // (2) Degenerate lengths: empty needle, needle longer than haystack, whole-string, single byte.
        check(b"", b"");
        check(b"abc", b"");
        check(b"abc", b"abcd");
        check(b"abc", b"abc");
        check(b"a", b"a");
        check(b"a", b"b");

        // (3) Multibyte UTF-8 haystack/needles straddling boundaries (repeat to cross 64B blocks).
        let s = "café みかん 🍎 résumé ｱｲｳｴｵ ".repeat(6);
        for n in ["みかん", "🍎", "é", "café", "ｳｴ", "résumé", "🍏 nope", " "] {
            check(s.as_bytes(), n.as_bytes());
        }

        // (4) Overlapping repeats and tail matches.
        let run = vec![b'a'; 200];
        check(&run, b"aa");
        check(&run, b"aaa");
        check(&run, &run); // needle == whole haystack
        let mut tail = vec![b'x'; 300];
        tail[297..300].copy_from_slice(b"END");
        check(&tail, b"END");
        check(&tail, b"ND");
        check(&tail, b"D");

        // (5) Multi-KB haystack: single match near the end, a miss, and a dense single byte.
        let mut long = vec![b'z'; 8192];
        long[8000..8005].copy_from_slice(b"MATCH");
        check(&long, b"MATCH");
        check(&long, b"ABSENT");
        check(&long, b"z"); // dense: find -> 0, rfind -> 8191

        // (6) Deterministic randomized cross-check over a 3-symbol alphabet (dependency-free
        // xorshift). Short needles over a boundary-spanning haystack length maximize candidate
        // density, exercising the SIMD verify path hard.
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut rng = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..3000 {
            let hlen = (rng() % 130) as usize;
            let nlen = (rng() % 6) as usize;
            let h: Vec<u8> = (0..hlen).map(|_| b'a' + (rng() % 3) as u8).collect();
            let n: Vec<u8> = (0..nlen).map(|_| b'a' + (rng() % 3) as u8).collect();
            check(&h, &n);
        }
    }

    #[test]
    fn str_eq_ignore_case_matches_reference() {
        let eq = |a: &[u8], b: &[u8]| unsafe {
            align_rt_str_eq_ignore_case(a.as_ptr(), a.len() as i64, b.as_ptr(), b.len() as i64)
        };
        let cases: &[(&[u8], &[u8])] = &[
            (b"Content-Type", b"content-type"),
            (b"GET", b"get"),
            (b"abc", b"abd"),
            (b"abc", b"abcd"),  // different length
            (b"", b""),
            (b"MiXeD123", b"mixed123"),
            ("café".as_bytes(), "CAFÉ".as_bytes()), // non-ASCII 'é' compares exactly → not equal
        ];
        for (a, b) in cases {
            assert_eq!(eq(a, b), a.eq_ignore_ascii_case(b) as i32, "eq_ignore_case({a:?}, {b:?})");
        }
    }

    #[test]
    fn str_trim_matches_byte_semantics_and_aliases() {
        // Each trim must equal the equivalent `[u8]::trim_ascii*` and return a sub-view that still
        // points *into* the original buffer (no allocation).
        let view = |s: &AlignStr| -> &[u8] {
            if s.len == 0 { &[] } else { unsafe { safe_slice(s.ptr, s.len) } }
        };
        let cases: &[&[u8]] = &[
            b"  hi  ",
            b"abc",            // no whitespace
            b"   ",            // all whitespace
            b"",               // empty
            b"\t\n foo \r\x0b",
            b"x ",
            b" x",
            "  café  ".as_bytes(), // multi-byte content is preserved; only ASCII ws is stripped
        ];
        for &h in cases {
            let both = unsafe { align_rt_str_trim(h.as_ptr(), h.len() as i64) };
            let start = unsafe { align_rt_str_trim_start(h.as_ptr(), h.len() as i64) };
            let end = unsafe { align_rt_str_trim_end(h.as_ptr(), h.len() as i64) };
            assert_eq!(view(&both), h.trim_ascii(), "trim({h:?})");
            assert_eq!(view(&start), h.trim_ascii_start(), "trim_start({h:?})");
            assert_eq!(view(&end), h.trim_ascii_end(), "trim_end({h:?})");
            // The result must alias the input: its bytes lie within the original range.
            let base = h.as_ptr() as usize;
            for s in [&both, &start, &end] {
                if s.len > 0 {
                    let p = s.ptr as usize;
                    assert!(p >= base && p + s.len as usize <= base + h.len(), "trim view aliases input");
                }
            }
        }
    }

    // --- `task_group` `tg_wait` (pool-backed, caller-participating work-claiming) ---

    /// A test trampoline: read `i64` from `env`, write `2*env` into `slot`, succeed. Matches the
    /// `(thunk, env, slot, err_slot) -> i32` ABI the codegen emits for a spawned closure.
    extern "C" fn double_tramp(_thunk: *const u8, env: *mut u8, slot: *mut u8, _err: *mut u8) -> i32 {
        unsafe {
            let v = *(env as *const i64);
            *(slot as *mut i64) = v * 2;
        }
        0
    }

    /// A failing trampoline: write the code from `env` into `err_slot`, return 1 (errored).
    extern "C" fn err_tramp(_thunk: *const u8, env: *mut u8, _slot: *mut u8, err: *mut u8) -> i32 {
        unsafe { *(err as *mut i64) = *(env as *const i64) };
        1
    }

    /// Register `n` `double` tasks (env = index) into a fresh group; return the group and slot ptrs.
    fn build_double_group(n: i64) -> (*mut TaskGroup, Vec<*mut u8>) {
        let tg = align_rt_tg_begin();
        let mut slots = Vec::new();
        for i in 0..n {
            let env = unsafe { align_rt_tg_alloc(tg, 8, 8) };
            unsafe { *(env as *mut i64) = i };
            let slot = unsafe { align_rt_tg_alloc(tg, 8, 8) };
            unsafe { align_rt_tg_register(tg, double_tramp, std::ptr::null(), env, slot, std::ptr::null_mut()) };
            slots.push(slot);
        }
        (tg, slots)
    }

    #[test]
    fn tg_wait_runs_all_tasks_pool_backed() {
        // Many short tasks: every slot must be written (all tasks ran), and the join must complete
        // (no deadlock) before we read the region. Repeat so the pool is exercised warm and cold.
        for &n in &[1i64, 2, 5, 64, 1000] {
            let (tg, slots) = build_double_group(n);
            let err = unsafe { align_rt_tg_wait(tg) };
            assert!(err.is_null(), "n={n}: no task errored");
            for (i, s) in slots.iter().enumerate() {
                assert_eq!(unsafe { *(*s as *const i64) }, (i as i64) * 2, "n={n} task {i}");
            }
            unsafe { align_rt_tg_end(tg) };
        }
    }

    #[test]
    fn tg_wait_returns_first_errored_slot_by_index() {
        // Tasks 3 and 7 error (codes 30, 70); `wait` must return the lowest-index errored slot (3).
        let tg = align_rt_tg_begin();
        let mut err_slots = Vec::new();
        for i in 0..10i64 {
            let env = unsafe { align_rt_tg_alloc(tg, 8, 8) };
            let slot = unsafe { align_rt_tg_alloc(tg, 8, 8) };
            let err_slot = unsafe { align_rt_tg_alloc(tg, 8, 8) };
            let (tramp, code): (extern "C" fn(*const u8, *mut u8, *mut u8, *mut u8) -> i32, i64) =
                if i == 3 || i == 7 { (err_tramp, i * 10) } else { (double_tramp, 0) };
            unsafe { *(env as *mut i64) = code };
            unsafe { align_rt_tg_register(tg, tramp, std::ptr::null(), env, slot, err_slot) };
            err_slots.push(err_slot);
        }
        let err = unsafe { align_rt_tg_wait(tg) };
        assert_eq!(err, err_slots[3], "lowest-index errored slot");
        assert_eq!(unsafe { *(err as *const i64) }, 30);
        unsafe { align_rt_tg_end(tg) };
    }

    /// A nested trampoline: `env` holds a base; open a *sub* `task_group` of 16 `double` tasks over
    /// `base..base+16`, `wait` on it (re-entering `tg_wait` on a pool worker), sum the results into
    /// `slot`. Exercises the finite-pool re-entrancy path the caller-participating design protects.
    extern "C" fn nested_tramp(_thunk: *const u8, env: *mut u8, slot: *mut u8, _err: *mut u8) -> i32 {
        let base = unsafe { *(env as *const i64) };
        let sub = align_rt_tg_begin();
        let mut subslots = Vec::new();
        for j in 0..16i64 {
            let e = unsafe { align_rt_tg_alloc(sub, 8, 8) };
            unsafe { *(e as *mut i64) = base + j };
            let s = unsafe { align_rt_tg_alloc(sub, 8, 8) };
            unsafe { align_rt_tg_register(sub, double_tramp, std::ptr::null(), e, s, std::ptr::null_mut()) };
            subslots.push(s);
        }
        let err = unsafe { align_rt_tg_wait(sub) };
        assert!(err.is_null());
        let sum: i64 = subslots.iter().map(|s| unsafe { *(*s as *const i64) }).sum();
        unsafe { *(slot as *mut i64) = sum };
        unsafe { align_rt_tg_end(sub) };
        0
    }

    #[test]
    fn tg_wait_nested_task_groups_do_not_deadlock() {
        // Enough outer tasks (> worker count) that some nested waits necessarily run on busy pool
        // workers. If the finite pool could deadlock on re-entry, this test would hang (CI timeout);
        // it passing proves the caller drains its own group. Each outer task's sum is
        // sum_{j=0}^{15} 2*(base+j) = 32*base + 240.
        let n = 64i64;
        let tg = align_rt_tg_begin();
        let mut slots = Vec::new();
        for base in 0..n {
            let env = unsafe { align_rt_tg_alloc(tg, 8, 8) };
            unsafe { *(env as *mut i64) = base };
            let slot = unsafe { align_rt_tg_alloc(tg, 8, 8) };
            unsafe { align_rt_tg_register(tg, nested_tramp, std::ptr::null(), env, slot, std::ptr::null_mut()) };
            slots.push(slot);
        }
        let err = unsafe { align_rt_tg_wait(tg) };
        assert!(err.is_null());
        for (base, s) in slots.iter().enumerate() {
            assert_eq!(unsafe { *(*s as *const i64) }, 32 * base as i64 + 240, "outer base={base}");
        }
        unsafe { align_rt_tg_end(tg) };
    }

    // --- std.fs Slice 3 -----------------------------------------------------------------------

    /// A unique temp path under the OS temp dir, cleaned up by the caller.
    fn tmp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("align-rt-fs-{}-{name}", std::process::id()))
    }

    fn view_of(s: &str) -> (*const u8, i64) {
        (s.as_ptr(), s.len() as i64)
    }

    #[test]
    fn fs_write_exists_remove_round_trip() {
        let path = tmp_path("wer");
        let ps = path.display().to_string();
        let (pp, pl) = view_of(&ps);
        let data = b"payload-123";
        // Not present yet.
        assert_eq!(unsafe { align_rt_fs_exists(pp, pl) }, 0);
        // Write, then it exists and reads back exactly.
        assert_eq!(unsafe { align_rt_fs_write_file(pp, pl, data.as_ptr(), data.len() as i64) }, 0);
        assert_eq!(unsafe { align_rt_fs_exists(pp, pl) }, 1);
        assert_eq!(std::fs::read(&path).unwrap(), data);
        // Remove, then it is gone.
        assert_eq!(unsafe { align_rt_fs_remove(pp, pl) }, 0);
        assert_eq!(unsafe { align_rt_fs_exists(pp, pl) }, 0);
        // Removing a missing file maps ENOENT -> NotFound.
        assert_eq!(unsafe { align_rt_fs_remove(pp, pl) }, AL_NOT_FOUND);
    }

    #[test]
    fn fs_write_file_empty_creates_empty() {
        let path = tmp_path("empty");
        let ps = path.display().to_string();
        let (pp, pl) = view_of(&ps);
        assert_eq!(unsafe { align_rt_fs_write_file(pp, pl, std::ptr::null(), 0) }, 0);
        assert_eq!(std::fs::read(&path).unwrap(), b"");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fs_read_dir_lists_and_deep_frees() {
        let dir = tmp_path("rd");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..4 {
            std::fs::write(dir.join(format!("e{i}")), b"x").unwrap();
        }
        let ds = dir.display().to_string();
        let (pp, pl) = view_of(&ds);
        let mut out = AlignStr { ptr: std::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_dir(pp, pl, &mut out) }, 0);
        assert_eq!(out.len, 4, "four entries");
        // Each header entry owns a non-null name buffer with a plausible name.
        let hdr = out.ptr as *const AlignStr;
        for i in 0..out.len as usize {
            let e = unsafe { *hdr.add(i) };
            assert!(!e.ptr.is_null() && e.len > 0);
            let name = unsafe { safe_slice(e.ptr, e.len) };
            assert!(name.starts_with(b"e"), "entry name looks like eN");
        }
        // Deep free (each name + header) — under a leak sanitizer this proves no leak.
        unsafe { align_rt_free_string_array(out.ptr as *mut u8, out.len) };
        // Missing dir -> NotFound.
        let miss = tmp_path("rd-missing");
        let ms = miss.display().to_string();
        let (mp, ml) = view_of(&ms);
        let mut o2 = AlignStr { ptr: std::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_dir(mp, ml, &mut o2) }, AL_NOT_FOUND);
        assert!(o2.ptr.is_null());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fs_read_file_view_mmaps_and_arena_end_unmaps() {
        let path = tmp_path("view");
        let content = b"mmapped-file-view-content";
        std::fs::write(&path, content).unwrap();
        let ps = path.display().to_string();
        let (pp, pl) = view_of(&ps);
        let arena = align_rt_arena_begin();
        let mut out = AlignStr { ptr: std::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_file_view(pp, pl, arena, &mut out) }, 0);
        assert_eq!(out.len, content.len() as i64);
        let got = unsafe { safe_slice(out.ptr, out.len) };
        assert_eq!(got, content, "the view bytes match the file");
        // The mapping was registered on the arena for munmap at end.
        assert_eq!(unsafe { (*arena).maps.len() }, 1, "one mapping registered");
        // arena_end munmaps it (and frees the arena) — must not crash / double free.
        unsafe { align_rt_arena_end(arena) };
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fs_read_file_view_empty_file_is_empty_view() {
        let path = tmp_path("view-empty");
        std::fs::write(&path, b"").unwrap();
        let ps = path.display().to_string();
        let (pp, pl) = view_of(&ps);
        let arena = align_rt_arena_begin();
        let mut out = AlignStr { ptr: std::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_file_view(pp, pl, arena, &mut out) }, 0);
        assert_eq!(out.len, 0, "empty file -> empty view");
        assert_eq!(unsafe { (*arena).maps.len() }, 0, "no mapping for a zero-length file");
        unsafe { align_rt_arena_end(arena) };
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fs_read_file_view_missing_is_not_found() {
        let path = tmp_path("view-missing");
        let _ = std::fs::remove_file(&path);
        let ps = path.display().to_string();
        let (pp, pl) = view_of(&ps);
        let arena = align_rt_arena_begin();
        let mut out = AlignStr { ptr: std::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_file_view(pp, pl, arena, &mut out) }, AL_NOT_FOUND);
        assert!(out.ptr.is_null());
        unsafe { align_rt_arena_end(arena) };
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fs_read_file_view_proc_falls_back_to_copy() {
        // /proc/self/stat is a regular file with st_size 0 but real content — the fallback reads it
        // into arena memory (no mmap registered).
        let ps = "/proc/self/stat".to_string();
        let (pp, pl) = view_of(&ps);
        let arena = align_rt_arena_begin();
        let mut out = AlignStr { ptr: std::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_file_view(pp, pl, arena, &mut out) }, 0);
        assert!(out.len > 0, "the proc file has real content via the fallback");
        assert_eq!(unsafe { (*arena).maps.len() }, 0, "a size-0 special file is not mmapped");
        unsafe { align_rt_arena_end(arena) };
    }

    // --- UTF-8 validation (draft §7/§12: a `str`/`string` is always valid UTF-8) ---------------

    /// Differential-test every SIMD validator path against the scalar oracle (`std::str::from_utf8`),
    /// across the cases that break naive SIMD: isolated continuations, truncated multibyte sequences,
    /// overlong encodings, surrogates, out-of-range 4-byte leads, and sequences straddling the 16/32
    /// byte block boundaries + the zero-padded tail. Same discipline as the decode-index SIMD test.
    #[test]
    fn utf8_validate_simd_matches_scalar_oracle() {
        let check = |bytes: &[u8]| {
            let want = validate_utf8_scalar(bytes);
            assert_eq!(validate_utf8(bytes), want, "dispatch mismatch on {bytes:02x?}");
            #[cfg(target_arch = "x86_64")]
            if is_x86_feature_detected!("avx2") {
                assert_eq!(unsafe { validate_utf8_avx2(bytes) }, want, "avx2 mismatch on {bytes:02x?}");
            }
            #[cfg(target_arch = "aarch64")]
            assert_eq!(unsafe { validate_utf8_neon(bytes) }, want, "neon mismatch on {bytes:02x?}"); // baseline
        };

        let snippets: &[&[u8]] = &[
            b"",
            b"a",
            b"hello, world",
            "café — 日本語テスト 😀🎉".as_bytes(), // 2/3/4-byte chars mixed
            &[0x80],                   // isolated continuation
            &[0xBF],
            &[0xC2],                   // 2-byte lead, truncated
            &[0xC2, 0x41],             // 2-byte lead + non-continuation
            &[0xC0, 0x80],             // overlong 2 (C0)
            &[0xC1, 0xBF],             // overlong 2 (C1)
            &[0xC2, 0xA9],             // valid © U+00A9
            &[0xE0, 0xA0],             // 3-byte lead, truncated
            &[0xE0, 0x80, 0x80],       // overlong 3
            &[0xE0, 0x9F, 0x80],       // 3-byte 2nd byte too low (overlong)
            &[0xE0, 0xA0, 0x80],       // valid U+0800 (min 3-byte)
            &[0xED, 0xA0, 0x80],       // surrogate U+D800
            &[0xED, 0xBF, 0xBF],       // surrogate U+DFFF
            &[0xED, 0x9F, 0xBF],       // valid U+D7FF (just below surrogates)
            &[0xEE, 0x80, 0x80],       // valid U+E000 (just above surrogates)
            &[0xE2, 0x82, 0xAC],       // valid € U+20AC
            &[0xF0, 0x90, 0x80, 0x80], // valid U+10000 (min 4-byte)
            &[0xF0, 0x80, 0x80, 0x80], // overlong 4
            &[0xF0, 0x8F, 0x80, 0x80], // 4-byte 2nd byte too low (overlong)
            &[0xF4, 0x8F, 0xBF, 0xBF], // valid U+10FFFF (max)
            &[0xF4, 0x90, 0x80, 0x80], // too large U+110000
            &[0xF5, 0x80, 0x80, 0x80], // 4-byte lead > F4
            &[0xF0, 0x90, 0x80],       // 4-byte truncated
            &[0xF8],                   // 5-byte lead (invalid in UTF-8)
            &[0xFF],                   // never valid
            &[0xC2, 0x80, 0xE0, 0xA0, 0x80, 0xF0, 0x90, 0x80, 0x80], // adjacent 2/3/4-byte chars
        ];
        for s in snippets {
            check(s);
        }
        // Embed each snippet at offsets that place its sequence across the 16/32-byte boundaries and
        // the tail — where SIMD carry / incompleteness bugs hide — both mid-buffer and at the end.
        for s in snippets {
            for pad in [0usize, 1, 13, 14, 15, 16, 17, 29, 30, 31, 32, 33, 62, 63, 64, 65] {
                let mut mid = vec![b'a'; pad];
                mid.extend_from_slice(s);
                mid.extend_from_slice(b"trailing bytes z");
                check(&mid);
                let mut end = vec![b'z'; pad];
                end.extend_from_slice(s); // snippet at the very end → incompleteness matters
                check(&end);
            }
        }

        // Randomized fuzz: a dependency-free LCG, reproducible. Raw random bytes biased toward the
        // UTF-8-significant ranges so malformed multibyte structure actually occurs.
        let mut state: u64 = 0x1234_5678_9abc_def1;
        let mut rng = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 33) as u32
        };
        for _ in 0..30_000 {
            let len = (rng() % 140) as usize;
            let bytes: Vec<u8> = (0..len)
                .map(|_| match rng() % 4 {
                    0 => (rng() % 0x80) as u8,        // ASCII
                    1 => 0x80 | (rng() % 0x40) as u8, // continuation
                    2 => 0xC0 | (rng() % 0x40) as u8, // lead
                    _ => rng() as u8,                 // anything
                })
                .collect();
            check(&bytes);
        }
        // Valid multibyte text, then a single corrupted byte.
        let alphabet: Vec<char> = "aZ0 é本😀€ß①".chars().collect();
        for _ in 0..5_000 {
            let nchars = (rng() % 40) as usize;
            let s: String = (0..nchars).map(|_| alphabet[(rng() as usize) % alphabet.len()]).collect();
            let mut b = s.into_bytes();
            check(&b);
            if !b.is_empty() {
                let i = (rng() as usize) % b.len();
                b[i] ^= (1 + (rng() % 255)) as u8;
                check(&b);
            }
        }
    }

    #[test]
    fn fs_read_file_rejects_non_utf8_content() {
        let dir = std::env::temp_dir();
        let uniq = format!("align-rt-utf8-{}-{:p}", std::process::id(), &dir as *const _);

        // Valid multibyte content larger than one read buffer (fast path) reads back intact.
        let good = dir.join(format!("{uniq}-good.txt"));
        let good_content = "日本語 test café 😀\n".repeat(5000).into_bytes();
        std::fs::write(&good, &good_content).unwrap();
        let gs = good.to_str().unwrap();
        let mut out = AlignStr { ptr: core::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_file(gs.as_ptr(), gs.len() as i64, &mut out) }, 0);
        let got = unsafe { safe_slice(out.ptr, out.len) }.to_vec();
        assert_eq!(got, good_content);
        unsafe { align_rt_free(out.ptr as *mut u8) };

        // A short valid multibyte file also accepts.
        let small = dir.join(format!("{uniq}-small.txt"));
        std::fs::write(&small, "é本".as_bytes()).unwrap();
        let ss = small.to_str().unwrap();
        let mut o2 = AlignStr { ptr: core::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_file(ss.as_ptr(), ss.len() as i64, &mut o2) }, 0);
        unsafe { align_rt_free(o2.ptr as *mut u8) };

        // Binary content (a 0..256 byte cycle: has 0xFF and lone continuations) → Error.Invalid, and
        // no buffer is handed out (the invariant: a `str`/`string` is always valid UTF-8).
        let bad = dir.join(format!("{uniq}-bad.bin"));
        let bad_content: Vec<u8> = (0..50_000u32).map(|i| (i % 256) as u8).collect();
        assert!(std::str::from_utf8(&bad_content).is_err());
        std::fs::write(&bad, &bad_content).unwrap();
        let bs = bad.to_str().unwrap();
        let mut o3 = AlignStr { ptr: core::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_file(bs.as_ptr(), bs.len() as i64, &mut o3) }, AL_INVALID);
        assert!(o3.ptr.is_null(), "no `str` view on invalid content");

        std::fs::remove_file(&good).ok();
        std::fs::remove_file(&small).ok();
        std::fs::remove_file(&bad).ok();
    }

    #[test]
    fn fs_read_file_view_rejects_non_utf8_content() {
        // Invalid content on the mmap path → Error.Invalid, mapping unmapped (none registered).
        let path = tmp_path("view-bad");
        let bad: Vec<u8> = (0..40_000u32).map(|i| (i % 256) as u8).collect();
        assert!(std::str::from_utf8(&bad).is_err());
        std::fs::write(&path, &bad).unwrap();
        let ps = path.display().to_string();
        let (pp, pl) = view_of(&ps);
        let arena = align_rt_arena_begin();
        let mut out = AlignStr { ptr: std::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_file_view(pp, pl, arena, &mut out) }, AL_INVALID);
        assert!(out.ptr.is_null());
        assert_eq!(unsafe { (*arena).maps.len() }, 0, "the invalid mapping was munmapped, not registered");
        unsafe { align_rt_arena_end(arena) };

        // Valid multibyte content maps fine and reads back intact.
        let good = tmp_path("view-good");
        let gc = "日本語テスト café 😀\n".repeat(3000).into_bytes();
        std::fs::write(&good, &gc).unwrap();
        let gs = good.display().to_string();
        let (gp, gl) = view_of(&gs);
        let arena2 = align_rt_arena_begin();
        let mut out2 = AlignStr { ptr: std::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_file_view(gp, gl, arena2, &mut out2) }, 0);
        assert_eq!(out2.len, gc.len() as i64);
        assert_eq!(unsafe { safe_slice(out2.ptr, out2.len) }, gc.as_slice());
        unsafe { align_rt_arena_end(arena2) };

        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&good).ok();
    }

    #[test]
    fn json_decode_rejects_non_utf8_input() {
        // A one-field struct `{ s: str }` — str field: kind 3, width 16, offset 0.
        let name = b"s";
        let descs = [JsonField { name_ptr: name.as_ptr(), name_len: 1, tag: (3 << 8) | 16, offset: 0 }];
        let decode = |src: &[u8]| -> i32 {
            let mut out = [0u8; 16];
            unsafe {
                align_rt_json_decode(src.as_ptr(), src.len() as i64, descs.as_ptr(), 1, out.as_mut_ptr(), 16, core::ptr::null(), 0, 0)
            }
        };
        // Valid: ASCII and multibyte string values decode.
        assert_eq!(decode(br#"{"s":"ok"}"#), 0);
        assert_eq!(decode("{\"s\":\"日本 café 😀\"}".as_bytes()), 0);
        // A raw continuation byte inside the string value makes the whole input non-UTF-8 → the
        // one-shot head check rejects before any `str` view into the input is handed out.
        let mut bad = b"{\"s\":\"o".to_vec();
        bad.push(0x80);
        bad.extend_from_slice(b"k\"}");
        assert!(std::str::from_utf8(&bad).is_err());
        assert_ne!(decode(&bad), 0, "non-UTF-8 input must not decode into a `str`");

        // The array decoder validates its input too.
        assert_eq!(decode(br#"{"s":"ok"}"#), 0); // sanity: the good path still decodes
        let mut arr = b"[1,2,".to_vec();
        arr.push(0xFF);
        arr.extend_from_slice(b"3]");
        let mut aout = AlignStr { ptr: core::ptr::null(), len: 0 };
        let tag = (1 << 16) | 8; // i64 elements (signed, width 8)
        assert_ne!(unsafe { align_rt_json_decode_array(arr.as_ptr(), arr.len() as i64, tag, &mut aout) }, 0);
        assert!(aout.ptr.is_null());
    }

    /// Rough throughput probe: the UTF-8 validation added at every `str`-returning I/O entry must cost on
    /// the order of a `memcpy` (it is a single linear pass), so decode/read paths degrade a few %, not
    /// materially. Prints SIMD-validate vs scalar-validate vs `memcpy` GB/s. `cargo test -p
    /// align_runtime -- --ignored --nocapture utf8_validate_throughput`.
    #[test]
    #[ignore]
    fn utf8_validate_throughput() {
        // ~64 MiB of realistic mostly-ASCII JSON text with some multibyte content.
        let unit = r#"{"name":"café 日本語 😀","id":123456,"active":true},"#;
        let n = (64 * 1024 * 1024) / unit.len();
        let mut buf = String::with_capacity(n * unit.len() + 2);
        buf.push('[');
        for _ in 0..n {
            buf.push_str(unit);
        }
        buf.push(']');
        let bytes = buf.as_bytes();
        let mb = bytes.len() as f64 / (1024.0 * 1024.0);
        let time = |mut f: Box<dyn FnMut() -> bool>| -> f64 {
            let mut best = f64::MAX;
            for _ in 0..20 {
                let t = std::time::Instant::now();
                std::hint::black_box(f());
                best = best.min(t.elapsed().as_secs_f64());
            }
            mb / 1024.0 / best // GB/s
        };
        let simd = time(Box::new(|| validate_utf8(bytes)));
        let scalar = time(Box::new(|| validate_utf8_scalar(bytes)));
        let memcpy = {
            let mut dst = vec![0u8; bytes.len()];
            let mut best = f64::MAX;
            for _ in 0..20 {
                let t = std::time::Instant::now();
                dst.copy_from_slice(bytes);
                std::hint::black_box(dst[0]);
                best = best.min(t.elapsed().as_secs_f64());
            }
            mb / 1024.0 / best
        };
        assert!(validate_utf8(bytes), "the probe buffer is valid UTF-8");
        println!(
            "utf8 validate over {:.0} MiB: SIMD {simd:.1} GB/s | scalar {scalar:.1} GB/s | memcpy {memcpy:.1} GB/s | SIMD/memcpy {:.0}%",
            mb,
            simd / memcpy * 100.0
        );
    }

    #[cfg(unix)]
    #[test]
    fn fs_read_dir_skips_non_utf8_names() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tmp_path("rd-utf8");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("good.txt"), b"x").unwrap();
        // A file whose name is not valid UTF-8 (0xFF is never valid) — cannot be a `string`.
        let bad_name = std::ffi::OsStr::from_bytes(b"bad-\xff-name");
        std::fs::write(dir.join(bad_name), b"y").unwrap();

        let ds = dir.display().to_string();
        let (pp, pl) = view_of(&ds);
        let mut out = AlignStr { ptr: std::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_fs_read_dir(pp, pl, &mut out) }, 0);
        assert_eq!(out.len, 1, "only the valid-UTF-8 name is listed; the broken name is excluded");
        let e = unsafe { *(out.ptr as *const AlignStr) };
        let nm = unsafe { safe_slice(e.ptr, e.len) };
        assert_eq!(nm, b"good.txt");
        unsafe { align_rt_free_string_array(out.ptr as *mut u8, out.len) };
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- std.path (Slice 4) — pure lexical byte ops -------------------------------------------

    /// A `path.base`/`dir`/`ext` result is a byte view; render it as a `&str` for comparison.
    fn view_str(s: AlignStr) -> String {
        if s.len <= 0 || s.ptr.is_null() {
            return String::new();
        }
        let b = unsafe { safe_slice(s.ptr, s.len) };
        String::from_utf8_lossy(b).into_owned()
    }
    /// Render an owned `string` result and free its buffer (it came from `align_rt_alloc`).
    fn owned_str(s: AlignStr) -> String {
        let out = view_str(s);
        if !s.ptr.is_null() {
            unsafe { align_rt_free(s.ptr as *mut u8) };
        }
        out
    }
    fn base(p: &str) -> String {
        let (pp, pl) = view_of(p);
        view_str(unsafe { align_rt_path_base(pp, pl) })
    }
    fn dir(p: &str) -> String {
        let (pp, pl) = view_of(p);
        view_str(unsafe { align_rt_path_dir(pp, pl) })
    }
    fn ext(p: &str) -> String {
        let (pp, pl) = view_of(p);
        view_str(unsafe { align_rt_path_ext(pp, pl) })
    }
    fn normalize(p: &str) -> String {
        let (pp, pl) = view_of(p);
        owned_str(unsafe { align_rt_path_normalize(pp, pl) })
    }
    fn join(a: &str, b: &str) -> String {
        let (ap, al) = view_of(a);
        let (bp, bl) = view_of(b);
        owned_str(unsafe { align_rt_path_join(ap, al, bp, bl) })
    }

    #[test]
    fn path_base_cases() {
        assert_eq!(base("/usr/bin/ls"), "ls");
        assert_eq!(base("/usr/bin/"), "bin");
        assert_eq!(base("file.txt"), "file.txt");
        assert_eq!(base("/"), "/");
        assert_eq!(base(""), "");
        assert_eq!(base("a/b/c"), "c");
    }

    #[test]
    fn path_dir_cases() {
        assert_eq!(dir("/usr/bin/ls"), "/usr/bin");
        assert_eq!(dir("a/b"), "a");
        assert_eq!(dir("a//b"), "a");
        assert_eq!(dir("file"), ""); // no separator → empty view (not ".")
        assert_eq!(dir("/file"), "/"); // separator at the root
        assert_eq!(dir("/"), "/");
        assert_eq!(dir(""), "");
    }

    #[test]
    fn path_ext_cases() {
        assert_eq!(ext("a.txt"), ".txt");
        assert_eq!(ext("archive.tar.gz"), ".gz");
        assert_eq!(ext(".bashrc"), ""); // leading dot → dotfile, no ext
        assert_eq!(ext("dir/.hidden"), "");
        assert_eq!(ext("a/b.c"), ".c");
        assert_eq!(ext("noext"), "");
        assert_eq!(ext("a.txt/"), ".txt");
    }

    #[test]
    fn path_normalize_cases() {
        assert_eq!(normalize("a/./b/../c"), "a/c");
        assert_eq!(normalize("../a"), "../a"); // leading `..` preserved (relative)
        assert_eq!(normalize("/../a"), "/a"); // `..` past the root dropped (absolute)
        assert_eq!(normalize("a//b"), "a/b");
        assert_eq!(normalize(""), ".");
        assert_eq!(normalize("/"), "/");
        assert_eq!(normalize("a/b/../.."), ".");
        assert_eq!(normalize("a/../../b"), "../b");
        assert_eq!(normalize("/usr/./local/../bin"), "/usr/bin");
    }

    #[test]
    fn path_join_cases() {
        assert_eq!(join("dir/sub", "file.txt"), "dir/sub/file.txt");
        assert_eq!(join("a/", "/b"), "a/b"); // boundary separators collapse to one
        assert_eq!(join("a", "b"), "a/b");
        assert_eq!(join("", "b"), "b");
        assert_eq!(join("a", ""), "a");
        assert_eq!(join("/", "b"), "/b");
    }

    // --- std.env / std.time (Slice 4) ---------------------------------------------------------

    #[test]
    fn env_set_get_round_trip() {
        let (np, nl) = view_of("ALIGN_RT_TEST_VAR");
        let (vp, vl) = view_of("rt-value");
        assert_eq!(unsafe { align_rt_env_set(np, nl, vp, vl) }, 0);
        let mut out = AlignStr { ptr: std::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_env_get(np, nl, &mut out) }, 1, "the var is set");
        assert_eq!(owned_str(out), "rt-value");
        // An unset name → flag 0, {null,0}.
        let (up, ul) = view_of("ALIGN_RT_UNSET_ZZZ");
        let mut out2 = AlignStr { ptr: std::ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_env_get(up, ul, &mut out2) }, 0, "the var is unset");
        assert!(out2.ptr.is_null());
    }

    #[test]
    fn env_set_invalid_name_is_invalid() {
        let (np, nl) = view_of("BAD=NAME");
        let (vp, vl) = view_of("x");
        assert_eq!(unsafe { align_rt_env_set(np, nl, vp, vl) }, AL_INVALID);
        let (ep, el) = view_of("");
        assert_eq!(unsafe { align_rt_env_set(ep, el, vp, vl) }, AL_INVALID, "empty name is invalid");
    }

    #[test]
    fn time_now_positive_and_instant_monotonic() {
        assert!(align_rt_time_now() > 0, "wall clock is after the epoch");
        let a = align_rt_time_instant();
        align_rt_time_sleep(2_000_000); // 2 ms
        let b = align_rt_time_instant();
        assert!(b - a >= 2_000_000, "instant is monotonic and reflects the sleep");
        // A non-positive sleep is a no-op (returns immediately).
        align_rt_time_sleep(-1);
        align_rt_time_sleep(0);
    }

    // --- std.rand ------------------------------------------------------------------------------

    /// Advance a fresh `[u64;4]` state through the FFI `next` entry, returning `count` outputs.
    fn seq_with(seed: i64, count: usize) -> Vec<i64> {
        let mut s = [0u64; 4];
        unsafe { align_rt_rng_seed_with(s.as_mut_ptr(), seed) };
        (0..count).map(|_| unsafe { align_rt_rng_next(s.as_mut_ptr()) }).collect()
    }

    #[test]
    fn seed_with_is_deterministic_and_advances() {
        // Same seed → identical sequence; different seeds → different sequences.
        assert_eq!(seq_with(42, 8), seq_with(42, 8), "same seed must reproduce the sequence");
        assert_ne!(seq_with(42, 8), seq_with(43, 8), "different seeds diverge");
        // The generator advances (consecutive outputs are not all equal).
        let s = seq_with(7, 4);
        assert!(s.iter().any(|&x| x != s[0]), "next() must advance the state");
        // Pin the first outputs (locks the Xoshiro256++ / SplitMix64 constants, portable).
        assert_eq!(seq_with(42, 2), vec![-3425465463722317665, 5881210131331364753]);
    }

    #[test]
    fn seed_os_fills_a_nonzero_state() {
        // Two OS seeds are (almost surely) different, and never the all-zero fixed point.
        let mut a = [0u64; 4];
        let mut b = [0u64; 4];
        unsafe { align_rt_rng_seed_os(a.as_mut_ptr()) };
        unsafe { align_rt_rng_seed_os(b.as_mut_ptr()) };
        assert_ne!(a, [0, 0, 0, 0], "OS seed must not be the all-zero fixed point");
        assert_ne!(a, b, "two OS seeds must (almost surely) differ");
    }

    #[test]
    fn range_is_half_open_and_bias_free() {
        let mut s = [0u64; 4];
        unsafe { align_rt_rng_seed_with(s.as_mut_ptr(), 1) };
        // A single-value range is always its lower bound (lo inclusive, hi exclusive).
        for _ in 0..1000 {
            assert_eq!(unsafe { align_rt_rng_range(s.as_mut_ptr(), 5, 6) }, 5);
        }
        // Every draw lands in [lo, hi); across many draws every value in a small range appears
        // (a coverage/uniformity smoke check — bias would drop or over-represent an endpoint).
        let mut seen = [false; 4];
        for _ in 0..2000 {
            let v = unsafe { align_rt_rng_range(s.as_mut_ptr(), -1, 3) }; // {-1,0,1,2}
            assert!((-1..3).contains(&v), "draw {v} outside [-1, 3)");
            seen[(v + 1) as usize] = true;
        }
        assert!(seen.iter().all(|&b| b), "every value in the range must be reachable");
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut s = [0u64; 4];
        unsafe { align_rt_rng_seed_with(s.as_mut_ptr(), 123) };
        let mut xs: Vec<i64> = (0..50).collect();
        let orig = xs.clone();
        unsafe { align_rt_rng_shuffle(s.as_mut_ptr(), xs.as_mut_ptr() as *mut u8, xs.len() as i64, 8) };
        // Same multiset (a permutation), and the order actually changed.
        let mut sorted = xs.clone();
        sorted.sort();
        assert_eq!(sorted, orig, "shuffle must preserve the multiset");
        assert_ne!(xs, orig, "shuffle must reorder (50 elements: p(identity) ~ 1/50!)");
        // A single-element / empty slice is left unchanged (no panic).
        let mut one = [99i64];
        unsafe { align_rt_rng_shuffle(s.as_mut_ptr(), one.as_mut_ptr() as *mut u8, 1, 8) };
        assert_eq!(one, [99]);
    }

    #[test]
    fn sample_draws_k_distinct_members() {
        let mut s = [0u64; 4];
        unsafe { align_rt_rng_seed_with(s.as_mut_ptr(), 5) };
        let src: Vec<i64> = (0..20).collect();
        let out = unsafe { align_rt_rng_sample(s.as_mut_ptr(), src.as_ptr() as *const u8, src.len() as i64, 8, 8) };
        assert_eq!(out.len, 8);
        let drawn: Vec<i64> = (0..8)
            .map(|i| unsafe { *(out.ptr as *const i64).add(i) })
            .collect();
        // Each drawn item is a member of the source, and all are distinct (without replacement).
        for &d in &drawn {
            assert!(src.contains(&d), "sampled {d} is not from the source");
        }
        let mut uniq = drawn.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), drawn.len(), "sampled items must be distinct");
        unsafe { align_rt_free(out.ptr as *mut u8) };
        // k == 0 → empty draw, no buffer.
        let empty = unsafe { align_rt_rng_sample(s.as_mut_ptr(), src.as_ptr() as *const u8, src.len() as i64, 0, 8) };
        assert_eq!(empty.len, 0);
        assert!(empty.ptr.is_null());
        // Sampling the whole slice is a permutation of it.
        let full = unsafe { align_rt_rng_sample(s.as_mut_ptr(), src.as_ptr() as *const u8, src.len() as i64, src.len() as i64, 8) };
        let mut got: Vec<i64> = (0..src.len()).map(|i| unsafe { *(full.ptr as *const i64).add(i) }).collect();
        got.sort();
        assert_eq!(got, src, "sampling n of n is a full permutation");
        unsafe { align_rt_free(full.ptr as *mut u8) };
    }

    // --- std.cli --------------------------------------------------------------------------------

    /// Build an `argv` `AlignStr` buffer from a slice of `&str` (each entry views its `&str`'s bytes).
    /// The returned `Vec` must outlive any `align_rt_cli_parse` call (its entries borrow `toks`).
    fn cli_argv(toks: &[&str]) -> Vec<AlignStr> {
        toks.iter().map(|s| AlignStr { ptr: s.as_ptr(), len: s.len() as i64 }).collect()
    }

    /// A `str` view over a `&str` (for the name / default arguments).
    fn cli_s(s: &str) -> (*const u8, i64) {
        (s.as_ptr(), s.len() as i64)
    }

    /// The tokenizer accepts all three v1 forms — bare `--bool`, `--str value` / `--str=value`,
    /// `--i64 value` / `--i64=value` — skips `argv[0]`, and fills defaults for unseen flags.
    #[test]
    fn cli_parse_three_forms_and_defaults() {
        let cmd = unsafe { align_rt_cli_command_new(cli_s("t").0, cli_s("t").1) };
        unsafe { align_rt_cli_flag_bool(cmd, cli_s("verbose").0, cli_s("verbose").1) };
        unsafe { align_rt_cli_flag_str(cmd, cli_s("name").0, cli_s("name").1, cli_s("world").0, cli_s("world").1) };
        unsafe { align_rt_cli_flag_i64(cmd, cli_s("count").0, cli_s("count").1, 3) };

        // argv[0] is the program name (skipped); mix the space form and the equals form.
        let argv = cli_argv(&["prog", "--verbose", "--name", "Align", "--count=42"]);
        let mut out: *mut CliParsed = core::ptr::null_mut();
        let rc = unsafe { align_rt_cli_parse(cmd, argv.as_ptr(), argv.len() as i64, &mut out) };
        assert_eq!(rc, 0);
        assert!(!out.is_null());
        assert_eq!(unsafe { align_rt_cli_get_bool(out, cli_s("verbose").0, cli_s("verbose").1) }, 1);
        let nv = unsafe { align_rt_cli_get_str(out, cli_s("name").0, cli_s("name").1) };
        assert_eq!(String::from_utf8_lossy(unsafe { bytes_view(nv.ptr, nv.len) }), "Align");
        assert_eq!(unsafe { align_rt_cli_get_i64(out, cli_s("count").0, cli_s("count").1) }, 42);
        unsafe { align_rt_cli_parsed_free(out) };

        // No args → every flag reports its default.
        let argv0 = cli_argv(&["prog"]);
        let mut out2: *mut CliParsed = core::ptr::null_mut();
        assert_eq!(unsafe { align_rt_cli_parse(cmd, argv0.as_ptr(), argv0.len() as i64, &mut out2) }, 0);
        assert_eq!(unsafe { align_rt_cli_get_bool(out2, cli_s("verbose").0, cli_s("verbose").1) }, 0);
        let nv2 = unsafe { align_rt_cli_get_str(out2, cli_s("name").0, cli_s("name").1) };
        assert_eq!(String::from_utf8_lossy(unsafe { bytes_view(nv2.ptr, nv2.len) }), "world");
        assert_eq!(unsafe { align_rt_cli_get_i64(out2, cli_s("count").0, cli_s("count").1) }, 3);
        unsafe { align_rt_cli_parsed_free(out2) };
        unsafe { align_rt_cli_command_free(cmd) };
    }

    /// Every input error maps to `AL_INVALID` and leaves `*out` null: unknown flag, missing value,
    /// malformed i64, a `=value` on a bool flag, and a bare (non-`--`) token.
    #[test]
    fn cli_parse_errors_map_to_invalid() {
        let cmd = unsafe { align_rt_cli_command_new(cli_s("t").0, cli_s("t").1) };
        unsafe { align_rt_cli_flag_bool(cmd, cli_s("verbose").0, cli_s("verbose").1) };
        unsafe { align_rt_cli_flag_i64(cmd, cli_s("count").0, cli_s("count").1, 0) };

        for bad in [
            vec!["prog", "--bogus"],          // unknown flag
            vec!["prog", "--count"],          // missing value
            vec!["prog", "--count", "abc"],   // malformed i64
            vec!["prog", "--verbose=1"],      // a bool takes no value
            vec!["prog", "positional"],       // not a --flag
        ] {
            let argv = cli_argv(&bad);
            let mut out: *mut CliParsed = core::ptr::null_mut();
            let rc = unsafe { align_rt_cli_parse(cmd, argv.as_ptr(), argv.len() as i64, &mut out) };
            assert_eq!(rc, AL_INVALID, "argv {bad:?} should be AL_INVALID");
            assert!(out.is_null(), "argv {bad:?} must leave *out null");
        }
        // A null out slot / null command is AL_INVALID, not UB.
        let argv = cli_argv(&["prog"]);
        assert_eq!(unsafe { align_rt_cli_parse(cmd, argv.as_ptr(), argv.len() as i64, core::ptr::null_mut()) }, AL_INVALID);
        let mut out: *mut CliParsed = core::ptr::null_mut();
        assert_eq!(unsafe { align_rt_cli_parse(core::ptr::null_mut(), argv.as_ptr(), argv.len() as i64, &mut out) }, AL_INVALID);
        unsafe { align_rt_cli_command_free(cmd) };
    }

    /// A repeated `--flag` keeps the last occurrence (last-wins).
    #[test]
    fn cli_parse_last_occurrence_wins() {
        let cmd = unsafe { align_rt_cli_command_new(cli_s("t").0, cli_s("t").1) };
        unsafe { align_rt_cli_flag_i64(cmd, cli_s("count").0, cli_s("count").1, 0) };
        let argv = cli_argv(&["prog", "--count", "1", "--count=9"]);
        let mut out: *mut CliParsed = core::ptr::null_mut();
        assert_eq!(unsafe { align_rt_cli_parse(cmd, argv.as_ptr(), argv.len() as i64, &mut out) }, 0);
        assert_eq!(unsafe { align_rt_cli_get_i64(out, cli_s("count").0, cli_s("count").1) }, 9);
        unsafe { align_rt_cli_parsed_free(out) };
        unsafe { align_rt_cli_command_free(cmd) };
    }

    /// `usage()` renders the command name and one line per registered flag; a null command is empty.
    #[test]
    fn cli_usage_renders_all_flags() {
        let cmd = unsafe { align_rt_cli_command_new(cli_s("tool").0, cli_s("tool").1) };
        unsafe { align_rt_cli_flag_bool(cmd, cli_s("verbose").0, cli_s("verbose").1) };
        unsafe { align_rt_cli_flag_str(cmd, cli_s("name").0, cli_s("name").1, cli_s("world").0, cli_s("world").1) };
        unsafe { align_rt_cli_flag_i64(cmd, cli_s("count").0, cli_s("count").1, 3) };
        let u = unsafe { align_rt_cli_usage(cmd) };
        let text = String::from_utf8_lossy(unsafe { bytes_view(u.ptr, u.len) }).into_owned();
        assert!(text.contains("usage: tool"), "{text}");
        assert!(text.contains("--verbose"), "{text}");
        assert!(text.contains("--name"), "{text}");
        assert!(text.contains("--count"), "{text}");
        assert!(text.contains("default: 3"), "{text}");
        unsafe { align_rt_free(u.ptr as *mut u8) };

        let empty = unsafe { align_rt_cli_usage(core::ptr::null()) };
        assert_eq!(empty.len, 0);
        assert!(empty.ptr.is_null());
        unsafe { align_rt_cli_command_free(cmd) };
    }

    /// The `*_free` symbols are null-safe (a moved-out / never-initialised slot drops harmlessly).
    #[test]
    fn cli_free_is_null_safe() {
        unsafe { align_rt_cli_command_free(core::ptr::null_mut()) };
        unsafe { align_rt_cli_parsed_free(core::ptr::null_mut()) };
        // A real round trip frees without leaking (Miri/ASan would catch a double-free here).
        let cmd = unsafe { align_rt_cli_command_new(cli_s("t").0, cli_s("t").1) };
        unsafe { align_rt_cli_flag_str(cmd, cli_s("name").0, cli_s("name").1, cli_s("world").0, cli_s("world").1) };
        let argv = cli_argv(&["prog", "--name", "Align"]);
        let mut out: *mut CliParsed = core::ptr::null_mut();
        assert_eq!(unsafe { align_rt_cli_parse(cmd, argv.as_ptr(), argv.len() as i64, &mut out) }, 0);
        unsafe { align_rt_cli_parsed_free(out) };
        unsafe { align_rt_cli_command_free(cmd) };
    }
}
