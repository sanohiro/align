#!/usr/bin/env bash
# JSON-to-SoA evidence benchmark. Preparation builds and seals every executable input;
# measurement verifies the seal and directly executes the prepared native harness.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
BENCH_NAME=json_soa
BENCH_BINARY=json-soa-bench
BENCH_EXPORTS="--export agg --export agg_len --export agg_aos --export agg_aos_len --export agg_proj --export agg_proj_len"

# shellcheck source=../json_escape/evidence/run-prepared-benchmark.sh
source "$SCRIPT_DIR/../json_escape/evidence/run-prepared-benchmark.sh"
