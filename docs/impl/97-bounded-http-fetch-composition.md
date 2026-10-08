# Bounded HTTP binary receive composition

This supplies a runnable `std.cli` / `std.http` / `std.io` composition through
existing contracts. The language, native ABI, transport, allocation policy and
library signatures remain unchanged. The shipped raw receive stream already
provides fixed caller storage and de-framed bytes; an additional download API or
HTTP package is unnecessary.

## Example contract

`examples/http_fetch.align` writes one GET response body to stdout. Its existing
CLI parser accepts `--url` (default empty), `--max-body-bytes` (default 67108864),
`--timeout-ns` (default 30000000000), and `--help` (default false).
Parse first; help writes usage plus the additional LF through checked stdout
writes, before numeric validation or network/input I/O. Output errors propagate.
Otherwise reject empty URL, then a body cap outside 1..1073741824, then a
nonpositive timeout, with `Error.Invalid`, before creating the client.
URL encoding, embedded NUL and syntax use existing request admission.

Set the client timeout and positive cumulative decoded-body cap before issuing
the request. Accept only final status 200; another status returns
`Error.Invalid` before stdout body output. There is no redirect, retry policy,
content-type restriction, decoding, authentication or URL discovery in the example.
Existing constructor and transport errors propagate.

Allocate one explicit 65536-byte output buffer after successful status admission.
Use `buffer.try_new(65536)?` so constructor errors propagate before reading;
then bind one unbuffered stdout writer. Repeated `stream.read(out)` overwrites
the buffer without growth; a positive result is written through
`output.write(out.bytes())` before the next read. Zero means
exact HTTP body completion. Preserve every binary octet and expose no chunk
framing or UTF-8 conversion. Body-cap errors remain `Error.Code(-1)`; framing,
timeout and output errors retain the existing error model.

Exit success means complete input and successful writes. A late failure may leave
a body prefix on stdout: redirection is not atomic file publication. Help is text,
so do not combine it with binary output redirection. The HTTP timeout applies
independently to connect/send/each transport receive; it does not bound DNS,
stdout writes, or total execution. A slowly progressing peer can take longer
than one timeout. The body cap limits decoded payload, not all wire metadata.

Named parsed/client/request/stream/buffer/writer owners preserve existing view lifetimes,
request consumption and ordinary cleanup on success, status refusal and `?`.
The buffer and writer are reused, and the application never accumulates the complete body.
The writer shell is created once before the first read, including empty bodies;
its Drop never closes stdout. Writes remain unbuffered and checked individually.
The existing stream owns its separately bounded head and transport scratch.
This is neither an end-to-end zero-copy claim nor a new latency/RSS promise.
There is no new artifact/cache identity or prerequisite milestone.

## Implementation and acceptance

| Obligation | Implementation | Existing owner target |
| --- | --- | --- |
| Exact CLI defaults and validation order before network/output | main, existing command/parsed values | http_read_stream: actual example help/invalid-argument matrix with a bound, unaccepted listener |
| Status admission before binary output | final stream.status check | Same target: non-200 final responses and malformed/truncated heads |
| Binary preservation and exact completion | fixed buffer, read/write loop | Same target: fixed/chunked/close-delimited/interim responses, all octet values and multiple windows |
| Incremental output with a single reused window | write before the next read | Same target: first body prefix observed while the response remains incomplete |
| Positive cumulative cap and failure propagation | client cap and `?` | Same target: exact/exceeded known and unknown lengths, truncated body, stalled head/body and stdout failure |
| Unavailable read window | fallible construction before the first read | Same target: actual source with an injected constructor Err requires its original code and no output for empty/nonempty bodies; plan148 |
| Ordinary source and native cleanup | source compiled from the checked-in example; fixture, child and socket owners | Same target: actual alignc build/run, exclusive ArtifactStage, bounded process-group kill/reap and socket waits |
| Guide parity | chapter 18 English and Japanese | Author ledger-to-prose pass and syntax/execution owner |

No compiler safety/ABI strategy changes, so no cross-layer closure matrix or
additional design gate is needed. Complete an author-side obligation-to-diff
pass and one fresh full-diff preflight review. Reuse `http_read_stream`; add no
test binary or shared harness modification. Benchmarks are not required because
the example changes no native performance path or resource promise.

The complete existing http_read_stream target passes on macOS Apple Silicon and
Linux ARM64: eleven owners, including the three new runnable-example owners.
The source check and author obligation-to-diff / bilingual guide-link pass are
complete. Existing whole/per-unit carrier and native protocol owners retain
their scope; the example uses the real compiler's ordinary build path.
