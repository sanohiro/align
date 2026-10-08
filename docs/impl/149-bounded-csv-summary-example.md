# Bounded typed CSV summary example

Status: **IMPLEMENTED 2026-10-08 — existing-module composition; Linux/macOS qualified.**

The shipped `pkg.csv` decodes in-memory UTF-8 into typed columns, but its vendorable
tree has no runnable application. Add `apps/csv/main.align` using the existing
package, regular-file admission, fallible buffers, CLI and checked stdout writes.
No language, compiler, runtime, package API or ownership strategy changes.

## Application policy and closure

| Boundary | Exact policy and acceptance |
| --- | --- |
| Entry and schema | `main(args: array<str>) -> Result<(), Error>`. Private `Score { active: bool, score: i32 }`; required named header, existing extra-column projection and scalar grammar. Missing, duplicate, invalid or out-of-range selected fields fail. |
| CLI | `--file` str defaults empty and is required; `--max-input-bytes` i64 defaults 8388608, admitted in 0..67108864; `--max-rows` i64 defaults 100000, admitted in 0..1000000; `--crlf` and `--help` bool default false. Parse, then help, then nonempty path, byte cap, row cap, before opening. LF by default; `--crlf` selects CRLF explicitly, with no inference. |
| Input | `fs.open_regular` returns one owned reader; ordinary symlinks are allowed, nonregular objects are rejected without waiting for a FIFO writer. The private helper consumes it. Allocate the complete maximum with `buffer.try_new(maximum)?`, then one 65536-byte read window with `buffer.try_new(65536)?`. Return either constructor's original error before reading. |
| Bound and encoding | Maintain `0 <= document.len() <= maximum`; reject a count above `maximum - document.len()` with `Error.Code(-1)` before append. Each admitted chunk is copied once into already reserved storage. Exact cap requires EOF; a rejection can observe one window beyond the cap. Convert the complete document with `bytes().as_str()?` only after EOF, allowing UTF-8 sequences and quoted records across read boundaries. No stable-content, time, RSS or whole-program OOM-recovery promise. |
| Decode and cleanup | In one named arena call `csv.decode` with `Header.Present`, the explicit line ending and row cap. Map `csv.Error.Invalid` to builtin `Error.Invalid`, `LimitExceeded` to `Error.Code(-1)`. Original open/read/write errors propagate. Existing CSV validation precedes its arena allocation; existing arena OOM policy remains. The primitive-only SoA depends on the arena. Reader, both buffers, CLI owners, report string and arena retain ordinary cleanup on success and early error. |
| Summary | Emit exactly `rows=N\nactive=N\nscore-sum=N\n`. Count all rows and active rows; widen each active i32 score to i64 before summing. At most 1000000 rows makes the sum representable even for every i32 extremum. Build the small report only after full admission/decode, then write through one bound stdout writer with `?`. Output failure may leave a prefix; input/parse failures emit no summary. |
| Owner | Extend the existing `pkg_csv` target with actual checked-in application source and canonical package files. Whole/per-unit checks and execution cover ordinary/header-only/reordered/extra-column/BOM/quoted inputs, both line endings, signed extrema, windows and UTF-8 splits, byte and row caps, malformed grammar/UTF-8/schema, CLI validation, regular-file errors, constructor-error propagation and stdout refusal. Reuse the established exclusive ArtifactStage and file-backed child pattern, with one deadline and immediate kill/reap guard. No new shared harness or test binary. |
| Documentation | Guide23 English/Japanese links to runnable source, exact CLI/defaults, schema/output and error/limit policy. No duplicate package-design/spec promise. HANDOFF records the capability once after qualification. |

This is one application composition, not a new streaming CSV parser. Existing
plan135 native payload/header failpoints prove buffer-constructor refusal; a
typed constructor-Err substitution in actual application source independently
owns propagation through this consumer. Existing `pkg_csv` owners retain package,
generic, lifetime and ABI coverage. No performance claim or benchmark is added.

## Qualification

The existing target's ten tests pass on macOS ARM64 and Linux ARM64.
`bounded_csv_summary_example_admission_and_output` compiles the unmodified app
through both whole-program and per-unit paths and applies the same input/error
matrix to both executables. The signed-extrema cases require sums outside i32;
64KiB boundary cases use exact byte caps; a UTF-8 scalar and a quoted multiline
field cross windows. Invalid limits win over a missing path, while valid limits
retain NotFound. FIFO/directory rejection, ordinary symlink success and read-only
stdout refusal retain their exact normal error exits and reports.

`bounded_csv_summary_example_propagates_buffer_refusal` independently substitutes
each of the two constructors with a typed `Error.Code(12)` result and requires
that original error with no output for empty and nonempty input. This is source
composition evidence, not a new allocator failpoint or physical-memory claim.
The documented CLI build and sample input also produce the exact three output
lines. English/Japanese command, CSV and output blocks agree.

```text
scripts/cargo.sh test -p align_driver --test pkg_csv
```
