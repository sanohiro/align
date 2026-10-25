# Bounded ordinary HTTP responses

AM-R22 selects a finite ordinary response operation. The current media source
at private commit `f9948006c965ead29865d0008c86061919e21cf0` closes HEAD
without output and sends other replies through bounded streams. LLM's ordinary
error replies and streaming responses remain valid; voice has no matching HTTP
consumer. Consumer adoption is external. AM-R23 connection observation is a
separate capability, and neither operation supplies model cancellation.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Source | `ctx.respond_timeout_ns(rb: response_builder, timeout_ns: i64) -> Result<(), Error>`; Impure; `ctx: http_request_ctx`. Both Move handles are consumed on success and every runtime error, as for `respond`. Receiver must be an admitted bound context place. No default argument. |
| Arguments | Evaluate receiver, builder and timeout once, in that order. Keep already evaluated owners valid until the call. An eager exit performs no response call or source nulling and leaves ordinary cleanup responsible for owners. A later argument cannot consume or replace an earlier owner. |
| Admission | Take ownership of non-null handles before returning any status. Validate `timeout_ns > 0`, both live handles, unspent context, then existing response validation. Zero and negative budgets are Invalid. Preserve the existing bodyless-status, status and caller-framing validation order. Invalid input performs no socket output and closes the context. Native aligned/live pointer validity remains the FFI caller's obligation. |
| Deadline | After scalar/handle admission, start exactly one monotonic total budget, before parking-lock acquisition and serialization. Check before and after each lock attempt, serialization, native setup, write, readiness wait and restoration, and immediately before parking/success. Expiration is Timeout and wins over a simultaneous native success/error after the budget starts. A deadline is not a hard real-time scheduling or allocator bound. Positive `i64::MAX` is valid. |
| HTTP | Reuse ordinary response serialization and validation. Exact uppercase HEAD sends no body and preserves the builder body's Content-Length; other method spellings retain ordinary behavior. HTTP/1.0, request Connection: close, residual pipelining, interim statuses and dead servers retain the existing close rules. Bodyless status restrictions are unchanged. |
| Progress and error | One budget covers the complete head/body and every partial write, EINTR and EAGAIN retry. Reuse the stream's checked partial-write/poll writer. No write or zero-time probe after observed expiration. Preserve native status mapping. Any failure closes exactly once; no retry response and no partial connection is parked. |
| Reuse | Hold the existing parking mutex across persistence selection, serialization, output and parking, but acquire it with deadline-checked try-lock attempts and bounded pauses. Poisoned mutexes retain the established recovery. Only a complete successful write, successful native-mode restoration, live server and existing keep-alive eligibility can park the fd. |
| Platform mode | Linux uses per-call MSG_DONTWAIT and SIGPIPE suppression, with no descriptor-mode change. Darwin uses the existing SIGPIPE-protected accepted socket; get flags and successfully set O_NONBLOCK before writing. Preserve unrelated flags. Before parking restore the original flags with checked F_SETFL even if no other work remains. Failed setup/restoration or expiration closes without further I/O or restoration. No fd alias is published. |
| Allocation and ownership | Existing builder body/header copies and one contiguous serialized response allocation remain explicit in the owning HTTP contract. No new buffer copy, handle, view, child allocation or retained input is introduced. Keep-alive uses the existing bounded parking set. Owners are nulled only after all operands complete, then runtime RAII owns both. |
| ABI / artifact identity | New HIR/MIR operation and `RuntimeKey::HttpRespondTimeoutNs`; `i32 align_rt_http_respond_timeout_ns(ptr ctx, ptr rb, i64 timeout_ns)`. HostState effects and consumed pointers match ordinary respond. No new type tag or wire format. Imported source bodies are reconstructed and independently checked using current interface records; existing compiler/runtime identity invalidates stale code. |
| Owner and prerequisite | std.http compiler/runtime, after shipped Plan90 timed writes and Plan240 authority. No later milestone or package prerequisite. |
| Agreement | This ledger, `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, Settled in `docs/open-questions.md`, English/Japanese `std-design/http.md`, checked-HIR and runtime ABI ledgers, and ABI declaration golden. No protocol encoding changes or new benchmark claim. |

## Implementation closure matrix

One capability spans source admission, independent replay, MIR lowering, ABI,
native deadlines and connection reuse. A native-only producer or compiler-only
method has no useful consumer and duplicates safety proof. If the complete
variant sweep and owners exceed 1,000 handwritten lines, keep this boundary:
splitting a dormant operation from its cleanup/deadline consumers increases
integration risk without an independently usable result.

| Axis | Implementation and discriminating owner |
| --- | --- |
| Formation, types, effects, malformed input | Sema plus independent checked-HIR validation; `http_response_budget` source arity/type/shared-borrow/effect negatives and forged HIR receiver/builder/timeout/result/effect mutations. Existing response validation remains the owner for unchanged framing. |
| Construction, move-in/out, source nulling, Drop, replacement, return | Existing context/builder owners, consuming move checker and staged consuming-argument lowering; source use-after-response, earlier-owner mutation in later operand, eager exits and native failed-response fd closure. Cross bound, fresh inline and conditional builders with a diverging timeout; nested consuming exits complete only their inner action. No new owner type. |
| if/match/else/?/map_err, branch/loop joins and early exits | Imported generic response wrapper and terminal-operand matrix in `http_response_budget`; validate both modes and verify no native action on divergence. Existing Result/Move joins cover unchanged representation. |
| Generic/interface, whole/per-unit/cache | Imported helper builds and reconstructed bodies in `http_response_budget`; same emitted ABI. Existing format roundtrip/cache invalidation owner covers unchanged record format and identity. |
| Native admission and deadline precedence | Controlled clock/write operations: invalid nonpositive budget and spent/null handle, expiry before/after setup, serialization/lock, writes, partial progress, EINTR, readiness and restoration. Cross simultaneous native success/error with timeout and assert no later output. |
| Platform mode and reuse | Scripted get/set failure, original unrelated flags and restoration; actual Darwin/Linux socket round trip; eligible HEAD/GET park then next request, ineligible close, failed/expired operation never parks. Check blocking mode before handoff. |
| Wire and recovery | Real raw-socket HEAD has exact Content-Length and zero body; GET carries bytes; stalled/slow peer hits a finite budget; a subsequent ordinary request succeeds. Existing bodyless/pipelining framing owners are reused through the single serializer. |
| Allocation and provenance | Same serialized bytes allocation, context/builder Drop and parking owner; no new allocation claim. Existing request-view use-after-consumption negatives plus failure fd owners. |
| ABI and exhaustive passes | HIR/MIR variant tripwires, runtime key/signature/effect golden and export inventory; every walker/cloner/validator includes all three operands. |

Author-side ledger pass: the argument, timeout, admission precedence, framing,
ownership, allocation and reuse rules above each have an owner row. The API
uses existing text serialization, so it introduces no new text/native input,
canonical format, structural fingerprint, reflection table or global-state
mutation. A fresh independent inspection accepted this strategy on 2026-10-25.
Its cleanup watchpoint is included above: existing two-operand response lowering
alone does not prove cleanup when the new third operand exits. Use the existing
staged consuming-argument machinery and its matching semantic action boundary.
