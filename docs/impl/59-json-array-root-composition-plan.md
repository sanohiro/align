# JSON dynamic-array root composition (R87)

Status: implemented; provider delivery gates and consumer adoption are separate.
The owner explicitly reopened R87 on 2026-09-12 after comparing the restriction
with Align's principles. No friction-count prerequisite remains. Plan 58's R87
deferral is superseded. This is one compiler capability; consumer adoption remains
external to Align.

## Public contract ledger

A dynamic array already representable as a JSON record field is also representable
as the root. Root position adds no element exclusion. This capability closes dynamic
array root composition; it does not introduce new element constructors, decode
semantics, implicit conversions, customization hooks or another encode operation.

| Surface | Exact record |
| --- | --- |
| APIs | Existing `json.encode(value: T) -> Result<string, Error>` and `json.encode_bounded(value: T, max_bytes: i64) -> Result<string, Error>`; `import core.json`; no defaults or new syntax. Existing local binding/shared-parameter/shared-match source forms apply. |
| Array domain | Dynamic `array<E>`, AoS for records. `E` is every existing JSON array element: signed/unsigned 8/16/32/64-bit integer, f32/f64, bool, str, string, or a record accepted by the corresponding existing JSON field route. Copy/view-bearing record schemas use the existing descriptor grammar; transitive owned-text record schemas use the existing closed V3 owned graph. No whole-array copy or user wrapper selects admission. |
| Closed exclusions | Existing element exclusions stay identical: arrays of options/arrays, unsupported enum element arrays, resources/opaque handles, invalid layouts and rejected mixed owned/view record graphs do not become encodable by moving to the root. Fixed arrays/SoA/slices retain their existing independently specified contracts. |
| JSON bytes | `[]` for empty; ascending element index, comma-separated, no whitespace; record fields in declaration order; existing string escapes, Option-field omission and R63 numeric spelling. A root array's bytes equal the value bytes of the same embedded array, excluding only the enclosing field name/object syntax. |
| Ownership / lifetime | Both operations borrow the complete input through completion; neither spine nor element is moved, nulled, cloned or retained. Input may be reused or dropped afterward. Success returns one independently owned string, including in an arena, and never retains an input/view lifetime. Existing input validity and single Drop remain unchanged. |
| Errors / validation | Source schema/type checking precedes HIR publication. The optional bound is checked as exact i64 after source admission; runtime evaluates existing argument expressions in order. Negative bound, nonfinite selected float or byte-limit overflow returns `Error.Invalid`; inclusive exact-fit succeeds; empty requires two bytes. Existing sticky builder failure/precedence applies, no partial output is published, and failed output storage is freed. Allocation failure retains the terminal policy. |
| Allocation / owner | `core.json` / compiler own schema selection and lowering; runtime owns the existing JSON builder and descriptor-driven array writer. Same output allocation model as encoding the embedded field; no source allocation/copy, scratch wrapper, or new retained descriptor. Descriptors are compiler-produced static data. No performance/resource improvement claim or benchmark gate. |
| ABI / artifact / cache | Existing `JsonEncodeObject`, `JsonEncodeStructArray`, `JsonEncodeScalarArray` ABIs and descriptors. No new runtime export or serialized interface field. V3 remains the exact record graph (root record/array element record); the array container is authenticated by the typed operand and existing interface function types. Compiler-artifact fingerprints invalidate old compiled code. Record graph depth retains its existing bound; root iteration uses the same per-record writer as embedded array iteration. |
| Prerequisite / acceptance | Uses already-shipped R63 builder errors, R85 borrowed-place reads and existing AoS/owned-record transport. One portable owner covers both encoders and whole/per-unit execution; native malformed-producer owners reject incorrect element type/graph/uninitialized or borrowed input laundering. |
| Sources of truth | draft.md, language-spec, design-notes, open-questions, core-design/json.md and ja mirror, HIR ledger 19, recursive-owned plan 25, numeric plan 47, plan 58 deferral, HANDOFF, and the external R87 register. Amend normative promises only; no version or milestone bump. |

## Implementation closure matrix

