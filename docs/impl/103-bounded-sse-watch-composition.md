# Bounded SSE receive composition

This supplies one runnable client over existing std.cli, std.http and std.io.
The multimodal reference already exposes job progress as SSE; the shipped
receiver implements framing, WHATWG event parsing and caller-buffer views.
No new parser, package, native ABI, ownership strategy or language surface is
needed. GPU/engine integration and K1 remain outside this capability.

## Exact example contract

| Surface | Record |
| --- | --- |
| Program and inputs | examples/http_sse_watch.align. Existing CLI flags: --url (str, empty), --max-events (i64, 1024), --event-bytes (i64, 65536), --max-body-bytes (i64, 67108864), --timeout-ns (i64, 30000000000), --help (bool, false). No environment/configuration input, hidden reconnect, timer or model state. |
| Admission and order | Parse first. Help writes usage plus the additional LF through checked stdout writes before numeric validation or input/network I/O. Stdout errors propagate. Otherwise reject empty URL, event count outside 1..1048576, event window outside 1..1048576, body cap outside 1..1073741824, then nonpositive timeout, with Error.Invalid. Create the explicit output buffer with buffer.try_new(event_bytes)? before network I/O; constructor errors propagate unchanged (plan148). Existing request URL encoding/NUL/syntax admission remains. |
| Request and interpretation | Configure the existing client timeout and positive cumulative decoded-body cap; issue GET with Accept: text/event-stream. After successful request/transport admission, only final status 200 enters the event loop; other statuses are Invalid before payload output. Constructor errors, including a declared body over the cap, precede that status check. Interpret the caller-selected endpoint as SSE with the existing consuming stream.sse operation, without content-type validation or application schema decoding. |
| Output and completion | Repeated events.next(out) yields existing Result<Option<http_sse_event>,Error>. Bind one stdout writer. For each Some, write event.data followed by exactly one LF through its checked writes before the next call. Embedded LFs remain; this is data text, not canonical JSONL or retained event boundaries. IDs, event names and retry metadata are not printed. Stop successfully at the requested number of dispatched events or clean stream EOF. Empty-data dispatched events print one LF; control-only blocks do not count. Existing incomplete final-block EOF behavior remains. Reaching the count intentionally stops before complete HTTP input and closes the checked-out stream through ordinary Drop. |
| Errors and resource bounds | HTTP/SSE explicit body/output cap failures preserve Error.Code(-1); SSE source-work overflow is Error.Invalid, with the existing body-cap precedence at the same source boundary. Framing, timeout, transport and stdout errors propagate through ?. Late errors can leave a stdout prefix. The output window caps all materialized event text, including the unprinted name/id, not only data. Existing transport/head, SSE source-work and persistent control storage bounds remain; the application never accumulates all events. A per-operation transport timeout does not bound DNS, stdout or total watch duration. No end-to-end zero-copy, heap/RSS or latency claim. |
| Ownership and identity | Parsed text stays bound through request construction. The client owns its pool; request is consumed, raw stream transfers into the SSE stream. Each event's views borrow the current output generation and are consumed before its next mutation. Ordinary success/error/early-count paths drop once. No nominal/IR/ABI/cache/format change or later-milestone prerequisite. |
| Acceptance and sources | Reuse the existing guarded runnable HTTP fixture in http_read_stream by making its source name explicit; no new test binary/shared harness. Actual checked-in source is compiled and executed. Cover CLI/help admission before connection, fixed/chunked/close framing, event text including multiline/empty/control blocks, first output before response completion, count termination while peer stays open, exact/exceeded event/body limits, non-200, malformed/truncated framing, stalled receive and stdout refusal. Guide chapter 18 English/Japanese agree; HANDOFF changes once at completed capability. |

## Closure and proportional verification

An author obligation-to-diff pass closes the table against the example and its
parameterized executable owner. Existing http_sse_stream owns parser, dependent
carrier and field lifetime semantics; it is run alongside http_read_stream.
The fixture retains exclusive ArtifactStage ownership and immediately guarded,
deadline-bound child/process-group cleanup. Existing binary-fetch owners rerun
to qualify the shared leaf fixture refactor. No compiler/FFI/safety strategy is
changed, so this is not a new cross-cutting design gate. One independent full-
diff review, owner bundles on macOS/Linux and final-SHA preflight precede PR.
Benchmarks are unnecessary because no native performance path or new resource
promise changes.

## Implemented closure

The three actual-example owners pass within http_read_stream on macOS ARM64 and
Linux ARM64. They discriminate incremental output before HTTP completion,
count completion with an open peer, multiline/empty/control/incomplete blocks,
exact and exceeded event/body bounds, unprinted-ID capacity, precise normal
error reports, late failure after an observed first event, and an injected
constructor Err before any connection. Plan148 updates that control to require
the original allocation error code; plan135 owns actual native refusal. The
shared fixture refactor preserves all eleven prior raw/fetch owners, and all
eight existing SSE lifetime/interface owners pass on both hosts.

No timing/allocation-count/RSS improvement is inferred from this example.
