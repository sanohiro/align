# Copy string sorting

Request 27 records direct string sorting as an independent useful capability.
This boundary completes the existing Copy `Ord` element domain of `.sort()`
and admits Copy `str` elements to `.sort_by_key(f)`. It reuses the stable MIR
sort, scalar string comparison and borrowed-element collection representation.
No comparator overload, owned-string copying, aggregate ordering or K1 is added.

## Public-contract ledger

| Surface | Exact contract and owner |
| --- | --- |
| Direct sort | `source.sort() -> array<T>` for `T` equal to an integer, float, `char` or `str`: exactly the existing scalar Copy `Ord` domain. No arguments. Stable ascending order; `str` compares UTF-8 bytes lexicographically, including empty text, prefixes and embedded NUL. Floats retain the existing IEEE partial-order behavior. `bool`, structs, owned `string`, resources and other Move values reject during type checking. `sort_strings::copy_sort_domain_and_diagnostics` owns admission. |
| Keyed sort | `source.sort_by_key(f) -> array<T>` for the existing primitive Copy elements plus `str`. The key is a Copy `Ord` value; owned `string` keys remain rejected. Named/lifted/imported functions and existing Copy captures retain their existing rules. Impure keys execute exactly once per surviving element, in input-index order, after collection and before the first comparison. `sort_strings::string_sort_order_and_stability` owns key count, ordering and stable equal-key order. |
| Source/result ownership | Fixed arrays, slices, dynamic arrays and supported pipelines borrow their sources. A result owns only its `{ptr,len}` spine; `str` elements retain their byte owners and are never cloned, moved or individually freed. Heap results have ordinary single spine Drop; arena results use bulk cleanup. Existing donation refuses `str`. Scratch and key buffers are shallow spines with balanced frees on their allocated paths. Array move-in/out, source nulling, replacement and return use existing array machinery. |
| Lifetime/aliasing | Every result `str` retains the final pipeline element's content dependencies and meets source/stage-capture lifetime with output allocation lifetime. Sorting does not extend those lifetimes. Returning local text views or using results after owner replacement, truncate, move or arena exit must reject; caller-owned views may return under existing helper summaries. Existing conservative source-generation loans remain: replacing, moving or reallocating a bound source array can invalidate a live result even when its copied bytes are static. Fresh result storage does not promise release of that source loan. `sort_strings::string_sort_lifetime_and_control` owns positive and negative paths. |
| Allocation/errors | Materialization remains visible at the terminal: one result spine, existing conditional merge scratch, and keyed decoration buffers. No string-byte allocation or implicit `.clone()`. Invalid allocation size and OOM retain terminal abort behavior; no Result or retry. Existing pipeline validation rejects malformed source/stage types before lowering. No native text/wire input or validation order changes. |
| Compiler/runtime/artifact | Existing `ArraySort` and `ArraySortBy` records and sort MIR; no new IR variant, native ABI symbol, type layout, runtime key, serialized field, CLI/build input or ambient configuration. Checked HIR independently requires exact final element, Copy membership, `Ord` for direct sort and key callable metadata. MIR structural fingerprints and compiler namespace cover changed emitted programs; imported source-template generic helpers retain nominal identities. Whole-program and per-unit owners exercise both modes. |

## Implementation closure matrix

| Axis | Implementation and exact owner |
| --- | --- |
| Formation/validation/malformed input | Sema uses existing `Ord` authority plus a closed Copy scalar domain; symbolic generic Ord/Num bounds authorize template checking, and concrete monomorphization rechecks the Copy restriction before executable HIR; keyed elements add `str` without admitting Move. Checked HIR repeats the domain independently. `copy_sort_domain_and_diagnostics` and `copy_string_sort_hir_rejects_forged_metadata` cover wrong element/key/result/source/arity and fail before LLVM. |
| Construction/materialization/provenance | Reuse `lower_array_collect` without cloning bytes; donation refuses `str`. Content region for both string-sort terminals follows `ArrayToArray`, and existing pipeline-element BorrowFact/header propagation supplies byte-owner generations. `string_sort_lifetime_and_control` covers locals, static views, mapped/captured views, slices and temporary spine cleanup. |
| Move/Drop/replacement/return | The result moves as an ordinary dynamic array, while source text remains borrowed. Result and source spines are physically distinct, but existing conservative source loans remain; byte-owner and bound-source invalidation are forbidden while a result is live. `string_sort_order_and_stability` exercises repeated sort and scratch exits; `string_sort_lifetime_and_control` covers return, replacement and use-after-move. |
| Control paths | `if`, `match`, `else`, `?`, `map_err`, branch/loop joins and early return retain the existing content facts. Parameterized `string_sort_lifetime_and_control` cases exercise selected string arrays and escaping/later-invalidated views. |
| Generic/interface/whole/per-unit | Imported helpers over `slice<str>`, `array<str>` and a generic `Ord` wrapper instantiated with Copy elements execute in both modes. No interface schema change. `string_sort_order_and_stability` owns imported compilation parity. |
| Algorithm/allocation parity | Existing insertion threshold, ordered early exit, merge and ordered-boundary paths compare `str` through the existing scalar comparator. Empty/singleton/31/32/33/63/64/65/129, sorted/reverse/duplicates/prefix/UTF-8/NUL witnesses compare exact sequences. Keyed witnesses identify equal-key order and exactly-once effects. Existing `sort_adaptive` is the numeric/float and allocation owner; new MIR assertions pin no string cloning and shallow scratch cleanup. No new performance promise or benchmark gate. |
| Deferred | Move `string` elements/keys, comparator overload, struct sorting, richer callable ownership, consumer adoption and K1. Request 27 remains partially fulfilled until its broader consumer requirements are closed. |

