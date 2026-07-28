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

The benchmark is a regression tracker, not a timing assertion. Record the command, compiler commit,
host, and output when comparing changes.
