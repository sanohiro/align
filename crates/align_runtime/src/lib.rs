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
    if len > 0 {
        if let Ok(n) = usize::try_from(len) {
            let bytes = unsafe { std::slice::from_raw_parts(ptr, n) };
            let _ = out.write_all(bytes);
        }
    }
    let _ = out.write_all(b"\n").and_then(|()| out.flush());
}

/// `io.stdout.write(s)` — write the bytes of a `str` to stdout with **no** trailing newline
/// (unlike `print`). Returns 0 on success, 1 on an I/O error. The first `std.io` surface
/// (`06-runtime-std.md`). An empty / null `{ptr,len}` writes nothing and succeeds.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_stdout_write(ptr: *const u8, len: i64) -> i32 {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let ok = if len > 0 && !ptr.is_null() {
        // `try_from` avoids a truncating `len as usize` (a heap OOB) on a 32-bit target; an
        // overflow is a write failure.
        match usize::try_from(len) {
            Ok(n) => {
                let bytes = unsafe { std::slice::from_raw_parts(ptr, n) };
                out.write_all(bytes).and_then(|()| out.flush()).is_ok()
            }
            Err(_) => false,
        }
    } else {
        // Nothing to write; still flush so ordering with other output is stable.
        out.flush().is_ok()
    };
    if ok {
        0
    } else {
        1
    }
}

/// `io.stdout.write(b)` for a `builder` — write the builder's accumulated bytes to stdout (no
/// newline), without consuming it (a borrow). Returns 0 on success, 1 on an I/O error.
///
/// # Safety
/// `b` must be a valid `Builder` pointer for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_stdout_write_builder(b: *mut Builder) -> i32 {
    use std::io::Write;
    // Codegen always passes a live builder handle, but guard the raw pointer at the FFI boundary
    // (this fn returns a status, so a null is a clean error rather than a deref UB).
    if b.is_null() {
        return 1;
    }
    let b = unsafe { &*b };
    let mut out = std::io::stdout().lock();
    if out.write_all(&b.buf).and_then(|()| out.flush()).is_ok() {
        0
    } else {
        1
    }
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_clone(ptr: *const u8, len: i64) -> AlignStr {
    if len <= 0 {
        return AlignStr { ptr: core::ptr::null(), len: 0 };
    }
    // Validate `len` fits a `usize` before allocating/copying. On a 32-bit target an unchecked
    // `len as usize` would truncate, so `align_rt_alloc` would size a tiny buffer while we return
    // the full `len` — a heap out-of-bounds. After this check, both the alloc and the copy are
    // exact (on 64-bit the check never fires).
    let n = match usize::try_from(len) {
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
/// (or a non-UTF-8 path), leaving `out` as the caller-zeroed `{null,0}`. An empty file yields a
/// null buffer with len 0 (no allocation). The first `std.fs` surface (`06-runtime-std.md`).
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
        let Ok(n) = usize::try_from(path_len) else {
            return 1;
        };
        unsafe { std::slice::from_raw_parts(path, n) }
    };
    let Ok(path_str) = std::str::from_utf8(path_bytes) else {
        return 1;
    };
    let data = match std::fs::read(path_str) {
        Ok(d) => d,
        Err(_) => return 1,
    };
    let len = data.len() as i64;
    // Copy into the runtime's own allocator so the generated `Drop` (which calls `free`) owns it.
    let dst = align_rt_alloc(len);
    if len > 0 {
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len()) };
    }
    unsafe { *out = AlignStr { ptr: dst, len } };
    0
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
    let count_usize = usize::try_from(count).unwrap_or_else(|_| panic_abort("chunks count overflow"));
    let bytes = count_usize
        .checked_mul(core::mem::size_of::<AlignStr>())
        .and_then(|b| i64::try_from(b).ok())
        .unwrap_or_else(|| panic_abort("chunks buffer size overflow"));
    let buf = align_rt_alloc(bytes) as *mut AlignStr;
    for i in 0..count {
        let start = i * n; // element offset of this chunk
        let len = core::cmp::min(n, src_len - start);
        let ptr = unsafe { src.add((start * elem_size) as usize) };
        unsafe { *buf.add(i as usize) = AlignStr { ptr, len } };
    }
    AlignStr { ptr: buf as *const u8, len: count }
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
    let count = usize::try_from(count).unwrap_or_else(|_| panic_abort("par_map count overflow"));
    let in_stride = in_stride as usize;
    let out_stride = out_stride as usize;
    let bytes = count
        .checked_mul(out_stride)
        .and_then(|b| i64::try_from(b).ok())
        .unwrap_or_else(|| panic_abort("par_map output size overflow"));
    let out_buf = align_rt_alloc(bytes);
    let in_addr = in_buf as usize;
    let out_addr = out_buf as usize;
    let nthreads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(count);
    // Single-thread fast path: run on the caller, skipping the `thread::scope` overhead.
    if nthreads <= 1 {
        for i in 0..count {
            let ip = (in_addr + i * in_stride) as *const u8;
            let op = (out_addr + i * out_stride) as *mut u8;
            thunk(ip, op);
        }
        return out_buf;
    }
    // Pass the buffers as `usize` addresses so the per-thread closures are `Send` (raw pointers
    // are not). Each thread owns a disjoint `[start, end)` output range, so this is race-free.
    let per = count.div_ceil(nthreads);
    std::thread::scope(|s| {
        for t in 0..nthreads {
            let start = t * per;
            if start >= count {
                break;
            }
            let end = (start + per).min(count);
            s.spawn(move || {
                for i in start..end {
                    let ip = (in_addr + i * in_stride) as *const u8;
                    let op = (out_addr + i * out_stride) as *mut u8;
                    thunk(ip, op);
                }
            });
        }
    });
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

