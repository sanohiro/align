#!/usr/bin/env bash
# http_path benchmark: what ONE request costs inside the server — allocations, bytes, and the
# server thread's own CPU time — against an in-process floor that does read + write and nothing
# else. Unlike `bench/web_e2e` this links `align_runtime` as a Rust lib, so a counting global
# allocator sees the runtime's own `Vec`/`String` traffic.
#
# Three arms — a plain read/write floor, the same plus the `poll` Align's keep-alive wait does, and
# Align — run in INTERLEAVED blocks so the box's between-run drift hits all of them alike.
#
#   bench/http_path/run.sh [requests-per-arm] [blocks]     (default: 100000 5)
set -euo pipefail
cd "$(dirname "$0")"

cargo build -q --release
./target/release/http-path-bench "${1:-100000}" "${2:-5}"
