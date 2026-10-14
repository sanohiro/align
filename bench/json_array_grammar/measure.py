#!/usr/bin/env python3
"""Local static-link control using the unchanged repository JSON benchmark kernels."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT / "bench"))
import native_probe

MAIN = '''import decode
import columns
import core.json
import std.fs
import std.time
fn main(args:array<str>)->Result<(),Error> {
  data:=fs.read_file(args[1])?
  expected:i64:=json.decode(args[2])?
  rows:i64:=json.decode(args[3])?
  if decode.decode_full_len(data)!=rows || decode.decode_proj_len(data)!=rows || columns.agg_len(data)!=rows || columns.agg_proj_len(data)!=rows {return Err(Error.Invalid)}
  mut mode:i64:=0
  loop {
    if mode==4 {break}
    mut iteration:i64:=0
    loop {
      if iteration==40 {break}
      start:=time.instant()
      value:=if mode==0 {decode.decode_full(data)} else {if mode==1 {decode.decode_proj(data)} else {if mode==2 {columns.agg(data)} else {columns.agg_proj(data)}}}
      elapsed:=time.instant()-start
      if value!=expected {return Err(Error.Invalid)}
      print(template "{mode} {iteration} {elapsed}")
      iteration=iteration+1
    }
    mode=mode+1
  }
  return Ok(())
}
'''

def digest(data):
    return hashlib.sha256(data).hexdigest()

def run(command, stage):
    with (stage / "stdout").open("wb") as output:
        native_probe.run_phase("probe", command, stdout=output)
    return (stage / "stdout").read_text()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("compiler", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--source-id", required=True)
    args = parser.parse_args()
    compiler, output = args.compiler.resolve(), args.output.resolve()
    for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, native_probe.record_signal)
    sources = {
        "decode": "module decode\n" + (ROOT / "bench/json_decode/kernel.align").read_text(),
        "columns": "module columns\n" + (ROOT / "bench/json_soa/kernel.align").read_text(),
        "main": MAIN,
    }
    result = {"source_commit": args.source_id, "host": platform.platform(),
              "compiler_sha256": digest(compiler.read_bytes()),
              "runtime_sha256": digest(compiler.with_name("libalign_runtime.a").read_bytes()),
              "mode": "static runtime; release generated code, native CPU, no runtime LTO, cache off; 3 fresh processes x 40 samples; input I/O and count checks excluded; aggregate verified every iteration",
              "source_sha256": {name: digest(source.encode()) for name, source in sources.items()},
              "rows": []}
    previous = Path.cwd()
    stage = Path(tempfile.mkdtemp(prefix="align-json-grammar-static-")).resolve()
    cleanup_safe = True
    try:
        os.chdir(stage)
        os.environ.update(ALIGNC_CACHE="off", TMPDIR=str(stage))
        for name, source in sources.items():
            (stage / (name + ".align")).write_text(source)
        run([str(compiler), "build", str(stage / "main.align"), "--profile", "release",
             "--target-cpu", "native", "--no-rt-lto", "--jobs", "1"], stage)
        for count in (10000, 100000, 1000000):
            state, expected, members = 0x9e3779b97f4a7c15, 0, []
            for i in range(count):
                state = (state * 6364136223846793005 + 1442695040888963407) & ((1 << 64) - 1)
                pay, score, active = (state >> 33) % 1000, (state >> 20) % 500, (state >> 40) & 1 == 0
                if active: expected += pay
                members.append(f'{{"active":{str(active).lower()},"pay":{pay},"score":{score},"extra":{i}}}')
            data = ("[" + ",".join(members) + "]").encode()
            (stage / "input.json").write_bytes(data)
            row = {"members": count, "expected_sum": expected, "input_sha256": digest(data), "samples": []}
            del data, members
            for sample in range(3):
                observed = run([str(stage / "main"), str(stage / "input.json"), str(expected), str(count)], stage)
                lines = [[int(value) for value in line.split()] for line in observed.splitlines()]
                if len(lines) != 160 or any(values[:2] != [i // 40, i % 40] or len(values) != 3 or values[2] < 0 for i, values in enumerate(lines)):
                    raise RuntimeError("invalid measurement output")
                row["samples"].append([values[2] for values in lines])
                print(count, sample, "validated", flush=True)
            result["rows"].append(row)
    except native_probe.CleanupError:
        cleanup_safe = False
        print(f"Preserving scratch after child cleanup failure: {stage}", file=sys.stderr)
        raise
    finally:
        os.chdir(previous)
        if cleanup_safe: shutil.rmtree(stage)
    native_probe.check_signal()
    output.write_text(json.dumps(result, indent=2) + "\n")

if __name__ == "__main__":
    main()
