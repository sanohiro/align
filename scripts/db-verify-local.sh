#!/usr/bin/env bash
# Local Docker replica of CI's "PostgreSQL integration (required)" job.
#
# CI is the final guard, not the discovery loop: any change touching apps/db or
# the pkg_db_* driver tests must pass this script locally before it is pushed.
# The container image, credentials, environment, and test steps mirror
# .github/workflows/ci.yml exactly, so a local pass predicts the required CI job.
set -euo pipefail

cd "$(dirname "$0")/.."

port="${ALIGN_DB_LOCAL_PORT:-55432}"
image="postgres:16.4"
name="align-db-verify-$$"

command -v docker >/dev/null 2>&1 || { echo "docker is required" >&2; exit 2; }
docker info >/dev/null 2>&1 || {
  echo "docker daemon is not running (start Docker Desktop first)" >&2
  exit 2
}

cleanup() { docker rm -f "$name" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker run -d --name "$name" \
  -e POSTGRES_DB=align -e POSTGRES_USER=align -e POSTGRES_PASSWORD=align \
  -p "127.0.0.1:${port}:5432" "$image" >/dev/null

# postgres's init entrypoint restarts the server once, so a single successful
# probe can race the restart; require three consecutive ready probes.
ready=0
attempt=0
while [ "$attempt" -lt 90 ]; do
  if docker exec "$name" pg_isready -U align -d align >/dev/null 2>&1; then
    ready=$((ready + 1))
    [ "$ready" -ge 3 ] && break
  else
    ready=0
  fi
  attempt=$((attempt + 1))
  sleep 1
done
if [ "$ready" -lt 3 ]; then
  echo "postgres did not become ready within 90s" >&2
  exit 1
fi

export ALIGN_DB_POSTGRES_REQUIRED=1
export ALIGN_DB_POSTGRES_URL="postgresql://align:align@127.0.0.1:${port}/align"

docker exec "$name" psql -U align -d align -Atqc 'SHOW server_version'

# Step 1 (inverted, as in CI): required mode with a missing URL must FAIL.
set +e
env -u ALIGN_DB_POSTGRES_URL scripts/cargo.sh test --locked \
  -p align_driver --test pkg_db_q2 postgres_required_mode_requires_configuration -- --exact
status=$?
set -e
if [ "$status" -eq 0 ]; then
  echo "required PostgreSQL mode accepted a missing URL" >&2
  exit 1
fi

# Steps 2-4: the same three required integration suites CI runs.
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q2 -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q3 -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q5a -- --nocapture

echo "local PostgreSQL verification passed (CI parity: pkg_db_q2 / q3 / q5a)"
