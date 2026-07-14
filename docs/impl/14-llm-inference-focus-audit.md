# LLM inference and MoE optimization focus audit

> Status: measured planning record, 2026-07-14. No compiler or language feature is shipped by this
> document. It extends the completed M12 align-LLM runway and the Future "Resource-oriented north
> star" with engine-level priorities pulled by model loading, one-token decode, and MoE execution.

## 1. Decision summary

Align already has the right orchestration substrate: arena-scoped binary `mmap`, inline
endian-explicit scalar reads, checked allocation lowering, offset I/O for `alignpack`, growable typed
builders, structured tasks, FFI, SIMD-visible loops, and streaming server I/O. Do not add LLM syntax
or implement a second generic tensor compiler before a real engine exists.

The priority order for an Align-written inference engine is:

1. Keep quantized weights packed and call mature shaped backend operations (`matvec`/`matmul`,
   attention, and MoE `matmul_id`) through FFI. Fuse dequantization with dot/matrix work; never
   materialize a full dequantized model or tensor merely to call a kernel.
2. Make model placement and execution-order layout inspectable. `mmap` removes a copy; it does not
   make pages resident. `alignpack`/the planner should arrange or pre-stage the pages the execution
   graph will consume, then report which tier and byte count each phase touches.
3. Treat KV layout as an operation/backend choice, not one universal row-major decision. Decode K
   score formation, V accumulation, append, prompt prefill, paged blocks, Flash Attention, CPU, and
   GPU have different profitable access directions.
4. For MoE, make selected-expert execution one shaped operation. Direct execution is the small-batch
   default; stable expert grouping is earned from route count, distinct-expert count, routing skew,
   and expert working-set/tier evidence.
5. Optimize tokenizer, sampling, and HTTP/SSE only after phase timings show they are visible. They
   matter to tail latency and concurrency, but weight/KV traffic dominates ordinary decode.

These are engine/pkg/backend decisions. The only possible future language-adjacent substrate is the
already-recorded minimal strided `ndslice`; even that stays consumer-gated. Backend tensor handles,
quant formats, VRAM tiers, expert caches, and layout enums belong in `pkg`, not core types.

## 2. Model loading: zero-copy is necessary but page order still dominates cold start

The shipped `fs.read_bytes_view` is the correct GGUF entry: one arena-scoped mapping, typed
descriptors/views into it, and no full-file copy. The loader must still validate every count, offset,
alignment, quant block extent, tensor overlap, and checked size product before creating a view or
allocation. Binary scalar reads abort on an unchecked out-of-range access by design, so an untrusted
model parser checks structure and returns an ordinary parse `Error` first.

**Cold page-in probe (2026-07-14, Linux ext4, Ryzen 9 5950X, 256 MiB file):** every trial evicted the
file from page cache, mapped it privately, spent the same 20 ms in a simulated metadata/init window,
then touched one byte per 4 KiB page. Trials were balanced and repeated seven times. The
out-of-execution-order control shuffled 1 MiB blocks while retaining sequential access inside each
block.

| first-touch plan | total including common 20 ms | after subtracting common 20 ms | major faults |
|---|---:|---:|---:|
| execution-order sequential demand paging | 66.7 ms | 46.7 ms | 1 |
| execution-order + `MADV_WILLNEED` | 179.5 ms | 159.5 ms | 31 |
| shuffled 1 MiB blocks, demand paging | 178.1 ms | 158.1 ms | 38 |
| shuffled 1 MiB blocks + `MADV_WILLNEED` | 156.6 ms | 136.6 ms | 37 |

Execution-order layout was 2.67x faster in total (3.38x after the common window) than the shuffled
block order. Advice was state-dependent: it improved the shuffled case by 1.14x but made the already
sequential case 2.69x slower on this host. Therefore prioritize `alignpack` execution-order/tier
relayout and measured warmup over a blanket `madvise` policy. Advice/prefetch remains an explicit
planner decision keyed by storage, access order, and overlap opportunity, with no source-language
effect. This probe measures page residency, not full model inference bandwidth; repeat on NVMe,
network filesystems, macOS, and a real GGUF execution trace before fixing thresholds.

## 3. KV cache: preserve layout choice through the shaped attention boundary

The existing paged-block/prefix-sharing memory plan solves allocation waste, not the inner access
layout. A strict ordered-`f32` CPU fallback was probed with head dimension 128 and context lengths
128 through 32,768. Both alternatives performed the same multiply/add order for each result and
matched byte-for-byte; clang 22 used scalar inner reductions for token-row K dots, but YMM
output-token lanes when time was contiguous. V showed the complementary result.

