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
    let rows: [(&str, Kernel, i64, Option<i64>); 9] = [
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
