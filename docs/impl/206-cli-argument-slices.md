# CLI parsing of borrowed argument slices

## Scope and evidence

Media AM-R10 passes the arguments beginning with `video` to the existing flag
parser. Its retained `cli-argument-slice.align` witness uses `args[1..]`;
`src/video.align::parse` currently needs an explicitly built `array<str>`.
All three consumer registers were reconciled before selection. Voice's
`command.parse` is its own slice parser, and LLM's similarly named parsers are
application parsers; neither requires a consumer edit for this delivery.

This is an additional input carrier for the same synchronous library operation,
not a change to the language's ownership, argv source or flag grammar. The native
parser already consumes a contiguous `AlignStr` span and copies parsed text.
No general subcommand framework, new parser, implicit array materialization or
consumer adoption is selected. AM-R09's task results and AM-R07's external
writable views have separate ownership boundaries.

## Public contract

| Surface | Exact contract |
| --- | --- |
| Signature | `c.parse(args: array<str>) -> Result<parsed, Error>` also accepts `slice<str>` through the same builtin. Both inputs are shared, call-scoped borrows. Fixed arrays use an explicit `[..]` slice; no other element type is admitted. The receiver remains a bound `cli command` local. |
| Input | The selected span is the complete argv for this call. Its element zero is skipped, whether it names a program or a subcommand. Empty and one-element spans parse only defaults. Slice bounds obey ordinary signed bounds checks before parsing; no ambient argv is read. |
| Grammar and errors | Preserve the exact existing registration, float-default validation, token grammar, last-value-wins and occurrence rules in Plan181. Unknown/missing/malformed options return `Error.Invalid`. Existing native allocation-failure behavior is unchanged. No slice-specific input error or fallback parser is added. |
| Encoding | Every element is an existing valid UTF-8 `str`, including empty text. Length-delimited embedded NUL is not a terminator; string values preserve it, numeric tokens reject it under their existing grammar. No native C-string conversion occurs. |
| Ownership and lifetime | Evaluate the receiver once, reserve it through evaluation of the argument once and the synchronous native call, then release the call reservation. The argv headers and every text backing must remain valid through that call. Parse does not consume, mutate or retain either input. Parsed owns independent name/value copies and can outlive the command, array, slice and text sources. `get_str` remains bounded by parsed. |
| Allocation and cleanup | Creating/passing a slice adds no allocation or element copy. Existing parsed-table/string allocation, partial-error cleanup, Move/source-nulling/replacement/return and deep Drop remain unchanged. There is no new cleanup bit or owning type. |
| Compiler/runtime | Sema source admission and checked-HIR schema accept precisely dynamic arrays and slices of Str. Existing MIR `CliParse`, span lowering, LLVM runtime call and runtime parser are reused. The ABI stays `i32(ptr command, ptr argv, i64 count, ptr out)` under `CliParse`; no interface tag, persisted format or cache schema changes. Compiler-source cache identity invalidates old admission results. |
| Prerequisites and bounds | Existing slice, CLI and Plan181 receiver-reservation machinery only. No later milestone, general aggregate provenance or exclusive-view authority is required. No throughput or resource-improvement claim; no benchmark is a correctness gate. |

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Exact formation / malformed input | Source admission in `check_cli_command_method` and `validate_hir`'s native expression schema; `cli_argument_slices` rejects fixed arrays without slicing and non-Str slices/arrays, wrong arity and unbound receivers. The existing parameterized `native_cli_parse` schema owner gains a slice twin and retains operand/result corruption refusals. |
| Values / grammar / extent | `cli_argument_slices` runs the exact media witness, offset/nested/empty/one-element slices, separate/equals/flag-like/empty/NUL values, repeated options and malformed inputs. Sentinel prefix/suffix tokens prove only the selected span is parsed. Existing `m10_cli` and `cli_admission` retain the complete parser and float/count domains. |
| Borrow / replacement / cleanup | `cli_argument_slices` proves parsed text survives argv/text/command scope, parsed-view escape still rejects, invalidated argv backing rejects, and eager argv evaluation cannot replace the reserved command. Existing Plan181 receiver and deep-Drop owners remain authoritative. No new move-in/out, source nulling, Drop or replacement implementation. |
| Control flow | Source owner covers an argv block that returns or propagates an error before the native action, plus if/match-selected slices and loop reuse. Existing CLI Result match/else/?/map_err and ordinary return/join cleanup rules apply unchanged; no new discriminator or join semantics. |
| Imports / generics / replay | `cli_argument_slices` forwards a slice through an imported generic identity into a parser helper; whole-program and per-unit execution plus cold/hit frontend cache use the same witness. Existing interface slice and CLI types are unchanged. |
| Allocation / runtime provenance / ABI | Existing pointer/count representation is reused; source owner exercises dynamic and fixed/arena-backed slices with independent parsed output. No runtime behavior or native ABI mutation. Existing native parser allocation/cleanup and ABI goldens cover the reused operation. |
| Source-of-truth agreement | Update draft.md, language-spec.md, design-notes.md, Settled open-questions.md, English/Japanese std-design/cli.md and the checked-HIR ledger. Record capability status once in HANDOFF.md and provider delivery in media's register after merge. |

The author ledger-to-matrix pass maps all input, error, lifetime, allocation and
exact-type obligations above to the named owners. Existing cumulative owners
cover unchanged parser and opaque-owner behavior; they are not duplicated per
carrier. The implementation changes admission at two compiler boundaries and
reuses representation and cleanup throughout. One independently mergeable PR
delivers the consumer-visible operation and its negative controls.

The fresh independent adversarial review of this input/ownership boundary was
CLEAN before implementation. Run the focused source and malformed-HIR owners,
the one candidate review, final-SHA preflight and required CI before merge.
Consumer code, compiler pins and adoption remain external. At request-batch
completion, run `cargo build --release --workspace` with the local LLVM setup.

## Author closure

`cli_slices_execute_and_replay` covers direct offset argv, imported/generic
forwarding, selected extents, empty/default inputs, if/match/loop composition,
early return and error propagation, whole/per-unit builds and cold/hit replay.
Explicit replacement releases the original argv array, owned NUL-bearing text
and command before the parsed value is observed; an arena-backed argv twin
survives arena exit. `cli_slices_reject_invalid_types_and_lifetimes` crosses
both frontends for the excluded carriers, receiver/arity errors, invalidated
array/text backing and invalidated parsed views. Existing `m10_cli` and
`cli_admission` retain parser grammar, float/count, return-escape and deep-Drop
coverage. Ordinary slicing supplies the unchanged signed bounds semantics.

`hir_body_validator_native` admits both carrier twins and corrupts their
command, argv domain and result independently. Wrong argv local declarations
are changed with their operands so unrelated local-type mismatch cannot mask
the exact carrier gate. The author ledger-to-diff pass found no new IR kind,
ownership producer, native layout, serialized tag or runtime action; only
source admission and checked-HIR carrier validation change behavior. Runtime
and MIR comments now name both carriers. No matrix cell is deferred.
