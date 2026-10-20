# Session handoff

## Current work direction

The owner's 2026-10-20 request pass addresses actionable consumer defects and
open GitHub issues in all three consumers. Useful deferred capabilities start
with later investigation under [Plan230](docs/impl/230-request-follow-up-plan.md),
including the evidence-dependent LLM residuals; an unavailable reproducer is
not a completed fix. Continue completing PR and merge for each selected
capability before starting the next. Reconcile all three request registers and
real consumer evidence before selection. Consumer adoption remains external.

Prioritize existing std/library usability, useful bounded transport/library
composition, then measured execution/storage/startup improvements. Complex
unfinished compiler extensions have lower priority. Read the narrow owning
plan before selecting work; the shipped package sequence is not an unstarted
queue. [Plan51](docs/impl/51-package-composition-review-plan.md) closes the
process, filesystem and CLI overlap assessment without selecting new exec,
tree or CLI packages. Plans92, 93 and 95 record those dispositions.

Align v0.10.2 is released with the task-callback and WebSocket fixes. The workspace
version and [release notes](RELEASE_NOTES_0.10.2.md) own that checkpoint. The newer
request capabilities below are delivered on main after that release. A release build does
not authorize a versioned release. Inspect Git/GitHub for branch, PR and CI state.

## Recent completed capabilities

- [Plan234](docs/impl/234-cancellation-sensitive-math.md) supplies media AM-R12/19
  `expm1`/`log1p` on f32/f64 and their explicit vectors through width-exact native
  math. Signed-zero/domain semantics and independent finite samples qualify the
  pair; scalar vector fallback is visible. No universal ULP/SIMD promise or
  consumer adoption claim is introduced.

- [Plan229](docs/impl/229-out-slice-read-certification.md) certifies initialized
  `out slice<T>` reads for LLM R157, including imported/generic callers. Readable
  header storage remains separate from writable backing authority; alias,
  lifetime and malformed/uninitialized-storage refusals remain. Consumer
  acquisition adoption is pending.

- [Plan226](docs/impl/226-deflate-framing.md) exposes caller-selected output
  limits on all four `std.compress` decoders for LLM R156. Optional `max_bytes`
  bounds output growth and defaults to 1 GiB; complete-input validation and
  atomic owned output remain. Consumer PNG adoption remains external.

- [Plan228](docs/impl/228-task-owned-results.md) enables self-contained owned
  task results for LLM Request152: string, buffer, ordinary dynamic arrays,
  records, tuples and sums. Successful wait permits one consuming owned get;
  Copy reads remain repeatable. Group exit joins all work and drops untaken
  results across every wait generation. Shared capture restrictions remain.
  The contiguous-band witness removes index arrays and packed output words;
  consumer migration and product validation remain external.

- [Plan227](docs/impl/227-json-field-encoding.md) composes both JSON encoders with
  stable nested record fields for media AM-R11. Selected values retain existing
  schema, borrowing and owned-output rules; checked HIR authenticates the exact
  source, and eager byte limits preserve the conservative root reservation.

- [Plan226](docs/impl/226-deflate-framing.md) completes `std.compress` with zlib
  and raw DEFLATE framing. Both require one complete stream, reject trailing
  bytes and preserve the inclusive output cap and independent owned buffer.
  Existing gzip/zstd concatenation remains; PNG policy and consumer adoption
  remain external.

- [Plan225](docs/impl/225-integer-vector-round-even.md) completes integer vector
  bitwise/shift operations and scalar/vector `round_even()` for LLM Request151
  and media AM-R13. Both broadcast directions use single-check numeric context;
  scalar shift semantics and existing ties-away `round()` remain unchanged.

- [Plan224](docs/impl/224-task-callback-access.md) restores `pkg.web` and `pkg.ws`
  through producer-owned callback access requirements and return-flow facts.
  Shared task writes remain rejected; interface21 preserves the proof across
  unit/cache boundaries. Both packages rejoin the release cache corpus, and
  WebSocket reset failures retain their native transport error on macOS.

- [Plan223](docs/impl/223-copy-record-scan.md) composes prefix `scan` with Copy
  record inputs and independent Copy record accumulator arrays. Complete AoS
  prefix states retain ordinary allocation/cleanup; independent contents shed
  callback-only borrows while fresh headers and borrowed outputs stay tracked.

- [Plan222](docs/impl/222-sequential-par-map-captures.md) preserves original HIR
  capture identities in sequential `par_map` fallback. Sibling branches no longer
  reuse non-dominating capture operands; existing execution selection, ordered
  mapping, allocation and cleanup remain unchanged.

- [Plan221](docs/impl/221-copy-record-sort-keys.md) composes `sort_by_key` with
  the existing independent Copy record domain. Complete AoS values retain the
  existing one-key-per-element and owned-string-key cleanup rules. Key-only
  captures are call-live rather than result contents; sort captures borrow
  stable HIR identities across sibling branches. Direct sort and Move/view
  record elements retain their limits. Consumer adoption remains external.

- [Plan220](docs/impl/220-copy-record-comparator-sort.md) extends `sort_by` to
  already-formed independent Copy records for LLM Request27's composite keys.
  Complete AoS values use the existing stable comparator and ordinary call ABI;
  view/Move/callable records and direct/key sorting retain their limits. Transient
  allocation/header type equations now validate even without an escaping owner.
  Consumer adoption remains external.

