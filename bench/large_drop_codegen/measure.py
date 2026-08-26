#!/usr/bin/env python3
"""Run one benchmark command and report wall time plus child peak RSS as JSON."""

import json
import os
import resource
import subprocess
import sys
import time


def main() -> int:
    if len(sys.argv) < 4 or sys.argv[2] != "--":
        print("usage: measure.py WORKDIR -- COMMAND [ARG ...]", file=sys.stderr)
        return 2
    workdir = sys.argv[1]
    command = sys.argv[3:]
    start = time.monotonic()
    completed = subprocess.run(
        command,
        cwd=workdir,
        env=os.environ.copy(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    elapsed = time.monotonic() - start
    rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    if sys.platform == "darwin":
        rss //= 1024
    if completed.returncode != 0:
        sys.stdout.buffer.write(completed.stdout)
        sys.stderr.buffer.write(completed.stderr)
        return completed.returncode
    print(json.dumps({"wall_seconds": round(elapsed, 3), "peak_rss_kib": rss}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
