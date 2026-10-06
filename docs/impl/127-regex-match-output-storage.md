# Direct regex match-array storage

`find_all` and `split` grow their eventual C-owned array directly, using the
existing checked heap ArrayBuilder with a scoped stack owner. This removes the
previous Rust Vec of spans and its final output copy.
Keep one engine traversal; do not count matches in a separate pass. Accept only
a measured reduction of staging allocation/copy without material latency loss.

## Existing public record

| Surface | Exact behavior retained |
| --- | --- |
| `re.find_all(text: str) -> array<regex_match>` | Borrow the bound regex and valid UTF-8 text for the call. Return every engine-reported leftmost non-overlapping span in order. `regex_match { start: i64, end: i64 }` is Copy, half-open UTF-8 byte offsets; both ends are character boundaries. No match yields an empty owned array. |
| `re.split(text: str) -> array<regex_match>` | Borrow the same inputs. For every reported match `[s,e)`, emit `[previous_end,s)` and advance previous_end=e; finally emit `[previous_end,text.len())`. Empty leading/interior/trailing fields remain. No matches, including a nonmatching pattern over empty text, yield one field. A zero-width match over empty text yields two empty fields. |
| Ownership/effects/errors | Both calls stay Pure and have no options/defaults or recoverable errors. Output owns its array spine, borrows neither input nor regex, and contains only Copy offsets. Nonempty output is C alloc/realloc-family storage freed exactly once by existing array Drop; empty find_all is null/zero. Allocation/size failure stays terminal. No per-element cleanup. |
| Native/producer boundary | Preserve existing status-0 and `AlignMatchArray { ptr, len: i64 }` out-slot ABI. Null out returns before work; otherwise zero the slot before rejecting null handle, negative extent, null-positive extent or invalid UTF-8. Arbitrary pointers retain the existing unsafe readable-range precondition. No compiler/IR/interface/export/cache identity change; the runtime archive follows its existing content fingerprint. |

The split formula is the existing implementation and adopted Rust-regex
semantics. EN/JA regex design prose distinguishes no match from an empty match, correcting
the previous overstatement "empty input yields one empty field".
A real existing-runtime C probe confirms empty pattern/empty text produces one
find_all span and two split fields. Preserve that behavior, adding explicit
coverage. This is a precision correction, not a new empty-input special case.
The draft/digest do not specify a contradictory split detail and need no change.

## Safety/implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Type/layout/initialization | Stack owner contains existing heap-only ArrayBuilder with stride equal to sizeof(AlignRegexMatch), two i64s. Every pushed span is completely initialized; C allocation alignment and 16-byte stride satisfy its alignment. Existing checked reserve admits len+1 and capacity*stride at growth. Between growths len < cap proves the typed slot and increment fit; the private helper always has heap-only storage. Native owners assert every span and alignment across growth boundaries. |
| Construction/admission | Form a nonallocating empty builder only after existing ABI/text admission; find_all returns its canonical zero on first iterator exhaustion before header setup. No region, arena, Move element or heap header mode. Invalid-input matrix owns canonical zero output; existing public compile/pattern limits are unchanged. |
| Move-in/write/growth | Push one initialized span through the existing checked builder storage path; increment initialized count only after the write. Growth retains the existing initialized prefix. One parameterized find_all/split owner covers 0/1/4/5/8/9/many spans and Unicode/empty-match advancement. |
| Move-out/source nulling/Drop/return | Scoped owner frees its nonnull C payload on Drop. Finish detaches payload by setting its source pointer null before returning ptr/len; ordinary local destruction then frees nothing. Publication has no intervening fallible work. Exercise ordinary drop without finish, finish then caller free, empty finish, and unwind after pushes in an isolated resource owner. No fresh output-to-output copy. |
| Replacement/control/early exits | Regex entrypoints do not introduce new Result/?/map_err/loop-join semantics; existing source array replacement/return/Drop paths and A22 out array remain. Every native rejection occurs before output storage allocation. Matching is one existing find_iter loop; no changed engine, captures, pattern rules, or next-match advancement. |
| Output independence | Drop regex and overwrite/drop text after native success, then inspect returned span values and free through align_rt_free. Both methods and empty/nonempty results share one guard/finish implementation. |
| Generic/whole/per-unit/interface | No compiler or serialized shape changes; existing runtime-materialized array lowering and regex source/drop owners remain authoritative. Run regex_tests and std_regex. Native byte/count/provenance owners directly reach both changed FFI entries. No new language type or consuming surface is introduced. |
| Resource proof | Observe eliminated private Rust staging relative to a warmed direct engine walk. C output allocation is a separate family: align_rt_realloc(null,n) is tracked by the existing requested-live probe but does not increment ALLOC_CALLS. Use a bounded exact-filter child for global C-probe work so its Rust bookkeeping cannot contaminate another owner. Distinguish cumulative requested bytes, live tracked bytes, output capacity and RSS; claim only what the producer-owned counters observe. A mutation restoring staging must make the relevant owner fail. |
| Performance | Compare actual non-test baseline/candidate release runtimes with one C harness and producer compile logs/distinct executable hashes. Cover both methods, no/one/sparse/dense/all-empty matches, ASCII and Unicode, empty/short/large text. Independently verify all spans before timing; check observed headers/counts/first-last spans and free every timed output. Warm engines, use adequate fast-case repeats and balanced ABBA. Resource probes never run in timing processes. Reject a material small/ordinary-case regression rather than adding a second scan. |

