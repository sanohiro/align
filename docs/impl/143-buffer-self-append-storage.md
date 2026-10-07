# Reuse the buffer's preserved prefix for self-append

Status: implemented; independent of deferred K1/plan61 and the parked
aggregate-provenance repair. Existing whole/per-unit source admission accepts
`b.append(b.bytes())`. This changes native scratch storage only, with no source
API, borrow rule, runtime ABI, layout, or serialized compiler record change.

## Existing contract and selected boundary

`buffer.append` copies source bytes onto the published prefix, may grow the
payload, updates length/read-window capacity and preserves requested alignment.
The native implementation snapshots any overlapping input into a temporary Vec
before truncate/growth. The snapshot is unnecessary when the complete source is
inside the buffer's published initialized prefix: growth preserves that prefix.

Before exclusive storage work, admit the complete source address range with
checked address addition. A nonempty range qualifies only if its start is at or
after the payload base and its end is at or before the checked published-prefix
end. Save a byte offset, never an old pointer for later dereference. After ordinary
checked reservation, derive both source and destination from the current owner's
fresh pointer and copy the admitted bytes after the old prefix. The source ends
no later than the destination starts, so nonoverlapping copying is valid even
when the buffer moved. Publish the new initialized length only after copying.

Keep one safe internal BufferStorage operation for copying a range of its own
initialized prefix; it checks the complete range and new length before reserve.
It uses the existing hard allocation-failure policy and exact alignment/layout
owner. This helper adds no raw input or externally callable symbol.

Nonoverlapping inputs keep their existing direct copy. Overlaps that are not
wholly within the published prefix keep the existing snapshot fallback, including
readable bytes in retained storage beyond the logical length. Null, nonpositive
and unrepresentable input lengths keep existing behavior. Empty inputs never
require pointer arithmetic or a new allocation. Do not widen unsafe input
preconditions, infer a stronger slice alignment, change read-window policy, or
weaken source alias checking.

## Implementation closure matrix

| Axis | Implementation and exact owner |
| --- | --- |
| Range admission and initialization | A native parameterized owner covers complete prefix, interior prefix, last byte, empty/null/negative input, external input and an initialized retained-storage overlap extending past the published prefix. Independent expected byte arrays pin admission, order, length and capacity. Checked address/range arithmetic precedes source relocation and writes. |
| Stable and moved allocations | Cross reserved-capacity and growth cases with alignments 1, 8, 64, 4096 and 16384. Extend the existing thread-local BufferStorage allocation probe with a test-only forced-move mode that allocates while the old owner is live, copies its initialized prefix, then frees the exact old layout. Native owners assert the probe was consumed, the pointer changed and all output bytes/alignment survived. Production growth remains ordinary realloc. |
| Ownership, replacement and Drop | BufferStorage remains the sole owner. Old allocation remains owned on refusal; successful growth publishes its new pointer before observers. Existing aligned-storage exact allocation/layout pairing and buffer byte-view owners retain move, replacement, shared publication and final Drop. New fixture handles arm cleanup immediately. |
| Allocation/resource evidence | An exact-filter bounded native child with requested-live instrumentation inactive uses the actual thread-local global allocator counter and a positive allocation witness. Compare the production operation with the old snapshot-equivalent reference after identical fixture setup. Prefix self-appends remove one temporary allocation of source length, with zero allocations when capacity suffices and only required payload growth otherwise. Fallback/external/empty controls retain their counts. Print compact requested-byte/count records as the local resource measurement; claim no elapsed-time, RSS, residency or uniform-throughput improvement. |
| Source formation, control and views | A focused whole/per-unit driver owner uses the shared owned_fixture guard for the complete compile/link/run phase and observes full/prefix self-append, growth, returned/moved owners and final contents. Existing aligned_buffer, fallible_buffer, buffer_pread_into and m9_io owners retain formation, if/match/else/try/map_err, loop/early-exit cleanup, stale-view rejection and read-window behavior. No compiler rule changes. |
| Generic, interface, ABI and malformed input | Existing generic cache and malformed HIR/MIR owners in aligned_buffer/fallible_buffer retain their unchanged boundary. Runtime export/signature inventory is unchanged. No new persistent record or native entry point. All supported native platforms run the same storage implementation. |
| Mutation and failure proof | Restoring the original snapshot path must fail the actual no-growth allocation owner. Forced relocation must be observed directly, not assumed from a large allocation. Existing allocation-refusal and exact-layout owners retain the old owner on reserve failure; invalid helper ranges reject before reserve or copy. Never execute a known invalid overlapping nonoverlapping-copy mutation. |

## Review and delivery

The new native relocation proof gets one fresh independent adversarial review
of this matrix before implementation. The author ledger-to-diff pass covers
every range, initialization, allocation and publication obligation, followed by
one fresh inspection-only code review and the normal final-SHA preflight.
One capability contains the native operation, forced-move owner, allocation
measurement and source consumers. Expected handwritten changes are below 1,000
lines. Preserve all external align-llm code, pins and adoption state.

## Acceptance and local resource evidence

`align_runtime::buffer_self_append_tests::prefix_copy_preserves_bytes_and_removes_scratch`
owns the range/relocation matrix, invalid-range abort precedence and isolated
allocation measurement (`--features alloc-count`). The existing
`buffer_storage::tests::aligned_storage_growth_and_adoption_match_every_allocation_layout`
also observes exact allocation/free layout pairing through forced relocation.
`align_driver --test buffer_self_append` owns complete whole/per-unit execution;
`aligned_buffer`, `fallible_buffer`, `buffer_pread_into` and `m9_io` retain the
unchanged formation, view, cleanup and read-window contracts.

On local macOS ARM64, a 65,536-byte full self-append at alignment 64 gives:

| Reserved payload | Production allocations / requested bytes | Snapshot reference allocations / requested bytes |
| --- | --- | --- |
| Enough capacity | 0 / 0 | 1 / 65,536 |
| Forced relocation | 1 / 131,072 | 2 / 196,608 |

The same owner checks exact counts for both input sizes, all five alignments and
every range/control row. Retained-tail overlap still allocates its four-byte
snapshot; external and empty controls preserve their counts. Restoring the old
snapshot path fails the no-growth owner with `(1, 8)` instead of `(0, 0)`.
No dangling-pointer or invalid-overlap mutation is executed. These are successful requested-allocation
counts/bytes, not live storage, timing or RSS measurements.
