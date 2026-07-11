#!/usr/bin/env bash
# M12 Slice A8 — per-request arena reuse benchmark (the measure-first ship gate).
#
# The arena pool measured ~1.06x over the pre-pool baseline on the gateway shape — below the settled
# >= ~1.15x gate — so it was reverted (record-and-close; see README.md). As shipped, this harness
# times the pre-pool arena (a) vs Rust `bumpalo` (d) vs `malloc`/`free` (e) over the gateway shape
# (`loop { arena { …KB-class allocs… } }`). `align_runtime` is an ordinary path dependency, so
# `cargo run` builds it; no `alignc` needed. All numbers are best-of-TRIALS (min).
#
#   bench/arena_pool/run.sh                 # 300k iterations, best of 9 (defaults)
#   ITERS=1000000 TRIALS=15 bench/arena_pool/run.sh
#
# The (b) pooled + (c) no-re-zero pool variants are reproducible from the prototype commit in this
# branch's git history (the feature-gated `arena_pool` build) — see README.md.
set -euo pipefail
cd "$(dirname "$0")"
exec cargo run -q --release