/// Open a builder. If `arena` is non-null, the finished string is allocated in that
/// arena (freed in bulk at the block's end); otherwise it is leaked (no owner yet).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_new(arena: *mut Arena) -> *mut Builder {
    Box::into_raw(Box::new(Builder { buf: Vec::new(), arena }))
}

/// Append raw bytes (a static template part or a `str` value).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write(b: *mut Builder, ptr: *const u8, len: i64) {
    let b = unsafe { &mut *b };
    if len > 0 {
        b.buf.extend_from_slice(unsafe { std::slice::from_raw_parts(ptr, len as usize) });
    }
}

/// Append a decimal integer.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_write_int(b: *mut Builder, v: i64) {
    use std::io::Write;
    let b = unsafe { &mut *b };
    let _ = write!(b.buf, "{v}");
}

/// Append `true`/`false`.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_write_bool(b: *mut Builder, v: i32) {
    let b = unsafe { &mut *b };
    b.buf.extend_from_slice(if v != 0 { &b"true"[..] } else { &b"false"[..] });
}

/// Append a `char`'s UTF-8 encoding.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_write_char(b: *mut Builder, c: u32) {
    let b = unsafe { &mut *b };
    let ch = char::from_u32(c).unwrap_or('\u{FFFD}');
    let mut tmp = [0u8; 4];
    b.buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
}

/// Append an `f64`'s shortest round-trip decimal.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_write_f64(b: *mut Builder, x: f64) {
    let b = unsafe { &mut *b };
    push_float(&mut b.buf, x);
}

/// Append an `f32`'s shortest round-trip decimal.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_write_f32(b: *mut Builder, x: f32) {
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
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        for &c in bytes {
            match c {
                b'"' => b.buf.extend_from_slice(b"\\\""),
                b'\\' => b.buf.extend_from_slice(b"\\\\"),
                0x08 => b.buf.extend_from_slice(b"\\b"),
                0x0c => b.buf.extend_from_slice(b"\\f"),
                b'\n' => b.buf.extend_from_slice(b"\\n"),
                b'\r' => b.buf.extend_from_slice(b"\\r"),
                b'\t' => b.buf.extend_from_slice(b"\\t"),
                c if c < 0x20 => {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    b.buf.extend_from_slice(b"\\u00");
                    b.buf.push(HEX[(c >> 4) as usize]);
                    b.buf.push(HEX[(c & 0xf) as usize]);
                }
                c => b.buf.push(c),
            }
        }
    }
    b.buf.push(b'"');
}

