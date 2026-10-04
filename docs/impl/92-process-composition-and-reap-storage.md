# Process composition and bounded reap scratch

Status: implementation candidate selected by the owner on 2026-10-04.
Baseline: `6f26fddac6d45a19a37c0e9fa710021f7763cb86`.
The independent inspection-only allocation-boundary review returned CLEAN before implementation.
This follows plan 51's existing-module assessment. Plans 49/50 retain the exact
public process contract. K1 and real inference-engine adoption remain deferred.

## Assessment and selected consumer

`examples/multimodal/execution.align` composes explicit launch, one-attempt
stdout/stderr reads, status observation, authenticated signalling, bounded reap
and retryable release. Its output limits, generation deadline, TERM grace,
failure classification and artifact publication are application policy. They
must not become hidden std.process defaults. The existing borrowable process
owners suffice; no second launcher, supervisor, async model or job package is
selected. Plan 49's observation helper supplies finite readiness waits for
applications that do not already alternate accept and process observation.

The reference's `reap` helper discards newly reaped records but repeatedly calls
`scope.reap(32)` while release is pending. Existing scope cleanup owners use 4096.
The native implementation reserves `max_events * 40` bytes of Rust Vec scratch
before its first nonblocking wait, including when that wait returns pending or
ECHILD. The final Align array is separately allocated for actual returned rows.
The selected improvement changes only the transient Rust reservation.

## Implementation boundary

Validate the complete count/stride range and scope state in their existing
order. Start with an empty Vec. Before each wait4 that could consume one event,
ensure space for one additional row. If full, request a geometric capacity of
`min(max(2 * current_capacity,1),max_events)`; use saturating multiplication
before the cap. Reserve exactly that additional capacity before wait4, never
after consuming the event. Keep each native status conversion, root-cache
update, error, output zeroing and final array materialization unchanged.

One free slot may be reserved before discovering pending/ECHILD/EINTR. Empty
calls therefore request one 40-byte row rather than the event budget. Capacity
grows with reached events and never requests more rows than the validated limit.
Dense batches retain geometric growth instead of one allocation per event.
Allocation failure retains the existing terminal Rust allocator policy; this
adds no recoverable allocator error. No new public type, signature, native
symbol, ABI, interface format, ownership carrier or process-global state exists.
No latency, allocation-count or cross-platform speed guarantee is introduced.

## Closure matrix

| Invariant | Implementation and owner |
| --- | --- |
| Range/type/stride validation precedes allocation or wait | Existing `capacity<Reaped>` and `Scope::valid`; `native_layout_and_ranges`, existing forged native-output owner. |
| A slot exists before every irreversible native wait | One private reserve helper immediately before wait4; geometric-growth owner simulates all reached row counts and pins capacity before push. |
| Scratch request follows reached events, not the configured budget | Native pending/one-event/empty-after-reap owner crosses limits 1/32/4096/1048576 and checks the returned Vec capacities; opt-in local measurement prints requested-capacity bytes. |
| Each growth is capped; repeated pending calls do not reserve the budget | Parameterized reserve owner crosses tiny/non-power-of-two/large limits, spare-capacity and full-capacity transitions; native sparse owner. |
| Root cache and status survive a later native error | Existing `restoration_and_reap_failure_state` consumes the root then injects EIO at the next attempt, checks canonical empty output and cached wait result. |
| Ownership, children, restore and Drop remain unchanged | Existing `exclusive_scope`, `adoption_and_drop`, `restoration_and_reap_failure_state`, non-SIGCHLD owner; driver `m11_process_scope`. |
| Output allocation/bytes/ABI and native error mapping remain unchanged | Existing scope output/layout owners and `m11_process_scope` whole/per-unit lifecycle; the export body is untouched. |
| Non-Linux acquisition refusal remains unchanged | Existing macOS native-layout/refusal and driver formation owners; reserve helper is Linux-only. |
| Generic/interface/control-flow/move/return | No changed compiler path; existing `m11_process_scope` transport owner is sufficient. No new compiler fixture. |

The reserve-before-wait rule is the existing safety strategy, applied to each
reached iteration. Inspect this closure before implementation and run one fresh
independent full-diff review on the committed candidate. Local verification is
the native process_scope owner on Linux, native refusal/layout on macOS and the
driver scope transport owner. The resource measurement is separate from the
correctness gate and makes no timing claim. No DB or actual-GPU owner is relevant.

## Local resource evidence

The same native sparse owner was run on Linux ARM64 with Rust 1.96.1 / LLVM22,
first against the original reservation and then the candidate. This observes
Vec capacity in rows times the 40-byte native row size, not process RSS, final
Align-array storage or production latency. Timing is not instrumented or claimed.

| Kernel outcome / max_events | Original scratch bytes | Candidate scratch bytes |
| --- | ---: | ---: |
| Pending / 32 | 1280 | 40 |
| Pending / 4096 | 163840 | 40 |
| Pending or ECHILD / 1048576 | 41943040 | 40 |
| One terminal root / 4096 | 163840 | 80 |

All nine native scope tests passed, including partial-reap failure, restoration,
foreign children, non-SIGCHLD children, adoption and Drop. Logs are retained
outside the repository; this table records the capability's resource result.

## Continuation

After this capability merges, continue plan 51's retained-directory traversal
assessment using an Align-owned real workflow. A new std operation requires a
concrete missing common behavior; application manifests, deletion policy and
model-specific orchestration retain their existing owners. Communication and
other independent measured optimizations may proceed without deferred K1.
