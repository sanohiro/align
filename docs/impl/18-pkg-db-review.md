# Query-centric database design review

## Status

**REVIEW OF RECORD — 2026-07-27; findings incorporated.**

This file preserves the independent design review and its F1–F95 finding
register. It is not the live implementation sequence. Current status is in
`HANDOFF.md`; current prerequisite dependencies are in
`17-library-boundary-prerequisites.md`; the complete product roadmap is in
`pkg-design/db.md` §23.

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

**Original verdict: REVISE. Current disposition: incorporated.**

The SQL-native, Query-centric direction is correct and should be retained. The original package
proposal was not implementable as ordinary Align code without either unsound native views,
database-named compiler exceptions, runtime reflection, or APIs that conflict with current
generics/package rules. The revised design resolves the API and compilation questions and records
the missing general language facilities in
[`17-library-boundary-prerequisites.md`](17-library-boundary-prerequisites.md).

Safe database driver implementation remains blocked until the complete L1a–L7
gate. L1a and L1b are complete; `HANDOFF.md` owns the exact current L2 status.
The remaining prerequisites follow a capability DAG rather than this review's
original one-label-per-PR sequence. This is not a recommendation to defer the
ideal design: L1a–L7 remain mandatory scheduled work and part of the database
delivery plan.

`pkg.db` has no dependency on `std.http`. Both should eventually use the same package-defined
resource, borrow-provenance, and owner-tied native-view machinery. Sharing that language foundation
is desirable; importing the HTTP package or extracting an HTTP-flavored database abstraction is not.

### 1.1 Public-contract ledger

This ledger is the author-side completion gate for the design. A surface is ready for independent
review only when its exact contract, implementation owner, acceptance/local-measurement owner, and every
listed source agree.

| Surface | Exact invariant | Owner | Verification | Sources that must agree |
|---|---|---|---|---|
| Recursive aggregates | A finite resolved field is Copy or recursively Move with one ordinary Drop plan | L1a/L1b | tagged construct/move/drop/error cleanup and cost | draft, language spec, prerequisite plan |
| Borrowed calls | `borrow`/`borrow mut` modes, all-peer alias rejection, return roots, and cleanup ABI are interface facts | L2 | direct/indirect/imported alias/provenance/cleanup matrix | draft, language spec, types/MIR plans, prerequisite plan |
| Opaque resources | public safe `resource.borrow`; raw forms are declaring-subtree privileged; dependent child and view provenance is explicit | L3 | cross-unit Drop, escape/raw-transfer negatives, resource/view overhead | draft, language spec, frontend/types/MIR, prerequisite plan |
| Region and builder | explicit `region`; closed `RegionPlain`; one measured builder compact pass | L4/L6/L7 | escape/bound/copy-count tests and push/freeze benchmark | draft, language spec, types/MIR, core builder, prerequisite plan |
| Query/command descriptors | one whole-body item, exact `Params`/flat `Row`, unique identity, structural reachable-definition fingerprints, canonical top-level/nested artifact codec, binder/decoder ABI versions, and producer-owned QueryMeta plan; no reflection | L5/D1 | descriptor/interface/cache/runtime-metadata-plan matrix and checked-in Query/command byte+digest goldens | frontend/types/MIR, prerequisite plan, DB EN/JA |
| Static SQL inputs | tagged sibling/relative/inline identity; source and driver-wire hashes/spans are distinct deterministic inputs | L5/D1 | create/change/delete/path/span/incremental matrix | language docs, pipeline/cache plans, DB EN/JA |
| Parameters | one dialect-aware source scan; stable first-occurrence ordinals; SQLite named and PostgreSQL `$n`; explicit retention/copy | D1/D4/D8 | scanner/rewrite/bind-lifetime matrix and bind benchmark | DB EN/JA |
| Typed rows | generated ordinal decoder; runtime count/type/NULL guard; row generation invalidates views; retention uses `clone_in` | D1/D8 | stale-view/decode/retention negatives and row-loop benchmark | memory model, DB EN/JA |
| Connection/transaction execution | closed `db.exec` resource-reference sum from both conn and tx; no public driver trait | D7 | alias/consume/rollback/dispatch tests and dispatch cost | prerequisite plan, DB EN/JA |
| Shaping | one visible Query execution and rows loop; Pure one-pass step; no structural extra SQL | D10 | many-to-one/one-to-many execution-count tests and shaping benchmark | DB EN/JA, core builder |
| Offline metadata artifacts | explicit prepare only; exact derived pathname and canonical fail-closed JSON/identity codecs with independent goldens; per-driver Missing/Present identity; normal build has no DB/network access | L5/D3/D5 | stale/malformed/reproducible/offline/cache/byte-golden matrix and artifact time/size | pipeline/cache plans, prerequisite plan, DB EN/JA |
| Options/errors/result | finite scope-specific sums; unsupported is error; connection-global state has one explicit lease; owned structured error; `exec_result` is Copy `{ rows_affected: Option<i64> }` | D1/D2/D4/D6/D7/D9/D12 | disposition/overlap/Drop-order/error-buffer tests and zero-allocation result check | DB EN/JA |
| Migrations | exact entry/catalog/driver/target CLI; versioned catalog/schema-identity codecs and independent goldens; atomic default; one-statement dirty exceptional path | D11 | CLI-input/byte-golden/checksum/crash/repair/status matrix and history scaling | roadmap, DB EN/JA |
| Metadata records | exact signature notation plus parseable positional calls, typed refs, pre-native identifier validation/precedence, detail/state/discriminator projection, ordinals/digest, duplicate-key identity, and flat Column/Key/Index/Query fields; explicit region; no native-buffer borrow; QueryMeta materializer ABI/code and descriptor-header version land with their first consumer | D12 | signature-table/syntax/input/detail/state/entry/field/identity/flatness/lifetime/category/query-count/cross-unit-materializer matrix and catalog/thunk benchmark | DB EN/JA, prerequisite plan |
| Nullability/origin | engine-reported query evidence only; ambiguous is `Unknown`; D0 evidence and D3/D5 support matrices precede checked metadata | D0/D3/D5 | outer-join/expression/catalog/runtime-NULL matrix | roadmap, DB EN/JA |
| Delivery dependencies | L1a–L7 prerequisite DAG, D0 parallel evidence, D1–D12 initial-release gate, D13–D14 complete committed roadmap; no consumer precedes its prerequisite | all | capability owner gates in §5; local measurements in §7 | roadmap, HANDOFF, prerequisite plan, DB EN/JA |

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
- **v1 impact:** blocker for sound L2; must ship in the L2 sequence before borrow syntax is exposed.

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
  compatible parameter-index sets and preserve target-relative capture roots as completed by F56;
  unresolved higher-order parameters use every compatible view/region input.
- **v1 impact:** soundness blocker for L2 and indirect calls; mandatory before the first borrowed
  function value is exposed.

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
- **Current Align constraint:** a by-value input may itself contain views or another value carrying
  owner/region provenance. L3 later adds dependent resources to that same class.
- **Actual failure:** an indirect identity over an existing recursively view-bearing aggregate
  could omit the input owner from its result summary, allowing the owner to be invalidated while
  the result remains live. After L3, `fn(Child) -> Child` has the same failure mode for its parent
  generation.
- **Recommendation:** recursively snapshot every compatible input's embedded owner/region roots
  before ordinary move/null, including by-value Move inputs, and attach their conservative union
  to the indirect result. Continue to reject a result that would expose a bare view of a dying
  owner.
- **v1 impact:** L2b soundness blocker for existing function values; L3 must extend the completed
  walker and tests to dependent children.

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
  database release as L1a–L7 plus driver-relevant D1–D12. D13/D14 remain committed additive work.
- **v1 impact:** D11/D12 are required for the first public DB release; they do not block early
  vertical development.

### F39 — streaming execution did not own or borrow parameter storage

- **Classification:** ownership or soundness risk; performance risk.
- **Problematic design location:** `rows`/`rows_stmt` with SQLite text/blob Params.
- **Current Align constraint:** `str` and slices are borrowed views, while SQLite may retain a bound
  pointer across the API return and perform `sqlite3_step` only on later `next`.
- **Actual failure:** the caller could drop or mutate parameter storage after `rows` returns,
  leaving SQLite with a dangling pointer.
- **Recommendation:** the common contract releases all Params provenance at operation return. V1
  SQLite uses `SQLITE_TRANSIENT`-equivalent bind copies; prepared statements keep the native copy
  until reset/rebind/finalize. Async/future PostgreSQL paths retain their own bytes. Pin source
  invalidation before first `next`, partial-bind cleanup, and copied-byte/allocation counts.
  Zero-copy bind requires a separate explicit driver-qualified contract and return provenance.
- **v1 impact:** soundness blocker for D8 text/blob streaming and D6 prepared text/blob execution.

### F40 — dynamic SQL did not expose its driver restriction

- **Classification:** specification ambiguity; Align philosophy conflict.
- **Problematic design location:** D14's concrete `db.dynamic_rows` example.
- **Current Align constraint:** dialect choice and native dependency must remain visible; dynamic SQL
  has no static artifact/preparer that can infer portability.
- **Actual failure:** relying on the runtime `exec` driver hides the restriction and delays a
  dialect mismatch until SQL execution.
- **Recommendation:** require an exact source-visible `db.Driver` argument with no `Any` value,
  compare it before sending SQL, and retain explicit parameter/execute-option slices. Native forms
  add a separate native option slice.
