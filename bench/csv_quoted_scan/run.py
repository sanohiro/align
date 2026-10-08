#!/usr/bin/env python3
"""Own the local CSV probe's build, link and execution process groups."""
import os
from pathlib import Path
import signal
import shutil
import subprocess
import sys
import tempfile
import time


LIMITS = {"build": 900, "link": 60, "probe": 60}
pending_signal = 0


class CleanupError(RuntimeError):
    pass


def record_signal(signum, _frame):
    # Defer interruption across spawn and cleanup so ownership is never lost.
    global pending_signal
    pending_signal = pending_signal or signum


def check_signal():
    if pending_signal:
        raise SystemExit(128 + pending_signal)


def run_phase(phase, argv, *, stdout=None):
    check_signal()
    child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=stdout,
                             start_new_session=True)
    try:
        deadline = time.monotonic() + LIMITS[phase]
        work_deadline = deadline - min(5, LIMITS[phase] / 5)
        while True:
            check_signal()
            # Retain the leader's PID until group cleanup, including when it
            # exits before a descendant. This prevents signalling a reused PID.
            result = os.waitid(os.P_PID, child.pid,
                               os.WEXITED | os.WNOHANG | os.WNOWAIT)
            if result is not None and result.si_pid != 0:
                if result.si_code != os.CLD_EXITED or result.si_status != 0:
                    raise RuntimeError(f"CSV benchmark {phase} failed: {result}")
                return
            if time.monotonic() >= work_deadline:
                raise RuntimeError(f"CSV benchmark {phase} exceeded {LIMITS[phase]} seconds")
            time.sleep(0.01)
    finally:
        # The whole group is retired even after a successful leader exit.
        # Reap the direct child before releasing the scratch directory.
        try:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                os.kill(child.pid, signal.SIGKILL)
            except PermissionError:
                # Darwin reports EPERM for a group containing only its zombie
                # leader. A live same-user descendant makes killpg succeed.
                if os.uname().sysname != "Darwin":
                    raise
                result = os.waitid(os.P_PID, child.pid,
                                   os.WEXITED | os.WNOHANG | os.WNOWAIT)
                if result is None or result.si_pid == 0:
                    raise
                os.kill(child.pid, signal.SIGKILL)
            child.wait(timeout=max(0, deadline - time.monotonic()))
        except (OSError, subprocess.SubprocessError) as error:
            raise CleanupError(f"CSV benchmark {phase} cleanup failed") from error


def main():
    if len(sys.argv) != 1:
        raise SystemExit("usage: bench/csv_quoted_scan/run.sh")
    for signum in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, record_signal)
    system = os.uname().sysname
    if system not in ("Darwin", "Linux"):
        raise SystemExit("CSV benchmark supports macOS and Linux")
    os.chdir(Path(__file__).resolve().parents[2])
    runtime = Path(os.environ.get("CARGO_TARGET_DIR") or "target") / "release/libalign_runtime.a"
    scratch = tempfile.mkdtemp(prefix="align-csv-quoted-")
    cleanup_safe = True
    try:
        run_phase("build", ["bash", "scripts/cargo.sh", "build", "-p", "align_runtime",
                            "--release"], stdout=sys.stderr)
        probe = str(Path(scratch) / "probe")
        flags = (["-Wl,-dead_strip"] if system == "Darwin" else
                 ["-Wl,--gc-sections", "-lpthread", "-ldl", "-lm"])
        run_phase("link", [os.environ.get("CC") or "cc", "-O3",
                           "bench/csv_quoted_scan/main.c", str(runtime), *flags, "-o", probe])
        run_phase("probe", [probe])
    except CleanupError:
        cleanup_safe = False
        print(f"Preserving scratch after child cleanup failure: {scratch}", file=sys.stderr)
        raise
    finally:
        if cleanup_safe:
            shutil.rmtree(scratch)
    check_signal()


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from error
