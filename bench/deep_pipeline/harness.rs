//! Timer for the deep-pipeline Align/clang controls. `run.sh` compiles both kernel objects through
//! LLVM 22 at the same O2/CPU settings; Rust only supplies runtime data and balanced AB/BA timing.

use std::hint::black_box;
use std::time::Instant;

#[repr(C)]
#[derive(Clone, Copy)]
struct Slice {
    ptr: *const i64,
    len: i64,
}

type PlainKernel = unsafe extern "C" fn(Slice) -> i64;
type CaptureKernel = unsafe extern "C" fn(Slice, i64) -> i64;

extern "C" {
    fn named_1(s: Slice) -> i64;
    fn c_named_1(s: Slice) -> i64;
    fn named_2(s: Slice) -> i64;
    fn c_named_2(s: Slice) -> i64;
    fn named_4(s: Slice) -> i64;
    fn c_named_4(s: Slice) -> i64;
    fn named_8(s: Slice) -> i64;
    fn c_named_8(s: Slice) -> i64;
    fn named_16(s: Slice) -> i64;
    fn c_named_16(s: Slice) -> i64;
    fn named_32(s: Slice) -> i64;
    fn c_named_32(s: Slice) -> i64;
    fn masked_1(s: Slice) -> i64;
    fn c_masked_1(s: Slice) -> i64;
    fn masked_2(s: Slice) -> i64;
    fn c_masked_2(s: Slice) -> i64;
    fn masked_4(s: Slice) -> i64;
    fn c_masked_4(s: Slice) -> i64;
    fn masked_8(s: Slice) -> i64;
    fn c_masked_8(s: Slice) -> i64;
    fn masked_16(s: Slice) -> i64;
    fn c_masked_16(s: Slice) -> i64;
    fn masked_32(s: Slice) -> i64;
    fn c_masked_32(s: Slice) -> i64;
    fn guarded_1(s: Slice) -> i64;
    fn c_guarded_1(s: Slice) -> i64;
    fn guarded_2(s: Slice) -> i64;
    fn c_guarded_2(s: Slice) -> i64;
    fn guarded_4(s: Slice) -> i64;
    fn c_guarded_4(s: Slice) -> i64;
    fn guarded_8(s: Slice) -> i64;
    fn c_guarded_8(s: Slice) -> i64;
    fn guarded_16(s: Slice) -> i64;
    fn c_guarded_16(s: Slice) -> i64;
    fn guarded_32(s: Slice) -> i64;
    fn c_guarded_32(s: Slice) -> i64;
    fn capture_1(s: Slice, k: i64) -> i64;
    fn c_capture_1(s: Slice, k: i64) -> i64;
    fn capture_2(s: Slice, k: i64) -> i64;
    fn c_capture_2(s: Slice, k: i64) -> i64;
    fn capture_4(s: Slice, k: i64) -> i64;
    fn c_capture_4(s: Slice, k: i64) -> i64;
    fn capture_8(s: Slice, k: i64) -> i64;
    fn c_capture_8(s: Slice, k: i64) -> i64;
    fn capture_16(s: Slice, k: i64) -> i64;
    fn c_capture_16(s: Slice, k: i64) -> i64;
    fn capture_32(s: Slice, k: i64) -> i64;
    fn c_capture_32(s: Slice, k: i64) -> i64;
}

struct PlainRow {
    depth: usize,
    align: PlainKernel,
    control: PlainKernel,
}

struct CaptureRow {
    depth: usize,
    align: CaptureKernel,
    control: CaptureKernel,
}

fn gen(n: usize) -> Vec<i64> {
    let mut state = 0x9E3779B97F4A7C15_u64;
    (0..n)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            state as i64
        })
        .collect()
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.total_cmp(b));
    samples[samples.len() / 2]
}

