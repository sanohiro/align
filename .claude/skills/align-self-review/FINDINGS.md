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

## Root-cause counts

| Root-cause key | Events | PRs | Prevention status |
|---|---:|---:|---|
| `validation-phase-completeness` | 9 | 4 | Recurred past the added owner (#727: static-validation-after-native-prepare; unvalidated v3 tail-reserved field). Required next: deduplicate the copied header validators into one shared function so an ABI version bump has one owner, and extend the parameterized phase-order owner across prepared/transaction paths. |
| `cross-stage-abi-exactness` | 4 | 1 | Covered by Gate 4 and exact ABI owner matrices. |
| `native-api-version-boundary` | 4 | 1 | Covered by Gate 2 plus version/conversion goldens. |
| `ownership-allocation-owner` | 3 | 1 | Covered by Gates 1, 2, and 4; every allocation and thunk needs one named owner. |
| `sql-scanner-lexical-closure` | 3 | 2 | Promoted: parameterized lexical-mode and token-free-tail owners are mandatory. |
| `native-link-order-closure` | 3 | 1 | Promoted: preserve caller order around the dependency closure and test arbitrary prefix/suffix libraries. |
| `producer-evidence-propagation` | 1 | 1 | Watch; exact producer-to-consumer evidence owner required. |
| `public-surface-completeness` | 2 | 2 | Promoted: compare the complete exported surface to the ledger and require an internal/sealed boundary for cross-module helpers. |
| `operation-matrix-completeness` | 3 | 2 | Three events: automation required. Add one parameterized dual-driver parity owner that runs the same descriptor/status/sentinel matrix through both drivers and asserts identical error-class mapping; also check native command tags (PostgreSQL answers `COMMIT` in an aborted transaction with `PGRES_COMMAND_OK` and a `ROLLBACK` tag). |
| `error-contract-identity` | 1 | 1 | Watch; pin error class, query identity, and precedence. |
| `source-of-truth-drift` | 1 | 1 | Watch; run ledger-to-prose and stale-obligation search. |
| `owner-test-topology` | 1 | 1 | Watch; retain whole/per-unit owners when refactoring fixtures. |
| `test-global-state-isolation` | 1 | 1 | Watch; prefer child-scoped environment and RAII restoration. |
| `test-entry-abi-exactness` | 1 | 1 | Watch; compile fixtures through the real entry ABI. |
| `mutation-noop-idempotency` | 1 | 1 | Watch; prove unchanged input performs no write. |
| `native-evidence-applicability` | 1 | 1 | Watch; project optional native evidence only for engines/access methods whose semantics are proved. |

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

## Rejected claims

Do not increase counts for rejected review claims. Add a short row only when a
claim is likely to recur and the durable explanation will save future review
time. The historical PR #232 `wyhash` final-mix claim remains the canonical
example: it was checked against the reference and rejected.
