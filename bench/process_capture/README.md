# Bounded process-capture probe

This local measurement compares the existing unbounded process capture with Request 11's exact
per-stream bounds. Each child writes the selected payload size to both stdout and stderr, so a
bounded row owns exactly `2 * limit` capture-layout bytes while it is live. Text and byte terminals
use the same capture engine; the text row additionally performs the completed-buffer UTF-8 checks.

Run it with:

```sh
bench/process_capture/run.sh
REPS=20 bench/process_capture/run.sh
```

The fixed consumer limits are 65,536 and 262,144 bytes. For each limit the harness measures bounded
text, bounded bytes, unbounded text, and unbounded bytes over identical child output. It validates
the exit code and exact stdout/stderr lengths outside the timing calculation for every repetition.
The report includes median throughput and the bounded maximum live capture-layout bytes. Unbounded
rows deliberately report no maximum because their growable `Vec` storage has no memory-bound
contract.

This is resource evidence, not a correctness gate. Pipe setup, fork/exec, and child generation are
included because they are part of the public terminal cost; compare rows at the same payload size.

Recorded Linux x86-64 run on 2026-08-14 (`REPS=5`, median):

```text
mode   bound      bytes/stream  median ms  MiB/s  max live capture layout
text   bounded           65536      2.489    50.2                    131072
bytes  bounded           65536      2.533    49.4                    131072
text   unbounded         65536      2.660    47.0                 unbounded
bytes  unbounded         65536      2.435    51.3                 unbounded
text   bounded          262144      2.710   184.5                    524288
bytes  bounded          262144      2.768   180.6                    524288
text   unbounded        262144      2.765   180.9                 unbounded
bytes  unbounded        262144      2.724   183.5                 unbounded
```
