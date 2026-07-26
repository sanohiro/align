//! Measure-first probe for the task-group record layout.
//!
//! The shipped codegen allocates a capture environment, a result slot, and (for fallible tasks) an
//! error slot separately from the task-group arena. The proposed packed form would allocate one
//! record and address those fields by offsets. This harness drives the same runtime registration
//! ABI with both layouts, so it measures the allocation-call and task-run costs without changing
//! production codegen or the runtime ABI.

use align_runtime::{
    align_rt_test_par_pool_initialized, align_rt_test_par_pool_wait_idle, align_rt_tg_alloc,
    align_rt_tg_begin, align_rt_tg_end, align_rt_tg_register, align_rt_tg_wait, TaskGroup,
};
use std::hint::black_box;
use std::mem::{align_of, size_of, transmute};
use std::time::{Duration, Instant};

const CACHE_LINE: usize = 64;
const RESULT_FIELD_MASK: u64 = 0xffff;

#[repr(C)]
#[derive(Clone, Copy)]
struct Env {
    seed: u64,
    rounds: u64,
    sleep_us: u64,
}

/// The builtin Align `Error` lowers to `{ i32 tag, i32 code }` in the generated task trampoline.
/// Keep this probe layout tied to that shipped ABI instead of using the host Rust enum layout.
#[repr(C)]
struct AlignError {
    tag: i32,
    code: i32,
}

type TaskThunk = extern "C" fn(*const u8) -> i64;
type TaskTrampoline = extern "C" fn(*const u8, *mut u8, *mut u8, *mut u8) -> i32;

#[derive(Clone, Copy)]
enum Layout {
    Split,
    PackedTight,
    PackedPadded,
}

impl Layout {
    const ALL: [Self; 3] = [Self::Split, Self::PackedTight, Self::PackedPadded];
}

fn expected_result(seed: u64, rounds: u64, sleep_us: u64) -> u64 {
    (seed << 48) | ((rounds & RESULT_FIELD_MASK) << 32) | (sleep_us & RESULT_FIELD_MASK)
}

/// The body is deliberately the same for every task and every record layout. `rounds == 0` prices
/// the scheduler/record path; the nonzero case keeps a body-cost control in the probe. `sleep_us`
/// adds a bounded blocking control without introducing filesystem or network setup into the probe.
extern "C" fn task_thunk(env: *const u8) -> i64 {
    let env = unsafe { &*env.cast::<Env>() };
    if env.sleep_us != 0 {
        std::thread::sleep(Duration::from_micros(env.sleep_us));
    }
    let mut value = env.seed;
    for _ in 0..env.rounds {
        value = value
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        value ^= value >> 29;
    }
    // Keep the body work live, but return a token whose fields let the caller validate every task
    // independently without repeating this CPU-heavy loop on the timing path.
    let _ = black_box(value);
    expected_result(env.seed, env.rounds, env.sleep_us) as i64
}

/// Match the generated task trampoline's ABI: invoke the typed body and write one final result.
extern "C" fn task_trampoline(
    thunk_ptr: *const u8,
    env: *mut u8,
    slot: *mut u8,
    _err_slot: *mut u8,
) -> i32 {
    let thunk: TaskThunk = unsafe { transmute(thunk_ptr) };
    let result = thunk(env.cast_const());
    unsafe { slot.cast::<i64>().write(result) };
    0
}

/// Exercise the real fallible error-slot write and `tg_wait` error-pointer return once per layout.
/// Performance rows use successful tasks so their layout cost is isolated from error selection.
extern "C" fn error_trampoline(
    _thunk_ptr: *const u8,
    env: *mut u8,
    _slot: *mut u8,
    err_slot: *mut u8,
) -> i32 {
    let code = unsafe { (*env.cast::<Env>()).seed as i32 + 7 };
    unsafe {
        err_slot
            .cast::<AlignError>()
            .write(AlignError { tag: 4, code });
    }
    1
}

fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    value
        .checked_add(align - 1)
        .expect("benchmark record layout overflow")
        & !(align - 1)
}

unsafe fn tg_alloc(tg: *mut TaskGroup, size: usize, align: usize) -> *mut u8 {
    let ptr = unsafe { align_rt_tg_alloc(tg, size as i64, align as i64) };
    assert!(!ptr.is_null(), "task-group arena allocation failed");
    ptr
}

fn register_task(
    tg: *mut TaskGroup,
    task_index: usize,
    rounds: u64,
    sleep_us: u64,
    layout: Layout,
    fallible: bool,
    error: bool,
) -> *mut i64 {
    assert!(
        task_index < RESULT_FIELD_MASK as usize,
        "benchmark task index and result seed are not encodable"
    );
    assert!(
        rounds <= RESULT_FIELD_MASK,
        "benchmark rounds are not encodable"
    );
    assert!(
        sleep_us <= RESULT_FIELD_MASK,
        "benchmark sleep duration is not encodable"
    );
    let env_size = size_of::<Env>();
    let scalar_size = size_of::<i64>();
    let scalar_align = align_of::<i64>();
    let error_size = size_of::<AlignError>();
    let error_align = align_of::<AlignError>();

    let (env, slot, err_slot) = match layout {
        Layout::Split => {
            let env = unsafe { tg_alloc(tg, env_size, align_of::<Env>()) };
            let slot = unsafe { tg_alloc(tg, scalar_size, scalar_align) };
            let err_slot = if fallible {
                unsafe { tg_alloc(tg, error_size, error_align) }
            } else {
                core::ptr::null_mut()
            };
            (env, slot, err_slot)
        }
        Layout::PackedTight | Layout::PackedPadded => {
            let padded = matches!(layout, Layout::PackedPadded);
            let record_align = if padded {
                CACHE_LINE
            } else {
                align_of::<Env>()
            };
            let slot_offset = if padded {
                CACHE_LINE
            } else {
                align_up(env_size, scalar_align)
            };
            let err_offset = if fallible {
                if padded {
                    align_up(slot_offset + scalar_size, CACHE_LINE)
                } else {
                    align_up(slot_offset + scalar_size, error_align)
                }
            } else {
                0
            };
            let end = if fallible {
                err_offset + error_size
            } else {
                slot_offset + scalar_size
            };
            let record_size = align_up(end, record_align);
            let record = unsafe { tg_alloc(tg, record_size, record_align) };
            let slot = unsafe { record.add(slot_offset) };
            let err_slot = if fallible {
                unsafe { record.add(err_offset) }
            } else {
                core::ptr::null_mut()
            };
            (record, slot, err_slot)
        }
    };

    unsafe {
        env.cast::<Env>().write(Env {
            seed: task_index as u64 + 1,
            rounds,
            sleep_us,
        });
        align_rt_tg_register(
            tg,
            if error {
                error_trampoline as TaskTrampoline
            } else {
                task_trampoline as TaskTrampoline
            },
            task_thunk as *const u8,
            env,
            slot,
            err_slot,
        );
    }
    slot.cast::<i64>()
}

struct TgGuard(*mut TaskGroup);

impl TgGuard {
    fn new() -> Self {
        let tg = align_rt_tg_begin();
        assert!(!tg.is_null(), "task-group begin failed");
        Self(tg)
    }
}

impl Drop for TgGuard {
    fn drop(&mut self) {
        unsafe { align_rt_tg_end(self.0) };
    }
}

struct CompletedRun {
    guard: TgGuard,
    slots: Vec<*mut i64>,
}

impl CompletedRun {
    fn checksum(self) -> u64 {
        let Self { guard, slots } = self;
        let checksum = slots.into_iter().fold(0u64, |sum, slot| {
            sum.wrapping_add(unsafe { slot.read() as u64 })
        });
        drop(guard);
        checksum
    }

