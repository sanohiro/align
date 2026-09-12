# R77–R83 follow-up implementation handoff

Assessment: 2026-09-12. The provider implementation is merged in #1027. The
follow-up Linux owners pass R82 native descriptor lifecycle and R83 reachable
whole/per-unit/ThinLTO execution; consumer adoption remains external.

## Starting point

[PR #1027](https://github.com/sanohiro/align/pull/1027) contains the six selected
repairs and [plan 56](56-r77-r83-composition-plan.md). The remaining provider
gap was acceptance depth: R82 exercised None only, the faithful R83 Document
had an empty main, and R83 lacked its required ThinLTO execution. The follow-up
extends those owners without changing the compiler or public contract.

The external request register still ends at R83. Its latest additions report
consumer verification against the explicit provider branch; they do not request
another API or change the reviewed ownership strategy. Plan 56 remains the sole
public-contract and closure-matrix authority. This handoff selects remaining
work under that plan, without widening its scope.

R77/R79/R80/R81/R82/R83 have a merged implementation plus provider owners.
R78 remains deferred under plan 23's heap-record grammar reopening prerequisite. The batch instruction
does not authorize enum-field admission. macOS `access_at` retains plan 54's
validated `ENOTSUP` behavior; it is not reopened by this batch.

## Evidence reconciliation

The external register supplies the consumer results. The provider rows below map
the retained and extended owners; they do not establish managed-pin adoption.

| Area | Latest evidence | Remaining acceptance |
| --- | --- | --- |
| R77 projected text, R79 nested signal | Existing composition owner covers the selected repairs. | Retain their existing positive/negative owners; no new defect is reported. |
| R80 scalar copy | Existing loop-copy owner covers the scalar-copy repair. | The separate row below owns non-loop Option replacement. |
| R80 FILE_SET replacement | Provider owner `owned_option_branch_replacement_keeps_nested_members` passes the mutable branch assignment and final Evidence transfer. | Consumer source-trust smoke remains external. |
| R81 borrowed Config | Provider owner `borrowed_config_forwarding_returns_independent_argv` passes direct string/array field forwarding and source reuse. | Consumer sandbox smoke remains external. |
| R81 source collection | Provider owner `borrowed_task_source_expansion_keeps_owned_rows_and_source_reusable` preserves the `collect_task` task -> expansion -> owned-row path. | `scripts/run-prompt-source-collection-smoke` adoption remains external. |
| R82 optional namespace | `m11_process_verified::borrowed_namespace_carriers_preserve_descriptor_authority` executes direct and record-contained Some, explicit command duplicates, invalid/duplicate-slot failures, source expiry and descriptor release on Linux; `borrowed_namespace_payloads_cannot_move_or_escape` rejects both borrowed extraction forms. | Application namespace integration remains external. The macOS portable None path was not executed on the Linux host; native refusal behavior is unchanged. |
| R83 task/evaluation inputs | `faithful_document_digest_and_template_return_path_is_admitted` executes real file reads, digest validation and imported None/Err/Some returns, then inspects the owned result after source expiry in whole/per-unit/ThinLTO modes. | Task-input/evaluation-input adoption remains external. |
| R83 measurement | `owned_measurement_final_digest_field_is_certified_after_canonicalization` executes the 32-field final digest replacement through an imported module in whole/per-unit/ThinLTO modes. | Consumer canonical measurement goldens and application execution remain external. |

The managed-pin prompt-task render/request owner is independent application
progress. It does not close repair, measurement, provider execution, or A2.

## Implementation work packages

All packages belong to the existing capability. The focused provider regressions
were added first; the only compiler change is the demonstrated view-wrapper
stability failure. The owner names below are now implemented tests.

| Package / proposed owner | Reproduction and required controls | First implementation seam if still failing |
| --- | --- | --- |
| `owned_option_branch_replacement_keeps_nested_members` | R80: initialize None, obtain an owned observation in a successful Result arm, clone its identity, assign Some(value), then transfer the Option into an owned Evidence return. Preserve nested owned member rows and no loop. Exercise absent/error/success, replacement of an existing Some, and source expiry; retain a genuinely borrowed-member escape rejection. | `align_sema::completed_value_fact`, `assign_completed_borrow_fact`, generation retirement and projected contents. Do not globally erase retained facts for owned containers. |
| `borrowed_config_forwarding_returns_independent_argv` | R81: borrowed Config containing owned strings and arrays -> validation -> argv builder -> owned return. Direct fields stay reusable after the call. | Shared descriptor formation and producer call-input authority. A readable slice header does not require transfer of its backing owner. |
| `borrowed_task_source_expansion_keeps_owned_rows_and_source_reusable` | R81: keep the `collect_task` boundary, task declarations, independently owned `expanded.files`, shared row reads and explicit cloned pushes into the caller builder. | `align_mir::producer` typed input/read dependencies and independently owned results. Preserve both task and expansion roots. |
| `faithful_document_digest_and_template_return_path_is_admitted` | R83: execute the resource-backed Document, two-phase digest field, optional selectors and seven-field Template return through an imported module. Cover missing selectors, invalid digest, insufficient byte budget, None, success and source expiry in whole/per-unit/ThinLTO modes. | Exact active-path input readiness and owned return-leaf equations. A passing Template field6 case cannot substitute for Document field1. |
| `owned_measurement_final_digest_field_is_certified_after_canonicalization` | R83: execute the 32-field TaskMeasurement shape, canonicalization call, digest replacement and owned return in whole/per-unit/ThinLTO modes. | Producer return-leaf certification plus replacement facts. No schema changes, dummy digest, blanket owned-call seed or ownership bypass. |

The reductions are now Align-owned in `owned_borrowed_composition.rs`; each keeps
the relevant type/call/control path. The
real consumer checks remain in the external register and are not inferred from a
synthetic control or a provider-branch override.

R82 provider coverage extends `m11_process_verified` and retains the runtime namespace owners:
direct and record-contained Some, repeated inheritance into independent commands,
source expiry after explicit duplication, invalid/duplicate slot failure leaving
source and command valid, and prohibited move/escape through the borrowed payload.
Linux verification passes all nine `m11_process_verified` tests. Both R83 native
owners pass whole/per-unit/ThinLTO execution with imported modules. macOS retains
portable None and existing refusal behavior; its path was not executed on this
host. Matching itself must neither duplicate nor close a descriptor.

## Acceptance boundary

Plan 56's matrix names the provider owners. R82 Linux descriptor lifecycle and
R83 native whole/per-unit/ThinLTO execution belong to provider acceptance;
application smoke, managed-pin adoption, canonical measurement goldens and A2
remain external. No provider owner advances a request to VERIFIED/CLOSED.

The implementation continues to use `align_sema::borrow_argument_source` in
sema, checked-HIR replay and MIR. Fixed-array views receive a materialized
descriptor; mutable owning view retypes remain rejected. The execution owners
change no public signature, IR shape, ABI or runtime operation. Read plan 56 and
the live request register before selecting further provider work. This handoff
does not authorize changes to align-llm code, pins, commits or branches.
