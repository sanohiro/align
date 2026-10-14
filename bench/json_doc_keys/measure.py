#!/usr/bin/env python3
"""Local V001 scaling witness; all artifacts stay in an exclusively owned stage."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import statistics
import subprocess
import tempfile


def sha(data):
    return hashlib.sha256(data).hexdigest()


def run(command, stage, timeout=55):
    # File-backed output bounds parent memory and preserves group ownership until
    # all descendants have retired. TemporaryDirectory owns every artifact.
    with (stage / "stdout").open("wb") as out, (stage / "stderr").open("wb") as err:
        child = subprocess.Popen(command, cwd=stage, stdin=subprocess.DEVNULL,
                                 stdout=out, stderr=err, start_new_session=True,
                                 env={**os.environ, "ALIGNC_CACHE": "off", "TMPDIR": str(stage)})
        try:
            result = child.wait(timeout=timeout)
        finally:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait(timeout=5)
    stdout = (stage / "stdout").read_text()
    stderr = (stage / "stderr").read_text()
    if result:
        raise RuntimeError(f"{command}: {result}\n{stdout}\n{stderr}")
    return stdout, stderr


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
    compiler = args.compiler.resolve()
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
    with tempfile.TemporaryDirectory(prefix="align-json-keys-") as directory:
        stage = Path(directory)
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
                    stdout, stderr = run([str(stage / mode), str(path)], stage)
                    lines = stdout.splitlines()
                    if stderr or len(lines) != 2 or int(lines[0]) != expected:
                        raise RuntimeError(f"invalid observation: {stdout!r} {stderr!r}")
                    samples.append(int(lines[1]))
                result["rows"].append({"mode": mode, "members": count,
                                       "expected_key_bytes": expected,
                                       "input_sha256": sha(data), "ns": samples,
                                       "median_ns": statistics.median(samples)})
                print(mode, count, result["rows"][-1]["median_ns"], flush=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
