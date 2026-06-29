# Benchmarks

Reproducible head-to-head benchmarks of Align against idiomatic, hand-written Rust. Align's promise
is *predictable performance from declarative data-oriented code*, so the bar is concrete: **on its
strong cases Align must match or beat Rust**.

```sh
bench/run.sh            # native (both sides at the host CPU's best — AVX2 etc.)
bench/run.sh baseline   # the portable floor (x86-64-v2 on amd64)
ALIGN_BENCH_PROFILE=1 bench/json_soa/run.sh native  # optional decomposition output
```

## How it works

- `kernels.align` is a **no-`main` library**; `alignc emit-obj` compiles it to an object (its `pub fn`s
  exported, no linking).
- `harness.rs` links that object and calls each kernel with the **same runtime-generated data** as an
  idiomatic Rust equivalent, then prints the ratio.
- Both `alignc` and `rustc` are pinned to the **same `--target-cpu`**, so the comparison is fair.

## Methodology (don't skip)

- **Runtime data, never literals.** A literal array constant-folds to its result at compile time
  (`[1..16].sum()` → `mov $136, eax`) — there is no loop to measure. Kernels take a `slice`/`soa`
  parameter; the harness fills a `Vec` at runtime (an LCG) so nothing folds.
- **Alternate + minimum.** The two kernels are timed in alternating rounds, keeping the per-kernel
  minimum. Timing *all* of A then *all* of B over a >cache working set produces wildly wrong ratios
  (the second kernel benefits from a warm-ish cache / settled clocks). This trap once made an
  identical-machine-code kernel look "20× slower" — it was a measurement artifact.
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
- **The lever that beats Rust is layout: `soa<T>`.** Idiomatic Rust uses `Vec<Struct>` (array-of-
  structs); a scan that touches a few fields still drags whole cache lines through memory. Align's
  `soa<T>` stores each field in its own column, so a field-subset scan reads only those columns:
  - `col_sum` (`ps.a.sum()`): **≈8–10× faster** than the AoS field sum (pure bandwidth).
  - `total_pay` (`rs.where(.active).pay.sum()`, the filtered aggregate): **≈3× faster** — the `where`
    lowers branchless (mask + `select`) so it vectorizes; otherwise it is branch-bound and only ties.
- **End-to-end JSON→SoA is parse-bound (`bench/json_soa/`); ≈0.61× → ≈0.82× after one parser fix.**
  The column-layout win above is on the *aggregation*; the realistic `json.decode → soa → aggregate`
  pipeline is dominated by the **parse**. Decomposing (Align `→soa` vs Align `→array` AoS vs `serde →
  Vec`) first showed the gap was the parser. Hand-rolling integer parsing
  (`str::from_utf8(..).parse` → a single-pass digit accumulation) moved it ≈0.61× → ≈0.82–0.85×
  (AoS ≈parity at 1M). The latest profile mode shows the aggregate itself is <1 ms at 1M rows, while
  the AoS→SoA materialization still costs ~10–25 ms at that size. Remaining gap → more scalar tuning
  + direct column fill / SIMD structural parsing. See `bench/json_soa/README.md` +
  `docs/open-questions.md`.
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
- **String building (`bench/string_builder/`): the `builder` reduce-append pattern ties/beats naive
  Rust and is ≈1.5× behind hand-optimized Rust.** Hand-rolling the runtime integer write (`write!` →
  itoa) halved the gap (Gemini measured the old builder ~2.8× behind optimized). The residual is
  **per-append FFI-call overhead** (3 runtime calls/element, not inlined) — measured, *not* the `Vec`
  realloc: adding `builder(capacity)` did **not** close it (`+cap` ≈ `build`). Profile mode confirms
  static writes and integer writes are both material costs; a runtime batch probe for
  `literal + int + literal` cuts the 100k workload from ~1.48 ms to ~0.95–0.99 ms. The lever is
  inlining/batching the appends. This is the string-accumulation tool the `str + str`-in-a-lambda
  error points to.
- **Data-parallel map (`bench/par_map/`): the persistent worker pool removed the spawn regression
  (100k went ~7× slower → same order as sequential).** Old `par_map` spawned OS threads per call; now
  it submits chunks to a process-lifetime pool. Chunk tuning helps, but profile mode shows cheap
  arithmetic still loses to Align's own sequential/vectorized `map().sum()` because every element
  crosses an indirect `thunk` call. Use `par_map` for heavier/non-vectorizable work; cheap maps need
  sequential fallback or thunk specialization.