- [Plan211](docs/impl/211-vectorization-cost-diagnostics.md) makes LLVM22's
  standalone vectorization-profitability refusal visible in ordinary
  `explain-opt` output for Request116. The diagnostic retains the source
  location and reports only the cost decision; interleaving and SLP observations
  remain distinct. This closes the observed reporting gap, not the umbrella's
  entire corpus or consumer performance qualification.

- [Plan209](docs/impl/209-builder-copy-reads.md) adds independent Copy indexed
  reads from unfinished typed builders for LLM Request28's numeric columns.
  Reads preserve initialized bounds and owner reservations while copied values
  outlive growth, build and Drop. Text-byte views, view-bearing elements and
  consumer adoption remain separate.

- [Plan208](docs/impl/208-comparator-sorting.md) adds `sort_by(cmp)` for scalar
  Copy elements, completing the comparator portion of LLM Request27. Stable
  insertion/merge comparisons permit sequential effects and preserve source
  content lifetimes without retaining comparator-only captures. Move elements
  and consumer adoption remain separate.

- [Plan207](docs/impl/207-task-copy-record-results.md) admits independent inline
  Copy record task results for media AM-R09. Successful wait authorizes repeated
  by-value reads; copied records outlive the group and captures. Typed result
  slots preserve target/source alignment. Plan228 supplies owned results; borrowed
  results and exclusive captures remain separate; consumer adoption remains external.

- [Plan206](docs/impl/206-cli-argument-slices.md) admits borrowed `slice<str>`
  arguments to the existing CLI parser for media AM-R10. The selected first
  element remains the command name; parsed text is independently owned, and
  slice transport adds no argv allocation. Consumer adoption remains external.

- [Plan205](docs/impl/205-indexed-array-field-copy-reads.md) admits
  `rows[row].values[index]` for independent Copy elements of existing owning
  array fields, including AM-R21's direct `poses.frames[i].points[j].x`
  projection through a canonical local Copy. Both indices retain signed bounds
  and the complete parent reservation; scalar and Copy-record results outlive
  the parent without a header/view escape or allocation. General views and existing whole fixed
  Move-record call/replacement restrictions remain separate.

- [Plan203](docs/impl/203-json-owned-string-root.md) admits a standalone owned
  `string` JSON decode root through the existing scalar decoder. Complete-root
  validation precedes ownership publication; failure frees unpublished bytes.
  Input/arena independence, ordinary Move/Drop and owner-bounded views remain
  explicit. Borrowed `str` roots remain separate.

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

## Sequential request follow-up

[Plan230](docs/impl/230-request-follow-up-plan.md) owns the queue and the required
evidence for unresolved LLM R37/R49/R80. Voice V006
([Plan231](docs/impl/231-fixed-scalar-heap-fields.md)) has the provider implementation:
primitive Copy fixed fields in heap records, exact zero-size storage and existing
complete-field Copy observation. Media AM-R16's independent fixed-field indexing
is implemented by [Plan232](docs/impl/232-indexed-fixed-field-values.md), including
chained record fields and an eager Copy snapshot before the inner index. The outer
receiver's backing must remain live through its read. Retained indexed-field
slices still require interprocedural authority and lifetime closure; the current
shared-helper-return witness loses read-only authority in both frontends.
[Plan233](docs/impl/233-c-record-fixed-arrays.md) implements AM-R17's positive
numeric fixed fields in C records, including raw storage and both native
by-value ABI families. [Plan234](docs/impl/234-cancellation-sensitive-math.md)
implements AM-R12/AM-R19's scalar/vector `expm1` and `log1p`. Consumer adoption
remains pending. AM-R14/AM-R20's `sin`, `cos` and `atan2` are next.
Establish consumer value, policy fit, the smallest complete useful capability,
prerequisites and cost before selection. Distinguish real settled restrictions
from implementation gaps; broad scope alone does not justify deferral.

## Deferred work and capability boundaries

Full K1 and adoption of the parked
[plan61 implementation](docs/impl/61-interprocedural-view-access-plan.md) and the
separately parked general aggregate-provenance experiment remain unselected.
The narrower HIR reuse candidate lacks
an established finite abstraction for recursive selected view origins;
whole-parameter lifetime roots lose independent field authority. Known
interprocedural write-authority and temporal-validation gaps remain.

The owner's 2026-10-20 direction selects the independent AM-R16 reads first
and a K1 scope/difficulty assessment before implementation.
[Plan230's assessment](docs/impl/230-request-follow-up-plan.md#k1-scope-and-difficulty-assessment-2026-10-20)
separates return/call authority, generation/validation transport, partial-owner
cleanup and active-payload exclusivity. A finite selected-origin representation
and a complete cross-unit proof remain the main K1 uncertainties. Neither parked
implementation is selected. Keep parked source and evidence archived outside the
active worktree; their adoption is not a prerequisite for independent work.

- Request28's independent Copy typed-builder reads ship under Plan209.
  [Plan210](docs/impl/210-text-builder-byte-views.md) confirms that text-byte
  views cannot reuse the current summaries: helper returns lose read-only
  authority. The complete view capability remains behind the interprocedural
  access prerequisite; this assessment does not resume K1. View-bearing typed
  elements remain separate.
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
