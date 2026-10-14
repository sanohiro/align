# Binary byte-write effects (Request146)

## Evidence and existing strategy

The consumer loop is rejected because the callee uses BytesSet. At provider
13a9bdc2, changing only set_u8(0,1) to indexed assignment passes; a straight-line
subview call followed by parent reuse fails too. This is a missing write-effect
producer, not a loop-generation retirement defect. Exact controls and hashes are
retained beside this proposal. Media tokenizers use typed setters; voice has no
matching setter use in src. No consumer code changes are authorized.

Plan65 row W already guarantees one full-width store before every normal return
from any of the 18 binary setters. Plan80 already permits original backing to
remain live when its existing must_write_original_element summary is proved.
Use that same record/transfer and checked replay. No syntax, ownership, alias,
ABI or serialized-summary change. Do not weaken the existing no-op/conditional
call fallback or infer a mandatory write from an empty fill/copy.

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Setter family | At completed built-in action, all BytesSet widths/signs/endian modes report a whole u8-element write to the captured receiver. Scalar value stores no borrowed roots. All 18 semantic summary cases and executable independent endian bytes. |
| Origin | Reuse entry-header flow plus known exact completed headers; copied/subsliced receivers preserve their actual generation. Rebind before write cannot establish must-write-original; writes after temporary rebind still retain may effects. Parameter/local/subview positives and rebind/conditional negatives. |
| Control/eager completion | Record only after all receiver/offset/value operands fall through. Branch/match/loop/return/?/map_err combine existing may/must rules. Terminating offset/value and optional zero-iteration write cannot forge must bits. |
| Sibling mutations | BytesFill/BytesCopyFrom may succeed empty: retain no mandatory-write claim. Report their may-write effects through the same helper; no memory ownership is transferred by byte copies. Native and source existing byte-operation tests retain bounds/alias/validated-UTF-8 behavior. |
| Calls/transport | Existing mutable view summary inference, replay, direct/imported/generic/target-resolved calls and interface hashing. Source whole/per-unit/import/cache owner uses consumer loop with zero/one/multiple iterations, live buffer backing and exact byte checks. Existing summary mutation/transport owners plus new setter-produced stale-record rejection. |
| Negative authority | Shared destinations, retained subviews across owner replacement/drop and validated text after byte writes remain rejected. No destination permission is inferred from source mut binding alone. |

This implements the reviewed Plan80 record semantics and Plan65 W contract.
Author-side matrix pass and one fresh preflight code review close the boundary;
no separate new public/safety strategy review is required. R133 is merged in #1336; all three registers were reconciled before selection.
Consumer-owned adoption remains pending.
