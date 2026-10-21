# Explicit document rendering policy

## Selection and evidence

LLM R159 is the next non-deferred request after R158 adoption. Its Qwen template
needs comma/colon followed by one ASCII space and decoded/re-escaped JSON strings.
R158 compact/token-preserving output is correct and remains the default. Exact
number lexemes remain mandatory; emulating a consumer's lossy numeric conversion
is outside this capability. Media/voice registers were reconciled; no overlapping
new rendering request or consumer adoption is selected.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Options | Ordinary builtin Copy record `json.encode_options { space_after_comma: bool, space_after_colon: bool, reescape_strings: bool }`, fields in that declaration order, no implicit field defaults. No ownership, allocation, lifetime or access authority. Literal, local, parameter, field, function/control result use the existing ordinary record rules. The dotted reserved nominal name cannot be redeclared by user code. |
| Calls | `json.encode(document: json.doc, options: json.encode_options) -> Result<string, Error>` and `json.encode_bounded(document: json.doc, max_bytes: i64, options: json.encode_options) -> Result<string, Error>`. Declarations here describe positional expressions, not named-call syntax. Existing one/two-argument calls retain their exact contracts and act as all-false options. Explicit options require a named document root/subtree; typed record/array/sum encoders reject extra options. No dynamic value tree, automatic reflection or template mode. |
| Separators | Each comma between array elements or object members is followed by exactly one ASCII space iff space_after_comma. Each object colon is followed by exactly one ASCII space iff space_after_colon. No space inside empty containers, before punctuation or at boundaries. All four separator combinations cross both string policies. Newlines, arbitrary separators and pretty indentation are not part of this boolean policy. |
| Strings | False preserves original string/key tokens exactly as R158. True decodes each key/value body incrementally and re-escapes: quote/backslash become \"/\\; backspace/formfeed/newline/return/tab use their short escapes; remaining U+0000..001F use lowercase \u00xx; slash is unescaped; all other Unicode scalars are raw UTF-8, including U+2028/U+2029. Valid UTF-16 surrogate pairs become one scalar. No normalization, ASCII-only escaping or HTML escaping. Same rule for keys and values, duplicates and member order preserved. |
| Other values | Numbers retain every original byte (negative zero, exponent spelling, huge integers/exponents); boolean/null and all existing real-kind/subtree semantics stay unchanged. Missing yields Invalid. No numeric parsing, float conversion, lossy canonicalization or key sorting. |
| Order/errors | Check arity, existing source admission, exact i64 limit if present, then exact nominal options type. Runtime evaluates limit then options once in written order while reserving the source/backing; divergence propagates and invalidation rejects under existing eager source checks. Negative bound returns Invalid without inspecting document data, after all source-language argument effects. Native malformed flags, tape or spans fail closed. Missing/exceeded cap return Invalid, no partial owner. All errors share Error.Invalid; no observable new error precedence discriminator. |
| Ownership/allocation | Existing borrowed doc/source/arena through both eager operands and rendering; output one independent owned UTF-8 string, valid after all sources expire. One existing bounded output builder, no decoded-string scratch, dynamic tree or traversal heap allocation. Constant <=4-byte UTF-8 scalar scratch and existing depth-bounded stack walk. All separator/escape bytes count against the inclusive output bound; reserve before each write/growth, fail as soon as the next output fragment cannot fit. No unlimited pre-normalization/counting pass. Existing allocator rounding/realloc overlap/parse storage exclusions remain. |
| Native ABI | Change existing private align_rt_json_encode_doc(builder:ptr,tape:ptr,node:i64,flags:i32)->void. Bits0/1/2 are comma-space/colon-space/reescape; others reject. No legacy ABI/shim; default compiler calls pass zero. Builder header/layout, public runtime ownership and output finalization stay unchanged. Fixed native identity, declarations, runtime signature pointer, effect ledger and malformed ABI owner update together. |
| Compiler proof/identity | JsonEncode HIR gains optional options expression; MIR gains optional options operand. It is permitted only for the single authenticated document piece and the exact builtin record shape. All evaluation/move/effect/region/replay/depth/remapping/operand-validation/optimization passes visit it. MIR validates type and readable producer; codegen extracts three Boolean fields and constructs the closed flags. Existing compiler fingerprint invalidates artifacts; builtin nominal field graph is represented by ordinary interface records, no new wire tag or ambient configuration. |
| Limits/owners | Source/HIR/MIR negatives; all eight policy combinations and frozen byte goldens; exact/short/zero/negative limits, huge escaped text under tiny caps; source expiry/move/replace/cleanup; imports/generic/cold/hit/change/revert; numeric token preservation; ABI signatures/exports. Extend R158 runtime and driver owners, retain old default matrix and typed JSON owners. No benchmark: no throughput claim. |
| Agreement | core-design/json EN/ja, draft/spec/design-notes/Settled existing JSON paragraphs, Plan237 forward note, HIR/runtime ABI ledgers, HANDOFF capability boundary and R159 provider receipt. Update only changed normative records. Consumer template and Codex/tool/image acceptance remain external. |

