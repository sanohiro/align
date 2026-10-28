# Scalar builder matched qualification

The owner selected R100's same-source comparison on 2026-10-28. Plan70's
guarded scalar push is already shipped. The missing authenticated historical
Mac tokenizer baseline cannot establish a comparative result; this qualification
uses an explicit generic-path control on the current compiler and unchanged
consumer source instead. It makes no language, runtime, ABI or optimizer change.

## Measurement boundary

| Obligation | Evidence |
| --- | --- |
| Controlled intervention | Build two compiler artifacts from the same production base. The private control only forces the scalar push fast-path predicate false; every push uses its existing slow branch. Preserve the exact patch and binary hashes. This is not a recovered historical compiler. |
| Scalar construction | `bench/array_builder` compares existing compiler artifacts on reserved and growing i64 builders, validates complete array contents and length before returning a checksum, and checks independent C expectations. |
| Native timing | Both arms link the same production release runtime with allocation instrumentation absent. Seven alternating pairs use the same baseline CPU target and source. Report per-case medians; correctness has no speed threshold. |
| Real tokenizer | A provider-owned caller imports an authenticated consumer snapshot and loads the retained Qwen2.5 vocabulary. Match every token ID across arms and repeated calls, then time the same workload. Preserve all input/output and source hashes. |
| Attribution | Profile tokenization separately with Callgrind, after loading the vocabulary. Report executed instructions and push/reserve calls, not CPU-sample percentages or the historical Mac 14.1% denominator. |
| Lifecycle | Reuse `bench/native_probe.py` for public build/link/run process groups, deadlines, signals, diagnostics and exclusive scratch cleanup. Existing actual-process supervisor owners retain their authority. |
| Limits | No original Mac-baseline claim, new optimization, whole-product throughput claim, consumer code change or adoption state transition. |

The public fixture is independently authored. Private consumer sources, model
data and raw profiles remain outside public Git. Existing `m12_array_builder`
owners retain scalar-family representation, growth, arena and ownership proof.

## Result (2026-10-28)

Production base `cf8d5495c88e34d2bb6833ecb45b12afcacf4f37`, LLVM22,
Linux x86_64 / WSL2 / Ryzen 5950X, pinned to CPU 2. The control applies only
[`generic-control.patch`](../../bench/array_builder/generic-control.patch) in a
disposable checkout. Both artifacts retain the same runtime, source, release
profile, baseline target and disabled rt-LTO. There is no production compiler
change in this qualification.

Seven alternating pairs, medians in milliseconds:

| Workload | Generic control | Shipped fast path | Control / shipped |
| --- | ---: | ---: | ---: |
| 300 growing arrays of 100,000 i64 values | 170.842 | 29.779 | 5.74x |
| 300 exactly reserved arrays of 100,000 i64 values | 170.552 | 29.323 | 5.82x |
| 120 retained tokenizer calls, 19,220 IDs each | 1313.275 | 784.813 | 1.67x |

The tokenizer source is consumer `9e3eb7284dbb21657f22c9debb943d93e112edd3`.
Its Qwen2.5-Coder-7B vocabulary is loaded once, followed by one reference call
outside measurement. Every measured call compares every returned ID against
that reference; all 14 complete reference outputs agree across arms. The
58,200-byte input repeats the following stanza 600 times, with a trailing
newline on each line:

```text
def update(items): return [x * 17 + 3 for x in items]
日本語の文章、café and emoji 🚀.
```

`prepare_retained` receives system text `You are a helpful coding assistant.`;
the rendered prompt is 58,315 bytes. Timing includes tokenization, complete-ID
comparison and per-call cleanup, but excludes vocabulary loading, reference
creation and output. This is a selected new workload, not the unavailable
historical 19,219-token Mac workload.

Separate Callgrind 3.24.0 runs instrument five calls after loading and reference
creation; both reproduce the same complete token output:

| Dynamic observation | Generic control | Shipped fast path |
| --- | ---: | ---: |
| Executed instructions | 826,733,610 | 508,332,490 |
| Generic scalar push calls | 4,973,560 | 375 |
| `ArrayBuilder::reserve` calls | 5,045,635 | 72,450 |
| Push + reserve exclusive instructions | 300,734,170 | 2,343,070 |
| Share of measured instructions | 36.376% | 0.461% |

These are instruction counts, not elapsed-time shares or CPU sample shares.
Remaining growth and non-scalar paths are expected. Ordinary native timings
run without the profiler or allocation instrumentation. The runner rejects an
instrumented archive before compilation; that negative control passes.

[`results-linux.csv`](../../bench/array_builder/results-linux.csv) retains every
array timing; [`results-linux.json`](../../bench/array_builder/results-linux.json)
binds compiler/runtime/model/source/caller/input/output hashes, every tokenizer
timing and the profile counts. Complete private replay sources and commands
remain in the provider's `r100-20261028` evidence archive.

Provider disposition: R100's selected same-source comparative qualification is
complete. The shipped fast path reduces work at identical output on this host.
The missing historical Mac baseline remains an attribution limit, not pending
provider implementation. This evidence does not change consumer adoption status.
