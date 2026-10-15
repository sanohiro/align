# Session handoff

## Current work direction

The owner requests continuous planned feature and improvement work: finish each
capability through PR and merge, then continue until asked to stop. The current
instruction was reaffirmed on 2026-10-14. It supersedes earlier one-batch stops.
Reconcile all three external request registers first, validate each request
against real consumer evidence and Align's design, and reassess deferred
requests after the current delivery. Implement coherent capabilities; retain a
reasoned deferral when the required design conflicts with Align's invariants.

Prioritize existing std/library usability, useful bounded transport/library
composition, then measured execution/storage/startup improvements. Complex
unfinished compiler extensions have lower priority. Read the narrow owning
plan before selecting work; the shipped package sequence is not an unstarted
queue. [Plan51](docs/impl/51-package-composition-review-plan.md) closes the
process, filesystem and CLI overlap assessment without selecting new exec,
tree or CLI packages. Plans92, 93 and 95 record those dispositions.

Align v0.9.0 is released; Requests138–141 are included. The workspace version and
[release notes](RELEASE_NOTES_0.9.0.md) own that checkpoint. A release build does
not authorize a versioned release. Inspect Git/GitHub for branch, PR and CI state.

## Recent completed capabilities

- [Plan202](docs/impl/202-native-json-allocation-authority.md) certifies native
  JSON record/AoS/union heap spines independently of borrowed input/arena text.
  Existing mutable calls now accept those owners, and cleanup releases their
  buffers inside arenas. Recursive union error cleanup follows the selected arm
  and clears failed payloads before outer cleanup; borrowed lifetimes and
  JSON/CSV SoA arena ownership remain unchanged.

- [Plan199](docs/impl/199-indirect-mutable-outputs.md) separates mutable-output
  dependencies from possible mutation, preserving borrowed siblings after another
  field changes. Known noncapturing function values consume the same checked
  records as direct calls. R126's archived real borrowed-session loop now passes
  whole/per-unit checking and object emission; consumer lifecycle and GPU
  qualification remain external. Interface20 certifies the complete fact; unknown
  or multiple targets and partial Move-field calls retain their existing limits.

- [Plan198](docs/impl/198-shared-match-field-origins.md) retains typed stable
  field origins through shared sum-payload matches for R130. Existing checked
  no-rebind effects preserve local view origins across repeated calls. Parent
  ownership, payload read-only access and alias/lifetime refusals remain;
  exclusive matched payloads and partial Move-field calls stay separate.

- [Plan197](docs/impl/197-fixed-text-array-borrows.md) certifies local fixed
  str-array shared/exclusive borrows through complete Copy construction and
  separate text/header authority. Input text lifetimes and sole cleanup remain;
  Move-element/record roots stay separate; Plan202 closes native JSON decoder proofs.

- [Plan195](docs/impl/195-mutable-borrow-storage-authority.md) separates readable
  borrowed-call contents from complete recursive owner and buffer authority.
  Typed arena dynamic arrays and fixed str-array record fields now execute
  through existing whole-place mutable calls. Plan197 closes named fixed str-array
  roots; Plan202 closes native JSON arena-decoder allocation authority.

- [Plan194](docs/impl/194-dynamic-array-field-writes.md) admits indexed writes to
  owned scalar-array record fields for media AM-R01 and LLM R147/R46. Exact selected
  backing lifetimes retain borrowed text through aliases and checked imported call
  summaries; eager operands precede current-header selection and bounds. No new allocation,
  owning-element overwrite or partial exclusive field borrow. Adoption is external.

- [Plan193](docs/impl/193-mixed-json-graphs.md) composes borrowed `str` and owned
  `string` in the existing recursive JSON graph for media AM-R02. Decode retains
  input/explicit-arena text lifetimes while strings and array spines remain
  individually owned; encoders return independent owned output. V4 graph and
  interface19 distinguish equal-layout text ownership. Consumer adoption is external.

- [Plan192](docs/impl/192-owned-field-extraction.md) completes direct owning
  record-field extraction for voice V002 and LLM R36: arrays, nested Move
  records and tuples transfer through the existing ownership model, with exact
  projected storage and cleanup. Deeper/indexed extraction and partial exclusive
  borrowing remain separate. Consumer adoption remains external.

