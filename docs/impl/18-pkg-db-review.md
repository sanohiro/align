# Query-centric database design review

## Status

**REVIEW OF RECORD — 2026-07-27.**

Reviewed design:

- [`pkg-design/db.md`](pkg-design/db.md)

Align sources checked:

- `draft.md`;
- `docs/language-spec.md`;
- `docs/design-notes.md`;
- `docs/open-questions.md`;
- `docs/impl/00-overview.md`;
- `docs/impl/01-pipeline.md`;
- `docs/impl/02-frontend.md`;
- `docs/impl/03-types.md`;
- `docs/impl/04-mir.md`;
- `docs/impl/07-roadmap.md`;
- `docs/impl/08-memory-model-v2.md`;
- `docs/impl/10-cache-first-optimization.md`;
- `docs/impl/pkg-design/web.md`;
- the Move-handle, I/O, error, FFI, region, builder, and streaming contracts in
  `docs/impl/std-design/` and `docs/impl/core-design/`.

This is a design and implementation-feasibility review. It does not authorize or contain a database
driver, compiler implementation, runtime implementation, commit, or release.

## 1. Verdict

**REVISE.**

The SQL-native, Query-centric direction is correct and should be retained. The original package
proposal was not implementable as ordinary Align code without either unsound native views,
database-named compiler exceptions, runtime reflection, or APIs that conflict with current
generics/package rules. The revised design resolves the API and compilation questions and records
the missing general language facilities in
[`17-library-boundary-prerequisites.md`](17-library-boundary-prerequisites.md).

Database driver implementation remains **blocked on L1a–L6**. Prerequisite implementation may start
with L1a. This is not a recommendation to defer the ideal design: L1a–L6 are mandatory scheduled
work and part of the database delivery plan.

`pkg.db` has no dependency on `std.http`. Both should eventually use the same package-defined
resource, borrow-provenance, and owner-tied native-view machinery. Sharing that language foundation
is desirable; importing the HTTP package or extracting an HTTP-flavored database abstraction is not.

## 2. Finding register

Severity means whether the issue blocks the corresponding v1 capability, not whether the design
idea is rejected.

### F1 — tagged Move payloads are not recursively supported

- **Classification:** current Align conflict; ownership or soundness risk.
- **Problematic design location:** `pkg-design/db.md` §15 structured `db.Error`, Query `Output`
  through `Result`, and nullable owned native detail.
- **Current Align constraint:** several `Option`/`Result`/user-sum payload paths still reject Move
  structs or rely on compiler-known handle exceptions. Discard, propagation, and branch cleanup do
  not share one recursive payload Drop plan.
- **Actual failure:** `Option<string>` inside `db.NativeError`, `Result<MoveOutput,db.Error>`,
  `else`, `?`, or `match` can be rejected or, if admitted incompletely, leak/double-drop the active
  payload.
- **Recommendation:** implement L1a recursive `DropPlan` framework plus the required
  `Option<string>` field leaf, then L1b Move structs/sums in all finite non-recursive
  sum/Option/Result payload paths. Tag-test Drop and null every moved source.
- **v1 impact:** blocker for the public error and Output contract. It is the first prerequisite PR.

### F2 — by-value Move parameters cannot express reusable handles

- **Classification:** prerequisite feature missing; current Align conflict.
- **Problematic design location:** `db.conn`, `db.tx`, prepared statement reuse, and a Pure
  Query-specific shaper.
- **Current Align constraint:** an ordinary Move parameter consumes its argument. Existing builtin
  handles have bespoke receiver behavior; there is no general public function parameter mode for a
  shared or exclusive borrow.
- **Actual failure:** a package function either consumes the connection/statement on every call or
  requires another compiler-known database handle exception. A reusable Move shaper state would
  have the same problem; Copy-state in-place mutation is covered separately by F20.
- **Recommendation:** L2 adds `borrow` and `borrow mut` parameter modes, unchanged call syntax,
  caller-side alias checks, interface-visible return provenance, and Pure classification for
  mutation rooted only in an explicit exclusive input.
- **v1 impact:** blocker for reusable execution, prepared statements, and compound Output.

### F3 — current region inference cannot describe imported borrow-returning functions

- **Classification:** specification ambiguity; ownership or soundness risk.
- **Problematic design location:** `exec_conn`, `exec_tx`, row accessors, and any helper returning a
  view derived from an input owner.
- **Current Align constraint:** current special cases compute `region_of` for known operations.
  A separately compiled package function has no general exported summary saying which input
  generations may back its return.
- **Actual failure:** same-unit and imported calls could disagree, or an imported returned view could
  be treated as Static and survive owner Drop.
- **Recommendation:** infer and serialize `ReturnBorrowSummary` and `ReturnRegionSummary`. Include
  every backing input, including a by-value Copy view/resource reference, not only parameters spelled
  `borrow`.
- **v1 impact:** soundness blocker.

### F4 — statement and rows lifetimes require dependent resources

- **Classification:** prerequisite feature missing; ownership or soundness risk.
- **Problematic design location:** `db.stmt<P,R>`, `db.rows<R>`, `prepare`, and `rows_stmt`.
- **Current Align constraint:** existing opaque handles are independently owned compiler variants.
  A Move child cannot generally state that its validity and Drop must precede a parent resource's
  move, mutable borrow, or Drop.
- **Actual failure:** a connection may close while its native statement exists; a statement may
  reset/finalize while rows still expose its current buffer; the result is use-after-close,
  use-after-finalize, or native double cleanup.
- **Recommendation:** L3 package-defined resources plus
  `resource.from_raw_borrowed(native,parent_ref)`. A live dependent child freezes the required parent
  generation until child Drop. Representation intrinsics are available to the declaring module's
  descendant subtree; raw-only Drop-hook modules never import the root type, keeping driver
  construction acyclic without a public raw constructor.
- **v1 impact:** blocker for safe prepare and streaming. D2 may avoid prepared statements but still
  needs the general connection resource.

