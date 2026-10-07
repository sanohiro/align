#!/usr/bin/env python3
"""Bounded local before/after checking measurement (plan 21, items 11–22)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import statistics
import subprocess
import tempfile
import time


WORK_TIMEOUT_SECONDS = 20
CLEANUP_TIMEOUT_SECONDS = 5


def run(binary, command, source):
    child = None
    started = time.perf_counter()
    try:
        child = subprocess.Popen(
            [str(binary), command, str(source)],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
        try:
            stdout, stderr = child.communicate(timeout=WORK_TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired as error:
            raise RuntimeError(f"20-second budget: {binary.name} {command} {source.name}") from error
        return child.returncode, stdout, stderr, time.perf_counter() - started
    finally:
        if child is not None:
            # The leader may exit while descendants still own its pipes or work.
            # Retire the entire owned group regardless of the leader status.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.communicate(timeout=CLEANUP_TIMEOUT_SECONDS)


def source_text(kind, count):
    if kind in ("fixed", "loop-fixed"):
        lines = ["fn main() {", "  mut total := 0"]
        if kind == "loop-fixed":
            lines.extend(["  mut turn := 0", "  loop {", "    if turn == 2 { break }"])
        for index in range(count):
            lines.extend([f"  value{index} := [1, 2, 3, 4]",
                          f"  total = total + value{index}[0]"])
        if kind == "loop-fixed":
            lines.extend(["    turn = turn + 1", "  }"])
        return "\n".join(lines + ["  print(total)", "}"]) + "\n"
    lines = [
        "fn step(value: i64) -> Result<i64, Error> = Ok(value + 1)",
        "fn exercise() -> Result<i64, Error> {",
        "  mut total := 0", "  mut index := 0", "  loop {",
        "    if index == 2 { break }",
    ]
    previous = "index"
    for index in range(count):
        if kind == "match":
            expression = (f"match step({previous}) {{ Ok(value) => {{ value }}, "
                          "Err(error) => { return Err(error) } }")
        elif kind == "try":
            expression = f"step({previous})?"
        else:
            expression = f"{previous} + 1"
        lines.append(f"    value{index} := {expression}")
        previous = f"value{index}"
    lines.extend([
        f"    total = total + {previous}", "    index = index + 1", "  }",
        "  return Ok(total)", "}",
        "fn main() -> Result<(), Error> { print(exercise()?); return Ok(()) }",
    ])
    return "\n".join(lines) + "\n"


def stop(signum, _frame):
    raise SystemExit(128 + signum)


def main():
    # Normal termination signals unwind through the active child guard.
    for signum in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, stop)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--repeats", type=int, default=5, choices=range(1, 11))
    parser.add_argument("--llvm-parity", action="store_true",
                        help="also compare raw LLVM, including cold attributes and branch weights")
    parser.add_argument("--corpus", choices=("numeric", "storage"), default="numeric")
    args = parser.parse_args()
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    print(json.dumps({"binaries": {
        name: {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
        for name, path in binaries.items()
    }, "repeats": args.repeats, "corpus": args.corpus}), flush=True)
    # Both revisions consume the exact same path and bytes. Each check is a fresh process;
    # neither check command reuses a persisted frontend cache.
    with tempfile.TemporaryDirectory(prefix="align-escape-transfer-") as temporary:
        directory = Path(temporary)
        kinds = ("straight", "try", "match") if args.corpus == "numeric" else ("fixed", "loop-fixed")
        for kind in kinds:
            for count in (16, 128, 512):
                source = directory / f"{kind}-{count}.align"
                text = source_text(kind, count)
                source.write_text(text)
                outputs = [run(binary, "emit-mir", source) for binary in binaries.values()]
                if any(output[0] != 0 for output in outputs) or outputs[0][1:3] != outputs[1][1:3]:
                    raise RuntimeError(f"MIR/diagnostic parity failed: {source.name}: {outputs[0][2]!r} / {outputs[1][2]!r}")
                mir_digest = hashlib.sha256(outputs[0][1]).hexdigest()
                del outputs
                llvm_digest = None
                if args.llvm_parity:
                    outputs = [run(binary, "emit-llvm", source) for binary in binaries.values()]
                    if any(output[0] != 0 for output in outputs) or outputs[0][1:3] != outputs[1][1:3]:
                        raise RuntimeError(f"LLVM/diagnostic parity failed: {source.name}")
                    llvm_digest = hashlib.sha256(outputs[0][1]).hexdigest()
                    del outputs
                for command in ("check", "check-per-unit"):
                    samples = {name: [] for name in binaries}
                    checked_output = {}
                    for round_number in range(args.repeats + 1):
                        order = list(binaries) if round_number % 2 == 0 else list(reversed(binaries))
                        for name in order:
                            code, stdout, stderr, elapsed = run(binaries[name], command, source)
                            if code != 0 or stderr:
                                raise RuntimeError(f"{name} {command} {source.name}: exit={code}, {stderr!r}")
                            observed = stdout.decode("utf-8")
                            if "ok: checked " not in observed or checked_output.setdefault(name, observed) != observed:
                                raise RuntimeError(f"unstable checked-work evidence: {name} {source.name}")
                            if round_number:
                                samples[name].append(elapsed)
                    if checked_output["baseline"] != checked_output["candidate"]:
                        raise RuntimeError(f"checked-work parity failed: {source.name} {command}")
                    print(json.dumps({
                        "kind": kind, "values": count, "source_bytes": len(text.encode()),
                        "source_lines": len(text.splitlines()), "command": command,
                        "source_sha256": hashlib.sha256(text.encode()).hexdigest(),
                        "mir_sha256": mir_digest, "samples_seconds": samples,
                        "checked_output": checked_output,
                        **({"llvm_sha256": llvm_digest} if llvm_digest is not None else {}),
                        "median_seconds": {name: statistics.median(values) for name, values in samples.items()},
                    }), flush=True)
        for label, text in (
            ("local-view", 'fn escaped() -> str { owner := "text".clone(); return owner }\nfn main() {}\n'),
            ("readonly-bytes", 'fn main() { mut bytes := "text".bytes(); bytes[0] = 1 }\n'),
        ):
            source = directory / f"invalid-{label}.align"
            source.write_text(text)
            for command in ("check", "check-per-unit"):
                outputs = [run(binary, command, source) for binary in binaries.values()]
                if any(output[0] == 0 for output in outputs) or outputs[0][:3] != outputs[1][:3]:
                    raise RuntimeError(f"rejection/diagnostic parity failed: {source.name} {command}")
                print(json.dumps({"rejection": label, "command": command, "diagnostics_equal": True}), flush=True)


if __name__ == "__main__":
    main()
