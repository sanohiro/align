# Line-body admission before buffer growth

Status: implemented. One native capability repairs the existing 64 MiB line-body
limit without a new API, buffering mode, ABI or compiler operation. A native
negative owner on main observed a 134,217,728-byte Vec capacity after rejecting
64 MiB plus 4 KiB of unterminated input. The existing guard runs after append.
The guard must precede append, and CRLF recognition must remain independent of
refill boundaries. This work does not reopen K1 or change borrowed-view rules.

## Public-contract ledger

| Surface | Exact record |
| --- | --- |
| Signature and inputs | Existing `r.read_line(b: mut buffer) -> Result<i64, Error>` on a bound buffered reader and bare mutable buffer. No new argument, default, CLI input, environment configuration or type. Native `unsafe extern "C" fn(*mut Reader, *mut Buffer) -> i64` is unchanged. Both native handles must be live, exclusive and disjoint; null handles keep Invalid. |
| Body and count | Bytes are binary; no UTF-8, NUL, BOM or encoding validation. Strip exactly one LF and its immediately preceding CR. A lone CR is body data. The cap is exactly 67,108,864 stripped body bytes, inclusive, independent of refill boundaries. Success returns consumed bytes including the terminator; empty LF/CRLF lines return 1/2; an unterminated nonempty tail returns its byte count; no remaining bytes returns 0. |
| Admission | Check the accumulated body length plus each proven-body span with checked arithmetic before copying it or growing output for it. A trailing CR in a span without LF is held as one stack boolean until following bytes or EOF distinguish body from terminator. Every other CR is ordinary body data. No output append may make the initialized body exceed the cap. |
| Errors and precedence | A definitely over-cap body returns Invalid before its append and before another refill. A pending CR at the cap needs another byte or EOF to decide: following LF completes the admitted line; another body byte or EOF returns Invalid; a refill failure returns its existing native error before that unresolved classification. Native refill EINTR retry and errno/Timeout mapping remain. Invalid numerical length precedes append. No failure publishes a partial body. |
| Ownership and state | Borrow both existing owners for the call; neither moves or closes. After non-null admission, clear initialized output once on call entry, before the first possible refill, and on every error or zero-result EOF; null-handle refusal leaves any other handle untouched. Preserve accumulated body across ordinary refills. A nonempty admitted EOF tail is success and publishes its body. Failure/zero-result EOF retains the published capacity; every success publishes body length and max(old capacity, body length). Existing allocation may be retained. Advance the reader through the selected LF, or through the complete scanned span on a known oversize failure; preserve subsequent lookahead. No rollback, retry of a record, retained input or new Drop. A pending CR is already included in consumed count and never counted twice. |
| Allocation/resource promise | Reuse existing caller output and 64 KiB lookahead. Valid body growth retains existing allocation/OOM policy. A known over-cap append performs zero output allocation/reallocation and copies zero bytes from its rejected span. The pending CR uses one scalar, never a separate heap buffer. Existing larger reservations and allocator over-allocation are not shrunk or bounded by the line cap; no total heap/RSS, latency or throughput ceiling is promised. |
| Owner and identity | Runtime owns admission and byte assembly; sema, checked HIR/MIR, LLVM and package interfaces remain unchanged. The existing keyed operation/signature/effects/export inventories do not change. Normal compiler/runtime artifact identity applies; no cache field, structural fingerprint, serialization tag or format changes. Shipped M12 buffered reader and BufferStorage are the only prerequisites. |
| Acceptance and measurement | Existing full-cap refusal owner records baseline 128 MiB backing and candidate no extra growth. A small-cap native owner uses the same loop and crosses all split positions of byte-oracle cases; exact-cap LF/CRLF, bare CR, repeated CR, binary NUL, empty/final/over-cap lines and following reads qualify output/count/state. Whole-Rust allocator counters with a positive control prove zero rejected-span allocation. A deliberately oversized reservation initialized with sentinels is inspected through its still-live private backing after rejection; unchanged bytes distinguish no copy from append-then-clear even when no allocation is needed. Both native platforms run these owners. Existing m12_read_line source owners preserve typed behavior. No speed claim or timing gate. |
| Source agreement | draft line-read contract, language-spec digest, existing Settled line-read item, design-notes rationale, core string English/ja capacity notes where applicable, this matrix, and HANDOFF capability status. No std module design file owns reader/writer separately. |

## Implementation closure matrix

