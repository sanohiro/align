#!/usr/bin/env python3
"""Own the local native runtime probes' build, link and execution process groups."""
import os
from pathlib import Path
import re
import signal
import shutil
import subprocess
import sys
import tempfile
import time


PROBES = (
    "base64_decode", "buffer_zero", "buffer_pages", "csv_quoted_scan", "csv_normalization",
    "escaped_decode", "encoding_writes", "utf8_lossy", "time_parse", "time_fields",
    "hex_decode", "regex_spans", "rand_sample", "str_parse_i64", "path_ext",
)
STRICT_C_PROBES = ("time_parse", "time_fields", "hex_decode", "regex_spans", "rand_sample", "str_parse_i64", "path_ext")
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
                    raise RuntimeError(f"native probe {phase} failed: {result}")
                return
            if time.monotonic() >= work_deadline:
                raise RuntimeError(f"native probe {phase} exceeded {LIMITS[phase]} seconds")
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
            raise CleanupError(f"native probe {phase} cleanup failed") from error


def valid_case_index(argument, maximum):
    # Match the fixtures' strtol base-10 grammar, including leading ASCII
    # whitespace, a sign and leading zeroes. Bound before Python conversion.
    if re.fullmatch(r"[ \t\r\n\v\f]*[+-]?[0-9]+", argument) is None:
        return False
    value = argument.lstrip(" \t\r\n\v\f")
    negative = value.startswith("-")
    digits = value.lstrip("+-").lstrip("0") or "0"
    if len(digits) > len(str(maximum)):
        return False
    index = int(digits)
    return index <= maximum and (not negative or index == 0)


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in PROBES:
        raise SystemExit("usage: python3 bench/native_probe.py {" + ",".join(PROBES) + "}")
    benchmark = sys.argv[1]
    arguments = sys.argv[2:]
    if benchmark == "time_parse":
        if len(arguments) != 2 or arguments[0] not in ("0", "1", "2", "3", "4") or arguments[1] not in ("0", "1", "2", "3"):
            raise SystemExit("usage: python3 bench/native_probe.py time_parse kind[0..4] group[0..3]")
    elif benchmark == "path_ext":
        if len(arguments) != 2 or arguments[0] not in ("0", "1", "2") or arguments[1] not in ("0", "1", "2", "3", "4"):
            raise SystemExit("usage: python3 bench/native_probe.py path_ext kind[0..2] group[0..4]")
    elif benchmark == "str_parse_i64":
        if len(arguments) != 1 or arguments[0] not in ("0", "1", "2", "3", "4", "5", "6"):
            raise SystemExit("usage: python3 bench/native_probe.py str_parse_i64 group[0..6]")
    elif benchmark == "time_fields":
        if len(arguments) > 1 or (arguments and arguments[0] not in ("0", "1", "2", "3", "4")):
            raise SystemExit("usage: python3 bench/native_probe.py time_fields [kind 0..4]")
    elif benchmark in ("hex_decode", "regex_spans"):
        maximum = 107 if benchmark == "hex_decode" else 49
        if len(arguments) > 1 or (arguments and not valid_case_index(arguments[0], maximum)):
            raise SystemExit(f"usage: python3 bench/native_probe.py {benchmark} [case 0..{maximum}]")
    elif arguments:
        raise SystemExit("native probe does not accept extra arguments")
    for signum in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, record_signal)
    system = os.uname().sysname
    if system not in ("Darwin", "Linux"):
        raise SystemExit("native probe supports macOS and Linux")
    os.chdir(Path(__file__).resolve().parents[1])
    runtime = Path(os.environ.get("CARGO_TARGET_DIR") or "target") / "release/libalign_runtime.a"
    scratch = tempfile.mkdtemp(prefix=f"align-{benchmark}-")
    cleanup_safe = True
    try:
        # Shared Cargo targets can retain another checkout's top-level archive
        # while this checkout is Fresh. Refresh the actual producer, not its bytes.
        Path("crates/align_runtime/src/lib.rs").touch()
        run_phase("build", ["bash", "scripts/cargo.sh", "build", "-p", "align_runtime",
                            "--release"], stdout=sys.stderr)
        probe = str(Path(scratch) / "probe")
        flags = (["-Wl,-dead_strip"] if system == "Darwin" else
                 ["-Wl,--gc-sections", "-lpthread", "-ldl", "-lm"])
        compiler_flags = (["-O2", "-Wall", "-Wextra", "-Werror"] if benchmark in STRICT_C_PROBES else ["-O3"])
        run_phase("link", [os.environ.get("CC") or "cc", *compiler_flags,
                           f"bench/{benchmark}/main.c", str(runtime), *flags, "-o", probe])
        run_phase("probe", [probe, *arguments])
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
