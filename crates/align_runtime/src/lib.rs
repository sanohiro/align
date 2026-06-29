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
    let b = unsafe { &mut *b };
    if l1 > 0 {
        b.buf.extend_from_slice(unsafe { std::slice::from_raw_parts(p1, l1 as usize) });
    }
    builder_push_i64(&mut b.buf, v);
    if l2 > 0 {
        b.buf.extend_from_slice(unsafe { std::slice::from_raw_parts(p2, l2 as usize) });
    }
}

/// Append `true`/`false`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write_bool(b: *mut Builder, v: i32) {
    let b = unsafe { &mut *b };
    b.buf.extend_from_slice(if v != 0 { &b"true"[..] } else { &b"false"[..] });
}

/// Append a `char`'s UTF-8 encoding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write_char(b: *mut Builder, c: u32) {
    let b = unsafe { &mut *b };
    let ch = char::from_u32(c).unwrap_or('\u{FFFD}');
    let mut tmp = [0u8; 4];
    b.buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
}

/// Append an `f64`'s shortest round-trip decimal.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write_f64(b: *mut Builder, x: f64) {
    let b = unsafe { &mut *b };
    push_float(&mut b.buf, x);
}

/// Append an `f32`'s shortest round-trip decimal.
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

/// The declared field's name bytes (for key verification + dispatch).
#[inline]
unsafe fn field_name<'a>(d: &JsonField) -> &'a [u8] {
    if d.name_ptr.is_null() || d.name_len <= 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(d.name_ptr, d.name_len as usize) }
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
        if d.offset < 0 || d.offset.checked_add(width).map_or(true, |end| end > self.esz) {
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
        for j in 0..8 {
            unsafe { *p.add(j) = pb[j] };
            unsafe { *p.add(8 + j) = lb[j] };
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
            if w == 4 {
                let b = (v as f32).to_le_bytes();
                for j in 0..4 {
                    unsafe { *p.add(j) = b[j] };
                }
            } else {
                let b = v.to_le_bytes();
                for j in 0..8 {
                    unsafe { *p.add(j) = b[j] };
                }
            }
        }
        _ => {
            if w != 1 && w != 2 && w != 4 && w != 8 {
                return None;
            }
            let v = vp.integer()?;
            let b = v.to_le_bytes();
            for j in 0..w {
                unsafe { *p.add(j) = b[j] };
            }
        }
    }
    Some(())
}

