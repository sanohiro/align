#!/usr/bin/env python3
"""Local V001 scaling witness; all artifacts stay in an exclusively owned stage."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import shutil
import statistics
import sys
import tempfile

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import native_probe


def sha(data):
    return hashlib.sha256(data).hexdigest()


def run(command, stage):
    # The shared supervisor pins the leader PID until its whole group retires.
    # File-backed stdout bounds parent memory; diagnostics inherit stderr.
    with (stage / "stdout").open("wb") as out:
        native_probe.run_phase("probe", command, stdout=out)
    return (stage / "stdout").read_text()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("compiler", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--source-id", required=True,
                        help="Exact compiler source commit or recorded working-tree identity")
    parser.add_argument("--repeats", type=int, default=5)
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("repeats must be positive")
    for signum in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, native_probe.record_signal)
    compiler = args.compiler.resolve()
    output = args.output.resolve()
    here = Path(__file__).resolve().parent
    runtime = compiler.with_name("libalign_runtime.a")
    result = {"compiler_source": args.source_id,
              "runtime_sha256": sha(runtime.read_bytes()),
              "compiler_sha256": sha(compiler.read_bytes()),
              "host": platform.platform(),
              "machine": platform.machine(),
              "mode": "release O2 generated code, baseline CPU, no runtime LTO, cache off; "
                      "parse excluded; timed region includes byte-count print; "
                      "keys validates every key byte and paired value after timing",
              "source_sha256": {}, "rows": []}
    stage = Path(tempfile.mkdtemp(prefix="align-json-keys-")).resolve()
    previous = Path.cwd()
    cleanup_safe = True
    try:
        os.chdir(stage)
        os.environ.update(ALIGNC_CACHE="off", TMPDIR=str(stage))
        for mode in ["key", "keys"]:
            source = here / f"{mode}.align"
            result["source_sha256"][mode] = sha(source.read_bytes())
            run([str(compiler), "build", str(source), "--profile", "release",
                 "--target-cpu", "baseline", "--no-rt-lto", "--jobs", "1"], stage)
            for count in ([4000, 8000, 16000] if mode == "key" else [4000, 8000, 16000, 151000]):
                data = json.dumps({str(i): i for i in range(count)},
                                  separators=(",", ":")).encode()
                path = stage / "input.json"
                path.write_bytes(data)
                expected = sum(len(str(i)) for i in range(count))
                samples = []
                for _ in range(args.repeats):
                    stdout = run([str(stage / mode), str(path)], stage)
                    lines = stdout.splitlines()
                    if len(lines) != 2 or int(lines[0]) != expected:
                        raise RuntimeError(f"invalid observation: {stdout!r}")
                    samples.append(int(lines[1]))
                result["rows"].append({"mode": mode, "members": count,
                                       "expected_key_bytes": expected,
                                       "input_sha256": sha(data), "ns": samples,
                                       "median_ns": statistics.median(samples)})
                print(mode, count, result["rows"][-1]["median_ns"], flush=True)
    except native_probe.CleanupError:
        cleanup_safe = False
        print(f"Preserving scratch after child cleanup failure: {stage}", file=sys.stderr)
        raise
    finally:
        os.chdir(previous)
        if cleanup_safe:
            shutil.rmtree(stage)
    native_probe.check_signal()
    output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