/// One field descriptor for `json.decode` (matches the codegen layout):
/// `{ name_ptr, name_len, tag, offset }`. `tag`: byte width for ints (1/2/4/8), 0 for bool.
#[repr(C)]
pub struct JsonField {
    pub name_ptr: *const u8,
    pub name_len: i64,
    pub tag: i32,
    pub offset: i64,
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
) -> i32 {
    // `from_raw_parts` is UB on a null pointer even with len 0, and an empty owned `string`
    // (e.g. an empty `builder().to_string()` or `str.clone()`) is `{null, 0}` — guard it.
    let src: &[u8] = if input_len <= 0 || input.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(input, input_len as usize) }
    };
    // `from_raw_parts` is UB on a null pointer even with len 0 — guard `fields` like `input`.
    let descs: &[JsonField] = if n_fields <= 0 || fields.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(fields, n_fields as usize) }
    };

    let mut p = JsonParser { src, pos: 0 };
    let ok = (|| -> Option<()> {
        unsafe { parse_object(&mut p, descs, out, out_size)? };
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

/// Parse one JSON object from `p` into the (caller-zeroed) struct at `out` (`out_size` bytes) per
/// the field `descs`, leaving `p` positioned just past the closing `}`. Returns `None` on a parse
/// error, a missing or duplicate declared field, or an out-of-range descriptor. Shared by the
/// single-struct decode and the `array<Struct>` AoS decode (MMv2 slice 8d).
///
/// # Safety
/// `out` must point to `out_size` writable, already-zeroed bytes; each descriptor's `name_ptr`
/// must describe a valid byte range. `str` fields write a `{ptr,len}` view into `p`'s input.
unsafe fn parse_object(p: &mut JsonParser, descs: &[JsonField], out: *mut u8, out_size: i64) -> Option<()> {
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
            // Find the matching field descriptor (unknown keys are skipped).
            let idx = descs.iter().position(|d| {
                let name = unsafe { std::slice::from_raw_parts(d.name_ptr, d.name_len.max(0) as usize) };
                name == key
            });
            match idx {
                Some(i) => {
                    if !seen.mark(i) {
                        return None; // duplicate field
                    }
                    let d = &descs[i];
                    // tag = (kind << 8) | byte-width. kind: 0 = int, 1 = bool, 2 = float.
                    let kind = (d.tag >> 8) & 0xff;
                    let width = (d.tag & 0xff) as i64;
                    // Defense in depth: never write outside the out struct, even if a
                    // descriptor offset/width were wrong (checked_add avoids i64 overflow).
                    if d.offset < 0 || d.offset.checked_add(width).map_or(true, |end| end > out_size) {
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
                            // Write the float repr at the field width (f32 / f64).
                            if w == 4 {
                                let bytes = (v as f32).to_le_bytes();
                                for k in 0..4 {
                                    unsafe { *out.add(off + k) = bytes[k] };
                                }
                            } else {
                                let bytes = v.to_le_bytes();
                                for k in 0..8 {
                                    unsafe { *out.add(off + k) = bytes[k] };
                                }
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
                            for k in 0..8 {
                                unsafe { *out.add(off + k) = ptr_bytes[k] };
                                unsafe { *out.add(off + 8 + k) = len_bytes[k] };
                            }
                        }
                        _ => {
                            if w != 1 && w != 2 && w != 4 && w != 8 {
                                return None;
                            }
                            let v = p.integer()?;
                            let bytes = v.to_le_bytes();
                            for k in 0..w {
                                unsafe { *out.add(off + k) = bytes[k] };
                            }
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
) -> i32 {
    // `from_raw_parts` is UB on a null pointer even with len 0 — guard an empty owned `string`.
    let src: &[u8] = if input_len <= 0 || input.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(input, input_len as usize) }
    };
    // `from_raw_parts` is UB on a null pointer even with len 0 — guard `fields` like `input`.
    let descs: &[JsonField] = if n_fields <= 0 || fields.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(fields, n_fields as usize) }
    };
    let esz = elem_size.max(0) as usize;
    let mut buf: Vec<u8> = Vec::new();
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
                // Grow the buffer by one zeroed element and decode the object into it. The decoded
                // `str` fields hold pointers into `src` (not `buf`), so reallocating `buf` on the
                // next `resize` keeps them valid.
                let base = buf.len();
                buf.resize(base + esz, 0);
                unsafe { parse_object(&mut p, descs, buf.as_mut_ptr().add(base), esz as i64)? };
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

/// Parse the JSON array in `input` into a freshly heap-allocated owned `array<T>`, writing the
/// materialized `{ptr, len}` (len = element count) to `out` (MMv2 slice 8c). `elem_tag` is the
/// element encoding `(kind << 8) | byte-width` (kind 0 = int, 1 = bool, 2 = float), matching the
/// struct-field tags. Elements are *copied* into the new buffer (not borrowed), so the result is
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
    // `from_raw_parts` is UB on a null pointer even with len 0, and an empty owned `string`
    // (e.g. an empty `builder().to_string()` or `str.clone()`) is `{null, 0}` — guard it.
    let src: &[u8] = if input_len <= 0 || input.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(input, input_len as usize) }
    };
    let kind = (elem_tag >> 8) & 0xff;
    let width = (elem_tag & 0xff) as usize;
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
                        let le = p.integer()?.to_le_bytes();
                        bytes.extend_from_slice(&le[..width]);
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
    /// it does not hold `self`, and the parser can keep advancing after.
    fn string(&mut self) -> Option<&'a [u8]> {
        self.expect(b'"')?;
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'"' {
                let s = &self.src[start..self.pos];
                self.pos += 1;
                return Some(s);
            }
            if c == b'\\' {
                return None; // escapes in keys unsupported (M5 cut)
            }
            self.pos += 1;
        }
        None
    }
    fn integer(&mut self) -> Option<i64> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let digits = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == digits {
            return None;
        }
        std::str::from_utf8(&self.src[start..self.pos]).ok()?.parse::<i64>().ok()
    }
    /// Read a JSON number (`-?digits(.digits)?([eE][+-]?digits)?`) as `f64`.
    fn number(&mut self) -> Option<f64> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if self.pos == start {
            return None;
        }
        std::str::from_utf8(&self.src[start..self.pos]).ok()?.parse::<f64>().ok()
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
    /// Skip a value of an unknown key (number / bool / string for the M5 cut).
    fn skip_value(&mut self) -> Option<()> {
        match self.peek() {
            Some(b't' | b'f') => self.boolean().map(|_| ()),
            Some(b'-' | b'0'..=b'9') => self.number().map(|_| ()),
            Some(b'"') => self.string().map(|_| ()),
            _ => None,
        }
    }
}

