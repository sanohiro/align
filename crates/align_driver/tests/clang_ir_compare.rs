//! M13 Slice V (c) — smoke test for the Clang-IR comparison harness (`bench/clang_ir_compare/`).
//!
//! This does NOT assert Align == clang (shape differences are *findings*, recorded in that dir's
//! README, not failures). It only proves the harness itself RUNS end to end when its tools are
//! present: it invokes `run.sh` with the freshly-built `alignc` (so no nested `cargo build`) and
//! `clang-19`, and checks it produced the comparison table and its own MATCH/DIVERGE summary.
//!
//! Gating: x86-64 host (the pinned CPU tiers are x86) + LLVM backend + `clang-19` present. Anywhere
//! one is missing the test skips cleanly — a machine without clang-19 is not a failure.

mod common;
use common::*;

use std::path::PathBuf;
use std::process::Command;

fn clang19_available() -> bool {
    Command::new("clang-19")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The repo's `bench/clang_ir_compare` directory, resolved from this test file's location so the
/// test is independent of the process working directory.
fn harness_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../bench/clang_ir_compare")
        .canonicalize()
        .expect("bench/clang_ir_compare exists")
}

#[test]
fn harness_runs_and_emits_the_comparison_table() {
    if !(cfg!(target_arch = "x86_64") && backend_available() && clang19_available()) {
        return;
    }
    let dir = harness_dir();
    // Hand the harness the alignc binary this crate builds, so `run.sh` skips its own cargo build
    // (nested cargo under `cargo test` is slow and can deadlock on the build lock).
    let out = Command::new("bash")
        .arg(dir.join("run.sh"))
        .env("ALIGNC", env!("CARGO_BIN_EXE_alignc"))
        .output()
        .expect("launch run.sh");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "harness exited non-zero.\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The harness must have produced the table (header) and its summary line, and covered all five
    // kernels (each prints an `align` row).
    assert!(stdout.contains("kernel"), "missing table header:\n{stdout}");
    assert!(
        stdout.contains("MATCH") || stdout.contains("DIVERGE"),
        "missing per-kernel verdict:\n{stdout}"
    );
    let align_rows = stdout.matches("| align |").count();
    assert!(
        align_rows >= 5,
        "expected >=5 kernel rows, got {align_rows}:\n{stdout}"
    );
}
