# Byte-access guard fusion

Status: implemented and locally qualified, 2026-10-30. This R109 follow-up preserves
Plans64/65/69's byte-access semantics and proof boundaries. It changes no
language, runtime ABI, ownership, allocation, interface or cache format.

## Evidence and scope

The current native M1 census at compiler `8a21f1cf` contains 1,115 sign tests.
That number includes necessary source arithmetic and search-result tests; it
does not establish 1,115 redundant bounds checks. The retained optimized
`alignpack.bytes_equal_at` shows the concrete remaining boundary: byte reads
still use the signed three-part range predicate, including for one-byte reads.
The same accessors occur in all three external consumers. Consumer code and
compiler pins remain externally owned.

Lower a one-byte read or write's failure condition to `(u64)offset >= (u64)len`.
For widths 2/4/8 use the existing two-comparison unsigned range predicate over
`offset`, wrapping `offset + width`, and `len`. SliceLen proves nonnegative
length; no LLVM assumption, extra integer flags or source-minted fact is added.
Keep the original signed start/end/length arguments to `RangeFail`, at exactly
the original evaluation point. This includes the wrapping end printed on an
overflowing offset. No address may be formed before success.

`byte_ranges` must authenticate the actual emitted unsigned predicate before
removing a guard: exact operands, i64-to-u64 casts, result types, length producer,
width, read, dominance, effects and recurrence. Its current signed-predicate
recognizer is replaced, not retained as a compatibility path. `loop_facts` must
recognize the one-byte range guard and retain its exact byte-read authentication;
the existing multi-byte and unknown-operation refusals remain. Its existing
generic two-comparison range recognizer remains unchanged. The invocation-local
eliminated-guard records and the later `byte_prepare` replay remain unchanged.
The expression-dominance walker must traverse only a correctly typed i64-to-u64
cast, checking both its definition and its source before use. The independent
live-guard path in `loop_facts` must validate source/result SSA types as well as
the cast's declared types before publishing any counted extent.

## Implementation closure matrix

| Boundary | Implementation and owner |
| --- | --- |
| Formation and construction | Existing numeric byte widths, semantic types and MIR variants. Shared fixed-width guard emitter used by reads and writes. New parameterized `byte_access_guards_use_unsigned_predicates` inspects width/sign/endian twins and checks the original RangeFail operands. |
| Predicate equivalence | Nonnegative length and i64 offset; one-byte unsigned Ge, wider unsigned start/end and end/length Gt. New parameterized owner crosses zero, width-minus-one, exact width, end, negative, i64 minimum/maximum and overflowing end against the signed reference predicate without dereferencing invalid memory. |
| Proof authentication | `byte_ranges::Facts` re-derives exact unsigned casts and predicates, including typed traversal in `expression_precedes`. Extend `byte_range_malformed_and_invalidated_proofs_fail_closed` for wrong casts/types/operands/comparisons and a non-dominating cast source; retain recurrence/effect/dominance negatives. Every mutation must actually occur and return no eliminated proof. |
| Loop composition | Adapt the width-one `loop_facts` recognizer while retaining the matching read/view/offset and dominance proof. Extend `live_width_one_byte_guard_authenticates_only_its_dominated_read` with comparison/cast/source/result type, view/offset/width and success-edge mutations; none may publish a counted extent. Existing composed byte owners and `paired_byte_ranges_keep_unproved_guards` close live/eliminated/unknown guards, writes and re-entry. |
| Evaluation and failure | Read receiver then offset; write receiver then offset then value, followed by the same trap. Existing negative/read-past-end owners plus a bounded native width/offset matrix assert unchanged signed RangeFail text and successful unaligned/endian reads/writes. |
| Ownership and control | No storage, move-in/out, nulling, Drop, replacement or return changes. No speculative read/write. Existing byte codec native tests cover loops, branches, early exits, `?` and partial output. Aggregate ownership is unaffected. |
| Generics, imports and compilation | Same MIR lowering in whole/per-unit, generic instantiation and `byte_prepare`; existing composed reader cache/ThinLTO owners plus a bounded native imported guard fixture. Compiler identity invalidates codegen artifacts; serialized formats stay unchanged. |
| Resource and measurement | No allocation or new runtime call. Compare baseline/candidate independent byte-scan objects and actual consumer-module IR, using identical inputs and outputs. Run a bounded local byte-scan benchmark only for this changed performance path. No global sign-count or application latency promise. |

