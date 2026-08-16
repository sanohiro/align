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

    failing = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    failing.acquire()
    failing.create_reservation("c" * 64, str(output))
    failing.release_lock_for_publication()
    failing.mark_published()
    original_fsync_parent = exclusive_run._fsync_parent
    fsync_failed = [False]

    def fail_once(path):
        if not fsync_failed[0]:
            fsync_failed[0] = True
            raise OSError("injected finalization fsync failure")
        return original_fsync_parent(path)

    exclusive_run._fsync_parent = fail_once
    try:
        failing.finalize_publication()
    except OSError as exc:
        assert "injected finalization fsync failure" in str(exc)
    else:
        raise AssertionError("finalization fsync failure was accepted")
    finally:
        exclusive_run._fsync_parent = original_fsync_parent
    assert reservation.exists()
    blocked = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    expect_error(blocked.acquire, "publication reservation")
    failing.abort(remove_reservation=False)
    reservation.unlink()

    closing = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    closing.acquire()
    closing.create_reservation("d" * 64, str(output))
    closing.release_lock_for_publication()
    closing.mark_published()

    original_release = closing._release_finalization_lock
    original_close = exclusive_run.os.close
    close_failed = [False]

    def release_with_real_close_failure():
        final_fd = closing._lock_fd

        def fail_after_close(fd):
            if fd == final_fd and not close_failed[0]:
                original_close(fd)
                close_failed[0] = True
                raise OSError("injected finalization lock close failure")
            return original_close(fd)

        exclusive_run.os.close = fail_after_close
        try:
            original_release()
        finally:
            exclusive_run.os.close = original_close

    closing._release_finalization_lock = release_with_real_close_failure
    closing.finalize_publication()
    assert close_failed[0]
    assert not reservation.exists()

    aborting = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    aborting.acquire()
    aborting.create_reservation("e" * 64, str(output))
    original_fsync_parent = exclusive_run._fsync_parent
    abort_fsync_failed = [False]

    def fail_abort_fsync(path):
        if not abort_fsync_failed[0]:
            abort_fsync_failed[0] = True
            raise OSError("injected abort reservation fsync failure")
        return original_fsync_parent(path)

    exclusive_run._fsync_parent = fail_abort_fsync
    try:
        aborting.abort(remove_reservation=True)
    except OSError as exc:
        assert "injected abort reservation fsync failure" in str(exc)
    else:
        raise AssertionError("abort reservation fsync failure was accepted")
    finally:
        exclusive_run._fsync_parent = original_fsync_parent
    assert aborting.locked
    assert reservation.exists()
    blocked = exclusive_run.ExclusiveRun(str(lock), str(reservation))
    expect_error(blocked.acquire, "publication reservation")
    aborting.abort(remove_reservation=False)
    reservation.unlink()

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
