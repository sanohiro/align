//! par_map duel: Align `s.par_map(work).sum()` (persistent worker pool) vs Rust sequential and Rust
//! `rayon` (work-stealing pool). `bench/par_map/run.sh threshold` separately probes the
//! caller-only/pool boundary with a balanced median ratio after warming the pool. The `chunks`
//! mode measures the explicit chunk-header producer allocation against the same cursor work
//! without allocating headers.

use rayon::prelude::*;
use std::time::Instant;

#[repr(C)]
#[derive(Clone, Copy)]
struct Slice {
    ptr: *const i64,
    len: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[cfg(feature = "probe")]
struct ChunkHeader {
    ptr: *const u8,
    len: i64,
}

#[cfg(feature = "probe")]
struct OwnedChunkHeaders(*mut u8);

#[cfg(feature = "probe")]
impl OwnedChunkHeaders {
    fn new(ptr: *const u8) -> Self {
        Self(ptr.cast_mut())
    }
}

#[cfg(feature = "probe")]
impl Drop for OwnedChunkHeaders {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { align_rt_free(self.0) };
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
#[cfg(feature = "probe")]
struct SliceI8 {
    ptr: *const i8,
    len: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[cfg(feature = "probe")]
struct SliceI32 {
    ptr: *const i32,
    len: i64,
}

#[cfg(feature = "probe")]
type RangeKernel = extern "C" fn(*const u8, *const u8, *mut u8, i64, i64);

extern "C" {
    /// `pub fn pmap_cheap(s: slice<i64>) -> i64` — cheap vectorizable body.
    #[cfg(feature = "probe")]
    fn pmap_cheap(s: Slice) -> i64;
    /// `pub fn smap_cheap(s: slice<i64>) -> i64` — sequential cheap body.
    #[cfg(feature = "probe")]
    fn smap_cheap(s: Slice) -> i64;
    /// `pub fn pmap(s: slice<i64>) -> i64` — `s.par_map(work).sum()`.
    fn pmap(s: Slice) -> i64;
    /// `pub fn smap(s: slice<i64>) -> i64` — sequential `s.map(work).sum()`.
    fn smap(s: Slice) -> i64;
    /// `pub fn pfilter(s: slice<i64>) -> i64` — stable count/prefix/scatter filter followed by a cheap map.
    fn pfilter(s: Slice) -> i64;
    /// Width-probe exports, linked only for the benchmark's opt-in `probe` feature.
    #[cfg(feature = "probe")]
    fn pwidth_i8(s: SliceI8) -> i64;
    #[cfg(feature = "probe")]
    fn swidth_i8(s: SliceI8) -> i64;
    #[cfg(feature = "probe")]
    fn pwidth_i32(s: SliceI32) -> i64;
    #[cfg(feature = "probe")]
    fn swidth_i32(s: SliceI32) -> i64;
    #[cfg(feature = "probe")]
    fn pwidth_i64(s: Slice) -> i64;
    #[cfg(feature = "probe")]
    fn swidth_i64(s: Slice) -> i64;
    #[cfg(feature = "probe")]
    fn pwidth_i8_to_i64(s: SliceI8) -> i64;
    #[cfg(feature = "probe")]
    fn swidth_i8_to_i64(s: SliceI8) -> i64;
    #[cfg(feature = "probe")]
    fn pwidth_i64_to_i8(s: Slice) -> i64;
    #[cfg(feature = "probe")]
    fn swidth_i64_to_i8(s: Slice) -> i64;
    #[cfg(feature = "probe")]
    fn pwidth_materialize_i8_to_i64(s: SliceI8) -> i64;
    #[cfg(feature = "probe")]
    fn swidth_materialize_i8_to_i64(s: SliceI8) -> i64;
    #[cfg(feature = "probe")]
    fn pwidth_materialize_i64_to_i8(s: Slice) -> i64;
    #[cfg(feature = "probe")]
    fn swidth_materialize_i64_to_i8(s: Slice) -> i64;
    /// Benchmark-only runtime switch, present in the opt-in `par-map-probe` runtime build.
    #[cfg(feature = "probe")]
    fn align_rt_test_par_map_force_caller(force: i32);
    /// Benchmark-only threshold getter, present in the opt-in `par-map-probe` runtime build.
    #[cfg(feature = "probe")]
    fn align_rt_test_par_map_min_chunk() -> i64;
    /// Benchmark-only per-body threshold getter, present in the opt-in `par-map-probe` runtime
    /// build. It keeps the byte/work model in one place instead of copying it into the harness.
    #[cfg(feature = "probe")]
    fn align_rt_test_par_map_min_chunk_for(
        in_stride: i64,
        out_stride: i64,
        work_weight: i64,
    ) -> i64;
    /// Benchmark-only worker-count getter, present in the opt-in `par-map-probe` runtime build.
    #[cfg(feature = "probe")]
    fn align_rt_test_par_map_workers() -> i64;
    /// Benchmark-only direct runtime entry point for aggregate-like stride probes. This bypasses
    /// compiler-generated aggregate lowering deliberately; the probe owns the concrete record
    /// kernel and measures only the runtime's materializing range scheduler.
    #[cfg(feature = "probe")]
    fn align_rt_par_map(
        context: *const u8,
        in_buf: *const u8,
        count: i64,
        in_stride: i64,
        out_stride: i64,
        work_weight: i64,
        kernel: RangeKernel,
    ) -> *mut u8;
    /// The output from `align_rt_par_map` uses the runtime allocator and must be released by its
    /// matching C-ABI free function.
    #[cfg(feature = "probe")]
    fn align_rt_free(ptr: *mut u8);
    /// Benchmark-only direct `chunks` producer. The returned header buffer is owned by the caller
    /// and is released through `align_rt_free`; the element pointers borrow the input slice.
    #[cfg(feature = "probe")]
    fn align_rt_chunks(src: *const u8, src_len: i64, n: i64, elem_size: i64) -> ChunkHeader;
}

/// Must match the Align kernel's `work` (wrapping arithmetic = Align's defined i64 overflow).
#[inline]
fn work(x: i64) -> i64 {
    let mut a = x;
    a = a.wrapping_mul(2654435761).wrapping_add(12345);
    a = a.wrapping_mul(a).wrapping_add(7);
    a = a.wrapping_mul(40503).wrapping_sub(99);
    a
}

fn rust_seq(s: &[i64]) -> i64 {
    s.iter().map(|&x| work(x)).fold(0i64, i64::wrapping_add)
}

fn rust_rayon(s: &[i64]) -> i64 {
    s.par_iter()
        .map(|&x| work(x))
        .reduce(|| 0i64, i64::wrapping_add)
}

fn rust_filter_seq(s: &[i64]) -> i64 {
    let output: Vec<i64> = s
        .iter()
        .filter(|&&x| x % 2 == 0)
        .map(|&x| x.wrapping_mul(3).wrapping_add(1))
        .collect();
    output.iter().copied().fold(0i64, i64::wrapping_add)
}

fn gen(n: usize) -> Vec<i64> {
    let mut v = vec![0i64; n];
    let mut s: u64 = 0x9E3779B97F4A7C15;
    for d in v.iter_mut() {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *d = (s >> 33) as i64;
    }
    v
}

#[cfg(feature = "probe")]
type Kernel = unsafe extern "C" fn(Slice) -> i64;

#[cfg(feature = "probe")]
#[derive(Clone, Copy)]
struct ThresholdCase {
    name: &'static str,
    par: Kernel,
    seq: Kernel,
    work_weight: i64,
}

#[cfg(feature = "probe")]
type WidthKernel = unsafe fn(*const u8, i64) -> i64;

#[cfg(feature = "probe")]
#[derive(Clone, Copy)]
enum WidthSource {
    I8,
    I32,
    I64,
}

#[cfg(feature = "probe")]
#[derive(Clone, Copy)]
struct WidthCase {
    name: &'static str,
    source: WidthSource,
    par: WidthKernel,
    seq: WidthKernel,
    in_stride: i64,
    out_stride: i64,
    work_weight: i64,
}

#[cfg(feature = "probe")]
macro_rules! width_wrapper {
    ($wrapper:ident, $kernel:ident, $slice:ident) => {
        unsafe fn $wrapper(ptr: *const u8, len: i64) -> i64 {
            unsafe {
                $kernel($slice {
                    ptr: ptr.cast(),
                    len,
                })
            }
        }
    };
}

#[cfg(feature = "probe")]
width_wrapper!(call_pwidth_i8, pwidth_i8, SliceI8);
#[cfg(feature = "probe")]
width_wrapper!(call_swidth_i8, swidth_i8, SliceI8);
#[cfg(feature = "probe")]
width_wrapper!(call_pwidth_i32, pwidth_i32, SliceI32);
#[cfg(feature = "probe")]
width_wrapper!(call_swidth_i32, swidth_i32, SliceI32);
#[cfg(feature = "probe")]
width_wrapper!(call_pwidth_i64, pwidth_i64, Slice);
#[cfg(feature = "probe")]
width_wrapper!(call_swidth_i64, swidth_i64, Slice);
#[cfg(feature = "probe")]
width_wrapper!(call_pwidth_i8_to_i64, pwidth_i8_to_i64, SliceI8);
#[cfg(feature = "probe")]
width_wrapper!(call_swidth_i8_to_i64, swidth_i8_to_i64, SliceI8);
#[cfg(feature = "probe")]
width_wrapper!(call_pwidth_i64_to_i8, pwidth_i64_to_i8, Slice);
#[cfg(feature = "probe")]
width_wrapper!(call_swidth_i64_to_i8, swidth_i64_to_i8, Slice);
#[cfg(feature = "probe")]
width_wrapper!(
    call_pwidth_materialize_i8_to_i64,
    pwidth_materialize_i8_to_i64,
    SliceI8
);
#[cfg(feature = "probe")]
width_wrapper!(
    call_swidth_materialize_i8_to_i64,
    swidth_materialize_i8_to_i64,
    SliceI8
);
#[cfg(feature = "probe")]
width_wrapper!(
    call_pwidth_materialize_i64_to_i8,
    pwidth_materialize_i64_to_i8,
    Slice
);
#[cfg(feature = "probe")]
width_wrapper!(
    call_swidth_materialize_i64_to_i8,
    swidth_materialize_i64_to_i8,
    Slice
);

#[cfg(feature = "probe")]
struct WidthData {
    i8s: Vec<i8>,
    i32s: Vec<i32>,
    i64s: Vec<i64>,
}

#[cfg(feature = "probe")]
impl WidthData {
    fn new(n: usize) -> Self {
        let base = gen(n);
        Self {
            i8s: base.iter().map(|&x| x as i8).collect(),
            i32s: base.iter().map(|&x| x as i32).collect(),
            i64s: base,
        }
    }