## Author ledger-to-prose pass

The domain is the existing Copy scalar `Ord` hierarchy, with no boolean or
aggregate order. Source storage and content ownership are distinct: `array<str>`
frees its spine, while `array<string>` recursively frees owned bytes. The two
outdated `str` deep-Drop phrases in the touched core array document must name
`string` instead. The region fold and pipeline content facts must agree before
new source admission; merely allowing comparison would leave returned local
views unsound. No new unsafe native operation or cache representation is needed.

Required agreement: draft.md ordering, docs/language-spec.md ordering,
docs/design-notes.md, Settled docs/open-questions.md scalar ordering,
checked-HIR ledger 19, current roadmap, core array document and its ja mirror.
Acceptance programs syntax-check declarations separately from positional calls.
HANDOFF records this capability once after verification. The external request
register's provider answer must state the Copy-only scope and consumer-owned
verification; its directory is currently read-only, so keep that answer locally.

## Independent plan review

The fresh inspection-only adversarial review found no actionable P1/P2. It
checked the str-only region fold, existing capture/content generation facts,
shallow result/scratch cleanup and whole/per-unit owner boundary before coding.

## Author implementation closure

`check_array_sort` admits the existing scalar Ord domain under concrete Copy
restrictions and symbolic Ord/Num bounds; instantiated bodies recheck String
rejection. `check_array_sort_by_key` admits str alongside its existing Copy
primitives. Checked HIR authenticates the direct ordering domain, final element,
result/source identity and key signature; the shared `pipeline_callable_ok`
already enforces Copy input/output and captures, so no second key-copy guard is
needed. Both str terminal forms join source/stage allocation regions through the
existing collect fold. Existing header/content generation, pipeline element and
move/escape paths already explicitly include both terminals. MIR sorting,
allocation, comparator, donation and cleanup code are unchanged.

The new driver owner covers 30 size/state cases through both direct and str-keyed
sorting, a 65-element stable length-key oracle, empty/singleton key effects,
characters, NUL/UTF-8/prefix ordering and local arena views. Imported generic
Str/Char execution and concrete String refusal close symbolic admission.
Control positives traverse if, match, else, try, map_err and replacement; the
parameterized direct/keyed negatives cover local/capture return, byte-owner
replacement/move, iteration Drop and arena return in both compilation modes.
Forged HIR covers result, source, local ordinal, element and key identity in all
four lowering entrypoints, including an otherwise consistent Bool terminal.

The ordering-domain omission mutation fails its named checked-HIR owner.
A trial duplicate key-copy guard was removed: the shared callback validator
already rejects that same malformed Move key. Omitting one auxiliary content
fact arm still leaves the independent header/lifetime paths enforcing the
invalidation owner; that probe is not claimed as a discriminating mutation.
Positive restoration and the final gate qualify the actual complete paths.
The existing float/NaN and tiny key-effect owners pass. No performance or native
allocation-policy claim is introduced; no runtime ABI inventory changes.

## Source-generation boundary clarification

An author-side positive probe replacing a bound source `array<str>` failed the
existing invalidation authority. The original plan's promise that the result
introduced no source-spine loan was stronger than the established collector
contract. The ledger now preserves existing conservative source-generation
loans for both terminal forms. Temporary-source cleanup remains the existing
hidden-owner path, and no generation-provenance precision or K1 work is added.
The parameterized `source-spine` negative pins this constraint in both modes.
This changes the stated admission boundary, not physical storage ownership,
byte cloning, native cleanup or the compiler's borrow strategy.

The fresh narrow boundary review accepted the conservative source-loan scope
and found one P2 prose conflict: the adjacent English/ja Regions section still
claimed materialized results had no region. Both now distinguish heap/arena
spine allocation provenance from retained string content lifetimes/source loans.
The same owning record and mirrors agree; no compiler strategy changed.
