# Key sorting for independent Copy records

Status: implemented; owner verification and preflight review close this capability.

Plan220 made complete independent Copy records usable with `sort_by`. This
capability composes that same element domain with the existing `sort_by_key`
terminal. It removes the scalar-index detour when one ordinary Ord key selects
an entire numeric/data record. LLM's existing positional text-key callers and
R27 establish the sorting family, but this is library composition, not a new
claim that R27 requires this spelling. All three consumer registers and current
sort callers were reconciled; media has no sort caller, and voice uses scalar
sort. Consumer adoption remains external.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Signature | Existing `source.sort_by_key(f) -> array<T>` with exactly one required `f: (T) -> K`, no defaults. Preserve primitive Copy/str elements; additionally admit exactly Plan220's already-formed independent Copy record domain, without explicit `align(N)`. The complete graph and current record/collection formation are authoritative, including inactive variants and zero-length fields. No new root tuple/sum/vector, view/callable/raw/resource/Move record or field formation. |
| Keys and call behavior | K retains the existing Ord domain: integer, float, char, str or owned string. Named, lifted, generic/imported callables receive a complete T by value and existing Copy captures. Keys are evaluated exactly once per surviving element, in input order after collection; empty calls zero times and singleton once. Sequential effects and the existing parallel-Pure requirement remain. Eager captures evaluate at their written position once. |
| Result and order | Canonical owned AoS array of complete T values, selected by final pipeline element type. Existing fixed/dynamic/slice/SoA sources and admitted map/where/project stages apply. Existing stable key order and adaptive/default-versus-off behavior remain, including NaN behavior. Keys never enter result contents; key-only captures do not constrain independent result contents. Ordinary `sort()` still requires scalar Copy Ord elements. |
| Ownership and lifetime | Borrow source and copy whole independent records; no source move, field nulling, implicit clone, per-element Drop or new permission. Ordinary heap/explicit-arena result allocation and region, result move/replacement/return/Drop remain. Source and capture reservations last through the terminal. Source numeric data may be used/replaced after collection; source/key views are not stored in the result. |
| Temporary keys and storage | Plan134's unchanged ownership strategy: comparison key and scratch columns are Copy; an owned String key's ordinary returned cleanup bit controls an unsorted nullable owner column. Only that column deep-drops keys, exactly once on every normal exit. False-bit keys remain externally owned. Complete record result/merge buffers reuse Plan220's typed allocation/header/load/store equations. No record reinterpretation or new lowering/ABI shortcut. |
| Allocation and errors | Existing sort terminal explicitly allocates result, key columns and merge scratch according to its current adaptive/threshold behavior. Owned keys add the existing heap owner column; key-function allocation remains visible in its source. Counts/strides/overflow, empty and zero-sized behavior remain ordinary collection rules. Invalid graph, key, callable mode/effect or metadata is diagnosed before LLVM; materialization OOM/overflow is the existing hard error. No performance promise or benchmark. |
| Ownership of implementation and identity | Sema and checked-HIR share one Copy sort element/result classifier plus ordinary formation validation. Both storage-generation content initialization and borrow-result facts exclude terminal key-only captures, while ordinary capture visitation keeps them call-live. MIR uses existing keyed-sort lowering and ordinary record call/returned cleanup ABI. No new IR variant, runtime symbol/effect, native encoding boundary, global state, interface field or persisted format. Existing nominal types, structural definition graphs and compiler artifact identity apply. |
| Sources and prerequisites | Reuse shipped Plans134/220; no later milestone. Update changed element-domain prose in draft/digest/design-notes/Settled, Plan12/19, Plan134/220 scope pointers, and core array-pipeline/guide English and Japanese. HANDOFF records this capability once. Consumer registers receive only relevant provider status, uncommitted; no consumer code or pins. |

## Implementation closure matrix

| Axis | Implementation and exact owner |
| --- | --- |
| Formation and malformed input | Rename/reuse the existing comparator classifier for both keyed/comparator terminals. `sort_comparator_domain_and_metadata` extends its record graph/result submatrix to both terminals; `owned_string_sort_key_hir_rejects_forged_metadata` covers scalar and record callback/cleanup/key/result metadata. Reuse `producer_plain_copy_type_graph`. Explicit source negatives cover view/Move/callable/raw/overaligned graphs and non-Ord keys. |
| Construction, source/stage and layout | Extend `sort_records::comparator_record_order_layout_and_replay` with keyed sorting in its imported mixed-width/nested/fixed/tuple/tagged/RNG record corpus and fixed/dynamic/slice/SoA/map/where/project inputs. Compare all fields and stable IDs against independent host expected order at 0/1/31/32/33/63/64/65/129, ordered/reverse/duplicates. Key-specific text/span and zero-sized cases belong to `sort_records::key_record_lifetimes_and_cleanup`. |
| Key ownership and native ABI | Parameterize `owned_sort_keys_preserve_cleanup_and_copy_comparisons` across scalar and record inputs: exact complete record call operand, Copy key columns, one unsorted owner column and one cleanup. `sort_records::key_record_lifetimes_and_cleanup` executes numeric, borrowed text and owned string keys under heap/arena result contexts and checks exact key effects and balanced allocation/free/live bytes. Reuse Plan134's `mixed_return_cleanup_bits_leave_external_key_bytes_live` and missing-Drop negative control: return-bit semantics are independent of the new input domain. |
| Move, source nulling, Drop and replacement | Same key-specific native owner covers source reuse, result move/return/replacement/discard, repeated materialization and source replacement, zero-sized records and tiny/merge/ordered exits. Only array spines transfer; there is no new element ownership. The existing `comparator_record_mir_storage_contract` remains the exact shared allocation/header malformed owner. |
| Control, captures and lifetimes | Key-specific source/native owner covers if/match/else/?/map_err/loop joins/early source exits, local text capture expiry after completion, arena-result escape, source mutation reservation and impure callback in parallel negatives. Extend both `escape_initializer_value` and `borrow_fact_one` comparator exclusions to keyed sorting; parameterized scalar/record positives and source-str expiry negatives distinguish key-only captures from actual result elements. Preserve stage/source facts and capture visitation, including replacement during the call versus after completion. Borrowed/owned key variants must yield independent record results. Reuse existing scalar keyed effects and capture-termination owners rather than duplicate unaffected evaluation proofs. |
| Generic/import/replay | Existing mixed-record owner invokes generic record helpers and imported/lifted key calls under whole/per-unit builds and cold/hit frontend replay; the cached program executes. No new serialized fact. |
| Siblings and verification | Run changed record/HIR/MIR owners in default and `ALIGN_SORT_ADAPTIVE=off`, plus `sort_by_key`, `sort_owned_keys`, `sort_strings`, `sort_adaptive`, `sort_merge` and `sort_comparator` for unchanged contracts. Final workspace build, bounded gate and separate-target Clippy precede push. No broad performance gate. |

