use std::hint::black_box;
use std::time::{Duration, Instant};

type Kernel = unsafe extern "C" fn(i64) -> i64;

extern "C" {
    fn scalar_rows(reps: i64) -> i64;
    fn tagged_none(reps: i64) -> i64;
    fn tagged_some(reps: i64) -> i64;
    fn tagged_sparse(reps: i64) -> i64;
    fn tagged_replace(reps: i64) -> i64;
    fn tagged_conditional_replace(reps: i64) -> i64;
    fn tagged_match_loop_replace(reps: i64) -> i64;
    fn tagged_early_try(reps: i64) -> i64;
    fn tagged_move_error_try(reps: i64) -> i64;
    fn tagged_recursive_ok(reps: i64) -> i64;
    fn tagged_recursive_native_error(reps: i64) -> i64;
    fn tagged_recursive_decode_error(reps: i64) -> i64;
    fn tagged_recursive_else(reps: i64) -> i64;
    fn tagged_recursive_map_err(reps: i64) -> i64;
    fn tagged_nested_none(reps: i64) -> i64;
    fn tagged_nested_some(reps: i64) -> i64;
    fn tagged_nested_decode(reps: i64) -> i64;
    fn tagged_nested_native(reps: i64) -> i64;
    fn tagged_multi_both(reps: i64) -> i64;
    fn tagged_multi_wildcard(reps: i64) -> i64;
    fn tagged_multi_early_try(reps: i64) -> i64;
    fn align_rt_alloc_count() -> i64;
    fn align_rt_free_count() -> i64;
}

fn measure(kernel: Kernel, reps: i64) -> (i64, i64, Duration, i64) {
    let (a0, f0) = unsafe { (align_rt_alloc_count(), align_rt_free_count()) };
    let start = Instant::now();
    let checksum = black_box(unsafe { kernel(black_box(reps)) });
    let elapsed = start.elapsed();
    let (a1, f1) = unsafe { (align_rt_alloc_count(), align_rt_free_count()) };
    (a1 - a0, f1 - f0, elapsed, checksum)
}

fn main() {
    let reps = 1_000_000;
    let rows: [(&str, Kernel, i64, Option<i64>); 21] = [
        ("scalar", scalar_rows, 0, None),
        ("none", tagged_none, 0, None),
        ("some", tagged_some, reps, None),
        ("sparse-1pct", tagged_sparse, (reps + 99) / 100, None),
        ("replace", tagged_replace, reps * 3, None),
        (
            "conditional",
            tagged_conditional_replace,
            reps + reps / 2,
            None,
        ),
        ("match-loop", tagged_match_loop_replace, reps * 3, None),
        ("early-try", tagged_early_try, reps, Some(0)),
        (
            "move-err-try",
            tagged_move_error_try,
            reps,
            Some(reps * 4),
        ),
        ("recursive-ok", tagged_recursive_ok, 0, Some(reps)),
        (
            "recursive-native",
            tagged_recursive_native_error,
            reps * 2,
            Some(reps * 3),
        ),
        (
            "recursive-decode",
            tagged_recursive_decode_error,
            reps,
            Some(reps * 6),
        ),
        (
            "recursive-else",
            tagged_recursive_else,
            reps * 2,
            Some(reps * 7),
        ),
        (
            "recursive-map-err",
            tagged_recursive_map_err,
            reps,
            Some(reps * 4),
        ),
        ("nested-none", tagged_nested_none, 0, Some(reps * 2)),
        (
            "nested-some",
            tagged_nested_some,
            reps * 2,
            Some(reps * 7),
        ),
        (
            "nested-decode",
            tagged_nested_decode,
            reps,
            Some(reps * 6),
        ),
        (
            "nested-native",
            tagged_nested_native,
            reps * 2,
            Some(reps * 7),
        ),
        (
            "multi-both",
            tagged_multi_both,
            reps * 2,
            Some(reps * 9),
        ),
        (
            "multi-wildcard",
            tagged_multi_wildcard,
            reps * 2,
            Some(reps * 7),
        ),
        (
            "multi-early-try",
            tagged_multi_early_try,
            reps * 2,
            Some(reps * 5),
        ),
    ];
    let mut failed = false;
    let mut checksum = None;
    for (name, kernel, expected_allocs, expected_checksum) in rows {
        let (allocs, frees, elapsed, got) = measure(kernel, reps);
        checksum.get_or_insert(got);
        let expected_checksum = expected_checksum.or(checksum);
        let ok =
            allocs == expected_allocs && frees == expected_allocs && expected_checksum == Some(got);
        println!(
            "{name:>11}: {:>9.3} ms  alloc={allocs:>8} free={frees:>8} checksum={got} {}",
            elapsed.as_secs_f64() * 1_000.0,
            if ok { "OK" } else { "FAIL" }
        );
        failed |= !ok;
    }
    if failed {
        eprintln!("OWNED TAGGED PAYLOAD GATE: FAIL");
        std::process::exit(1);
    }
    println!("OWNED TAGGED PAYLOAD GATE: PASS");
}
