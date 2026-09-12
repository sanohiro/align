#!/usr/bin/env bash
# Offline host-review boundary owners; --live selects the authenticated agy qualification.
set -euo pipefail
exec python3 "$(dirname "$0")/test-review-bounded.py" "$@"
