# Checked text integer parsing

## Capability boundary

Request 26 needs checked decimal conversion without JSON decoding or a private
parser. Ship one complete source-to-native capability, independent of K1.
The source method, checked HIR, MIR status/output operation, runtime scanner,
and ABI registration form one producer-to-consumer chain. Splitting these into
dormant prerequisites would duplicate boundary proof without a useful consumer.
The exhaustive IR sweeps and owners may take this capability above 1,000 changed
hand-written lines; keeping the scalar operation together lowers integration
risk. This does not widen integer arithmetic or introduce another error model.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Signature | `s.parse_i64() -> Result<i64, Error>` on `str`, with zero arguments. `string` receivers use the existing automatic shared borrow. No import, radix, locale, or default argument exists. |
| Grammar | Entire input matches ASCII `[+-]?[0-9]+`. One optional leading sign, at least one digit, leading zeros and signed zero are accepted. Whitespace, embedded NUL, non-ASCII digits, separators, fractions, exponents, radix prefixes and embedded signs are rejected. `.trim()` remains explicit. |
| Range and errors | Inclusive `-9223372036854775808` through `9223372036854775807`. Every malformed or out-of-range input returns `Err(Error.Invalid)`. Empty and sign-only input reject. Conversion never wraps or aborts for text errors. |
| Evaluation and ownership | Evaluate the receiver exactly once. Borrow its bytes for this operation; preserve a bound owner and release any fresh temporary owner after the native read. The Copy result contains no view, retains no address, and can outlive the input. Ordinary Result construction, assignment, return, `?`, `else`, `match` and `map_err` rules apply. |
| Effects and allocation | Source operation is Pure. It performs no heap allocation, input copy, I/O, or locale lookup. It uses scalar stack storage and one checked byte scan. Parallel read-only use is admitted. No throughput claim or benchmark gate is introduced. |
| Owner and prerequisites | Core text intrinsic; sema owns admission and checked-HIR validation, MIR owns Result construction/cleanup, codegen owns pure physical lowering, runtime owns checked scanning. Existing Result/Error and text borrowing are sufficient; no future milestone or K1 authority is consumed. |
| Artifact identity | Interface generic templates remain source text and are re-parsed by the existing checker; no HIR enum tag is serialized. The complete structural MIR codegen hash includes the new operation, input and output slot and is namespaced by compiler build/schema. Interface format 17 is unchanged. One new RuntimeKey has one exact native symbol and declaration; no new physical ABI shape or public persisted format. |
| Acceptance | `align_driver::str_parse_i64` checks grammar/range, scalar ownership, control paths, generic/imported whole/per-unit parity, Pure closure admission, and diagnostics. Native runtime owners check raw input validation and boundary values. Checked-HIR and malformed-MIR owners reject forged operand/result/output types. Exact ABI goldens and export checks cover the physical boundary. |
| Sources that agree | `draft.md` section 12, `docs/language-spec.md`, `docs/design-notes.md`, Settled `docs/open-questions.md`, core-design `string.md` and `ja/string.md`, checked-HIR ledger 19, runtime ABI ledger 20, and this ledger. Consumer adoption is recorded only in the external request register. |

## Native and IR ledger

| Record | Exact contract |
| --- | --- |
| HIR | `StrParseI64 { input: Expr }`, input exactly `str`, output exactly `Result<i64, Error>`. Child termination propagates normally. Replay, traversal, effect, borrow and move consumers visit the input in source order. No result borrow roots. |
| MIR | `StrParseI64 { input: Operand, out: Slot }` produces exact `i32` status and writes an `i64` output slot. Native success is status 0; failure is 2. Lowering loads the output only on success and uses `make_error_from_status` (2 maps to Invalid). The input is exact `str`; producer validation rejects malformed types and slot identities before LLVM. |
| Key and symbol | `RuntimeKey::StrParseI64`, `align_rt_str_parse_i64`, reused A08: `i32(ptr data, i64 len, ptr out_i64)`. Data length is in bytes; no terminator is read. Writable aligned output and a live readable positive-length data span are compiler-owned preconditions. Output must not overlap the input span. |
| Defensive validation | First reject a null output with status 2 without dereference. With a valid output, initialize it to zero; then reject negative length, lengths unrepresentable by `usize` or above `isize::MAX`, and null data for positive length. Empty input rejects without forming a null raw slice. Grammar/range validation follows. All detected errors return 2 and leave output zero. Only a complete valid parse publishes its scalar. |
| Runtime effects | Conservative `IndirectStorage`, `ArgMem::Unstated`; data parameter 0 has the existing Read attribute. Output parameter 2 is semantically written but has no curated parameter attribute: the current registry permits only Read, and this capability does not widen it. No escape, release, fresh return, callback, or divergence. Function attribute `nounwind` only. Source Pure does not mean a read-only native call with an output pointer. |
| Scanner | Checked multiply/add for positive values and checked multiply/subtract for negative values, so the minimum value is representable without negating it. Every input byte participates in whole-input admission. No unchecked conversion or raw slice exists before extent validation. |

