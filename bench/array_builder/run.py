#!/usr/bin/env python3
"""Compare scalar array construction with one production runtime archive."""
import csv
import importlib.util
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import sys
import tempfile

REPO = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("native_probe", REPO / "bench/native_probe.py")
owner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(owner)


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: run.py BASELINE_ALIGNC CANDIDATE_ALIGNC PRODUCTION_RUNTIME_A")
    compilers = [Path(value).resolve(strict=True) for value in sys.argv[1:3]]
    if any(not path.is_file() or not os.access(path, os.X_OK) for path in compilers):
        raise SystemExit("both compiler paths must be executable files")
    runtime = Path(sys.argv[3]).resolve(strict=True)
    if not runtime.is_file():
        raise SystemExit("runtime must be an archive file")
    system = os.uname().sysname
    if system not in ("Darwin", "Linux"):
        raise SystemExit("probe supports macOS and Linux")
    for signum in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, owner.record_signal)
    scratch = Path(tempfile.mkdtemp(prefix="align-array-builder-"))
    cleanup_safe = True
    try:
        archive = scratch / "runtime.a"
        shutil.copyfile(runtime, archive)
        symbols = scratch / "symbols.txt"
        with symbols.open("w") as stream:
            owner.run_phase("link", ["nm", "-g", str(archive)], stdout=stream)
        if any(line.split() and line.split()[-1].lstrip("_") == "align_rt_alloc_count"
               for line in symbols.read_text().splitlines()):
            raise RuntimeError("timing requires a production runtime without alloc-count")
        def digest(path):
            with path.open("rb") as stream:
                return hashlib.file_digest(stream, "sha256").hexdigest()
        print(json.dumps({"runtime_sha256": digest(archive),
                          "compiler_sha256": [digest(p) for p in compilers],
                          "kernel_sha256": digest(HERE / "kernel.align"),
                          "probe_sha256": digest(HERE / "probe.c")}), file=sys.stderr)
        flags = (["-Wl,-dead_strip"] if system == "Darwin" else
                 ["-Wl,--gc-sections", "-lpthread", "-ldl", "-lm"])
        for label, compiler in zip(("baseline", "candidate"), compilers):
            obj = scratch / f"{label}.o"
            owner.run_phase("build", [str(compiler), "emit-obj", str(HERE / "kernel.align"),
                                      str(obj), "--target-cpu", "baseline", "--profile", "release",
                                      "--no-rt-lto", "--export", "build_checked"], stdout=sys.stderr)
            owner.run_phase("link", [os.environ.get("CC") or "cc", "-O2", "-Wall", "-Wextra", "-Werror",
                                     str(HERE / "probe.c"), str(obj), str(archive),
                                     *flags, "-o", str(scratch / label)],
                            stdout=sys.stderr)
        writer = csv.writer(sys.stdout, lineterminator="\n")
        writer.writerow(["repeat", "revision", "kind", "ns", "checksum"])
        for repeat in range(7):
            labels = ("baseline", "candidate") if repeat % 2 == 0 else ("candidate", "baseline")
            for label in labels:
                for kind in range(2):
                    output = scratch / "sample.csv"
                    with output.open("w") as stream:
                        owner.run_phase("probe", [str(scratch / label), str(kind)], stdout=stream)
                    row = output.read_text().strip().split(",")
                    if len(row) != 3 or int(row[0]) != kind or any(int(v) < 0 for v in row):
                        raise RuntimeError("invalid native measurement record")
                    writer.writerow([repeat, label, *row])
                    sys.stdout.flush()
    except owner.CleanupError:
        cleanup_safe = False
        print(f"Preserving scratch after child cleanup failure: {scratch}", file=sys.stderr)
        raise
    finally:
        if cleanup_safe:
            shutil.rmtree(scratch)
    owner.check_signal()


if __name__ == "__main__":
    main()
