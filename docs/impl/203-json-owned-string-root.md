# JSON owned-string roots

Plan183 supplies `array<string>` roots while the same independently owned string
still requires a wrapper record when decoded alone. This library composition
admits `string` through the existing scalar decoder. It introduces no type,
syntax, ownership mode or runtime signature. All three consumer registers were
reconciled: voice V005 is already delivered, and this closes the remaining
owned-text root documented beside it without performing consumer adoption.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| API | `json.decode(input: str) -> Result<string, Error>`, inferred from the expected target, with `import core.json`. One positional argument; an owned `string` input uses the existing shared borrow. No defaults, explicit type arguments, options, environment inputs or new syntax. Pure apart from argument effects. |
| Input/result | Length-delimited UTF-8 containing exactly one JSON string, with JSON whitespace before/after. Empty/clean text, existing short escapes, Unicode escapes, valid surrogate pairs and escaped U+0000 are accepted with their decoded bytes. Raw C0, malformed escapes/surrogates, invalid UTF-8, other JSON shapes and trailing non-whitespace return `Error.Code(1)`; no partial result. |
| Validation order | Compiler checks capability/arity/expected target, then input typing, before publishing checked HIR. Evaluate the input once; an eager exit performs no decode. Runtime validates the private tag, complete UTF-8, the string token, then trailing whitespace/EOF, then publishes. All recoverable parse/representability failures use Code(1); existing terminal allocation failure remains. |
| Ownership/lifetime | Success is an ordinary Move `string`, independent of input and lexical arena. Clean and escaped text both own their bytes. Returning from an arena, replacing/dropping input, generic forwarding, Result controls, source nulling, replacement and Drop use the existing model. Any borrowed view of the output remains bounded by that output owner. |
| Allocation/publication | Empty text is null/0 with no Align allocation. Each nonempty success has one exact decoded-byte allocation and no container spine. The parser owns that allocation until EOF succeeds; trailing-input failure frees it once and leaves the output header untouched. No retained pointer into input or arena. Success transfers the header once to canonical string Drop. No benchmark/performance promise. |
| Native ABI | Existing A05 `i32 align_rt_json_decode_scalar(ptr input, i64 input_len, i32 elem_tag, ptr out)`. String is exactly `0x810` (kind8, width16, sign0), a 16-byte AlignStr header. Valid numeric tags remain kind0 widths1/2/4/8 with sign bit16 zero/one, bool0x101, floats0x204/0x208; all other bits/tags reject before parsing. Safe callers supply valid input range and correctly aligned writable output. Compiler zeroes output and consumes it only on status0; string errors leave it untouched. Existing numeric failure bytes are unspecified and ignored. No symbol/key/effect/count/signature change. |
| Compiler/artifacts | Reuse `JsonDecodeScalar` with target String; checked HIR authenticates exact target/input/result and MIR the unique output slot/type and grounded input. Fresh storage is individual with no contained input/arena facts. LLVM emits the exact tag and zero/status branch. Type/Move/Drop and serializable IR shape already exist; compiler identity invalidates artifacts without interface format changes. No reflection, descriptor graph or artifact I/O. |
| Limits/prerequisites | Existing String, Result, scalar decoder and owned JSON string parser are prerequisites already shipped. Bare str, array<str>, char and doc-view admission remain separate. Numeric roots and existing array/record/union/SoA/scanner surfaces retain their contracts. |
| Sources/acceptance | This ledger, draft.md, language-spec, design-notes, Settled open-questions, core-design/json English/Japanese, HIR ledger19 and ABI ledger20 must agree. Plan183 gains only a forward reference. Native grammar/tag/sentinel/allocation, malformed HIR/MIR, and source lifecycle/cache owners below close the contract. |

## Implementation closure matrix