## Implementation closure matrix

| Axis | Implementation and discriminating owner |
| --- | --- |
| Type formation/transport | Central builtin options shape; both encoder checkers, ordinary Copy construction/field/function/control/import/generic transport; missing/extra/wrong fields, arbitrary similarly shaped record, extra options for typed root and malformed checked-HIR definition reject. No Move/Drop/nulling strategy changes. |
| Eager effects/source lifetime | Every HIR child visitor including MoveCheck, checked replay/depth, origin/effect analysis and lower_json_encode carries options after the limit. Counted effect/order owner crosses runtime options, source invalidation in options, early return/?/match/if/loop choices and a safe independent effect. No stale doc handle can reach native code. |
| Plan/IR | Source proof still authenticates canonical root and exact one Doc piece. Forged options on other pieces/owned plan or wrong type/operand reject independently before LLVM. MIR walkers include the operand in use/rewrite/validation; producer admission requires readable Copy record. Whole/per-unit and forged-HIR/MIR owners close omission risk. |
| Bytes/bounds | Runtime writer parameterizes punctuation and incremental string decoding. Independent UTF-8/escape/control/surrogate and numeric goldens cross all policies and root/get/at, duplicates, empty/nested containers. Exact inclusive cap, one short, zero/negative, huge compressible escapes under tiny cap and partial-prefix cleanup retain bounded capacity. Malformed flags/spans/nodes/escapes reject. |
| Ownership/cleanup | Reuse builder success transfer and failed finish. Requested-live positive witness/zero retained bytes after repeated success/failure, output survives arena/source, returned output moves/replaces/drops. No input or arena mutation. Existing R158 lifecycle owner continues unchanged for defaults. |
| ABI/replay | Update single native signature/effects/export inventory and codegen exact physical arguments; native collision negatives. Options record survives imported concrete/generic bodies, whole/per-unit and cold/hit/changed options/exact revert. Compiler identity owns invalidation. |
| Documentation/consumer | One ledger decision propagated to both language specs and EN/ja library design. R159 receipt records exact calls/flags/limits and provider tests only. No consumer code, pins, commits, branches or adoption modifications. |

This is one complete renderer capability, with its option expression transported
through the existing encode action. The implementation may cross more than three
compiler layers because source effects, IR certification and native rendering must
agree; separate dormant option producers/consumers would duplicate proof. Expected
handwritten diff is below 1,000 lines; revisit and explain the boundary if exceeded.
Author ledger-to-prose pass complete before one independent strategy review.

## Independent strategy review

The fresh inspection accepted the three-boolean surface and complete renderer
boundary without actionable findings. Implementation must stream without full-span
unescape/counting helpers, authenticate the exact nominal builtin record, reserve
source backing across both eager operands and retain default/all-false parity.
The implementation follows those boundaries.

## Author closure and owner evidence

The matrix-to-diff pass closes the applicable cells without changing the source
reservation, ordinary Copy-record or builder ownership strategies:

- `align_runtime::json_doc_encode` crosses all eight byte policies and exact,
  short, zero and negative bounds, malformed metadata/escapes, no Rust scratch,
  requested-live cleanup and bounded output capacity.
- `align_driver --test json_doc_encode` closes native whole/per-unit execution,
  imported concrete/generic options, cold/hit/change/revert cache behavior,
  source invalidation rejection, eager effect order and early return/`?`/branch/
  loop choices. Independent expected bytes, source expiry and output replacement
  discriminate rendering and ownership. Ordinary record formation rules are
  reused; nominal lookalikes and options on typed roots have explicit negatives.
- MIR `json_doc_` and LLVM `json_doc_` owners independently reject forged child
  types, record definitions, producers and piece sequences; the runtime ABI
  owner authenticates declarations and the changed physical signature.
- Interface builtin-spelling owners cover nominal transport. Existing
  `json_field_encode`, `json_owned_record_array`, `json_record_array_grammar`,
  `deferred_aggregate_transfers` and `expr_depth` owners pass unchanged, covering
  default/typed encoding, source admission, cleanup and bounded traversal.

The changed HIR/MIR child inventories, replay, move/effect/region passes, builtin
identity and native signature were checked against the self-review gates.
Specifications and the English/Japanese library records agree with this ledger.
Consumer template/Codex acceptance remains consumer-owned.
