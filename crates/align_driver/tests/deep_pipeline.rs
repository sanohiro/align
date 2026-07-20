//! Deep-stage pipeline performance-contract gate (`docs/impl/12-...` §4.5).
//!
//! The throughput harness lives in `bench/deep_pipeline`; this test pins the non-timing invariants
//! that are stable enough for CI. It compiles the shared 4-family × 6-depth fixture on an explicit
//! 2 MiB stack through check, MIR, optimized LLVM, and object emission. Every kernel must remain one
//! fused MIR loop with no intermediate allocation. Simple named/capturing stages must inline, and
//! the three vectorization-legal families must retain an integer vector reduction through depth 32.

mod common;
use common::*;

use align_mir::Term;
use std::path::PathBuf;
use std::process::Command;

const DEPTHS: [usize; 6] = [1, 2, 4, 8, 16, 32];
const FAMILIES: [&str; 4] = ["named", "masked", "capture", "guarded"];
const VECTORIZABLE_FAMILIES: [&str; 3] = ["named", "masked", "capture"];
const SOURCE: &str = include_str!("../../../bench/deep_pipeline/kernels.align");

fn target() -> BuildTarget {
    if cfg!(target_arch = "x86_64") {
        BuildTarget::Cpu("x86-64-v3".to_string())
    } else {
        BuildTarget::Baseline
    }
}

fn clang22_available() -> bool {
    let clang = std::env::var("CLANG").unwrap_or_else(|_| "clang-22".to_string());
    Command::new(clang)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn benchmark_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../bench/deep_pipeline")
        .canonicalize()
        .expect("bench/deep_pipeline exists")
}

fn kernel_names() -> Vec<String> {
    FAMILIES
        .iter()
        .flat_map(|family| DEPTHS.iter().map(move |depth| format!("{family}_{depth}")))
        .collect()
}

fn cyclic_components(f: &align_mir::Function) -> usize {
    let n = f.blocks.len();
    let mut reach = vec![vec![false; n]; n];
    for block in &f.blocks {
        let from = block.id as usize;
        match &block.term {
            Term::Goto(target) => reach[from][*target as usize] = true,
            Term::Branch(_, then_target, else_target) => {
                reach[from][*then_target as usize] = true;
                reach[from][*else_target as usize] = true;
            }
            Term::Return(_) | Term::Unreachable => {}
        }
    }
    for via in 0..n {
        for from in 0..n {
            for to in 0..n {
                reach[from][to] |= reach[from][via] && reach[via][to];
            }
        }
    }

    let mut seen = vec![false; n];
    let mut cyclic = 0;
    for root in 0..n {
        if seen[root] {
            continue;
        }
        let component: Vec<usize> = (0..n)
            .filter(|&other| other == root || (reach[root][other] && reach[other][root]))
            .collect();
        for &member in &component {
            seen[member] = true;
        }
        if component.len() > 1 || reach[root][root] {
            cyclic += 1;
        }
    }
    cyclic
}

fn llvm_function<'a>(ir: &'a str, name: &str) -> &'a str {
    let symbol = format!("@{name}(");
    let mut search_from = 0;
    let start = loop {
        let symbol_pos = ir[search_from..]
            .find(&symbol)
            .map(|position| search_from + position)
            .unwrap_or_else(|| panic!("missing LLVM definition for `{name}`"));
        let line_start = ir[..symbol_pos]
            .rfind('\n')
            .map(|position| position + 1)
            .unwrap_or(0);
        let line_end = ir[symbol_pos..]
            .find('\n')
            .map(|position| symbol_pos + position)
            .unwrap_or(ir.len());
        if ir[line_start..line_end].trim_start().starts_with("define ") {
            break line_start;
        }
        search_from = symbol_pos + symbol.len();
    };
    let tail = &ir[start..];
    let end = tail
        .find("\n}\n")
        .unwrap_or_else(|| panic!("unterminated LLVM definition for `{name}`"))
        + 3;
    &tail[..end]
}

struct TempObject(PathBuf);

impl Drop for TempObject {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn depth_sweep_preserves_fusion_inlining_vectorization_and_small_stack_survival() {
    if !backend_available() {
        return;
    }

    // A stack overflow aborts the process rather than unwinding, so merely reaching `join` is the
    // robustness assertion. The explicit size makes the gate independent of the test runner's
    // platform default and exercises the documented small-stack constraint.
    std::thread::Builder::new()
        .name("deep-pipeline-2mib".to_string())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut sm = SourceMap::new();
            let checked = check(&mut sm, "deep-pipeline", SOURCE);
            assert!(
                !checked.diags.has_errors(),
                "deep pipeline fixture failed to check:\n{}",
                align_driver::format_diagnostics(&sm, &checked.diags)
            );

