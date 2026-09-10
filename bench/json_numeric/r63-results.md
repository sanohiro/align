# R63 local performance acceptance

Linux x86_64, AMD Ryzen 9 5950X, Rust 1.96.0, ordinary release profile,
2026-09-10. Baseline: `d469adcb931848ba03d18dcff022d32d95c2fda9`.
Nine alternating baseline/candidate pairs per workload; no competing build or
test ran during timing. Existing harness inputs and iteration counts are unchanged.
The numeric probe is [main.c](main.c), compiled with `cc -O3 -std=c11 -ldl`.
All 18 final numeric executions produced the same 229-byte encoding.

Values are median [minimum, maximum]. Ratio is the median of paired
candidate/baseline durations; lower is faster. These are local observations,
not a portable throughput guarantee or a CI timing threshold.

| Workload | Baseline | Candidate | Paired ratio |
| --- | --- | --- | --- |
| decode/full, 10,000 rows (ms) | 1.511 [1.497, 1.601] | 1.531 [1.498, 1.596] | 1.0073 |
| decode/projection, 10,000 rows (ms) | 1.428 [1.420, 1.457] | 1.441 [1.412, 1.489] | 1.0070 |
| decode/full, 100,000 rows (ms) | 13.276 [13.205, 13.819] | 13.231 [13.152, 13.508] | 1.0020 |
| decode/projection, 100,000 rows (ms) | 13.354 [13.259, 13.833] | 13.317 [13.212, 13.577] | 1.0008 |
| decode/full, 1,000,000 rows (ms) | 166.939 [165.326, 167.721] | 166.724 [164.603, 168.070] | 0.9975 |
| decode/projection, 1,000,000 rows (ms) | 158.781 [157.045, 160.099] | 158.084 [156.704, 158.975] | 0.9992 |
| soa/soa, 10,000 rows (ms) | 1.333 [1.304, 1.412] | 1.315 [1.300, 1.329] | 0.9925 |
| soa/aos, 10,000 rows (ms) | 1.329 [1.304, 1.407] | 1.308 [1.300, 1.397] | 0.9931 |
| soa/projection, 10,000 rows (ms) | 1.333 [1.321, 1.362] | 1.338 [1.316, 1.356] | 1.0067 |
| soa/soa, 100,000 rows (ms) | 13.420 [13.284, 13.770] | 13.409 [13.213, 13.739] | 0.9999 |
| soa/aos, 100,000 rows (ms) | 13.540 [13.364, 13.861] | 13.479 [13.330, 13.794] | 0.9993 |
| soa/projection, 100,000 rows (ms) | 13.661 [13.368, 13.850] | 13.629 [13.349, 13.804] | 1.0000 |
| soa/soa, 1,000,000 rows (ms) | 151.517 [150.725, 152.430] | 150.852 [149.141, 151.260] | 0.9939 |
| soa/aos, 1,000,000 rows (ms) | 167.397 [165.708, 168.768] | 166.511 [165.122, 167.716] | 0.9959 |
| soa/projection, 1,000,000 rows (ms) | 152.113 [151.477, 153.666] | 151.659 [149.892, 153.251] | 0.9974 |
| decode_f32 (ns/call) | 26.676 [26.214, 27.661] | 24.443 [24.324, 24.902] | 0.9163 |
| decode_f64 (ns/call) | 26.328 [26.077, 27.656] | 25.578 [25.308, 26.737] | 0.9726 |
| decode_u64 (ns/call) | 25.317 [24.709, 26.386] | 24.390 [24.208, 25.824] | 0.9729 |
| encode_raw (ns/call) | 596.615 [590.166, 625.057] | 580.432 [570.073, 592.651] | 0.9741 |
| encode_owned_copy (ns/call) | 609.093 [599.465, 633.031] | 580.432 [570.073, 592.651] | 0.9540 |

Ordinary integer/text decode controls show overlapping spreads and no reproducible
regression. The first numeric implementation exposed a host-rounding defect in
Rust's native fast path; the integer-only converter repairs it without a parsing
slowdown. The first fallible encoder had a reproducible approximately 18 ns/call
increase. JSON now writes identical Display digits through `fmt::Write` directly,
avoiding the `io::Write` adapter; the final nine pairs above close that regression.
Ordinary templates/print retain their existing formatter. Decoder control runs
precede this final JSON-only formatting optimization; decoder implementation did
not change afterward. The final numeric pairs use the final production runtime.

## Resources

Resource probes run in separate, untimed `alloc-count` processes using
[resources.py](resources.py). The same output has 256 bytes of retained builder
capacity. Baseline raw and candidate each peak at 384 requested live bytes,
including simultaneous old/new growth layouts; the baseline explicit owned copy
peaks at 485 bytes and retains 229 bytes after freeing the grow buffer. Every
probe returns to zero live bytes after receiving-owner cleanup. R63 transfers
one grow buffer and makes no final full-result allocation/copy. The existing
allocation-call counter excludes realloc, so it is not presented as a total
allocator-call count. Runtime owner tests separately pin pointer transfer,
failure cleanup, capacity ceilings, and omitted-cleanup negative controls.

## Artifact identity and execution detail

The final numeric runtime SHA-256 is
`df7d55182dc0b381db40096ee827676c1a4cf901d23153cce6d3dded40f81afb`.
Numeric C source SHA-256 is
`916ac736a797030ca69426b5dd12a700ed3a941dd0a88f97f29b106d1f1d8846`.
Prepared decode/SoA artifact-manifest SHA-256 values:

| Prepared artifact | Manifest SHA-256 |
| --- | --- |
| base-decode | `91a22cbffa519e134a4cefa689850483c5a3df4199cbcb5a8c59179cb40babeb` |
| optimized-decode | `de7f62e5a83c6e5865a1d26baab7f34ff578d997dcb2e8353badc77a6c3fb7b7` |
| base-soa | `a62be888ebc2914c52c6b2b9e683604bdb1f8d373f747c99759824f93b1516d2` |
| optimized-soa | `d49b564c1299ae5cc8f96bf1a1d1086892f1de7b4c7fc074155ee0c09d3213d1` |

Raw outputs, manifests, prepared binaries and the alternating runner are retained
under `/tmp/r63-perf`. The existing Linux fd-preload `native` wrapper could not
resolve the prepared runtime's DT_NEEDED name because that shared library has no
DT_SONAME. For these local measurements, the runner verified each prepared tree
against its manifest, then executed its unchanged native harness with an explicit
private `LD_LIBRARY_PATH`. No sealed artifact was edited. This is a local timing
workaround, not a claim of protected hostile-input evidence execution. The final
numeric probe loads its recorded runtime directly with `dlopen`.
