# Align review finding memory

This is the durable, counted memory behind `align-self-review`. It complements
the historical 388-finding aggregate in `SKILL.md` with auditable recent
events. Counts begin with the preserved review logs for PRs #718, #720, #723,
and #724. Older examples remain useful in the skill, but are not assigned
invented exact counts here.

## Counting rules

- Count only verified actionable findings from an independent design or code
  review. Do not count rejected claims, editorials, test failures, or preflight
  failures.
- Count one event per distinct reviewer finding. Repeated rendering of the same
  finding in one log counts once.
- `Events` counts findings; `PRs` counts distinct affected PRs. Several findings
  in one PR can share one root-cause class.
- Record a merged PR's findings at the start of the next capability, not after
  the reviewed final SHA. This preserves SHA-bound preflight evidence.
- First occurrence: record the event and its discriminating owner. Second:
  promote an explicit question into `SKILL.md`. Third: add a lint, structural
  assertion, or parameterized owner where feasible; otherwise make the closure
  matrix cell mandatory and record why automation is not sound.
- A class found by internal investigation is tracked in its owning ledger rather
  than counted here. The silent-empty-MIR / producer-delegation class
  (`docs/impl/19-hir-validation-ledger.md`) is the current example, at eight
  occurrences with its own source-analysis owners.

## Root-cause counts

