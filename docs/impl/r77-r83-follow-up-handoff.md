# R77–R83 follow-up implementation handoff

Assessment: 2026-09-12. The follow-up provider implementation and focused
verification are complete on `feat/r77-r83-composition`; consumer adoption remains
external.

## Starting point

`main` was pulled with `--ff-only` and remains at `21b3151e`.
[PR #1027](https://github.com/sanohiro/align/pull/1027) is open at
`651cd0a2581b27a5c893c29428ddd706a0ddf232`; its implementation and
[plan 56](https://github.com/sanohiro/align/blob/651cd0a2581b27a5c893c29428ddd706a0ddf232/docs/impl/56-r77-r83-composition-plan.md)
are not yet in main. Inspect live Git state before resuming. Continue from that
implementation, or from main after it contains the implementation. Do not rebuild
the six repairs from main merely because plan 56 is absent there.

The external request register still ends at R83. Its latest additions report
consumer verification against the explicit provider branch; they do not request
another API or change the reviewed ownership strategy. Plan 56 remains the sole
public-contract and closure-matrix authority. This handoff selects remaining
work under that plan, without widening its scope.

R77/R79/R80/R81/R82/R83 have an implementation candidate plus provider owners.
R78 remains deferred under plan 23's heap-record grammar reopening prerequisite. The batch instruction
does not authorize enum-field admission. macOS `access_at` retains plan 54's
validated `ENOTSUP` behavior; it is not reopened by this batch.

## Evidence reconciliation

The external register supplies the consumer results; the provider rows below were
rerun on this branch. Provider-branch success is distinct from managed-pin adoption.

| Area | Latest evidence | Remaining provider acceptance |
| --- | --- | --- |
| R77 projected text, R79 nested signal | Existing composition owner covers the selected repairs. | Retain their existing positive/negative owners; no new defect is reported. |
| R80 scalar copy | Existing loop-copy owner passes on the provider branch. | Does not establish non-loop owned Option replacement. |
| R80 FILE_SET replacement | Provider owner `owned_option_branch_replacement_keeps_nested_members` passes the mutable branch assignment and final Evidence transfer. | Consumer source-trust smoke remains external. |
| R81 borrowed Config | Provider owner `borrowed_config_forwarding_returns_independent_argv` passes direct string/array field forwarding and source reuse. | Consumer sandbox smoke remains external. |
| R81 source collection | Provider owner `borrowed_task_source_expansion_keeps_owned_rows_and_source_reusable` preserves the `collect_task` task -> expansion -> owned-row path. | `scripts/run-prompt-source-collection-smoke` adoption remains external. |
| R82 optional namespace | Existing composition owner checks the portable None form. | Qualify the shared Some carrier with existing native namespace owners; type checking alone is not Linux execution evidence. |
| R83 task/evaluation inputs | Provider owner `faithful_document_digest_and_template_return_path_is_admitted` passes the resource-backed Document and nested optional return path. | Task-input/evaluation-input adoption remains external. |
| R83 measurement | Provider owner `owned_measurement_final_digest_field_is_certified_after_canonicalization` passes the 32-field final digest replacement in whole/per-unit execution. | Canonical measurement goldens and native execution remain external. |

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
| `faithful_document_digest_and_template_return_path_is_admitted` | R83: retain the resource-backed Document, two-phase digest field, optional selectors and seven-field Template return. | Exact active-path input readiness and owned return-leaf equations. A passing Template field6 case cannot substitute for Document field1. |
| `owned_measurement_final_digest_field_is_certified_after_canonicalization` | R83: retain the 32-field TaskMeasurement shape, canonicalization call, digest replacement and owned return. | Producer return-leaf certification plus replacement facts. No schema changes, dummy digest, blanket owned-call seed or ownership bypass. |

The reductions are now Align-owned in `owned_borrowed_composition.rs`; each keeps
the relevant type/call/control path and passes against the repaired provider. The
real consumer checks remain in the external register and are not inferred from a
synthetic control or a provider-branch override.

R82 provider coverage retains/reuses `m11_process_verified` and the runtime namespace owners:
direct and record-contained Some, repeated inheritance into independent commands,
source expiry after explicit duplication, invalid/duplicate slot failure leaving
source and command valid, and prohibited move/escape through the borrowed payload.
Linux owns real namespace execution; macOS owns portable None and existing refusal
behavior. Matching itself must neither duplicate nor close a descriptor.

## Acceptance and resumption order

1. Read plan 56 and the live register, then use the existing implementation
   branch. Keep this handoff available when switching from main.
2. Close R80 replacement and R81 borrowed Config first: the provider owners now
   exercise the originally rejected forms. The source-collection and R83
   admission/measurement shapes are preserved in reproducible provider owners.
3. Run reachable positives through whole-program and per-unit compilation and
   execution; retain plan 56's R83 ThinLTO obligation. Keep source-escape,
   uninitialized-input and borrowed Store/Load-to-owned-return negatives. Reuse
   `owned_borrowed_composition` and the existing producer entrypoint helpers.
4. Plan 56's matrix now names the actual provider owners and records the wrapper
   normalization seam. Mark consumer adoption and platform-specific execution
   as external rather than treating them as provider failures.
5. The demonstrated defects were repaired under the existing strategy: sema,
   checked-HIR replay and MIR all use `align_sema::borrow_argument_source` to
   normalize `StrBorrow`/`ArrayToSlice` before stable-place validation and lowering;
   fixed-array views receive a materialized descriptor, mutable owning view retypes
   are rejected, root retypes use the canonical producer predicate, and generic
   indexed-field arguments use the field checker. No public signature, IR shape,
   ABI or runtime operation changed.
6. Use the narrow owner first, then the repository gates appropriate to the final
   code diff. Keep the 30-minute test budget; no unrelated DB service gate.
   At implementation-batch completion run `cargo build --release --workspace`
   with the host LLVM/library environment resolved as required.

Merge readiness now has the provider cells in plan 56 and the focused owner
evidence. Consumer managed-pin adoption, application smoke owners and A2 remain
separate consumer work after the combined provider delivery. The final candidate
has a clean host review and still requires the release build, preflight and branch
publication. Do not advance to ALIGN_MERGED before a real merge or to
VERIFIED/CLOSED based on an explicit local provider override. This handoff records
provider work only; it does not change align-llm code, pin, commits or branches.