This is one useful consumer boundary: admission, independent validation and
executable record/key lifecycle coverage ship together. It reuses the completed
record buffer producer and owned-key lifecycle, without introducing dormant
intermediate APIs. The expected handwritten diff is below 1,000 lines.

## Author ledger-to-boundary pass

Source inspection locates the scalar restriction only at keyed source admission
and checked-HIR. Key lowering already copies complete typed elements through the
same Plan220 buffers and calls the key through the ordinary returned-cleanup ABI.
Region analysis retains source/stage contents only for str elements; independent
records use the result allocation region. Terminal key captures remain call-live, but the pre-implementation independent
review found that `escape_initializer_value` and `borrow_fact_one` still include
them in result contents. Extend their existing comparator exclusions to keyed
sorting, preserving source/stage contents and call visitation. A baseline scalar
source returning `[2,1].sort_by_key(fn value:i64 { view })` with a local owned
text capture reproduces the erroneous escape rejection. The same owner must
accept independent records and reject expired source-str elements. These are
verified by the focused owners,
not inferred merely from Copy classification. No new ownership strategy is added.

The ledger fixes the complete element/key types, frequency, effects, ownership,
allocation/error behavior and identity. No new CLI/default/configuration, native
text validation, wire format, reflection table or process-global state exists.
All changed normative statements are the same domain extension; historical
implementation scope receives a forward pointer instead of being rewritten.

Independent strategy review found one P2 provenance-completeness issue: keyed
terminal captures were still copied into result facts at both analysis sites.
The matrix and author seam audit above were corrected before implementation.
The reviewer confirmed that the two-site exclusion plus scalar/record positives
and source-text/call-reservation negatives closes that finding, with no other
strategy findings. The inspection introduced no new ownership or ABI strategy.

Author native qualification additionally found that `SortOrder` cloned HIR
captures into a temporary Vec. The eager worklist keys results by HIR address;
allocator reuse across sibling sort branches could reuse a non-dominating value.
Both keyed and comparator strategies now borrow the original HIR captures with
a Rust lifetime, matching the existing scan strategy. The native lifecycle owner
executes sibling borrowed/owned key branches; `key_record_lifetimes_and_cleanup` also executes scalar/record and
key/comparator branch twins, closing the same input-identity class. No cache rule,
SSA representation or runtime ownership strategy changes. The separate sequential
par_map fallback's synthetic stages are outside this sort strategy; inspect that
existing temporary-HIR boundary as the next independently qualified repair.

## Author matrix-to-diff closure

`copy_sort_result` is shared by both source terminals and independent checked-HIR
validation, with the existing array formation gate. The parameterized HIR graph
and owned-key metadata owners cover complete type/call/result/cleanup rejection.
Both result-fact analyses now omit only terminal ordering captures; existing
source/stage facts, the scalar string lifetime owner and
`comparator_capture_write_authority` retain actual borrowed contents and
call-time write exclusions. The native lifecycle owner returns independent
scalar/record results past local borrowed/owned key captures and replaces a
capture owner after completion. It covers heap/arena, tiny/merge/ordered paths,
zero-size records and all named normal control/transfer paths with key counts
and allocation/free/live-byte accounting measured after helper returns.

`SortOrder<'a>` makes original-HIR capture identity explicit in both strategies.
The same native owner executes differently captured sibling branches for scalar
keys/comparators and record comparators, alongside three record key branches.
The imported mixed-record order owner now exercises keyed sorting under both
compilation modes and executes a cache-hit replay. Float/character/text keys and
fixed/slice/SoA/map/where/project source twins preserve complete fields and stable
IDs. Existing owned-key false-bit/missing-Drop controls and shared physical
record-buffer mutation owners close unchanged ABI/storage axes. No runtime,
wire, LLVM representation or general allocation rule changes. The documentation
changes only the selected element domain and terminal-capture lifetime promise;
no new normative code example requires a separate syntax owner.