- **v1 impact:** D14 API blocker; not part of the D1–D12 first release.

### F41 — verified core signature tables included unimplemented L4/L6 forms

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** `core-design/arena-heap.md` and
  `core-design/array-slice-pipeline.md` “Signatures (verified)” tables.
- **Current Align constraint:** `arena out {}` and the region builder are settled prerequisites but
  not implemented before L4/L6.
- **Actual failure:** an implementation agent could treat unavailable APIs as shipped, and current
  example verification cannot prove those lines.
- **Recommendation:** keep only shipped forms in verified tables and place L4/L6 signatures in
  clearly marked “required, not implemented yet” blocks in both language mirrors.
- **v1 impact:** status/documentation blocker before prerequisite implementation starts.

### F42 — category metadata calls omitted the mandatory option slice

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** §18.2 category calls versus §13.2's one option-bearing form.
- **Current Align constraint:** Align has no default arguments; `[]` is the explicit no-option value.
- **Actual failure:** D12 could implement optionless public overloads or leave unclear whether
  category calls are wrappers around another primitive.
- **Recommendation:** every `meta_*` category primitive ends in `slice<db.MetaOption>`. Native forms
  take that common slice plus a separate native option slice. Show `[]` in every conceptual call.
- **v1 impact:** D12 API blocker and therefore first-public-release blocker.

### F43 — metadata results had no destination ownership

- **Classification:** specification ambiguity; ownership or soundness risk.
- **Problematic design location:** `pkg-design/db.md` §18 category calls and result descriptions.
- **Current Align constraint:** strings/slices are views, dynamic arrays are Move, and allocation
  destination cannot be chosen implicitly; L4/L6 exist specifically to make result ownership
  explicit.
- **Actual failure:** a driver could return dangling catalog-buffer views or silently heap-allocate
  strings/nested collections. Separate implementations would invent incompatible result shapes.
- **Recommendation:** fix flat `RegionPlain` metadata/plan records, add `out: region` to every
  category and EXPLAIN operation, copy strings before native cleanup, represent multi-term
  keys/indexes as repeated flat rows, and make `meta_table` use `NotFound`.
- **v1 impact:** ownership/API blocker for D12 and therefore the first public release.

### F44 — checked policy was not defined per permitted driver

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** `pkg-design/db.md` §16.1/§16.4 `AnySupportedDriver` plus
  `CheckedRequired`.
- **Current Align constraint:** a descriptor has an exact closed driver restriction and normal build
  must decide from deterministic per-driver artifacts without contacting either database.
- **Actual failure:** with only SQLite metadata, one implementation could accept a portable
  CheckedRequired Query while another rejects it; the former could execute unverified PostgreSQL.
- **Recommendation:** store `DriverVerification` independently for every permitted driver.
  CheckedRequired requires all entries; CheckedOptional may be mixed and inspection must report the
  map. Pinning is the explicit way to reduce the set.
- **v1 impact:** D3/D5 artifact-policy blocker and first-release blocker.

### F45 — command artifacts were not executable at the D2 boundary

- **Classification:** prerequisite feature gap; implementation complexity.
- **Problematic design location:** L5/D1 Query-only detail versus D2's required
  `db.command<Params>` insert.
- **Current Align constraint:** command static input, binder thunk, interface, object symbol, and
  cache identity need the same producer-owned machinery as Query; reflection fallback is forbidden.
- **Actual failure:** D2 would have to invent command IDs, bind retention, checked metadata, hashes,
  and invalidation behavior.
- **Recommendation:** specify `StaticCommandArtifact`, `IStaticCommand`, and `CommandStatic` as the
  shared statement envelope minus Row/result/decode; make D1 generate and test it before D2.
- **v1 impact:** L5/D1/D2 implementation blocker.

### F46 — required native option variants were left to implementers

- **Classification:** specification ambiguity; unnecessary broad scope.
- **Problematic design location:** `pkg-design/db.md` §§11–13 described option scopes but called the
  variants examples or future consumer choices.
- **Current Align constraint:** public sum types are closed finite APIs; unknown option tags,
  reflection bags, and silent ignore are unavailable and contrary to One Way.
- **Actual failure:** another model cannot implement the public API or the required
  applied/unsupported/conflicting matrix without inventing variants and defaults.
- **Recommendation:** settle the minimum common/SQLite/PostgreSQL constructors, defaults,
  applicability, duplicate/conflict rules, positive-duration rule, and unsupported behavior.
- **v1 impact:** API blocker across D1–D12.

### F47 — option ownership was scheduled after its consumers

- **Classification:** implementation complexity; specification ambiguity.
- **Problematic design location:** D6 prepare and D7 transactions required options while D9 claimed
  ownership of all option scopes.
- **Current Align constraint:** one PR cannot call an undeclared future public sum, and pre-release
  code must not introduce a disposable compatibility surface.
- **Actual failure:** D6/D7 are impossible in order or must create preliminary APIs that D9 later
  replaces.
- **Recommendation:** assign static options to D1, SQLite/PostgreSQL connection/execution variants to
  D2/D4, prepare to D6, transaction to D7, and metadata/EXPLAIN to D12. D9 completes common
  deadlines/cancellation and the cross-scope disposition audit.
- **v1 impact:** roadmap blocker, not a reason to postpone the final option design.

### F48 — migration transaction and partial-failure behavior was unspecified

- **Classification:** specification ambiguity; ownership or soundness risk.
- **Problematic design location:** `pkg-design/db.md` §17.4 and D11.
- **Current Align constraint:** migrations are visible SQL and failures are explicit Results; a tool
  may not silently retry outside a transaction or guess that partially applied native state is safe.
- **Actual failure:** transaction-forbidden statements could be partially applied, unrecorded, or
  automatically retried under incompatible policies.
- **Recommendation:** use an exact first-line SQL comment directive, required-by-default atomic
  execution/history, a one-statement forbidden mode with Applying/Failed dirty rows, blocked
  continuation, and checksum-bound explicit repair.
- **v1 impact:** D11 and first-release safety blocker.

### F49 — the one-parent shaper accepted one partial-NULL direction

- **Classification:** specification ambiguity; ownership or soundness risk.
- **Problematic design location:** `pkg-design/db.md` §7.2 returned success immediately when
  `group_id` was NULL without checking `group_name`.
- **Current Align constraint:** the Row contract uses independent `Option` fields and no implicit
  aggregate-null invariant exists.
- **Actual failure:** `(group_id = NULL, group_name = non-NULL)` was silently discarded while the
  reverse shape correctly failed, contradicting the all-NULL absence rule.
- **Recommendation:** when ID is absent, require name to be absent before returning no child; reject
  either partial shape with the same contract error.
- **v1 impact:** D10 correctness blocker; no new language feature.

### F50 — the Japanese mirror exposed future `next_batch`

- **Classification:** specification ambiguity.
- **Problematic design location:** Japanese `pkg-design/db.md` §6.2 listed `next_batch` as a current
  result mode while English and the roadmap assign it to D13.
- **Current Align constraint:** English is authoritative, but the mirror must not publish a second
  executable API.
- **Actual failure:** an implementation consumer could depend on a batch surface absent from the
  D1–D12 release.
- **Recommendation:** remove it from the initial operation list and explicitly label D13 as its only
  owner.
- **v1 impact:** documentation/release-contract blocker.

### F51 — the Japanese many-parent shaper contradicted the segmented design

- **Classification:** current Align conflict; performance risk.
- **Problematic design location:** Japanese `pkg-design/db.md` §7.3 instructed pushing a completed
  per-parent Output into an outer builder.
- **Current Align constraint:** a region builder accepts `RegionPlain`; dynamic `array` fields are not
  RegionPlain and an arena array cannot cross an ordinary by-value call.
- **Actual failure:** the mirror either fails type checking or creates one child allocation per
  parent, defeating the required segmented representation.
- **Recommendation:** build parallel parent, child, and offset arrays with independent region
  builders; do not push array-bearing parent Outputs.
- **v1 impact:** D10 soundness/performance/documentation blocker.

### F52 — PostgreSQL could remain untested at the release gate

- **Classification:** specification ambiguity; performance risk.
- **Problematic design location:** D4 allowed PostgreSQL integration to skip whenever configuration
  was unavailable.
- **Current Align constraint:** the first release promises both drivers and requires measured native
  behavior; an optional local environment cannot establish that gate.
- **Actual failure:** all ordinary commands could report success while libpq bind/decode/error/
  cleanup paths never ran.
- **Recommendation:** retain reported local skips, but require a provisioned `db-postgres` CI job
  with a required-mode environment flag for D4 merge and every database release; skip becomes
  failure there.
- **v1 impact:** D4 merge and first-release verification blocker.

### F53 — ordinary generic package APIs were not representable

- **Classification:** current Align conflict; prerequisite feature gap.
- **Problematic design location:** common `rows_stmt<P,R>`/`all<P,R>` APIs and the L1a–L7 release
  gate.
- **Current Align constraint:** the shipped generic checker rejects `array<T>`/`slice<T>` and a
  generic `stmt<P,R>`/`rows<R>` application inside a generic function.
- **Actual failure:** the compiler could generate static descriptors, but ordinary `pkg.db` code
  could not define the common typed execution helpers without DB-specific builtin functions.
- **Recommendation:** add mandatory L7 nested symbolic generic applications plus the closed
  structural `RegionPlain` builtin bound, monomorphized before ownership/escape/MIR with no runtime
  dictionaries or user traits.
