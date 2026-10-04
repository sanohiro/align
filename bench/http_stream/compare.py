#!/usr/bin/env python3
"""Ten balanced paired runs, retaining raw rows and premeasured repeatability."""
import csv
import hashlib
import json
import os
from pathlib import Path
import signal
import statistics
import subprocess
import sys


def run(binary, destination):
    with destination.open("x") as output:
        child = subprocess.Popen([str(binary)], stdout=output, stderr=subprocess.STDOUT,
                                 start_new_session=True)
        try:
            code = child.wait(timeout=60)
        finally:
            if child.poll() is None:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass  # The child can finish between poll and killpg.
                child.wait()
        if code != 0:
            raise RuntimeError(f"benchmark failed; evidence: {destination}")
    with destination.open() as source:
        rows = list(csv.DictReader(source))
    assert len(rows) == 20 and all(row["mode"] == "timing" for row in rows)
    return {(row["http11"], row["event"], row["payload_bytes"]): row for row in rows}


def value(row, metric):
    if metric == "wire_bytes_per_second":
        return int(row["received_bytes"]) * 1e9 / int(row["elapsed_ns"])
    text = row[metric]
    return None if text == "NA" else int(text)


def summarize(output, phases):
    summary = []
    for key in phases["comparison"][0][0]:
        for metric in ["first_send_ns", "p50_ns", "p95_ns", "first_payload_ns", "wire_bytes_per_second"]:
            differences = {}
            values = {}
            for phase, pairs in phases.items():
                samples, left, right = [], [], []
                for pair in pairs:
                    a, b = [value(pair[arm][key], metric) for arm in [0, 1]]
                    if a is not None and b is not None:
                        samples.append(b - a)
                        left.append(a)
                        right.append(b)
                differences[phase] = samples
                values[phase] = (left, right)
            if not differences["comparison"]:
                continue
            envelope = max(abs(delta) for delta in differences["repeatability"])
            delta = statistics.median(differences["comparison"])
            baseline, candidate = [statistics.median(arm) for arm in values["comparison"]]
            slower = -delta if metric == "wire_bytes_per_second" else delta
            summary.append({"http11": key[0], "event": key[1], "payload_bytes": int(key[2]),
                            "metric": metric, "baseline_median": baseline, "candidate_median": candidate,
                            "paired_median_delta": delta,
                            "candidate_baseline_ratio": candidate / baseline if baseline > 0 else None,
                            "premeasured_repeatability_envelope": envelope,
                            "outside_repeatability": slower > envelope})
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    failures = [row for row in summary if row["payload_bytes"] <= 3 and row["outside_repeatability"]]
    print(json.dumps({"evidence": str(output.resolve()), "token_rows_outside_repeatability": failures}, indent=2))
    return bool(failures)


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--summarize":
        output = Path(sys.argv[2])
        phases = {}
        for phase in ["repeatability", "comparison"]:
            pairs = []
            for pair in range(10):
                arms = {}
                for arm in [0, 1]:
                    with (output / f"{phase}-{pair:02}-{arm}.csv").open() as source:
                        rows = list(csv.DictReader(source))
                    assert len(rows) == 20 and all(row["mode"] == "timing" for row in rows)
                    arms[arm] = {(row["http11"], row["event"], row["payload_bytes"]): row for row in rows}
                pairs.append(arms)
            phases[phase] = pairs
        return summarize(output, phases)
    if len(sys.argv) != 4:
        raise SystemExit("usage: compare.py BASELINE_BINARY CANDIDATE_BINARY NEW_OUTPUT_DIRECTORY")
    baseline, candidate = [Path(path).resolve(strict=True) for path in sys.argv[1:3]]
    output = Path(sys.argv[3])
    output.mkdir()  # Exclusive ownership; never overwrite another run's evidence.
    manifest = {"host": list(os.uname()), "pairs": 10, "binaries": {
        name: {"path": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
        for name, binary in [("baseline", baseline), ("candidate", candidate)]}}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    run(baseline, output / "warm-baseline.csv")
    run(candidate, output / "warm-candidate.csv")
    phases = {}
    for phase, binaries in [("repeatability", (baseline, baseline)), ("comparison", (baseline, candidate))]:
        pairs = []
        for pair in range(10):
            arms = {}
            for arm in ([0, 1] if pair % 2 == 0 else [1, 0]):
                arms[arm] = run(binaries[arm], output / f"{phase}-{pair:02}-{arm}.csv")
            assert arms[0].keys() == arms[1].keys()
            pairs.append(arms)
            print(f"{phase}: pair {pair + 1}/10 complete", flush=True)
        phases[phase] = pairs
    return summarize(output, phases)


if __name__ == "__main__":
    sys.exit(main())
