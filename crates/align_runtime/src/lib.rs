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
    use std::io::Read;
    // Fast path: a regular file with a known nonzero length — allocate the owned buffer once and
    // read straight into it, skipping the `std::fs::read` Vec and the second copy into the runtime
    // allocator (~1.8× on a 128 MiB file; `work/io_perf_probe.rs`). Special / streaming files
    // (length 0, `/proc`, pipes, char devices) and any file that shrinks or grows under us fall
    // back to the copy path below — which re-opens by path, so a partial read here is harmless.
    if let Ok(mut file) = std::fs::File::open(path_str) {
        if let Ok(meta) = file.metadata() {
            let flen = meta.len();
            // Regular files only (`is_file`), nonzero length (skips empty / size-unknown special
            // files). `isize::try_from` is the single guard that keeps the rest sound on every
            // target: a positive `isize` fits both `usize` (the slice len) and `i64` (the alloc
            // size) losslessly, and is `<= isize::MAX` so `from_raw_parts_mut` is not UB. A larger
            // file (only reachable on a 32-bit target) just takes the fallback path.
            if meta.is_file() && flen > 0 {
                if let Ok(len_z) = isize::try_from(flen) {
                    let len_i = len_z as i64;
                    let len_u = len_z as usize;
                    let dst = align_rt_alloc(len_i);
                    let buf = unsafe { core::slice::from_raw_parts_mut(dst, len_u) };
                    // `read_exact` fills the whole buffer (a shorter file errors). On success one
                    // more read must hit EOF — otherwise the file grew past the snapshot and the
                    // buffer would silently truncate, so fall back. Any failure frees and falls back.
                    if file.read_exact(buf).is_ok() && matches!(file.read(&mut [0u8; 1]), Ok(0)) {
                        unsafe { *out = AlignStr { ptr: dst, len: len_i } };
                        return 0;
                    }
                    unsafe { align_rt_free(dst) };
                }
            }
        }
    }
    // Fallback (empty / special / changed file): read into a Vec, then copy into the owned buffer.
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
    // Run `[start, end)` of the map on this thread (buffers passed as `usize` so the closures are
    // `Send` — raw pointers are not; the ranges are disjoint, so this is race-free).
    let run = move |start: usize, end: usize| {
        for i in start..end {
            let ip = (in_addr + i * in_stride) as *const u8;
            let op = (out_addr + i * out_stride) as *mut u8;
            thunk(ip, op);
        }
    };

    let (pool, workers) = par_pool();
    // Don't parallelize trivially-small work: a chunk must be at least `PAR_MIN_CHUNK` elements, so
    // tiny maps (where the pool round-trip would dwarf the work) fall to the single-chunk caller
    // path. Standard parallel-runtime tuning (Rayon/OpenMP have an analogue); not thunk-cost-aware.
    const PAR_MIN_CHUNK: usize = 4096;
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
    let remaining: std::sync::Arc<(std::sync::Mutex<(usize, Option<PanicBox>)>, std::sync::Condvar)> =
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
            if let Err(p) = res {
                if st.1.is_none() {
                    st.1 = Some(p);
                }
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
    if let Ok(cap) = usize::try_from(capacity) {
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
    let b = unsafe { &mut *b };
    if len > 0 {
        b.buf.extend_from_slice(unsafe { std::slice::from_raw_parts(ptr, len as usize) });
    }
}

/// Append a decimal integer. Hand-rolled itoa straight into the buffer — no generic `write!`
/// formatter (runtime format-string parsing + trait dispatch), the builder's hot path.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_write_int(b: *mut Builder, v: i64) {
    let b = unsafe { &mut *b };
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
    b.buf.extend_from_slice(&tmp[i..]);
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
    phf: *const i32,
    phf_len: i64,
    phf_seed: i64,
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

/// FNV-1a over `bytes`, seeded — the hash behind the compile-time perfect-hash field dispatch.
/// **MUST byte-for-byte match the codegen-side `phf_hash` in `align_codegen_llvm`**, which computes
/// each field name's slot at compile time; if the two diverge, a field would hash to the wrong slot.
#[inline]
fn json_phf_hash(bytes: &[u8], seed: u64) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ seed;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
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
        Some(unsafe { std::slice::from_raw_parts(ptr, len as usize) })
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
    phf: *const i32,
    phf_len: i64,
    phf_seed: i64,
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
    let phf = unsafe { phf_slice(phf, phf_len) };
    let phf_seed = phf_seed as u64;
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
                unsafe { parse_object(&mut p, descs, buf.as_mut_ptr().add(base), esz as i64, phf, phf_seed)? };
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
/// Mechanism: an open-addressing (linear-probe) table that grows to track the live group count
/// (doubling past a 0.75 load) — a primitive-key, no-boxing, cache-tight aggregate, the lever vs
/// Rust's generic `HashMap`. Three dense parallel arrays (key / acc / used) probe-scan well (a naive
/// interleaved-slot layout measured *worse*; `docs/open-questions.md`).
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
            if !occ[slot] {
                occ[slot] = true;
                tkey[slot] = k;
                tacc[slot] = v;
                count += 1;
                if count * 4 > tsize * 3 {
                    let ns = tsize * 2;
                    let nm = ns - 1;
                    let mut nk = vec![0i64; ns];
                    let mut na = vec![0i64; ns];
                    let mut no = vec![false; ns];
                    for s in 0..tsize {
                        if occ[s] {
                            let mut t = group_slot(tkey[s], nm);
                            while no[t] {
                                t = (t + 1) & nm;
                            }
                            no[t] = true;
                            nk[t] = tkey[s];
                            na[t] = tacc[s];
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
            if tkey[slot] == k {
                tacc[slot] = combine(tacc[slot], v);
                break;
            }
            slot = (slot + 1) & mask;
        }
    }

    if cap < 0 || count > cap as usize || out_keys.is_null() || out_vals.is_null() {
        return -1;
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
    if len <= 0 || keys.is_null() || vals.is_null() {
        (&[], &[])
    } else {
        let n = len as usize;
        unsafe { (std::slice::from_raw_parts(keys, n), std::slice::from_raw_parts(vals, n)) }
    }
}

/// `group_by(.key).sum(.value)` — per-group sum. Wraps + sums in row order.
///
/// # Safety
/// `keys`/`vals` must be valid for `len` `i64`s; `out_keys`/`out_vals` for `cap` `i64`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_sum_i64(keys: *const i64, vals: *const i64, len: i64, out_keys: *mut i64, out_vals: *mut i64, cap: i64) -> i64 {
    let (keys, vals) = unsafe { group_io(keys, vals, len) };
    unsafe { group_agg_i64(keys, out_keys, out_vals, cap, |i| vals[i], |a, b| a.wrapping_add(b)) }
}

/// `group_by(.key).min(.value)` — per-group minimum.
///
/// # Safety
/// Same as [`align_rt_group_sum_i64`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_min_i64(keys: *const i64, vals: *const i64, len: i64, out_keys: *mut i64, out_vals: *mut i64, cap: i64) -> i64 {
    let (keys, vals) = unsafe { group_io(keys, vals, len) };
    unsafe { group_agg_i64(keys, out_keys, out_vals, cap, |i| vals[i], |a, b| a.min(b)) }
}

/// `group_by(.key).max(.value)` — per-group maximum.
///
/// # Safety
/// Same as [`align_rt_group_sum_i64`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_max_i64(keys: *const i64, vals: *const i64, len: i64, out_keys: *mut i64, out_vals: *mut i64, cap: i64) -> i64 {
    let (keys, vals) = unsafe { group_io(keys, vals, len) };
    unsafe { group_agg_i64(keys, out_keys, out_vals, cap, |i| vals[i], |a, b| a.max(b)) }
}

