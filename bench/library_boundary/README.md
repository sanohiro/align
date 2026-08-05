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

`provenance` builds a deterministic 256-function caller-before-callee dependency chain whose final
exported function returns only parameter 1. This declaration order requires reverse-worklist
propagation rather than succeeding through one in-place source-order scan. It also validates a
deterministic 128-definition generic chain through 256 borrowing signatures. The same group includes
a 256-function, high-CFG fixture with three expression-valued branches per function. It reports:

- `summary-inference`: full frontend-check milliseconds per iteration plus the canonical serialized
  interface bytes, so inference cost and summary-size growth are recorded together.
- `import-validation`: semantic-import milliseconds per iteration, including the complete type-shape
  walk, borrow/growth fixed points, dependency-cycle check, and provenance-root validation.
- `mir-global-type-validation`: whole-program MIR-lowering milliseconds per iteration for a trivial
  body plus 512 concrete nominal roots, isolating the global type-domain/root/cycle preflight and
  its required metadata copies.
- `mir-nominal-link-validation`: whole-program MIR-lowering milliseconds per iteration for nominal
  structs/enums, repeated id-free source-shape twins, and one linked library, isolating the
  nominal/source-identity, enum-base, alignment, and link-name preflight.
- `canonical-source-shape-comparison`: whole-program MIR-lowering milliseconds per iteration for
  the deterministic repeated-source-shape workload used by the canonical comparator observer.
- `canonical-type-graph`: milliseconds to construct canonical semantic records for every function
  root in a 128-nominal graph.
- `mir-callable-namespace-validation`: whole-program MIR-lowering milliseconds per iteration for
  512 typed program declarations and 256 direct program targets.
- `mir-header-validation`: paired valid and malformed whole-program MIR-lowering milliseconds per
  iteration for a large function-header/signature fixture, isolating declaration/header validation
  and its canonical-empty failure path.
- `mir-continuation-lowering`: L2b-a2-ac whole-program MIR-lowering milliseconds per iteration and
  the fixture's total basic-block count, tracking the O(1) required-child continuation protocol.

L2b-b adds the `indirect-return` row to the same group after target-relative function-value
provenance lands.

The benchmark is a regression tracker, not a timing assertion. Record the command, compiler commit,
host, and output when comparing changes.
