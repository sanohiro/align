#!/usr/bin/env bash
# Print and enforce pkg.db's libpq 17 baseline.
#
# A development install gets a compile-time signature probe against libpq-fe.h.
# Runtime-only local hosts still verify the loaded ABI version and every newly
# required symbol through ctypes, so db-verify-local cannot silently exercise
# an older client library.
set -euo pipefail

minimum=170000

if command -v pg_config >/dev/null 2>&1; then
  workdir="$(mktemp -d)"
  cleanup() { rm -rf "$workdir"; }
  trap cleanup EXIT
  include_dir="$(pg_config --includedir)"
  library_dir="$(pg_config --libdir)"
  cat > "$workdir/libpq_probe.c" <<'EOF'
#include <libpq-fe.h>
#include <stdio.h>

static int (*const single_row_mode)(PGconn *) = PQsetSingleRowMode;
static int (*const chunked_rows_mode)(PGconn *, int) = PQsetChunkedRowsMode;
static PGcancelConn *(*const cancel_create)(PGconn *) = PQcancelCreate;
static int (*const cancel_start)(PGcancelConn *) = PQcancelStart;
static PostgresPollingStatusType (*const cancel_poll)(PGcancelConn *) = PQcancelPoll;
static int (*const cancel_socket)(const PGcancelConn *) = PQcancelSocket;
static char *(*const cancel_error)(const PGcancelConn *) = PQcancelErrorMessage;
static void (*const cancel_finish)(PGcancelConn *) = PQcancelFinish;
static int (*const socket_poll)(int, int, int, pg_usec_time_t) = PQsocketPoll;
static pg_usec_time_t (*const current_time_usec)(void) = PQgetCurrentTimeUSec;

int main(void) {
  (void)single_row_mode;
  (void)chunked_rows_mode;
  (void)cancel_create;
  (void)cancel_start;
  (void)cancel_poll;
  (void)cancel_socket;
  (void)cancel_error;
  (void)cancel_finish;
  (void)socket_poll;
  (void)current_time_usec;
  int version = PQlibVersion();
  printf("libpq client version: %d.%d (%d; compiled signature probe)\n",
         version / 10000, version % 10000, version);
  return version < 170000;
}
EOF
  cc -std=c11 -Wall -Wextra -Werror -I"$include_dir" "$workdir/libpq_probe.c" \
    -L"$library_dir" -lpq -o "$workdir/libpq_probe"
  "$workdir/libpq_probe"
  exit 0
fi

python3 - "$minimum" <<'PY'
import ctypes
import ctypes.util
import sys

minimum = int(sys.argv[1])
name = ctypes.util.find_library("pq")
if not name:
    raise SystemExit("libpq runtime library was not found")
library = ctypes.CDLL(name)
library.PQlibVersion.restype = ctypes.c_int
version = library.PQlibVersion()
required = [
    "PQsetSingleRowMode",
    "PQsetChunkedRowsMode",
    "PQcancelCreate",
    "PQcancelStart",
    "PQcancelPoll",
    "PQcancelSocket",
    "PQcancelErrorMessage",
    "PQcancelFinish",
    "PQsocketPoll",
    "PQgetCurrentTimeUSec",
]
missing = [symbol for symbol in required if not hasattr(library, symbol)]
if missing:
    raise SystemExit("libpq is missing required symbols: " + ", ".join(missing))
print(
    f"libpq client version: {version // 10000}.{version % 10000} "
    f"({version}; runtime ABI probe)"
)
if version < minimum:
    raise SystemExit(f"libpq >= 17 is required, found numeric version {version}")
PY
