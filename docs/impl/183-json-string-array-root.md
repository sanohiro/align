# JSON owned-string array roots (voice V005)

Voice's NFC conformance input is naturally an array of strings. Its current
wrapper record is mechanically necessary only because the root decoder rejects
an existing owned element type. This capability admits that root through the
existing `JsonDecodeArray` operation. It uses ordinary `array<string>` ownership,
the existing string parser and deep Drop. The independently useful boundary is
one root domain; llm Request133's owned-record AoS decoder is a separate parser
and cleanup failure domain. Media AM-R02's mixed view/owner graph is also separate.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| API | `json.decode(input: str) -> Result<array<string>, Error>`, inferred from the expected type, with `import core.json`. One positional argument; owned `string` inputs use the existing shared input borrow. No defaults, type arguments, options, environment inputs or new syntax. Pure apart from argument effects. |
| Input and output | Length-delimited UTF-8. One JSON array containing only JSON strings, with JSON whitespace before/after and no trailing non-whitespace. Empty arrays/strings, every existing escape, valid surrogate pairs and escaped U+0000 are accepted. Preserve array order and decoded semantic bytes. Raw C0, malformed escapes/surrogates, invalid UTF-8, non-string elements, malformed separators/brackets and trailing input reject. |
| Error order | Capability/arity/expected target admission, then input typing, before checked HIR publication. Runtime validates the private element tag, then complete UTF-8, then parses elements left-to-right, then trailing whitespace/end-of-input, then publishes. All recoverable failures return existing `Error.Code(1)`; no partial array. Input expressions evaluate once; an early exit performs no decode. |
| Ownership | Success owns a contiguous spine of 16-byte string headers and every nonempty decoded payload. No input or arena provenance, including decode inside an arena; no retained pointer, implicit clone or wrapper. Indexing returns the existing owner-bounded `str` view. Move/source nulling, replacement, Result controls and deep Drop remain canonical. |
| Allocation and cleanup | Empty array has null/0 and no Align allocation. Nonempty array has one exact-size header allocation plus one exact-size payload per nonempty string. Per-call Rust staging Vec is transient; no fixed total-allocation or performance claim. The RAII staging list owns completed payloads until complete root validation; every error frees that prefix once in index order, with no header publication. Header-size/count conversion failure returns Code(1); allocator failure retains terminal policy. Success transfers all payloads to ordinary element-then-spine Drop. |
| Runtime ABI | Existing A05 `i32 align_rt_json_decode_array(ptr input, i64 input_len, i32 elem_tag, ptr out)`. String tag is exactly `0x810` (kind8, width16, sign0). Numeric tags remain widths1/2/4/8 with kind0 and sign0/1; bool0x101; floats0x204/0x208. All other tags reject even for empty input. Safe caller supplies valid input range and writable AlignStr; runtime leaves out untouched on error, compiler initializes null/0. No new export, layout, runtime key or signature. |
| Compiler/artifacts | Sema admits String only in the existing array branch; checked HIR reconstructs exact element/input/result; MIR certifies exact output slot and owned provenance; LLVM emits the tag and existing zero-output/status branch. No IR shape or interface format changes. Existing compiler identity invalidates cached artifacts. No descriptor graph, reflection or artifact I/O is introduced. |
| Limits | Bare string, array<str>, char and owned-record array roots retain separate admission. Borrowed JSON, numeric roots and owned record fields retain their existing contract. No new container/optional/error/ownership model. |
| Sources/acceptance | This plan, draft.md, language-spec, design-notes, Settled open-questions, core-design/json English and Japanese, HIR ledger19 and ABI ledger20. Existing array/String/Result support is the prerequisite. Native root grammar/allocation owner, checked-HIR/MIR mutation owners and whole/per-unit/cache source lifecycle owner below. No benchmark requirement. |

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Formation and malformed producers | Sema/checked-HIR admit exact String array target and Str input only. `json_string_array_decode_checked_hir` mutates element, input and result and checks all lowering entry points; `producer_json_string_array_output` mutates tag type, output slot and input and checks whole/partition certification. Source owner rejects unsupported root types, wrong arity and already-invalid operands. |
| Parse and unpublished prefix | Factor the record-field string-array parser into a shared RAII-list parser. Field wrapper publishes at its existing closing bracket; root publishes only after EOF. `json_string_array_root_grammar_and_cleanup` crosses empty/nonempty success, all escapes/UTF-8, each malformed prefix position, separator/bracket errors, trailing input and invalid tags, with exact Align alloc/free and requested-live-byte evidence. Reuse field failure/success owners to qualify shared helper. |
| Construction/transfer/Drop | `json_string_array_root_lifecycle_and_replay` executes owned input replacement/expiry, arena expiry, returned Result and direct arrays, discard, replacement, generic forwarding and imported producer. Exact positive allocation/free deltas and zero retained requested bytes qualify success/failure. Existing source nulling/deep Drop is reused. The native array producer must use individual allocation provenance and empty contained-input and Move-borrow facts, for both existing numeric arrays and new string arrays; lexical arena presence cannot retag its native heap storage. |
| Control flow | Same source owner exercises if/match/loop joins, early Err, `?`, `else`, `map_err`, and terminating input; no new CFG operation. Negative escaped element views retain the array owner lifetime. |
| Interface/cache/ABI | Whole/per-unit execution plus cold/hit imported frontend replay must agree; malformed checked-HIR/MIR reject before LLVM. Runtime tests invoke A05 directly with exact tags and output sentinel; driver native compilation qualifies LLVM tag/header agreement. No serialized graph or ABI-count change. |
| Compatibility of existing contracts | Run numeric root range owners, existing owned-string-array field prefix/sibling failure and success transition owners, and focused existing owned JSON driver owners. Full local bounded gate and Clippy remain required before push. |

