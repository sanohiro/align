#!/usr/bin/env bash
# http_path benchmark: what ONE request costs inside the server — allocations, bytes, and the
# server thread's own CPU time — against an in-process floor that does read + write and nothing
# else. Unlike `bench/web_e2e` this links `align_runtime` as a Rust lib, so a counting global
# allocator sees the runtime's own `Vec`/`String` traffic.
#
#   bench/http_path/run.sh [iters]        (default: 200000 — see README on why not fewer)
set -euo pipefail
cd "$(dirname "$0")"

iters="${1:-200000}"

cargo build -q --release
./target/release/http-path-bench "$iters"
