use std::time::Instant;

#[repr(C)]
struct AlignStr {
    ptr: *const u8,
    len: i64,
}

unsafe extern "C" {
    fn align_rt_frame_inner_join_i64_v1(
        left_data: *const u8,
        left_rows: i64,
        right_data: *const u8,
        right_rows: i64,
        max_pairs: i64,
        out: *mut AlignStr,
    ) -> i32;
    fn align_rt_frame_inner_join_str_v1(
        left_offsets: *const u8,
        left_data: *const u8,
        left_rows: i64,
        right_offsets: *const u8,
        right_data: *const u8,
        right_rows: i64,
        max_pairs: i64,
        out: *mut AlignStr,
    ) -> i32;
    fn align_rt_hash64(ptr: *const u8, len: i64) -> u64;
    fn align_rt_free(ptr: *mut u8);
}

struct StrColumn {
    offsets: Vec<u8>,
    data: Vec<u8>,
    rows: usize,
}

fn i64_bytes(values: &[i64]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn str_column(values: &[Vec<u8>]) -> StrColumn {
    let mut offsets = Vec::with_capacity((values.len() + 1) * 4);
    let mut data = Vec::new();
    offsets.extend_from_slice(&0_i32.to_le_bytes());
    for value in values {
        data.extend_from_slice(value);
        offsets.extend_from_slice(
            &i32::try_from(data.len())
                .expect("benchmark string data fits i32")
                .to_le_bytes(),
        );
    }
    StrColumn {
        offsets,
        data,
        rows: values.len(),
    }
}

fn capacity(rows: usize) -> usize {
    let requested = (rows + rows.div_ceil(3)).max(8);
    requested.next_power_of_two()
}

fn scratch_bytes(rows: usize) -> usize {
    16 * capacity(rows) + 8 * rows
}

fn take_count(out: &mut AlignStr) -> usize {
    let count = usize::try_from(out.len).expect("nonnegative result count");
    unsafe { align_rt_free(out.ptr.cast_mut()) };
    out.ptr = std::ptr::null();
    out.len = 0;
    count
}

fn measure(mut call: impl FnMut() -> usize, expected: usize) -> f64 {
    assert_eq!(call(), expected);
    let mut best = f64::MAX;
    for _ in 0..9 {
        let started = Instant::now();
        std::hint::black_box(call());
        best = best.min(started.elapsed().as_secs_f64() * 1e3);
    }
    best
}

fn measure_i64(left: &[i64], right: &[i64], expected: usize) -> f64 {
    let left = i64_bytes(left);
    let right = i64_bytes(right);
    measure(
        || {
            let mut out = AlignStr {
                ptr: std::ptr::null(),
                len: 0,
            };
            let status = unsafe {
                align_rt_frame_inner_join_i64_v1(
                    left.as_ptr(),
                    i64::try_from(left.len() / 8).unwrap(),
                    right.as_ptr(),
                    i64::try_from(right.len() / 8).unwrap(),
                    i64::try_from(expected).unwrap(),
                    &mut out,
                )
            };
            assert_eq!(status, 0);
            take_count(&mut out)
        },
        expected,
    )
}

fn measure_str(left: &StrColumn, right: &StrColumn, expected: usize) -> f64 {
    measure(
        || {
            let mut out = AlignStr {
                ptr: std::ptr::null(),
                len: 0,
            };
            let status = unsafe {
                align_rt_frame_inner_join_str_v1(
                    left.offsets.as_ptr(),
                    left.data.as_ptr(),
                    i64::try_from(left.rows).unwrap(),
                    right.offsets.as_ptr(),
                    right.data.as_ptr(),
                    i64::try_from(right.rows).unwrap(),
                    i64::try_from(expected).unwrap(),
                    &mut out,
                )
            };
            assert_eq!(status, 0);
            take_count(&mut out)
        },
        expected,
    )
}

fn colliding_strings(rows: usize) -> Vec<Vec<u8>> {
    let mask = capacity(rows) - 1;
    let mut result = Vec::with_capacity(rows);
    let mut candidate = 0_u64;
    let mut bucket = None;
    while result.len() < rows {
        let value = format!("collision-prefix-{candidate:016x}").into_bytes();
        let hash = unsafe { align_rt_hash64(value.as_ptr(), value.len() as i64) };
        let current = (hash as usize) & mask;
        if bucket.is_none_or(|selected| selected == current) {
            bucket = Some(current);
            result.push(value);
        }
        candidate = candidate.wrapping_add(1);
    }
    result
}

fn row(label: &str, build: usize, probe: usize, output: usize, millis: f64) {
    println!(
        "{label:>22} {build:>10} {probe:>10} {output:>10} {:>14} {:>14} {millis:>10.3}",
        scratch_bytes(build),
        output * 16,
    );
}

fn main() {
    println!(
        "{:>22} {:>10} {:>10} {:>10} {:>14} {:>14} {:>10}",
        "corpus", "build", "probe", "output", "scratch bytes", "output bytes", "best ms",
    );

    let one_to_one: Vec<i64> = (0..200_000).collect();
    let millis = measure_i64(&one_to_one, &one_to_one, one_to_one.len());
    row(
        "i64 one-to-one",
        one_to_one.len(),
        one_to_one.len(),
        one_to_one.len(),
        millis,
    );

    let duplicate_left: Vec<i64> = (0..100_000).map(|value| value % 10_000).collect();
    let duplicate_right: Vec<i64> = (0..20_000).map(|value| value % 10_000).collect();
    let duplicate_output = 200_000;
    let millis = measure_i64(&duplicate_left, &duplicate_right, duplicate_output);
    row(
        "i64 duplicate fanout",
        duplicate_right.len(),
        duplicate_left.len(),
        duplicate_output,
        millis,
    );

    let equal_values: Vec<Vec<u8>> = (0..80_000)
        .map(|value| format!("equal-byte-{value:08}").into_bytes())
        .collect();
    let equal_left = str_column(&equal_values);
    let equal_right = str_column(&equal_values);
    let millis = measure_str(&equal_left, &equal_right, equal_values.len());
    row(
        "str equal-byte",
        equal_right.rows,
        equal_left.rows,
        equal_values.len(),
        millis,
    );

    let collision_values = colliding_strings(512);
    let collision_left = str_column(&collision_values[..128]);
    let collision_right = str_column(&collision_values);
    let millis = measure_str(&collision_left, &collision_right, collision_left.rows);
    row(
        "str bucket-collision",
        collision_right.rows,
        collision_left.rows,
        collision_left.rows,
        millis,
    );
}