    fn view(&self, source: WidthSource) -> (*const u8, i64) {
        match source {
            WidthSource::I8 => (self.i8s.as_ptr().cast(), self.i8s.len() as i64),
            WidthSource::I32 => (self.i32s.as_ptr().cast(), self.i32s.len() as i64),
            WidthSource::I64 => (self.i64s.as_ptr().cast(), self.i64s.len() as i64),
        }
    }
}

#[cfg(feature = "probe")]
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct AggregateRecord<const WORDS: usize> {
    words: [u64; WORDS],
}

#[cfg(feature = "probe")]
fn aggregate_transform<const WORDS: usize>(
    mut record: AggregateRecord<WORDS>,
) -> AggregateRecord<WORDS> {
    for (word_index, word) in record.words.iter_mut().enumerate() {
        let salt = 0x9E3779B97F4A7C15u64
            .wrapping_add((word_index as u64).wrapping_mul(0xD1B54A32D192ED03));
        *word = word
            .wrapping_add(salt)
            .rotate_left((word_index % 63) as u32);
    }
    record
}

#[cfg(feature = "probe")]
fn aggregate_kernel_impl<const WORDS: usize>(
    in_buf: *const u8,
    out_buf: *mut u8,
    start: i64,
    end: i64,
) {
    let Ok(start) = usize::try_from(start) else {
        return;
    };
    let Ok(end) = usize::try_from(end) else {
        return;
    };
    if end < start {
        return;
    }
    let input = in_buf.cast::<AggregateRecord<WORDS>>();
    let output = out_buf.cast::<AggregateRecord<WORDS>>();
    for index in start..end {
        let record = unsafe { input.add(index).read() };
        unsafe {
            output.add(index).write(aggregate_transform(record));
        }
    }
}

#[cfg(feature = "probe")]
macro_rules! aggregate_kernel_wrapper {
    ($name:ident, $words:literal) => {
        extern "C" fn $name(
            _context: *const u8,
            in_buf: *const u8,
            out_buf: *mut u8,
            start: i64,
            end: i64,
        ) {
            aggregate_kernel_impl::<$words>(in_buf, out_buf, start, end);
        }
    };
}

#[cfg(feature = "probe")]
aggregate_kernel_wrapper!(aggregate_kernel_16, 2);
#[cfg(feature = "probe")]
aggregate_kernel_wrapper!(aggregate_kernel_32, 4);
#[cfg(feature = "probe")]
aggregate_kernel_wrapper!(aggregate_kernel_64, 8);
#[cfg(feature = "probe")]
aggregate_kernel_wrapper!(aggregate_kernel_128, 16);

#[cfg(feature = "probe")]
fn aggregate_data<const WORDS: usize>(n: usize) -> Vec<AggregateRecord<WORDS>> {
    let mut state = 0x243F6A8885A308D3u64;
    let mut data = Vec::with_capacity(n);
    for row in 0..n {
        let mut words = [0u64; WORDS];
        for (word_index, word) in words.iter_mut().enumerate() {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *word = state ^ (row as u64).rotate_left((word_index % 63) as u32);
        }
        data.push(AggregateRecord { words });
    }
    data
}

#[cfg(feature = "probe")]
fn aggregate_checksum<const WORDS: usize>(data: &[AggregateRecord<WORDS>]) -> u64 {
    data.iter().enumerate().fold(0u64, |total, (row, record)| {
        record
            .words
            .iter()
            .enumerate()
            .fold(total, |total, (word_index, &word)| {
                let weight = (row as u64 + 1).wrapping_mul(word_index as u64 + 3);
                total.wrapping_add(word.wrapping_mul(weight))
            })
    })
}

#[cfg(feature = "probe")]
fn aggregate_seq_checksum<const WORDS: usize>(data: &[AggregateRecord<WORDS>]) -> u64 {
    let output: Vec<_> = data.iter().copied().map(aggregate_transform).collect();
    aggregate_checksum(&output)
}

#[cfg(feature = "probe")]
fn aggregate_runtime_output<const WORDS: usize>(
    data: &[AggregateRecord<WORDS>],
    kernel: RangeKernel,
) -> *mut u8 {
    let stride = std::mem::size_of::<AggregateRecord<WORDS>>() as i64;
    let output = unsafe {
        align_rt_par_map(
            std::ptr::null(),
            data.as_ptr().cast(),
            data.len() as i64,
            stride,
            stride,
            1,
            kernel,
        )
    };
    assert!(!output.is_null(), "aggregate probe returned a null output");
    output
}

#[cfg(feature = "probe")]
fn aggregate_runtime_checksum<const WORDS: usize>(
    data: &[AggregateRecord<WORDS>],
    kernel: RangeKernel,
) -> u64 {
    let output = aggregate_runtime_output(data, kernel);
    let output_slice =
        unsafe { std::slice::from_raw_parts(output.cast::<AggregateRecord<WORDS>>(), data.len()) };
    let checksum = std::hint::black_box(aggregate_checksum(output_slice));
    unsafe { align_rt_free(output) };
    checksum
}

#[cfg(feature = "probe")]
#[inline(never)]
fn checked_chunk_entry_checksum(
    data: &[i64],
    ptr: *const u8,
    len: i64,
    start: usize,
    expected_len: usize,
) -> u64 {
    let byte_offset = std::hint::black_box(ptr as usize)
        .checked_sub(std::hint::black_box(data.as_ptr() as usize))
        .expect("chunk pointer precedes the source");
    assert_eq!(
        byte_offset % std::mem::size_of::<i64>(),
        0,
        "chunk pointer is not element-aligned"
    );
    let offset = byte_offset / std::mem::size_of::<i64>();
    assert!(offset < data.len(), "chunk pointer is outside the source");
    assert_eq!(offset, start, "chunk pointer is out of source order");
    let len =
        usize::try_from(std::hint::black_box(len)).expect("chunk length must be non-negative");
    assert!(len <= data.len(), "chunk length is outside the source");
    assert_eq!(
        len, expected_len,
        "chunk length does not match the source layout"
    );
    (offset as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(len as u64)
}

#[cfg(feature = "probe")]
fn chunk_cursor_checksum(data: &[i64], chunk: usize) -> u64 {
    assert!(chunk > 0);
    let mut start = 0usize;
    let mut checksum = 0u64;
    while start < data.len() {
        let len = chunk.min(data.len() - start);
        let ptr = unsafe { data.as_ptr().add(start).cast() };
        checksum = checksum.wrapping_add(checked_chunk_entry_checksum(
            data, ptr, len as i64, start, len,
        ));
        start += len;
    }
    std::hint::black_box(checksum)
}

#[cfg(feature = "probe")]
fn materialized_chunk_checksum(data: &[i64], chunk: usize) -> u64 {
    assert!(chunk > 0);
    let headers = unsafe {
        align_rt_chunks(
            data.as_ptr().cast(),
            data.len() as i64,
            chunk as i64,
            std::mem::size_of::<i64>() as i64,
        )
    };
    let _owned_headers = OwnedChunkHeaders::new(headers.ptr);
    assert!(
        !headers.ptr.is_null(),
        "chunks producer returned no header buffer"
    );
    let expected_count = data.len().div_ceil(chunk);
    let expected_count = i64::try_from(expected_count).expect("chunks count must fit the ABI");
    assert!(
        headers.len >= 0 && headers.len <= expected_count,
        "chunks producer returned an out-of-range header count"
    );
    assert_eq!(
        headers.len, expected_count,
        "chunks producer returned an unexpected header count"
    );
    let count = usize::try_from(headers.len).expect("chunks producer returned a valid count");
    let entries = unsafe { std::slice::from_raw_parts(headers.ptr.cast::<ChunkHeader>(), count) };
    let mut checksum = 0u64;
    for (index, entry) in entries.iter().enumerate() {
        let start = index
            .checked_mul(chunk)
            .expect("chunk start offset must fit usize");
        let expected_len = chunk.min(data.len() - start);
        checksum = checksum.wrapping_add(checked_chunk_entry_checksum(
            data,
            entry.ptr,
            entry.len,
            start,
            expected_len,
        ));
    }
    std::hint::black_box(checksum)
}

#[cfg(feature = "probe")]
fn validate_materialized_chunks(data: &[i64], chunk: usize) {
    let headers = unsafe {
        align_rt_chunks(
            data.as_ptr().cast(),
            data.len() as i64,
            chunk as i64,
            std::mem::size_of::<i64>() as i64,
        )
    };
    let _owned_headers = OwnedChunkHeaders::new(headers.ptr);
    assert!(
        !headers.ptr.is_null(),
        "chunks producer returned no header buffer"
    );
    let expected_count = data.len().div_ceil(chunk);
    let expected_count = i64::try_from(expected_count).expect("chunks count must fit the ABI");
    assert!(
        headers.len >= 0 && headers.len <= expected_count,
        "chunks producer returned an out-of-range header count"
    );
    assert_eq!(
        headers.len, expected_count,
        "chunks producer returned an unexpected header count"
    );
    let count = usize::try_from(headers.len).expect("chunks producer returned a valid count");
    let entries = unsafe { std::slice::from_raw_parts(headers.ptr.cast::<ChunkHeader>(), count) };
    for (index, entry) in entries.iter().enumerate() {
        let start = index
            .checked_mul(chunk)
            .expect("chunk start offset must fit usize");
        let expected_len = chunk.min(data.len() - start);
        checked_chunk_entry_checksum(data, entry.ptr, entry.len, start, expected_len);
    }
}

#[cfg(feature = "probe")]
fn elapsed_chunk_materialize_ms(data: &[i64], chunk: usize, reps: usize) -> (f64, u64) {
    let started = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..reps {
        checksum = checksum.wrapping_add(materialized_chunk_checksum(data, chunk));
    }
    (started.elapsed().as_secs_f64() * 1e3, checksum)
}

#[cfg(feature = "probe")]
fn elapsed_chunk_cursor_ms(data: &[i64], chunk: usize, reps: usize) -> (f64, u64) {
    let started = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..reps {
        checksum = checksum.wrapping_add(chunk_cursor_checksum(data, chunk));
    }
    (started.elapsed().as_secs_f64() * 1e3, checksum)
}

#[cfg(feature = "probe")]
fn run_chunks() {
    const ROUNDS: usize = 15;
    const SOURCE_LEN: usize = 1_000_000;
    const TARGET_HEADERS: usize = 1_000_000;
    let data = gen(SOURCE_LEN);
    let runtime_workers = usize::try_from(unsafe { align_rt_test_par_map_workers() })
        .expect("runtime par_map worker count must be positive");
    let warm = materialized_chunk_checksum(&data, 1024);
    std::hint::black_box(warm);

    println!(
        "par_map chunks header probe ({ROUNDS} balanced samples, {runtime_workers} workers; source {SOURCE_LEN} i64 elements)"
    );
    println!(
        "Materialized = align_rt_chunks malloc/fill/free; cursor = same chunk pointer/length work without a header allocation."
    );
    println!(
        "{:>10}  {:>10}  {:>10}  {:>19}  {:>17}  {:>17}",
        "chunk", "headers", "reps", "materialize ms", "cursor ms", "materialize/cursor"
    );
    for &chunk in &[1usize, 2, 8, 64, 256, 1024] {
        let headers = SOURCE_LEN.div_ceil(chunk);
        let reps = (TARGET_HEADERS / headers).max(1);
        // Validate a fresh producer result before timing. Both timed arms use the same helper, so
        // their validation and checksum work remains symmetric.
        validate_materialized_chunks(&data, chunk);
        let expected = chunk_cursor_checksum(&data, chunk);
        assert_eq!(materialized_chunk_checksum(&data, chunk), expected);

        let mut materialized = Vec::with_capacity(ROUNDS);
        let mut cursor = Vec::with_capacity(ROUNDS);
        for round in 0..ROUNDS {
            let order = if round % 2 == 0 { [0, 1] } else { [1, 0] };
            let mut elapsed = [0.0; 2];
            let mut checksums = [0u64; 2];
            for arm in order {
                (elapsed[arm], checksums[arm]) = match arm {
                    0 => elapsed_chunk_materialize_ms(&data, chunk, reps),
                    1 => elapsed_chunk_cursor_ms(&data, chunk, reps),
                    _ => unreachable!(),
                };
            }
            let expected_total = expected.wrapping_mul(reps as u64);
            assert_eq!(
                checksums, [expected_total; 2],
                "chunk checksum changed for chunk={chunk}"
            );
            materialized.push(elapsed[0]);
            cursor.push(elapsed[1]);
        }
        let materialized_ms = median(materialized);
        let cursor_ms = median(cursor);
        println!(
            "{chunk:>10}  {headers:>10}  {reps:>10}  {materialized_ms:>19.3}  {cursor_ms:>17.3}  {:>17.3}x",
            materialized_ms / cursor_ms,
        );
    }
}

#[cfg(feature = "probe")]
fn aggregate_runtime_validate<const WORDS: usize>(
    data: &[AggregateRecord<WORDS>],
    kernel: RangeKernel,
    expected: &[AggregateRecord<WORDS>],
    caller_only: bool,
    name: &str,
) {
    if caller_only {
        unsafe { align_rt_test_par_map_force_caller(1) };
    }
    let output = aggregate_runtime_output(data, kernel);
    let output_slice =
        unsafe { std::slice::from_raw_parts(output.cast::<AggregateRecord<WORDS>>(), data.len()) };
    let matches = output_slice == expected;
    unsafe { align_rt_free(output) };
    if caller_only {
        unsafe { align_rt_test_par_map_force_caller(0) };
    }
    assert!(matches, "aggregate runtime output changed for {name}");
}

#[cfg(feature = "probe")]
fn elapsed_aggregate_runtime_ms<const WORDS: usize>(
    data: &[AggregateRecord<WORDS>],
    kernel: RangeKernel,
    caller_only: bool,
    reps: usize,
) -> (f64, u64) {
    if caller_only {
        unsafe { align_rt_test_par_map_force_caller(1) };
    }
    let started = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..reps {
        checksum = checksum.wrapping_add(std::hint::black_box(aggregate_runtime_checksum(
            data, kernel,
        )));
    }
    let elapsed = started.elapsed().as_secs_f64() * 1e3;
    if caller_only {
        unsafe { align_rt_test_par_map_force_caller(0) };
    }
    (elapsed, checksum)
}

#[cfg(feature = "probe")]
fn elapsed_aggregate_seq_ms<const WORDS: usize>(
    data: &[AggregateRecord<WORDS>],
    reps: usize,
) -> (f64, u64) {
    let started = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..reps {
        checksum = checksum.wrapping_add(std::hint::black_box(aggregate_seq_checksum(data)));
    }
    (started.elapsed().as_secs_f64() * 1e3, checksum)
}

#[cfg(feature = "probe")]
fn run_aggregate_case<const WORDS: usize>(
    name: &str,
    kernel: RangeKernel,
    rounds: usize,
    target_elements: usize,
) {
    let stride = std::mem::size_of::<AggregateRecord<WORDS>>();
    assert!(matches!(stride, 16 | 32 | 64 | 128));
    let floor = usize::try_from(unsafe {
        align_rt_test_par_map_min_chunk_for(stride as i64, stride as i64, 1)
    })
    .expect("runtime par_map aggregate floor must be non-negative");
    let delta = (floor / 8).max(1);
    let counts = [
        floor.saturating_sub(delta).max(1),
        floor,
        floor.saturating_add(1),
        floor.saturating_add(delta),
    ];

    for &n in &counts {
        let data = aggregate_data::<WORDS>(n);
        let expected_output: Vec<_> = data.iter().copied().map(aggregate_transform).collect();
        aggregate_runtime_validate(&data, kernel, &expected_output, false, name);
        aggregate_runtime_validate(&data, kernel, &expected_output, true, name);
        let expected = aggregate_checksum(&expected_output);
        let reps = (target_elements / n).max(1);
        let mut pool_seq_ratios = Vec::with_capacity(rounds);
        let mut pool_caller_ratios = Vec::with_capacity(rounds);
        for round in 0..rounds {
            let order = match round % 6 {
                0 => [0, 1, 2],
                1 => [2, 1, 0],
                2 => [1, 0, 2],
                3 => [2, 0, 1],
                4 => [1, 2, 0],
                _ => [0, 2, 1],
            };
            let mut elapsed = [0.0; 3];
            let mut checksums = [0u64; 3];
            for arm in order {
                (elapsed[arm], checksums[arm]) = match arm {
                    0 => elapsed_aggregate_runtime_ms(&data, kernel, false, reps),
                    1 => elapsed_aggregate_runtime_ms(&data, kernel, true, reps),
                    _ => elapsed_aggregate_seq_ms(&data, reps),
                };
            }
            let expected_total = expected.wrapping_mul(reps as u64);
            assert_eq!(
                checksums, [expected_total; 3],
                "aggregate checksum changed for {name}, n={n}"
            );
            pool_seq_ratios.push(elapsed[0] / elapsed[2]);
            pool_caller_ratios.push(elapsed[0] / elapsed[1]);
        }
        pool_seq_ratios.sort_by(f64::total_cmp);
        pool_caller_ratios.sort_by(f64::total_cmp);
        println!(
            "{name:>19}  {n:>9}  {floor:>12}  {:>18.3}  {:>19.3}  {:.3}..{:.3}",
            percentile(&pool_seq_ratios, 0.5),
            percentile(&pool_caller_ratios, 0.5),
            percentile(&pool_seq_ratios, 0.1),
            percentile(&pool_seq_ratios, 0.9),
        );
    }
}

#[cfg(feature = "probe")]
fn call(kernel: Kernel, slice: Slice) -> i64 {
    // The benchmark exports are checked Align functions with the same C ABI; the harness owns the
    // backing Vec for the full duration of every call.
    unsafe { kernel(slice) }
}

#[cfg(feature = "probe")]
fn elapsed_ms(kernel: Kernel, slice: Slice, reps: usize) -> f64 {
    let start = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(call(kernel, slice));
    }
    start.elapsed().as_secs_f64() * 1e3
}

#[cfg(feature = "probe")]
fn elapsed_caller_ms(kernel: Kernel, slice: Slice, reps: usize) -> f64 {
    unsafe { align_rt_test_par_map_force_caller(1) };
    let elapsed = elapsed_ms(kernel, slice, reps);
    unsafe { align_rt_test_par_map_force_caller(0) };
    elapsed
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let index = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[index]
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    percentile(&samples, 0.5)
}

fn run_standard() {
    let rounds = 50;
    let profile = std::env::var_os("ALIGN_BENCH_PROFILE").is_some();
    println!("par_map(work).sum() — Align (pool) vs Rust sequential / rayon");
    println!(
        "{:>9}  {:>10}  {:>10}  {:>10}  {:>9}  {:>9}",
        "n", "align ms", "seq ms", "rayon ms", "vs seq", "vs rayon"
    );
    for &n in &[1_000usize, 10_000, 100_000, 1_000_000] {
        let data = gen(n);
        let sl = Slice {
            ptr: data.as_ptr(),
            len: n as i64,
        };

        // Correctness: Align (pool, parallel) must equal the sequential fold (no races / lost work).
        let a0 = unsafe {
            pmap(Slice {
                ptr: sl.ptr,
                len: sl.len,
            })
        };
        assert_eq!(a0, rust_seq(&data), "align vs sequential");
        assert_eq!(a0, rust_rayon(&data), "align vs rayon");
        assert_eq!(
            unsafe {
                smap(Slice {
                    ptr: sl.ptr,
                    len: sl.len,
                })
            },
            a0,
            "align sequential vs par_map"
        );

        let mut align_samples = Vec::with_capacity(rounds);
        let mut seq_samples = Vec::with_capacity(rounds);
        let mut rayon_samples = Vec::with_capacity(rounds);
        let mut align_seq_samples = Vec::with_capacity(rounds);
        for round in 0..rounds {
            let align = || {
                let t = Instant::now();
                std::hint::black_box(unsafe {
                    pmap(Slice {
                        ptr: sl.ptr,
                        len: sl.len,
                    })
                });
                t.elapsed().as_secs_f64() * 1e3
            };
            let seq = || {
                let t = Instant::now();
                std::hint::black_box(rust_seq(&data));
                t.elapsed().as_secs_f64() * 1e3
            };
            let rayon = || {
                let t = Instant::now();
                std::hint::black_box(rust_rayon(&data));
                t.elapsed().as_secs_f64() * 1e3
            };
            let align_seq = || {
                let t = Instant::now();
                std::hint::black_box(unsafe {
                    smap(Slice {
                        ptr: sl.ptr,
                        len: sl.len,
                    })
                });
                t.elapsed().as_secs_f64() * 1e3
            };
            if round % 2 == 0 {
                align_samples.push(align());
                seq_samples.push(seq());
                rayon_samples.push(rayon());
                if profile {
                    align_seq_samples.push(align_seq());
                }
            } else {
                rayon_samples.push(rayon());
                seq_samples.push(seq());
                align_samples.push(align());
                if profile {
                    align_seq_samples.push(align_seq());
                }
            }
        }
        let am = median(align_samples);
        let sm = median(seq_samples);
        let rm = median(rayon_samples);
        println!(
            "{:>9}  {:>10.3}  {:>10.3}  {:>10.3}  {:>8.2}x  {:>8.2}x",
            n,
            am,
            sm,
            rm,
            sm / am,
            rm / am
        );
        if profile {
            let align_seq = median(align_seq_samples);
            println!(
                "profile n={n}: align-seq {:8.3} ms; pmap is {:5.2}x align-seq",
                align_seq,
                am / align_seq
            );
        }
    }
}

#[cfg(feature = "probe")]
fn run_threshold() {
    // Keep the expected value explicit for the checked-in measurement table, but read the actual
    // runtime value too so a future retune cannot silently leave the probe out of sync.
    const EXPECTED_PAR_MIN_CHUNK: usize = 65_536;
    let runtime_min = usize::try_from(unsafe { align_rt_test_par_map_min_chunk() })
        .expect("runtime par_map threshold must be non-negative");
    assert_eq!(
        runtime_min, EXPECTED_PAR_MIN_CHUNK,
        "update the checked-in threshold probe after changing the runtime threshold"
    );
    let runtime_workers = usize::try_from(unsafe { align_rt_test_par_map_workers() })
        .expect("runtime par_map worker count must be positive");
    if runtime_workers <= 1 {
        println!("par_map threshold probe skipped: runtime reports {runtime_workers} worker; the pool path is intentionally disabled on a one-worker host");
        return;
    }
    const ROUNDS: usize = 31;
    let counts = [
        16_384usize,
        32_768,
        32_769,
        49_152,
        65_535,
        65_536,
        65_537,
        73_728,
        81_920,
        98_304,
        131_072,
        196_608,
        262_144,
    ];
    let cases = [
        ThresholdCase {
            name: "cheap",
            par: pmap_cheap,
            seq: smap_cheap,
            work_weight: 1,
        },
        ThresholdCase {
            name: "heavy",
            par: pmap,
            seq: smap,
            work_weight: 2,
        },
    ];

    // Initialize the persistent pool once, so the table measures the steady-state choice at the
    // boundary. Cold-start behavior is pinned separately by the runtime integration test.
    let warm = gen(runtime_min * 2);
    let warm_slice = Slice {
        ptr: warm.as_ptr(),
        len: warm.len() as i64,
    };
    std::hint::black_box(call(pmap, warm_slice));

    println!("par_map threshold probe (warm pool, {ROUNDS} balanced ratio samples, {runtime_workers} workers)");
    println!("cheap floor={} elements; heavy floor={} elements; floors use input/output bytes plus the compiler body hint", unsafe {
        align_rt_test_par_map_min_chunk_for(8, 8, 1)
    }, unsafe {
        align_rt_test_par_map_min_chunk_for(8, 8, 2)
    });
    println!(
        "{:>9}  {:>8}  {:>18}  {:>18}  {:>18}  {:>12}",
        "n", "case", "median pool/seq", "median pool/caller", "pool/seq p10..p90", "floor"
    );
    for &n in &counts {
        let data = gen(n);
        let slice = Slice {
            ptr: data.as_ptr(),
            len: n as i64,
        };
        // Batch small maps so each timing contains roughly one million body elements. Repeating
        // the same call preserves the per-call scheduler cost while making the clock resolution
        // and transient worker wakeups a small part of each sample.
        let reps = (1_048_576usize / n).max(1);
        for case in cases {
            let case_min_chunk = usize::try_from(unsafe {
                align_rt_test_par_map_min_chunk_for(8, 8, case.work_weight)
            })
            .expect("runtime par_map threshold must be non-negative");
            let expected = call(case.seq, slice);
            assert_eq!(call(case.par, slice), expected, "{} n={n}", case.name);
            // Cycle through all six permutations so pool, caller, and sequential each occupy every
            // timing slot. The median of ratios keeps correlated frequency drift in the pair.
            let mut pool_seq_ratios = Vec::with_capacity(ROUNDS);
            let mut pool_caller_ratios = Vec::with_capacity(ROUNDS);
            for round in 0..ROUNDS {
                let order = match round % 6 {
                    0 => [0, 1, 2],
                    1 => [2, 1, 0],
                    2 => [1, 0, 2],
                    3 => [2, 0, 1],
                    4 => [1, 2, 0],
                    _ => [0, 2, 1],
                };
                let mut elapsed = [0.0; 3];
                for arm in order {
                    elapsed[arm] = match arm {
                        0 => elapsed_ms(case.par, slice, reps),
                        1 => elapsed_caller_ms(case.par, slice, reps),
                        _ => elapsed_ms(case.seq, slice, reps),
                    };
                }
                let [pool_ms, caller_ms, seq_ms] = elapsed;
                pool_seq_ratios.push(pool_ms / seq_ms);
                pool_caller_ratios.push(pool_ms / caller_ms);
            }
            pool_seq_ratios.sort_by(f64::total_cmp);
            pool_caller_ratios.sort_by(f64::total_cmp);
            let pool_seq_median = percentile(&pool_seq_ratios, 0.5);
            let pool_caller_median = percentile(&pool_caller_ratios, 0.5);
            let p10 = percentile(&pool_seq_ratios, 0.1);
            let p90 = percentile(&pool_seq_ratios, 0.9);
            println!(
                "{n:>9}  {:>8}  {pool_seq_median:>18.3}  {pool_caller_median:>18.3}  {p10:.3}..{p90:.3}  {case_min_chunk:>12}",
                case.name,
            );
        }
    }
}

#[cfg(feature = "probe")]
fn elapsed_width_ms(kernel: WidthKernel, ptr: *const u8, len: i64, reps: usize) -> f64 {
    let start = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(unsafe { kernel(ptr, len) });
    }
    start.elapsed().as_secs_f64() * 1e3
}

#[cfg(feature = "probe")]
fn elapsed_width_caller_ms(kernel: WidthKernel, ptr: *const u8, len: i64, reps: usize) -> f64 {
    unsafe { align_rt_test_par_map_force_caller(1) };
    let elapsed = elapsed_width_ms(kernel, ptr, len, reps);
    unsafe { align_rt_test_par_map_force_caller(0) };
    elapsed
}

#[cfg(feature = "probe")]
fn run_width() {
    let runtime_workers = usize::try_from(unsafe { align_rt_test_par_map_workers() })
        .expect("runtime par_map worker count must be positive");
    if runtime_workers <= 1 {
        println!(
            "par_map width probe skipped: runtime reports {runtime_workers} worker; the pool path is intentionally disabled on a one-worker host"
        );
        return;
    }

    const ROUNDS: usize = 7;
    const TARGET_ELEMENTS: usize = 262_144;
    let cases = [
        WidthCase {
            name: "reduce i8 -> i8",
            source: WidthSource::I8,
            par: call_pwidth_i8,
            seq: call_swidth_i8,
            in_stride: 1,
            out_stride: 1,
            work_weight: 1,
        },
        WidthCase {
            name: "reduce i32 -> i32",
            source: WidthSource::I32,
            par: call_pwidth_i32,
            seq: call_swidth_i32,
            in_stride: 4,
            out_stride: 4,
            work_weight: 1,
        },
        WidthCase {
            name: "reduce i64 -> i64",
            source: WidthSource::I64,
            par: call_pwidth_i64,
            seq: call_swidth_i64,
            in_stride: 8,
            out_stride: 8,
            work_weight: 1,
        },
        WidthCase {
            name: "reduce i8 -> i64",
            source: WidthSource::I8,
            par: call_pwidth_i8_to_i64,
            seq: call_swidth_i8_to_i64,
            in_stride: 1,
            out_stride: 8,
            work_weight: 1,
        },
        WidthCase {
            name: "reduce i64 -> i8",
            source: WidthSource::I64,
            par: call_pwidth_i64_to_i8,
            seq: call_swidth_i64_to_i8,
            in_stride: 8,
            out_stride: 1,
            work_weight: 1,
        },
        WidthCase {
            name: "materialize i8 -> i64",
            source: WidthSource::I8,
            par: call_pwidth_materialize_i8_to_i64,
            seq: call_swidth_materialize_i8_to_i64,
            in_stride: 1,
            out_stride: 8,
            work_weight: 1,
        },
        WidthCase {
            name: "materialize i64 -> i8",
            source: WidthSource::I64,
            par: call_pwidth_materialize_i64_to_i8,
            seq: call_swidth_materialize_i64_to_i8,
            in_stride: 8,
            out_stride: 1,
            work_weight: 1,
        },
    ];

    // Start the persistent pool once. Every case then compares warm pool and caller-only choices
    // for the same generated kernel and data shape.
    let warm = WidthData::new(65_536 * 2);
    let (warm_ptr, warm_len) = warm.view(WidthSource::I64);
    std::hint::black_box(unsafe { call_pwidth_i64(warm_ptr, warm_len) });

    println!(
        "par_map width probe ({ROUNDS} balanced samples, {runtime_workers} workers; target {TARGET_ELEMENTS} elements per timing)"
    );
    println!(
        "The runtime floor is reported for each input/output stride; counts are floor-δ, floor, floor+1, and floor+δ."
    );
    println!(
        "{:>23}  {:>9}  {:>12}  {:>18}  {:>19}  {:>18}",
        "case", "n", "floor", "median pool/seq", "median pool/caller", "pool/seq p10..p90"
    );

    for case in cases {
        let floor = usize::try_from(unsafe {
            align_rt_test_par_map_min_chunk_for(case.in_stride, case.out_stride, case.work_weight)
        })
        .expect("runtime par_map threshold must be non-negative");
        let delta = (floor / 8).max(1);
        let counts = [
            floor.saturating_sub(delta).max(1),
            floor,
            floor.saturating_add(1),
            floor.saturating_add(delta),
        ];

        for &n in &counts {
            let data = WidthData::new(n);
            let (ptr, len) = data.view(case.source);
            let expected = unsafe { (case.seq)(ptr, len) };
            assert_eq!(
                unsafe { (case.par)(ptr, len) },
                expected,
                "{} n={n}",
                case.name
            );
            let reps = (TARGET_ELEMENTS / n).max(1);

            // Six permutations put pool, caller-only, and sequential controls in every timing
            // position. The paired median is less sensitive to frequency drift than raw samples.
            let mut pool_seq_ratios = Vec::with_capacity(ROUNDS);
            let mut pool_caller_ratios = Vec::with_capacity(ROUNDS);
            for round in 0..ROUNDS {
                let order = match round % 6 {
                    0 => [0, 1, 2],
                    1 => [2, 1, 0],
                    2 => [1, 0, 2],
                    3 => [2, 0, 1],
                    4 => [1, 2, 0],
                    _ => [0, 2, 1],
                };
                let mut elapsed = [0.0; 3];
                for arm in order {
                    elapsed[arm] = match arm {
                        0 => elapsed_width_ms(case.par, ptr, len, reps),
                        1 => elapsed_width_caller_ms(case.par, ptr, len, reps),
                        _ => elapsed_width_ms(case.seq, ptr, len, reps),
                    };
                }
                let [pool_ms, caller_ms, seq_ms] = elapsed;
                pool_seq_ratios.push(pool_ms / seq_ms);
                pool_caller_ratios.push(pool_ms / caller_ms);
            }
            pool_seq_ratios.sort_by(f64::total_cmp);
            pool_caller_ratios.sort_by(f64::total_cmp);
            let pool_seq_median = percentile(&pool_seq_ratios, 0.5);
            let pool_caller_median = percentile(&pool_caller_ratios, 0.5);
            let p10 = percentile(&pool_seq_ratios, 0.1);
            let p90 = percentile(&pool_seq_ratios, 0.9);
            println!(
                "{:>23}  {n:>9}  {floor:>12}  {pool_seq_median:>18.3}  {pool_caller_median:>19.3}  {p10:.3}..{p90:.3}",
                case.name,
            );
        }
    }
}

#[cfg(feature = "probe")]
fn run_aggregate() {
    let runtime_workers = usize::try_from(unsafe { align_rt_test_par_map_workers() })
        .expect("runtime par_map worker count must be positive");
    if runtime_workers <= 1 {
        println!(
            "par_map aggregate stride probe skipped: runtime reports {runtime_workers} worker; the pool path is intentionally disabled on a one-worker host"
        );
        return;
    }

    const ROUNDS: usize = 7;
    const TARGET_ELEMENTS: usize = 131_072;
    // Warm the process-lifetime pool before the first record shape. The probe is about the
    // steady-state range scheduler; cold-start behavior belongs to the separate runtime gate.
    let warm = aggregate_data::<2>(131_072);
    std::hint::black_box(aggregate_runtime_checksum(&warm, aggregate_kernel_16));

    println!(
        "par_map aggregate stride probe ({ROUNDS} balanced samples, {runtime_workers} workers; target {TARGET_ELEMENTS} elements per timing)"
    );
    println!(
        "Runtime-only materializing probe: aggregate compiler forms remain sequential; counts are floor-δ, floor, floor+1, and floor+δ."
    );
    println!(
        "{:>19}  {:>9}  {:>12}  {:>18}  {:>19}  {:>18}",
        "record", "n", "floor", "median pool/seq", "median pool/caller", "pool/seq p10..p90"
    );
    run_aggregate_case::<2>(
        "record 16 bytes",
        aggregate_kernel_16,
        ROUNDS,
        TARGET_ELEMENTS,
    );
    run_aggregate_case::<4>(
        "record 32 bytes",
        aggregate_kernel_32,
        ROUNDS,
        TARGET_ELEMENTS,
    );
    run_aggregate_case::<8>(
        "record 64 bytes",
        aggregate_kernel_64,
        ROUNDS,
        TARGET_ELEMENTS,
    );
    run_aggregate_case::<16>(
        "record 128 bytes",
        aggregate_kernel_128,
        ROUNDS,
        TARGET_ELEMENTS,
    );
}

fn run_filter() {
    const ROUNDS: usize = 15;
    println!(
        "par_map filter (stable count/prefix/scatter vs Rust materializing sequential control)"
    );
    println!(
        "{:>9}  {:>12}  {:>12}  {:>10}",
        "n", "parallel ms", "rust seq ms", "vs Rust"
    );
    for &n in &[16_384usize, 65_536, 65_537, 131_072, 1_000_000] {
        let data = gen(n);
        let slice = Slice {
            ptr: data.as_ptr(),
            len: n as i64,
        };
        let expected = rust_filter_seq(&data);
        assert_eq!(
            unsafe { pfilter(slice) },
            expected,
            "parallel filter result"
        );

        let mut parallel_samples = Vec::with_capacity(ROUNDS);
        let mut rust_seq_samples = Vec::with_capacity(ROUNDS);
        for round in 0..ROUNDS {
            let mut elapsed = [0.0; 2];
            let order = match round % 2 {
                0 => [0, 1],
                _ => [1, 0],
            };
            for arm in order {
                let started = Instant::now();
                match arm {
                    0 => std::hint::black_box(unsafe { pfilter(slice) }),
                    1 => std::hint::black_box(rust_filter_seq(&data)),
                    _ => unreachable!(),
                };
                elapsed[arm] = started.elapsed().as_secs_f64() * 1e3;
            }
            parallel_samples.push(elapsed[0]);
            rust_seq_samples.push(elapsed[1]);
        }
        let parallel_ms = median(parallel_samples);
        let rust_seq_ms = median(rust_seq_samples);
        println!(
            "{n:>9}  {parallel_ms:>12.3}  {rust_seq_ms:>12.3}  {:>9.2}x",
            rust_seq_ms / parallel_ms,
        );
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("threshold") {
        #[cfg(feature = "probe")]
        {
            run_threshold();
            return;
        }
        #[cfg(not(feature = "probe"))]
        {
            eprintln!("threshold mode requires the probe feature");
            std::process::exit(2);
        }
    }
    if std::env::args().nth(1).as_deref() == Some("filter") {
        run_filter();
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("width") {
        #[cfg(feature = "probe")]
        {
            run_width();
            return;
        }
        #[cfg(not(feature = "probe"))]
        {
            eprintln!("width mode requires the probe feature");
            std::process::exit(2);
        }
    }
    if std::env::args().nth(1).as_deref() == Some("aggregate") {
        #[cfg(feature = "probe")]
        {
            run_aggregate();
            return;
        }
        #[cfg(not(feature = "probe"))]
        {
            eprintln!("aggregate mode requires the probe feature");
            std::process::exit(2);
        }
    }
    if std::env::args().nth(1).as_deref() == Some("chunks") {
        #[cfg(feature = "probe")]
        {
            run_chunks();
            return;
        }
        #[cfg(not(feature = "probe"))]
        {
            eprintln!("chunks mode requires the probe feature");
            std::process::exit(2);
        }
    }
    run_standard();
}
