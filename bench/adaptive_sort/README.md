# `adaptive_sort` — total-order stable-sort fast-path probe (doc-12 §4.1)

Measures the three adaptive refinements added to `lower_array_sort` for **total-order** keys:

1. **whole-input ordered early exit** — return the input untouched (no ping buffer) when it is already
   sorted;
2. **ordered run-boundary straight-copy** — copy an adjacent run pair straight into `tmp` (no
   comparison merge) when its boundary is already ordered, or the right run is empty;
3. **delayed merge-only scratch** — allocate the `tmp` (and keyed `ktmp`) ping buffers only behind a
   `len > 32` gate, so a `len <= 32` sort allocates only the materialize buffer(s).

```sh
bench/adaptive_sort/run.sh [baseline|v3|native]   # default native
```

`run.sh` compiles `kernel.align` with the **current** (post-change) `alignc` for the `after` symbols
and, after sed-renaming the exports with a `_before` suffix, with a **main-worktree** `alignc` for the
`before` symbols, links both into one harness, and links the runtime cdylib built with
`--features alloc-count`. Each kernel borrows its input as a `slice` (so the harness keeps ownership
and threads the same input across rounds); `.sort()`/`.sort_by_key()` **materializes a working copy**
and sorts it (the "copy the fixed source into a working array, then sort" methodology), then a terminal
reduction consumes the result so the sort is load-bearing. All inputs are LCG-generated at runtime.
This is a **manual** probe, not a CI assertion.

## Methodology note — use the SEQUENTIAL table for the no-regression gate

The harness prints two throughput views:

- an **in-process AB/BA** table (interleaved after/before calls) — convenient but **biased**: the two
  kernels differ in code size, so interleaving pollutes each other's i-cache/branch history and
  inflates the apparent cost of the (larger) `after` kernel by several percent;
- a **SEQUENTIAL** table (all `after` samples, then all `before`, min of block-medians over reps) —
  the trustworthy comparison. A control run where **both** symbols are the identical `after` code
  measures 1.00–1.01× under the sequential method, confirming it is unbiased.

Use the sequential table for the negative-workload no-regression judgement.

## Result (2026-07-16, AMD Ryzen 9 5950X, **WSL2** — no CPU-frequency isolation, `taskset -c` pinned)

### Short-size scratch matrix — `align_rt_alloc` calls per sort (delayed-scratch win, refinement 3)

| kernel | n | after allocs | before allocs |
|---|---:|---:|---:|
| `sort_u64` (plain) | 2 / 8 / 16 / 32 | **1** | 2 |
| `sort_by_key_u64` (keyed) | 2 / 8 / 16 / 32 | **2** | 4 |

A `len <= 32` sort allocates only the materialize buffer (plain) / materialize + `keys` (keyed) — the
`tmp`/`ktmp` ping buffers are gone. Frees match allocs (no leak/double-free).

### Throughput — `before/after` speedup (SEQUENTIAL table; >1 = after faster)

`sort_u64` (plain):

| state | 1,024 | 100,000 | 1,000,000 |
|---|---:|---:|---:|
| already sorted | 4.2x | 3.8–4.2x | 3.6–4.2x |
| tail swap | 1.40x | 1.13x | 1.14x |
| 1% adjacent swaps | 1.7–1.9x | 1.16–1.18x | 1.12–1.18x |
| random | 0.99x | 0.96–0.97x | 0.96x |
| reverse | 0.96x | 0.94–0.95x | 0.94x |
| 16-value cardinality | 0.96x | 0.99x | 0.99x |

`sort_by_key_u64` (identity key): already-sorted 5.7–15.9x, tail-swap/1% 1.15–1.18x, negatives
0.93–1.04x. `sort_str` (byte-lex key): already-sorted 11.4x, random/reverse 1.00–1.01x.

## Gate status — NOT a clean pass on WSL2 (reverse/random exceed the 3% no-regression gate)

The doc-12 §4.1 candidate (bare-metal 5950X, pinned core, frequency-controlled) measured the negative
workloads at **0.97–1.03x** (within the 3% no-regression gate). This WSL2 reproduction measures:

- **reverse ~0.94–0.96x** (≈ 4–6% regression) — clearly outside the 3% gate;
- **random ~0.96–0.99x** (≈ 1–4%) — borderline;
- **16-value cardinality ~0.96–0.99x** — borderline / within.

The cost is **inherent** to refinement (2): the ordered run-boundary check runs once per merged run
pair, and pass 1 (`width = 32`) has the most run pairs relative to the work per pair, so a merge-heavy
input (reverse/random) pays ≈ 3–6% of added outer-loop work it cannot amortize. The hot merge and
copyback loops are byte-for-byte identically vectorized before/after (optimized-IR diff: same 22 ×
`<4 x i64>`), so this is not an optimization regression — it is the price of the tail-swap / 1% /
already-sorted wins, which share the same pass-1 boundary check.

**Do not ship on this measurement alone.** Confirm on an isolated bare-metal core (matching the doc's
setup) before merging, or reduce the per-run-pair boundary-check cost. The correctness of the feature
is fully covered by `crates/align_driver/tests/sort_adaptive.rs` regardless of the throughput verdict.
