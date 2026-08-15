#!/usr/bin/env bash
# Deterministic owner for benchmark_evidence_exclusive_run.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - <<'PY'
import tempfile
from pathlib import Path

from scripts.benchmark_evidence import exclusive_run


def expect_error(call, fragment):
    try:
        call()
    except exclusive_run.ExclusiveRunError as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid exclusive-run transition: {fragment}")


with tempfile.TemporaryDirectory(prefix="align-exclusive-owner-") as name:
    root = Path(name)
    lock = root / "host.lock"
    reservation = root / "publication.reservation"
    output = root / "report"
    run_id = "a" * 64

    first = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    second = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    first.acquire()
    expect_error(second.acquire, "host lock")
    expect_error(lambda: first.release_lock_for_publication(), "durable reservation")
    first.create_reservation(run_id, str(output))
    assert reservation.read_text() == f"run_id={run_id}\noutput_dir={output}\n"
    expect_error(second.acquire, "publication reservation")
    first.release_lock_for_publication()
    expect_error(second.acquire, "publication reservation")
    expect_error(first.finalize_publication, "publication")
    first.mark_published()
    second = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    expect_error(second.acquire, "publication reservation")
    first.finalize_publication()
    assert not reservation.exists()

    second.acquire()
    second.abort(remove_reservation=False)

    stranded = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    stranded.acquire()
    stranded.create_reservation("b" * 64, str(output))
    stranded.abort(remove_reservation=False)
    assert reservation.exists()
    blocked = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    expect_error(blocked.acquire, "publication reservation")
    reservation.unlink()
    recovered = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    recovered.acquire()
    recovered.abort(remove_reservation=True)

    symlink = root / "symlink.reservation"
    symlink.symlink_to(output)
    nofollow = exclusive_run.ExclusiveRun(str(root / "other.lock"), str(symlink))
    expect_error(nofollow.acquire, "publication reservation")
    symlink.unlink()

    invalid = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    invalid.acquire()
    expect_error(lambda: invalid.create_reservation("short", str(output)), "run ID")
    invalid.abort(remove_reservation=True)
    expect_error(
        lambda: exclusive_run.ExclusiveRun("/", str(reservation)),
        "absolute non-root",
    )
    path_invalid = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    path_invalid.acquire()
    expect_error(
        lambda: path_invalid.create_reservation(run_id, str(root / "résumé")),
        "invalid path",
    )
    expect_error(
        lambda: path_invalid.create_reservation(run_id, str(root / "line\nbreak")),
        "invalid path",
    )
    path_invalid.abort(remove_reservation=True)

print("exclusive-run checks passed")
PY