            let mir = lower_to_mir(&checked.hir);
            let names = kernel_names();
            for name in &names {
                let function = mir
                    .fns
                    .iter()
                    .find(|f| f.name == *name)
                    .unwrap_or_else(|| panic!("missing MIR function `{name}`"));
                assert_eq!(
                    cyclic_components(function),
                    1,
                    "`{name}` must contain exactly one fused loop:\n{}",
                    align_mir::print::function_to_string(function)
                );
                let text = align_mir::print::function_to_string(function);
                assert!(
                    !text.contains("heap_alloc")
                        && !text.contains("arena_alloc")
                        && !text.contains("soa_alloc"),
                    "`{name}` must not allocate an intermediate collection or closure:\n{text}"
                );
            }

            let ir =
                emit_llvm_ir(&mir, target(), true, &names, false).expect("emit optimized LLVM");
            for name in &names {
                let body = llvm_function(&ir, name);
                for line in body.lines().filter(|line| line.contains(" call ")) {
                    assert!(
                        line.contains("@llvm."),
                        "`{name}` retained a non-intrinsic stage/runtime call:\n{line}\n\n{body}"
                    );
                }
            }

            // The vector-SHAPE gate is x86-64-v3 only, deliberately — the fusion, inlining,
            // no-allocation and no-runtime-call gates above are the arch-independent ones and run
            // everywhere.
            //
            // These kernels reduce a chain of `i64` multiplies (`mix`), and **AArch64 NEON has no
            // 64-bit-lane integer multiply** (`MUL` covers `.8b/.16b/.4h/.8h/.2s/.4s`, not `.2d`),
            // so there is no lane-reduced form for LLVM to pick: it emits scalar `madd`, and past a
            // shallow depth stops vectorizing the loop at all. x86-64-v3 has no 64-bit vector
            // multiply either (`vpmullq` is AVX-512DQ), but LLVM emulates it with 32-bit multiplies
            // and finds that profitable, which is why the shape holds there at every depth.
            //
            // Measured before scoping this, so it is not a guess and not a papered-over regression:
            //   - Not an LLVM 22 regression — `opt` 19, 20, 21 and 22 all produce the same scalar
            //     result from the same input IR.
            //   - Not an Align codegen defect — the identical loop written in C and compiled by
            //     clang for arm64 does not vectorize at ANY depth, while Align's still does at
            //     depth 1-2. A C loop with the multiply REMOVED becomes `<2 x i64>` +
            //     `llvm.vector.reduce.add`, and with 32-bit lanes `<4 x i32>`, which is what pins
            //     the multiply as the sole cause.
            //
            // The aarch64 arm of this gate previously asserted the x86-64 shape and so could never
            // have passed. Asserting today's aarch64 shape instead would be curve-fitting to one
            // LLVM cost-model cutoff — the depth at which it gives up is a threshold, not a
            // contract — so nothing is asserted there rather than something meaningless.
            if cfg!(target_arch = "x86_64") {
                for family in VECTORIZABLE_FAMILIES {
                    for depth in DEPTHS {
                        let name = format!("{family}_{depth}");
                        let body = llvm_function(&ir, &name);
                        assert!(
                            body.contains("llvm.vector.reduce.add"),
                            "`{name}` lost its vector reduction:\n{body}"
                        );
                        assert_eq!(
                            body.lines()
                                .filter(|line| line.trim_start().starts_with("vector.ph:"))
                                .count(),
                            1,
                            "`{name}` must contain one main vector loop:\n{body}"
                        );
                    }
                }
            }

            let object = std::env::temp_dir().join(format!(
                "align-deep-pipeline-{}-{}.o",
                std::process::id(),
                std::thread::current().name().unwrap_or("worker")
            ));
            let _cleanup = TempObject(object.clone());
            emit_object_file(&mir, &object, target(), Profile::Release, &names, false)
                .expect("emit deep-pipeline object");
            assert!(
                object.is_file(),
                "object emission did not produce an artifact"
            );
        })
        .expect("spawn 2 MiB deep-pipeline worker")
        .join()
        .expect("deep-pipeline worker panicked");
}

#[test]
fn equal_llvm_harness_checks_all_depth_shapes_and_results() {
    if !(backend_available()
        && clang22_available()
        && cfg!(any(target_arch = "x86_64", target_arch = "aarch64")))
    {
        return;
    }

    let output = Command::new("bash")
        .arg(benchmark_dir().join("run.sh"))
        .arg("baseline")
        .env("ALIGNC", env!("CARGO_BIN_EXE_alignc"))
        .env("ALIGNC_CACHE", "off")
        .env("DEEP_PIPELINE_N", "4096")
        .env("DEEP_PIPELINE_ROUNDS", "3")
        .env("DEEP_PIPELINE_STAGE_ELEMENTS", "65536")
        .output()
        .expect("launch deep-pipeline harness");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "deep-pipeline harness failed\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("optimized-IR reduction parity: 24/24"),
        "missing complete IR-parity result:\n{stdout}"
    );
    assert!(
        stdout.contains("worst Align/clang ratio:"),
        "throughput harness did not complete:\n{stdout}"
    );
}
