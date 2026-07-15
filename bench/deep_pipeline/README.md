# Deep pipeline scaling

This harness implements the performance-contract gate recorded in
[`docs/impl/12-pipeline-closure-memory-io-simd-audit.md`](../../docs/impl/12-pipeline-closure-memory-io-simd-audit.md)
§4.5. It measures pipeline callable depth 1/2/4/8/16/32 across four shapes:

- named arithmetic maps ending in `sum`;
- maps followed by a reducing `where`, retaining the legal mask path;
- scalar-capturing inline lambdas; and
- the correctness-required guarded callable suffix after `where`.

For `masked`, the suffix is the total stage count (`D-1` maps plus one predicate). For `guarded`,
the depth counts callables after one fixed leading predicate; that is the dimension whose inlining
and dependency-chain cost the negative control is meant to stress.

The shared [`kernels.align`](kernels.align) fixture is also compiled by the driver integration test.
That non-timing gate runs on an explicit 2 MiB stack and asserts one fused MIR loop, no intermediate
allocation, no residual non-intrinsic calls, and retained vector reductions for every legal family.

## Run

```sh
bench/deep_pipeline/run.sh native
bench/deep_pipeline/run.sh baseline
bench/deep_pipeline/run.sh v3       # x86-64 only
```

Align and the C controls are both built through LLVM 22 at O2 for the same CPU target. Before
timing, the script compares all 24 optimized functions and fails if the vector-reduction decision
or lane width differs. Rust only supplies the timer and runtime-generated input. Each
point uses balanced AB/BA order and reports the median of an odd number of rounds. The result is
checked before timing. `align ns/stage` normalizes useful added work; raw latency is expected to
increase with depth. An indexed Align `loop` is deliberately not a control: it is the noncanonical
data-path spelling and shares more of the compiler than the independent equal-LLVM C ceiling does.

Useful controls:

```sh
DEEP_PIPELINE_N=1048576 DEEP_PIPELINE_ROUNDS=9 bench/deep_pipeline/run.sh native
DEEP_PIPELINE_MAX_RATIO=1.10 bench/deep_pipeline/run.sh native
```

`DEEP_PIPELINE_MAX_RATIO` is opt-in because shared CI timing is not stable enough for a universal
wall-clock threshold. The IR/CFG invariants are the mandatory regression gate; recorded dedicated
machine results establish whether a throughput threshold is earned. `clang-22` is required.

## Recorded baseline

**2026-07-15, Ryzen 9 5950X, Linux x86-64, LLVM/clang 22.1.8, O2, 1,048,576
elements, 9 balanced rounds.** Both native and portable x86-64-v2 runs stayed within 7.1% of the
equal-LLVM C control at every one of the 24 points:

| family | native worst ratio | native depth-32 | baseline worst ratio | baseline depth-32 |
|---|---:|---:|---:|---:|
| named maps | 1.026 | 0.991 | 1.012 | 1.005 |
| masked reduce | 1.055 | 1.011 | 1.071 | 1.000 |
| guarded suffix | 1.016 | 0.999 | 1.001 | 1.001 |
| scalar capture | 1.013 | 0.981 | 1.004 | 1.004 |

There is no Align-specific depth cliff. Native named-map cost stayed 0.207→0.219 ns per stage and
capture cost 0.158→0.162 ns per stage from depth 1→32. The portable v2 target showed the same
depth-dependent increase as C for long serial dependency chains (named 0.380→0.706 ns/stage,
capture 0.306→0.527); that is a target/code-shape cost of the useful work, not pipeline abstraction
overhead. The correctness-required guarded suffix likewise rises on both sides because the branch
and long dependency chain are real semantics.

With compiler cache disabled, the full 24-kernel fixture measured:

| compiler stage | elapsed | sampled peak RSS |
|---|---:|---:|
| check | 0.017 s | 52,152 KiB |
| MIR emission | 0.017 s | 46,788 KiB |
| optimized LLVM emission | 0.204 s | 70,292 KiB |
| release object emission | 0.510 s | 77,844 KiB |

RSS was sampled from Linux `/proc/<pid>/status` at 1 ms intervals because GNU `time` was not
installed. The mandatory integration test completes the same deepest path on an explicit 2 MiB
thread stack (0.64 s in the recorded run).