| Root-cause key | Events | PRs | Prevention status |
|---|---:|---:|---|
| `validation-phase-completeness` | 17 | 8 | Recurred past the added owner (#727: static-validation-after-native-prepare; unvalidated v3 tail-reserved field; #770: batch layout checked after native advancement; #772: prepared-option, generation-metadata/cardinality, lease/measurement, and static/vector ordering; #773: descriptor-v6 count-thunk preflight; #798: exact response-head syntax before chunked framing selection). Required next: deduplicate copied header validators into one shared function so an ABI version bump has one owner, and extend the parameterized phase-order owner across prepared/transaction paths. |
| `cross-stage-abi-exactness` | 7 | 2 | Covered by Gate 4 and exact ABI owner matrices; #772 additionally requires generated count evidence and canonical cross-driver context bytes. |
| `native-api-version-boundary` | 4 | 1 | Covered by Gate 2 plus version/conversion goldens. |
| `ownership-allocation-owner` | 6 | 4 | Three-plus events: scalar fixed-array admission now routes through the canonical recursive `DropPlan`, with source and handcrafted-HIR owners rejecting every non-struct element that lacks per-element cleanup. Gates 1, 2, and 4 still require every allocation, Drop path, and thunk to have one named owner; #772 pins present-empty binary bytea to a non-null zero-length sentinel owner, and #773 pins normalized-plan allocation/use provenance before native calls. |
| `sql-scanner-lexical-closure` | 3 | 2 | Promoted: parameterized lexical-mode and token-free-tail owners are mandatory. |
| `native-link-order-closure` | 3 | 1 | Promoted: preserve caller order around the dependency closure and test arbitrary prefix/suffix libraries. |
| `producer-evidence-propagation` | 1 | 1 | Watch; exact producer-to-consumer evidence owner required. |
| `public-surface-completeness` | 2 | 2 | Promoted: compare the complete exported surface to the ledger and require an internal/sealed boundary for cross-module helpers. |
| `operation-matrix-completeness` | 7 | 6 | Three-plus events: automation required. Add one parameterized dual-driver parity owner that runs the same descriptor/status/sentinel matrix through both drivers and asserts identical error-class mapping; also check native command tags (PostgreSQL answers `COMMIT` in an aborted transaction with `PGRES_COMMAND_OK` and a `ROLLBACK` tag), retain failed rows state after every cleanup failure, and keep PostgreSQL parameter/result format axes independent. |
| `error-contract-identity` | 3 | 3 | Three-plus events: a lexical lint cannot see whether an error *value* carries its own identity, so the automated owner is per-error-type and the closure-matrix cell is mandatory. Every fallible boundary that can refuse needs a discriminating owner asserting the refusing pass/stage and the offending identity in the produced error (#774's `lower_program_checked_reports_a_vanished_checked_program` names the validation pass and function; `ContractError` pins error class, query identity, and complete multi-invalid precedence). |
| `source-of-truth-drift` | 13 | 10 | Three-plus events: the mandatory ledger-to-diff extraction pass in `SKILL.md` is the owner — extract every `must`/`exact`/`every`/`before`/`reject`/`required` line from the touched design sections and point each at implementation or an explicit deferral. Re-read adjacent comments and counter documentation against behavior, including internal interface/cache invalidation, runtime ABI inventory totals, and coordinated external request-register lifecycle state (#798). A lexical lint is not sound here because the drift is semantic, so the extraction pass is mandatory rather than automated. |
| `owner-test-topology` | 4 | 4 | Three-plus events: every named closure-matrix cell must have a direct owner whose witness is **mutation-verified against the pre-fix compiler**, not merely present; retain whole/per-unit owners when refactoring fixtures. A witness that counts something the defect does not duplicate (a `.clone()` outside the duplicated subtree) passes on the broken compiler and closes nothing. #774 adds the negative direction: a delegated gate also needs an owner proving it still refuses a genuinely different type. |
| `test-global-state-isolation` | 1 | 1 | Watch; prefer child-scoped environment and RAII restoration. |
| `test-entry-abi-exactness` | 1 | 1 | Watch; compile fixtures through the real entry ABI. |
| `mutation-noop-idempotency` | 1 | 1 | Watch; prove unchanged input performs no write. |
| `native-evidence-applicability` | 1 | 1 | Watch; project optional native evidence only for engines/access methods whose semantics are proved. |
| `analysis-control-path-completeness` | 6 | 3 | Three-plus events: automation required and shipped in #734 — call effects route through one shared atomic post-argument transition (`apply_borrow_mut_calls`/`apply_borrow_mut_call_effects`) used by the exhaustive walk, the eager worklist, and the transparent spine, with the parameterized `borrow_mut_shaper_retention` owner module covering each control path. Extended for MIR *lowering*: `borrow_mode_differs` is the single authority for "borrow mode is not `lower_expr`", gating both `lower_expr_for_borrow`'s dispatch (`_ => unreachable!`) and `eager_worklist_children`'s filter, with `borrow_transparent_scope_block` as the one scope-kind enumeration shared with `moved_drop_flag`/`temporary_drop_flag`; the `borrowed_control_flow_temporaries_lower_exactly_once` owner sweeps all 14 cells with mutation-verified structural counts. #786 recurred *past* that automation from a new direction — the authority was correct, but admitting a new receiver type silently added cells the sweep never enumerated — so any change that widens what a borrow-mode gate accepts must re-run the 14-cell sweep over the newly admitted shapes. |
| `analysis-fact-consumer-sweep` | 5 | 2 | Three-plus events: automation required. #734 closed the exact/fallback selection into one shared `borrow_mut_source_indices` used by both liveness and escape consumers. Remaining open cell: EscapeCheck still carries three unshared destination/incoming region computations (`mutable_destination_storage_region`, the `AssignField` arm, the whole-`Assign` arm); consolidation into one shared authority is the next capability, with four verified fail-open probes recorded in its plan. #774 extends the rule to delegated gates: fix the reported gate by sweeping every sibling that re-derives the same producer fact. |
| `memoized-input-completeness` | 1 | 1 | Watch; a memoized step must key every semantic input it reads, including caller-seeded diagnostics and process toggles. |
| `resource-bound-completeness` | 7 | 3 | Three-plus events: long-lived process caches require a byte-accounted retention bound and reachable refusal owner; native protocols require parameter-count, aggregate-message, per-encoding accepted-limit/rejected-next, and fixed-field accounting owners. HTTP streaming decoders additionally require a cumulative metadata-work bound plus exact-limit/rejected-next owners, and allocation ceilings must count stored capacity and simultaneously live old/new growth layouts (#798). |
| `canonical-key-encoding` | 1 | 1 | Watch; variable key fields must use one length-delimited canonical encoder rather than ad hoc separators. |
| `shared-artifact-concurrency` | 1 | 1 | Watch; retention derived from emitted files must fail closed when emissions overlap on one path. |
| `performance-evidence-completeness` | 1 | 1 | Watch; measure the unaffected one-shot path before enabling a process-wide performance optimization by default. |
| `infallible-entrypoint-contract` | 1 | 1 | Watch; an inspection entry point documented as infallible must stay fail-closed on input no producer emits (unchecked or hand-overridden HIR). Only the explicitly fallible boundary may reject, and every production path must use that boundary. |
| `historical-workflow-source-boundary` | 1 | 1 | Watch; a workflow that checks out an older tag must not assume newly added helper scripts exist in that historical source tree. |

## Event log

| PR | Reviewed candidate | Severity | Root-cause key | Finding |
|---:|---|---|---|---|
| #718 | `bc9b245c` | P1 | `producer-evidence-propagation` | Thread checked snapshots into driver metadata. |
| #718 | `bc9b245c` | P2 | `public-surface-completeness` | Expose explicit-path file constructors. |
| #718 | `bc9b245c` | P2 | `sql-scanner-lexical-closure` | Keep PostgreSQL escape strings opaque to placeholder scanning. |
| #718 | `bc9b245c` | P2 | `sql-scanner-lexical-closure` | Classify the statement after a CTE. |
| #720 | `37e7fd88` | P1 | `cross-stage-abi-exactness` | Pin every execution-header field offset. |
| #720 | `37e7fd88` | P1 | `cross-stage-abi-exactness` | Define the binder context ABI before indirect calls. |
| #720 | `37e7fd88` | P1 | `validation-phase-completeness` | Validate result-column identities before ordinal decoding. |
| #720 | `37e7fd88` | P1 | `operation-matrix-completeness` | Cover PostgreSQL command-result semantics. |
| #720 | `37e7fd88` | P1 | `error-contract-identity` | Keep query IDs on `ContractError` only. |
| #720 | `37e7fd88` | P2 | `ownership-allocation-owner` | Permit allocations for owned contract errors. |
| #720 | `37e7fd88` | P2 | `native-api-version-boundary` | Define native timeout-conversion boundaries. |
| #720 | `5806b6e3` | P1 | `cross-stage-abi-exactness` | Define the row-validation callback ABI. |
| #720 | `5806b6e3` | P1 | `cross-stage-abi-exactness` | Define the mandatory QueryMeta thunk signature. |
| #720 | `5806b6e3` | P2 | `validation-phase-completeness` | Specify deterministic multi-invalid precedence. |
| #720 | `5806b6e3` | P2 | `native-api-version-boundary` | Pin PostgreSQL client encoding to UTF-8. |
| #720 | `92be487e` | P1 | `validation-phase-completeness` | Validate the header before constructing option errors. |
| #720 | `92be487e` | P2 | `ownership-allocation-owner` | Assign a runtime owner for static descriptor options. |
| #720 | `92be487e` | P2 | `native-api-version-boundary` | Honor libpq's two-second timeout floor. |
| #720 | `92be487e` | P2 | `native-api-version-boundary` | Define the post-connect encoding error. |
| #720 | `92be487e` | P2 | `ownership-allocation-owner` | Propagate QueryMeta thunk ownership across plans. |
| #720 | `93fab8a3` | P2 | `validation-phase-completeness` | Define cardinality-versus-decode precedence. |
| #720 | `93fab8a3` | P2 | `validation-phase-completeness` | Check connection status before encoding. |
| #720 | `93fab8a3` | P2 | `source-of-truth-drift` | Remove stale Q1 materializer-thunk obligations. |
| #720 | `45c87df8` | P2 | `native-link-order-closure` | Preserve `-lcrypto` after dependent static libraries. |
| #720 | `45c87df8` | P2 | `owner-test-topology` | Retain the whole-program execution owner. |
| #720 | `78427656` | P2 | `native-link-order-closure` | Preserve libraries following the libpq closure. |
| #720 | `e524e96a` | P2 | `native-link-order-closure` | Preserve suffix libraries before closure prerequisites. |
| #720 | `40fdeec9` | P1 | `test-global-state-isolation` | Do not mutate process-global environment from parallel tests. |
| #720 | `56272b85` | P2 | `test-entry-abi-exactness` | Return `Result` from the argv-based test entry. |
| #723 | `8a66fbf2` | P1 | `sql-scanner-lexical-closure` | Ignore token-free tails after the final SQLite statement. |
| #723 | `8a66fbf2` | P1 | `mutation-noop-idempotency` | Avoid rewriting unchanged SQLite history. |
| #724 | `2f18d6ca` | P2 | `validation-phase-completeness` | Reject poisoned execution targets before QueryMeta materialization. |
| #726 | `7a4a8094` | P2 | `native-evidence-applicability` | Leave PostgreSQL sort/null fields absent for non-B-tree index methods. |
| #726 | `7a4a8094` | P2 | `public-surface-completeness` | Seal cross-module catalog adapters behind the package's internal boundary. |
| #726 | `7a4a8094` | P2 | `validation-phase-completeness` | Check native metadata driver restriction before complete live-state validity. |
| #727 | `3c4de7a8` | P1 | `operation-matrix-completeness` | Reject PostgreSQL's implicit rollback on commit (check the command tag, not only the status). |
| #727 | `3c4de7a8` | P2 | `validation-phase-completeness` | Run SQLite static validation before lease acquisition and native prepare. |
| #727 | `3c4de7a8` | P2 | `operation-matrix-completeness` | Map SQLite's unsupported-shape sentinel like the PostgreSQL path already does. |
| #727 | `3c4de7a8` | P2 | `validation-phase-completeness` | Validate the v3 tail-reserved field in every duplicated header validator. |
| #734 | `709feeba` | P1 | `source-of-truth-drift` | Reconcile exact direct retention with L6's all-argument conservative rule before implementation. |
| #734 | `709feeba` | P2 | `operation-matrix-completeness` | Cover both partial-NULL child directions and their precedence in the segmented contract. |
| #734 | `709feeba` | P2 | `source-of-truth-drift` | Scope one-execution counting to post-stream-formation, separating pre-send and send-failure counts. |
| #734 | `6912e79b` | P1 | `analysis-control-path-completeness` | Collect mutable destinations on transparent `?` and return edges after the child call effect. |
| #734 | `6912e79b` | P1 | `analysis-control-path-completeness` | Apply call effects only after all eager arguments complete, snapshotting facts at that point. |
| #734 | `6912e79b` | P1 | `analysis-control-path-completeness` | Snapshot all source and destination regions before any multi-destination update. |
| #734 | `6912e79b` | P2 | `analysis-control-path-completeness` | Route unary transparent-spine calls through the same atomic direct-call transition. |
| #734 | `1c5167ae` | P1 | `analysis-fact-consumer-sweep` | Distinguish owned-storage roots from contained roots in the retention summary. |
| #734 | `1c5167ae` | P1 | `analysis-fact-consumer-sweep` | Keep mutable `resource_ref` peers rooted in their common owner for alias checks. |
| #734 | `c84bb3d4` | P1 | `analysis-fact-consumer-sweep` | Preserve the typed storage edge through escape-flow region selection. |
| #734 | `c84bb3d4` | P1 | `analysis-fact-consumer-sweep` | Expand unavailable-call fallbacks to both contained and storage edges in one shared operation. |
| #739 | `334775fe` | P1 | `ownership-allocation-owner` | Reject owned strings until fixed-array element Drop exists. |
| #757 | `89aed25b` | P2 | `memoized-input-completeness` | Include the caller-seeded diagnostic sink because sema reads it when deciding descriptor discovery. |
| #757 | `89aed25b` | P2 | `resource-bound-completeness` | Replace the entry-count cap with byte-accounted memo retention and a reachable refusal path. |
| #757 | `89aed25b` | P3 | `canonical-key-encoding` | Encode environment-toggle key fields through the canonical length-delimited field encoder. |
| #757 | `89aed25b` | P3 | `shared-artifact-concurrency` | Skip object retention when two emissions overlap on the same output path. |
| #757 | `89aed25b` | P3 | `owner-test-topology` | Add direct discriminating owners for the three previously paper-only closure cells. |
| #757 | `89aed25b` | P3 | `source-of-truth-drift` | Align memo hit/miss counter documentation with the implemented accounting semantics. |
| #757 | `89aed25b` | P3 | `performance-evidence-completeness` | Measure cold one-shot CLI overhead before keeping the memo enabled by default. |
| #768 | `3c892f01` | P2 | `analysis-control-path-completeness` | Collapse the hand-written borrow-mode kind list into one authority gating both the dispatch and the eager-worklist filter. |
| #768 | `3c892f01` | P2 | `owner-test-topology` | Cover the five borrow-transparent scope kinds and give the bound cells a witness that discriminates against the pre-fix compiler. |
| #757 | `89aed25b` | P3 | `source-of-truth-drift` | Scope the test environment-lock safety comment to the isolation it actually proves. |
| #770 | `4667cc8f` | P2 | `operation-matrix-completeness` | Retain failed rows state when clean-terminal synchronization or blocking restoration fails. |
| #770 | `4667cc8f` | P2 | `validation-phase-completeness` | Validate batch fixed-layout representability before advancing the native stream. |
| #770 | `4667cc8f` | P2 | `historical-workflow-source-boundary` | Keep libpq setup/version fallback available when rebuilding a tag that predates the new helper scripts. |
| #771 | `1187713` | P2 | `source-of-truth-drift` | Update the completed PostgreSQL delivery boundaries in the live handoff table. |
| #772 | `1f89ddd` | P1 | `ownership-allocation-owner` | Give present-empty Binary bytea a non-null zero-length transport sentinel. |
| #772 | `1f89ddd` | P1 | `cross-stage-abi-exactness` | Specify the complete normalized format-plan ABI and ownership boundary. |
| #772 | `1f89ddd` | P2 | `error-contract-identity` | Define exact binary failure records and multi-invalid precedence. |
| #772 | `88971d8` | P1 | `cross-stage-abi-exactness` | Retain producer-owned parameter-count evidence in prepared statement v4. |
| #772 | `88971d8` | P1 | `validation-phase-completeness` | Put complete prepared-header validation before option lookup that depends on it. |
| #772 | `9bd0f1e` | P1 | `validation-phase-completeness` | Validate result-generation metadata before zero-row and cardinality handling. |
| #772 | `9bd0f1e` | P1 | `operation-matrix-completeness` | Cross PostgreSQL parameter plans independently with result format. |
| #772 | `2922c73` | P1 | `cross-stage-abi-exactness` | Define canonical SQLite zero bytes in the shared context ABI. |
| #772 | `2922c73` | P1 | `source-of-truth-drift` | Include compiler-private interface hashes and importer dependency keys in cache invalidation. |
| #772 | `2922c73` | P2 | `validation-phase-completeness` | Make metadata validation precede second-row cardinality. |
| #772 | `2922c73` | P2 | `resource-bound-completeness` | Pin each selected encoding's accepted maximum and rejected next length. |
| #772 | `3b380d2` | P2 | `resource-bound-completeness` | Bound the complete PostgreSQL Bind message, not only individual values. |
| #772 | `3b380d2` | P2 | `resource-bound-completeness` | Cap PostgreSQL parameter counts at the protocol Int16 limit. |
| #772 | `7191131` | P2 | `validation-phase-completeness` | Keep direct parameter measurement behind the execution lease. |
| #772 | `7191131` | P2 | `validation-phase-completeness` | Choose one static-validation and full-vector installation order. |
| #772 | `8f282e9` | P2 | `resource-bound-completeness` | Account for libpq's fixed one-entry result-format field in every Bind budget. |
| #773 | `857b9489` | P1 | `validation-phase-completeness` | Validate descriptor-v6 count-thunk shape, value agreement, and the protocol parameter limit before LLVM construction. |
| #773 | `857b9489` | P1 | `ownership-allocation-owner` | Reject normalized PostgreSQL format plans whose HIR allocation and use provenance is not the exact compiler-owned shape. |
| #773 | `857b9489` | P2 | `source-of-truth-drift` | Update the runtime ABI ledgers from the stale 5/288 inventory to the shipped 13/296 base entries. |
| #776 | `3da5488c` | P1 | `operation-matrix-completeness` | Prove driver-native transaction idleness before returning every wrapper-idle pooled connection. |
| #776 | `3da5488c` | P2 | `source-of-truth-drift` | Reconcile pooled transaction Drop with the earlier direct-connection close-only rule. |
| #776 | `3da5488c` | P2 | `source-of-truth-drift` | Record the exact fixed-capacity non-waiting pool contract in Settled. |
| #774 | `a23f5a98` | P1 | `infallible-entrypoint-contract` | Keep the infallible `lower_to_mir*` inspection surface fail-closed; only the fallible boundary may reject. |
| #774 | `a23f5a98` | P2 | `source-of-truth-drift` | `orderable_body_ty` was narrower than sema's `Bound::Ord`; both now call `align_sema::ord_body_ty`. |
| #774 | `a23f5a98` | P2 | `analysis-fact-consumer-sweep` | Sweep the other seven delegated gates rather than fixing only the reported one. |
| #774 | `a23f5a98` | P2 | `error-contract-identity` | `LoweringRejected` must name the refusing validation pass and function. |
| #774 | `a23f5a98` | P2 | `owner-test-topology` | Add negative owners proving a delegated gate still refuses a genuinely different type. |
| #786 | `efd6edaa` | P1 | `analysis-control-path-completeness` | Newly admitted unbound and value-carrying `String` clone receivers must use borrow-owned lowering and the eager-child filter, not ordinary lowering. |
| #786 | `efd6edaa` | P2 | `source-of-truth-drift` | Synchronize the exact `StrClone` expression row with the new producer-delegation row. |
| #798 | `abc363c` | P1 | `validation-phase-completeness` | Validate exact response-head wire syntax before admitting chunked framing or body suppression. |
| #798 | `abc363c` | P1 | `resource-bound-completeness` | Add a cumulative chunk-framing byte bound with exact-limit and rejected-next owners. |
| #798 | `abc363c` | P2 | `resource-bound-completeness` | Define an implementable capacity ceiling that counts transient old/new accumulator allocations. |
| #798 | `4349bcb` | P2 | `source-of-truth-drift` | Synchronize the accepted Request 4 design with the mandatory align-llm lifecycle register and summary. |

The rows above are the reviews whose logs are reachable from this checkout's
`.git/`. Capabilities produced in agent worktrees (#777 and later, other than
#786) keep their logs with those worktrees; their finding sets are unrecorded
here and belong to their owning sessions to reconcile.

## Rejected claims

Do not increase counts for rejected review claims. Add a short row only when a
claim is likely to recur and the durable explanation will save future review
time. The historical PR #232 `wyhash` final-mix claim remains the canonical
example: it was checked against the reference and rejected.
