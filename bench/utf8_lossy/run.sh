#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
[[ $# -eq 0 ]] || { echo "usage: bench/utf8_lossy/run.sh" >&2; exit 2; }
exec python3 -B bench/native_probe.py utf8_lossy