/// Finish the builder, returning a `str` view over the (leaked) contents and freeing
/// the builder object.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_finish(b: *mut Builder) -> AlignStr {
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
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_into_string(b: *mut Builder) -> AlignStr {
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

/// Byte-equality of two `str` views (M5). Returns 1 if equal, else 0.
///
/// # Safety
/// Both `ptr`/`len` pairs must describe valid byte ranges for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_eq(a: *const u8, alen: i64, b: *const u8, blen: i64) -> i32 {
    if alen != blen {
        return 0;
    }
    // Same view, or both empty: equal without touching memory. This also avoids
    // `from_raw_parts` on a (possibly null) pointer of a zero-length view, which is UB.
    if a == b || alen == 0 {
        return 1;
    }
    let (x, y) = unsafe {
        (
            std::slice::from_raw_parts(a, alen as usize),
            std::slice::from_raw_parts(b, blen as usize),
        )
    };
    (x == y) as i32
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

/// A bump allocator (`docs/impl/06-runtime-std.md` §3). Memory is carved from a list of
/// fixed-size chunks; individual allocations are never freed — the whole arena is
/// released at once by [`align_rt_arena_end`]. Chunk buffers are heap-stable (the outer
/// `Vec` growing never moves an inner buffer), so returned pointers stay valid until end.
pub struct Arena {
    chunks: Vec<Vec<u8>>,
    /// Byte offset into the last chunk.
    off: usize,
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
    Box::into_raw(Box::new(Arena { chunks: Vec::new(), off: 0 }))
}

/// Bump-allocate `size` bytes (with `align`) from the arena.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_arena_alloc(arena: *mut Arena, size: i64, align: i64) -> *mut u8 {
    let arena = unsafe { &mut *arena };
    arena.alloc(size as usize, align as usize)
}

