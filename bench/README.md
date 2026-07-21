# Benchmarks

Reproducible head-to-head benchmarks of Align against idiomatic hand-written Rust or equal-LLVM C
controls. Align's promise is *predictable performance from declarative data-oriented code*, so the
bar is concrete: **on its strong cases Align must match or beat the control**.

```sh
bench/run.sh            # native (both sides at the host CPU's best — AVX2 etc.)
bench/run.sh baseline   # the portable floor (x86-64-v2 on amd64)
ALIGN_BENCH_PROFILE=1 bench/json_soa/run.sh native  # optional decomposition output
bench/deep_pipeline/run.sh native  # stage-depth scaling: 1/2/4/8/16/32
```

## How it works

- `kernels.align` is a **no-`main` library**; `alignc emit-obj` compiles it to an object (no linking).
- Every Align program function is `internal` by default (M13 Slice 1) — a whole-program build has no
  separate compilation, so nothing needs external linkage except the C entry `main`. A no-`main`
  object like this one names its C-ABI surface explicitly with **`emit-obj --export <name>`**
  (repeatable), one flag per kernel the harness calls: `--export` is an object-level C-ABI boundary,
  **separate from Align's `pub` visibility** — `pub` is a *source-level module* visibility resolved
  entirely inside the compiler, while `--export` controls *linker* visibility; a non-`pub` function
  can be exported too. Un-exported functions (even `pub` ones) may be inlined or dead-code-eliminated
  entirely, since `external` linkage doubles as the DCE root set.
- `harness.rs` links that object and calls each exported kernel with the **same runtime-generated
  data** as an idiomatic Rust equivalent, then prints the ratio.
- Both `alignc` and `rustc` are pinned to the **same `--target-cpu`**, so the comparison is fair.

## Methodology (don't skip)

- **Runtime data, never literals.** A literal array constant-folds to its result at compile time
  (`[1..16].sum()` → `mov $136, eax`) — there is no loop to measure. Kernels take a `slice`/`soa`
  parameter; the harness fills a `Vec` at runtime (an LCG) so nothing folds.
- **Balanced order + an explicit statistic.** The primary harness alternates and keeps the
  per-kernel minimum; the depth sweep balances AB/BA order and keeps the median. Timing *all* of A
  then *all* of B over a >cache working set produces wildly wrong ratios
  (the second kernel benefits from a warm-ish cache / settled clocks). This trap once made an
  identical-machine-code kernel look "20× slower" — it was a measurement artifact.
- **A difference of two large measurements carries both their noises.** `web_e2e` reports "Align's
  protocol path above the floor" by subtracting two ~70 µs end-to-end numbers, each stable to ~1% —
  so the 4 µs difference is stable to ~35% (measured: 3.3 / 3.9 / 4.8 on adjacent runs). Anything
  smaller than that spread cannot be priced there however many times you run it. When the quantity
  you care about is a small difference, **measure it directly**: `bench/http_path` prices the same
  path in-process on an exact allocation count and the server thread's own CPU time, gets ±1.3%, and
  agrees with `web_e2e`'s absolute number — which is what makes both trustworthy.
- **When the result disappoints, autopsy — don't guess.** If a mechanism that *should* win comes back
  flat or slower, do not reason about the cause from intuition: build an **absolute-ms breakdown** that
  starts from the fast variant and adds one realistic cost at a time (stage-1 alone → + materialize →
  + the real key matching → …), each measured. The delta that jumps is the bottleneck. This pinned the
  JSON two-stage decode's real cost to per-field key matching (`find_field`), *not* the materialization
  a first guess had blamed — buf-resize + final copy added only +1.6 ms (`docs/open-questions.md`
  "JSON two-stage SIMD decode"). A wrong guess sends the next slice optimizing the wrong thing; the
  autopsy is cheap insurance against that.

## What we've learned

- **Flat scalar pipelines (`where`/`map`/`reduce` over `slice<T>`): Align ≈ Rust.** They lower to the
  *same* LLVM IR and the *same* machine code (the `where` is branchless — `pcmpgtq` + `pand` mask —
  for both). So Align matches hand-tuned Rust automatically, from shorter code; it can't *beat* it
  here (shared LLVM backend → identical code is the ceiling).
- **Deep pipeline scaling (`bench/deep_pipeline/`) is a first-class regression gate.** The shared
  4-family × 6-depth fixture pins one fused loop, no intermediate allocation, stage inlining, legal
  vectorization, and compilation on a 2 MiB stack; the head-to-head harness separately measures
  throughput against same-target, equal-LLVM O2 C controls.
- **The lever that beats Rust is layout: `soa<T>`.** Idiomatic Rust uses `Vec<Struct>` (array-of-
  structs); a scan that touches a few fields still drags whole cache lines through memory. Align's
  `soa<T>` stores each field in its own column, so a field-subset scan reads only those columns:
  - `col_sum` (`ps.a.sum()`): **≈8–10× faster** than the AoS field sum (pure bandwidth).
  - `total_pay` (`rs.where(.active).pay.sum()`, the filtered aggregate): **≈3× faster** — the `where`
    lowers branchless (mask + `select`) so it vectorizes; otherwise it is branch-bound and only ties.
