# Benchmarks

Reproducible head-to-head benchmarks of Align against idiomatic, hand-written Rust. Align's promise
is *predictable performance from declarative data-oriented code*, so the bar is concrete: **on its
strong cases Align must match or beat Rust**.

```sh
bench/run.sh            # native (both sides at the host CPU's best — AVX2 etc.)
bench/run.sh baseline   # the portable floor (x86-64-v2 on amd64)
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

## What we've learned

- **Flat scalar pipelines (`where`/`map`/`reduce` over `slice<T>`): Align ≈ Rust.** They lower to the
  *same* LLVM IR and the *same* machine code (the `where` is branchless — `pcmpgtq` + `pand` mask —
  for both). So Align matches hand-tuned Rust automatically, from shorter code; it can't *beat* it
  here (shared LLVM backend → identical code is the ceiling).
- **The lever that beats Rust is layout: `soa<T>`.** Idiomatic Rust uses `Vec<Struct>` (array-of-
  structs); a scan that touches a few fields still drags whole cache lines through memory. Align's
  `soa<T>` stores each field in its own column, so a field-subset scan reads only those columns —
  measured **≈3.7× faster** than the AoS scan on a memory-bound workload. That kernel joins here as
  `soa<T>` lands.