- [Plan191](docs/impl/191-unicode-nfc.md) supplies explicit Unicode16 NFC through
  `pkg.unicode` for voice V004, with independent owned output, fallible allocation
  and official complete NFC conformance. Tokenizer adoption remains external.

- [Plan190](docs/impl/190-tuple-carrier-composition.md) composes existing flat
  tuples with Option/Result, user sums, ordinary record fields and Align
  callbacks for voice V003. Contextual layout, cleanup and borrowed origins
  preserve the existing ownership model; collection/box/native and shared Move
  projection restrictions remain. Consumer adoption remains external.

- [Plan189](docs/impl/189-binary-byte-write-effects.md) records binary setter
  guarantees in the existing mutable Copy-view effect model for Request146.
  Repeated subview calls preserve caller backing; empty-capable fill/copy and
  conditional writes retain conservative may effects. Consumer adoption remains
  external.

- [Plan188](docs/impl/188-owned-json-record-array-root.md) admits JSON decoding
  directly into owned record arrays for Request133. The existing V3 schema and
  deep cleanup make every row independent of input/arena; checked root kinds and
  owner-bounded element views preserve the ownership contract. Consumer adoption
  remains external.

- [Plan187](docs/impl/187-indexed-json-array-grammar.md) validates complete JSON
  record-array syntax for AoS/SoA and consumes selected numeric tokens exactly.
  Malformed syntax rejects before decoded allocation; valid-syntax schema
  failures retain the existing partial-owner cleanup. Request133 schema
  admission remains separate.

- [Plan186](docs/impl/186-json-doc-keys.md) adds linear `json.doc.keys()`
  materialization for voice V001, preserving document order, duplicates and
  `elems()` pairing with explicit arena/input lifetimes. Consumer adoption
  remains external.

- [Plan185](docs/impl/185-read-only-regular-file.md) adds `fs.open_ro_regular`
  for media AM-R03: regular-file admission and positional I/O share one owned
  descriptor and the existing File model. Consumer adoption remains external.

- [Plan184](docs/impl/184-record-array-subslice-producer.md) certifies existing
  AoS record-array subslices beside fallible owned returns for media AM-R06.
  Exact source/result/stride checks retain shared backing authority and cleanup;
  GPU/image qualification and consumer adoption remain external.
- [Plan183](docs/impl/183-json-string-array-root.md) adds the inferred JSON
  `array<string>` root using independent string owners and complete prefix
  cleanup. Voice V005 consumer adoption remains external.
- [Plan182](docs/impl/182-copy-record-producer.md) certifies independent inline
  Copy data projected from borrowed records beside fresh owned return storage
  for Request134. Source authority, contained views and Move siblings retain
  their existing checks; general aggregate provenance remains separate.
- [Plan181](docs/impl/181-cli-float-occurrences.md) supplies finite `std.cli`
  float flags and explicit occurrence counts for media AM-R04/05. All CLI
  receivers remain live through later eager operands; scalar observations
  release their call reservation, while `get_str` retains its owner dependency.
  Consumer adoption remains external.
- [Plan180](docs/impl/180-structured-task-progress.md) supplies caller-assisted
  structured-task progress for media AM-R08; consumer adoption remains external.
- [Plan179](docs/impl/179-form-output-prefix.md) maps admitted same-width
  prefixes in expanded form output, preserving space-to-plus conversion and the
  scalar remainder. Native byte/extent/allocation and source owners qualify the
  unchanged contract. The encoding record retains both the adopted out-of-line
  wrapper and rejected inlined experiment, with all local gains and costs.
- [Plan178](docs/impl/178-percent-output-prefix.md) bulk-copies admitted ordinary
  prefixes in expanded percent/component and percent/path output. The existing
  exact allocation and scalar remainder writer remain; native extent/allocation
  and source owners pass. Local gains, control costs and the rejected experiment
  are retained in the encoding measurement record.
