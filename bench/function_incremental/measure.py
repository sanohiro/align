#!/usr/bin/env python3
import resource
import subprocess
import sys
import time


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: measure.py COMMAND [ARG ...]", file=sys.stderr)
        return 2
    started = time.monotonic()
    completed = subprocess.run(sys.argv[1:], check=False)
    elapsed = time.monotonic() - started
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    rss_kb = usage.ru_maxrss
    if sys.platform == "darwin":
        rss_kb //= 1024
    print(f"wall_seconds={elapsed:.6f}")
    print(f"peak_rss_kb={rss_kb}")
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
