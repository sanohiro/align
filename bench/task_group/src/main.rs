//! Measure-first probe for the task-group record layout.
//!
//! The shipped codegen allocates a capture environment, a result slot, and (for fallible tasks) an
//! error slot separately from the task-group arena. The proposed packed form would allocate one
//! record and address those fields by offsets. This harness drives the same runtime registration
//! ABI with both layouts, so it measures the allocation-call and task-run costs without changing
//! production codegen or the runtime ABI.

use align_runtime::{
    align_rt_tg_alloc, align_rt_tg_begin, align_rt_tg_end, align_rt_tg_register, align_rt_tg_wait,
    TaskGroup,
};
use std::hint::black_box;
use std::mem::{align_of, size_of, transmute};
use std::time::Instant;

const CACHE_LINE: usize = 64;

#[repr(C)]
#[derive(Clone, Copy)]
struct Env {
    seed: u64,
    rounds: u64,
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

/// The body is deliberately the same for every task and every record layout. `rounds == 0` prices
/// the scheduler/record path; the nonzero case keeps a body-cost control in the probe.
extern "C" fn task_thunk(env: *const u8) -> i64 {
    let env = unsafe { &*env.cast::<Env>() };
    let mut value = env.seed;
    for _ in 0..env.rounds {
        value = value
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        value ^= value >> 29;
    }
    value as i64
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
    layout: Layout,
    fallible: bool,
) -> *mut i64 {
    let env_size = size_of::<Env>();
    let scalar_size = size_of::<i64>();
    let scalar_align = align_of::<i64>();
    let error_size = 16usize;

    let (env, slot, err_slot) = match layout {
        Layout::Split => {
            let env = unsafe { tg_alloc(tg, env_size, align_of::<Env>()) };
            let slot = unsafe { tg_alloc(tg, scalar_size, scalar_align) };
            let err_slot = if fallible {
                unsafe { tg_alloc(tg, error_size, 8) }
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
                    align_up(slot_offset + scalar_size, 8)
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
        });
        align_rt_tg_register(
            tg,
            task_trampoline as TaskTrampoline,
            task_thunk as *const u8,
            env,
            slot,
            err_slot,
        );
    }
    slot.cast::<i64>()
}

fn run_once(tasks: usize, rounds: u64, layout: Layout, fallible: bool) -> u64 {
    let tg = align_rt_tg_begin();
    assert!(!tg.is_null(), "task-group begin failed");
    let mut slots = Vec::with_capacity(tasks);
    for task_index in 0..tasks {
        slots.push(register_task(tg, task_index, rounds, layout, fallible));
    }

    let err = unsafe { align_rt_tg_wait(tg) };
    assert!(err.is_null(), "the probe task body must not fail");
    let checksum = slots.into_iter().fold(0u64, |sum, slot| {
        sum.wrapping_add(unsafe { slot.read() as u64 })
    });
    unsafe { align_rt_tg_end(tg) };
    checksum
}

fn timed(tasks: usize, rounds: u64, layout: Layout, fallible: bool, reps: usize) -> (f64, u64) {
    let started = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..reps {
        checksum ^= black_box(run_once(tasks, rounds, layout, fallible));
    }
    let ns_per_task = started.elapsed().as_secs_f64() * 1e9 / (tasks * reps) as f64;
    (ns_per_task, checksum)
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn run_case(tasks: usize, rounds: u64, fallible: bool, trials: usize, reps: usize) {
    let mut samples = [
        Vec::with_capacity(trials),
        Vec::with_capacity(trials),
        Vec::with_capacity(trials),
    ];
    let mut ratios = Vec::with_capacity(trials);
    let mut padded_ratios = Vec::with_capacity(trials);
    let warm_checksum = run_once(tasks, rounds, Layout::Split, fallible);
    let expected_checksum = if reps.is_multiple_of(2) {
        0
    } else {
        warm_checksum
    };
    for trial in 0..trials {
        let order = if trial % 2 == 0 { [0, 1, 2] } else { [2, 1, 0] };
        let mut elapsed = [0.0; 3];
        for arm in order {
            let (ns, checksum) = timed(tasks, rounds, Layout::ALL[arm], fallible, reps);
            assert_eq!(
                checksum, expected_checksum,
                "record layouts changed task results"
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
        "{tasks:>6}  {rounds:>6}  {:>8}  {:>10.1}  {:>12.1}  {:>12.1}  {packed_over_split:>8.3}  {padded_over_packed:>8.3}  {allocs}",
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
        .unwrap_or(9)
        .max(1);
    let reps = std::env::var("REPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3)
        .max(1);
    println!("task_group record probe — current split allocation vs one-record layouts");
    println!("balanced order, median of {trials} trials, {reps} repetitions per timing arm");
    println!("packed-tight keeps fields adjacent; packed-padded puts each result/error on a cache-line boundary");
    println!("tasks  rounds  fallible       split  packed-tight  packed-padded  tight/split  padded/tight  allocs/task");
    for &(tasks, rounds) in &[(8, 0), (128, 0), (4096, 0), (128, 64), (4096, 64)] {
        for fallible in [false, true] {
            run_case(tasks, rounds, fallible, trials, reps);
        }
    }
}