| Axis | Implementation closure | Exact owner or unchanged authority |
| --- | --- | --- |
| Formation / validation | Shared scalar-array encode predicate includes owned string; dynamic record roots choose the same existing record schema route. HIR source reconstruction binds one array piece to the exact declared base/type; owned plan binds exact root or element id. No permissive raw-template fallback. | `client_admission_composition::json_dynamic_array_root_matrix`; HIR root/graph/source mutation owner |
| Construction / move / source nulling | Arrays remain existing constructed/bound owners. Ordinary Local lowering preserves authenticated borrowed projections. Every encode form reads without transfer. | Root matrix: repeat encode, source reads after encode, borrowed optional carrier and negative consuming/escape controls |
| Drop / replacement / return | Existing builder guard owns partial output; input remains live on failure; success can outlive source and cross a function/interface boundary. Arrays/element owners still drop once. | Whole/per-unit source-expiry, failure-followed-by-reuse and positive alloc/free cleanup owner |
| Controls | None/Some selects the source through existing match; bounded Err uses existing Result formation. if/match/else/?/map_err/loop/early return share existing JsonEncode lowering and input bookkeeping; no new branch opcode. | Parameterized composition fixture and existing `value_control_flow` owner; malformed source replay |
| Generic / interface / per-unit | Existing record graph remapping/serialization and function array types remain authoritative. Generalize MIR owned-record JSON piece name to describe one record or a record array; every exhaustive consumer changes together. | Imported generic and non-generic array roots; whole/per-unit execution; interface owner and malformed graph identity |
| MIR / LLVM / native | Typed owned-record piece selects object versus AoS array call using the existing V3 table and native element allocation stride. Other record arrays use existing descriptor array pieces. Owned string array elements use the same 16-byte text value writer with no ownership operation. Reject wrong scalar tags, record id/graph and layout before codegen. | MIR producer mutation owners plus canonical bytes/nonfinite/string-escape/empty tests; existing runtime ABI owner |
| Element parity | int widths/sign, floats, bool, str/string and Copy/owned/nested/optional records accepted exactly where an existing field can encode them. Unsupported element families remain negative. | Root-vs-embedded value byte matrix, exact/under/negative caps, unsupported element controls |
| Allocation / provenance | No new native ABI or allocation strategy. Output builder remains the only new owner; input storage remains borrowed. | Explicit positive allocation/free deltas and returned-output/source-expiry controls; existing builder limit owners |

## Author plan consistency pass

The public record fixes source forms, all supported element discriminators,
empty/nonempty and bounded/unbounded outcomes, error/ownership/allocation rules,
byte ordering and the unchanged descriptor/ABI/cache identity. Required examples
will be executable owner sources. No milestone or performance claim is consumed.
Before implementation, one fresh independent inspection checks this ledger,
source-to-MIR certification and native descriptor/ownership boundaries. Before
publication, align-self-review and one fresh full-diff review check the implemented
matrix; local owner and preflight gates precede pushing and CI precedes merge.
After merge run exactly `cargo build --release --workspace`; update only the
external request register and leave that consumer edit uncommitted.

The independent strategy inspection accepted this boundary. Its closure points are
exact typed root/source binding, complete bracket-owning array pieces, encode-only
owned-text admission, reservation before bound evaluation, concrete generic record
monomorphs, and the negative/cleanup matrix above. No ABI or interface change is needed.

## Author matrix-to-diff closure

The extracted exact/before/reject obligations map to `json_encode_parts`,
`owned_json_source_ok`, `json_encode_parts_match_source`,
`template_piece_is_type_safe`, `owned_json_piece` and the two existing array-writer
LLVM branches. All admitted scalar widths and both record routes share the root/field
byte matrix. The owned HIR mutation sweep and borrowed-place producer owner now
parameterize record versus record-array roots; the additional array-piece HIR owner
rejects forged source/element identities and extra delimiters.

`json_array_root_output_survives_borrowed_optional_source` covers concrete generic
records, imported shared projections and output-after-input lifetime.
`json_array_root_nonfinite_failure_and_cleanup` and
`json_array_root_arena_output_has_independent_cleanup` activate native live-byte
accounting, prove a positive live allocation, and require zero after failure/success
cleanup. `json_array_root_rejects_unsupported_elements_and_source_replacement`
closes unchanged exclusions and source reservation through bound evaluation.
Existing `value_control_flow` owns unchanged Result/control dispatch. No new native
ABI, allocation strategy, branch opcode or unclosed representation cell is introduced.

The self-review pass checked the complete renamed MIR consumer set, encode-only
scalar allowance versus decoder grammar, exact typed graph authentication, existing
record stride and string-header ABI, failure sentinels, source-before-bound checking,
and the English/Japanese contract amendments. No performance claim is made.
