//! Pins that a tiny `par_map` and a single-task `task_group` never spin up the global worker
//! pool (Codex audit item 5, `docs/open-questions.md` → "External binary-optimization audit
//! (Codex, 2026-07-12) — adoption record"). Before the fix, `par_pool()` was called *before* the
//! single-chunk threshold check, so even an 8-element `par_map` (or a `task_group` with exactly
//! one task) paid the pool's cold-start cost (measured ~69µs cold vs ~125ns warm) though it never
//! actually used the pool.
//!
//! This lives in its own integration-test file (its own process) on purpose: the pool is a
//! process-lifetime `OnceLock` (`align_runtime::PAR_POOL`, private to the crate), so any other
//! test that drives a large `par_map`/`task_group` anywhere in the *same process* would poison
//! the "never initialized" observation. Do not add another test to this file that touches a
//! large workload before the deliberate "sanity: a big workload still initializes the pool" check
//! at the end.

extern "C" fn double(_context: *const u8, input: *const u8, output: *mut u8, start: i64, end: i64) {
    let start = usize::try_from(start).unwrap();
    let end = usize::try_from(end).unwrap();
    for i in start..end {
        unsafe {
            *output.cast::<i64>().add(i) = *input.cast::<i64>().add(i) * 2;
        }
    }
}

extern "C" fn double_tramp(_thunk: *const u8, env: *mut u8, slot: *mut u8, _err: *mut u8) -> i32 {
    unsafe { *(slot as *mut i64) = *(env as *const i64) * 2 };
    0
}

#[test]
fn tiny_par_map_and_single_task_group_skip_pool_init() {
    assert!(
        !align_runtime::align_rt_test_par_pool_initialized(),
        "the pool must not be touched before this fresh process does any parallel work"
    );

    // A tiny `par_map` (far below `PAR_MIN_CHUNK` = 65_536) must run entirely on the caller.
    const COUNT: i64 = 8;
    let input: Vec<i64> = (0..COUNT).collect();
    let output = unsafe {
        align_runtime::align_rt_par_map(
            std::ptr::null(),
            input.as_ptr() as *const u8,
            COUNT,
            std::mem::size_of::<i64>() as i64,
            std::mem::size_of::<i64>() as i64,
            1,
            double,
        )
    };
    let values = unsafe { std::slice::from_raw_parts(output as *const i64, COUNT as usize) };
    for (i, &v) in values.iter().enumerate() {
        assert_eq!(v, (i as i64) * 2, "index {i}");
    }
    unsafe { align_runtime::align_rt_free(output) };
    assert!(
        !align_runtime::align_rt_test_par_pool_initialized(),
        "a tiny par_map must not spin up the global worker pool (Codex audit item 5)"
    );

    // The exact boundary must take the same caller-only path, not merely a much smaller input.
    const BOUNDARY: i64 = 65_536;
    let boundary_input: Vec<i64> = (0..BOUNDARY).collect();
    let boundary_output = unsafe {
        align_runtime::align_rt_par_map(
            std::ptr::null(),
            boundary_input.as_ptr() as *const u8,
            BOUNDARY,
            std::mem::size_of::<i64>() as i64,
            std::mem::size_of::<i64>() as i64,
            1,
            double,
        )
    };
    let boundary_values =
        unsafe { std::slice::from_raw_parts(boundary_output as *const i64, BOUNDARY as usize) };
    assert_eq!(boundary_values.first(), Some(&0));
    assert_eq!(boundary_values.last(), Some(&(2 * (BOUNDARY - 1))));
    unsafe { align_runtime::align_rt_free(boundary_output) };
    assert!(
        !align_runtime::align_rt_test_par_pool_initialized(),
        "a par_map at PAR_MIN_CHUNK must not spin up the global worker pool"
    );

    // A single-task `task_group` (`n == 1`) must likewise never touch the pool: `workers.min(n-1)`
    // is always 0 for `n == 1`, so no helper would ever be submitted even if the pool existed.
    let tg = align_runtime::align_rt_tg_begin();
    let env = unsafe { align_runtime::align_rt_tg_alloc(tg, 8, 8) };
    unsafe { *(env as *mut i64) = 21 };
    let slot = unsafe { align_runtime::align_rt_tg_alloc(tg, 8, 8) };
    unsafe {
        align_runtime::align_rt_tg_register(
            tg,
            double_tramp,
            std::ptr::null(),
            env,
            slot,
            std::ptr::null_mut(),
        )
    };
    let err = unsafe { align_runtime::align_rt_tg_wait(tg) };
    assert!(err.is_null());
    assert_eq!(unsafe { *(slot as *const i64) }, 42);
    unsafe { align_runtime::align_rt_tg_end(tg) };
    assert!(
        !align_runtime::align_rt_test_par_pool_initialized(),
        "a single-task task_group must not spin up the global worker pool (Codex audit item 5)"
    );

    // Sanity check on the observation mechanism itself: on a multi-worker host, a workload that
    // DOES cross the threshold must initialize the pool as before. A one-worker host intentionally
    // remains caller-only, so the final assertion follows the runtime's host policy. Must run last
    // in this process -- it deliberately poisons the "never touched" state on multi-worker hosts.
    const BIG: i64 = 65_537;
    let big_input: Vec<i64> = (0..BIG).collect();
    let big_output = unsafe {
        align_runtime::align_rt_par_map(
            std::ptr::null(),
            big_input.as_ptr() as *const u8,
            BIG,
            std::mem::size_of::<i64>() as i64,
            std::mem::size_of::<i64>() as i64,
            1,
            double,
        )
    };
    assert!(!big_output.is_null());
    unsafe { align_runtime::align_rt_free(big_output) };
    let workers = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    if workers > 1 {
        assert!(
            align_runtime::align_rt_test_par_pool_initialized(),
            "a workload above PAR_MIN_CHUNK must initialize the pool on a multi-worker host"
        );
    } else {
        assert!(
            !align_runtime::align_rt_test_par_pool_initialized(),
            "a one-worker host must keep par_map caller-only without initializing the pool"
        );
    }
}