fn time_plain(kernel: PlainKernel, slice: Slice, reps: usize) -> f64 {
    let start = Instant::now();
    let mut sink = 0_i64;
    for _ in 0..reps {
        sink ^= unsafe { black_box(kernel)(black_box(slice)) };
    }
    black_box(sink);
    start.elapsed().as_secs_f64() * 1e9 / reps as f64
}

fn duel_plain(row: &PlainRow, data: &[i64], reps: usize, rounds: usize) -> (f64, f64) {
    let slice = Slice {
        ptr: data.as_ptr(),
        len: data.len() as i64,
    };
    assert_eq!(
        unsafe { (row.align)(slice) },
        unsafe { (row.control)(slice) },
        "depth {} kernels disagree",
        row.depth
    );

    let mut align_samples = Vec::with_capacity(rounds);
    let mut control_samples = Vec::with_capacity(rounds);
    for round in 0..rounds {
        if round % 2 == 0 {
            align_samples.push(time_plain(row.align, slice, reps));
            control_samples.push(time_plain(row.control, slice, reps));
        } else {
            control_samples.push(time_plain(row.control, slice, reps));
            align_samples.push(time_plain(row.align, slice, reps));
        }
    }
    (median(align_samples), median(control_samples))
}

fn time_capture(kernel: CaptureKernel, slice: Slice, k: i64, reps: usize) -> f64 {
    let start = Instant::now();
    let mut sink = 0_i64;
    for _ in 0..reps {
        sink ^= unsafe { black_box(kernel)(black_box(slice), black_box(k)) };
    }
    black_box(sink);
    start.elapsed().as_secs_f64() * 1e9 / reps as f64
}

fn duel_capture(row: &CaptureRow, data: &[i64], k: i64, reps: usize, rounds: usize) -> (f64, f64) {
    let slice = Slice {
        ptr: data.as_ptr(),
        len: data.len() as i64,
    };
    assert_eq!(
        unsafe { (row.align)(slice, k) },
        unsafe { (row.control)(slice, k) },
        "capture depth {} kernels disagree",
        row.depth
    );

    let mut align_samples = Vec::with_capacity(rounds);
    let mut control_samples = Vec::with_capacity(rounds);
    for round in 0..rounds {
        if round % 2 == 0 {
            align_samples.push(time_capture(row.align, slice, k, reps));
            control_samples.push(time_capture(row.control, slice, k, reps));
        } else {
            control_samples.push(time_capture(row.control, slice, k, reps));
            align_samples.push(time_capture(row.align, slice, k, reps));
        }
    }
    (median(align_samples), median(control_samples))
}

fn report(family: &str, depth: usize, n: usize, align_ns: f64, control_ns: f64) -> f64 {
    let align_per_element = align_ns / n as f64;
    let control_per_element = control_ns / n as f64;
    let ratio = align_ns / control_ns;
    println!(
        "{family:<8} {depth:>5} {align_per_element:>12.4} {control_per_element:>12.4} \
         {ratio:>8.3} {align_stage:>12.4}",
        align_stage = align_per_element / depth as f64,
    );
    ratio
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&v| v > 0)
        .unwrap_or(default)
}