| Axis | Implementation and discriminating owner |
| --- | --- |
| Formation/validation | Admit String in the existing expected scalar branch; reject an already-invalid input before constructing HIR. Both HIR envelope and result derivation admit the exact String target; doc scalar checks remain separate. Parameterized checked-HIR and MIR owners mutate target, input, output/result type and output identity through full/partition lowering. Source refusals retain str/char/array<str>, wrong arity, wrong input and already-invalid input. Owners: `validate_hir_tests::json_string_decode_checked_hir`, `producer::tests::producer_json_string_output`, and `json_string_array_root::json_string_root_refusals`. |
| Native construction/failure | Reuse `owned_string_value`; retain unpublished owner until complete EOF and free on failure. One native owner crosses empty/nonempty/clean/escaped/UTF-8 success, all string grammar failures, every other JSON shape, trailing input and invalid private tags. Nonzero output sentinel proves no failure publication. Activated requested-live accounting and positive success evidence prove exact allocation, zero retained bytes and alloc/free parity without test cleanup hiding an error leak. Owner: `json_array_root_tests::json_string_root_grammar_and_cleanup`; controls: `json_array_root_tests::json_string_array_root_grammar_and_cleanup` and `m5::json_decode_top_level_scalar_error_paths`. |
| Move/Drop/replace/return | Classify the new owned target as fresh individual storage with empty borrowed content; numeric targets have no storage headers. Existing canonical string/Result transfer and source nulling handle move-in/out and returns. Whole/per-unit native owner crosses owned input replacement/expiry, lexical arena expiry, output replacement/discard, empty output and generic/imported forwarding, with positive allocation and complete free counts. Borrowed output view escape/replacement remain errors. Owners: `json_string_array_root::json_string_root_lifecycle_and_replay` and `json_string_array_root::json_string_root_refusals`. |
| Control flow | The same source owner crosses if/match/else/?/map_err, loop joins, early Err and eager-diverging input, observing status and cleanup after every scope. Owner: `json_string_array_root::json_string_root_lifecycle_and_replay`; Copy controls: `m5::json_decode_top_level_scalars` and `m5::json_decode_top_level_scalar_returnable_from_arena`. |
| ABI/cache/monomorphization | Native direct ABI tests pin exact tags/header/sentinels; emitted source execution pins compiler-to-runtime agreement. Imported concrete generic forwarding and whole/per-unit/cold/hit frontend replay must agree. Existing type identity and compiler-byte cache key suffice; no persisted record changes. Owners: `json_array_root_tests::json_string_root_grammar_and_cleanup` and `json_string_array_root::json_string_root_lifecycle_and_replay`. |
| Regression boundary | Reuse owned string-array root/field cleanup and numeric scalar grammar/range owners. Extend the existing string-array refusal case from newly admitted string to still-unsupported str. Existing controls: `json_string_array_root::json_string_array_root_lifecycle_and_replay`, `json_array_root_tests::json_string_array_root_grammar_and_cleanup`, and `m5::json_decode_top_level_scalar_error_paths`. Run focused owners, bounded local gate and Clippy before push; one fresh full-diff inspection runs on that same committed candidate. |

## Author consistency pass

The exact target, tag, publication order, allocation and lifetime promises each
have an implementation and observable owner above. No later milestone or
friction-ledger exception is required. The new native ownership class requires
one independent boundary inspection before implementation. The implementation
will remain one useful scalar-root capability, with no dormant producer seam.

The independent boundary inspection found no ownership/API strategy defect. It
required exact owner bindings instead of generic test descriptions; every matrix
row now names its planned or existing test target before implementation.

## Author matrix-to-diff closure

The extracted exact/every/before/reject obligations map to `check_json_decode`,
both uses of `json_scalar_target_ok`, the native output producer rule and the
LLVM tag. `native_storage_is_individual` and FreshEmpty storage agree with the
unchanged Static region and empty borrowed roots. The shared
`json_scalar_tag_valid` prevents scalar/array private-tag drift. Runtime string
publication follows EOF; the native sentinel/live-byte owner observes error
cleanup before any test free. Canonical String/Result lowering handles transfer.

All named owners passed: native JSON grammar/cleanup, checked-HIR/MIR mutations,
whole/per-unit/cold/hit lifecycle and refusal controls, and existing numeric
root tests. The source fixture also verifies exactly-once input evaluation and
no decode after eager termination. Existing scalar encoding admission is outside
this decoder capability. English normative sources and the Japanese JSON mirror
agree; no runtime ABI or interface-format inventory changes are needed.