- **v1 impact:** blocks every safe common typed driver vertical.

### F54 — Move-return ABI omitted the dynamic cleanup bit

- **Classification:** ownership or soundness risk; current Align conflict.
- **Problematic design location:** L2 return summaries and helpers returning
  `Result<array<R>, db.Error>`.
- **Current Align constraint:** ownership mode is a path-local runtime bit independent of the joined
  region; a tagged return may select arena-owned `Ok` or individually owned `Err`.
- **Actual failure:** an imported/direct/indirect caller could leak the error or free arena storage
  if it inferred cleanup from static return-region provenance.
- **Recommendation:** every recursively Move return ABI carries the selected cleanup bit beside the
  value; `IFnSig`/`FnTy`/ABI fingerprints record its presence and callers store it.
- **v1 impact:** soundness blocker for L1/L2 and all compound Query results.

### F55 — mutable-borrow alias checking covered only selected peer modes

- **Classification:** ownership or soundness risk.
- **Problematic design location:** L2 call-site exclusivity wording.
- **Current Align constraint:** generation invalidation happens at call entry and cannot leave any
  already-evaluated peer argument dangling.
- **Actual failure:** overlap hidden in another `BorrowMut`, `Out`, or distinct aggregate holder
  could bypass a by-value-only recursive scan.
- **Recommendation:** for every `BorrowMut`, recursively scan every peer mode
  (`ByValue`/`Borrow`/`BorrowMut`/`Out`) for direct place or embedded provenance overlap.
- **v1 impact:** L2 soundness blocker.

### F56 — capturing closure return provenance had no capture roots

- **Classification:** ownership or soundness risk; prerequisite feature gap.
- **Problematic design location:** `ReturnBorrowSummary`/`ReturnRegionSummary` parameter-only model.
- **Current Align constraint:** a zero-argument closure may return a captured `str` or slice;
  parameter indices cannot name its environment owner. L3 adds `resource_ref`, and L4 adds
  explicitly region-owned values, to the same capture-root model.
- **Actual failure:** an indirect result could outlive the closure environment or captured owner,
  especially after a function-value join or move.
- **Recommendation:** concrete closure targets carry sorted target-relative capture-slot roots;
  indirect calls resolve them through the selected environment, and roots travel with moved
  function values. Named interfaces export only resolved parameter roots.
- **v1 impact:** L2b soundness blocker for existing closures; L3 and L4 must extend the completed
  capture-root engine and tests when their new types land.

### F57 — mutable-borrow replacement wording suppressed required Drop

- **Classification:** ownership or soundness risk; specification ambiguity.
- **Problematic design location:** MIR statement that borrowed pointees receive no cleanup.
- **Current Align constraint:** caller retains ownership, but replacing a live
  string/array/resource must run the old Drop plan exactly once before the store.
- **Actual failure:** treating all pointee cleanup as forbidden leaks the old value; emitting
  function-exit cleanup double-drops unchanged storage.
- **Recommendation:** prohibit only callee function-exit cleanup for an unchanged pointee; lower
  replacement to guarded old-value Drop, store, and caller cleanup-bit update.
- **v1 impact:** L2/L3 soundness blocker.

### F58 — `resource.into_raw` implied field-level ownership state

- **Classification:** ownership or soundness risk; implementation complexity.
- **Problematic design location:** L3 `ResourceIntoRaw { owner_place }` accepted an unspecified
  place while aggregates have one cleanup bit.
- **Current Align constraint:** transferring only one resource field cannot be represented by the
  aggregate's single path-local cleanup bit.
- **Actual failure:** aggregate cleanup could double-destroy the transferred field or require a
  second hidden per-field ownership system.
- **Recommendation:** restrict v1 `resource.into_raw` to a standalone initialized local/by-value
  resource parameter; reject fields, elements, projections, borrowed/out values, and temporaries.
- **v1 impact:** L3 soundness/API blocker; projected transfer may be reconsidered only with a general
  partial-move ownership design.

### F59 — static manifests omitted derived checked-metadata state

- **Classification:** current Align conflict; implementation complexity.
- **Problematic design location:** L5 `StaticInputManifest` versus DB §16 metadata artifacts.
- **Current Align constraint:** a pre-frontend cache hit must validate every exact deterministic
  input without directory scans or online access.
- **Actual failure:** running `alignc db prepare`, deleting metadata, or changing one driver's
  artifact could reuse a stale producer object.
- **Recommendation:** record each descriptor/permitted-driver exact metadata logical path and
  `Missing | Present(content_hash, format_version)` in the manifest/action key; revalidate exact
  paths before the hit.
- **v1 impact:** L5/D3/D5 correctness blocker for CheckedOptional and CheckedRequired.

### F60 — field-selector function-type examples retained the old ABI shape

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** `03-types.md` field-selector examples after adding
  `ReturnCleanupAbi` and capture-root summaries.
- **Current Align constraint:** `Fn` now has six semantic fields and the canonical return summary is
  `Roots { params, captures }`, not `Params`.
- **Actual failure:** an implementation agent could update the primary type definition but preserve
  the obsolete arity/codec at synthetic field selectors.
- **Recommendation:** spell the complete six-field Fn value and canonical empty-capture root in
  every example; derive cleanup ABI from the projected return type.
- **v1 impact:** L2 ABI/documentation blocker.

### F61 — NUL-bearing strings were not reconciled with native C APIs

- **Classification:** current Align conflict; ownership or soundness risk.
- **Problematic design location:** exact SQL bytes, PostgreSQL Text Params, and libpq connection
  options.
- **Current Align constraint:** Align `str` may contain U+0000, while libpq SQL/options/text
  parameters use NUL-terminated C representations and SQLite has different length-aware behavior.
- **Actual failure:** reviewed/hash input could differ from executed SQL, a text value could be
  truncated, or driver behavior could diverge.
- **Recommendation:** reject U+0000 in static/dynamic/migration SQL before native calls; reject it in
  libpq URL/options and v1 Text Params with `Encode`; keep Binary-format bytea length-aware.
- **v1 impact:** L5/D2/D4/D11 semantic-safety blocker.

### F62 — PostgreSQL first-release type mapping included undecided types

- **Classification:** specification ambiguity; unnecessary broad scope.
- **Problematic design location:** DB §10.3 listed temporal/numeric/UUID/JSON/array mappings while
  §25 left their representations open.
- **Current Align constraint:** public logical/native types must be explicit finite contracts; an
  implementation model may not invent them.
- **Actual failure:** different drivers/agents could silently choose strings, custom structs, or
  native wrappers and call each choice first-release behavior.
- **Recommendation:** fix the first-release mapping to integer/float/bool/text/bytea/Option only and
  require explicit D12–D14 mapping decisions before each additional type.
- **v1 impact:** D4/first-release scope blocker; additional types remain additive.

### F63 — cancellation promised an unimplementable public resource

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** English §20/D9 versus the Japanese mirror's “explicit cancel
  resource.”
- **Current Align constraint:** L3 resources/references are non-Send, and synchronous execution has
  no sound concurrent caller that can invoke a shared cancel handle.
- **Actual failure:** an agent would have to invent a Send escape hatch, unsafe shared raw pointer,
  hidden worker, or a second concurrency model.
- **Recommendation:** D9 owns exact deadline enforcement and driver-owned native
  interruption/cancellation cleanup with no hidden SQL. State explicitly that v1 has no external
  cancel resource; a public handle needs a later general Send/thread-safe-resource prerequisite and
  named roadmap slice.
- **v1 impact:** D9 API/soundness blocker; external user-triggered cancellation is additive.

### F64 — SQLite execution busy timeout ended before streamed execution

- **Classification:** specification ambiguity; performance risk.
- **Problematic design location:** SQLite `ExecuteOption.BusyTimeoutNs` restored “before rows
  exposure.”
- **Current Align constraint:** `rows`/`rows_stmt` defer native stepping until later `next` calls;
  the returned dependent resource, not the constructor call, owns the active execution lifetime.
- **Actual failure:** lock waits during `next` would use the connection's old timeout, silently
  ignoring the requested execution option.
- **Recommendation:** materialized operations restore before return; streaming rows retain the
  override/prior package-tracked value and restore on exhaustion, terminal error, or Drop before
  releasing the dependency. Restore failure poisons/closes the connection.
- **v1 impact:** D2/D8 option correctness blocker.

### F65 — synthetic field selectors checked only a top-level view

- **Classification:** current Align conflict; ownership or soundness risk.
- **Problematic design location:** `03-types.md` field-selector return-borrow rule.
- **Current Align constraint:** return provenance walks recursively through every view-bearing
  aggregate and sum payload; synthetic callables use the same rule as named functions.
- **Actual failure:** projecting `Option<str>` or a struct containing a view could record no receiver
  root, allowing an indirect selector result to outlive its owner.
- **Recommendation:** derive the receiver root from the complete projected type recursively and add
  a nested-view selector acceptance case to L2.
- **v1 impact:** L2 soundness blocker.

### F66 — raw `bytea` was treated as length-aware in libpq Text format

- **Classification:** specification ambiguity; ownership or soundness risk.
- **Problematic design location:** DB §5.6.1/§12.2 PostgreSQL bind formats.
- **Current Align constraint:** libpq ignores parameter lengths for Text-format values; only Binary
  format treats raw bytes as length-delimited.
- **Actual failure:** a Text-format `bytea` containing `0x00` would truncate if the binder passed raw
  bytes, despite the stated explicit-length guarantee.
