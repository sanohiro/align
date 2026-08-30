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
| `validation-phase-completeness` | 25 | 13 | Recurred past the added owner (#727: static-validation-after-native-prepare; unvalidated v3 tail-reserved field; #770: batch layout checked after native advancement; #772: prepared-option, generation-metadata/cardinality, lease/measurement, and static/vector ordering; #773: descriptor-v6 count-thunk preflight; #798: exact response-head syntax before chunked framing selection; #800: recognizable malformed response framing before the next blocking transport read; #866: root grammar before relative-view validation/allocation; #883: total speculative setup precedence while valid-magic PGO rejection remains on cache-miss libLLVM codegen; #893: loaded-LLVM identity mismatch precedes target mismatch; #895: observe lexical static paths before resolution, keep semantic errors separate from filesystem state, and nonblocking-open then descriptor-validate every supported file input). Required next: deduplicate copied header validators into one shared function so an ABI version bump has one owner, extend parameterized phase-order owners across every speculative/side-effecting boundary, and retain a multi-invalid owner for each ordered product. |
| `cross-stage-abi-exactness` | 7 | 2 | Covered by Gate 4 and exact ABI owner matrices; #772 additionally requires generated count evidence and canonical cross-driver context bytes. |
| `native-api-version-boundary` | 4 | 1 | Covered by Gate 2 plus version/conversion goldens. |
| `ownership-allocation-owner` | 6 | 4 | Three-plus events: scalar fixed-array admission now routes through the canonical recursive `DropPlan`, with source and handcrafted-HIR owners rejecting every non-struct element that lacks per-element cleanup. Gates 1, 2, and 4 still require every allocation, Drop path, and thunk to have one named owner; #772 pins present-empty binary bytea to a non-null zero-length sentinel owner, and #773 pins normalized-plan allocation/use provenance before native calls. |
| `sql-scanner-lexical-closure` | 3 | 2 | Promoted: parameterized lexical-mode and token-free-tail owners are mandatory. |
| `native-link-order-closure` | 3 | 1 | Promoted: preserve caller order around the dependency closure and test arbitrary prefix/suffix libraries. |
| `producer-evidence-propagation` | 6 | 4 | Three-plus events: automation required. Identities must come from producer-owned evidence for the resource actually in use; a loader-owned mapped image, not its replaceable pathname, owns dynamic-library build identity. #895 adds parameterized observation owners for opened-inode rearm, graph-root identity, and one stable checked-metadata observation. #898 requires a graph-wide cached analysis artifact to reach descriptor encoding rather than being discarded before the terminal consumer recomputes it. Require an exact producer-to-consumer evidence owner for every new resource identity or cached analysis result. |
| `public-surface-completeness` | 2 | 2 | Promoted: compare the complete exported surface to the ledger and require an internal/sealed boundary for cross-module helpers. |
| `operation-matrix-completeness` | 16 | 11 | Three-plus events: automation required. Add one parameterized dual-driver parity owner that runs the same descriptor/status/sentinel matrix through both drivers and asserts identical error-class mapping; also check native command tags (PostgreSQL answers `COMMIT` in an aborted transaction with `PGRES_COMMAND_OK` and a `ROLLBACK` tag), retain failed rows state after every cleanup failure, and keep PostgreSQL parameter/result format axes independent. PR #864 extends the rule beyond drivers: a new call-place form must enumerate direct, imported, local/function-field, and joined indirect targets through one parameterized target-shape owner. PR #866 adds the filesystem direction: every public operation pair needs an explicit same-resource cell, with barrier-controlled absent/newly-installed/pre-existing-regular/pre-existing-special witnesses for same-final open/create. PR #881 adds workflow event provenance: a two-parent commit is not proof of a protected PR merge; exact commit-to-merged-PR evidence owns that cell. PR #895 adds long-running mode parity: EOF transitions, retry diagnostic policy, cache-stat labels, PGO notices, and valueless CLI spellings require direct one-shot/watch or state-transition owners. PR #906 requires every dependent stream method to cross the shared-head versus owned-or-mutable-cursor receiver axis through one parameterized sema and checked-HIR owner. |
| `error-contract-identity` | 8 | 5 | Three-plus events: a lexical lint cannot see whether an error *value* carries its own identity, so the automated owner is per-error-type and the closure-matrix cell is mandatory. Every fallible boundary that can refuse needs a discriminating owner asserting the refusing pass/stage and the offending identity in the produced error (#774's `lower_program_checked_reports_a_vanished_checked_program` names the validation pass and function; `ContractError` pins error class, query identity, and complete multi-invalid precedence; #893 keeps unavailable immutable packaged blobs as clean misses; #895 round-trips arbitrary-byte watch paths exactly once and pins the full refusing output identity). |
| `source-of-truth-drift` | 16 | 13 | Three-plus events: the mandatory ledger-to-diff extraction pass in `SKILL.md` is the owner — extract every `must`/`exact`/`every`/`before`/`reject`/`required` line from the touched design sections and point each at implementation or an explicit deferral. Re-read adjacent comments and counter documentation against behavior, including internal interface/cache invalidation, runtime ABI inventory totals, coordinated external request-register lifecycle state (#798), and release-package native dependency closure (#893). PR #864 adds backend ownership: bounds failure is MIR semantics and LLVM may only lower the already-guarded pointer. #883 requires one-attempt library results to agree with the CLI-owned bounded retry. A lexical lint is not sound here because the drift is semantic, so the extraction pass is mandatory rather than automated. |
| `owner-test-topology` | 11 | 10 | Three-plus events: every named closure-matrix cell must have a direct owner whose witness is **mutation-verified against the pre-fix compiler**, not merely present; retain whole/per-unit owners when refactoring fixtures. A witness that counts something the defect does not duplicate (a `.clone()` outside the duplicated subtree) passes on the broken compiler and closes nothing. #774 adds the negative direction: a delegated gate also needs an owner proving it still refuses a genuinely different type; #802 requires the exact external consumer field graph rather than a representative lookalike. #864 requires a parameterized sweep of every admitted region-bearing Copy leaf and field-complete mutation owners for every checked ownership record. #876 requires exact returned-cleanup-SSA-to-drop-flag-store identity rather than counting arbitrary boolean SSA stores. #881 requires shared `tests/common*` infrastructure to remain in database-owner scope beside nested shared test modules. #882 requires every transitive shared binary-runner dependency to remain in that same scope. #883 requires field-complete parity owners for both legacy walk projections when their shared producer is refactored. |
| `test-global-state-isolation` | 1 | 1 | Watch; prefer child-scoped environment and RAII restoration. |
| `test-entry-abi-exactness` | 1 | 1 | Watch; compile fixtures through the real entry ABI. |
| `test-execution-boundedness` | 1 | 1 | Watch; every generated child-process owner needs a local deadline plus deterministic kill/reap coverage. |
| `mutation-noop-idempotency` | 1 | 1 | Watch; prove unchanged input performs no write. |
| `native-evidence-applicability` | 1 | 1 | Watch; project optional native evidence only for engines/access methods whose semantics are proved. |
| `analysis-control-path-completeness` | 9 | 4 | Three-plus events: automation required and shipped in #734 — call effects route through one shared atomic post-argument transition (`apply_borrow_mut_calls`/`apply_borrow_mut_call_effects`) used by the exhaustive walk, the eager worklist, and the transparent spine, with the parameterized `borrow_mut_shaper_retention` owner module covering each control path. Extended for MIR *lowering*: `borrow_mode_differs` is the single authority for "borrow mode is not `lower_expr`", gating both `lower_expr_for_borrow`'s dispatch (`_ => unreachable!`) and `eager_worklist_children`'s filter, with `borrow_transparent_scope_block` as the one scope-kind enumeration shared with `moved_drop_flag`/`temporary_drop_flag`; the `borrowed_control_flow_temporaries_lower_exactly_once` owner sweeps all 14 cells with mutation-verified structural counts. #786 recurred *past* that automation from a new direction — the authority was correct, but admitting a new receiver type silently added cells the sweep never enumerated — so any change that widens what a borrow-mode gate accepts must re-run the 14-cell sweep over the newly admitted shapes. #864 adds call-only indexed borrows: reserve before the index, retain through every later eager argument, form no guard/descriptor/action after a terminating child, and parameterize every argument position. |
| `analysis-fact-consumer-sweep` | 12 | 5 | Three-plus events: automation required. #734 closed the exact/fallback selection into one shared `borrow_mut_source_indices` used by both liveness and escape consumers. Remaining open cell: EscapeCheck still carries three unshared destination/incoming region computations (`mutable_destination_storage_region`, the `AssignField` arm, the whole-`Assign` arm); consolidation into one shared authority is the next capability, with four verified fail-open probes recorded in its plan. #774 extends the rule to delegated gates: fix the reported gate by sweeping every sibling that re-derives the same producer fact. #802 adds the type-formation direction: newly admitting a field shape requires an automated sweep over existing tagged payload construction, transfer, and Drop consumers. #864 adds indexed return/`borrow mut` retention and ordinary Copy `Index`/`ElemField` provenance; both must consume complete storage generations and contained roots through one shared classifier-backed mapping. #897 adds the release-history direction: retain every historical owner root across transfers, qualify roots by storage generation and exact projection, rename those identities atomically through every analysis state and snapshot, and keep partial-owner invalidation projection-specific. Parameterized owners cover terminal consume, replacement, and Drop after transfer, fresh rebound, and unaffected projected siblings. |
| `memoized-input-completeness` | 2 | 2 | Promoted: a memoized step must key every semantic input it reads, including caller-seeded diagnostics and process toggles. If checked rehydration disproves a key, reject it before invalidation and make lookup/publication recheck authorization after validation so it cannot be reused or republished (#893). |
| `resource-bound-completeness` | 13 | 6 | Three-plus events: long-lived process caches require a byte-accounted retention bound and reachable refusal owner; native protocols require parameter-count, aggregate-message, per-encoding accepted-limit/rejected-next, and fixed-field accounting owners. HTTP streaming decoders additionally require a cumulative metadata-work bound plus exact-limit/rejected-next owners, and allocation ceilings must count stored capacity and simultaneously live old/new growth layouts (#798). #884 adds compiler resources: worker creation is bounded by outstanding work and malformed file headers are rejected before allocating or reading an untrusted tail. #893 extends the rule to loader metadata and immutable packaged-cache action/CAS inputs: parse and hash bounded ranges without whole-file buffering. #895 requires keyed unions at both watch-baseline layers, with exact 32,768-by-32,768 disjoint owners preventing quadratic fallback. |
| `fallible-resource-lifecycle` | 3 | 1 | Three events in #895: automation shipped. Arm pathname cleanup immediately after creation, centralize retry-on-`EINTR` for interruptible reads/status operations, and give unlink cleanup its own retry owner. Every acquired descriptor, child, or pathname must have a direct failure-after-acquisition owner. |
| `canonical-key-encoding` | 1 | 1 | Watch; variable key fields must use one length-delimited canonical encoder rather than ad hoc separators. |
| `shared-artifact-concurrency` | 2 | 2 | Promoted: retention derived from emitted files must fail closed when emissions overlap on one path, and cache-key rejection must linearize with readers and writers through final post-validation/post-write authorization checks. Barrier-controlled owners must cover both interleavings (#893). |
| `rejected-artifact-self-healing` | 1 | 1 | Watch; a rejected writable cache blob must be removed before a later publication can treat it as reusable, including the oversized-file path. Immutable packaged artifacts remain clean misses instead. |
| `artifact-stage-ownership` | 3 | 1 | Three events in #883: automation required. A producer must claim a unique non-shareable `ArtifactStage`; failure states expose no artifact paths; only a complete result owns and lends immutable paths through their last consumer. `PL22` requires concurrent-stage and success/failure availability owners. |
| `worker-lifecycle-cleanup` | 2 | 1 | Promoted: every claimed task needs an unwind-safe completion guard that cancels and notifies before thread exit, the coordinator joins all workers before return or resumed unwind, and code must not temporarily replace the process-global panic hook. |
| `diagnostic-source-map-identity` | 1 | 1 | Watch; return or render diagnostics only while the exact `SourceMap` that allocated their file ids remains alive; a retry owns a distinct map. |
| `performance-evidence-completeness` | 2 | 2 | Promoted: measure the unaffected one-shot path before enabling a process-wide performance optimization by default, and derive changed/unaffected work counts from producer-owned outcomes rather than echoing an expected unit count. |
| `infallible-entrypoint-contract` | 1 | 1 | Watch; an inspection entry point documented as infallible must stay fail-closed on input no producer emits (unchecked or hand-overridden HIR). Only the explicitly fallible boundary may reject, and every production path must use that boundary. |
| `historical-workflow-source-boundary` | 1 | 1 | Watch; a workflow that checks out an older tag must not assume newly added helper scripts exist in that historical source tree. |
| `workflow-trust-provenance` | 1 | 1 | Watch; PR-controlled workflow code must execute classifiers from the trusted base, with an explicit bootstrap arm when the trusted base predates the classifier. |
| `review-checkpoint-completeness` | 1 | 1 | Watch; only a wrapper-owned cycle verdict recorded after successful reviewer exit and result validation may authorize changed-slice continuation. |
| `hot-path-scan-regression` | 1 | 1 | Watch; incremental boundary discovery must retain the established bulk-search primitive rather than regress to a byte-at-a-time scan. |
| `native-thread-state-scope` | 1 | 1 | Watch; a constructor-scoped native thread-state guard does not cover delayed stream I/O or Drop. Establish the guard at every operation that can invoke the guarded native API, and pin read plus teardown with thread-local counter owners. |

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
| #800 | `c725ea4` | P2 | `validation-phase-completeness` | Reject recognizable malformed response heads and chunk prefixes before issuing another blocking transport read. |
| #800 | `c725ea4` | P2 | `hot-path-scan-regression` | Keep response-head delimiter discovery on the existing `memchr` bulk-search path. |
| #802 | `b07d199` | P2 | `owner-test-topology` | Pin the exact C6 consumer field graph instead of permitting representative record fixtures. |
| #802 | `b07d199` | P2 | `analysis-fact-consumer-sweep` | Cover existing Option, Result, and user-sum ownership paths made reachable by direct `array<string>` record fields. |
| #864 | `05426c6c` | P1 | `analysis-fact-consumer-sweep` | Apply exact/fallback mutable-destination retention to views derived from indexed borrowed elements. |
| #864 | `05426c6c` | P1 | `analysis-control-path-completeness` | Reserve the array root before evaluating an index that can invalidate it. |
| #864 | `05426c6c` | P2 | `operation-matrix-completeness` | Cover imported and function-value indexed-borrow calls beside direct calls. |
| #864 | `05426c6c` | P2 | `source-of-truth-drift` | Keep bounds-failure semantics in MIR and make LLVM pure guarded-pointer lowering. |
| #864 | `2cd4cf94` | P1 | `analysis-control-path-completeness` | Preserve the indexed root through every later eager argument to the call action. |
| #864 | `05177850` | P1 | `analysis-control-path-completeness` | A terminating index or later argument must form no guard, descriptor, pointer, or call action. |
| #864 | `05177850` | P1 | `analysis-fact-consumer-sweep` | Preserve complete region owners for ordinary Copy `Index`/`ElemField` through array-bearing projections. |
| #864 | `98f24dc1` | P1 | `owner-test-topology` | Sweep slice-bearing and every other admitted region-bearing Copy-record projection, not only `str`. |
| #864 | `98f24dc1` | P2 | `owner-test-topology` | Mutate every `BorrowedElementBase.owner_fact` vector and record field, including stale flow facts. |
| #866 | `7f1a33dc` | P2 | `validation-phase-completeness` | Complete root grammar validation before inspecting or allocating the relative view. |
| #866 | `7f1a33dc` | P2 | `operation-matrix-completeness` | Define and own every same-final open/create timing and pre-existing-entry outcome. |
| #870 | `649e63f0` | P1 | `test-execution-boundedness` | Removing the Result fuzz quarantine left generated child execution unbounded; add a local deadline and direct kill/reap owner. |
| #876 | `9efdd7bd` | P2 | `owner-test-topology` | Match each returned cleanup SSA id to the exact drop-flag store instead of accepting any boolean SSA value. |
| #881 | `4b47ba7f` | P1 | `workflow-trust-provenance` | Run the PostgreSQL scope classifier from the trusted base rather than the PR-controlled workflow copy. |
| #881 | `4b47ba7f` | P2 | `owner-test-topology` | Keep shared `tests/common*` infrastructure inside database-owner scope. |
| #881 | `4b47ba7f` | P2 | `operation-matrix-completeness` | Verify an exact merged PR before treating a two-parent main commit as a protected merge. |
| #882 | `0e5f2ea5` | P1 | `owner-test-topology` | Classify every transitive shared database binary-runner dependency. |
| #883 | `b1340918` | P2 | `artifact-stage-ownership` | Keep the object stage alive through linking and make its owner explicit. |
| #883 | `b1340918` | P2 | `worker-lifecycle-cleanup` | Define cancellation, notification, and joining when a codegen worker panics. |
| #883 | `b1340918` | P2 | `owner-test-topology` | Add field-complete parity owners for the refactored package and per-unit walk projections. |
| #883 | `a64baf8d` | P1 | `artifact-stage-ownership` | Prevent concurrent callers from sharing deterministic object paths and poisoning cache publication. |
| #883 | `a64baf8d` | P2 | `artifact-stage-ownership` | Expose object paths only from a complete result whose owning stage keeps them valid. |
| #883 | `06c57578` | P2 | `diagnostic-source-map-identity` | Render each retry attempt's diagnostics against the `SourceMap` that allocated their file ids. |
| #883 | `06c57578` | P2 | `source-of-truth-drift` | Reconcile the one-attempt stale result with the CLI-owned single retry contract. |
| #883 | `06c57578` | P2 | `validation-phase-completeness` | Totally order stage, PGO, key, stale-entry, target, codegen, and link failures. |
| #883 | `06c57578` | P2 | `worker-lifecycle-cleanup` | Do not use caught panics as ordered diagnostics without accounting for the process-global panic hook. |
| #883 | `64e49de2` | P2 | `validation-phase-completeness` | Keep valid-magic malformed PGO rejection on cache-miss libLLVM codegen, not shallow setup validation. |
| #884 | `f14e4e81` | P1 | `resource-bound-completeness` | Cap fallible pipeline worker creation by outstanding work instead of eagerly spawning `jobs - 1` threads. |
| #884 | `f14e4e81` | P2 | `resource-bound-completeness` | Reject a malformed PGO header before allocating or reading the snapshot tail. |
| #884 | `f14e4e81` | P2 | `performance-evidence-completeness` | Derive both revisions' frontend/codegen counts from actual cold-cache outcomes instead of echoing the corpus unit count. |
| #893 | `ba9f229a` | P1 | `producer-evidence-propagation` | Bind the LLVM build identity to loader-owned metadata for the mapped image, not a replaceable pathname. |
| #893 | `ba9f229a` | P1 | `resource-bound-completeness` | Parse bounded LLVM image ranges instead of reading the whole native library. |
| #893 | `ba9f229a` | P2 | `error-contract-identity` | Treat unavailable immutable packaged blobs as clean cache misses. |
| #893 | `93e4a617` | P1 | `source-of-truth-drift` | Include SQLite development files in the Debian release-package dependency closure. |
| #893 | `93e4a617` | P2 | `resource-bound-completeness` | Bound and stream immutable packaged-cache action and CAS reads. |
| #893 | `426daa9e` | P1 | `memoized-input-completeness` | Revoke and never republish a frontend key whose checked HIR proves the key incomplete. |
| #893 | `426daa9e` | P2 | `rejected-artifact-self-healing` | Remove an oversized rejected writable CAS blob before later publication. |
| #893 | `426daa9e` | P2 | `validation-phase-completeness` | Report loaded-LLVM identity mismatch before target mismatch. |
| #893 | `fdfd2086` | P1 | `shared-artifact-concurrency` | Recheck key rejection after cache action and CAS validation so a concurrent reader cannot return a revoked value. |
| #895 | `ff6f4735` | P1 | `validation-phase-completeness` | Observe lexical static-input paths before canonicalization can omit or redirect their identity. |
| #895 | `ff6f4735` | P2 | `error-contract-identity` | Preserve arbitrary-byte raw entry paths through observed builds. |
| #895 | `ff6f4735` | P2 | `producer-evidence-propagation` | Require a rearm hash and the final graph to identify the same opened inode. |
| #895 | `ff6f4735` | P2 | `operation-matrix-completeness` | Remove EOF child streams from the poll set instead of spinning on `POLLHUP`. |
| #895 | `8f85f1d7` | P1 | `validation-phase-completeness` | Preserve filesystem classification independently of semantic static-file errors. |
| #895 | `8f85f1d7` | P2 | `error-contract-identity` | Encode watch diagnostic paths exactly once. |
| #895 | `8f85f1d7` | P2 | `operation-matrix-completeness` | Suppress retry-only warning echoes as the one-shot retry path does. |
| #895 | `8f85f1d7` | P2 | `operation-matrix-completeness` | Preserve frontend cache labels and summaries in watch output. |
| #895 | `8f85f1d7` | P2 | `operation-matrix-completeness` | Emit the PGO instrumentation destination in watch mode. |
| #895 | `8f85f1d7` | P2 | `fallible-resource-lifecycle` | Arm probe-path cleanup before a post-create `fstat` can fail. |
| #895 | `8f85f1d7` | P2 | `resource-bound-completeness` | Use a keyed union when retaining successful inputs after a failed revision. |
| #895 | `1f3d8a46` | P1 | `producer-evidence-propagation` | Record one identical stable checked-metadata observation per attempt. |
| #895 | `1f3d8a46` | P2 | `error-contract-identity` | Encode imported and static-input diagnostic paths without lossy display rendering. |
| #895 | `1f3d8a46` | P2 | `producer-evidence-propagation` | Include the observed graph root in opened-identity checks. |
| #895 | `1f3d8a46` | P2 | `operation-matrix-completeness` | Reject value-bearing spellings of the valueless `--watch` flag. |
| #895 | `1f3d8a46` | P2 | `fallible-resource-lifecycle` | Retry interrupted child-status and pipe-drain operations. |
| #895 | `1f3d8a46` | P2 | `fallible-resource-lifecycle` | Retry interrupted probe unlinks before abandoning owned cleanup. |
| #895 | `1f3d8a46` | P2 | `error-contract-identity` | Report the complete output path in probe identity errors. |
| #895 | `0262a877` | P2 | `validation-phase-completeness` | Open supported static inputs nonblocking before descriptor-level file-type validation. |
| #895 | `0262a877` | P2 | `resource-bound-completeness` | Index failed-input paths before retaining a disjoint prior baseline. |
| #896 | `879b33cb` | P1 | `review-checkpoint-completeness` | Require a wrapper-completed ancestor cycle before narrowing a continuation review to changed slices. |
| #897 | `e2cbaea8` | P1 | `analysis-fact-consumer-sweep` | Preserve every historical release root across owner transfers. |
| #897 | `e2cbaea8` | P2 | `analysis-fact-consumer-sweep` | Preserve exact projection paths when invalidating release roots. |
| #897 | `d51de4fc` | P2 | `analysis-fact-consumer-sweep` | Qualify historical release roots by storage generation so old consumption cannot poison a rebound local. |
| #897 | `d51de4fc` | P2 | `analysis-fact-consumer-sweep` | Keep partial-owner invalidation projection-specific so an ended field cannot poison a live sibling. |
| #898 | `c7ae6fa3` | P1 | `producer-evidence-propagation` | Reuse cached graph-wide string reachability during descriptor encoding instead of recomputing it per root. |
| #906 | `a6a88f8b` | P1 | `native-thread-state-scope` | Guard streamed TLS reads and TLS teardown at the delayed operation site, not only while constructing the stream. |
| #906 | `a6a88f8b` | P2 | `operation-matrix-completeness` | Reject shared-borrow receivers for the state-advancing raw stream cursor while retaining shared status and header getters. |

The rows above are the reviews whose logs are reachable from this checkout's
`.git/`. Capabilities produced in agent worktrees (#777 and later, other than
#786) keep their logs with those worktrees; their finding sets are unrecorded
here and belong to their owning sessions to reconcile.

## Rejected claims

Do not increase counts for rejected review claims. Add a short row only when a
claim is likely to recur and the durable explanation will save future review
time. The historical PR #232 `wyhash` final-mix claim remains the canonical
example: it was checked against the reference and rejected.
