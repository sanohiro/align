//! M13 Slice V (a) — `--target-cpu` ISA-selection verification.
//!
//! The consultation flagged a concern: [`BuildTarget::Cpu(name)`] passes an **empty feature string**
//! to `create_target_machine(cpu, "")` (`align_codegen_llvm`), so a named CPU tier might not enable
//! that CPU's ISA extensions (AVX2 etc.) — leaving the vectorizer's `<4 x i64>` to be scalarized or
//! narrowed at the backend. This suite settles it **empirically** at the machine-code level.
//!
//! Verdict (probed 2026-07-11, LLVM 19): the empty feature string is CORRECT. LLVM derives the ISA
//! feature set from the CPU name itself (`getFeaturesForCPU`), so `x86-64-v3` enables AVX2 and the
//! backend selects `ymm` / `vpaddq`; `x86-64-v2` stays on SSE (`xmm` / `paddq`, no `ymm`). No fix
//! needed. This pins the residual that `vectorize_shapes.rs` does not: that suite pins the *IR*
//! widths (`<4 x i64>` at v3, `<2 x i64>` at v2); this one pins actual **instruction selection** —
//! the last link in the chain the empty-feature-string concern was about.
//!
//! Gating: x86-64 host + LLVM backend + `objdump` present. Anywhere one is missing the tests skip
//! cleanly (a machine without `objdump`, or a non-x86 host where these CPU names don't match the
//! triple, is not a failure). Re-verify the `ymm`/`vpaddq`/`paddq` spellings at the LLVM upgrade.

mod common;
use common::*;

use std::path::PathBuf;

/// Whether `objdump` is on PATH — these tests disassemble the emitted object. Skip where absent.
fn objdump_available() -> bool {
    std::process::Command::new("objdump")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The pinned CPU names are x86 tiers, so the suite only runs on an x86-64 host with the backend and
/// `objdump` available. Elsewhere the requested CPU would not match the host triple.
fn ready() -> bool {
    cfg!(target_arch = "x86_64") && backend_available() && objdump_available()
}

/// A vectorizable int reduction over a runtime-length prefix (unknown trip count → the loop
/// survives to vectorize; a fixed literal length would constant-fold away). Mirrors the
/// `vectorize_shapes` reduction kernel so the two suites pin the same lowering from two lenses.
const KERNEL: &str = "\
fn dbl(x: i64) -> i64 = x * 2\n\
fn run(xs: slice<i64>) -> i64 = xs.map(dbl).sum()\n\
fn main(args: array<str>) -> Result<(), Error> {\n  \
  a := [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]\n  \
  s : slice<i64> := a[0..args.len()]\n  \
  print(run(s))\n  \
  return Ok(())\n\
}\n";

/// Removes a temp object on scope exit (incl. panic), so a failing assert leaks nothing.
struct TempObj(PathBuf);
impl Drop for TempObj {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Compile `KERNEL` to an object at `--target-cpu cpu` and return its `objdump -d` disassembly.
fn disasm_at(cpu: &str, tag: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, tag, KERNEL);
    assert!(
        !checked.diags.has_errors(),
        "kernel failed to compile:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let obj = std::env::temp_dir().join(format!("align-isa-{}-{tag}.o", std::process::id()));
    let _guard = TempObj(obj.clone());
    emit_object_file(&mir, &obj, BuildTarget::Cpu(cpu.to_string()), Profile::Release)
        .expect("codegen");
    let out = std::process::Command::new("objdump")
        .arg("-d")
        .arg(&obj)
        .output()
        .expect("run objdump");
    assert!(out.status.success(), "objdump failed on {}", obj.display());
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// --- x86-64-v3 (AVX2) — the CPU name alone must enable 256-bit vector instruction selection ---

#[test]
fn cpu_v3_selects_avx2_instructions() {
    if !ready() {
        return;
    }
    let d = disasm_at("x86-64-v3", "v3");
    // AVX2: 256-bit `ymm` registers and the VEX-encoded `vpaddq` (packed 64-bit add) from the
    // vectorized reduction. If the empty feature string had NOT enabled AVX2, the vectorizer's
    // `<4 x i64>` would lower to `xmm`/`paddq` (SSE) or scalarize — so these presence checks are
    // exactly the empty-feature-string verification.
    assert!(d.contains("ymm"), "v3 must select 256-bit ymm registers; disasm:\n{d}");
    assert!(d.contains("vpaddq"), "v3 must select AVX2 vpaddq; disasm:\n{d}");
}

// --- x86-64-v2 (SSE only) — pins that the CPU name TRACKS the tier (no AVX2 leakage) ---

#[test]
fn cpu_v2_selects_sse_not_avx2() {
    if !ready() {
        return;
    }
    let d = disasm_at("x86-64-v2", "v2");
    // v2 tops out at SSE4.2 — no AVX2, so no 256-bit `ymm`. The vectorized reduction uses 128-bit
    // `xmm`/`paddq` instead. Absence-of-`ymm` is the teeth: it proves the feature set is derived
    // from *this* CPU name (not silently widened to the host's).
    assert!(
        !d.contains("ymm"),
        "v2 must NOT select 256-bit ymm (SSE tier); disasm:\n{d}"
    );
    assert!(d.contains("paddq"), "v2 must select SSE paddq; disasm:\n{d}");
}

// --- a named microarchitecture (skylake) — the same derivation works for a real CPU name ---

#[test]
fn named_cpu_skylake_selects_avx2() {
    if !ready() {
        return;
    }
    let d = disasm_at("skylake", "skylake");
    // Skylake has AVX2; the CPU name alone must enable it (same `getFeaturesForCPU` path as the
    // vN tiers), proving the empty feature string is correct for named CPUs too.
    assert!(
        d.contains("ymm"),
        "skylake must select 256-bit ymm registers; disasm:\n{d}"
    );
}