### F5 — raw FFI pointers cannot safely become borrowed `str`/byte views

- **Classification:** prerequisite feature missing; ownership or soundness risk.
- **Problematic design location:** zero-copy Row text/blob decode.
- **Current Align constraint:** FFI can manipulate raw values, but there is intentionally no safe
  general raw-pointer-to-view return. Existing std views use builtin-specific lowering.
- **Actual failure:** ordinary package code cannot construct the promised view, while an unowned
  constructor would permit invalid length/alignment/UTF-8 or a view that outlives the SQLite/libpq
  buffer.
- **Recommendation:** L3 adds unsafe, owner-required
  `resource.view_from_raw(owner_ref,ptr,len)`. Validate length/null/alignment and UTF-8 before
  producing a safe view; retain owner generation provenance.
- **v1 impact:** blocker for text/blob streaming. The scalar-only D2/D4 vertical can land before
  exposing text views, but no unsafe public substitute is allowed.

### F6 — `next()` invalidation needs generation semantics

- **Classification:** specification ambiguity; ownership or soundness risk.
- **Problematic design location:** `db.rows<Row>` where `str` or byte fields point into the current
  native row.
- **Current Align constraint:** a long-lived `rows` owner region alone is too broad because the next
  step may reuse the same storage.
- **Actual failure:** a caller could retain row N's `str`, call `next()`, then observe overwritten
  bytes through the old view.
- **Recommendation:** `next(borrow mut rows)` ends the previous resource generation. Returned row
  views belong to the fresh current-row generation; the next mutable borrow invalidates them.
  Persisting data requires visible `clone_in(out)`.
- **v1 impact:** blocker for D8 typed streaming; not needed for D2/D4 scalar-only `one`.

### F7 — caller-selected materialization has no nameable region capability

- **Classification:** prerequisite feature missing; current Align conflict.
- **Problematic design location:** `one`, `maybe_one`, `all`, and compound `Output` accepting an
  allocation destination.
- **Current Align constraint:** `arena {}` establishes an inferred lexical region but does not
  produce a value that an ordinary library function may receive.
- **Actual failure:** the package must choose a hidden heap, hard-code a builtin, or return values
  tied to an unexpressed lifetime.
- **Recommendation:** L4 adds `arena out {}`, the non-storable Copy `region` capability, and
  `clone_in(out)`. It reuses the existing arena begin/end and escape model; no visible lifetime
  annotations are introduced.
- **v1 impact:** blocker for materialized and compound results; not for a scalar command probe.

### F8 — generic `db.fold` is incompatible with Align generics and hides control

- **Classification:** current Align conflict; Align philosophy conflict.
- **Problematic design location:** the original generic Query shaper/fold proposal.
- **Current Align constraint:** generics are deliberately narrow and scalar-oriented; there is one
  sequential control form, `loop`. Higher-order generic database folds would require new generic
  shape classes and complicated region/effect signatures.
- **Actual failure:** the API is not expressible today, and a database-specific callback rule would
  hide the row loop and encourage special compiler treatment.
- **Recommendation:** remove `db.fold`. Query-local Impure `run` creates one rows resource and has
  one visible `loop`; a Pure step receives `borrow mut state`, zero or more separate
  `borrow mut` builders, `row`, and `out`, but no DB handle. Effect checking structurally prevents
  additional SQL from the shaper.
- **v1 impact:** original design blocker; resolved in the revised specification without adding
  general higher-order generics.

### F9 — region array building is missing for efficient compound Output

- **Classification:** prerequisite feature missing; performance risk.
- **Problematic design location:** one-to-many shaping and `all`.
- **Current Align constraint:** the shipped heap `array_builder<T>()` has zero-copy freeze, but there
  is no builder that grows inside a caller-selected region and handles plain structs containing
  borrowed/region-cloned fields.
- **Actual failure:** building a child list requires hidden heap allocation, repeated copying, or an
  unbounded pre-count/two-pass algorithm. Retaining current-row views is unsound.
- **Recommendation:** L6 `array_builder<T>(out)` for recursive `RegionPlain` elements, chunked
  growth, and exactly one disclosed compacting pass. Require `clone_in(out)` for short row views.
  Keep each builder as its own mutable local; a Pure step may receive it through `borrow mut`, but
  no builder enters the shaping-state aggregate. `all<R>` is structurally restricted to
  `RegionPlain<R>`, which every v1 static Row must satisfy.
- **v1 impact:** blocker for D10 and the initial release's required one-to-many example; later than
  the scalar D2/D4 probes.

### F10 — `.sql` files are not current deterministic compilation inputs

- **Classification:** prerequisite feature missing; specification ambiguity.
- **Problematic design location:** same-basename `.align`/`.sql` Queries and offline checking.
- **Current Align constraint:** the current frontend's unit identity is based on reachable Align
  sources. Arbitrary sibling files are neither discovered nor represented in the Query producer's
  cache identity.
- **Actual failure:** SQL-only edits can reuse stale code/metadata, directory scans become
  nondeterministic, or build scripts/network access enter ordinary builds.
- **Recommendation:** L5 compiler-registered static inputs. Resolve an exact module-relative literal
  or same-basename path, reject root/symlink escape, preserve exact UTF-8 bytes, register SourceMap,
  and hash logical path/content/kind into producer implementation identity. Registration follows
  resolved callee identity; only a source/import-digest-bound manifest may bypass fresh resolution.
- **v1 impact:** blocker for a named file Query and D1.

### F11 — one descriptor needs two compilation artifacts

- **Classification:** implementation complexity; specification ambiguity.
- **Problematic design location:** exporting `db.query<Params,Row>` across module boundaries.
- **Current Align constraint:** a normal public interface should not force every consumer to
  recompile for private implementation bytes, but SQL and generated binder/decoder code must still
  invalidate the producer.
- **Actual failure:** putting all SQL in the public interface causes broad rebuilds; omitting it from
  all artifacts gives stale linking/metadata; runtime descriptor reflection violates the design.
