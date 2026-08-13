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
  echo "docker daemon is not running (start Docker Desktop on macOS/WSL2, or 'systemctl start docker' on Linux)" >&2
  exit 2
}

scripts/check-libpq-version.sh

# Some developer hosts intentionally carry only libpq's stable runtime ABI
# (`libpq.so.5`) and not the development-package linker name (`libpq.so`). The
# version/symbol probe above proves that runtime is usable; expose only its
# linker name for this invocation so the generated owner executables still use
# the verified library. CI has pg_config/libpq-dev and never takes this branch.
libpq_shim=""
if ! command -v pg_config >/dev/null 2>&1; then
  if command -v ldconfig >/dev/null 2>&1; then
    libpq_runtime="$(ldconfig -p | awk '$1 == "libpq.so.5" { print $NF; exit }')"
  else
    libpq_runtime=""
  fi
  if [ -z "$libpq_runtime" ]; then
    echo "libpq development linker name is unavailable and no libpq.so.5 runtime was found" >&2
    exit 2
  fi
  libpq_shim="$(mktemp -d)"
  ln -s "$libpq_runtime" "$libpq_shim/libpq.so"
  export LIBRARY_PATH="$libpq_shim${LIBRARY_PATH:+:$LIBRARY_PATH}"
  echo "using verified runtime libpq through a temporary linker shim: $libpq_runtime"
fi

cleanup() {
  docker rm -f "$name" >/dev/null 2>&1 || true
  if [ -n "$libpq_shim" ]; then
    [ ! -L "$libpq_shim/libpq.so" ] || unlink "$libpq_shim/libpq.so"
    rmdir "$libpq_shim"
  fi
}
trap cleanup EXIT

docker run -d --name "$name" \
  -e POSTGRES_DB=align -e POSTGRES_USER=align -e POSTGRES_PASSWORD=align \
  -p "127.0.0.1:${port}:5432" "$image" >/dev/null

# postgres's init entrypoint starts a temporary server (unix socket only,
# listen_addresses='') and then restarts. Probe over TCP: the temp server has
# no TCP listener, so three consecutive TCP successes prove the final server.
ready=0
attempt=0
while [ "$attempt" -lt 90 ]; do
  if docker exec "$name" pg_isready -h 127.0.0.1 -U align -d align >/dev/null 2>&1; then
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

docker exec "$name" psql -h 127.0.0.1 -U align -d align -Atqc 'SHOW server_version'

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

# Steps 2-14: the same thirteen required integration suites CI runs.
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q1 -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q2 -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q3 -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q4a -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q4b -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q5a -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q5b1 -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q5b2 -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_q6 -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_a1 -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_pool -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_a2 -- --nocapture
scripts/cargo.sh test --locked -p align_driver --test pkg_db_callbacks -- --nocapture

echo "local PostgreSQL verification passed (CI parity: all thirteen pkg.db owner suites)"
