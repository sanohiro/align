//! Build-target selection (`BuildTarget`) — the codegen CPU baseline. The default is a portable
//! per-architecture floor (`x86-64-v2` on amd64, `armv8-a` on arm64) so one binary runs across a
//! varied cloud/Docker fleet; `--target-cpu native` opts into the build host's exact CPU. Both must
//! produce a correct executable (verified here by building + running under each target).

mod common;
use common::*;

use std::path::PathBuf;

/// Build `src` with `target`, run it, and return the process exit code. Mirrors `build_and_run` but
/// lets the test pick the CPU target.
fn build_run_with(name: &str, src: &str, target: BuildTarget) -> Option<i32> {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(!checked.diags.has_errors(), "unexpected errors");
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let obj: PathBuf = dir.join(format!("align-bt-{pid}-{name}.o"));
    let exe: PathBuf = dir.join(format!("align-bt-{pid}-{name}{}", std::env::consts::EXE_SUFFIX));
    emit_object_file(&mir, &obj, target, Profile::Release, &[], false).expect("codegen");
    link_executable(&align_driver::CDriver::default(), &obj, &exe, &mir.link_libs, Profile::Release).expect("link");
    let code = std::process::Command::new(&exe).status().expect("run").code();
    let _ = std::fs::remove_file(&obj);
    let _ = std::fs::remove_file(&exe);
    code
}

const PROG: &str = "fn main() -> i32 {\n  return [1, 2, 3, 4].map(dbl).where(big).sum()\n}\nfn dbl(x: i32) -> i32 = x * 2\nfn big(x: i32) -> bool = x > 4\n";

#[test]
fn the_portable_baseline_builds_and_runs() {
    if !backend_available() {
        return;
    }
    // 6+8 = 14 (only doubled values > 4 are kept: 6 and 8).
    assert_eq!(build_run_with("bt-baseline", PROG, BuildTarget::Baseline), Some(14));
}

#[test]
fn the_native_target_builds_and_runs() {
    if !backend_available() {
        return;
    }
    // The host CPU build must also be correct (it runs on the machine that built it).
    assert_eq!(build_run_with("bt-native", PROG, BuildTarget::Native), Some(14));
}
#[test]
fn explicit_cpu_names_are_validated_before_emission_or_cache_identity() {
    if !backend_available() {
        return;
    }
    let wrong_arch = if cfg!(target_arch = "aarch64") {
        "x86-64-v3"
    } else {
        "apple-m1"
    };
    for name in ["", "not-an-align-cpu", "native\0suffix", wrong_arch] {
        let error = align_codegen_llvm::resolve_target_identity(&BuildTarget::Cpu(name.into()))
            .err()
            .expect("invalid CPU must be a Rust error, not an LLVM fatal error");
        assert!(error.to_string().contains("CPU"), "{error}");
    }
    let cpu = if cfg!(target_arch = "aarch64") {
        "cortex-a53"
    } else {
        "x86-64-v3"
    };
    let resolved = align_codegen_llvm::resolve_target_identity(&BuildTarget::Cpu(cpu.into()))
        .expect("LLVM's matching CPU inventory must remain available");
    assert_eq!(resolved.cpu, cpu);
}
