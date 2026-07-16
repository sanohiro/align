# `adaptive_sort` — total-order stable-sort fast-path probe (doc-12 §4.1, SHIPPED `w64`)

Measures the three adaptive refinements added to `lower_array_sort` for **total-order** keys:

1. **whole-input ordered early exit** — return the input untouched (no ping buffer) when it is already
   sorted;
2. **ordered run-boundary straight-copy** — copy an adjacent run pair straight into `tmp` (no
   comparison merge) when its boundary is already ordered, or the right run is empty — **applied only
   from pass 2** (`width >= 2 * SORT_INSERTION_THRESHOLD == 64`; the narrow pass-1 check was measured
   net-negative);
3. **delayed merge-only scratch** — allocate the `tmp` (and keyed `ktmp`) ping buffers only behind a
   `len > 32` gate, so a `len <= 32` sort allocates only the materialize buffer(s).

```sh
bench/adaptive_sort/run.sh [baseline|v3|native]   # default native
```

`run.sh` compiles `kernel.align` **three times with the current alignc**: `after` = shipped shape,
`before` = `ALIGN_SORT_ADAPTIVE=off` (the pre-change baseline, from the same compiler — so before/after
differ only in the sort shape, never in the compiler build), and `ctrl` = the shipped shape again under
a third symbol set. It links all three + the runtime cdylib built with `--features alloc-count`.

## Methodology — drift-immune ratios + an identical-code control (this environment needs it)

WSL2 has no CPU-frequency control. Two measurement traps, both fixed here:

- **Between-block frequency drift.** Timing all `after` samples, then all `before`, lets the CPU
  frequency drift ±25 % between the two blocks and corrupts the ratio. Fix: measure `after` and the
  other kernel **adjacent** (same instantaneous frequency) and take the **median of per-pair ratios**.
- **Cross-kernel i-cache/position bias.** Two differently-sized kernels in one process pollute each
  other's i-cache/branch history. Fix: a **control** measures `after` vs `ctrl` (identical shipped
  code, different symbols); its deviation from 1.00 is pure bias, and the reported **`corrected` =
  real / control** removes it. This bias is state-dependent and can reach 10 % for fast sorts — it is
  what made a naive block-sequential run misread the throughput-neutral delayed-scratch refinement as
  a 10 % regression. Always read the `corrected` column; run pinned (`taskset -c`).

## Result (2026-07-16, AMD Ryzen 9 5950X, WSL2, `taskset -c` pinned)

### Short-size scratch matrix — `align_rt_alloc` calls per sort (refinement 3)

| kernel | n | after allocs | before allocs |
|---|---:|---:|---:|
| `sort_u64` (plain) | 2 / 8 / 16 / 32 | **1** | 2 |
| `sort_by_key_u64` (keyed) | 2 / 8 / 16 / 32 | **2** | 4 |

A `len <= 32` sort allocates only the materialize buffer (plain) / materialize + `keys` (keyed).

### Throughput — `corrected` (drift-immune, bias-removed) `before/after`, > 1 = shipped faster

`sort_u64` (plain):

| state | 100,000 | 1,000,000 |
|---|---:|---:|
| already sorted | 3.63x | 3.61x |
| tail swap | 1.15x | 1.14x |
| 1% adjacent swaps | 1.17x | 1.14x |
| random | 1.00x | 1.00x |
| reverse | 0.98x | 0.99x |
| 16-value cardinality | 0.99x | 1.00x |

`sort_by_key_u64` (identity key): already-sorted 15.6x / 4.6x (precheck + decorate), tail-swap/1%
1.00-1.16x. `sort_str` (byte-lex key): already-sorted 10.9x, random/reverse 0.99-1.01x. Plain-sort
negatives are all within ≈ 2 % (gate met). **One keyed workload is over the 3 % line:** `sort_by_key`
on a ≤ 16-distinct-value key at 100k is a stable ≈ 3.5 % regression (corrected 0.963/0.966/0.963x over
three runs, control 0.996-0.999x — real, not bias); the same key at 1M is ≈ 1.00x and every other keyed
workload is within ≈ 2 %. Cause: the keyed straight-copy moves two buffers (elements + keys), so
refinement 2 has less upside for keyed sorts while the tie-heavy 16-value boundary decision mispredicts.
Accepted as a bounded, measured single-cell exception (see doc-12 §4.1).

## Root-cause note (why `w64`, not the first cut)

The first implementation ran the ordered-boundary check on **every** merge pass and measured a real
≈ 7 % regression on random/reverse. An isolation sweep (`ALIGN_SORT_ADAPTIVE=off` baseline + each
refinement independently, drift-immune + control) localized it exactly: the pass-1 check (`width==32`)
has the most run pairs and the least straight-copy benefit, so it is pure overhead on merge-heavy
inputs. The delayed-scratch refinement is throughput-neutral (its apparent cost was 100 % measurement
bias — control == real), and the precheck is free on out-of-order inputs. Gating the boundary check to
`width >= 64` removes the regression while keeping the higher-pass wins. Correctness is covered by
`crates/align_driver/tests/sort_adaptive.rs` independent of the throughput verdict.