This capability changes private output construction/transfer. One fresh
adversarial review of this matrix/strategy was clean before implementation;
the author matrix-to-diff pass and ordinary full-diff review remain required.
One PR contains both sibling operations and their shared owner. Expected scope
is below 1,000 hand-written lines; K1 remains deferred. Update only this plan,
the directly affected EN/JA regex allocation/empty-result prose, and benchmark
record. No broad language decision is reopened.

Exact owner names (one parameterized owner may close several rows):
- `regex_match_storage_tests::spans_match_engine_and_explicit_empty_goldens`:
  both sibling methods, byte offsets, growth counts and explicit empty products.
- `regex_match_storage_tests::invalid_views_publish_zero_before_storage`:
  native detection/admission and canonical output.
- `regex_match_storage_tests::returned_spans_outlive_text_and_regex`:
  independent output, public free and alignment.
- `regex_match_storage_tests::staging_allocation_matches_warmed_engine_only`:
  exact-filter child with C live probe inactive; real Rust counts/bytes versus
  direct engine walk; restore-staging negative control.
- `regex_match_storage_tests::scoped_output_frees_or_transfers_once`:
  separate exact-filter child using tracked C live bytes for ordinary/unwind
  Drop and finish/caller Drop, including empty/growth. No Rust-count assertion
  runs while the C probe's bookkeeping is active.
- Existing `regex_tests` plus `align_driver --test std_regex` own unchanged
  public method semantics, source admission, array/regex Drop and effects.
  Existing `array_builder_record_capacity_arithmetic_failures_are_terminal`
  owns the reused reserve arithmetic; the new helper must not bypass it.

## Verification and resource boundary

Both macOS ARM64 and Linux ARM64 pass the five native storage owners, all 33
existing tests in the regex_tests runtime module, the terminal builder capacity-arithmetic owner,
and all 21 std_regex driver owners. The restore-Vec negative control fails the
staging allocation owner; restoring direct output passes it. Requested-live
and Rust allocation instrumentation use separate isolated child processes.

The [release benchmark](../../bench/regex_spans/README.md) records the exact
producer/fixture identities, complete case matrix, per-case ABBA methodology,
and observed latency limits. Private Rust staging requests are eliminated.
The result may retain amortized spare capacity and growth may internally copy;
no exact-capacity or RSS promise is introduced. Linux short nonmatching calls
increase by at most 2.31 ns in this measurement; the change is accepted for its
staging/copy reduction and otherwise comparable or improved repeated-call cost.
