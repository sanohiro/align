#!/usr/bin/env bash
# JSON decode-throughput evidence benchmark. Preparation builds and seals every executable input;
# measurement verifies the seal and directly executes the prepared native harness.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
BENCH_NAME=json_decode
BENCH_BINARY=json-decode-bench
BENCH_EXPORTS="--export decode_full --export decode_full_len --export decode_proj --export decode_proj_len"

# shellcheck source=../json_escape/evidence/run-prepared-benchmark.sh
source "$SCRIPT_DIR/../json_escape/evidence/run-prepared-benchmark.sh"