| Axis | Implementation boundary / owner |
| --- | --- |
| Formation, validation, construction | Existing buffered-reader and bare-mut-buffer sema admission. No type/IR variant. Existing m12_read_line positive and rejection owners remain; null native admission is retained. |
| Move-in/out, nulling, Drop, replacement, return | No ownership changes. Reader/Buffer stay borrowed; existing generated cleanup and source owners remain. Output is cleared once on admitted call entry before the first possible refill and on every error/zero-result EOF, preserved across ordinary refills, and published after terminated or nonempty EOF-tail completion. Native split/cap tests inspect length, capacity, subsequent reads and owner cleanup. |
| LF, CRLF and EOF | One internal assembly loop accepts only test caps in 0..=READ_LINE_CAP and is also production's loop. Hold only an unresolved trailing CR, check before any body append, and flush it as body only at EOF/ordinary following bytes. Independent byte oracle across every split point closes exact-cap CRLF without allocating the terminator. |
| Known oversize, unresolved CR, native errors | Check before growth, consume only the selected scanned span, clear output, retain published window. Parameterized prefilled-reader tests inspect cursor/surplus; an invalid fd with a pending cap-edge CR qualifies error precedence. Existing EINTR/native mapping code is untouched. |
| if/match/else/?/map_err, branches/loops/exits | Existing ordinary i64/Error Result lowering is unchanged. Run existing m12_read_line whole source target; no new compiler fixture is needed for unchanged control lowering. |
| Generic/interface/whole/per-unit/cache | No source admission or IR/ABI/interface change. Existing source owners and bounded compiler/interface gate retain this proof; runtime producer identity changes normally. No new cache format or language mirror is introduced. |
| Runtime provenance and allocation | Existing BufferStorage exclusive mutation and Reader lookahead. Every append has an admitted body-length proof. Reserved-output/rejected-span owners inspect initialized sentinels through still-live private backing, in addition to pointer/capacity/allocator counts, including an oversized-reservation control that would hide growth and a known over-cap newline/no-newline span. CRLF and bare-CR boundaries qualify classification separately. Native signature remains compile-time type-pinned. |
| Resource claim | Baseline full-cap owner measures 134,217,728-byte backing. Candidate repeats that input; small-cap existing-allocation counters verify zero rejected-span work with a positive allocating control. Measurements concern requested Vec backing and allocation calls only. |
| Explicit exclusions | Fallible general buffer growth, configurable line limits, zero-copy line views/callbacks, new reader ownership, GPU APIs, consumer adoption and K1. |

The expected handwritten diff is below 1,000 lines. One native loop and its
parameterized state/resource owner are a complete useful capability. A fresh
independent adversarial matrix review precedes production edits because this
repairs the allocation-admission strategy. Perform author ledger extraction and
matrix-to-diff closure, the self-review skill, one fresh full-diff review, local
owners on both platforms and final-SHA preflight before PR publication.

## Implemented closure

| Obligation | Implementation | Regression owner |
| --- | --- | --- |
| Complete length admission before any append | `append_line_body`, checked additions before `BufferStorage::with_mut` | `read_line_body_cap_rejected_spans_leave_backing_untouched`, including oversized reserved backing; the append-before-check mutation fails on copied sentinels |
| CR/byte classification, inclusive cap, count and surplus | `read_line_with_cap`, one local pending-CR scalar and shared LF/EOF completion | `read_line_body_cap_split_oracle`, every split of complete independent byte vectors at caps 0..4 |
| Native failure before unresolved CR classification, error zero/window retention, successful growth and EOF | Existing `Reader::refill` mapping; explicit admitted entry/error clearing and completion publication | `read_line_body_cap_pending_cr_error_order_and_capacity` plus existing strip/count/empty/refill owners |
| No output allocation for a rejected span | Admission helper returns before mutation; existing lookahead retained | `read_line_body_cap_rejected_allocation_parity`, whole-Rust thread-local counter and positive Box control |
| Actual production cap and requested backing | Public call passes exactly READ_LINE_CAP | Existing `read_line_over_cap_is_invalid` now includes exact-cap CRLF, byte content, published capacity and real Vec backing observations |
| Native ABI and typed source behavior | Existing C entry plus exact compile-time function-type pin | Existing `m12_read_line` source target and bounded compiler/interface gate |
| Public sources and mirror consistency | Inclusive stripped-body wording propagated once | Author ledger extraction and core string English/ja comparison |

The existing full-cap refusal reproducer measured 134,217,728-byte Vec backing
on the baseline. Candidate native measurements on macOS ARM64 and Linux ARM64
record 67,108,864 bytes for both rejected over-cap input and accepted exact-cap
CRLF input. These are requested backing observations for this initially small
window, not allocator residency, general reservation or latency guarantees.
The same native bundle's nine owners pass, including the positive allocation
control. Existing eleven typed line-read owners preserve canonical line, JSONL,
interleaving, UTF-8-view and sema rejection behavior.

The fresh independent design review found two P2 ledger/owner gaps: EOF versus
ordinary-refill clearing, and a no-copy oracle missing oversized-reservation
sentinels. Both were corrected in the authoritative rows before production
edits and closed by narrow reviewer inspection. The append-before-check mutation
then failed the named sentinel owner, with production source restored and the
positive bundle rerun. This is one completed design review, not a repeated
full-diff completion loop.
