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

for TREE in apps/web/pkg apps/frame/pkg apps/auth/pkg apps/db/pkg apps/kv/pkg apps/csv/pkg apps/ws/pkg apps/template/pkg; do
  cp -R "$REPO_ROOT/$TREE/." "$PROJECT_ROOT/pkg/"
done

cat > "$PROJECT_ROOT/main.align" <<'ALIGN'
module align_release_cache_warm

import pkg.db
import pkg.db.sqlite
import pkg.db.postgres
import pkg.db.pool
import pkg.web
import pkg.web.types
import pkg.web.cookie
import pkg.web.cors
import pkg.web.multipart
import pkg.frame
import pkg.auth
import pkg.kv
import pkg.csv
import pkg.ws
import pkg.template

fn main() -> i32 = 0
ALIGN
