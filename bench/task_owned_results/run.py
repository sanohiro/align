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
import subprocess
import tempfile


def run(arguments, directory, env):
    process = subprocess.Popen(arguments, cwd=directory, env=env, start_new_session=True,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        stdout, stderr = process.communicate(timeout=60)
    except BaseException:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        stdout, stderr = process.communicate(timeout=5)
        print(stdout, end="")
        print(stderr, end="", file=__import__("sys").stderr)
        raise
    if process.returncode:
        raise RuntimeError(f"{arguments}: exit {process.returncode}\n{stdout}\n{stderr}")
    return stdout


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("alignc", type=Path)
    parser.add_argument("--samples", type=int, default=7)
    args = parser.parse_args()
    if args.samples < 1:
        parser.error("samples must be positive")
    compiler = args.alignc.resolve(strict=True)
    source = Path(__file__).resolve().parent
    environment = dict(os.environ, ALIGNC_CACHE="off")
    measurements = {"sequential": [], "parallel": []}
    witness = None
    with tempfile.TemporaryDirectory(prefix="align-owned-task-") as temporary:
        directory = Path(temporary)
        for name in ["main.align", "kernel.align"]:
            shutil.copyfile(source / name, directory / name)
        run([str(compiler), "build", "main.align", "--profile", "release",
             "--target-cpu", "native"], directory, environment)
        # Each sample is a fresh process. It warms both arms and checks every byte before
        # timing one complete arm (allocation, work, joins and ordered output assembly).
        for sample in range(args.samples):
            order = ["sequential", "parallel"] if sample % 2 == 0 else ["parallel", "sequential"]
            for mode in order:
                stdout = run([str(directory / "main")], directory,
                             dict(environment, ALIGN_TASK_MODE=mode))
                values = [int(value) for value in stdout.splitlines()]
                if len(values) != 3 or values[0] <= 0 or values[1] != 4194307:
                    raise RuntimeError(f"invalid measured outcome: {stdout}")
                if witness is None:
                    witness = values[1:]
                if values[1:] != witness:
                    raise RuntimeError(f"output witness drift: {values[1:]} != {witness}")
                measurements[mode].append(values[0])
                print(json.dumps({"sample": sample, "mode": mode, "nanoseconds": values[0]}), flush=True)
    print(json.dumps({"host": platform.platform(), "machine": platform.machine(),
                      "logical_cpus": os.cpu_count(), "bands": 8, "bytes_checksum": witness,
                      "compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
                      "samples_ns": measurements,
                      "median_speedup": statistics.median(measurements["sequential"]) /
                                        statistics.median(measurements["parallel"])}, sort_keys=True))


if __name__ == "__main__":
    main()