- **Recommendation:** L5 emits `StaticQueryArtifact` for producer implementation and
  `IStaticQuery` for public contract. The interface carries Query identity, Params/Row identities,
  driver restriction, and public static semantic options. SQL bytes/hash, occurrence maps, checked
  metadata policy/digest, and generated thunk bodies stay in the producer artifact.
- **v1 impact:** blocker for separate/incremental compilation and D1.

### F12 — named SQL parameter rewriting is lexical, not textual

- **Classification:** implementation complexity.
- **Problematic design location:** common named Params with SQLite and PostgreSQL.
- **Current Align constraint:** the compiler has no SQL parser, and normal builds must remain
  offline. PostgreSQL needs `$n`; SQLite accepts named bindings.
- **Actual failure:** regex replacement corrupts string literals, comments, casts, or dollar-quoted
  bodies. Repeated parameter names can be assigned inconsistent ordinals.
- **Recommendation:** a small dialect-aware scanner recognizes code/comment/string/quoted
  identifier/dollar-quote states. Produce an occurrence table plus source spans. SQLite binds its
  named token/index; PostgreSQL rewrites first source occurrence to `$1`, reuses that ordinal for
  repeats, and binds Params in declared-field order mapped through the table. Keep exact source SQL
  hash separate from deterministic per-driver wire SQL/hash and retain a source-to-wire map. Reject
  mixed named and positional forms.
- **v1 impact:** blocker for D4 PostgreSQL; D1 must prove the scanner before libpq.

### F13 — runtime reflection is unnecessary and off-design

- **Classification:** Align philosophy conflict; performance risk.
- **Problematic design location:** typed Params binding and `Row` decoding.
- **Current Align constraint:** Align has no general runtime reflection and intentionally avoids
  hidden name-based work.
- **Actual failure:** a reflective mapper adds per-row name lookup, dynamic typing, allocation, and
  late errors; adding reflection only for DB would violate package/language layering.
- **Recommendation:** L5/D1 generate monomorphized binder and ordinal decoder thunks from known
  Params/Row layouts. Checked metadata strengthens static checks; every execution still validates
  runtime column count/type/nullability before constructing Row.
- **v1 impact:** blocker. No reflective fallback.

### F14 — proposed package names are modules, not independent package versions

- **Classification:** current Align conflict; specification ambiguity.
- **Problematic design location:** `pkg.db`, `pkg.db.sqlite`, `pkg.db.postgres`.
- **Current Align constraint:** one vendored package subtree has one version/build identity and an
  acyclic module graph. A common module importing driver modules while they import common would
  cycle.
- **Actual failure:** treating the three names as separately versioned first-party packages creates
  impossible dependency direction and falsely implies external driver registration.
- **Recommendation:** make them three public module boundaries in one `pkg/db` subtree. Root/internal
  never imports public drivers. Public drivers call a closed common/internal tagged dispatch. A
  third-party driver uses another package root until an external registration ABI is designed.
- **v1 impact:** blocker for repository/package layout; resolved by revised §4.

### F15 — a public connection/transaction trait hierarchy is unavailable and unwanted

- **Classification:** current Align conflict; Align philosophy conflict.
- **Problematic design location:** allowing one Query to execute against `db.conn` or `db.tx`.
- **Current Align constraint:** there is no required public trait/object system, and adding one for
  two closed resource kinds conflicts with the narrow-generics/one-way design.
- **Actual failure:** duplicated Query APIs or a broad new language feature would appear solely to
  abstract two execution roots.
- **Recommendation:** `exec_conn(borrow conn)` and `exec_tx(borrow tx)` return one Copy `db.exec`
  sum over `resource_ref<db.conn|db.tx>`. It retains generation provenance and uses closed internal
  dispatch.
- **v1 impact:** blocker for common transaction reuse; resolved by L2/L3 plus D7.

### F16 — online ordinary builds violate hermetic compilation

- **Classification:** Align philosophy conflict; performance risk.
- **Problematic design location:** authoritative SQLite/PostgreSQL Query validation.
- **Current Align constraint:** compilation inputs must be explicit and reproducible; normal builds
  must not depend on services, credentials, or mutable database state.
- **Actual failure:** builds become slow, non-reproducible, secret-dependent, and unavailable
  offline.
- **Recommendation:** normal build uses declared contracts and checked artifacts only. Explicit
  `alignc db prepare ...` invokes real SQLite/libpq, writes canonical `.align-db/<driver>` metadata,
  and `--check` verifies without writing. PostgreSQL URL comes through an explicitly named runtime
  environment variable only in the prepare tool.
- **v1 impact:** blocker for checked metadata. D2 may run declared/runtime checks before D3, but may
  not claim database-checked Query safety.

### F17 — native options need typed scope and explicit disposition

- **Classification:** specification ambiguity; implementation complexity.
- **Problematic design location:** connection, Query, prepare, execution, transaction, metadata,
  and EXPLAIN options.
- **Current Align constraint:** there is no reflection-friendly generic property bag or public
  driver trait. SQLite and PostgreSQL have materially different controls.
- **Actual failure:** a common string map silently ignores controls, confuses static Query identity
  with runtime execution state, and loses type checking.
- **Recommendation:** finite sum `*Option` types per scope, passed as typed slices; `[]` means none.
  Static Query options must be compiler-known literal constructor lists and enter the artifact.
  Native operations accept separate common and driver-native slices. Every option is applied,
  unsupported, or conflicting; never ignored.
- **v1 impact:** D9 blocker, not a D2/D4 scalar vertical blocker.

### F18 — broad metadata/native/dynamic work obscures the first product

- **Classification:** unnecessarily broad scope.
- **Problematic design location:** complete catalogs, COPY/pipeline/backup/blob/callbacks, pools,
  additional drivers, and dynamic rows.
- **Current Align constraint:** each new native resource/callback/value shape carries ownership,
  cleanup, cache, and FFI proof obligations. Shipping all at once prevents a reviewable vertical.