The author-side matrix-to-diff pass maps every row to the implementation and
owners above. This is one capability: the emitter and both proof consumers
must change together so guard fusion does not silently disable existing proofs.

The independent strategy review found two missing owner axes: typed cast
traversal in `byte_ranges` and malformed direct-live guards in `loop_facts`.
Both are incorporated above before implementation. Arithmetic equivalence and
the unchanged trap boundary had no findings.

## Qualification

The focused owners are `align_mir --lib loop_facts::tests` (8 checks) and the
driver targets `byte_access_guards` (3 checks, one lifecycle helper ignored),
`bytes_ops` (16), `loop_facts` (46), and `runway_a2_binary_codec` (36). They cover
the unchanged recurrence, effects, exceptional edges, byte preparation and
whole/per-unit/cache paths as well as the new predicate and malformed-cast axes.
Restoring the three production files from baseline `8a21f1cf` makes
`byte_access_guards_use_unsigned_predicates` fail on its signed-guard assertion;
the owner distinguishes the optimization from ordinary successful execution.

Matched optimized-IR inspection uses baseline `8a21f1cf`, LLVM 22 and native
Apple M1 on 2026-10-30. The unchanged consumer file is
`align-llm/src/alignpack.align` at `83eb5123`, compiled with
`emit-llvm --stage optimized --export bytes_equal_at`. In
`bytes_equal_at`, comparisons fall from 7 to 5, signed comparisons from 4 to 2,
and retained `align_rt_range_fail` call sites from 2 to 1. The remaining signed
comparisons are source-level offset/extent validation. This is module evidence,
not a new whole-product census or consumer acceptance receipt.

An independent four-function byte scan keeps the sum/equality loops unchanged;
its standalone `read8` falls from two comparisons to one unsigned comparison.
`read32` retains two comparisons, both unsigned. A three-export object containing
`read8`, `read32` and `sum` shrinks from 716 to 696 text bytes. A bounded C probe
independently checks all 256 byte reads, all 253 unaligned four-byte reads, and
the sum before timing; seven alternating baseline/candidate pairs retain exact
matching checksums. Median times are:

| Work per sample | Baseline | Candidate | Candidate/baseline |
| --- | ---: | ---: | ---: |
| 10 million varying-offset `read8` calls | 15.837 ms | 15.951 ms | 1.007 |
| 10 million varying-offset `read32` calls | 15.738 ms | 15.842 ms | 1.007 |
| 1 million 256-byte sums, unchanged control | 18.427 ms | 18.202 ms | 0.988 |

The measurements establish no latency improvement; the delivery is fewer guard
instructions and smaller code with preserved behavior. Compact sources, commands,
CSV samples and IR extracts are retained in the local qualification evidence.
R109's original global count threshold remains unqualified.

## Other request dispositions

R97's remaining dynamic prefix call is LLVM's ordinary inline-cost decision
(retained Linux remark: cost 50, threshold 45), not the repaired target-attribute
defect. The current call is a final response consistency check. Plan70 excludes
forcing `inlinehint`/`alwaysinline`; no measured benefit or new defect justifies
changing that policy. Original zero-symbol acceptance remains unmet and must
not be relabeled as a pass. The constant-predicate delivery remains verified.

R162 proposes direct fixed Copy-array Result payloads. Existing `scalar_arg`
rejects them, and admitting them requires complete tagged layout, transport and
extraction support rather than removing that guard. The originating cache trial
is withdrawn and no active consumer needs the proposal. Defer implementation
until a current consumer is selected; no delivery or consumer acceptance is
claimed. R132/R142 and AM-R26 retain their settled restrictions.
