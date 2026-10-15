# Benchmarks

Benchmarks of Align against idiomatic hand-written Rust or equal-LLVM C controls.
They measure concrete implementations and workloads; a recorded ratio is not a
language guarantee. The goal is competitive performance from declarative
data-oriented code, with the input, target, layout and measurement limits visible.

```sh
bench/run.sh            # native (both sides target the host CPU)
bench/run.sh baseline   # the portable floor (x86-64-v2 on amd64)
work_dir="$(mktemp -d)"
prepare_output="$(ALIGN_BENCH_WORK_DIR="$work_dir" bench/json_soa/run.sh prepare native)"
digest="$(printf '%s\n' "$prepare_output" | sed -n 's/^artifact-manifest-sha256: //p')"
ALIGN_BENCH_WORK_DIR="$work_dir" ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256="$digest" \
  bench/json_soa/run.sh native  # fixed evidence rows from the sealed artifacts
bench/deep_pipeline/run.sh native  # stage-depth scaling: 1/2/4/8/16/32
bench/frame_join/run.sh       # pkg.frame i64/str hash-join implementation-class probe
bench/task_group/run.sh  # task-group split vs packed-record/cache-line probe
bench/function_incremental/run.sh  # item-6 edit/cold/RSS/size controls
```

The JSON benchmarks require `ALIGN_BENCH_WORK_DIR`: an existing, empty absolute directory outside
the repository. Preparation retains sealed artifacts for one or more direct native invocations.
After the benchmark process (or its containing development environment) has terminated, the caller
removes that work directory; script-level failure leaves only a directory skeleton below its owned
child so candidate-side cleanup never removes a raced directory entry.

## How it works

- `kernels.align` is a **no-`main` library**; `alignc emit-obj` compiles it to an object (no linking).
- This no-`main` object names its C-ABI surface explicitly with **`emit-obj --export <name>`**
  (repeatable), one flag per kernel the harness calls: `--export` is an object-level C-ABI boundary,
  **separate from Align's `pub` visibility** — `pub` is a *source-level module* visibility resolved
  entirely inside the compiler, while `--export` controls *linker* visibility; a non-`pub` function
  can be exported too. Un-exported functions (even `pub` ones) may be inlined or dead-code-eliminated
  entirely, since `external` linkage doubles as the DCE root set.
- `harness.rs` links that object and calls each exported kernel with the **same runtime-generated
  data** as an idiomatic Rust equivalent, then prints the ratio.
- Both `alignc` and `rustc` use the **same CPU target**. Also record compiler/LLVM
  versions, optimization profiles, floating-point semantics and LTO settings;
  matching the CPU alone does not control those differences.

## Methodology (don't skip)

- **Use runtime input.** A literal array can constant-fold to its result at compile time,
  leaving no loop to measure. Kernels take a `slice`/`soa` parameter; the harness fills a `Vec`
  at runtime (an LCG) and keeps the result observable.
- **Balanced order + an explicit statistic.** The primary `harness.rs` currently runs fixed
  Align-then-Rust pairs and keeps each arm's minimum; it does **not** counterbalance AB/BA.
  Treat its small timing differences cautiously. The depth sweep balances AB/BA order and
  keeps the median. Timing all of A
  then all of B over a working set larger than cache can bias the comparison
  through cache warmth or clock changes. This once made an
  identical-machine-code kernel look "20× slower" — it was a measurement artifact.
- **A difference of two large measurements carries both their noises.** `web_e2e` reports "Align's
  protocol path above the floor" by subtracting two ~70 µs end-to-end numbers, each stable to ~1% —
  so the 4 µs difference is stable to ~35% (measured: 3.3 / 3.9 / 4.8 on adjacent runs). Anything
  smaller than that spread cannot be priced there however many times you run it. When the quantity
  you care about is a small difference, **measure it directly**: `bench/http_path` prices the same
  path in-process on an exact allocation count plus the server thread's own CPU time (~2% run to
  run). The client-path comparison had the same limitation: `bench/http_client` reported
  ~65 µs/req end to end, which could not resolve the ~0.6 µs component in question.
  `bench/http_client_path` measures that path in-process (~1.1% run to run).
- **A floor must do the same syscalls, or its difference is not the thing you named.** `http_path`'s
  first version compared Align (`poll` → `read` → `write`) against a floor that only read and wrote,
  and called the difference Align's CPU cost. **The missing `poll` was ~0.9 µs of the 4.3 µs** — 21%
  of the headline, attributed to work that does not exist. It now runs both floors and reports both
  differences.