- **Actual failure:** the first implementation PR becomes unreviewable and may validate neither
  Query artifacts nor lifecycle thoroughly.
- **Recommendation:** D2 and D4 are exact scalar verticals; D10 proves compound Output. Schedule
  migrations D11 and metadata/EXPLAIN D12 in the first public database release; batch/native paths
  D13 and dynamic SQL/callbacks D14 remain committed additive work. These are scheduled
  prerequisites for their features, not vague “maybe later” items.
- **v1 impact:** D1–D12 block the first public database release. D2/D4 remain the minimum driver
  verticals used to validate architecture before that release is complete.

### F19 — reserved `resource`/`borrow` words make required intrinsics unparsable

- **Classification:** current Align conflict; specification ambiguity.
- **Problematic design location:** L2 parameter syntax and L3 `resource.from_raw`/
  `resource.borrow` calls.
- **Current Align constraint:** dotted paths require identifier segments; globally reserving
  `resource` and `borrow` removes those identifiers.
- **Actual failure:** the canonical resource API cannot reach parsing, and `out: region` is
  greedily misread as an out-mode prefix.
- **Recommendation:** make `borrow`, `out`, and `resource` contextual. Exact token lookahead
  distinguishes mode/declaration positions from ordinary parameter names and dotted paths.
- **v1 impact:** blocker for L2/L3 syntax; resolved in the revised prerequisite grammar.

### F20 — Move-only `borrow mut` cannot update the canonical Copy shaper state

- **Classification:** current Align conflict; prerequisite feature missing.
- **Problematic design location:** `pkg-design/db.md` §§6.3 and 7.2 `step(borrow mut state: State)`.
- **Current Align constraint:** the proposed L2 text originally rejected both borrow modes for Copy,
  but the state made from bool/integer/view fields is Copy.
- **Actual failure:** passing it by value discards mutations, while the declared `borrow mut` form is
  rejected.
- **Recommendation:** keep shared borrow Move-only, but admit mutable borrow of any writable Move or
  Copy place with one pointer-to-caller-storage ABI and generation invalidation.
- **v1 impact:** blocker for D10; resolved as a general in-place state capability.

### F21 — resource Drop hooks lacked valid syntax and separate-compilation linkage

- **Classification:** current Align conflict; ownership or soundness risk.
- **Problematic design location:** L3's original `unsafe fn drop_conn(raw)` and private sibling hook.
- **Current Align constraint:** Align has `unsafe {}` blocks, not unsafe-function declarations;
  private sibling items are neither source-visible to the root nor linkable from consumer cleanup.
- **Actual failure:** the resource declaration cannot resolve, or an imported resource Drop has no
  legal symbol.
- **Recommendation:** use a `pub fn(raw) -> ()` inside the package's allowed `internal` subtree with
  an unsafe body. The resource producer synthesizes a non-user-callable hidden support thunk and
  exports its symbol/ABI fingerprint in resource metadata.
- **v1 impact:** blocker and soundness requirement for every L3 resource.

### F22 — function values erased borrow/out parameter modes

- **Classification:** current Align conflict; ownership or soundness risk.
- **Problematic design location:** L2 function modes versus existing first-class named functions.
- **Current Align constraint:** current `Fn`/`FnTy` records argument types/return/effect but not
  `ParamMode`.
- **Actual failure:** binding `inspect` then indirectly calling it can select a by-value ABI, consume
  a Move owner, or miscompile a caller-storage pointer.
- **Recommendation:** define `Fn` with mode/type entries, effect, and both return-provenance
  summaries end to end. Exact mode equality and the direct-call ABI apply to bindings, joins,
  interfaces, indirect calls, and codegen; summary joins are detailed in F26.
- **v1 impact:** blocker for sound L2; must ship in the L2 prerequisite PR.

### F23 — `out: region` conflicts with out-parameter parsing

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** every L4/D10 destination-region signature.
- **Current Align constraint:** `out` is a contextual parameter-mode word.
- **Actual failure:** greedy mode parsing consumes the intended parameter name and fails at `:`.
- **Recommendation:** exact lookahead: `out ident :` is a mode plus name; `out :` is an ordinary
  name. Apply the same contextual rule to `borrow`.
- **v1 impact:** blocker for L4 syntax; resolved in the frontend contract.

### F24 — region-built arrays cannot cross the ordinary by-value helper boundary

- **Classification:** current Align conflict; ownership or soundness risk.
- **Problematic design location:** original `finish(state, groups.build())`.
- **Current Align constraint:** an arena-owned Move value cannot be passed by value to an ordinary
  function.
- **Actual failure:** the canonical compound-output example is ill-typed or would need an unsafe
  hidden transfer.
- **Recommendation:** validate Copy/view state before build, then consume the builder and construct
  Output/Option/Result inline in Query-local `run`. Do not add a DB-specific region-transfer call.
- **v1 impact:** blocker for the example, resolved without broadening L4.

### F25 — function-only Query IDs collide when one function has multiple constructors

- **Classification:** specification ambiguity; ownership or soundness risk.
- **Problematic design location:** static constructors as unrestricted ordinary calls versus Query
  ID = module + descriptor function.
- **Current Align constraint:** one function can otherwise contain nested, conditional, or repeated
  calls with the same item identity.
- **Actual failure:** SQL bytes, generated thunks, and artifact slots collide or become
  control-flow-dependent build inputs.
- **Recommendation:** accept a recognized constructor only as the entire single-expression body of
  one named zero-argument non-generic descriptor function. Reject every other placement.
- **v1 impact:** blocker for deterministic L5 artifacts; resolved in the revised descriptor rule.

### F26 — function values must retain return provenance, not only parameter modes

- **Classification:** ownership or soundness risk; prerequisite feature missing.
- **Problematic design location:** L2 borrow-returning named functions used as first-class values.
- **Current Align constraint:** a function value may join multiple named targets; written
  parameter/return types do not say which input backs a returned view.
- **Actual failure:** an indirect caller may release the true owner/region because its `Fn` fact
  retains modes but loses `ReturnBorrowSummary`/`ReturnRegionSummary`.