    fn validated_checksum(self, rounds: u64, sleep_us: u64) -> u64 {
        let Self { guard, slots } = self;
        let checksum = slots
            .into_iter()
            .enumerate()
            .fold(0u64, |sum, (task_index, slot)| {
                let value = unsafe { slot.read() as u64 };
                assert_eq!(
                    value,
                    expected_result((task_index + 1) as u64, rounds, sleep_us),
                    "record layout changed task {task_index} result"
                );
                sum.wrapping_add(value)
            });
        drop(guard);
        checksum
    }
}

fn begin_run(
    tasks: usize,
    rounds: u64,
    sleep_us: u64,
    layout: Layout,
    fallible: bool,
) -> CompletedRun {
    let guard = TgGuard::new();
    let tg = guard.0;
    let mut slots = Vec::with_capacity(tasks);
    for task_index in 0..tasks {
        slots.push(register_task(
            tg, task_index, rounds, sleep_us, layout, fallible, false,
        ));
    }

    let err = unsafe { align_rt_tg_wait(tg) };
    assert!(err.is_null(), "the probe task body must not fail");
    CompletedRun { guard, slots }
}

fn run_once(tasks: usize, rounds: u64, sleep_us: u64, layout: Layout, fallible: bool) -> u64 {
    begin_run(tasks, rounds, sleep_us, layout, fallible).checksum()
}

fn validate_once(tasks: usize, rounds: u64, sleep_us: u64, layout: Layout, fallible: bool) -> u64 {
    begin_run(tasks, rounds, sleep_us, layout, fallible).validated_checksum(rounds, sleep_us)
}

fn run_error_path(layout: Layout) {
    let guard = TgGuard::new();
    let tg = guard.0;
    for task_index in 0..8 {
        register_task(tg, task_index, 0, 0, layout, true, task_index == 3);
    }
    let err = unsafe { align_rt_tg_wait(tg) };
    assert!(!err.is_null(), "the error path must return an err slot");
    let error = unsafe { err.cast::<AlignError>().read() };
    assert_eq!(
        (error.tag, error.code),
        (4, 11),
        "fallible error slot ABI mismatch"
    );
}

