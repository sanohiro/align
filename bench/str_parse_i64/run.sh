#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
exec python3 -B bench/native_probe.py str_parse_i64 "$@"
