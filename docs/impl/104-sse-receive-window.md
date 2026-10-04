# SSE payload receive-window reuse

The shipped next_sse feeds one de-framed source byte at a time. The same one-
byte output also controls fixed/close-delimited native read requests, despite
an already-owned 32KiB scratch. A post-head 32KiB event uses 32775 read calls in
both modes, versus 2 in chunked mode. Baseline local macOS/Linux observations
and actual Linux client-thread read counts are retained outside the repository
in align-sse-receive-evidence. This selects an implementation refinement of
[std.http's streaming ledger](std-design/http.md).
No public API, ABI, compiler representation or ownership strategy changes.

## Implementation closure

| Cell | Implementation and required owner |
| --- | --- |
| Transport selection | Keep one decoder/read owner. Supply remaining SSE source allowance separately from its one-byte unpublished parser output. Fixed/close-delimited reads fill at most the existing 32KiB scratch within that remaining allowance; a fixed body's existing one-byte residual completion probe remains when its known body fits. Chunked/trailer receive sizing retains its existing framing allowance and one-byte parser-output policy. Raw mode retains its caller-output-sized selection. A scoped test-only thread-local scalar probe observes actual requested/received sizes; `http_sse_receive_window_batches_payload_and_preserves_raw_and_event_boundaries` discriminates bulk SSE reads from bounded raw reads, preserves the next event, and checks the fixed completion probe plus actual co-read residual pooling. |
| Source admission | Each de-framed byte is charged and fed individually. Successful dispatch stops at its exact blank line, preserving following logical payload in scratch. Exact allowance probing stays zero-output/one-wire-byte plus the existing close-delimited peek. No next payload is read past that boundary. `http_sse_receive_window_never_reads_the_over_guard_payload` observes exactly the allowance received before refusal, and the existing exact/rejected-next owner retains Invalid versus body-cap Code(-1) precedence. |
| Errors and state | Fixed/chunked/close, head co-read and fragmented input, output/body/work caps, incomplete EOF, timeout, malformed framing, committed ID/retry and stable terminal errors keep their existing owner behavior. Run native http_sse, https_sse and streaming-framing owners, plus actual raw/fetch/SSE-watch and borrowed-event driver owners. No new parser or output/state commit path. |
| Lifetime, construction and cleanup | Existing client/request/stream/output ownership and ABI remain. No new allocation, buffer resize, source lifetime extension, handle or pool policy. Finite completion, residual scratch/TLS exclusion, early event return and ordinary Drop use unchanged owners. New fixtures retain immediate native handle guards, bounded socket/channel/accept operations and joined peer cleanup even on assertion failure. Existing compiler/control/import/interface owners remain applicable. |
| Measurement | Five baseline/candidate samples per framing assert byte equality; count actual native client-thread reads separately from uninstrumented timing. Observe unaffected chunked mode alongside fixed/close modes. This is a local receive-call refinement, with no fixed speedup, RSS or end-to-end throughput promise. An old output-sized-selection mutation must fail the receive-width oracle; no benchmark is a correctness gate. |

The author pass maps the unchanged public ledger to this private selection,
existing owners and the two new discriminating native owners. No public contract,
ABI, ownership-safety strategy or compiler-layer change is introduced, so one
fresh full-diff preflight review closes the boundary rather than reopening the
reviewed HTTP design. The capability remains one native producer/consumer path.

## Qualification

The native ordinary SSE 16, streaming-framing 2 and driver raw/fetch/watch 14 plus
borrowed-SSE 8 owners pass on macOS ARM64 and Linux ARM64. Linux also passes both
existing HTTPS/SSE owners and x86_64 runtime Clippy. Removing the new receive
selection fails the fixed-body width oracle at 1 instead of 32768; source was
restored and both new owners pass afterward. A post-head fixed completion case
checks its extra wire-byte request and excludes pooling only when a residual
byte was actually co-read, preserving arbitrary short-read behavior.

For the same 32KiB post-head event, actual Linux reads fall from 32775 to 2 in
fixed/close modes; chunked remains 2. Linux paired medians change from about 9ms
to 0.65–0.69ms in those modes while chunked stays about 0.70ms. Interleaved macOS
frozen-artifact samples put fixed/close around 0.6–0.7ms versus 11–12ms, with
chunked remaining within baseline spread. An initial common macOS timing rise
was not reproduced and remains retained with the continuation observations.
No universal speedup, source-to-stdout latency or RSS guarantee follows.

The two native macOS HTTPS/SSE fixtures fail during request_stream on unchanged
main and the candidate, before SSE body receive. Their baseline logs are retained;
they are not claimed as locally qualified or repaired. Linux closes the common
TLS body path; no unrelated TLS strategy is added to this refinement.

