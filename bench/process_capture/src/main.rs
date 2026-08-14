use align_runtime::{
    align_rt_command_free, align_rt_command_max_capture, align_rt_command_new,
    align_rt_command_run, align_rt_command_run_bytes, align_rt_run_bytes_code,
    align_rt_run_bytes_free, align_rt_run_bytes_stderr, align_rt_run_bytes_stdout,
    align_rt_run_output_code, align_rt_run_output_free, align_rt_run_output_stderr,
    align_rt_run_output_stdout, AlignStr, Command, RunBytes, RunOutput,
};
use std::time::Instant;

#[derive(Clone, Copy)]
enum Mode {
    Text,
    Bytes,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Bytes => "bytes",
        }
    }
}

struct CommandGuard(*mut Command);

impl Drop for CommandGuard {
    fn drop(&mut self) {
        unsafe { align_rt_command_free(self.0) };
    }
}

fn command(payload: usize, bounded: bool) -> CommandGuard {
    let program = format!("head -c {payload} /dev/zero; head -c {payload} /dev/zero 1>&2");
    let argv_storage = ["/bin/sh".to_string(), "-c".to_string(), program];
    let argv = argv_storage
        .iter()
        .map(|arg| AlignStr {
            ptr: arg.as_ptr(),
            len: arg.len() as i64,
        })
        .collect::<Vec<_>>();
    let path = "/bin/sh";
    let raw = unsafe {
        align_rt_command_new(
            path.as_ptr(),
            path.len() as i64,
            argv.as_ptr(),
            argv.len() as i64,
        )
    };
    assert!(!raw.is_null());
    if bounded {
        unsafe { align_rt_command_max_capture(raw, payload as i64) };
    }
    CommandGuard(raw)
}

unsafe fn view_len(view: AlignStr) -> usize {
    assert!(view.len >= 0);
    view.len as usize
}

fn run_once(mode: Mode, payload: usize, bounded: bool) -> u128 {
    let command = command(payload, bounded);
    let start = Instant::now();
    match mode {
        Mode::Text => {
            let mut out: *mut RunOutput = std::ptr::null_mut();
            assert_eq!(unsafe { align_rt_command_run(command.0, &mut out) }, 0);
            let elapsed = start.elapsed().as_nanos();
            assert_eq!(unsafe { align_rt_run_output_code(out) }, 0);
            assert_eq!(
                unsafe { view_len(align_rt_run_output_stdout(out)) },
                payload
            );
            assert_eq!(
                unsafe { view_len(align_rt_run_output_stderr(out)) },
                payload
            );
            unsafe { align_rt_run_output_free(out) };
            elapsed
        }
        Mode::Bytes => {
            let mut out: *mut RunBytes = std::ptr::null_mut();
            assert_eq!(
                unsafe { align_rt_command_run_bytes(command.0, &mut out) },
                0
            );
            let elapsed = start.elapsed().as_nanos();
            assert_eq!(unsafe { align_rt_run_bytes_code(out) }, 0);
            assert_eq!(unsafe { view_len(align_rt_run_bytes_stdout(out)) }, payload);
            assert_eq!(unsafe { view_len(align_rt_run_bytes_stderr(out)) }, payload);
            unsafe { align_rt_run_bytes_free(out) };
            elapsed
        }
    }
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn main() {
    assert!(std::path::Path::new("/bin/sh").exists());
    assert!(std::path::Path::new("/dev/zero").exists());
    let reps = std::env::var("REPS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10);
    assert!(reps > 0);

    println!(
        "mode\tbound\tpayload_per_stream\tmedian_ms\tMiB_per_s\tmax_live_capture_layout_bytes"
    );
    for payload in [65_536usize, 262_144] {
        for bounded in [true, false] {
            for mode in [Mode::Text, Mode::Bytes] {
                let _ = run_once(mode, payload, bounded);
                let elapsed = (0..reps)
                    .map(|_| run_once(mode, payload, bounded))
                    .collect::<Vec<_>>();
                let ns = median(elapsed);
                let mib_per_s =
                    (payload * 2) as f64 * 1_000_000_000.0 / ns as f64 / (1024.0 * 1024.0);
                let bound = if bounded { "bounded" } else { "unbounded" };
                let layout = if bounded {
                    (payload * 2).to_string()
                } else {
                    "unbounded".to_string()
                };
                println!(
                    "{}\t{}\t{}\t{:.3}\t{:.1}\t{}",
                    mode.name(),
                    bound,
                    payload,
                    ns as f64 / 1_000_000.0,
                    mib_per_s,
                    layout
                );
            }
        }
    }
}
