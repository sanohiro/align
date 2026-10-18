#!/usr/bin/env python3
"""Local contiguous-output witness. Build alignc and an uninstrumented runtime first."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import statistics
import sys
import tempfile

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import native_probe


def run(arguments, directory):
    # Pin the leader PID until the entire group is retired, including descendants
    # left by a successful leader. Signals are deferred across spawn and cleanup.
    with (directory / "stdout").open("wb") as output:
        native_probe.run_phase("probe", arguments, stdout=output)
    return (directory / "stdout").read_text()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("alignc", type=Path)
    parser.add_argument("--samples", type=int, default=7)
    args = parser.parse_args()
    if args.samples < 1:
        parser.error("samples must be positive")
    for signum in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, native_probe.record_signal)
    compiler = args.alignc.resolve(strict=True)
    source = Path(__file__).resolve().parent
    measurements = {"sequential": [], "parallel": []}
    witness = None
    directory = Path(tempfile.mkdtemp(prefix="align-owned-task-")).resolve()
    previous = Path.cwd()
    cleanup_safe = True
    try:
        os.chdir(directory)
        os.environ.update(ALIGNC_CACHE="off", TMPDIR=str(directory))
        for name in ["main.align", "kernel.align"]:
            shutil.copyfile(source / name, directory / name)
        run([str(compiler), "build", "main.align", "--profile", "release",
             "--target-cpu", "native"], directory)
        # Each sample is a fresh process. It warms both arms and checks every byte before
        # timing one complete arm (allocation, work, joins and ordered output assembly).
        for sample in range(args.samples):
            order = ["sequential", "parallel"] if sample % 2 == 0 else ["parallel", "sequential"]
            for mode in order:
                os.environ["ALIGN_TASK_MODE"] = mode
                stdout = run([str(directory / "main")], directory)
                values = [int(value) for value in stdout.splitlines()]
                if len(values) != 3 or values[0] <= 0 or values[1] != 4194307:
                    raise RuntimeError(f"invalid measured outcome: {stdout}")
                if witness is None:
                    witness = values[1:]
                if values[1:] != witness:
                    raise RuntimeError(f"output witness drift: {values[1:]} != {witness}")
                measurements[mode].append(values[0])
                print(json.dumps({"sample": sample, "mode": mode, "nanoseconds": values[0]}), flush=True)
    except native_probe.CleanupError:
        cleanup_safe = False
        print(f"Preserving scratch after child cleanup failure: {directory}", file=sys.stderr)
        raise
    finally:
        os.chdir(previous)
        if cleanup_safe:
            shutil.rmtree(directory)
    native_probe.check_signal()
    print(json.dumps({"host": platform.platform(), "machine": platform.machine(),
                      "logical_cpus": os.cpu_count(), "bands": 8, "bytes_checksum": witness,
                      "compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
                      "samples_ns": measurements,
                      "median_speedup": statistics.median(measurements["sequential"]) /
                                        statistics.median(measurements["parallel"])}, sort_keys=True))


if __name__ == "__main__":
    main()
