# Fallible read windows in streaming examples

Plan135 supplies `buffer.try_new`, whose success owns the exact requested read
window and whose payload/header refusal returns `Error.Code(ENOMEM)`. The four
streaming examples still use best-effort construction followed by a capacity
check that maps a refused window to `Error.Invalid`. Adopt the shipped fallible
constructor through ordinary `?`; no new API or allocation policy is needed.

## Example contract and closure

| Boundary | Implementation and acceptance |
| --- | --- |
| File copy | `examples/file_copy.align` acquires `buffer.try_new(65536)?` after opening the source and exclusively creating the destination. Constructor error propagates before any read/write; the new destination remains empty. Existing argument, alias refusal, binary completion and flush behavior remain. `file_copy_examples` owns actual-source normal and refused-constructor cases. |
| SHA-256 | `examples/file_sha256.align` acquires the same window after reader/digest creation and before the first read/update. Constructor error produces no digest, including empty file/stdin input; existing owners drop normally. `m11_crypto_stream` owns source whole/per-unit agreement, binary vectors and exact error/no-output results. |
| HTTP fetch | `examples/http_fetch.align` acquires the same window after successful request and status-200 admission, before binding stdout or reading the body. Constructor error produces no output for empty/nonempty bodies. Existing HTTP status/error ordering remains. `http_read_stream` owns actual-source byte, framing, limit and failure cases. |
| SSE watch | `examples/http_sse_watch.align` acquires `buffer.try_new(event_bytes)?` after existing ordered CLI validation and before client/request/connection creation. Refusal returns its original error without connecting. The same `http_read_stream` owner retains event/count/limit/late-prefix behavior. |
| Failure composition | Each existing degraded-window fixture instead substitutes the unique fallible call with a typed `Result<buffer, Error>` helper returning `Error.Code(12)`, the supported-host ENOMEM value. This independently tests source-level propagation and phase boundaries without host memory exhaustion or a runtime test hook. Plan135's existing native payload/header failpoint owners supply the actual constructor-refusal/cleanup proof. Normal fixtures still compile unmodified checked-in sources. No new binary or shared harness. |
| Ownership and allocation | One explicit Move buffer is created on success and reused exactly as before. `?` uses existing Result/Drop lowering; no new borrow, view, IR, FFI, generic/interface or cache strategy. Other allocations and subsequent buffer growth retain their existing policies. No whole-program recoverability, physical-memory, latency or RSS promise. |
| Guides and plans | English guide chapters02/13/18 use the same constructor and ordinary error propagation; Japanese mirrors agree. Syntax-check all six edited snippets. Update only the changed window-admission promises in plans97/100/103/110/114; plan135 remains the constructor authority. |

The four consumers form one useful capability with the same admission change.
Run their three existing owner targets on macOS/Linux, perform an author-side
matrix-to-diff pass and one fresh independent preflight review, then the normal
SHA-bound gate. No new compiler/runtime design or benchmark is required.

All 24 tests across `file_copy_examples`, `m11_crypto_stream` and
`http_read_stream` pass on macOS and Linux ARM64. The six edited guide snippets
pass `alignc check`, and each English/Japanese code pair is identical.