- **Iterations do not cure drift.** More iterations shrink *within-run* noise only. On this box an
  unchanged binary read 4201 ns in one batch and 4682 ten minutes later — a between-run shift larger
  than its own σ. The fix is structural: **alternate the arms in blocks inside one process and take
  the median**, so drift hits every arm alike. (Three samples suggested ±1.3%; eleven showed 3.5%.
  Record the spread together with its sample size.)
- **Alternating is not enough — counterbalance the order.** Even interleaved, the *position* in the
  cycle carries a bias: three **identical** floors in `http_path`'s three slots read slot 2 ~115 ns
  above slot 1 and slot 3 ~109 ns below it, systematically, and it does not shrink with more blocks.
  That was 11% of one reported line. Reverse the order on alternate blocks (`A,B,C` then `C,B,A`) so
  every arm sees every slot equally often — this is what "balanced order" above actually requires.
- **A minimum estimates a best observed time, not typical latency.** Use a median and spread for
  typical behavior, and label whichever statistic a harness reports. Taking
  each arm's min picks two uncorrelated low outliers and *adds* their noise, and discards exactly the
  correlated drift interleaving exists to cancel. Measured on the same data: median σ 88/173/99 ns vs
  min σ 146/214/127.
- **Measure the components of an unexpected result.** If a change is flat or slower,
  build an absolute-ms breakdown that
  starts from the fast variant and adds one realistic cost at a time (stage-1 alone → + materialize →
  + the real key matching → …), each measured. The delta that jumps is the bottleneck. This pinned the
  JSON two-stage decode's real cost to per-field key matching (`find_field`), *not* the materialization
  a first guess had blamed — buf-resize + final copy added only +1.6 ms (`docs/open-questions.md`
  "JSON two-stage SIMD decode"). Use the measured component cost to choose the next change.

## What we've learned

These are historical findings from the linked harness records, with their
original machines, inputs and compiler revisions. They are not fresh results
for the current checkout. Re-run the relevant harness before using a number in
a performance claim. Compare equal layouts as well when the question is
compiler code quality rather than the benefit of changing a data representation.

- **Flat scalar pipelines (`where`/`map`/`reduce` over `slice<T>`): recorded parity with Rust.**
  The compared kernels produced the same LLVM IR and machine code, including a branchless
  `where` (`pcmpgtq` + `pand` mask). This explains the similar timings in that comparison.
- **Deep pipeline scaling (`bench/deep_pipeline/`) has structural regression coverage.** The shared
  4-family × 6-depth fixture pins one fused loop, no intermediate allocation, stage inlining, legal
  vectorization, and compilation on a 2 MiB stack; the head-to-head harness separately measures
  throughput against same-target, equal-LLVM O2 C controls.
- **The recorded SoA-versus-AoS comparison measures a layout change.** This Rust control uses `Vec<Struct>` (array-of-
  structs); a scan of a few fields can load cache lines containing unused fields. Align's
  `soa<T>` stores each field in its own column, so a field-subset scan reads only those columns:
  - `col_sum` (`ps.a.sum()`): **≈8–10× faster** than the AoS field sum (pure bandwidth).
  - `total_pay` (`rs.where(.active).pay.sum()`, the filtered aggregate): **≈3× faster** — the `where`
  lowers branchless (mask + `select`) so it vectorizes in the measured case.
  Rust can also store columns; this comparison measures the chosen layouts, not an inherent
  limit of Rust or a universal Align speedup.
- **Recorded end-to-end JSON→SoA throughput (`bench/json_soa/`): ≈0.61× → ≈0.82× → ≈1.03× of `serde_json` at 1M rows.**
  The column-layout result above measures aggregation; the `json.decode → soa → aggregate`
  pipeline is dominated by the **parse**. Hand-rolling integer parsing first moved it ≈0.61× →
  ≈0.82–0.85×. Then **direct SoA decode** (`align_rt_json_decode_soa`: count rows → arena-allocate
  columns → fill them in one value-parse pass, no AoS intermediate / heap copy / transpose) removed
  the 10–25 ms AoS→SoA materialization the profile mode had isolated, taking the SoA path ≈0.82× →
  **≈1.03×** in that measurement. See `bench/json_soa/README.md` for the later projection
  measurements and their parsing costs.
- **JSON decode-throughput tracker (`bench/json_decode/`):** the historical recursive-descent
  baseline ≈ties `serde_json` (full ≈1.03×, projecting ≈1.09×). The implemented indexed/Mison
  path's recorded projection result is **≈1.16–1.65×**, with full decode around parity.
  The **~3.4–4.1×** experimental positional-walk probe omits work paid by the general decoder;
  it is neither the shipped decoder's result nor a promised future speedup. The linked record
  explains that distinction and the current sealed prepare/direct-native measurement interface.
