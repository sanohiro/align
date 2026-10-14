# Indexed JSON grammar cost observation

Plan187 repairs complete JSON grammar validation and selected-number boundaries
in the existing AoS/SoA array decoders. These measurements record the cost of
that correctness repair; they do not promise a parser or application speedup.

The normal sealed `json_decode` preparation failed on this Mac when the system
linker rejected `libalign_runtime.dylib` with `mis-aligned LINKEDIT string pool`.
This fallback copies the unchanged `json_decode/kernel.align` and
`json_soa/kernel.align` into an exclusive temporary directory and builds one
ordinary executable with the statically linked runtime. It makes no serde
comparison. The existing signal-aware `native_probe` supervisor owns every
compiler/executable process group and retires children before scratch cleanup.

Build each committed producer with `cargo build --release --workspace` in the
repository's resolved LLVM environment, then run without competing builds:

```text
python3 bench/json_array_grammar/measure.py /absolute/producer/alignc /absolute/results.json --source-id COMMIT
```

The adjacent `libalign_runtime.a` must belong to that same producer build.
Generated code uses the release profile, native CPU, disabled runtime LTO and
disabled codegen cache. The four paths are AoS full, AoS projected, SoA full and
SoA projected. Input is the existing benchmark's deterministic four-field LCG
corpus at 10,000, 100,000 and 1,000,000 records. Input I/O and all four row-count
checks are outside timing; each measured aggregate is checked against the
independent generator after timing. Each size has three fresh executable
processes, each with forty timings per path in that fixed order. No warm-up
samples are removed.

`before.json` and `after.json` retain every nanosecond sample, exact expected
sums, input and source hashes, compiler/runtime hashes, source commits and host.
The reported statistic is the median of the three process medians, in
milliseconds. This is one local macOS ARM64 observation; it does not measure
portable throughput or consumer startup. Generated input, objects and native
executables are removed after each run.

| Records | Path | Before ms | After ms | Change |
| ---: | --- | ---: | ---: | ---: |
| 10,000 | AoS full | 1.434 | 2.040 | +42.2% |
| 10,000 | AoS projected | 1.399 | 1.982 | +41.7% |
| 10,000 | SoA full | 1.417 | 1.939 | +36.8% |
| 10,000 | SoA projected | 1.372 | 1.904 | +38.7% |
| 100,000 | AoS full | 14.276 | 20.663 | +44.7% |
| 100,000 | AoS projected | 14.018 | 19.998 | +42.7% |
| 100,000 | SoA full | 14.212 | 19.532 | +37.4% |
| 100,000 | SoA projected | 13.938 | 19.136 | +37.3% |
| 1,000,000 | AoS full | 145.142 | 207.808 | +43.2% |
| 1,000,000 | AoS projected | 139.978 | 200.577 | +43.3% |
| 1,000,000 | SoA full | 143.146 | 196.265 | +37.1% |
| 1,000,000 | SoA projected | 140.079 | 191.698 | +36.8% |