## Author consistency pass

The ledger binds the sole new target to input validation, ownership, exact
element tag and publication order. Every changed public promise is represented
in the matrix. Examples will be the executable owner fixture. No milestone,
future capability or performance promise is consumed. Before implementation,
one independent inspection qualifies this ownership/parse boundary. After the
author matrix-to-diff and self-review passes, one fresh full-diff code review and
local gates run on the same committed candidate. Consumer adoption stays external.

The independent initial boundary inspection found no material issues. It checked
the shared RAII parser, exact tag/header, delayed publication, ordinary deep Drop
and the malformed-producer/lifetime/control matrix before implementation.

## Author matrix-to-diff closure

The extracted exact/every/before/reject obligations map to `check_json_decode`,
`json_array_target_ok` (both HIR envelope and result derivation), the existing
MIR output-slot producer rule, and `gen_json_decode_array`'s exact tag. The
shared `parse_owned_string_array` retains payload ownership until each caller's
publication point; the root's UTF-8/grammar/tag/sentinel/allocation matrix closes
its complete input domain. Existing record-field prefix, sibling-failure and
success owners qualify the shared parser.

Source owners execute named-arena input expiry, owned-input replacement, imported
and generic return, Result controls, replacement/discard and numeric-array arena
independence in whole/per-unit/cache-hit modes. The decode lifecycle has exactly
35 matching Align allocation/free events and zero remaining requested bytes.
Encoder round-trip is checked separately with requested-live accounting because
its transferred grow buffer is not an `align_rt_alloc` event. Negative owners
prove element views cannot escape or survive array replacement. Malformed HIR
and MIR element/input/result/output mutations reject at all applicable entry
points. The exact V005 reproducer rejects at the prior compiler's target-domain
diagnostic, establishing that the owner discriminates the added capability.

The self-review covers type admission, both HIR validators, the complete existing
`JsonDecodeArray` pass inventory, input-borrow versus returned-owner facts,
individual native allocation provenance, shared parser cleanup, unchanged
ABI/layout and cache identity, and the English/Japanese amendments. No new IR
variant or general JSON/root/ownership widening is hidden in this change.