/// Mison **speculation** fast path: the record's colon count matched the learned pattern, so for each
/// declared field at its learned ordinal, **verify** the key (a byte compare) and write the value —
/// no `find_field` hashing, and the unqueried fields' colons are never touched. Returns `false` on
/// any key mismatch (the caller then falls back); a partial write is harmless (the fallback overwrites
/// the slot or errors). `rec_cols[o]` is the index position of the record's o-th colon; `pat_field[o]`
/// is the declared field at ordinal `o`, or `-1` for an unqueried position.
///
/// # Safety
/// `dst` must resolve to writable bytes for every written field.
unsafe fn json_speculate<D: FieldDst>(src: &[u8], idx: &[u32], rec_cols: &[usize], pat_field: &[i32], descs: &[JsonField], dst: &D) -> bool {
    for (o, &k) in rec_cols.iter().enumerate() {
        let fi = pat_field[o];
        if fi < 0 {
            continue; // an unqueried position — skip it entirely (projection)
        }
        let d = &descs[fi as usize];
        if !key_matches_before_colon(src, idx[k] as usize, unsafe { field_name(d) }) {
            return false; // structure drifted from the pattern — fall back
        }
        if unsafe { write_field_indexed(src, idx, k, fi as usize, d, dst) }.is_none() {
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
    descs: &[JsonField],
    dst: &D,
    phf: Option<&[i32]>,
    phf_seed: u64,
    seen: &mut SeenSet,
    pat_field: &mut Vec<i32>,
) -> Option<()> {
    *seen = SeenSet::new(descs.len());
    pat_field.clear();
    pat_field.resize(rec_cols.len(), -1);
    for (o, &k) in rec_cols.iter().enumerate() {
        let Some(key) = key_before_colon(src, idx[k] as usize) else {
            return None; // a `:` not preceded by a `"..."` key — malformed object
        };
        if let Some(fi) = unsafe { find_field(descs, key, phf, phf_seed) } {
            if !seen.mark(fi) {
                return None; // duplicate field
            }
            pat_field[o] = fi as i32;
            unsafe { write_field_indexed(src, idx, k, fi, &descs[fi], dst)? };
        }
    }
    if seen.all_seen(descs.len()) {
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
                        let spec = pat_ncol == rec_cols.len() as i64
                            && unsafe { json_speculate(src, &idx, &rec_cols, &pat_field, descs, &dst) };
                        if !spec {
                            unsafe { json_fallback(src, &idx, &rec_cols, descs, &dst, phf, phf_seed, &mut seen, &mut pat_field)? };
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
fn soa_layout(widths: &[usize], n: usize) -> Option<(Vec<(usize, usize)>, usize, usize)> {
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
    let src: &[u8] = if input_len <= 0 || input.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(input, input_len as usize) }
    };
    let descs: &[JsonField] = if n_fields <= 0 || fields.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(fields, n_fields as usize) }
    };
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
                        let spec = pat_ncol == rec_cols.len() as i64
                            && unsafe { json_speculate(src, &idx, &rec_cols, &pat_field, descs, &dst) };
                        if !spec {
                            unsafe { json_fallback(src, &idx, &rec_cols, descs, &dst, phf, phf_seed, &mut seen, &mut pat_field)? };
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
            if occ[idx] {
                acc[idx] = combine(acc[idx], v);
            } else {
                occ[idx] = true;
                acc[idx] = v;
                count += 1;
                if count > cap as usize {
                    return -1;
                }
            }
        }
        let out_keys = unsafe { std::slice::from_raw_parts_mut(out_keys, count) };
        let out_vals = unsafe { std::slice::from_raw_parts_mut(out_vals, count) };
        let mut g = 0;
        for s in 0..slots {
            if occ[s] {
                out_keys[g] = kmin + s as i64; // kmin + span = kmax, so this never overflows i64.
                out_vals[g] = acc[s];
                g += 1;
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
            if !occ[slot] {
                occ[slot] = true;
                tkey[slot] = k;
                tacc[slot] = v;
                count += 1;
                if count > cap as usize {
                    return -1;
                }
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
#[derive(Default)]
struct FxHasher {
    hash: u64,
}

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        let mut h = self.hash;
        let mut b = bytes;
        while b.len() >= 8 {
            let chunk = u64::from_le_bytes(b[..8].try_into().unwrap());
            h = (h.rotate_left(5) ^ chunk).wrapping_mul(K);
            b = &b[8..];
        }
        if !b.is_empty() {
            let mut buf = [0u8; 8];
            buf[..b.len()].copy_from_slice(b);
            h = (h.rotate_left(5) ^ u64::from_le_bytes(buf)).wrapping_mul(K);
        }
        self.hash = h;
    }
    #[inline]
    fn finish(&self) -> u64 {
        let mut h = self.hash;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^= h >> 33;
        h
    }
}

type FxBuildHasher = std::hash::BuildHasherDefault<FxHasher>;

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
        unsafe { std::slice::from_raw_parts(ks.ptr, ks.len as usize) }
    };
    (bytes, ks)
}

unsafe fn group_agg_str(
    base: *const u8,
    n: i64,
    stride: i64,
    key_off: i64,
    out_keys: *mut AlignStr,
    out_vals: *mut i64,
    cap: i64,
    value_at: impl Fn(*const u8) -> i64,
    combine: impl Fn(i64, i64) -> i64,
) -> i64 {
    use std::collections::HashMap;
    if n <= 0 || base.is_null() {
        return 0;
    }
    if cap < 0 || out_keys.is_null() || out_vals.is_null() {
        return -1;
    }
    let n = n as usize;
    let (stride, key_off) = (stride as usize, key_off as usize);
    // Reserve up front to avoid the early grow-and-rehash churn; the group count is unknown, so cap
    // at a sane starting size (n is the worst case = all-distinct, but don't over-reserve for huge n).
    let initial = n.min(cap as usize).min(1024);
    let mut ids: HashMap<&[u8], usize, FxBuildHasher> = HashMap::with_capacity_and_hasher(initial, FxBuildHasher::default());
    let mut acc: Vec<i64> = Vec::with_capacity(initial);
    let mut reprs: Vec<AlignStr> = Vec::with_capacity(initial);
    for i in 0..n {
        let row = unsafe { base.add(i * stride) };
        let (bytes, ks) = unsafe { read_key_slice(row, key_off) };
        // `value_at` reads from the already-computed `row` (no re-deriving `base + i*stride`).
        let v = value_at(row);
        match ids.get(bytes) {
            Some(&id) => acc[id] = combine(acc[id], v),
            None => {
                let id = acc.len();
                if id >= cap as usize {
                    return -1;
                }
                ids.insert(bytes, id);
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

/// `value_at` reading the i64 value at `val_off` from a row pointer — for sum/min/max (count
/// uses `1`). The caller passes the per-row pointer, so this just offsets to the value column.
#[inline]
fn str_value_reader(val_off: i64) -> impl Fn(*const u8) -> i64 {
    let val_off = val_off as usize;
    move |row| unsafe { (row.add(val_off) as *const i64).read_unaligned() }
}

/// `group_by(.str_key).sum(.i64_value)` over an AoS `array<Struct>`.
///
/// # Safety
/// See [`group_agg_str`]; `val_off` must address a valid `i64` in each row.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_sum_str(base: *const u8, n: i64, stride: i64, key_off: i64, val_off: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    let read = str_value_reader(val_off);
    unsafe { group_agg_str(base, n, stride, key_off, out_keys, out_vals, cap, read, |a, b| a.wrapping_add(b)) }
}

/// `group_by(.str_key).min(.i64_value)` — per-group minimum.
///
/// # Safety
/// See [`align_rt_group_sum_str`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_min_str(base: *const u8, n: i64, stride: i64, key_off: i64, val_off: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    let read = str_value_reader(val_off);
    unsafe { group_agg_str(base, n, stride, key_off, out_keys, out_vals, cap, read, |a, b| a.min(b)) }
}

/// `group_by(.str_key).max(.i64_value)` — per-group maximum.
///
/// # Safety
/// See [`align_rt_group_sum_str`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_max_str(base: *const u8, n: i64, stride: i64, key_off: i64, val_off: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    let read = str_value_reader(val_off);
    unsafe { group_agg_str(base, n, stride, key_off, out_keys, out_vals, cap, read, |a, b| a.max(b)) }
}

/// `group_by(.str_key).count()` — per-group row count (no value column; `val_off` is unused).
///
/// # Safety
/// See [`group_agg_str`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_count_str(base: *const u8, n: i64, stride: i64, key_off: i64, _val_off: i64, out_keys: *mut AlignStr, out_vals: *mut i64, cap: i64) -> i64 {
    unsafe { group_agg_str(base, n, stride, key_off, out_keys, out_vals, cap, |_| 1, |a, b| a.wrapping_add(b)) }
}

/// A fast non-cryptographic hasher (FxHash-style: rotate-xor-multiply over 8-byte chunks) for the


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
    let n = n as usize;
    let k = k as usize;
    let (stride, key_off) = (stride as usize, key_off as usize);
    let specs: &[GroupMultiSpec] = if k == 0 { &[] } else { unsafe { std::slice::from_raw_parts(specs, k) } };
    // Per aggregate: the value reader (a row pointer → i64; `count` reads `1`) and the combine op. The
    // combine is selected once per aggregate (not per row) so the inner fold is a small fixed match.
    let ops: Vec<i64> = specs.iter().map(|s| s.op).collect();
    let val_offs: Vec<usize> = specs.iter().map(|s| s.val_off as usize).collect();

    let initial = n.min(cap as usize).min(1024);
    let mut ids: HashMap<&[u8], usize, FxBuildHasher> = HashMap::with_capacity_and_hasher(initial, FxBuildHasher::default());
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
        match ids.get(bytes) {
            Some(&id) => {
                // `id < reprs.len()` and `acc.len() == reprs.len() * k`, so `g + j < acc.len()`.
                let g = id * k;
                for j in 0..k {
                    unsafe {
                        let slot = acc.get_unchecked_mut(g + j);
                        *slot = combine(*slot, read(row, j), j);
                    }
                }
            }
            None => {
                let id = reprs.len();
                // Bail early if the group count would exceed the caller's output capacity, before
                // growing the tables further (cap = the row count in generated code, so unreachable
                // there, but keeps the function safe for any caller).
                if id >= cap as usize {
                    return -1;
                }
                ids.insert(bytes, id);
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
    let n = n as usize;
    let (stride, key_off) = (stride as usize, key_off as usize);
    let out_ids = unsafe { std::slice::from_raw_parts_mut(out_ids, n) };
    let initial = n.min(cap as usize).min(1024);
    let mut ids: HashMap<&[u8], i64, FxBuildHasher> = HashMap::with_capacity_and_hasher(initial, FxBuildHasher::default());
    let mut reprs: Vec<AlignStr> = Vec::with_capacity(initial);
    for i in 0..n {
        let row = unsafe { base.add(i * stride) };
        let (bytes, ks) = unsafe { read_key_slice(row, key_off) };
        let id = match ids.get(bytes) {
            Some(&id) => id,
            None => {
                let id = reprs.len() as i64;
                // The dictionary would exceed `out_dict`'s capacity — abort early (don't grow the
                // table for a result we can't return).
                if id >= cap {
                    return -1;
                }
                ids.insert(bytes, id);
                reprs.push(ks);
                id
            }
        };
        out_ids[i] = id;
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
    let Ok(n) = usize::try_from(n) else { return };
    let ids = unsafe { std::slice::from_raw_parts(ids, n) };
    let out = unsafe { std::slice::from_raw_parts_mut(out, n) };
    let dict_len = usize::try_from(dict_len).unwrap_or(0);
    let dict: &[AlignStr] = if dict_len == 0 || dict.is_null() { &[] } else { unsafe { std::slice::from_raw_parts(dict, dict_len) } };
    let empty = AlignStr { ptr: core::ptr::NonNull::dangling().as_ptr(), len: 0 };
    for i in 0..n {
        out[i] = usize::try_from(ids[i]).ok().and_then(|id| dict.get(id).copied()).unwrap_or(empty);
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
    if n <= 0 || base.is_null() || out.is_null() || stride < 0 || off < 0 {
        return;
    }
    let n = n as usize;
    let (stride, off) = (stride as usize, off as usize);
    let out = unsafe { std::slice::from_raw_parts_mut(out, n) };
    for (i, slot) in out.iter_mut().enumerate() {
        let row = unsafe { base.add(i * stride) };
        *slot = unsafe { (row.add(off) as *const i64).read_unaligned() };
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

// ===================== JSON structural index (simdjson-style stage 1) =====================
//
// Stage 1 of a two-stage JSON parser: scan the input and emit the byte positions of the
// **structural** tokens — the punctuation `{ } [ ] : ,` that lies OUTSIDE string literals, plus
// every string-delimiter `"`. String interiors are masked so a `:` or `,` inside `"a,b"` is not
// mistaken for structure, and `\"` / `\\` escapes are handled (an escaped quote is not a delimiter).
// Stage 2 (a later slice) walks this index instead of stepping byte-by-byte — the lever that beat
// `serde_json` ~3.4–4.1× in the `work/` probe. This slice is the indexer + its correctness harness;
// it is wired into the decoder in the next slice.
//
// The fast path is AVX2 + carry-less multiply (runtime-detected, baseline-binary-safe — a CPU
// without AVX2 falls back to the scalar reference, which is also the oracle the SIMD is tested
// against). The scalar reference is the source of truth for the emitted set.

/// Scalar reference / fallback: emit structural-token positions (escape-aware). The SIMD path must
/// produce byte-for-byte the same `out`. The emitted set: each unescaped `"` (string delimiter), and
/// each `{ } [ ] : ,` that occurs outside a string.
#[cfg_attr(not(test), allow(dead_code))]
fn json_structural_index_scalar(src: &[u8], out: &mut Vec<u32>) {
    out.clear();
    out.reserve(src.len() / 8); // structural tokens are a fraction of the bytes; reuse keeps it a no-op
    // `esc` = the current byte is escaped (preceded by an odd-length backslash run). It suppresses
    // only a `"` (an escaped quote is not a delimiter) — NOT the punctuation, matching the SIMD's
    // `real_quote = quote & ~escaped` / `op & ~in_string` (escapes touch quotes, not `op`). This is
    // the exact spec the AVX2 path is tested against, so it is defined for any bytes (incl. invalid
    // JSON with a stray backslash); on valid JSON, backslashes only occur inside strings anyway.
    let mut in_string = false;
    let mut esc = false;
    for (i, &b) in src.iter().enumerate() {
        if b == b'"' && !esc {
            in_string = !in_string;
            out.push(i as u32);
        } else if !in_string && matches!(b, b'{' | b'}' | b'[' | b']' | b':' | b',') {
            out.push(i as u32);
        }
        esc = b == b'\\' && !esc;
    }
}

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

/// AVX2 structural index. Processes 64 bytes per iteration (two 32-byte vectors); a scalar tail
/// finishes the remainder continuing the carried string/escape state.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,pclmulqdq")]
unsafe fn json_structural_index_avx2(src: &[u8], out: &mut Vec<u32>) {
    use core::arch::x86_64::*;
    out.clear();
    out.reserve(src.len() / 8); // structural tokens are a fraction of the bytes; reuse keeps it a no-op
    let n = src.len();

    // Closures inherit the enclosing fn's enabled features, so the intrinsics are available; they
    // are `unsafe`, hence the inner `unsafe {}` (edition-2024 `unsafe_op_in_unsafe_fn`).
    let eqm = |v: __m256i, c: u8| -> u32 { _mm256_movemask_epi8(_mm256_cmpeq_epi8(v, _mm256_set1_epi8(c as i8))) as u32 };
    let op_bits = |v: __m256i| -> u32 { eqm(v, b'{') | eqm(v, b'}') | eqm(v, b'[') | eqm(v, b']') | eqm(v, b':') | eqm(v, b',') };

    let mut string_carry: u64 = 0; // 0 or all-ones: in-string parity at the end of the prev block
    let mut escape_carry: u64 = 0; // 0 or 1: a pending escape crossing the block boundary
    let mut i = 0usize;
    while i + 64 <= n {
        let p = unsafe { src.as_ptr().add(i) };
        let lo = unsafe { _mm256_loadu_si256(p as *const __m256i) };
        let hi = unsafe { _mm256_loadu_si256(p.add(32) as *const __m256i) };
        let quote = (eqm(lo, b'"') as u64) | ((eqm(hi, b'"') as u64) << 32);
        let backslash = (eqm(lo, b'\\') as u64) | ((eqm(hi, b'\\') as u64) << 32);
        let op = (op_bits(lo) as u64) | ((op_bits(hi) as u64) << 32);

        let escaped = find_escaped(backslash, &mut escape_carry);
        let real_quote = quote & !escaped; // an escaped \" is not a string delimiter
        let in_string = unsafe { prefix_xor(real_quote) } ^ string_carry;
        string_carry = (in_string as i64 >> 63) as u64; // sign-extend the top bit → 0 / all-ones

        // Structural = punctuation outside strings, plus every real delimiter quote.
        let mut bits = (op & !in_string) | real_quote;
        while bits != 0 {
            out.push(i as u32 + bits.trailing_zeros());
            bits &= bits - 1;
        }
        i += 64;
    }

    // Scalar tail, continuing the carried state (low bit of each carry is the live parity / flag).
    // Same semantics as `json_structural_index_scalar`: `esc` suppresses only a `"`, not `op`.
    let mut in_string = string_carry & 1 == 1;
    let mut esc = escape_carry & 1 == 1;
    while i < n {
        let b = unsafe { *src.get_unchecked(i) };
        if b == b'"' && !esc {
            in_string = !in_string;
            out.push(i as u32);
        } else if !in_string && matches!(b, b'{' | b'}' | b'[' | b']' | b':' | b',') {
            out.push(i as u32);
        }
        esc = b == b'\\' && !esc;
        i += 1;
    }
}

/// Fill `out` with the JSON structural-token positions (see [`json_structural_index_scalar`]).
/// Runtime-dispatched: AVX2 + `pclmulqdq` when present (baseline-binary-safe), else the scalar
/// reference. Wired into the decoder in a later slice.
#[cfg_attr(not(test), allow(dead_code))]
fn json_structural_index(src: &[u8], out: &mut Vec<u32>) {
    // Positions are `u32`, so a 4 GiB+ input would silently truncate/wrap — corrupting stage 2.
    // Reject it up front (simdjson has the same limit); a real document never approaches this.
    if src.len() > u32::MAX as usize {
        panic_abort("JSON input exceeds the 4 GiB structural-index limit");
    }
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("pclmulqdq") {
            unsafe { json_structural_index_avx2(src, out) };
            return;
        }
    }
    json_structural_index_scalar(src, out);
}

/// A **lean** decode index for the two-stage `array<Struct>` decoder: emits only `{ } [ ] :`
/// (structure + field separators) outside strings — **not** quotes or commas. The decoder navigates
/// records by the braces and fields by the colons, and recovers keys / `str` values by a short scan
/// of the raw bytes around the colon (so the quotes need not be in the index). Roughly a third the
/// size of [`json_structural_index`] for object-heavy input — and the index size dominates the decode
/// (`docs/open-questions.md` "JSON two-stage SIMD decode" autopsy), so the smaller index is the win.
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
    // `const` keeps it in `.rodata` (no per-call stack materialization).
    const WEIGHTS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
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
        return;
    }
    #[allow(unreachable_code)]
    json_decode_index_scalar(src, out);
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

/// A buffered writer over a raw file descriptor (`io.stdout.buffered()` → fd 1,
/// `io.stderr.buffered()` → fd 2): the sink-first fast path. Bytes accumulate in a fixed-capacity
/// buffer and reach the fd in one `write(2)` only when the buffer fills or on an explicit `flush` /
/// drop — so per-`write` calls do no syscall and memory stays O(buffer), not O(total output). Writes
/// go straight to the fd, skipping the `std::io::Stdout`/`Stderr` lock + line buffer.
pub struct BufferedWriter {
    /// The destination file descriptor (1 = stdout, 2 = stderr). Chosen by the constructor; every
    /// flush / large-chunk passthrough targets it.
    fd: i32,
    buf: Vec<u8>,
    /// Sticky: an internal flush (on a full buffer) failed. `write` returns `()`, so the error is
    /// latched here and surfaced by the next `flush` — matching `out.write(..); out.flush()?`.
    err: bool,
}

/// 64 KiB — large enough to amortize the syscall over many small writes, small enough to stay in
/// cache and bound memory.
const BUF_WRITER_CAP: usize = 64 * 1024;

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

impl BufferedWriter {
    /// Flush the buffer to the writer's fd, clearing it on success and latching `err` on failure.
    fn flush_buf(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        if write_all_fd(self.fd, &self.buf) {
            self.buf.clear();
        } else {
            self.err = true;
            self.buf.clear(); // drop the unwritten bytes; the latched error reports the loss
        }
    }
}

/// `io.stdout.buffered()` / `io.stderr.buffered()` — open a buffered writer over `fd` (1 = stdout,
/// 2 = stderr). Freed (after a final flush) by the generated `Drop` via [`align_rt_io_buf_free`].
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_io_buf_new(fd: i32) -> *mut BufferedWriter {
    Box::into_raw(Box::new(BufferedWriter { fd, buf: Vec::with_capacity(BUF_WRITER_CAP), err: false }))
}

/// `w.write(s)` — append a `str`'s bytes, flushing to the writer's fd only when the buffer would
/// overflow. A chunk larger than the whole buffer is written straight through (no buffering, no
/// double copy). Infallible at the surface; an internal flush failure is latched and surfaces at
/// the next `flush`.
///
/// # Safety
/// `w` must be a valid `BufferedWriter` pointer; `ptr`/`len` must describe a valid byte range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_buf_write(w: *mut BufferedWriter, ptr: *const u8, len: i64) {
    if w.is_null() || len <= 0 || ptr.is_null() {
        return;
    }
    let w = unsafe { &mut *w };
    let Ok(n) = usize::try_from(len) else { return };
    let bytes = unsafe { std::slice::from_raw_parts(ptr, n) };
    // If it won't fit in the remaining space, flush what's buffered first.
    if w.buf.len() + n > BUF_WRITER_CAP {
        w.flush_buf();
        if w.err {
            return;
        }
        // A chunk at least as big as the buffer would just be copied in and flushed right back
        // out — write it straight to the fd instead.
        if n >= BUF_WRITER_CAP {
            if !write_all_fd(w.fd, bytes) {
                w.err = true;
            }
            return;
        }
    }
    w.buf.extend_from_slice(bytes);
}

/// `w.flush()` — write any buffered bytes to the writer's fd. Returns 0 on success, 1 if this flush
/// or any earlier internal flush failed (the latched error is then cleared).
///
/// # Safety
/// `w` must be a valid `BufferedWriter` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_io_buf_flush(w: *mut BufferedWriter) -> i32 {
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
pub unsafe extern "C" fn align_rt_io_buf_free(w: *mut BufferedWriter) {
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
            std::slice::from_raw_parts(hptr, hlen as usize),
            std::slice::from_raw_parts(nptr, nlen as usize),
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
            std::slice::from_raw_parts(hptr, hlen as usize),
            std::slice::from_raw_parts(nptr, nlen as usize),
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
            std::slice::from_raw_parts(aptr, alen as usize),
            std::slice::from_raw_parts(bptr, blen as usize),
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
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
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
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
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
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    str_subview(bytes.trim_ascii_end())
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

/// Out-of-bounds range slice (`xs[start..end]`): report the whole `start..end` against `len` and
/// abort. Unlike an element index, a range has three ways to fail — a negative `start`, an inverted
/// `start > end`, or an over-length `end` — so all three values are reported together, avoiding the
/// `(index, len)` form's ambiguity (e.g. for an inverted range both bounds are individually valid).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_range_fail(start: i64, end: i64, len: i64) -> ! {
    eprintln!("align: panic: slice range out of bounds: {start}..{end} is not within length {len}");
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
pub unsafe extern "C" fn align_rt_arena_alloc(arena: *mut Arena, size: i64, align: i64) -> *mut u8 {
    let arena = unsafe { &mut *arena };
    arena.alloc(size as usize, align as usize)
}

/// Bulk-release every allocation, keeping the arena for reuse.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_arena_reset(arena: *mut Arena) {
    let arena = unsafe { &mut *arena };
    arena.chunks.clear();
    arena.off = 0;
}

/// Release every allocation and the arena itself.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_arena_end(arena: *mut Arena) {
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
pub unsafe extern "C" fn align_rt_tg_alloc(tg: *mut TaskGroup, size: i64, align: i64) -> *mut u8 {
    unsafe { &mut *tg }.arena.alloc(size as usize, align as usize)
}

/// Register a deferred task (its trampoline + closure pointer + env + result slot).
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

/// Run every registered task **in parallel** — spawn a worker thread per task, then join them all
/// (fork-join). All allocations happened at `spawn` time (on this thread), so no thread mutates
/// the region during the run; each worker only reads its own `env` and writes its own `slot`.
///
/// Uses `std::thread::scope` (like `align_rt_par_map`) so that *every* spawned thread is joined
/// before this returns even if a later `spawn` panics — otherwise an unwinding panic would detach
/// running threads and they would read the arena after `tg_end` frees it (a use-after-free).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_tg_wait(tg: *mut TaskGroup) -> *mut u8 {
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
    fn buffered_writer_accumulates_small_writes_without_flushing() {
        // Small writes stay buffered (no syscall, nothing reaches the fd): the buffer holds exactly
        // the concatenated bytes and no error is latched, and the writer records its target fd. The
        // flush and large-chunk pass-through paths are covered end-to-end (they necessarily touch a
        // real fd). fd 2 (stderr) is used so the buffered bytes, if ever flushed, don't pollute the
        // test harness's stdout.
        let w = align_rt_io_buf_new(2);
        for part in [&b"hello "[..], b"world", b"!"] {
            unsafe { align_rt_io_buf_write(w, part.as_ptr(), part.len() as i64) };
        }
        {
            let wr = unsafe { &mut *w };
            assert_eq!(wr.fd, 2, "writer targets the fd it was constructed with");
            assert_eq!(wr.buf, b"hello world!", "small writes accumulate, unflushed");
            assert!(!wr.err);
            wr.buf.clear(); // so the drop-flush below emits nothing
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
    fn json_structural_index_simd_matches_scalar_oracle() {
        // The AVX2 indexer must emit byte-for-byte the same positions as the scalar reference, on
        // every shape that stresses the string/escape masking and the 64-byte block carry.
        // (`is_x86_feature_detected!` can only be *named* on x86, so the detection lives inside the
        // `#[cfg(target_arch = "x86_64")]` block, not in a cross-arch `let`.)
        let check = |src: &[u8]| {
            let mut want = Vec::new();
            json_structural_index_scalar(src, &mut want);
            // The dispatch must equal the scalar oracle (it *is* the scalar path when no SIMD).
            let mut got = Vec::new();
            json_structural_index(src, &mut got);
            assert_eq!(got, want, "dispatch mismatch on {:?}", String::from_utf8_lossy(src));
            #[cfg(target_arch = "x86_64")]
            if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("pclmulqdq") {
                let mut g2 = Vec::new();
                unsafe { json_structural_index_avx2(src, &mut g2) };
                assert_eq!(g2, want, "avx2 mismatch on {:?}", String::from_utf8_lossy(src));
            }
        };

        // Hand-written shapes: objects, arrays, nesting, and every escape interaction.
        let cases: &[&[u8]] = &[
            b"",
            b"{}",
            b"[]",
            b"{\"a\":1}",
            b"[{\"active\":true,\"pay\":12},{\"active\":false,\"pay\":3}]",
            b"{\"s\":\"a,b:c{}[]\"}",          // structural chars INSIDE a string — must be ignored
            b"{\"s\":\"he said \\\"hi\\\"\"}", // escaped quotes \" inside the value
            b"{\"s\":\"back\\\\slash\"}",      // escaped backslash \\
            b"{\"s\":\"\\\\\\\"\"}",           // \\ then \" — the second quote is a real delimiter... no, escaped
            b"{\"a\":{\"b\":{\"c\":[1,2,3]}}}",
            b"{\"k\":\"\"}",                   // empty string value
            b"\"\\\\\"",                       // a string that is just an escaped backslash
        ];
        for c in cases {
            check(c);
        }

        // Block-boundary stress: place an escaped quote / backslash run at each offset across the
        // first two 64-byte blocks, so the escape + string carries are exercised at the seam.
        for pad in 55..75usize {
            let mut s = Vec::new();
            s.push(b'{');
            s.push(b'"');
            s.extend(std::iter::repeat(b'x').take(pad));
            s.extend_from_slice(b"\\\"end"); // an escaped quote straddling/near the boundary
            s.extend_from_slice(b"\":1}");
            check(&s);

            // A run of backslashes of varying length right at the boundary (parity matters).
            for run in 1..6usize {
                let mut t = Vec::new();
                t.extend_from_slice(b"{\"k\":\"");
                t.extend(std::iter::repeat(b'y').take(pad));
                t.extend(std::iter::repeat(b'\\').take(run));
                t.extend_from_slice(b"\"}"); // whether this " is escaped depends on run parity
                check(&t);
            }
        }

        // Pseudo-random fuzz: bytes drawn from a JSON-ish alphabet (so strings/escapes actually form).
        let mut state: u64 = 0x1234_5678_9abc_def0;
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
    fn json_decode_index_simd_matches_scalar_oracle() {
        // The SIMD decode index (AVX2 on x86_64, NEON on aarch64) must emit byte-for-byte the same
        // lean `{ } [ ] :` positions as the scalar reference, including the string/escape masking and
        // the 64-byte block carry. Same oracle discipline as the structural-index test.
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
            s.extend(std::iter::repeat(b'x').take(pad));
            s.extend_from_slice(b"\\\"end");
            s.extend_from_slice(b"\":1}");
            check(&s);
            for run in 1..6usize {
                let mut t = Vec::new();
                t.extend_from_slice(b"{\"k\":\"");
                t.extend(std::iter::repeat(b'y').take(pad));
                t.extend(std::iter::repeat(b'\\').take(run));
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
            let orig = unsafe { std::slice::from_raw_parts(r.key.ptr, r.key.len as usize) };
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
            keys.iter().zip(vals).map(|(k, &v)| (unsafe { std::slice::from_raw_parts(k.ptr, k.len as usize) }.to_vec(), v)).collect()
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
            for j in 0..8 {
                b[j] = unsafe { *out.ptr.add(off + j) };
            }
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
            if s.len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(s.ptr, s.len as usize) } }
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
}