/// `group_by(.key).count()` — per-group row count (no value column).
///
/// # Safety
/// `keys` must be valid for `len` `i64`s; `out_keys`/`out_vals` for `cap` `i64`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_count_i64(keys: *const i64, len: i64, out_keys: *mut i64, out_vals: *mut i64, cap: i64) -> i64 {
    let keys: &[i64] = if len <= 0 || keys.is_null() { &[] } else { unsafe { std::slice::from_raw_parts(keys, len as usize) } };
    unsafe { group_agg_i64(keys, out_keys, out_vals, cap, |_| 1, |a, b| a.wrapping_add(b)) }
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

/// A buffered stdout writer (`io.stdout.buffered()`): the sink-first fast path. Bytes accumulate
/// in a fixed-capacity buffer and reach fd 1 in one `write(2)` only when the buffer fills or on an
/// explicit `flush` / drop — so per-`write` calls do no syscall and memory stays O(buffer), not
/// O(total output). Writes go straight to fd 1, skipping the `std::io::Stdout` lock + line buffer.
pub struct StdoutWriter {
    buf: Vec<u8>,
    /// Sticky: an internal flush (on a full buffer) failed. `write` returns `()`, so the error is
    /// latched here and surfaced by the next `flush` — matching `out.write(..); out.flush()?`.
    err: bool,
}