fn main() {
    let n = env_usize("DEEP_PIPELINE_N", 262_144);
    let rounds = env_usize("DEEP_PIPELINE_ROUNDS", 7) | 1;
    let target_stage_elements = env_usize("DEEP_PIPELINE_STAGE_ELEMENTS", 8 * 1024 * 1024);
    let data = gen(n);
    let mut worst_ratio = 0.0_f64;

    println!("deep pipeline scaling: n={n}, rounds={rounds}, target-stage-elements={target_stage_elements}");
    println!("family   depth  align ns/el clang ns/el    ratio align ns/stage");
    println!("-------- ----- ------------ ------------ -------- --------------");

    let plain_families: [(&str, Vec<PlainRow>); 3] = [
        (
            "named",
            vec![
                PlainRow {
                    depth: 1,
                    align: named_1,
                    control: c_named_1,
                },
                PlainRow {
                    depth: 2,
                    align: named_2,
                    control: c_named_2,
                },
                PlainRow {
                    depth: 4,
                    align: named_4,
                    control: c_named_4,
                },
                PlainRow {
                    depth: 8,
                    align: named_8,
                    control: c_named_8,
                },
                PlainRow {
                    depth: 16,
                    align: named_16,
                    control: c_named_16,
                },
                PlainRow {
                    depth: 32,
                    align: named_32,
                    control: c_named_32,
                },
            ],
        ),
        (
            "masked",
            vec![
                PlainRow {
                    depth: 1,
                    align: masked_1,
                    control: c_masked_1,
                },
                PlainRow {
                    depth: 2,
                    align: masked_2,
                    control: c_masked_2,
                },
                PlainRow {
                    depth: 4,
                    align: masked_4,
                    control: c_masked_4,
                },
                PlainRow {
                    depth: 8,
                    align: masked_8,
                    control: c_masked_8,
                },
                PlainRow {
                    depth: 16,
                    align: masked_16,
                    control: c_masked_16,
                },
                PlainRow {
                    depth: 32,
                    align: masked_32,
                    control: c_masked_32,
                },
            ],
        ),
        (
            "guarded",
            vec![
                PlainRow {
                    depth: 1,
                    align: guarded_1,
                    control: c_guarded_1,
                },
                PlainRow {
                    depth: 2,
                    align: guarded_2,
                    control: c_guarded_2,
                },
                PlainRow {
                    depth: 4,
                    align: guarded_4,
                    control: c_guarded_4,
                },
                PlainRow {
                    depth: 8,
                    align: guarded_8,
                    control: c_guarded_8,
                },
                PlainRow {
                    depth: 16,
                    align: guarded_16,
                    control: c_guarded_16,
                },
                PlainRow {
                    depth: 32,
                    align: guarded_32,
                    control: c_guarded_32,
                },
            ],
        ),
    ];
    for (family, rows) in &plain_families {
        for row in rows {
            let reps = (target_stage_elements / n.saturating_mul(row.depth)).max(1);
            let (align_ns, control_ns) = duel_plain(row, &data, reps, rounds);
            worst_ratio = worst_ratio.max(report(family, row.depth, n, align_ns, control_ns));
        }
    }

    let capture_rows = [
        CaptureRow {
            depth: 1,
            align: capture_1,
            control: c_capture_1,
        },
        CaptureRow {
            depth: 2,
            align: capture_2,
            control: c_capture_2,
        },
        CaptureRow {
            depth: 4,
            align: capture_4,
            control: c_capture_4,
        },
        CaptureRow {
            depth: 8,
            align: capture_8,
            control: c_capture_8,
        },
        CaptureRow {
            depth: 16,
            align: capture_16,
            control: c_capture_16,
        },
        CaptureRow {
            depth: 32,
            align: capture_32,
            control: c_capture_32,
        },
    ];
    let k = 0x5A5A5A5A5A5A5A5A_i64;
    for row in &capture_rows {
        let reps = (target_stage_elements / n.saturating_mul(row.depth)).max(1);
        let (align_ns, control_ns) = duel_capture(row, &data, k, reps, rounds);
        worst_ratio = worst_ratio.max(report("capture", row.depth, n, align_ns, control_ns));
    }

    println!("worst Align/clang ratio: {worst_ratio:.3}");
    if worst_ratio > 1.10 {
        eprintln!("warning: an Align depth point is more than 10% slower than its clang control");
    }
    if let Some(limit) = std::env::var("DEEP_PIPELINE_MAX_RATIO")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
    {
        if worst_ratio > limit {
            eprintln!(
                "error: worst ratio {worst_ratio:.3} exceeds DEEP_PIPELINE_MAX_RATIO={limit:.3}"
            );
            std::process::exit(1);
        }
    }
}
