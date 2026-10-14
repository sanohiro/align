# Linear JSON object key materialization

## Capability and evidence

Voice V001 needs all keys of a real 151,000-member tokenizer vocabulary.
The unchanged consumer reproducer over 4,000/8,000/16,000 members produced
five-run median traversal times of 29.885/119.761/479.643 ms on this Mac;
parsing is excluded, and every result's key-byte count was checked. The local
baseline record retains exact source/compiler/input identities and samples.
Media safetensors and LLM request/header code also pair elems() with key(i).
The tape already carries sibling-skip offsets; this is a missing contiguous
key materialization, not a need for iterators, maps or another ownership model.

One capability supplies source admission through native output, lifetime and
cache validation, and the public English/Japanese contract. Splitting the
dormant producer from its consumer would duplicate the safety proof. Expected
hand-written changes may exceed 1,000 lines because both materialization modes
must share complete compiler-pass and malformed-record owners; one mergeable
boundary avoids inconsistent output types and duplicated traversal policy.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Source | `d.keys() -> slice<str>` for `d: json.doc`, exactly zero arguments, with the existing core.json import rules and an explicit enclosing arena. No defaults, type arguments, options or ambient configuration. Existing inferred purity matches elems(). |
| Order and presence | Object members produce exactly one decoded key each, in original document order. Preserve every duplicate, including distinct JSON spellings of the same decoded bytes. For an Object, keys()[i] and elems()[i] refer to the same member for every valid i. Array, String, Number, Bool, Null, Missing and empty Object yield an empty slice. Array elements are not object keys. |
| Encoding and errors | Input was fully validated as UTF-8 JSON by json.doc; keys are semantic UTF-8 bytes, decoding every settled short escape and Unicode surrogate pair, including embedded U+0000. No new fallible result or error type. Parsing retains existing Error behavior. Allocation/representability exhaustion retains the arena's terminal allocation policy; no silently truncated successful result. |
| Ownership and lifetime | The Copy slice header and every str element are views. The header array and all escaped-key decoded bytes are allocated in the explicit enclosing arena at the keys() call; clean keys directly borrow their input spans. The entire result conservatively depends on both the receiver doc's input/parse-arena lifetime and the materialization arena. No element may escape either dependency. No input retention beyond those static roots, individual Drop, hidden heap copy, new owner, or shared-view write authority. Repeated calls explicitly materialize distinct header arrays, without a cache. |
| Allocation and work | One contiguous n-element AlignStr header allocation (16 bytes per member on supported 64-bit hosts), plus one exact decoded-byte arena allocation per escaped key; no clean-key byte copy and no per-member heap owner. One forward level traversal using key.next and value.next; O(n + total key bytes) work, independent of nested value subtree sizes. Indexing the resulting slice is O(1). Arena chunk overhead follows the existing arena allocator. No fixed speedup or application-throughput promise. |
| Existing surfaces | elems() retains its exact values/array behavior, output shape and arena policy. key(i), get and as_str retain their existing results, duplicate policy and parse-arena allocation; key(i) still performs its documented prefix walk. Factor decoded-string writing with an explicit destination arena so keys() does not allocate into a different arena. |
| Internal representation | Replace HIR/MIR JsonDocElems with one JsonDocCollect record carrying `kind: JsonDocCollection` (`Values` or `Keys`), receiver doc and, in MIR, current ArenaHandle/private output slot. Values produces Slice(JsonDoc); Keys produces Slice(Str). MIR returns Unit. Discriminator, exact types, replay, body identity and all lifetime/storage/effect/move walkers are exhaustive. No new public type, Move/Drop path or general provenance capability. |
| Native ABI | New key JsonDocKeys / symbol align_rt_json_doc_keys uses existing A71 `void(ptr tape, i64 node, ptr arena, ptr out)`, Rust `unsafe extern "C" fn(*const DocTape, i64, *mut Arena, *mut AlignStr)`. Output is the ordinary pointer/i64 slice header; each element is the pointer/i64 UTF-8 str header. Effects IndirectStorage, ArgMem Unstated, params [], escapes [0,2,3], no release/fresh/divergence, matching elems(). |
| Native boundary | Caller supplies a null or live parser-produced tape, live immutable input for that tape, null or live destination arena, and null or aligned writable disjoint output header. Nonnull arbitrary/malformed tape addresses are outside this private ABI. Null out is a no-op before other reads; otherwise null arena, null tape, negative/out-of-range node, wrong node kind and zero members publish `{null,0}`. Supported hosts are 64-bit. Checked header extent precedes allocation. A live valid Object publishes all n initialized key headers only after the complete walk; there is no partially published slice. Internal impossible validated-key decode failure terminates rather than returning a successful short/empty Object. No file, native process-global or connection-global state. |
| Identity and counts | Existing type/interface layout is unchanged. Private HIR/MIR collection discriminator participates in compiler/body cache identity; generic exported bodies reconstruct from source, and cold/hit replay preserves the mode. No new user wire/persisted format, reflection or runtime artifact I/O. After Plan185: 468 keyed, 486 base, 493 alloc-count, 490 par-map, 486 task and 497 maximum symbols. Registry, native typed witnesses and independent export manifests agree. |
| Sources and owner | This Plan186, draft.md, docs/language-spec.md, docs/design-notes.md, Settled docs/open-questions.md, core-design/json English/Japanese, HIR ledger19 and runtime ABI ledger20. No future milestone dependency. Voice owns removal of its rewrite, compiler pin and original-tokenizer parity qualification; no consumer code is modified. |