- **Recommendation:** encode Text `bytea` as exact lowercase PostgreSQL `\x` hex; pass raw bytes only
  in Binary format with an explicit length.
- **v1 impact:** D4/D8 data-correctness blocker.

### F67 — authoritative cancellation text omitted uncertain-connection disposition

- **Classification:** specification ambiguity; ownership or soundness risk.
- **Problematic design location:** English DB §20/D9 versus the Japanese mirror.
- **Current Align constraint:** an opaque connection resource may be reused only while its native
  protocol and transaction state remain proved.
- **Actual failure:** PostgreSQL cancellation can leave unread results or uncertain state, and an
  implementation following only the English contract could return a desynchronized connection.
- **Recommendation:** require drain-and-resynchronize proof or poison/close before any reuse.
- **v1 impact:** D9 resource-safety blocker.

### F68 — connection encoding errors required a fabricated Query identity

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** PostgreSQL connection NUL rejection versus
  `ContractError.query_id: string`.
- **Current Align constraint:** connection validation occurs before a Query exists and errors must
  expose truthful data rather than sentinel identities.
- **Actual failure:** the wrapper would invent a Query ID or use an unrelated error category.
- **Recommendation:** make `query_id` optional; use `Some(id)` for Query/command contracts and
  `None` plus an exact `item` for Query-less operation/input validation.
- **v1 impact:** D2/D4 public error-contract blocker.

### F69 — deferred PostgreSQL logical types remained in a first-release example

- **Classification:** specification ambiguity; unnecessary broad scope.
- **Problematic design location:** DB §5 Params example and §10.1 after narrowing §10.3.
- **Current Align constraint:** temporal/decimal/UUID/JSON/container representations remain explicit
  D12–D14 consumer decisions, while first-release examples are executable specification.
- **Actual failure:** an implementation agent could invent `db.timestamp` early to make the example
  compile, reopening the scope F62 closed.
- **Recommendation:** use only an `i64` parameter in the initial example and state that every
  additional logical type appears only after its exact cross-driver contract is settled.
- **v1 impact:** first-release scope blocker; those types remain additive.

### F70 — metadata wording could discard an available Query identity

- **Classification:** specification ambiguity.
- **Problematic design location:** DB error semantics after making `ContractError.query_id`
  optional.
- **Current Align constraint:** `meta_query` and Query EXPLAIN are metadata operations but still have
  a concrete Query subject and stable descriptor identity.
- **Actual failure:** grouping all metadata validation under `None` would conflict with the
  Query-contract rule and permit loss of useful identity.
- **Recommendation:** use `Some(id)` whenever a Query/command subject exists, including metadata and
  EXPLAIN; use `None` only for truly Query-less operation/input validation.
- **v1 impact:** D12 diagnostic-contract blocker.

### F71 — the struct-field rule contradicted L1a

- **Classification:** current Align conflict; ownership or soundness risk.
- **Problematic design location:** `draft.md`'s aggregate field restriction versus L1a's required
  `Option<string>` field.
- **Current Align constraint:** ownership is a property of the resolved type; a finite aggregate can
  be Move when the compiler has one recursive Drop plan.
- **Actual failure:** the literal “every struct field is Copy” rule made the first prerequisite's
  only new field shape invalid and gave an implementation agent two incompatible specifications.
- **Recommendation:** permit Copy or finite recursively Move fields after substitution when the
  ordinary Drop plan is known; retain the explicit unsupported collection/recursive-shape errors.
- **v1 impact:** L1a blocker; resolved in the language documents before implementation.

### F72 — safe `resource.borrow` was grouped with privileged representation access

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** prerequisite §3.2 native-construction wording and example.
- **Current Align constraint:** consumers must be able to obtain an owner-tied opaque
  `resource_ref<R>` without seeing or extracting the native representation.
- **Actual failure:** an agent could restrict `resource.borrow` to the declaring package, making
  ordinary safe borrowed APIs unusable, or could treat unsafe as bypassing module visibility.
- **Recommendation:** make `resource.borrow` public and safe wherever `R` is visible; privilege only
  raw construction, extraction, transfer, and owner-tied raw-view operations.
- **v1 impact:** L3 public API blocker; resolved in the prerequisite plan.

### F73 — `db.exec_result` exposed an unspecified ownership choice

- **Classification:** specification ambiguity; ownership or soundness risk.
- **Problematic design location:** DB §6.1's affected-row/native-status result.
- **Current Align constraint:** native command-status text often borrows a result buffer, while an
  owned string needs a visible owner/destination and allocation.
- **Actual failure:** a wrapper could return a dangling native view or silently allocate a string
  despite the common result appearing Copy.
- **Recommendation:** make the initial common result exactly the allocation-free Copy record
  `{ rows_affected: Option<i64> }`; any later native status surface must expose ownership/storage.
- **v1 impact:** D2/D4 soundness/API blocker; native status is additive.

### F74 — migration commands omitted deterministic project and target inputs

- **Classification:** specification ambiguity; implementation complexity.
- **Problematic design location:** DB §§17.2/17.5 command forms.
- **Current Align constraint:** normal and explicit tool inputs must be reproducible and may not
  depend on ambient current-directory scans, inferred drivers, or secret defaults.
- **Actual failure:** two implementations could select different entry graphs, migration catalogs,
  drivers, or databases, and repair could target a different catalog from migrate.
- **Recommendation:** require exact `--entry`, `--migrations`, `--driver`, and matching
  SQLite-path/PostgreSQL-URL-environment inputs on every migrate/status/check/repair form.
- **v1 impact:** D11 correctness/safety blocker.

### F75 — metadata filters had no public input types

- **Classification:** specification ambiguity.
- **Problematic design location:** DB §18.2 `schema_filter` and `table_ref` signatures.
- **Current Align constraint:** public package generics/signatures require concrete types and cannot
  rely on reflection, anonymous records, search paths, or a driver-selected default schema.
- **Actual failure:** implementations could disagree on name qualification, lifetime, allocation,
  escaping, or SQL interpolation.
- **Recommendation:** define Copy `db.SchemaRef` and `db.TableRef` inputs, borrow them only for the
  call, require exact schema/name lookup, and bind/escape rather than concatenate identifiers.
- **v1 impact:** D12 public API/safety blocker.

### F76 — `KeyMeta` did not contain the promised constraint semantics

- **Classification:** specification ambiguity.
- **Problematic design location:** DB §§18.2/18.7.
- **Current Align constraint:** metadata categories are explicit flat records; prose-only native
  facts cannot be recovered by reflection or hidden nested objects.
- **Actual failure:** foreign-key match/actions, deferrability, initial state, and validation could
  be silently discarded despite the design promising faithful key/constraint metadata.
- **Recommendation:** add exact optional match/action/deferral/validation fields and finite common
  enums; use `None` only when unavailable.
- **v1 impact:** D12 contract blocker.

### F77 — `IndexMeta` did not contain the promised index semantics

- **Classification:** specification ambiguity; performance risk.
- **Problematic design location:** DB §§18.2/18.8.
- **Current Align constraint:** one flat row per term must distinguish key from included terms and
  preserve order without a hidden nested allocation.
- **Actual failure:** callers could not reconstruct covering/partial/expression index shape,
  primary backing, order/null placement, or validity/readiness, making introspection inaccurate and
  performance decisions unreliable.
- **Recommendation:** add exact term-kind, primary-backed, sort/null-order, valid/ready, and existing
  native method/opclass fields; order key terms before included terms.
- **v1 impact:** D12 correctness blocker.

### F78 — `ColumnMeta` and `QueryMeta` omitted fields promised by their own sections

- **Classification:** specification ambiguity.
- **Problematic design location:** DB §18.2 exact records versus §§16.3/18.6/18.9.
- **Current Align constraint:** separate compilation and offline inspection can consume only fields
  serialized in the exact artifact/public record contract.
- **Actual failure:** native type identity, column descriptive fields, source/wire hashes, rewrite
  version, prepare/schema/server identity, and structured result origin could disappear or be
  represented incompatibly.
- **Recommendation:** add every promised field explicitly, with structured origin components and
  `Option` only for genuinely unavailable evidence.
- **v1 impact:** D3/D5/D12 artifact and public metadata blocker.

### F79 — checked metadata consumed nullability policy before its owner milestone

- **Classification:** prerequisite feature missing; specification ambiguity; ownership or
  soundness risk.
- **Problematic design location:** DB D3/D5 versus §25's former D12–D14 open decision.
- **Current Align constraint:** a typed decoder must conservatively reject unexpected NULL at
  runtime; neither SQLite nor PostgreSQL catalog nullability alone describes an arbitrary Query
  result after joins and expressions.
- **Actual failure:** D3/D5 could merge using an invented optimistic rule, and later policy changes
  would invalidate checked artifacts or make non-`Option` decoding unsound.
- **Recommendation:** D0 records actual engine/version evidence; D3/D5 own checked-in fail-closed
  support matrices. Ambiguous evidence is `Unknown`, never proves non-null, and never removes the
  runtime NULL guard.
- **v1 impact:** D3/D5 merge blocker; resolved as a prerequisite policy rather than deferred.

### F80 — metadata detail/discriminator projection was not executable

- **Classification:** specification ambiguity; implementation complexity.
- **Problematic design location:** DB §18.1/§18.2 record lists before §18.2.1.
- **Current Align constraint:** flat `RegionPlain` metadata has no runtime reflection or tagged
  property bag that can recover unspecified presence, ordering, or ordinal semantics.
