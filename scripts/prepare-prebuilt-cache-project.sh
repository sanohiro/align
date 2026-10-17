#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/prepare-prebuilt-cache-project.sh PROJECT_ROOT" >&2
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_ROOT="$1"
if [[ -e "$PROJECT_ROOT" ]]; then
  echo "prebuilt cache project: destination already exists: $PROJECT_ROOT" >&2
  exit 1
fi
mkdir -p "$PROJECT_ROOT/pkg"

# Keep complete, dependency-closed trees. Web and WebSocket warming is deferred
# until the documented structured-task callback compatibility gap is repaired
# (docs/impl/21-build-perf-plan.md, item 4). Selected compilation failures are fatal.
for TREE in apps/frame/pkg apps/auth/pkg apps/db/pkg apps/kv/pkg apps/csv/pkg apps/template/pkg apps/s3/pkg; do
  cp -R "$REPO_ROOT/$TREE/." "$PROJECT_ROOT/pkg/"
done

cat > "$PROJECT_ROOT/main.align" <<'ALIGN'
module align_release_cache_warm

import pkg.db
import pkg.db.sqlite
import pkg.db.postgres
import pkg.db.pool
import pkg.frame
import pkg.auth
import pkg.kv
import pkg.csv
import pkg.template
import pkg.s3

fn main() -> i32 = 0
ALIGN