/// Bulk-release every allocation, keeping the arena for reuse.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_arena_reset(arena: *mut Arena) {
    let arena = unsafe { &mut *arena };
    arena.chunks.clear();
    arena.off = 0;
}

/// Release every allocation and the arena itself.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_arena_end(arena: *mut Arena) {
    drop(unsafe { Box::from_raw(arena) });
}

// `task_group` runtime (slice ④b). A `TaskGroup` owns a region (arena) holding each spawned
// task's environment + result slot, plus the deferred task list. ④b-1 runs the tasks
// sequentially at `wait`; ④b-2 will spawn a thread per task and join at `wait` (the per-task
// trampoline, env, and slot are already heap-stable in the region, so that is the only change).
struct TgTask {
    /// `tramp(thunk, env, slot)` — runs the spawned closure and writes its result into `slot`.
    tramp: extern "C" fn(*const u8, *mut u8, *mut u8),
    /// The closure's function pointer (env-ABI `fn(env) -> R`), passed through to the trampoline.
    thunk: *const u8,
    /// The task's environment (capture snapshot) — a fresh region allocation per `spawn`.
    env: *mut u8,
    /// The task's result slot (a region allocation sized for `R`).
    slot: *mut u8,
}

pub struct TaskGroup {
    arena: Arena,
    tasks: Vec<TgTask>,
}

/// Open a `task_group`. Freed by [`align_rt_tg_end`].
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_tg_begin() -> *mut TaskGroup {
    Box::into_raw(Box::new(TaskGroup { arena: Arena { chunks: Vec::new(), off: 0 }, tasks: Vec::new() }))
}

/// Bump-allocate `size` bytes (with `align`) from the task group's region (envs + result slots).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_tg_alloc(tg: *mut TaskGroup, size: i64, align: i64) -> *mut u8 {
    unsafe { &mut *tg }.arena.alloc(size as usize, align as usize)
}

/// Register a deferred task (its trampoline + closure pointer + env + result slot).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_tg_register(
    tg: *mut TaskGroup,
    tramp: extern "C" fn(*const u8, *mut u8, *mut u8),
    thunk: *const u8,
    env: *mut u8,
    slot: *mut u8,
) {
    unsafe { &mut *tg }.tasks.push(TgTask { tramp, thunk, env, slot });
}

/// A task's data, made `Send` so it can move into a worker thread. Safe by construction (slice
/// ④b): each task's `env`/`slot` are a fresh, private region allocation — no task shares them, the
/// `env` is only read (its capture snapshot) and the `slot` only written, and the region outlives
/// the join (`wait` happens before `tg_end`). `get()` reads a slot only after the join (④c).
struct TgRun {
    tramp: extern "C" fn(*const u8, *mut u8, *mut u8),
    thunk: *const u8,
    env: *mut u8,
    slot: *mut u8,
}
unsafe impl Send for TgRun {}

/// Run every registered task **in parallel** — spawn a worker thread per task, then join them all
/// (fork-join). All allocations happened at `spawn` time (on this thread), so no thread mutates
/// the region during the run; each worker only reads its own `env` and writes its own `slot`.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_tg_wait(tg: *mut TaskGroup) {
    let tg = unsafe { &mut *tg };
    let tasks = std::mem::take(&mut tg.tasks);
    let handles: Vec<std::thread::JoinHandle<()>> = tasks
        .into_iter()
        .map(|t| {
            let run = TgRun { tramp: t.tramp, thunk: t.thunk, env: t.env, slot: t.slot };
            std::thread::spawn(move || {
                let run = run;
                (run.tramp)(run.thunk, run.env, run.slot);
            })
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
}

/// Release the task group's region and the handle.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_tg_end(tg: *mut TaskGroup) {
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
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_alloc_is_stable_across_chunks() {
        let a = align_rt_arena_begin();
        // Many allocations spanning multiple chunks; earlier pointers stay writable.
        let first = align_rt_arena_alloc(a, 8, 8) as *mut i64;
        unsafe { *first = 42 };
        for _ in 0..50_000 {
            let p = align_rt_arena_alloc(a, 8, 8) as *mut i64;
            unsafe { *p = 1 };
        }
        assert_eq!(unsafe { *first }, 42, "earlier allocation must remain valid");
        align_rt_arena_end(a);
    }
}
