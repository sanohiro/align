# SSE initialized-prefix work storage

The shipped SSE watcher and streaming-client APIs retain a work block and a
committed last-event ID. Before this change, `HttpSseBytes::grow_exact` allocated
a zero-filled box for the complete caller-derived capacity, then copied only the
initialized prefix. Even a small first event requested at least 262,144
zero-filled bytes.
The parser publishes only `0..len`; it never reads the unused capacity.

Use the same exact `Box<[MaybeUninit<u8>]>` representation already used by
`HttpResponseAccumulator`. Initialize only the copied prefix and subsequently
written bytes. Preserve the existing SSE storage ledger in `std-design/http.md`:
exact capacities, simultaneous old/new lifetime, growth policy, source/output
limits, block commit, rollback, ID inheritance and allocation/Drop ownership.
No public API, ABI, parser grammar, compiler behavior or new memory ceiling.

## Implementation closure

| Cell | Implementation and acceptance owner |
| --- | --- |
| Empty formation, capacity, growth, replacement, Drop | `HttpSseBytes` retains an exact uninitialized box and initialized length. Growth copies only `0..len` before replacing the old owner; no-op growth retains address and bytes. `sse_storage_tests::prefix_growth_clear_and_reuse` crosses empty/nonempty, shrink requests, exact/full prefixes, growth and clear/reuse, including distinct new/old addresses at growth and copied-prefix equality. `http_sse_storage_grows_to_the_exact_caller_derived_bounds` retains exact capacity coverage. |
| Reads and writes | Only `as_slice` constructs a byte view, bounded by initialized length. `push` initializes its slot before advancing length. Committed-ID copies use `write_copy_of_slice`; lossy ID output borrows `MaybeUninit` slots directly and publishes length only after completion. `sse_storage_tests::committed_id_prefix_and_event_publication` crosses raw/lossy IDs, replacement expansion, empty/reset/inherited IDs and later smaller output capacities; sentinel tail bytes must never appear in a publication. |
| Control and error paths | Existing `http_sse` and `https_sse` parser/transport owners cover data/control-only blocks, blank commits, BOM, malformed UTF-8, fragmented lines, limits, incomplete EOF, timeout and terminal rollback. New prefix owners reuse the same parser and compare committed bytes and successful event spans to independent expected bytes. No source path, `?`, ownership transfer or error precedence changes. |
| Allocation mechanism | Extend the existing test-only thread-local allocator observer with successful zeroed-allocation requested bytes. `sse_storage_tests::growth_allocates_without_zeroing_spare_capacity` begins with a real zeroed-allocation positive control. Exact grow performs one allocation of the retained capacity and requests zero zero-filled bytes; no-op/reuse performs none. Running the owner against the old zero-filled acquisition must fail. Production allocation counters and allocator selection stay unchanged. |
| Cross-stage scope | No type/IR/interface/generic or whole/per-unit boundary changes. Existing SSE-watch/raw/fetch and borrowed-SSE driver owners retain source-level behavior and Drop coverage. K1/plan61 and parked aggregate provenance remain deferred. |
| Measurement | Qualify fresh preparation plus parser dispatch and retained reuse with short and large output capacities, actual event/ID byte checks and observed operation counts. Compare release baseline/candidate on macOS and Linux without competing builds/tests. Report allocation-zeroing requests separately from time; no network throughput, RSS or portable speedup claim. Retain the change only if its bounded workload improves without a material reuse regression. |

The author-side matrix pass must bind every changed read and initialized-length
publication to an owner. The representation follows the existing HTTP response
accumulator's prefix invariant, but the SSE block and ID writers require their
own focused boundary review before implementation. Keep the storage producer,
all writers and their owners in one capability; splitting them would duplicate
the same initialization proof.

Focused suites: native `sse_storage_tests`, `http_sse`, `https_sse` and
`http_streaming`; driver `http_sse_stream`, `http_read_stream` and
`http_client_composition`. Tests retain their existing platform qualifications.


## Qualification

The three new prefix/allocation owners and the existing native SSE/TLS/framing
owners pass on macOS and Linux ARM64: 23 native tests plus 33 driver tests per
host. The one ignored test is the separately executed manual measurement.
Restoring the former zero-filled acquisition fails the exact zero-initialization
owner; the candidate is restored before the complete owner runs.

The [local measurement](../../bench/sse_prefix_storage/README.md) retains all
samples and observed work counts. Fresh preparation/dispatch improves 3.7–7.5×
on macOS and 12.2–20.1× on Linux in this bounded corpus; reuse has no measured
regression. Allocation/page-state variation and the exclusion of network work
prevent a portable startup/throughput or RSS claim. The allocation size/count
and public storage ceilings are unchanged.