- **Library-boundary prerequisites (`bench/library_boundary/`):** the cumulative L2a–L7 regression
  harness named by `docs/impl/17-library-boundary-prerequisites.md`. L2a's `interface` group records
  canonical interface bytes and checked decode throughput; later slices add provenance, cleanup ABI,
  borrow-call, resource, region, static-artifact, and builder rows without replacing earlier gates.
- **Grouped aggregation (`bench/group_by/`): the recorded dense-key cases ran ≈5–6× faster than
  the `std::HashMap` control and ≈2–3× faster than the `ahash` control.** `s.group_by(.k).sum(.v)`
  takes a dense-id direct-index path when the key range is tight (`acc[key - min]`, no hashing), which
  is the shape used by the benchmark. The older open-addressing hash path still backs sparse /
  wide-range keys, which need a separate comparison. The benchmark caught
  the original table-sizing bug and measured the direct-index improvement.
- **String building (`bench/string_builder/`): the `builder` reduce-append pattern was faster than
  the basic Rust control and ≈1.4× slower than the optimized control (previously ≈1.5×).** Replacing the runtime integer write
  (`write!` → itoa) halved the original gap (Gemini measured the old builder ~2.8× behind optimized).
  The residual was **per-append FFI-call overhead** (3 runtime calls/element) — measured, *not* the
  `Vec` realloc: adding `builder(capacity)` did **not** close it (`+cap` ≈ `build`). The compiler now
  **batch-lowers** `literal + int + literal` to one `align_rt_builder_write_str_int_str` call
  (`fuse_builder_writes` MIR peephole), which moved generated `build` ~1.65 → ~1.30 ms at 100k
  (≈21%, same-host before/after), within ~0.19 ms of the direct batch probe. The recorded follow-up
  is a general builder-chain batcher for shapes beyond `str,int,str`. This is the string-accumulation
  tool the `str + str`-in-a-lambda error points to.
- **Data-parallel map (`bench/par_map/`): the persistent worker pool removed the spawn regression
  (100k went ~7× slower → same order as sequential).** The current whole-range kernel emits one
  typed loop per claimed range, so direct bodies can inline and vectorize; the remaining cost is
  range scheduling and, for materializing forms, intermediate output. A directly consumed
  supported integer sum uses range partials instead. The threshold probe keeps the body-agnostic
  caller-only floor at 65,536 elements on the current host. Cheap maps can still lose to the
  sequential fused loop; measure before choosing explicit parallelism.
- **Adaptive stable sort (`bench/adaptive_sort/`): recorded improvements on ordered input
  (already-sorted 3.6×, tail-swap/1%-swap 1.14–1.17×), with merge-heavy controls within ≈2%.**
  On WSL2 without CPU-frequency control, a block-sequential AB comparison of two
  differently-sized kernels was affected by ±25% frequency drift between the after-block and
  before-block, and state-dependent cross-kernel i-cache/position bias (up to 10% for fast sorts). The
  fix — used here as the standard for two-build comparisons in one binary — is **median of per-pair
  adjacent ratios** (after and before measured back-to-back share the instantaneous frequency) plus an
  **identical-code control** (`after` vs a second copy of `after` under different symbols; its
  deviation from 1.00 is pure bias, and `corrected = real/control` removes it). That control caught a
  throughput-neutral refinement being misread as a 10% regression, and an isolation sweep (each
  refinement toggled independently from one compiler via an env knob + baseline) localized a real 7%
  regression to a single pass-1 check, fixed by gating it to wider passes. Read the `corrected`
  column, pin with `taskset -c`; see `bench/adaptive_sort/README.md`.

## Choose a measurement for the changed work

| Question | Focused harnesses |
| --- | --- |
| Fused loops, layout and stage depth | [base kernels](kernels.align), [deep pipelines](deep_pipeline/README.md) |
| Parallel setup, grain size and task composition | [parallel map](par_map/README.md), [task groups](task_group/README.md) |
| Decode, text conversion and output allocation | [JSON decode](json_decode/README.md), [JSON SoA](json_soa/README.md), [encoding writes](encoding_writes/README.md), [lossy UTF-8](utf8_lossy/README.md) |
| Grouping, sorting and repeated analytics | [grouping](group_by/README.md), [group reuse](group_by_reuse/README.md), [adaptive sort](adaptive_sort/README.md) |
| Compiler startup, edits and link/optimization cost | [startup](startup/README.md), [build pipeline](build_pipeline/README.md), [function incremental](function_incremental/README.md), [runtime LTO](rt_lto/README.md) |

Read the selected harness's preparation, input and cleanup contract first.
Measure the affected path and retain its negative controls; running every
benchmark does not make an unrelated performance claim stronger. For application
work, use the English/Japanese [performance guide](../docs/guide/26-performance.md)
to connect those measurements to storage reuse, numerical semantics and end-to-end cost.