## Implementation closure matrix

| Axis | Implementation and acceptance owner |
| --- | --- |
| Formation | Shared materialization checker and checked-HIR record derive exact output from kind. Parameterized valid/invalid receiver, result, argument count and missing-arena owners cover both modes. Check both clean and already-invalid source children without a compiler panic. |
| Native layout and decoding | Parameterized runtime json_doc collection owner checks Object empty/single/many, all non-Object kinds and Missing, null output/arena/tape and invalid node. Exact keys cover clean UTF-8, every short escape, Unicode BMP/surrogate pairs, NUL, duplicate decoded keys, empty keys and deeply nested values. Compare full key bytes and paired values to an independent expected sequence, not key(i) alone. Clean pointers equal exact input offsets; escaped pointers belong to the materialization arena, even when it differs from the parse arena. |
| Extent/publication | Checked n*sizeof(AlignStr), exact count/order and header initialization. Native canary output and empty/default controls; valid Object has no short publication. Existing parser owns raw tape validity and strict malformed JSON; reuse those owners. Native allocation accounting isolates the existing process-global probe in a bounded child if used. |
| Region and authority | Both modes retain doc/input/parse-arena roots plus current allocation region. Fresh header storage contents inherit receiver roots. Driver negatives reject slice or selected-key escape from input, parse arena or materialization arena, including nested arena / helper return / stored view; reject mutation through shared borrowed keys. Positive input+arena scope witnesses ensure useful ordinary reads compile. |
| Control-flow and cleanup | Whole/per-unit driver owner covers direct/if/match view selection and loop scalar results, ?/else/map_err parse failure, early return, repeated materialization, nested arenas and original-document reuse. Input remains borrowed, no source nulling or per-element Drop. Existing conservative refusal to break a materialized slice out of a loop remains. Existing arena cleanup frees staged headers/bytes on every exit, without a new partial heap owner. |
| MIR exactness | Explicit contract derives Unit result, JsonDoc receiver, ArenaHandle and private Slice(kind.element) output. Mutate discriminator/output mismatch, receiver, arena, status, invalid slot and provenance; safe rejection precedes LLVM. Both valid discriminators lower to their exact runtime symbol. Shared output provenance remains shared. |
| Pass/replay/interface sweep | Every former JsonDocElems/JsonDocElements match becomes a kind-preserving exhaustive collection arm; enum variant sweep, region/storage/move/effect/closure walks, HIR depth, replay clone, MIR producer/loop facts/print and lowering. Imported ordinary and generic helper source, whole-program/per-unit and cold/hit frontend cache owners verify all keys and values. No native pointer serialization. |
| ABI | Registry A71 signature/effect owner, compile-time native signature witness, golden symbols and full feature-count/export audit; same widths and key identity in codegen. |
| Performance evidence | Local before/after 4k/8k/16k and real-size 151k corpus, parse excluded, repeated samples, exact source/compiler/input identities and all-result validation; count allocation bytes separately if measured. Baseline key(i) and new keys() loops have the same observable keys. A paired elems()/keys() case verifies semantic member pairing; timing is not a CI correctness gate. |
| Fixtures and gates | New driver owner uses helpers/owned_fixture.rs, direct compiler APIs and parent-owned bounded subprocess guards. No predictable TempProject pre-removal. Run native json_doc, focused HIR/MIR/storage, driver collection owner and ABI exports locally, then one final-SHA preflight with code review. |

## Author consistency pass

Every normative promise above has an exact source/result, lifetime/allocation,
native producer and acceptance row. Object/non-Object, duplicate spelling,
escaped/clean and same/nested arena axes have total behavior. Source examples
are syntax-checked during implementation. The fresh independent adversarial plan review was CLEAN before implementation,
including native lifetime/allocation, exact MIR output/provenance and owner coverage. No provider delivery is claimed by this plan.


## Author closure

The extracted must/exact/every/before/reject/required obligations map to the
native `json_doc_keys_order_decoding_arena_and_empty_matrix`, source
`json_doc_keys_lifecycle_and_replay` and
`json_doc_keys_rejects_invalid_source_and_escaping_views`, and paired
`json_doc_collection_hir_exact_discriminator_and_arena` /
`json_doc_collection_mir_exact_outputs_and_symbols` owners. The storage variant
sweep includes both collection kinds. Existing json_doc parser/accessor owners
remain the malformed-input and Values behavior controls. Runtime ABI registry,
typed native witness and independent feature/export audit own all widths,
ordinal/signature/effect and count promises. The local
[measurement](../../bench/json_doc_keys/README.md) records the scaling witness;
it does not replace correctness tests.

The author pass checked the entire former JsonDocElems pass set, exact Slice
result from the collection discriminator, conservative shared output authority,
source/input and both arena roots, current-arena escaped-byte allocation, complete
publication and checked extent arithmetic. No Move field, alias authority,
cleanup strategy, native global state or public representation was widened.
Both source and native fixtures own their scratch/resources before fallible work.
