#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
[[ $# -eq 0 ]] || { echo "usage: bench/csv_quoted_scan/run.sh" >&2; exit 2; }
exec python3 -B bench/csv_quoted_scan/run.py
