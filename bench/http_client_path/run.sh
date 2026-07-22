#!/usr/bin/env bash
# http_client_path benchmark: what ONE `http.get` costs inside the CLIENT — allocations, bytes, and
# the calling thread's own CPU time — against an in-process floor that writes the same request and
# reads the same response and nothing else. Like `bench/http_path` (its server-side twin) this links
# `align_runtime` as a Rust lib, so a counting global allocator sees the runtime's own traffic.
#
# Two arms run in INTERLEAVED, counterbalanced blocks so the box's between-run drift hits both alike.
#
#   bench/http_client_path/run.sh [requests-per-arm] [blocks]     (default: 100000 6)
set -euo pipefail
cd "$(dirname "$0")"

cargo build -q --release
./target/release/http-client-path-bench "${1:-100000}" "${2:-6}"