- **Actual failure:** D12 implementations could return different row counts at `Names`, duplicate
  summary-only fields, choose different ordinal bases, or hash different artifact inputs while all
  claiming to implement the listed record types.
- **Recommendation:** define the exhaustive category/detail and `MetaQueryEntry` matrices, exact
  ordering/ordinal bases, inapplicable/unavailable `Option` behavior, and exact artifact digest.
- **v1 impact:** D12 public compatibility blocker.

### F81 — metadata identifier views lacked native-boundary encoding rejection

- **Classification:** specification ambiguity; ownership or soundness risk.
- **Problematic design location:** DB §18.2 `SchemaRef`/`TableRef` input semantics.
- **Current Align constraint:** `str` is length-delimited and can contain U+0000, while native SQL
  and catalog control paths may be NUL-terminated.
- **Actual failure:** a safe metadata lookup could truncate a schema/table name or behave
  differently between SQLite and PostgreSQL before the package notices.
- **Recommendation:** reject U+0000 in each ref component before native/catalog access with exact
  Query-less `db.Error.Encode` items and negative tests on both drivers.
- **v1 impact:** D12 input-safety blocker.

### F82 — non-optional nullability cells remained implicit

- **Classification:** specification ambiguity; ownership or soundness risk.
- **Problematic design location:** DB §18.2.1 optional/unavailable rule versus non-optional
  `ColumnMeta.nullable`/`QueryMeta.nullable`.
- **Current Align constraint:** the decoder must not manufacture positive nullability evidence when
  detail suppresses it, a Query is Declared, or the engine has no answer.
- **Actual failure:** implementations could select different enum values for the same matrix cell
  and one could treat absent evidence as non-null.
- **Recommendation:** require `Unknown` for every suppressed, unavailable, ambiguous, or Declared
  cell and test the complete state/detail product.
- **v1 impact:** D3/D5/D12 soundness/compatibility blocker.

### F83 — Query metadata row groups could be interleaved

- **Classification:** specification ambiguity.
- **Problematic design location:** DB §18.2.1 QueryMeta ordering.
- **Current Align constraint:** flat region records have no secondary iterator contract; consumers
  depend on one deterministic sequence.
- **Actual failure:** Parameter and Column rows could each be internally ordered yet interleaved
  differently by the two drivers.
- **Recommendation:** require Summary, then all Parameters, then all Columns and test a Query
  containing both groups.
- **v1 impact:** D12 cross-driver compatibility blocker.

### F84 — artifact digest inputs exceeded the canonical artifact schema

- **Classification:** current Align conflict; specification ambiguity.
- **Problematic design location:** DB §18.2.1 `artifact_digest` versus prerequisite §6.2.
- **Current Align constraint:** a digest over exact emitted bytes can include only serialized
  versioned artifact fields.
- **Actual failure:** the metadata contract named Params/Row fingerprints and binder/decoder ABI
  versions that the canonical artifact record did not serialize, forcing an agent to omit them or
  invent an incompatible encoding.
- **Recommendation:** add those exact fields to Query/command artifact schemas and D1
  round-trip/invalidation tests.
- **v1 impact:** L5/D1/D12 cache/separate-compilation blocker.

### F85 — multi-invalid metadata refs had no error precedence

- **Classification:** specification ambiguity.
- **Problematic design location:** DB §18.2 SchemaRef/TableRef U+0000 errors.
- **Current Align constraint:** diagnostics and no-side-effect validation must be deterministic
  across drivers.
- **Actual failure:** a TableRef with U+0000 in both components could return schema or name error
  depending on driver validation order.
- **Recommendation:** validate public record declaration order (`schema` then `name`) and pin the
  dual-invalid case.
- **v1 impact:** D12 diagnostic compatibility blocker.

### F86 — constraint name was used as if it were unique

- **Classification:** specification ambiguity.
- **Problematic design location:** DB §18.2.1 KeyMeta ordering/grouping.
- **Current Align constraint:** flat rows need an explicit stable group identity; SQLite permits
  multiple constraints with the same declared name.
- **Actual failure:** `(name, term_ordinal)` cannot distinguish same-named keys and lets catalog
  iteration order affect common output.
- **Recommendation:** make the reported name optional, add zero-based `key_ordinal`, derive it from
  a canonical complete common key signature before detail suppression, and test unnamed/duplicate
  names.
- **v1 impact:** D12 metadata identity/determinism blocker.

### F87 — the canonical key-group signature omitted Full-detail policy fields

- **Classification:** specification ambiguity; performance risk.
- **Problematic design location:** DB §18.2.1 `KeyMeta` canonical grouping.
- **Current Align constraint:** deterministic flat metadata cannot use hidden catalog row order as a
  final tie-breaker.
- **Actual failure:** two same-named constraints with identical terms but different match/action,
  deferral, or validation fields compared equal without being byte-identical, so drivers could
  assign different `key_ordinal` values.
- **Recommendation:** sort by every Full-detail common field, require one normalized group-level
  value for repeated policy/evidence fields, and reject contradictory native rows.
- **v1 impact:** D12 cross-driver determinism blocker.

### F88 — the canonical Query artifact named fields without a complete codec

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** prerequisite §6.2 artifact codec.
- **Current Align constraint:** separate and incremental compilation require independently
  reproducible bytes, not a producer/consumer pair that merely agrees with itself.
- **Actual failure:** integer widths, nested rewrite/span/occurrence/binding records, sequence order,
  and option/state payloads were unspecified; two correct-looking implementations could emit
  different artifact bytes and digests.
- **Recommendation:** fix every scalar/tag, nested record, top-level field order, canonical sequence,
  decoder invariant, and Query/command byte+digest golden vector.
- **v1 impact:** L5/D1 separate-compilation/cache blocker.

### F89 — the author-side ledger omitted the strengthened descriptor artifact contract

- **Classification:** specification ambiguity.
- **Problematic design location:** §1.1 Query/command descriptor ledger row.
- **Current Align constraint:** the ledger is the author-side propagation gate and must name every
  public/cache identity invariant it owns.
- **Actual failure:** F84's fingerprints and ABI versions plus F88's codec/goldens were present only
  in prose, allowing later author passes to report the surface complete without checking them.
- **Recommendation:** put the canonical codec, fingerprints, ABI versions, golden tests, and
  prerequisite source in the ledger row.
- **v1 impact:** design-completion blocker before L5 implementation starts.

### F90 — nominal type references were mistaken for structural fingerprints

- **Classification:** current Align conflict; implementation complexity.
- **Problematic design location:** prerequisite §6.2 Params/Row fingerprint codec.
- **Current Align constraint:** a named type reference does not encode its same-path field
  definition, while checked metadata must become stale when that definition changes.
- **Actual failure:** field name/order/type/nullability edits could leave artifact and metadata
  fingerprints unchanged.
- **Recommendation:** serialize the complete sorted reachable instantiated definition graph and
  hash that structural contract; pin same-path definition edits.
- **v1 impact:** L5/D1/D3/D5 cache and decode soundness blocker.

### F91 — runtime Query metadata had no producer-owned data source

- **Classification:** prerequisite feature missing; current Align conflict.
- **Problematic design location:** prerequisite §6.3 versus DB §18.2 `meta_query`.
- **Current Align constraint:** ordinary package code has no reflection and normal runtime code
  cannot read compiler artifacts or `.align-db`.
- **Actual failure:** a separately compiled Query could not provide declared parameter/Row records
  or checked native/origin evidence promised by D12.
- **Recommendation:** serialize a producer-owned QueryMeta plan/evidence table in L5/D1, then emit
  the exact materialization thunk and descriptor ABI extension in D12 with its first consumer.
- **v1 impact:** L5/D1/D12 implementation blocker.

### F92 — checked metadata had no exact path or canonical JSON codec

- **Classification:** specification ambiguity; current Align conflict.
- **Problematic design location:** DB §16.3.
- **Current Align constraint:** prepare and offline compilation are independent producers/consumers
  and must agree on exact paths, bytes, malformed-input behavior, and digests.
- **Actual failure:** key order, tags, escaping, numeric forms, query-ID hashing, and evidence
  identities could differ while each implementation claimed canonical JSON.
- **Recommendation:** fix descriptor-ID pathname derivation, every JSON field/key order/encoding,
  derived identity streams, fail-closed validation, and independent SQLite/PostgreSQL goldens.
- **v1 impact:** D3/D5 offline/reproducibility blocker.

### F93 — SQLite streamed timeout overrides could overlap

- **Classification:** ownership or soundness risk; performance risk.
- **Problematic design location:** DB §11.2 `BusyTimeoutNs` with Copy `db.exec`.
- **Current Align constraint:** dependent resources block owner destruction, not a second Copy
  execution view, while SQLite busy timeout is connection-global.
- **Actual failure:** two row streams could overwrite and restore each other's timeout in the wrong
  order and expose one execution's policy to another.
- **Recommendation:** give each SQLite native connection one explicit active-execution lease and
  reject every overlapping native operation before state change/call; pin cleanup order.
- **v1 impact:** D2/D8 runtime correctness blocker.

### F94 — migration catalog tuples had no canonical byte identity

- **Classification:** specification ambiguity.
- **Problematic design location:** DB §16.6/D11 migration fingerprint.
- **Current Align constraint:** SQLite preparation and migration commands are separate consumers of
  one schema identity.
- **Actual failure:** version width, filename framing, content hash, and final digest could differ,
  producing false stale/current results.
