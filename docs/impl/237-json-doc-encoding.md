# JSON document subtree encoding

LLM Request158 needs arbitrary tool parameter schemas to cross an already
validated `json.doc` boundary without a second application JSON tree or writer.
The three consumer registers were reconciled on 2026-10-21. Media and voice
already use document navigation and typed encoders; their typed schema, string
escaping, ownership and numeric conversion contracts must remain unchanged.
This capability supplies provider serialization, not consumer protocol adoption.

## Public contract

| Surface | Exact rule |
| --- | --- |
| API and input | Existing `json.encode(value) -> Result<string, Error>` and `json.encode_bounded(value, max_bytes: i64) -> Result<string, Error>` additionally admit a live `json.doc` as a named local or parameter, using existing stable source authority. A named `get`/`at` result is a subtree. Ordinary record formation excludes `json.doc` fields; this capability does not widen it. No new method, default, ambient setting, general dynamic value or typed-record JSON field admission. |
| Values | Every real document kind is accepted: object, array, string, number, boolean and null, including empty containers. Missing is not a JSON value and returns `Error.Invalid`. Objects preserve original member order and every duplicate occurrence; arrays preserve element order. |
| Bytes | Emit compact JSON: only structural punctuation separates members/elements. Copy each validated string token's original body and escapes between quotes, including key tokens; preserve the decoded Unicode/NUL bytes without an unescape/re-escape allocation. Copy each number token byte-for-byte, including negative zero, exponent spelling and numbers outside i64/u64/f64. Boolean/null spellings are fixed. Whitespace inside string tokens remains data. No normalization or canonicalization promise; typed encoder bytes and numeric formatting do not change. |
| Ownership | Borrow the document, source bytes and parse arena only through the call. Existing source/backing reservation spans eager limit evaluation. Never move, mutate or retain input. Success transfers one independent owned UTF-8 string with ordinary Move/Drop, valid after source/arena expiry. Failure publishes no partial owner and frees private output. |
| Bounds and errors | Source/place/type/lifetime checking precedes runtime work. Evaluate the exact-i64 bound once before serialization. Negative bound, Missing, or exceeded inclusive output cap returns `Error.Invalid`; exact fit succeeds. Negative bound takes precedence and skips document traversal. Zero cannot encode a real value. Once failure is sticky, stop traversal and growth. Allocation failure retains the existing terminal policy. |
| Allocation | Reuse the existing JSON builder, whose output capacity requests never exceed the cap. Transfer its output without a final copy. No tape, source, decoded-string, descriptor or traversal heap allocation. Traversal uses bounded stack depth (the parser's existing maximum 128). Earlier parse storage, allocator rounding and transient realloc overlap are outside the output bound; this is not an RSS ceiling. |
| Owner and identity | `core.json`, sema checked source/parts, MIR JSON pieces, LLVM runtime registry and runtime tape writer own this capability. New compiler-private part/piece and runtime append symbol; no document layout, user ABI, interface wire version or persisted format change. Imported source is rechecked; compiler identity invalidates derived caches. No global native state or inspection table. |
| Prerequisites/performance | Existing document tape, stable JSON source and bounded builder contracts. No later milestone prerequisite and no speed promise or benchmark gate. Allocation-bound acceptance is instrumented correctness coverage. |
| Sources/mirrors | draft.md, docs/language-spec.md, docs/design-notes.md, docs/open-questions.md Settled JSON entry, core-design/json.md and ja/json.md; checked-HIR ledger19, ABI ledger20, existing numeric plan47 and source-place plan227 receive only necessary contract cross-references. HANDOFF records delivery once. |

## Implementation closure matrix

| Axis | Implementation and acceptance owner |
| --- | --- |
| Formation and exact source | Add one `TemplatePart::JsonDoc` under existing `JsonEncodePlan::Pieces`; both encoders share `json_encode_parts`. Checked HIR reconstructs one exact document source and refuses wrong kind/type/path, missing/extra/reordered pieces or document parts in ordinary templates. `validate_hir_tests` mutation matrix and `json_doc_encode` source-domain owners cover named locals, parameters, named get/at and unchanged record-field/typed-schema exclusions. |
| MIR and all compiler passes | One `TemplatePiece::JsonDoc` carries the ordinary lowered Copy handle. Enumerate every part/piece walker, replay clone, depth, move/region/liveness, validation, operand remap, optimization, structural fingerprint and display pass. Producer certification reads an initialized exact JsonDoc operand and produces only an independent string. Malformed HIR/MIR and ABI owners reject forged scalar/source/slot pieces before LLVM. No synthetic expression-address memoization. |
| Tape semantics | Runtime writer follows the producer's preorder tape, subtree skip and counts; bounded recursion and checked span/index arithmetic prevent malformed metadata from panicking. `align_runtime::tests::json_doc_encode_*` parameterizes all kinds, nesting, duplicates, escaped keys/text, Unicode/NUL, exact numeric tokens, selected get/at and Missing. Independently written expected bytes, not another invocation of this writer, own output. |
| Limit/order/allocation | Shared bounded BuilderBuf checks before growth. Parameterized native owners cover negative/zero/exact/short caps, repeated failed/successful writes, large escaped string and wide/deep subtree under a tiny cap. Observe capacity/request peak and cleanup, excluding already-created parse storage. Include null/invalid node/span metadata and negative-bound-before-doc controls. Existing typed bounded owners remain mandatory. |
| Move-in/out and cleanup | No new input owner; output uses existing owned Result lowering and failure finalization. `json_doc_encode` exercises source reuse, returned output after source/arena destruction, move, replacement, discarded output, repeated success/failure and no retained native bytes. Existing typed ownership owners remain regression controls. |
| Control and lifetime | Driver owners cover if/match/else/?/map_err, branch/loop joins, early return and diverging limit through the common Result path. Negatives cover expired source/arena, source mutation/drop in eager bound, overlapping access and malformed already-invalid inputs; controls remain valid. |
| Imports/generics/cache | Same source corpus checks and executes whole/per-unit; imported document helper, generic typed control, cold/hit caches and sibling encoders in equality/call arguments preserve independent source identities and output. A changed literal invalidates cached output. |
| Native ABI | `align_rt_json_encode_doc(builder: *mut Builder, tape: *const DocTape, node: i64) -> ()` appends or sets sticky failure. Existing finish returns status/output. Mutable builder exclusively owns output; tape/input remain live read-only and never escape. Registry declaration, conservative effects, symbol audit and native signature assignment agree. |
| Existing JSON family | Run new doc encoder owners plus existing `json_doc_keys`, `json_doc_error`, `json_field_encode`, `m5_json_bounded`, `m5_owned_json`, `mixed_json`, and narrow runtime json_doc/bounded owners. Typed strings/numeric formatting, unknown-field handling, duplicates, existing graph schemas and borrow cleanup are unchanged. |
| Consumer delivery | Update R158 only with exact provider surface, limits, validation and PR. Consumer pin, native Codex/image integration, smoke and final inference qualifications remain consumer-owned. No other pending request is closed. |

One complete producer-to-consumer PR closes both encoders, typed-regression
controls and lifecycle proof. If this exceeds 1,000 handwritten lines, splitting
the dormant compiler/runtime path would repeat the same source, bound and owner
proof without a useful intermediate consumer; keep the capability together.

## Author ledger-to-source pass

`DocNode` already retains original string/number spans and preorder subtree ends;
the parser validates complete UTF-8, escapes and number grammar and bounds depth
at 128. Reusing tokens preserves numeric semantics without conversion and avoids
unbounded escaped-string scratch during encoding. `BuilderBuf::reserve` checks
its inclusive limit before capacity growth; the existing JSON finish owns failure
cleanup and transfer. `JsonEncodeSource` and `json_encode_parts_match_source`
provide one exact input authority; a document piece must join that proof, not
bypass it. The matrix above covers each public rule with a discriminating owner
and deliberately retains typed encoder regression checks.

## Independent strategy review

The fresh `json_doc_strategy_review` inspected tape/parser, bounded builder, source
reservation, canonical HIR parts, MIR producer and LLVM ABI boundaries and found
no P1/P2 issue. It emphasized exact depth admission (0 through 128), sticky failure
before tape access, checked metadata traversal, and rejection of wrong-source or
ordinary-template pieces. Formation inspection additionally confirms json.doc
record fields remain excluded; there is no field-formation widening. Ordinary
block-local owners may remain alive until function cleanup: the MIR witness
places source Drop after encode, so lifetime negatives use an actual consumed
owner or ended arena, rather than mistaking lexical visibility for destruction.

## Author implementation closure

The extracted exact/before/every/reject obligations above map to these owners:

- `json_doc_encode_checked_source_and_piece_matrix` authenticates both HIR
  encoders, exact source identity, shape, type and ordinary-template exclusion.
  `json_doc_encoder_mir_exact_source_and_private_output` closes MIR operand,
  sequence, limit and private output authority through producer certification
  and LLVM emission. Part walkers, replay cloning, depth/Drop, move/escape/effect
  checks and lowering are exhaustive; MIR operand certification shares
  `XmlAccessAnalyzer::check_template_piece`, with no separate dynamic shape.
- Runtime `json_doc_encode_token_and_bound_matrix` owns every value kind,
  exact tokens, duplicate order, subtree selection and inclusive bounds.
  `json_doc_encode_depth_metadata_and_early_stop` owns depth 128, checked metadata,
  large input under a tiny cap and sticky failure. The isolated
  `json_doc_encode_output_allocation_and_cleanup` owns zero Rust scratch,
  bounded capacity, native output cleanup and no-copy final transfer.
- Driver `document_encoding_lifecycle_import_and_cache` owns the output lifetime,
  Result/control-flow family, eager-bound evaluation, source reuse, generic
  replay, whole/per-unit execution, rt-LTO, cold/hit/changed-literal caches and
  native allocation balance. `document_encoding_rejects_invalid_sources` owns
  source/arena expiry, eager-bound consumption/replacement, field formation,
  temporary source and bound-type exclusions in both frontend modes.
- Runtime ABI registry/declaration owners and `test-runtime-abi-exports.sh`
  bind A73, effects, exact exports and signatures across probe configurations.
  Existing JSON driver owners named in the matrix pass unchanged; no typed
  numeric, string, record-field or ownership contract was widened.

No matrix cell is deferred within this capability. Consumer protocol adoption
and record-held document formation remain outside its public contract.

## Explicit policy extension

[Plan238](238-json-document-render-options.md) adds an optional ordinary policy
record to both document encoders. Calls without it retain every byte/error/
ownership guarantee above. Its private ABI revision and streaming re-escaping
are owned by that plan; typed encoders retain their existing surface.