- [Plan177](docs/impl/177-form-same-length-output.md) maps same-length form
  output through its existing checked count, preserving space-to-plus conversion
  and expanded percent escapes. Native byte/allocation and source owners qualify
  the unchanged contract. The local 240-case corpus retains gains and costs.
- [Plan176](docs/impl/176-unchanged-escaped-output.md) copies unchanged
  percent/component, percent/path and HTML output using exact expansion counts,
  including template appends. [Plan169](docs/impl/169-percent-form-length-count.md)
  owns the preceding bounded narrow counts. The
  [encoding measurement record](bench/encoding_writes/README.md) retains every
  sample, producer identity and short/expanded/control cost; no portable or
  application-throughput promise follows.
- [Plan175](docs/impl/175-rng-record-composition.md) admits Copy RNG fields in
  ordinary concrete/generic/nested records with the existing 32-byte state.
  Explicit mutable borrowing advances stored state; native receiver-local and
  specialized placement restrictions remain. Request145 adoption is external.
- [Plan174](docs/impl/174-named-arena-loop-validation.md) inventories named-arena
  handles in checked-HIR loops while preserving exact range, type, scope and
  region-exit restrictions.
- [Plans171](docs/impl/171-assignment-target-liveness.md),
  [172](docs/impl/172-field-assignment-publication.md) and
  [173](docs/impl/173-deferred-aggregate-transfers.md) close indexed-assignment
  target liveness, complete field-RHS publication and deferred aggregate source
  transfers. Counted native/control-flow and HIR/MIR owners qualify the existing
  ownership contract; these repairs do not supply general provenance precision.
- [Plan170](docs/impl/170-fixed-scalar-array-borrows.md) delivers Request148's
  local fixed scalar-array borrows through complete inline construction and
  separate readable-payload/exclusive-storage checks. Strict literal validation,
  pooled rodata and isolated versioned-loop construction remain.
- [Plans163](docs/impl/163-fixed-array-recursive-drop.md),
  [164](docs/impl/164-fixed-owned-string-arrays.md) and
  [165](docs/impl/165-fixed-dynamic-array-elements.md) close recursive fixed-array
  Move/Drop and deliver Requests129/143's owned-string/dynamic-array elements.
  Their exact formation, shared borrowing and producer limits remain authoritative.
- [Plan162](docs/impl/162-retained-directory-available-space.md) delivers
  Request144's advisory retained-directory available-byte observation without
  Align/Rust heap allocation. Quotas, reservations and fallible writes remain
  consumer policy.

Earlier completed work lives in the owning implementation plans and the
[2026-10-13 historical handoff](docs/archive/HANDOFF-2026-10-13.md).
That archive preserves the prior narrative; it is not a source of current status.

## Deferred work and capability boundaries

Full K1 and adoption of the parked
[plan61 implementation](docs/impl/61-interprocedural-view-access-plan.md) and the
separately parked general aggregate-provenance experiment remain unselected.
The narrower HIR reuse candidate lacks
an established finite abstraction for recursive selected view origins;
whole-parameter lifetime roots lose independent field authority. Known
interprocedural write-authority and temporal-validation gaps remain.

The owner's 2026-10-14 instruction authorizes fresh assessment of deferred
requests after the current delivery. It does not select either archived
implementation: qualify each real-use requirement and its safety strategy
independently. Keep parked source and evidence archived outside the active
worktree; their adoption is not a prerequisite for independent work.

- Request28's indexed/view builder readers still require their own provenance
  closure; the shipped scalar length and mutable-output capabilities do not
  resume K1.
- [Plan196](docs/impl/196-indexed-array-field-length.md) implements R137's scalar
  `rows[index].ids.len()` observation across stable fixed/dynamic AoS and slice
  bases. The parent remains owned, checked bounds precede the read, and the i64
  result retains no borrow. Wider indexed Move-field views and their Plan61
  authority prerequisite remain unselected; consumer adoption is external.
- Unknown, multiple-target and capturing function-value mutable outputs retain
  conservative handling. Plan199 closes the uniquely resolved noncapturing case;
  it does not infer projected origins or admit partial exclusive Move fields.
