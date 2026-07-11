# `bench/arena_pool` — M12 Slice A8, per-request arena reuse (measure-first gate)

**Verdict: measured, BELOW the ship gate → record-and-close.** The per-thread arena pool with the
mandated re-zero comes in at **~1.06×** over the pre-pool baseline on the realistic gateway shape —
short of the settled **≥ ~1.15×** gate. The pooling code is therefore **not shipped** (reverted from
the runtime); this bench + record stay as the negative-result artifact (the builder-capacity /
group_by-interleave precedent). See `docs/impl/07-roadmap.md` M12 Slice A8.

## What it measures

The gateway shape — a long-running server loop that resets per request:
`loop { arena { …transients… } }`. Each iteration opens an arena, carves KB-class allocations (a
~2 KiB rendered-template buffer + 8 small parse-table-ish allocs, ~2.5 KiB total → one 64 KiB chunk
per arena), touches them, and ends the arena. Five variants of that identical shape:

| id  | variant                                  | how it is built                                    |
|-----|------------------------------------------|----------------------------------------------------|
| (a) | align arena, **pre-pool** baseline       | `cargo run --release --no-default-features`         |
| (b) | align arena, **pooled + re-zero** (A8)   | `cargo run --release` (default `pool` feature)      |
| (c) | align arena, **pooled, no re-zero**      | (b) with `chunk.fill(0)` in `arena_pool_take` commented out — an *upper bound*; measured then reverted |
| (d) | Rust `bumpalo` reset loop                | same shape, keep-largest-chunk bump allocator       |
| (e) | plain Rust `malloc`/`free` (`Vec`)       | same shape, freed each iteration                    |

All variants drive the **shipped runtime's C-ABI** (`align_rt_arena_begin/alloc/end`) directly, so
(a)/(b)/(c) time real runtime code. Numbers are best-of-N trials (min — least-noise for a microbench).

## Results

Machine: AMD Ryzen 9 5950X (32 threads), 62 GiB RAM, Linux 6.18 (WSL2), rustc 1.96.0, `--release`
(`opt-level=3`, `lto=true`, `codegen-units=1`). 500k iterations, best of 15 trials.

| variant                              | ns / iter | vs (a) baseline |
|--------------------------------------|-----------|-----------------|
| (a) align arena — pre-pool           |   555.8   | 1.00×           |
| (b) align arena — pooled + re-zero   |   523.9   | **1.061×**      |
| (c) align arena — pooled, no re-zero |    41.2   | 13.5×           |
| (d) rust `bumpalo` — reset loop      |    19.2   | 28.9×           |
| (e) rust `malloc`/`free` — `Vec`     |    23.7   | 23.4×           |

(300k / best-of-9 reproduces the same ratios: (a) 556.8, (b) 522.7.)

## Reading the numbers

- **The re-zero is the entire cost.** (b) vs (c) shows that memset-ing the full 64 KiB chunk on reuse
  (≈480 ns) dwarfs the malloc/free the pool removes (≈32 ns, exactly (a)−(b)). The settled record
  mandates the re-zero for v1 (fresh chunks are fully zero-initialized and code may rely on it;
  dropping the re-zero is a separate, provably-safe follow-up). *With* the re-zero, pooling cannot
  clear the gate.
- **The upper bound (c) is real and large** — 13.5× — and is what the recorded drop-the-re-zero
  follow-up would unlock (approaching bumpalo/malloc territory). That is a *future* slice, not this one.
- Both (a) and (b) memset 64 KiB per iteration (calloc / `vec![0u8; CHUNK]` on a recycled heap block
  actively zeroes; the fixed-size no-doubling chunk means it is ∝ CHUNK, not request size). This is
  exactly the cost the roadmap flagged, and pooling with re-zero does not remove it.

## Reproducing the pool variants (a)/(b)/(c)

The pool is **reverted from the shipped runtime**; the full prototype (feature `arena_pool`, its unit
tests, and this feature-gated bench) is preserved in this branch's git history (the commit before the
revert). To re-measure the follow-up, resurrect that commit and:

```sh
bench/arena_pool/run.sh            # (b) pool ON, then (a) pool OFF; (d)/(e) print in both
ITERS=1000000 TRIALS=15 bench/arena_pool/run.sh
# (c): comment out `chunk.fill(0)` in align_runtime's `arena_pool_take`, then re-run the (b) build.
```

As shipped (pool reverted), `run.sh` measures the pre-pool arena vs `bumpalo` vs `malloc`.