- **End-to-end JSON→SoA now beats `serde_json` (`bench/json_soa/`); ≈0.61× → ≈0.82× → ≈1.03× at 1M.**
  The column-layout win above is on the *aggregation*; the realistic `json.decode → soa → aggregate`
  pipeline is dominated by the **parse**. Hand-rolling integer parsing first moved it ≈0.61× →
  ≈0.82–0.85×. Then **direct SoA decode** (`align_rt_json_decode_soa`: count rows → arena-allocate
  columns → fill them in one value-parse pass, no AoS intermediate / heap copy / transpose) removed
  the 10–25 ms AoS→SoA materialization the profile mode had isolated, taking the SoA path ≈0.82× →
  **≈1.03×** (it now even edges the AoS decode-only path, which still heap-materializes). Remaining
  gap → SIMD structural parsing. See `bench/json_soa/README.md` + `docs/open-questions.md`.
- **JSON decode-throughput tracker (`bench/json_decode/`):** the regression harness for the parser
  rewrite (recursive-descent → simdjson-style two-stage SIMD). The recursive-descent baseline ≈ties
  `serde_json` (full ≈1.03×, projecting ≈1.09×); a validated `work/` probe (SIMD structural index +
  projecting two-stage) reaches **~3.4–4.1×** over `serde_json` (~3.2–3.9× into soa columns). The
  rewrite lands that here — watch the `align/serde` ratios climb per slice.
- **Grouped aggregation (`bench/group_by/`): Align beats the *default* `std::HashMap` everywhere
  (≈5–6×) and beats `ahash` on dense integer-key analytics (≈2–3×).** `s.group_by(.k).sum(.v)` now
  takes a dense-id direct-index path when the key range is tight (`acc[key - min]`, no hashing), which
  is the shape used by the benchmark. The older open-addressing hash path still backs sparse /
  wide-range keys; beating `ahash` there still wants a SwissTable-style layout. The benchmark caught
  both the original table-sizing bug and the denser direct-index win.
- **String building (`bench/string_builder/`): the `builder` reduce-append pattern beats naive Rust
  and is ≈1.4× behind hand-optimized Rust (was ≈1.5×).** Hand-rolling the runtime integer write
  (`write!` → itoa) halved the original gap (Gemini measured the old builder ~2.8× behind optimized).
  The residual was **per-append FFI-call overhead** (3 runtime calls/element) — measured, *not* the
  `Vec` realloc: adding `builder(capacity)` did **not** close it (`+cap` ≈ `build`). The compiler now
  **batch-lowers** `literal + int + literal` to one `align_rt_builder_write_str_int_str` call
  (`fuse_builder_writes` MIR peephole), which moved generated `build` ~1.65 → ~1.30 ms at 100k
  (≈21%, same-host before/after), within ~0.19 ms of the direct batch probe. The recorded follow-up
  is a general builder-chain batcher for shapes beyond `str,int,str`. This is the string-accumulation
  tool the `str + str`-in-a-lambda error points to.
- **Data-parallel map (`bench/par_map/`): the persistent worker pool removed the spawn regression
  (100k went ~7× slower → same order as sequential).** Old `par_map` spawned OS threads per call; now
  it submits chunks to a process-lifetime pool. Chunk tuning helps, but profile mode shows cheap
  arithmetic still loses to Align's own sequential/vectorized `map().sum()` because every element
  crosses an indirect `thunk` call. Use `par_map` for heavier/non-vectorizable work; cheap maps need
  sequential fallback or thunk specialization.
- **Adaptive stable sort (`bench/adaptive_sort/`): SHIPPED — ordered-input wins (already-sorted 3.6×,
  tail-swap/1%-swap 1.14–1.17×) with merge-heavy negatives within ≈2%.** Getting there was a
  measurement lesson. On WSL2 (no CPU-frequency control) a naive block-sequential AB comparison of two
  differently-sized kernels is doubly corrupted: ±25% frequency drift *between* the after-block and
  before-block, and state-dependent cross-kernel i-cache/position bias (up to 10% for fast sorts). The
  fix — used here as the standard for two-build comparisons in one binary — is **median of per-pair
  adjacent ratios** (after and before measured back-to-back share the instantaneous frequency) plus an
  **identical-code control** (`after` vs a second copy of `after` under different symbols; its
  deviation from 1.00 is pure bias, and `corrected = real/control` removes it). That control caught a
  throughput-neutral refinement being misread as a 10% regression, and an isolation sweep (each
  refinement toggled independently from one compiler via an env knob + baseline) localized a real 7%
  regression to a single pass-1 check, fixed by gating it to wider passes. Read the `corrected`
  column, pin with `taskset -c`; see `bench/adaptive_sort/README.md`.