- Request142's exclusive matched owned payload is a separate ownership
  capability. It requires its own complete projection/alias/cleanup design;
  existing shared and owning matches do not imply exclusive admission.
- Request78 enum fields, Request128 shared optional native-resource matching,
  and std.id scalar admission retain their friction-ledger prerequisites.
- Fully escaping function values need a selected consumer and a settled
  heap-owned environment/Drop model. Top-level array<str> JSON decode needs a
  result carrying the input region; record-contained array<str> already ships.
- PostgreSQL COPY remains consumer/measurement-gated under D13. pkg.kv TLS and
  wider Redis operations, additional cloud capabilities and other transports
  retain their individual design/consumer gates. The first pkg.web consumer
  application remains separately owner-scheduled.
- Real voice/media adapters, GPU qualification and product policy remain with
  their consumers. The bounded multimodal reference and HTTP write budgets are
  implemented under plans90–91; they do not schedule engine integration.
- Request149 is a non-blocking numerical-kernel policy discussion with no
  implementation deadline or selected migration. Existing native consumers may
  continue; no language widening or kernel rewrite is implied.

## External consumer boundary

Before request-driven work, read `docs/align-requests.md` in each of
`../align-llm`, `../align-media`, and `../align-voice`. Reconcile shared needs,
follow the owner's priority, and inspect affected use in all three consumers.
Their registers own live request/adoption state. Provider implementation and
consumer verification are separate; write Align's responses to affected
registers, leaving those edits uncommitted unless requested. Do not change
consumer code, tests, pins, branches, commits or merge state merely to advance
a status. [CLAUDE.md](CLAUDE.md) owns the complete scope rule.

At a completed request batch, run exactly `cargo build --release --workspace`
with the required local LLVM/native environment. That builds artifacts only;
a version/tag release still requires an explicit user request.

## Compiler and library baseline

M0–M15, LLVM22, separate compilation, per-unit/in-process/frontend caches,
parallel codegen, ThinLTO and PGO are implemented. The
[roadmap](docs/impl/07-roadmap.md) retains milestone evidence. The
[build-performance plan](docs/impl/21-build-perf-plan.md) owns caching, watch,
function partitions, gate costs and later sema performance work. Remaining
performance axes require their own evidence; completed items are not a queue.

The post-XML consolidation and direct stage-free chunks.par_map virtual source
are complete under [plan32](docs/impl/32-post-xml-consolidation-plan.md).
[Plan31](docs/impl/31-execution-storage-startup-plan.md) governs independent later
optimization gates. HTTP streaming, asymmetric signatures, pkg.db, pkg.web,
pkg.kv, pkg.csv, pkg.ws, pkg.template, std.xml and S3/SigV4/presigning are shipped
within their exact design ledgers. Consult those ledgers for current limits and
[the runtime ABI inventory](docs/impl/20-runtime-abi-ledger.md) for active keys;
old inventory counts in the archive are historical.

## Working and verification notes

Follow [CLAUDE.md](CLAUDE.md) for the full review, final-SHA preflight, publication,
external-consumer and cleanup rules. Use `scripts/cargo.sh` for ordinary local
Cargo work; it resolves LLVM22 and platform native paths. Keep Clippy in
`CARGO_TARGET_DIR=target/clippy` and reuse compatible build/test caches.

After a runtime source change, build the workspace before driver tests or
`alignc run`; user programs link the runtime archive. Do not mutate runtime
sources during a test run. Select the narrow owner from
[the test policy](docs/impl/16-test-policy.md); timings are separate evidence.
DB-boundary changes require `scripts/db-verify-local.sh` before push.

Preflight classifies the committed range. Run the matching tier once against
final HEAD, then push and open/update the PR without changing that HEAD. A small
non-normative Markdown-only change uses `--docs-only`; it needs whitespace and
at most one directly relevant consistency check, not a compiler build/review.
Independent code review and gates follow the canonical one-cycle rules.

Keep this file about current direction, boundaries and the next decision,
roughly 200 lines. Archive completed chronology in one pass instead of adding
PR lifecycle detail. Preserve current caches and required evidence; remove
obsolete task binaries, traces and scratch after their final consumer.