- **Recommendation:** concrete `Fn`/`FnTy` carries both summaries. Function-value joins union
  parameter-index sets; unresolved higher-order parameters use every compatible view/region input.
- **v1 impact:** soundness blocker for L2 and indirect calls; mandatory in the L2 PR.

### F27 — inline SQL needs a deterministic non-file source identity

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** `db.query(sql_literal, ...)` versus an artifact schema requiring
  an SQL logical file path.
- **Current Align constraint:** an inline literal belongs to the defining `.align` SourceMap file,
  not to a sibling SQL file.
- **Actual failure:** cache/artifact encoding and diagnostics either invent unstable paths or cannot
  identify the inline source.
- **Recommendation:** use tagged `File(logical_path) | Inline(query_id)` identity. Hash exact
  decoded literal bytes and retain a decoded-byte-to-`.align` literal span map.
- **v1 impact:** blocker for the promised inline L5 form; sibling-file Queries could ship without it,
  but the accepted unified design includes it in L5.

### F28 — migration-backed SQLite preparation had no deterministic order

- **Classification:** specification ambiguity; implementation complexity.
- **Problematic design location:** `alignc db prepare --memory --migrations <dir>`.
- **Current Align constraint:** filesystem enumeration order is not stable across hosts.
- **Actual failure:** dependent migration scripts may execute in different orders and produce
  different checked metadata/schema fingerprints.
- **Recommendation:** one nonrecursive canonical catalog: exact four-digit snake-case filenames,
  no symlinks/non-UTF-8, unique contiguous versions from `0001`, numeric order, exact UTF-8 bytes,
  and an ordered tuple fingerprint. Validate the whole catalog before execution.
- **v1 impact:** D3 blocker for migration-backed SQLite prepare; resolved in §§16.6/17.

### F29 — unresolved higher-order calls lost provenance embedded in Move inputs

- **Classification:** ownership or soundness risk; prerequisite feature missing.
- **Problematic design location:** L2's fail-closed return-provenance rule for an unresolved
  function target.
- **Current Align constraint:** a by-value Move input may itself contain views, a dependent
  resource, or another value carrying owner/region provenance.
- **Actual failure:** an indirect identity such as `fn(Child) -> Child` could omit the input's
  parent generation from its result summary, allowing the parent to be invalidated while the
  returned child remains live.
- **Recommendation:** recursively snapshot every compatible input's embedded owner/region roots
  before ordinary move/null, including by-value Move inputs, and attach their conservative union
  to the indirect result. Continue to reject a result that would expose a bare view of a dying
  owner.
- **v1 impact:** soundness blocker for L2 and function values.

### F30 — resource Drop-hook examples used an unresolvable relative path

- **Classification:** current Align conflict; specification ambiguity.
- **Problematic design location:** L3 resource declaration examples using `internal.drop_*`.
- **Current Align constraint:** sibling/cross-module names are absolute qualified paths; Align has
  no import alias or implicit relative `internal` namespace.
- **Actual failure:** the canonical example does not resolve unless the compiler adds a
  resource-specific name lookup exception.
- **Recommendation:** import the internal module normally and spell the hook as the fully qualified
  `pkg.db.internal.resource.drop_name`. The generated producer thunk records that resolved symbol;
  no special path rule is added.
- **v1 impact:** blocker for an implementable L3 declaration grammar; now resolved.

### F31 — PostgreSQL full-result delivery contradicted the cardinality cost promise

- **Classification:** performance risk; specification ambiguity.
- **Problematic design location:** `one`/`maybe_one` promised to read only enough rows for
  cardinality while the initial libpq design used full-result delivery.
- **Current Align constraint:** ordinary libpq buffered-result execution transports and stores the
  full server result before Align can inspect its first two rows.
- **Actual failure:** users could reasonably infer bounded network/memory work from a decoder-only
  two-row limit; a large cardinality error would still transport and buffer every row.
- **Recommendation:** state separately that `one`/`maybe_one` decode at most two delivered rows.
  Label each driver observation `Step`, `BufferedFull`, `SingleRow`, or `PortalBatch`, pin
  transported/buffered/decoded counts, and make the initial PostgreSQL path explicitly
  `BufferedFull`. D13 adds explicitly selected single-row/portal delivery; requested unsupported
  modes fail instead of downgrading.
- **v1 impact:** not a correctness blocker for the D4 baseline after disclosure; bounded PostgreSQL
  transport is a scheduled D13 capability and must not be falsely promised earlier.

### F32 — L1a and L1b both claimed `Option<MoveStruct>`

- **Classification:** specification ambiguity; implementation complexity.
- **Problematic design location:** L1a's exact first-PR acceptance versus L1b's Move tagged-payload
  scope.
- **Current Align constraint:** `Option<string>` as an owned field leaf is a narrower compiler step
  than admitting arbitrary Move structs as tagged payloads.
- **Actual failure:** an implementer could not tell whether `Option<MoveStruct>` must pass or remain
  a diagnostic in the first PR, making its acceptance and review boundary unstable.
- **Recommendation:** L1a establishes the recursive `DropPlan` framework but admits only
  `Option<string>` as the owned field leaf, including outer structs that contain such fields.
  `Option<MoveStruct>` remains an explicit L1b diagnostic. L1b alone admits Move struct/sum
  payloads in Option/Result/user sums.
- **v1 impact:** does not change the required L1a→L1b sequence; the exact split must be fixed before
  implementation starts.

### F33 — dependent resource and native-view semantics were absent from the MIR inventory

- **Classification:** prerequisite feature missing; ownership or soundness risk.
- **Problematic design location:** L3 `from_raw_borrowed` and checked owner-tied raw views versus
  `04-mir.md`'s generic operation list.
- **Current Align constraint:** ownership, dependencies, and unsafe-to-safe validation must be
  settled in typed HIR/MIR; LLVM is pure lowering.
- **Actual failure:** a generic raw cast or late codegen decision could lose the parent generation
  or omit size/null/alignment/UTF-8 checks.