## Implementation closure matrix

| Axis | Implementation / invariant | Discriminating owner |
| --- | --- | --- |
| Formation and malformed input | Zero method arity; exact input/result HIR types; exact MIR input/status/output types and valid slots; no unchecked backend conversion | `str_parse_i64::diagnostics`; `str_parse_i64_hir_rejects_forged_types_in_every_entrypoint` and `str_parse_i64_mir_gate_requires_exact_native_slots` |
| Construction and native ABI | Byte-counted receiver, single native call, exact A08 declaration and effect metadata, success-only scalar load | `str_parse_i64_grammar_range_and_raw_extents`; ABI declaration golden and `test-runtime-abi-exports.sh`; codegen declaration golden and native producer owner |
| Move-in/out and source nulling | Borrow input, never consume a bound string; scalar result has no storage roots; normal by-value scalar Result transfers | `str_parse_i64::receiver_and_control_matrix` |
| Drop and replacement | Existing temporary text-borrow cleanup after native use; result assignment uses Copy storage; receiver owner can be replaced after conversion | Receiver/control matrix and existing `owned_temporaries` borrow-shape owner extended for parsing |
| If/match and branch joins | Selected receiver expressions preserve read lifetime; selected Result arms hold scalar payload only | Receiver/control matrix, both successful and invalid texts |
| Else, try, map_err and early exits | Existing canonical Result tags and Error.Invalid; cleanup on successful continuation and propagated/discarded errors | `str_parse_i64::receiver_and_control_matrix` |
| Loop joins | Value-carrying loop receivers and result break values; terminating child is not lowered into a native call | Receiver/control matrix |
| Generic and interface | New node survives replay and generic template serialization; no nominal type or recursive definition changes | `str_parse_i64::grammar_range_and_imported_generic_parity` whole/per-unit owner |
| Whole-program and per-unit | Same status/output ABI and Result/Error construction, fresh temporary cleanup | Whole/per-unit grammar and imported-generic owners |
| Purity and parallel | Read-only captured/input text accepted in Pure closures; native output is function-local | `str_parse_i64::pure_parallel_reader` |
| Provenance and allocation parity | Input bytes remain caller/temporary owned; output is stack i64; scanner allocates nothing and retains nothing | Runtime raw-span owner; `str_parse_i64_allocates_nothing` for repeated fixed-input parses |
| Outside this capability | No radix/float parsing, general parser interface, borrowed aggregate authority, function-value surface, or consumer-code adoption | Explicitly deferred; no dormant implementation |

## Author consistency pass

The public ledger and IR ledger agree on exact signed widths, one grammar, one
error category, the byte length, and the no-retention lifetime. Native validation
order precedes all input dereferences; zeroing a valid private output is the only
pre-validation write. All malformed cases share Invalid, so no competing public
error precedence exists. There is no ambient configuration, global state,
canonical format, runtime reflection, new aggregate ownership, or milestone
dependency. Existing type/cache structural graph rules apply rather than a new
fingerprint policy. The closure matrix enumerates each changed producer and
consumer; its final author pass will bind each owner to the completed diff.

## Pre-implementation review closure

The independent boundary review identified that the existing ABI effect validator
admits only Read parameter attributes. The output write remains explicit in the
native/MIR ledger, while the runtime row leaves that parameter unannotated. This
uses the existing conservative effect strategy and requires no global validator
widening. The declaration owner pins absence of an output read-only claim.


## Author matrix-to-diff pass

Sema admits only text/zero arity and emits the exact builtin Result identity.
Its exhaustive effect, escape, storage, move, replay and depth consumers classify
one borrowed child and a root-free scalar result. The checked-HIR owner mutates
input, signed width, result shape and error identity. MIR constructs the status
branch, releases temporary borrow owners after scanning, loads the scalar on
success, and builds Invalid through the existing status decoder. The nine joined
receiver and five transparent-scope cells in `owned_temporaries` now also run
this consumer with exactly-once branch/store/frame counts and native execution.
The dedicated driver owner covers receiver retention/replacement, Result joins,
try/else/map_err, return and loop/scope receivers, termination, Pure parallel
readers and imported generic whole/per-unit parity.

The native-owner MIR contract fixes str input, i32 status, one private i64 scratch
slot, and participates in the output/operand inventories; its malformed owner
mutates each type, missing scratch identity, and parameter-slot reuse. Codegen
performs the A08 call only after producer validation. The golden independently
pins the output's absent read-only attribute; the native export checker fixes
all physical signatures and optional export totals. Runtime grammar/boundary
and raw-extent owners exercise success and canonical-zero Invalid failures;
its allocation-count owner records zero global allocation delta. No input view
is retained, no new cleanup authority is introduced, and no matrix cell is
silently delegated to K1 or consumer adoption.
