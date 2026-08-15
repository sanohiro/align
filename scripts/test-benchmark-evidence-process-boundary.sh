#!/usr/bin/env bash
# Deterministic owner for benchmark_evidence_process_boundary_matrix.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - <<'PY'
import hashlib

from scripts.benchmark_evidence import process_boundary as boundary


def expect_error(call, fragment, error_type=boundary.ProcessBoundaryError):
    try:
        call()
    except error_type as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid process-boundary input: {fragment}")


environment = boundary.fixed_environment()
assert tuple(environment.as_dict()) == (
    "PATH",
    "LC_ALL",
    "TZ",
    "HOME",
    "CARGO_NET_OFFLINE",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "TMPDIR",
    "ALIGN_BENCH_WORK_DIR",
)
assert environment.as_dict()["HOME"] == "/nonexistent"
assert environment.sha256() == hashlib.sha256(environment.as_bytes()).hexdigest()
boundary.validate_environment(environment.as_dict(), environment)
expect_error(
    lambda: boundary.validate_environment(
        {**environment.as_dict(), "HTTP_PROXY": "injected"}, environment
    ),
    "fixed environment",
)
expect_error(
    lambda: boundary.validate_environment(
        {**environment.as_dict(), "HOME": "/tmp"}, environment
    ),
    "fixed environment",
)
expect_error(lambda: boundary.fixed_environment(work="/work/../escape"), "aliases")

assert boundary.validate_argv(("bench/json_decode/run.sh", "native")) == (
    "bench/json_decode/run.sh",
    "native",
)
expect_error(lambda: boundary.validate_argv(()), "non-empty")
expect_error(lambda: boundary.validate_argv(("run\x00",)), "NUL-free")
expect_error(lambda: boundary.validate_argv(("run\n",)), "printable ASCII")

boundary.DescriptorMap(0, 1, 2).validate()
boundary.validate_fd_inventory((2, 0, 1))
expect_error(
    lambda: boundary.DescriptorMap(0, 1, 2, (3,)).validate(),
    "unexpected descriptors",
)
expect_error(lambda: boundary.validate_fd_inventory((0, 1, 3)), "exactly")

capture = boundary.BoundedCapture(limit=8, tail_limit=4)
capture.feed(b"abc")
capture.feed(b"defgh")
result = capture.finish()
assert result.size == 8
assert result.sha256 == hashlib.sha256(b"abcdefgh").hexdigest()
assert result.tail_hex == b"efgh".hex()
expect_error(lambda: capture.feed(b"x"), "already closed")

overflow = boundary.BoundedCapture(limit=4, tail_limit=4)
overflow.feed(b"abcd")
expect_error(lambda: overflow.feed(b"e"), "exceeded", boundary.CaptureOverflow)
expect_error(lambda: overflow.finish(), "overflow")

assert boundary.ChildExit(0).accepted()
for status in (
    boundary.ChildExit(1),
    boundary.ChildExit(None, signal=9),
    boundary.ChildExit(0, timed_out=True),
    boundary.ChildExit(0, truncated=True),
):
    expect_error(lambda status=status: boundary.require_success(status), "accepted zero-exit")
expect_error(lambda: boundary.ChildExit(None).validate(), "no exit")
expect_error(lambda: boundary.ChildExit(256).validate(), "exit code")

print("process boundary checks passed")
PY