/// 64 KiB — large enough to amortize the syscall over many small writes, small enough to stay in
/// cache and bound memory.
const STDOUT_WRITER_CAP: usize = 64 * 1024;

/// Write all of `bytes` to `fd`, looping over partial writes and retrying `EINTR`. Returns false on
/// any other error. An empty slice succeeds without a syscall.
fn write_all_fd(fd: i32, mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        let n = unsafe { write(fd, bytes.as_ptr() as *const core::ffi::c_void, bytes.len()) };
        if n > 0 {
            bytes = &bytes[n as usize..];
        } else if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue; // interrupted before writing: retry
        } else {
            return false; // error, or a 0-byte write (treat as failure rather than spin)
        }
    }
    true
}

impl StdoutWriter {
    /// Flush the buffer to fd 1, clearing it on success and latching `err` on failure.
    fn flush_buf(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        if write_all_fd(1, &self.buf) {
            self.buf.clear();
        } else {
            self.err = true;
            self.buf.clear(); // drop the unwritten bytes; the latched error reports the loss
        }
    }
}

/// `io.stdout.buffered()` — open a buffered stdout writer. Freed (after a final flush) by the
/// generated `Drop` via [`align_rt_io_buf_free`].
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_io_buf_new() -> *mut StdoutWriter {
    Box::into_raw(Box::new(StdoutWriter { buf: Vec::with_capacity(STDOUT_WRITER_CAP), err: false }))
}

/// `w.write(s)` — append a `str`'s bytes, flushing to fd 1 only when the buffer would overflow.
/// A chunk larger than the whole buffer is written straight through (no buffering, no double copy).
/// Infallible at the surface; an internal flush failure is latched and surfaces at the next `flush`.
///
/// # Safety
/// `w` must be a valid `StdoutWriter` pointer; `ptr`/`len` must describe a valid byte range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_buf_write(w: *mut StdoutWriter, ptr: *const u8, len: i64) {
    if w.is_null() || len <= 0 || ptr.is_null() {
        return;
    }
    let w = unsafe { &mut *w };
    let Ok(n) = usize::try_from(len) else { return };
    let bytes = unsafe { std::slice::from_raw_parts(ptr, n) };
    // If it won't fit in the remaining space, flush what's buffered first.
    if w.buf.len() + n > STDOUT_WRITER_CAP {
        w.flush_buf();
        if w.err {
            return;
        }
        // A chunk at least as big as the buffer would just be copied in and flushed right back
        // out — write it straight to fd 1 instead.
        if n >= STDOUT_WRITER_CAP {
            if !write_all_fd(1, bytes) {
                w.err = true;
            }
            return;
        }
    }
    w.buf.extend_from_slice(bytes);
}

/// `w.flush()` — write any buffered bytes to fd 1. Returns 0 on success, 1 if this flush or any
/// earlier internal flush failed (the latched error is then cleared).
///
/// # Safety
/// `w` must be a valid `StdoutWriter` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_buf_flush(w: *mut StdoutWriter) -> i32 {
    if w.is_null() {
        return 1;
    }
    let w = unsafe { &mut *w };
    w.flush_buf();
    if w.err {
        w.err = false; // report once, then clear so the writer stays usable
        1
    } else {
        0
    }
}

/// Free a buffered stdout writer, flushing any remaining bytes best-effort first (a drop-time
/// safety net; errors are not observable here — use an explicit `flush()?` to handle them).
/// Null-safe, so a never-initialised owned slot drops harmlessly.
///
/// # Safety
/// `w` must be null or a pointer returned by [`align_rt_io_buf_new`] and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_buf_free(w: *mut StdoutWriter) {
    if !w.is_null() {
        let mut w = unsafe { Box::from_raw(w) };
        w.flush_buf();
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
            std::slice::from_raw_parts(hptr, hlen as usize),
            std::slice::from_raw_parts(nptr, nlen as usize),
        )
    };
    memchr::memmem::find(hay, needle).is_some() as i32
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
            std::slice::from_raw_parts(hptr, plen as usize),
            std::slice::from_raw_parts(pptr, plen as usize),
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
            std::slice::from_raw_parts(tail, slen as usize),
            std::slice::from_raw_parts(sptr, slen as usize),
        )
    };
    (hay == suf) as i32
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