| decode operation | profitable layout on this ordered CPU fallback | measured advantage |
|---|---|---:|
| `score[t] = dot(q, K[t])` | K dimension-major, time contiguous; tokens are SIMD lanes | 5.8-11.0x over token-row K |
| `out[d] += p[t] * V[t,d]` | V token-major, dimension contiguous; output dimensions are SIMD lanes | 5.4-13.8x over time-major V |

The tradeoff is visible: appending one 128-value K row took about 6-16 ns in token-major storage but
201-587 ns in time-contiguous storage because the latter writes one value into each far-separated
dimension row. Even at context 128, however, the measured score-read saving was several
microseconds, larger than the append penalty. Prompt prefill changes the ratio and can block/transpose
many tokens at once; do not infer its policy from the one-token result.

Adopt a **P1 engine/backend design gate**, not a fixed representation: a KV descriptor carries
layout/strides and paged-block boundaries into the attention operation. The backend chooses or
converts once per phase based on prefill vs decode, context length, batch, head/GQA dimensions,
element/quant type, ordered vs approximate FP contract, and CPU/GPU/Flash-Attention capability.
Never silently transpose on every token. Gate with end-to-end attention including softmax, K/V append,
page lookup, quant/dequant, and conversion cost. Current llama.cpp itself carries a `v_trans` state
and serializes transposed and non-transposed V differently, confirming that a production backend
cannot assume one universal KV layout.

## 4. MoE: grouped experts are conditional, placement is first-class

The useful backend vocabulary is router/top-k plus selected-expert matrix work (`matmul_id` class),
not a generic source sort. A native fallback may build a stable count/prefix route plan, compute each
route result independently by expert, then combine each token's top-k results in original route order.
That preserves deterministic tie/output order and ordered FP combination while allowing contiguous
expert work.

**Route-grouping probe (same host/compiler):** 64 experts, top-2, each expert a 512x256 `f32`
matrix (32 MiB total), explicit AVX2 matvec, route-plan construction included. Warm AB/BA and a cold
96 MiB cache-flush control were repeated twice.

| routing state | batch 1 | batch 8 | batch 32 | batch 128 |
|---|---:|---:|---:|---:|
| uniform, warm | 0.95-1.01x | 1.00-1.02x | 0.97-1.01x | **1.28-1.46x** |
| uniform, cold | 0.92-0.98x | 1.03-1.05x | 1.05-1.15x | **1.39-1.58x** |
| only four hot experts, warm | 1.00-1.04x | 1.01-1.02x | 1.01-1.06x | 1.00-1.03x |
| only four hot experts, cold | 1.00-1.03x | 1.18-1.27x | 1.06-1.07x | 1.03-1.06x |

Grouping is adopted only as a **measured large/diverse-route strategy**. At batch 1 it is noise or a
loss; when four experts are already hot, regrouping normally adds no steady-state value. A simple
dispatcher should use route count, distinct experts, routes per expert, expert bytes, and the target
cache/tier. Do not expose its threshold in source and do not build a general adaptive scheduler.

MoE placement can matter more than grouping: retain per-layer expert-selection counters, distinguish
cold-start and steady distributions, and let the planner/`alignpack` place hot experts contiguously
or in VRAM while keeping the portable full-model mapping as the oracle. Evaluate LFU/windowed-hotness
and admission hysteresis against uniform, Zipf, phase-shifting, and adversarial routes; an LRU chosen
without traces is not a design. Disk paging in the token-critical path remains a last resort. For a
mature backend, call its quantized `matmul_id`/fusion rather than rebuilding expert kernels in Align.

## 5. What to profile in the first real engine

Instrumentation should be part of the engine plan before kernel tuning. Record per request and per
layer, without requiring compiler features:

```text
load       metadata parse, descriptor build, mapped/resident/copied bytes, page faults by tier
prefill    tokens/s, batch/ubatch, attention vs dense/MoE FFN, KV write/conversion bytes
decode     ms/token and useful bytes/token for qmatvec, K score, softmax, V accumulate, FFN
MoE        selected/distinct experts, routes/expert, grouping-plan cost, expert cache hit/miss/evict
KV         live/allocated/shared blocks, layout conversions, copy-on-write, context/page occupancy
service    queue/batch wait, tokenizer, sampling, SSE write, p50/p95/p99 and cancellation waste
```

The first acceptance benchmark is a matrix, not one tokens/s number: dense and MoE models; prompt
prefill vs one-token decode; batch 1/interactive vs continuous batches; short/long context; warm/cold
model; CPU-only, partial offload, and full GPU; uniform/skew/phase-changing expert routes. Report
latency, throughput, peak RSS/VRAM, bytes moved per token, and model/output equivalence.

## 6. Explicit non-priorities