- **Recommendation:** add explicit
  `ResourceFromRawBorrowed { resource_def, raw, parent_ref }` and
  `ResourceViewFromRaw { resource_def, owner_ref, ptr, len, view_kind, validation_plan }`
  operations. Tests inspect exact MIR and prove the dependency and complete checked validation plan
  survive separate compilation.
- **v1 impact:** soundness blocker for L3 and therefore for safe DB text/blob rows.

### F34 — `borrow mut` did not reject overlapping by-value provenance

- **Classification:** ownership or soundness risk; current Align conflict.
- **Problematic design location:** L2's call-site alias matrix.
- **Current Align constraint:** Copy views, `resource_ref`, Row, dependent resources, and aggregates
  may carry the same owner generation even when their parameter mode is ordinary by-value.
- **Actual failure:** a call could pass `borrow mut owner` beside an old by-value view of that owner.
  Call entry invalidates the old generation, so the callee receives a dangling peer argument.
- **Recommendation:** compare recursive provenance for every call argument. Reject any by-value
  Copy/Move value overlapping a `BorrowMut` owner generation, just as overlapping borrow/out
  arguments are rejected. Evaluation order does not repair the invalid delivery.
- **v1 impact:** L2 soundness blocker.

### F35 — result-mode signatures implied forbidden optionless overloads

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** `pkg-design/db.md` §6.2 versus §13.2.
- **Current Align constraint:** Align has no default arguments, and the settled DB surface requires
  exactly one option-bearing primitive form with `[]` for no options.
- **Actual failure:** implementers could create optionless overloads from the normative list or make
  examples with explicit `[]` disagree with the API.
- **Recommendation:** include `slice<db.ExecuteOption>` on execute/result operations and
  `slice<db.PrepareOption>` on prepare in every normative signature.
- **v1 impact:** API blocker for D2/D4/D9; now resolved.

### F36 — canonical mutable-borrow examples used immutable bindings

- **Classification:** current Align conflict; implementation complexity.
- **Problematic design location:** Query-local rows loop and prepared-statement examples.
- **Current Align constraint:** a `borrow mut` parameter accepts only a writable bound place; call
  syntax itself has no parameter-mode marker.
- **Actual failure:** `rows := ...; db.next(rows)` and `stmt := ...; db.rows_stmt(stmt,...)` fail
  checking once L2 is implemented.
- **Recommendation:** bind `rows` and reusable `stmt` with `mut`; keep ordinary call syntax.
- **v1 impact:** example/API conformance blocker for D6/D8; now resolved.

### F37 — Japanese prepared-statement example put a mode at the call site

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** Japanese `pkg-design/db.md` §6.4.
- **Current Align constraint:** `borrow mut` appears only in a parameter declaration; calls remain
  unchanged.
- **Actual failure:** the mirror's canonical `db.rows_stmt(borrow mut stmt, ...)` does not parse and
  disagrees with the authoritative English contract.
- **Recommendation:** use `mut stmt := ...` followed by `db.rows_stmt(stmt, ...)`, and keep the
  English/Japanese mirrors synchronized.
- **v1 impact:** documentation blocker, not a new implementation feature.

### F38 — the initial release gate excluded surfaces promised as initial

- **Classification:** specification ambiguity.
- **Problematic design location:** initial release gate D1–D10 versus SQLite migration and
  PostgreSQL/basic metadata-plan promises assigned to D11/D12.
- **Current Align constraint:** a milestone must have one testable boundary; a capability cannot be
  both required for that release and scheduled after its gate.
- **Actual failure:** one implementation agent could release at D10 while another correctly waits
  for migrations, category metadata, and explicit Query-plan access.
- **Recommendation:** retain the small D2/D4 architecture verticals, but define the first public
  database release as L1a–L6 plus driver-relevant D1–D12. D13/D14 remain committed additive work.
- **v1 impact:** D11/D12 are required for the first public DB release; they do not block early
  vertical development.

## 3. Answers to the requested feasibility checks

1. **Language compatibility:** the original proposal conflicts at Move payloads, borrowed Move/Copy
   calls, function-value modes, imported return provenance, dependent native handles/linkable Drop,
   contextual parameter parsing, named regions, current generics, and static non-Align inputs.
   L1a–L6 are the required general repairs. Module/FFI/package rules are compatible after the
   revised layering.
2. **Package boundary:** `pkg.db`, `.sqlite`, and `.postgres` are appropriate public names only as
   three modules of one vendorable `pkg/db` subtree. They are not three independent versions.
3. **Ordinary package code:** public data shapes, options/errors/metadata, safe FFI wrappers,
   connection/transaction/statement/rows lifecycle, driver operations, Query-local shaping, and
   migration orchestration.
4. **Runtime support:** generic checked owner-tied raw views, UTF-8 validation, and region-builder
   chunk/compact helpers. No DB-specific runtime Query engine or handle type.
5. **Compiler/semantic support:** L1a–L6, recognized Query constructors, static-input tracking,
   Query contract checking, placeholder scan/source maps, artifacts/hashes, and generated
   binder/decoder thunks.
6. **Static `db.query<Params,Row>`:** a Copy immutable descriptor data record plus direct generated
   binder/decoder functions. Its function body is exactly one constructor, giving the item one
   unique artifact identity. It is not an object with reflection or a runtime SQL parser.
7. **Sibling `.align`/`.sql`:** a path-free registered constructor maps the defining module's exact
   extension to `.sql`; exact source bytes, logical path, kind, and digest are deterministic inputs,
   separate from deterministic driver wire bytes. Inline SQL instead uses `Inline(query_id)` and a
   decoded-literal source map.
8. **Module export:** `IStaticQuery` carries the public contract; `StaticQueryArtifact` carries SQL
   and implementation metadata. A private SQL-only edit rebuilds/relinks the producer without
   invalidating unchanged consumers. Function values similarly retain/join return provenance across
   separate compilation.
