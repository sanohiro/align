#!/usr/bin/env bash
# std.http R6 benchmark (http.md): the keepalive connection pool vs a plain-Rust `std::net` baseline,
# over a localhost plaintext HTTP/1.1 server. Drives the shipped pool via its C-ABI entry points.
#
#   bench/http_client/run.sh            # 30k GETs, best of 7 (defaults)
#   N=100000 TRIALS=9 bench/http_client/run.sh
#
# `align_runtime` is an ordinary path dependency here, so `cargo run` builds it; no `alignc` needed.
set -euo pipefail
cd "$(dirname "$0")"
exec cargo run -q --release