- **Recommendation:** define the versioned `ALIGNMIG`/`ALIGNSID` binary streams and independent
  empty/non-empty byte+digest goldens.
- **v1 impact:** D3/D11 reproducibility blocker.

### F95 — normative metadata examples used nonexistent named/typed call arguments

- **Classification:** current Align conflict.
- **Problematic design location:** DB §18.2 metadata calls.
- **Current Align constraint:** Align calls are positional; `name: Type` is declaration syntax, not
  an argument expression.
- **Actual failure:** the promised exact public API examples did not parse and left signatures
  ambiguous for implementation.
- **Recommendation:** show exact signatures as non-source notation, use typed local bindings plus
  positional calls, and syntax-check every normative Align example.
- **v1 impact:** D12 API-definition blocker.

## 3. Answers to the requested feasibility checks

1. **Language compatibility:** the original proposal conflicts at Move payloads, borrowed Move/Copy
   calls, function-value modes, imported return provenance, dependent native handles/linkable Drop,
   contextual parameter parsing, named regions, nested generic package types, Move-return cleanup,
   closure capture roots, and static non-Align inputs. L1a–L7 are the required general repairs.
   Module/FFI/package rules are compatible after the
   revised layering.
2. **Package boundary:** `pkg.db`, `.sqlite`, and `.postgres` are appropriate public names only as
   three modules of one vendorable `pkg/db` subtree. They are not three independent versions.
3. **Ordinary package code:** public data shapes, options/errors/metadata, safe FFI wrappers,
   connection/transaction/statement/rows lifecycle, driver operations, Query-local shaping, and
   migration orchestration.
4. **Runtime support:** generic checked owner-tied raw views, UTF-8 validation, and region-builder
   chunk/compact helpers. No DB-specific runtime Query engine or handle type.
5. **Compiler/semantic support:** L1a–L7, recognized Query constructors, static-input tracking,
   Query contract checking, placeholder scan/source maps, artifacts/hashes, generated
   binder/decoder thunks, a D1 QueryMeta plan, and the D12 materializer thunk.
6. **Static `db.query<Params,Row>`:** a Copy immutable descriptor data record plus direct generated
   binder/decoder functions, the producer-owned QueryMeta plan, the D12 materializer, and structural
   type contracts. Its function body is exactly
   one constructor, giving the item one unique artifact identity. The rowless
   `db.command<Params>` uses the same statement artifact, static ABI, binder, and cache rules minus
   Row/result/decode/QueryMeta. Neither is an object with reflection or a runtime SQL parser. L7
   makes ordinary generic Query execution helpers representable.
7. **Sibling `.align`/`.sql`:** a path-free registered constructor maps the defining module's exact
   extension to `.sql`; exact source bytes, logical path, kind, and digest are deterministic inputs,
   separate from deterministic driver wire bytes. Inline SQL instead uses `Inline(query_id)` and a
   decoded-literal source map.
8. **Module export:** `IStaticQuery` carries the public contract; `StaticQueryArtifact` carries SQL
   and implementation metadata, including the structural contract and producer-owned runtime
   QueryMeta plan. D12 extends the descriptor with its exact materializer. A private SQL-only edit
   rebuilds/relinks the producer without
   invalidating unchanged consumers. Function values retain parameter/capture provenance and the
   Move-return cleanup ABI across separate compilation. Static manifests also key every exact
   per-driver checked-metadata missing/present state.
9. **Named parameters:** dialect-aware lexical occurrence table. SQLite uses native named
    parameter indices; PostgreSQL rewrites unique names to stable `$n` positions and reuses an
    ordinal for repeats. Streaming APIs release source Params provenance at return by using
    execution-owned/native bind storage; SQLite v1 uses transient-copy semantics. SQL source rejects
    U+0000, PostgreSQL Text values/options reject embedded NUL, Text `bytea` is lowercase hex, and
    raw `bytea` is Binary-only.
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
14. **Compound Output builders:** L6 supplies the region-backed builder with chunk growth and one
    measured compacting pass; L7 supplies the structural `RegionPlain` bound and generic
    `array<R>` package signature. Builders stay separate mutable locals and are borrowed by the
    Pure step.
15. **Offline checked metadata:** explicit prepare/check tooling invokes real database engines;
    ordinary build consumes the exact derived path and fail-closed canonical JSON and never
    connects. Explicit SQLite migration replay uses the versioned `ALIGNMIG`/`ALIGNSID` identity;
    mutable targets name a schema ID. Checked state is driver-indexed, so an unpinned
    CheckedRequired descriptor requires current artifacts for both SQLite and PostgreSQL. The
    pre-frontend manifest keys exact Missing/Present metadata state.
16. **Native options:** distinct typed finite sums at all seven scopes, with separate common/native
    slices and no silent ignore. The minimum constructors/defaults/conflicts are fixed in §§11–13;
    their owning milestones precede every consumer. Category metadata calls carry an explicit
    destination region and the mandatory `MetaOption` slice. D9 enforces deadlines through
    driver-owned cancellation cleanup; it does not expose an unsound non-Send cancel resource and
    never reuses an unproved post-cancel connection.
17. **Roadmap:** move all language work before drivers; split Move work into L1a/L1b and add L7
    nested generic package composition; prove fake
    Query/command artifacts before native code; assign each option sum to D1/D2/D4/D6/D7/D12 before
    its consumer and use D9 for shared deadline enforcement/native cancellation cleanup; put SQLite
    and PostgreSQL scalar verticals before streaming/transactions/compound output; complete exact migration and
    region-owned category metadata/EXPLAIN contracts in D11/D12 before the first public release;
    schedule D13/D14 as additive native/dynamic work.
18. **Minimum SQLite vertical:** in-memory connection, one scalar command, one sibling-file scalar
    Query, `execute`/`one`, cardinality/error/cleanup/execution-count tests, no text views.
19. **Minimum PostgreSQL vertical:** same common Query module, named-to-positional rewrite, scalar
    bind/decode from the fixed integer/float/bool/text/bytea subset, SQLSTATE error, driver
    restriction, explicit configured ephemeral/local server,
    and a non-skippable provisioned CI gate for merge/release.
20. **Capability order:** L1a/L1b precede the L2 capability waves; after L2, L3/L4/L5 may proceed
    together, L6 follows L4, and L7 closes the prerequisite integration. D0 may run at any time.
    After D1, the SQLite and PostgreSQL driver/metadata branches proceed by the §5 dependency DAG;
    D1–D12 close the initial release and D13–D14 close the complete committed roadmap. Labels own
    tests and local measurement rails, not mandatory individual PRs.

## 4. Required specification revisions

The reviewed specification is acceptable only with these revisions, now incorporated in the design
documents:

1. Make L1a–L7 mandatory before a safe driver.
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
34. Define parameter-storage retention through streaming execution and measure its bind copies.
35. Require an exact visible driver argument on every dynamic SQL operation.
36. Separate shipped verified signatures from required-but-unimplemented L4/L6 signatures.
37. Add `slice<db.MetaOption>` to every category metadata primitive and a separate native slice to
    driver-native forms.
38. Give metadata/EXPLAIN exact flat `RegionPlain` result shapes and an explicit destination region.
39. Track checked metadata per permitted driver and require every entry under CheckedRequired.
40. Complete the command artifact/static ABI/binder/cache contract in L5/D1 before D2.
41. Fix the mandatory first-release common/SQLite/PostgreSQL option variants, defaults, and conflicts.
42. Assign option API ownership to D1/D2/D4/D6/D7/D12 before D9's shared completion audit.
43. Define required-by-default and forbidden migration transaction algorithms, dirty state, and
    checksum-bound repair.
44. Reject either partial-NULL child shape in the canonical one-to-many shaper.
45. Keep `next_batch` solely in D13 in both language documents.
46. Keep the Japanese many-parent design on the same parallel parent/child/offset representation.
47. Make provisioned PostgreSQL integration non-skippable for D4 merge and database releases.
48. Add L7 nested generic package applications and the closed structural `RegionPlain` bound.
49. Carry a dynamic cleanup bit with every recursively Move return across all call/interface ABIs.
50. Check every argument mode for direct or recursively embedded overlap beside `BorrowMut`.
51. Carry target-relative capture roots in concrete closure return provenance.
52. Distinguish no callee function-exit cleanup from required drop-old replacement through
    `BorrowMut`.
53. Restrict `resource.into_raw` to standalone owned resource roots in v1.
54. Include exact per-driver checked-metadata Missing/Present state in static manifests/action keys.
55. Update every synthetic/function-value example to the capture-root and Move-return cleanup ABI.
56. Reject U+0000 at every NUL-terminated native boundary before SQL send or connection setup.
57. Make the first-release PostgreSQL type mapping exact and leave undecided native types out.
58. Define D9 as deadline enforcement/native cancellation cleanup and remove the unsupported v1
    public cancel resource.
59. Keep SQLite execution-scoped busy timeout active for the complete streamed rows lifetime.
60. Derive synthetic field-selector return roots recursively through nested view-bearing types.
61. Encode PostgreSQL Text `bytea` as lowercase hex and reserve raw length-delimited bytes for
    Binary format.
