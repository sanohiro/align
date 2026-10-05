# Bounded binary SHA-256 example

Status: **IMPLEMENTED 2026-10-05 — existing-module composition; Linux/macOS qualified.**

Select an executable ordinary Align composition for artifact/file or stdin digests. Existing incremental crypto, reader, writer, CLI and hex encoding suffice; no library/native/compiler API change.

| Surface | Exact sample policy |
| --- | --- |
| Entry | examples/file_sha256.align; main(args: array<str>) -> Result<(), Error>. |
| CLI | --file str defaults empty (stdin); --max-input-bytes i64 defaults to 1073741824; --help bool defaults false. Parse first; help reports usage; require maximum in 0..2305843009213693951 before any filesystem/input/crypto work. |
| Input | Empty file flag selects io.stdin; nonempty path uses fs.open. All file bytes are binary. No UTF-8 conversion. Private helper consumes reader. One reused 65536-byte buffer; require capacity 65536 or return Invalid before the first read/update. Each read overwrites it. |
| Cap | Maintain `0 <= seen <= maximum`; reject n>maximum-seen with Error.Code(-1) before update. A rejection can have read one full window past the byte cap. At exact cap, read until EOF or reject further data. Byte cap does not bound blocking read/wait time. |
| Digest | Existing crypto.sha256_stream/update/finish policy; update borrows a window for the call, finish consumes context and owns 32 bytes. Existing provider/allocation/cumulative-length hard failures remain. |
| Output | No digest bytes until input completes. Hex encode 32 bytes, then one bound io.stdout writer emits lowercase 64 characters followed by newline with checked writes. Output failure may leave a digest prefix. Help follows existing print-usage conventions. |
| Errors | Parse/invalid cap Invalid; input open/read and output errors propagate; cap Code(-1). Native path/encoding admission remains std.fs-owned. Successful digest describes observed bytes, no stable snapshot/certified identity promise. |
| Owner | Existing m11_crypto_stream receives one parameterized runnable-example owner plus source whole/per-unit checking. Reuse ArtifactStage exclusive fixture and bounded child-owner strategy: one deadline at spawn, kill/reap guard before fallible post-spawn work, file-backed streams. No shared harness/new binary. |
| Acceptance | Empty, abc, NUL/non-UTF-8 and 65535/65536/65537/multiple windows; file/stdin equal independent SHA-256 goldens; zero/exact/exceeded caps; invalid cap before blocking FIFO open; unknown flag/help and missing path/directory read errors; readonly stdout; no digest before error. Local Linux/macOS qualifies existing surface. |
| Source agreement | Guide18 English/ja references runnable example and its limits; plan100 records sample policy and acceptance. Existing crypto design/spec remain unchanged. Record previous merged plan99 P2 in FINDINGS at capability start. |

Private helper and sample use existing Move/Drop, Result and loop rules. No new ownership/FFI/safety strategy: bounded test fixtures follow already-reviewed plan97's file-backed child pattern. Full preflight review includes the concrete child topology and owner semantics. No performance benchmark: fixed source window is a composition bound, no new engine/throughput/RSS claim.

## Closure and qualification

`bounded_file_sha256_example_binary_cli_and_errors` in the existing m11_crypto_stream
owner checks whole/per-unit source agreement and builds one executable. It runs
file/stdin empty and binary vectors, every 64KiB boundary, exact/exceeded/zero caps,
invalid caps and unknown flags before a blocking FIFO open, help without input,
missing-file and directory-read errors, and a read-only stdout refusal. Independent
Python hashlib vectors own exact SHA-256 output. File-backed streams, exclusive
ArtifactStage cleanup and the immediate bounded child guard own fixture lifecycle.
All negative cases require the expected normal exit and exact standard error report,
so incorrect error reports and signal termination cannot satisfy the owner.
The cap-to-Invalid mutation is rejected by that owner.
The degraded-window owner in the same target substitutes buffer(0) in the actual
example and requires normal Invalid with no digest for empty/nonempty file and
stdin input (plan114), independently of host memory pressure.

The owner passes on macOS ARM64 and Linux ARM64. There is no native/compiler change
or performance benchmark. The runnable program's byte cap does not bound input
waiting; the test runner's deadline bounds its own children. Evidence lives outside
the repository in align-file-sha256-evidence.

```text
scripts/cargo.sh test -p align_driver --test m11_crypto_stream bounded_file_sha256_example
```
