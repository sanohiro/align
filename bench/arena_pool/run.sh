#!/usr/bin/env bash
# M12 Slice A8 — per-request arena reuse benchmark (the measure-first ship gate).
#
# Runs the gateway shape (`loop { arena { …KB-class allocs… } }`) twice: once with the arena pool
# compiled IN (variant (b) — the A8 slice) and once with it compiled OUT (variant (a) — pre-pool
# baseline). The Rust `bumpalo` (d) and `malloc/free` (e) baselines are feature-independent and print
# in both runs as a stability check. `align_runtime` is an ordinary path dependency, so `cargo run`
# builds it; no `alignc` needed. All numbers are best-of-TRIALS (min).
#
#   bench/arena_pool/run.sh                 # 300k iterations, best of 9 (defaults)
#   ITERS=1000000 TRIALS=15 bench/arena_pool/run.sh
#
# Variant (c) (pooled, no re-zero — the upper bound) is measured by temporarily commenting out the
# `chunk.fill(0)` line in `align_rt_arena` `arena_pool_take` and re-running (b); it is not reproducible
# from this harness alone by design (the re-zero is the shipped v1 behavior).
set -euo pipefail
cd "$(dirname "$0")"

echo "=== (b) pool ON (A8 slice) + (d) bumpalo + (e) malloc ==="
cargo run -q --release

echo
echo "=== (a) pool OFF (pre-pool baseline) ==="
cargo run -q --release --no-default-features