62. Require cancellation to resynchronize or poison/close a connection before reuse.
63. Represent Query-less contract failures without a fabricated Query ID.
64. Remove deferred logical types from first-release executable examples.
65. Preserve Query identity in metadata/EXPLAIN errors whenever the operation has a Query subject.
66. Make finite recursively Move struct fields agree with L1a's ordinary Drop-plan rule.
67. Separate public safe `resource.borrow` from declaring-subtree raw representation operations.
68. Fix `db.exec_result` as one allocation-free Copy affected-row record.
69. Give every migration/repair command exact entry, catalog, driver, and matching target inputs.
70. Give metadata filters concrete exact-schema `SchemaRef`/`TableRef` types.
71. Put every promised foreign-key/constraint field in the exact flat `KeyMeta` record.
72. Put every promised key/include/order/state field in the exact flat `IndexMeta` record.
73. Align the exact `ColumnMeta`/`QueryMeta` records with artifact and category promises.
74. Move the nullability/origin evidence contract to D0/D3/D5 and keep runtime NULL guards.
75. Define exact metadata detail/discriminator projections, ordinals, ordering, and artifact digest.
76. Reject U+0000 in every metadata schema/table reference before a native/catalog request.
77. Fill every non-optional nullability cell with fail-closed `Unknown` when evidence is absent.
78. Order QueryMeta rows as Summary, all Parameters, then all Columns.
79. Serialize the fingerprints and binder/decoder ABI versions named by the artifact digest.
80. Define declaration-order error precedence for multi-invalid metadata references.
81. Represent absent constraint names honestly and group duplicate names with a canonical
    zero-based `key_ordinal`.
82. Include every Full-detail policy/evidence field in the canonical key-group signature.
83. Fix every Query/command artifact scalar, nested record, order, rejection rule, and independent
    byte/digest golden.
84. Keep the strengthened codec/fingerprint/ABI/golden contract in the author-side ledger.
85. Fingerprint complete reachable structural Params/Row definitions, not nominal references.
86. Carry a producer-owned QueryMeta plan in the D1 artifact and introduce its exact materialization
    thunk/descriptor ABI only in D12 with the first consumer.
87. Define the exact checked-metadata descriptor path, canonical JSON/identity codecs, validation,
    and independent driver goldens.
88. Serialize SQLite connection-global execution with one explicit overlap-safe lease.
89. Define the versioned migration catalog/schema-identity codecs and independent goldens.
90. Show exact metadata signature notation separately from syntax-checked positional call examples.

## 5. Revised capability roadmap

This table assigns contract, test, and measurement ownership. It is not a
one-row-per-PR queue. Current prerequisite waves and product dependencies are
defined by `HANDOFF.md`, `17-library-boundary-prerequisites.md`, and
`pkg-design/db.md` §23.

The current default publication boundaries are:

| Wave | Acceptance owners | Outcome |
|---|---|---|
| C-A | remaining L2 canonical callable cells through c3 | one callable/type/ABI authority |
| C-B | return provenance cells + L2c/L2d/L2e | complete public borrow/ownership behavior |
| F-A / F-B / F-C | L3 / L4+L6 / L5 | parallel resources, region materialization, and static artifacts |
| F-D | L7 | integrated ordinary-package prerequisite gate |
| Q1 | D1 | fake-driver static Query vertical |
| Q2 | D2 + D4 | one dual-driver scalar product |
| Q3 | D3 + D5 | one dual-driver checked/offline contract |
| Q4a | D6 + D7 | reusable prepared/transaction execution |
| Q4b | D8 + D9 | streaming, deadline, cancellation, and cleanup resilience |
| Q5a / Q5b | D11 / D12 | parallel mutation and read-only schema capabilities |
| Q6 | D10 | compound-output release closure |
| A1 / A2 | D13 / D14 | additive throughput/native and dynamic/callback release trains |

Q1–Q4b and Q6 default to one capability PR each. Q5 uses two parallel PRs
because schema mutation and inspection are independent failure domains. A1/A2
may publish independently useful common or driver rails in parallel. No other
acceptance label creates a PR boundary by itself.

| Milestone | Scope | Required focused tests | Local measurement/evidence |
|---|---|---|---|
| L1a | Recursive DropPlan framework; `Option<string>` fields | `owned_tagged_payloads`, analysis coverage | tagged construct/pass/drop |
| L1b | Move sum/Option/Result completion | `?`/`else`/`match`/join cleanup | no-allocation `Ok`, error cleanup |
| L2a | parameter-mode and borrow/region-summary records across HIR/MIR/interface/ABI identity; existing behavior only | codec/hash goldens, corrupt summaries, exhaustive consumer audit | interface size and decode cost |
| L2b | recursive parameter/capture return provenance for existing values and function-value joins | nested-view selectors, captured/joined direct/indirect/imported matrix | summary size and inference cost |
| L2c | cleanup-ABI record plus dynamic bit for recursively Move direct/indirect/imported returns | codec/hash goldens, None/Some/Err control-path parity, ABI mismatch rejection | return ABI cost |
| L2d | shared `borrow` mode over Move owners | reusable owner, move rejection, returned-view lifetime, function-value/import parity | borrowed-call cost |
| L2e | `borrow mut`, unified Out/BorrowMut exclusivity, Copy/Move mutation, drop-old, Pure shaping | recursive alias/stale-view/drop-count/effect matrix | exclusive-call cost |
| L3 | resource/ref, linkable Drop thunk, dependent child/native view, root-only raw transfer | exact MIR, cross-unit Drop, invalid pointer/escape/projection, all-peer resource aliases, captured/joined refs, dependent identity provenance | resource/ref/view overhead and IR |
| L4 | named arena `region`, `clone_in` | all escape paths, module propagation, captured/joined region-owned values | named versus anonymous arena |
| L5 | tagged file/inline/checked-metadata inputs, structural Query/command artifacts, QueryMeta descriptor plan | cache/path/inline-span/same-path-type-edit/metadata-create-change-delete/runtime-plan/golden/reproducibility matrix | cold/warm producer/consumer rebuild and metadata-plan size |
| L6 | region `RegionPlain` builder | copy count, no heap, current-row rejection | push/freeze throughput and bytes |
| L7 | nested generic package applications and closed `RegionPlain` bound | inference/substitution, mono/interface parity, bound negatives, no dictionaries | compile time, interface/mono size, code size |
| D0 | SQLite/libpq capability probes only | native lifecycle plus exact origin/nullability evidence observations | recorded driver/version evidence matrix |
| D1 | fake-driver Query/command binder, Query decoder/metadata plan, scanner, static options | independent artifact byte/digest golden plus structural fingerprint/cache/placeholder/retention/runtime-metadata matrices | thunk overhead and warm cache |
| D2 | scalar SQLite Query/command + connection/execution options/lease | cardinality, option/overlap disposition, NUL/PRAGMA validation, cleanup, execution count | package/lease versus libsqlite3 |
| D3 | SQLite prepare/check metadata | exact JSON/path/identity goldens, malformed/stale/policy/offline, `ALIGNMIG`/`ALIGNSID`, and fail-closed origin/nullability matrix | prepare/check/codec time and artifact size |
| D4 | scalar PostgreSQL Query vertical + connection/execution options (`BufferedFull`) | rewrite, fixed type map, Text-hex/Binary bytea, NUL/query-less error, option disposition, SQLSTATE, mismatch, cleanup, required CI | package versus libpq |
| D5 | PostgreSQL checked metadata | exact JSON/path/identity golden, recreated-schema reproducibility, expression/outer-join/catalog-nullability matrix | describe/prepare/codec time |
| D6 | dependent prepared statements + prepare option sums | sequential reuse, disposition, bind-storage cleanup, child-before-parent Drop | prepare reuse/rebind |
| D7 | tx options plus common exec view | combination rejection, consume/commit/rollback/fail-safe Drop | tx/common-dispatch overhead |
| D8 | typed rows and row generations | old-view rejection, Params-source invalidation, type matrix, busy-timeout lifetime, stream-overlap/Drop lease, clone retention, delivery counts | row decode/iteration/bind copies/lease |
| D9 | deadline enforcement/native cancellation cleanup + all-scope audit | applied/unsupported/conflict/precedence, hidden-SQL/public-cancel absence, resynchronize-or-close | deadline/cancellation overhead |
| D10 | one-pass compound Output | many-to-one/one-to-many, exactly one SQL | shaping allocation/copy/throughput |
| D11 | exact-input exact-policy SQL migrations with versioned identity codec; initial-release gate | CLI selector, byte/digest golden, checksum/order/atomic/dirty/repair/status | migration fingerprint/startup/large history |
| D12 | exact signature notation and parseable validated calls/typed-ref/detail/discriminator/record, region-owned category metadata and EXPLAIN options, QueryMeta materializer ABI/code and descriptor-header version; initial-release gate | signature-table parity; syntax; cross-unit runtime plan/materializer source; multi-invalid precedence; duplicate-key identity; detail/state/entry Unknown/group order; field/ordinal/digest/lifetime/allocation/flatness/category isolation; Query-ID context; ANALYZE visible | catalog query count/record bytes/latency and metadata thunk |
| D13 | batch/SoA/native paths/pool | generation, native lifecycle, exact semantics | driver-specific throughput rails |
| D14 | driver-restricted dynamic rows and proved callbacks | pre-send mismatch, allocation/lifetime/reentrancy/cleanup | dynamic decode/callback overhead |

The delivery dependencies are:

```text
L1a/L1b -> C-A -> C-B
C-B -> { F-A, F-B, F-C } -> F-D -> Q1 -> Q2
Q2 -> { Q4a -> Q4b -> Q6, Q3 -> { Q5a, Q5b } } -> initial release
initial release -> { A1, A2 } -> complete committed roadmap
P0/D0 evidence runs in parallel before Q2.
```