fn timed(
    tasks: usize,
    rounds: u64,
    sleep_us: u64,
    layout: Layout,
    fallible: bool,
    reps: usize,
    expected: u64,
) -> f64 {
    // Keep the aggregate checksum in the measured path so result writes remain observable, but
    // perform the per-task oracle checks in the unmeasured warm-up pass.
    let mut checksum = 0u64;
    // `tg_wait` joins task bodies before returning, but the persistent runtime pool may still have
    // detached no-op helpers queued behind an exhausted cursor. When the pool is active, time each
    // repetition separately and drain those helpers between repetitions outside the clock. The
    // one-task caller-only path has no pool and keeps the lower-overhead aggregate timer.
    let elapsed_seconds = if align_rt_test_par_pool_initialized() {
        let mut elapsed = Duration::ZERO;
        for _ in 0..reps {
            assert!(
                align_rt_test_par_pool_wait_idle(),
                "a measurement pool job panicked"
            );
            let started = Instant::now();
            checksum = checksum.wrapping_add(black_box(run_once(
                tasks, rounds, sleep_us, layout, fallible,
            )));
            elapsed += started.elapsed();
        }
        elapsed.as_secs_f64()
    } else {
        let started = Instant::now();
        for _ in 0..reps {
            checksum = checksum.wrapping_add(black_box(run_once(
                tasks, rounds, sleep_us, layout, fallible,
            )));
        }
        started.elapsed().as_secs_f64()
    };
    let elapsed = elapsed_seconds * 1e9 / (tasks * reps) as f64;
    assert_eq!(
        checksum,
        expected.wrapping_mul(reps as u64),
        "record layout changed task results"
    );
    elapsed
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn run_case(tasks: usize, rounds: u64, sleep_us: u64, fallible: bool, trials: usize, reps: usize) {
    assert!(
        tasks > 0,
        "task-group record probe requires at least one task"
    );
    let mut samples = [
        Vec::with_capacity(trials),
        Vec::with_capacity(trials),
        Vec::with_capacity(trials),
    ];
    let mut ratios = Vec::with_capacity(trials);
    let mut padded_ratios = Vec::with_capacity(trials);
    let warm_checksum = validate_once(tasks, rounds, sleep_us, Layout::Split, fallible);
    for layout in Layout::ALL {
        let checksum = validate_once(tasks, rounds, sleep_us, layout, fallible);
        assert_eq!(
            checksum, warm_checksum,
            "record layout changed task results"
        );
    }
    const ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
        [1, 0, 2],
        [0, 2, 1],
    ];
    assert_eq!(
        trials % ORDERS.len(),
        0,
        "TRIALS must be a multiple of the six balanced layout orders"
    );
    for trial in 0..trials {
        let order = ORDERS[trial % ORDERS.len()];
        let mut elapsed = [0.0; 3];
        for arm in order {
            let ns = timed(
                tasks,
                rounds,
                sleep_us,
                Layout::ALL[arm],
                fallible,
                reps,
                warm_checksum,
            );
            elapsed[arm] = ns;
            samples[arm].push(ns);
        }
        ratios.push(elapsed[1] / elapsed[0]);
        padded_ratios.push(elapsed[2] / elapsed[1]);
    }
    let medians = [
        median(&mut samples[0]),
        median(&mut samples[1]),
        median(&mut samples[2]),
    ];
    let packed_over_split = median(&mut ratios);
    let padded_over_packed = median(&mut padded_ratios);
    let allocs = if fallible { "3/1/1" } else { "2/1/1" };
    println!(
        "{tasks:>6}  {rounds:>6}  {sleep_us:>8}  {:>8}  {:>10.1}  {:>12.1}  {:>12.1}  {packed_over_split:>8.3}  {padded_over_packed:>8.3}  {allocs}",
        if fallible { "yes" } else { "no" },
        medians[0],
        medians[1],
        medians[2],
    );
}

fn main() {
    let trials = std::env::var("TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6)
        .max(1);
    let reps = std::env::var("REPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3)
        .max(1);
    let workers = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    println!("task_group record probe — current split allocation vs one-record layouts");
    println!("host parallelism / task-group runtime workers: {workers}");
    println!(
        "six-order balanced cycle, median of {trials} trials, {reps} repetitions per timing arm"
    );
    println!(
        "packed-tight keeps fields adjacent; packed-padded measures cache-line-separated padding"
    );
    println!("tasks  rounds  sleep-us  error-slot       split  packed-tight  packed-padded  tight/split  padded/tight  allocs/task");
    let mut cases = vec![
        (1, 0, 0),
        (8, 0, 0),
        (128, 0, 0),
        (4096, 0, 0),
        (128, 64, 0),
        (4096, 64, 0),
        (128, 0, 50),
    ];
    let max_tasks = RESULT_FIELD_MASK as usize;
    for tasks in [
        2,
        workers.saturating_sub(1),
        workers,
        workers.saturating_add(1),
        workers.saturating_mul(4).saturating_add(1),
    ] {
        if tasks > 0
            && tasks <= max_tasks
            && !cases.iter().any(|&(existing, _, _)| existing == tasks)
        {
            cases.push((tasks, 0, 0));
        }
    }
    for &(tasks, rounds, sleep_us) in &cases {
        for fallible in [false, true] {
            run_case(tasks, rounds, sleep_us, fallible, trials, reps);
        }
    }
    for layout in Layout::ALL {
        run_error_path(layout);
    }
    if align_rt_test_par_pool_initialized() {
        assert!(
            align_rt_test_par_pool_wait_idle(),
            "a measurement pool job panicked"
        );
    }
}
