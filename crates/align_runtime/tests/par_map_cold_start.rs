//! Tiny `par_map` stays on the caller without initializing the worker pool. Structured tasks
//! publish work at registration, including singleton groups. This isolated process owns the
//! pool initialization observations; keep the tiny parallel-map checks first.

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
fn tiny_par_map_skips_pool_and_registered_tasks_use_workers() {
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

    // Sanity check on the observation mechanism itself: on a multi-worker host, a workload that
    // DOES cross the threshold must initialize the pool as before. A one-worker host intentionally
    // remains caller-only, so the final assertion follows the runtime's host policy. Run before the first structured task.
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
    // A singleton structured task is worker-eligible at registration, also on a one-core host.
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
            None,
        )
    };
    let err = unsafe { align_runtime::align_rt_tg_wait(tg) };
    assert!(err.is_null());
    assert_eq!(unsafe { *(slot as *const i64) }, 42);
    unsafe { align_runtime::align_rt_tg_end(tg) };
    assert!(
        align_runtime::align_rt_test_par_pool_initialized(),
        "a registered task must initialize the process worker pool"
    );

}
