# Shared recursive-Drop codegen benchmark

This local, non-gating benchmark owns the resource promise in
`docs/impl/21-build-perf-plan.md` item 3a. It compares optimized compiler
binaries from the pre-change and candidate revisions on two inputs:

- `control.align`, a one-shot small Move record whose instrumented runtime must
  print its value and report exactly one allocation and one destructor free; and
- align-llm Request 19's `prompt_verifier_smoke.align`, the pathological wide
  Move-record and early-exit client fixture.

Build each revision's `alignc` and then rebuild the adjacent runtime archive
with `align_runtime`'s `alloc-count` feature. Keep each binary and archive in a
separate directory, then run:

```text
BASELINE_ALIGNC=/path/to/baseline/alignc \
CANDIDATE_ALIGNC=/path/to/candidate/alignc \
REQUEST19_SOURCE=/path/to/align-llm/src/prompt_verifier_smoke.align \
bench/large_drop_codegen/run.sh
```

The harness uses fresh caches to report actual frontend/codegen miss counts,
then reports raw-IR lines, release object bytes, build wall time and peak
compiler RSS, and generated-program cleanup wall time and peak RSS. It requires
byte-identical program output, the exact `1 / 1` control allocation/free result,
and Request 19's PASS line. Both arms use `--no-rt-lto` so the allocation
counters come from the instrumented archive rather than the ordinary runtime
bitcode. Run the real consumer build separately with its default runtime-LTO
policy before accepting the item.