This is the publication relation. The exact internal D-cell dependency remains
in `pkg-design/db.md` §23; each wave still closes every applicable owner row
before claiming its consumer capability.

## 6. Historical first implementation checkpoint

This section records the original first implementation boundary. L1a has since
completed. Its commands and benchmark were the acceptance evidence used for
that checkpoint, not a reusable gate for later work.

The first implementation PR was **L1a only**. It did not contain database,
resource, borrow, region, or static-input code.

Scope:

- add one canonical recursive owned-value `DropPlan` after type definitions resolve;
- allow `Option<string>` as a struct field and classify its enclosing struct as Move;
- emit tag-tested Drop on normal return, early return, and reassignment;
- move/null the live payload on supported whole-value moves;
- retain explicit diagnostics for `Option<MoveStruct>` (owned by L1b), recursive types, unsupported
  deep partial moves, arbitrary Move collection elements, unsupported Move-leaf replacement, and
  fixed-array Move-leaf reads; whole-struct/whole-element replacement remains supported.

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
cargo clippy --workspace --lib --bins --locked -- -D warnings
bench/owned_tagged_payload/run.sh
```

Acceptance behavior:

- resolved finite struct fields follow one rule: Copy, or recursively Move only when the canonical
  Drop plan exists after substitution;
- `None` never reads or drops uninitialized payload storage;
- `Some` drops exactly once across return, `?`, branches, loops, and reassignment;
- `Some(old) -> Some(new)` and `Some -> None` drop old before replacement;
- `None -> Some` does not run a spurious Drop;
- nested outer Move structs containing `Option<string>` fields recurse through the same plan;
- partial Move-leaf replacement and fixed-array Move-leaf reads fail closed unless their exact
  typed Drop/borrow lowering exists; whole-struct/whole-element replacement uses the recursive plan;
- `Option<MoveStruct>` remains a clean compile error naming L1b;
- malformed/unsupported types produce diagnostics rather than compiler panic;
- generated LLVM has a tag guard and introduces no allocation on `None`.

## 7. Local measurement inventory

The named paths remain locally measurable through:

- L1a/L1b tagged-Move branch, allocation, and Drop counts;
- L2 direct/indirect Move borrow and Copy-state mutable borrow versus current builtin receiver,
  including all-peer recursive alias-check cost, capture-root join/transfer, dynamic return-bit
  overhead, and return-summary/interface size/time;
- L3 resource construction/ref/dependent cross-unit Drop thunk/native-view checks versus direct
  current handle path, with exact MIR-operation counts;
- L4 named/anonymous arena parity;
- L5 cold/warm rebuild matrix for unchanged, source-SQL-only, wire-rewrite, private, public, and
  checked-metadata create/change/delete states, plus structural-definition, QueryMeta plan,
  file/inline, and descriptor-count scaling;
- L6 exact heap bytes, region bytes, push throughput, and one compacting pass;
- L7 nested-generic inference/monomorph compile time, interface and mono-key size, emitted code size,
  cache reuse, and absence of runtime dictionary/indirect-call overhead;
- D0 records the exact engine/version origin/nullability evidence matrix rather than timing a
  production path;
- D1 generated Query/command binder and Query decoder plus metadata-plan size versus hand-written
  field/ordinal data; D12 measures the materializer thunk;
- D2 direct libsqlite3 comparison, including zero-allocation `db.exec_result` and lease overhead;
- D3 prepare/check canonical JSON time/size, structural evidence scaling, and migration
  fingerprint/replay scaling at 10/100/1000 files;
- D4 direct libpq comparison with transported/buffered/decoded rows reported separately and
  zero-allocation `db.exec_result`;
- D6 prepare reuse versus reprepare;
- D8 row iteration/decode, Params bind copied bytes/allocations, physical-delivery counts, and
  retained-row-copy/lease costs;
- D10 one-pass one-to-many shaping, allocation/copy count, and exact one SQL execution;
- D11 migration CLI startup/status cost and ordered history scaling at 10/100/1000 applied files,
  including identity-codec time, with target parsing separated from database-open time;
- D12 category metadata query count, exact record bytes, region bytes/compact count, native-buffer
  copy bytes, and EXPLAIN latency;
- D13 batch/SoA/native throughput on each driver;
- D14 dynamic dispatch/mismatch overhead versus direct driver-qualified execution.

Run a measurement when its named performance path first lands or materially
changes, or when a human explicitly investigates it. Measurements are not
ordinary regression tests and are not PR, release, or milestone gates. They may
not justify removing ownership, runtime contract validation, explicit options,
or one-statement semantics.

## 8. Review execution record

The first final adversarial pass reviewed the complete 24-file, roughly 7,200-added-line design diff
as one unit. It was still making useful progress after reading ownership, resource, byte-view,
module, binding-retention, and option paths, but reached the repository's 15-minute bound after
20,787 log lines without returning a verdict. It was recorded as a tool failure, never as CLEAN.
The replacement three-document DB-contract pass completed in about ten minutes and returned ten
actionable findings, F43–F52. The subsequent language-foundation pass completed its assigned scope
and returned seven further findings, F53–F59.
The final whole-diff consistency invocation read the complete document set and reached candidate
validation but ended at its configured invocation bound before emitting a verdict. Its preserved
log exposed five independently verified issues, F60–F64. Those exact areas were corrected and
checked directly; the complete review was not restarted from the beginning.
The checkpoint continuation then reviewed only those corrections and their dependent contracts in
about eight minutes. It returned four further findings, F65–F68; direct validation of the same
type-scope area found F69. The fixes were again made from that checkpoint rather than restarting the
full document scan.
The next two-minute continuation found one remaining wording ambiguity, F70, in Query-specific
metadata error context; it did not restart the completed scan.
After those corrections, a fresh independent post-open review still found F71–F79. Those findings
were valid. Their common cause was not an obscure engine detail: the authoring process had no
complete public-contract ledger before prose was declared ready. Consequently exact records,
command inputs, milestone ownership, and one language rule were checked only when a reviewer
compared each prose promise with its executable surface. Using review as that completion loop was
unacceptably late and explains the repeated corrections.

The public-contract ledger in §1.1 now closes those categories in one pass, and the canonical
repository guidance requires the ledger before independent review. F71–F79 were propagated from
that ledger through the authoritative language/package/roadmap documents and the Japanese mirror;
the review was not restarted as another unconstrained whole-document search.
The final ledger closeout then found F80–F81 in the one metadata row: the row named exact record
types but had not expanded detail/discriminator presence or input encoding. The stale-HEAD host
review was stopped immediately rather than allowed to consume its remaining bound. The metadata
ledger row was expanded first, then propagated as §18.2.1 plus the two-driver negative-test gate.
The first focused verification of that table found F82–F85: four still-unfilled Cartesian-product
or cross-schema cells. The table was made total for Unknown state and group order, the canonical
artifact schema gained the fields named by its digest, and input validation gained explicit
multi-invalid precedence. Each stale review was stopped at the finding rather than restarted or
allowed to run to its bound.
The stopped host checkpoint also demonstrated that SQLite accepts duplicate constraint names. F86
therefore replaces name-based grouping with the canonical `key_ordinal` before the next closeout.
The focused closeout of that correction found F87–F89 without reopening unrelated surfaces: the
key signature still omitted policy/evidence fields, the artifact codec still named nested values
without fixing their bytes, and the ledger had not absorbed the strengthened descriptor contract.
The key signature now contains every Full-detail common field, §6.2 fixes the complete top-level and
nested codec plus independent Query/command goldens, and the ledger owns those exact requirements.
The required final whole-diff host review then found six older cross-surface omissions, F90–F95,
which the intentionally narrow F87–F89 closeout did not cover. They were not handled by another
open-ended author/reviewer loop: one ledger pass added structural type identity, the runtime
QueryMeta source, exact checked-metadata and migration codecs, SQLite connection-state exclusion,
and metadata signature-table parity plus syntax-checked positional calls, then propagated those six
decisions together.

The mistake was not that the complete pass consumed fifteen minutes. It was that elapsed time was
used as a substitute for inspecting whether the pass was still producing new, relevant analysis.
Terminating a productive pass and restarting the same review from the beginning increased total
time. Future design reviews of this size use this order:

1. Build one invariant matrix before prose review: public signature, owner/region, static/runtime
   identity, driver restriction, failure state, owning milestone, acceptance test, and benchmark.
2. Review language foundations (`ownership/region/Move/FFI/artifact`) as one bounded group.
3. Review the authoritative English package contract
   (`Query/command/options/metadata/migrations/drivers`) as a second bounded group.
4. Review roadmap, acceptance, cache/separate-compilation integration, and mirrors as a third group.
5. Make the authoritative English contract clean before synchronizing the Japanese mirror once.
6. Run one final whole-diff consistency scan only after all grouped findings are fixed.

During every long-running review, inspect process state, log growth, the most recently completed
area, and whether the output contains new findings rather than repeated analysis. Elapsed time alone
is not a reason to abandon useful work. Stop or redirect a run only when output has stalled, repeats
the same analysis, leaves the requested scope, or the review tool has actually failed. If a formal
review invocation must be stopped at an automation bound, preserve its completed analysis and
continue from the first unreviewed area; never restart the whole review merely because the invocation
ended. A timeout never implies cleanliness. This progress-based monitoring and checkpointed
partitioning reduce repeated work without weakening the required final whole-diff review.