- Do not write general quantized matmul, Flash Attention, CUDA/Metal scheduling, or every MoE fusion
  in Align before the borrowed backend baseline. These are mature, ISA/device-specific kernels.
- Do not add core `f16`/`bf16`, `MemoryTier`, GGUF, expert, or GPU types from these measurements.
- Do not apply `MADV_WILLNEED`, expert grouping, KV transposition, manual prefetch, or parallelism
  unconditionally; every measured result above changes with state.
- Do not optimize GGUF metadata parsing ahead of page placement and weight/KV traffic unless a real
  profile makes it visible. Model files contain thousands of descriptors but billions of weight bytes.

## 7. FFI boundary optimization: adopt selective substrate, not another ABI

The ordinary FFI call path is already the correct baseline: a native LLVM `call`, view-to-data-pointer
extraction, and no marshaling, pinning, GC transition, or stack switch. For model loading and shaped
backend operations (`matmul`, attention, `matmul_id`, command submission), the application/pkg must
first make the boundary coarse enough that the call is negligible. Do not add an Align-specific fast
calling convention or automatically batch calls.

Three generic follow-ups are worth retaining:

1. **P1, consumer-gated — pkg-provided selective bitcode/LTO.** Generalize the existing guarded
   `--rt-lto` plumbing so a pkg may optionally provide a small, self-contained C/Rust/Zig helper
   module as matching LLVM bitcode. This is for a measured fine-grained call that blocks inlining,
   constant propagation, fusion, or vectorization; normal `.a`/`.so` linking remains the default.
   Scope it by an explicit symbol/artifact allowlist, include target/triple/datalayout/toolchain and
   bitcode digests in the build cache key, retain a loud ordinary-object fallback, and preserve the
   current one post-link optimization run. The existing evidence requires selectivity: guarded
   `str_eq`-class runtime bitcode made `eq_count` 2.95x faster with a 1.01x numeric control and about
   2 ms compile-time cost, while the earlier full probe made `str_cmp` regress to 0.72x. Ship a pkg
   artifact only when its real consumer clears the usual roughly 1.15x wall-time gate without an
   unacceptable code-size/compile-time change and with output equivalence pinned. Do not LTO an
   entire ggml/BLAS/CUDA/Metal backend merely because it is available.
2. **P1 diagnostic — expose hot-loop FFI barriers in `explain-opt`.** Attribute an external call to
   its Align source loop and report when it prevents vectorization/fusion, remains at per-element or
   per-block frequency, or blocks invariant-call motion. Suggest one of the existing mechanisms:
   move the loop into a shaped backend operation, pass a slice/batch, or make the small helper
   LTO-visible. This is an optimization explanation, not a blanket warning on every extern call;
   emit it only from an actual missed optimization or measured/hot context.
3. **P1 foundation, first backend consumer — pkg-definable opaque Move resource with one Drop.** A
   long-lived model/context/GPU-buffer handle can be prototyped today as `raw` plus an explicit
   destroy call, but that is not yet an ergonomic safe pkg abstraction. Add one generic resource
   mechanism only when the first real backend wrapper needs it: non-Copy ownership, exactly-once
   destructor at ordinary/early/error scope exits, and resource-tied borrowed views. This is not a
   faster call instruction; it keeps backend ownership and `unsafe` inside one wrapper so the fast
   path can be reused safely. It must not introduce LLM/GPU-specific core types.

**Deferred behind evidence:** an audited foreign-function contract (`memory(...)`, `readonly`,
`writeonly`, `captures(none)`, `noalias`, `nonnull`, `dereferenceable`, `nounwind`, or a Pure effect)
may help an opaque call only when batching and LTO do not. A false contract can miscompile or admit a
race, so do not expose casual per-call annotations. Reopen with a concrete missed-optimization IR and
ABI tests; prefer attributes inferred from a visible body or generated by an audited pkg wrapper.

**Not adopted:** a second FFI ABI, automatic FFI batching, always-on whole-library LTO, a casual
`extern pure`, user-written raw LLVM attributes, or tensor/MoE-specific FFI syntax.

Primary implementation references: llama.cpp's
[`llama-kv-cache.cpp`](https://github.com/ggml-org/llama.cpp/blob/master/src/llama-kv-cache.cpp)
shows layout-dependent K/V state including `v_trans`; ggml's
[`ggml.h`](https://github.com/ggml-org/llama.cpp/blob/master/ggml/include/ggml.h) defines explicit
tensor strides plus `MUL_MAT`/`MUL_MAT_ID`; llama.cpp's
[`common.h`](https://github.com/ggml-org/llama.cpp/blob/master/common/common.h) exposes mmap, mlock,
warmup, cache types, and offload choices as runtime policy rather than language semantics.
