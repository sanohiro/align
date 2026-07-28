# Library-boundary prerequisite benchmarks

This cumulative harness owns the measurements named by
`docs/impl/17-library-boundary-prerequisites.md`. Run one slice group through `run.sh`:

```text
bench/library_boundary/run.sh interface
```

`interface` builds a deterministic 512-function exported surface with explicit L2a parameter modes
and empty return-borrow/return-region summaries. It reports:

- `interface-size`: canonical serialized artifact bytes;
- `decode-throughput`: repeated checked deserialization throughput in MiB/s.

`provenance` builds a deterministic 256-function reverse dependency chain whose final exported
function returns only parameter 1. L2b-a1 reports:

- `summary-inference`: full frontend-check milliseconds per iteration plus the canonical serialized
  interface bytes, so inference cost and summary-size growth are recorded together.

L2b-b adds the `indirect-return` row to the same group after target-relative function-value
provenance lands.

The benchmark is a regression tracker, not a timing assertion. Record the command, compiler commit,
host, and output when comparing changes.