9. **Named parameters:** dialect-aware lexical occurrence table. SQLite uses native named
   parameter indices; PostgreSQL rewrites unique names to stable `$n` positions and reuses an
   ordinal for repeats.
10. **Typed Row decode:** monomorphized ordinal decoder thunk with direct field writes and runtime
    contract guards; no reflection or per-row name lookup. V1 static Row is `RegionPlain`; owned
    strings/arrays remain Params/Output forms rather than a hidden alternate Row materializer.
11. **Row view lifetime:** resource generations. `next(borrow mut rows)` invalidates the previous
    generation; `clone_in(out)` is required to retain data.
12. **Conn/tx execution:** two concrete constructors produce one `db.exec` Copy sum of resource
    references. No public trait hierarchy.
13. **One-pass shaping:** Query-local visible rows loop plus Pure exclusive-state `step`; the step
    has no DB handle, and transitive effects reject hidden I/O. The orchestrator constructs its
    arena-owned Output inline after builder finalization.
14. **Compound Output builders:** L6 region-backed `RegionPlain` builder with chunk growth and one
    measured compacting pass. Builders stay separate mutable locals and are borrowed by the Pure
    step.
15. **Offline checked metadata:** explicit prepare/check tooling invokes real database engines;
    ordinary build consumes canonical artifacts and never connects. Explicit SQLite migration
    replay uses one canonical validated version order and ordered fingerprint.
16. **Native options:** distinct typed finite sums at all seven scopes, with separate common/native
    slices and no silent ignore.
17. **Roadmap:** move all language work before drivers; split Move work into L1a/L1b; prove fake
    Query artifacts before native code; put SQLite and PostgreSQL scalar verticals before
    streaming/transactions/compound output; complete migrations and category metadata/EXPLAIN in
    D11/D12 before the first public release; schedule D13/D14 as additive native/dynamic work.
18. **Minimum SQLite vertical:** in-memory connection, one scalar command, one sibling-file scalar
    Query, `execute`/`one`, cardinality/error/cleanup/execution-count tests, no text views.
19. **Minimum PostgreSQL vertical:** same common Query module, named-to-positional rewrite, scalar
    bind/decode, SQLSTATE error, driver restriction, explicit configured ephemeral/local server.
20. **Small PR order:** L1a, L1b, L2, L3, L4, L5, L6, D0, D1, D2, D3, D4, D5, D6, D7, D8, D9,
    D10, D11, D12, then D13–D14. Each owns only the tests and benchmark rail listed below.

## 4. Required specification revisions

The reviewed specification is acceptable only with these revisions, now incorporated in the design
documents:

1. Make L1a–L6 mandatory before a safe driver.
2. Replace builtin-handle enumeration with general package-defined/dependent resources.
3. Add contextual borrowed parameters, mutable Copy-state update, function-value parameter modes,
   imported return provenance, and the exclusive-input purity rule.
4. Add named region capabilities and a region-backed plain-struct builder.
5. Define exact static-input resolution, SourceMap behavior, hashes, and cache boundaries.
6. Split Query public interface from producer implementation artifact.
7. Generate direct binder/decoder thunks; prohibit runtime reflection.
8. Replace generic `db.fold` with the visible Query-local loop plus Pure step.
9. Define row-buffer generation invalidation and explicit retention through `clone_in`.
10. Treat the three requested package paths as modules in one subtree.
11. Use a closed `db.exec` resource-reference sum instead of a public trait.
12. Define a dialect-aware named-parameter scanner and stable ordinal policy.
13. Make option representation/scopes/disposition exact.
14. Define explicit offline prepare/check modes and canonical metadata invalidation.
15. Split minimal SQLite/PostgreSQL verticals from broad metadata/native/dynamic work.
16. Make resource Drop source hooks valid `pub` internal functions with unsafe bodies and generated
    producer-owned linkable thunks.
17. Parse `borrow`/`out`/`resource` contextually, including `out: region`.
18. Restrict each static constructor to one whole-body descriptor item with a unique Query ID.
19. Keep region builders as separate locals and construct arena-owned compound output inline.
20. Separate exact source SQL identity from deterministic per-driver wire SQL identity.
21. Carry and join return-borrow/region summaries in function values and indirect calls.
22. Give inline SQL a tagged descriptor-item identity and decoded-literal diagnostic map.
23. Define one validated numeric migration order/fingerprint for SQLite prepare and D11 migrate.
24. Preserve owner/region provenance embedded in compatible by-value Move inputs across unresolved
    higher-order calls.
25. Use only normally resolved fully qualified resource Drop-hook paths.
26. Separate decoder cardinality limits from driver transport/buffering and label every delivery
    mode.
27. Assign `Option<MoveStruct>` exclusively to L1b while keeping L1a limited to
    `Option<string>` field leaves.
28. Represent dependent construction and checked owner-tied raw views explicitly in MIR.
29. Reject recursive by-value provenance aliases beside a `borrow mut` argument.
30. Put the mandatory option slice in every normative execute/result/prepare signature.
31. Bind every canonical mutable-borrow owner as `mut` while keeping call syntax mode-free.
32. Keep the Japanese prepared-statement example identical in semantics to the English original.
33. Make D11 SQL migrations and D12 category metadata/EXPLAIN part of the first public release
    gate; retain D13/D14 as committed additive work.

## 5. Revised implementation roadmap

