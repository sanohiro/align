# Pipelined compilation benchmark

This local, non-CI benchmark compares optimized compiler binaries from the
pre-item-3 and item-3 revisions on the same cache-off multi-unit `apps/db`
corpus:

```text
BASELINE_ALIGNC=/path/to/baseline/alignc \
PIPELINED_ALIGNC=/path/to/pipelined/alignc \
bench/build_pipeline/run.sh
```

Build each revision with `scripts/cargo.sh build --release --workspace` first;
the linker consumes that revision's adjacent `libalign_runtime.a`.

It alternates seven measured pairs after one warm-up pair, reports medians and
work counts, requires byte-identical executables, and on Linux samples `/proc`
task CPU ticks to observe intervals where the coordinator and an LLVM worker
both advance. `ALIGN_PIPELINE_SAMPLES` may increase the pair count but must be
at least three.