/// Run every registered task **in parallel** — spawn a worker thread per task, then join them all
/// (fork-join). All allocations happened at `spawn` time (on this thread), so no thread mutates
/// the region during the run; each worker only reads its own `env` and writes its own `slot`.
///
/// Uses `std::thread::scope` (like `align_rt_par_map`) so that *every* spawned thread is joined
/// before this returns even if a later `spawn` panics — otherwise an unwinding panic would detach
/// running threads and they would read the arena after `tg_end` frees it (a use-after-free).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_tg_wait(tg: *mut TaskGroup) -> *mut u8 {
    let tg = unsafe { &mut *tg };
    let tasks = std::mem::take(&mut tg.tasks);
    // The `err_slot` of the first task (in spawn order) that errored, or null if all succeeded.
    let mut first_err: *mut u8 = std::ptr::null_mut();
    std::thread::scope(|s| {
        let handles: Vec<(_, *mut u8)> = tasks
            .into_iter()
            .map(|t| {
                let run = TgRun { tramp: t.tramp, thunk: t.thunk, env: t.env, slot: t.slot, err_slot: t.err_slot };
                let es = t.err_slot;
                // Rebind the whole value so the closure captures the `Send` `TgRun` as a unit
                // (edition-2021 disjoint capture would otherwise grab the non-`Send` raw fields).
                (s.spawn(move || {
                    let run = run;
                    (run.tramp)(run.thunk, run.env, run.slot, run.err_slot)
                }), es)
            })
            .collect();
        // Join all (the scope joins even on panic); record the first errored task's `err_slot`.
        // A worker panic must not be swallowed (that would falsely report success and then read an
        // unwritten slot) — re-raise it on the joining thread.
        for (h, es) in handles {
            match h.join() {
                Ok(errored) => {
                    if first_err.is_null() && errored != 0 {
                        first_err = es;
                    }
                }
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });
    first_err
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
    // POSIX `write(2)` — the buffered stdout writer streams straight to fd 1, bypassing the
    // `std::io::Stdout` lock + line-buffering that `print` / `io.stdout.write` pay per call.
    fn write(fd: i32, buf: *const core::ffi::c_void, count: usize) -> isize;
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
    fn stdout_writer_accumulates_small_writes_without_flushing() {
        // Small writes stay buffered (no syscall, nothing reaches fd 1): the buffer holds exactly
        // the concatenated bytes and no error is latched. The flush-to-fd-1 and large-chunk
        // pass-through paths are covered end-to-end (they necessarily touch real stdout).
        let w = align_rt_io_buf_new();
        for part in [&b"hello "[..], b"world", b"!"] {
            unsafe { align_rt_io_buf_write(w, part.as_ptr(), part.len() as i64) };
        }
        {
            let wr = unsafe { &mut *w };
            assert_eq!(wr.buf, b"hello world!", "small writes accumulate, unflushed");
            assert!(!wr.err);
            wr.buf.clear(); // so the drop-flush below emits nothing to fd 1
        }
        unsafe { align_rt_io_buf_free(w) };
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
                let v = unsafe { core::slice::from_raw_parts(out.ptr, out.len as usize) }.to_vec();
                unsafe { align_rt_free(out.ptr as *mut u8) };
                Some(v)
            } else {
                // rc==0 with len 0 owns no buffer (null ptr) — nothing to free.
                if rc == 0 { Some(Vec::new()) } else { None }
            };
            (rc, bytes)
        };

        // Fast path: a regular file larger than one read buffer — the whole content comes back
        // intact (exercises `read_exact` filling the owned buffer + the EOF guard).
        let big_path = dir.join(format!("{uniq}-big.bin"));
        let content: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
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
            align_rt_builder_write_int(&mut b, v);
            assert_eq!(String::from_utf8(b.buf).unwrap(), format!("{v}"), "write_int({v})");
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
    fn phf_hash_matches_codegen() {
        // The same pinned value as `align_codegen_llvm`'s `phf_hash_is_pinned` test. If these two
        // ever diverge, the compile-time perfect-hash table would route JSON keys to wrong slots.
        assert_eq!(json_phf_hash(b"score", 0), 0xc10e_63fb_e7f6_24f5);
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
        let first = align_rt_arena_alloc(a, 8, 8) as *mut i64;
        unsafe { *first = 42 };
        for _ in 0..50_000 {
            let p = align_rt_arena_alloc(a, 8, 8) as *mut i64;
            unsafe { *p = 1 };
        }
        assert_eq!(unsafe { *first }, 42, "earlier allocation must remain valid");
        align_rt_arena_end(a);
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
        }
    }
}