| PR | Scope | Required focused tests | Benchmark/evidence |
|---|---|---|---|
| L1a | Recursive DropPlan framework; `Option<string>` fields | `owned_tagged_payloads`, analysis coverage | tagged construct/pass/drop |
| L1b | Move sum/Option/Result completion | `?`/`else`/`match`/join cleanup | no-allocation `Ok`, error cleanup |
| L2 | contextual borrow modes, Copy mutation, Fn modes/provenance | recursive alias matrix, joined-target direct/indirect, per-unit parity | borrowed-call and interface-size cost |
| L3 | resource/ref, linkable Drop thunk, dependent child/native view | exact MIR, cross-unit Drop, invalid pointer/escape | resource/ref/view overhead and IR |
| L4 | named arena `region`, `clone_in` | all escape paths and module propagation | named versus anonymous arena |
| L5 | tagged file/inline inputs, Query artifacts, descriptor skeleton | cache/path/inline-span/reproducibility matrix | cold/warm producer/consumer rebuild |
| L6 | region `RegionPlain` builder | copy count, no heap, current-row rejection | push/freeze throughput and bytes |
| D0 | SQLite/libpq capability probes only | native lifecycle/metadata observations | recorded driver evidence |
| D1 | fake-driver Query binder/decoder and scanner | artifact/cache/placeholder matrix | thunk overhead and warm cache |
| D2 | scalar SQLite Query vertical | cardinality, cleanup, execution count | package versus libsqlite3 |
| D3 | SQLite prepare/check metadata | stale/policy/offline plus migration catalog/order matrix | prepare/check time and artifact size |
| D4 | scalar PostgreSQL Query vertical (`BufferedFull`) | rewrite, SQLSTATE, mismatch, cleanup, delivery counts | package versus libpq |
| D5 | PostgreSQL checked metadata | recreated-schema reproducibility | describe/prepare time |
| D6 | dependent prepared statements | sequential reuse and child-before-parent Drop | prepare reuse/reprepare |
| D7 | tx plus common exec view | consume/commit/rollback/fail-safe Drop | tx/common-dispatch overhead |
| D8 | typed rows and row generations | old-view rejection, clone retention, physical delivery counts | row decode/iteration |
| D9 | all option scopes, timeout, cancellation | applied/unsupported/conflict matrix | option/cancellation overhead |
| D10 | one-pass compound Output | many-to-one/one-to-many, exactly one SQL | shaping allocation/copy/throughput |
| D11 | SQL migrations; initial-release gate | checksum/order/transaction/status | migration startup/large history |
| D12 | category metadata and EXPLAIN; initial-release gate | category isolation; ANALYZE executes visibly | catalog query count/latency |
| D13 | batch/SoA/native paths/pool | generation, native lifecycle, exact semantics | driver-specific throughput rails |
| D14 | dynamic rows and proved callback surfaces | allocation/lifetime/reentrancy/cleanup | dynamic decode/callback overhead |

## 6. Exact first implementation PR

The first implementation PR is **L1a only**. It must not contain database, resource, borrow, region,
or static-input code.

Scope:

- add one canonical recursive owned-value `DropPlan` after type definitions resolve;
- allow `Option<string>` as a struct field and classify its enclosing struct as Move;
- emit tag-tested Drop on normal return, early return, and reassignment;
- move/null the live payload on supported whole-value moves;
- retain explicit diagnostics for `Option<MoveStruct>` (owned by L1b), recursive types, unsupported
  deep partial moves, and arbitrary Move collection elements.

Planned files:

```text
crates/align_sema/src/lib.rs
crates/align_mir/src/lib.rs
crates/align_codegen_llvm/src/lib.rs
crates/align_driver/tests/owned_tagged_payloads.rs
crates/align_driver/tests/analysis_coverage.rs
bench/owned_tagged_payload/README.md
bench/owned_tagged_payload/.gitignore
bench/owned_tagged_payload/Cargo.lock
bench/owned_tagged_payload/Cargo.toml
bench/owned_tagged_payload/build.rs
bench/owned_tagged_payload/kernel.align
bench/owned_tagged_payload/run.sh
bench/owned_tagged_payload/src/main.rs
docs/impl/07-roadmap.md
docs/impl/17-library-boundary-prerequisites.md
HANDOFF.md
```

Acceptance commands:

```text
cargo test -p align_driver --test owned_tagged_payloads
cargo test -p align_driver --test analysis_coverage
scripts/test-pr.sh
cargo clippy --workspace --all-targets -- -D warnings
bench/owned_tagged_payload/run.sh
```

Acceptance behavior:

- `None` never reads or drops uninitialized payload storage;
- `Some` drops exactly once across return, `?`, branches, loops, and reassignment;
- `Some(old) -> Some(new)` and `Some -> None` drop old before replacement;
- `None -> Some` does not run a spurious Drop;
- nested outer Move structs containing `Option<string>` fields recurse through the same plan;
- `Option<MoveStruct>` remains a clean compile error naming L1b;
- malformed/unsupported types produce diagnostics rather than compiler panic;
- generated LLVM has a tag guard and introduces no allocation on `None`.

## 7. Required benchmark set

The delivery is not performance-complete without:

- L1a/L1b tagged-Move branch, allocation, and Drop counts;
- L2 direct/indirect Move borrow and Copy-state mutable borrow versus current builtin receiver,
  including recursive alias-check cost plus joined return-summary/interface size/time;
- L3 resource construction/ref/dependent cross-unit Drop thunk/native-view checks versus direct
  current handle path, with exact MIR-operation counts;
- L4 named/anonymous arena parity;
- L5 cold/warm rebuild matrix for unchanged, source-SQL-only, wire-rewrite, private, and public
  contract changes, plus file/inline and descriptor-count scaling;
- L6 exact heap bytes, region bytes, push throughput, and one compacting pass;
- D1 generated binder/decoder versus hand-written field/ordinal code;
- D2 direct libsqlite3 comparison;
- D3 prepare/check artifact time/size and canonical migration catalog/replay scaling at 10/100/1000
  files;
- D4 direct libpq comparison with transported/buffered/decoded rows reported separately;
- D6 prepare reuse versus reprepare;
- D8 row iteration/decode, physical-delivery counts, and retained-copy costs;
- D10 one-pass one-to-many shaping, allocation/copy count, and exact one SQL execution;
- D11 migration startup/status cost and ordered history scaling at 10/100/1000 applied files;
- D12 category metadata query count and EXPLAIN latency;
- D13 batch/SoA/native throughput on each driver.

Benchmark results are evidence and regression anchors. They may not justify removing ownership,
runtime contract validation, explicit options, or one-statement semantics.
