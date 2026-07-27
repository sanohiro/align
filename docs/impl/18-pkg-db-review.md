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
- **Recommendation:** implement L1a recursive `DropPlan` plus `Option<Move>` fields, then L1b all
  finite non-recursive Move sum/Option/Result payload paths. Tag-test Drop and null every moved
  source.
- **v1 impact:** blocker for the public error and Output contract. It is the first prerequisite PR.

### F2 — by-value Move parameters cannot express reusable handles

- **Classification:** prerequisite feature missing; current Align conflict.
- **Problematic design location:** `db.conn`, `db.tx`, prepared statement reuse, and a Pure
  Query-specific shaper.
- **Current Align constraint:** an ordinary Move parameter consumes its argument. Existing builtin
  handles have bespoke receiver behavior; there is no general public function parameter mode for a
  shared or exclusive borrow.
- **Actual failure:** a package function either consumes the connection/statement/state on every
  call or requires another compiler-known database handle exception. Passing a Move shaping state by
  value also makes the one-pass shaper unusable.
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
  migrations D11, metadata/EXPLAIN D12, batch/native paths D13, and dynamic SQL/callbacks D14.
  These are scheduled prerequisites for their features, not vague “maybe later” items.
- **v1 impact:** later additions; only D1–D10 block the initial Query product.

## 3. Answers to the requested feasibility checks

1. **Language compatibility:** the original proposal conflicts at Move payloads, borrowed Move
   calls, imported return provenance, dependent native handles, named regions, current generics, and
   static non-Align inputs. L1a–L6 are the required general repairs. Module/FFI/package rules are
   compatible after the revised layering.
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
   binder/decoder functions. It is not an object with reflection or a runtime SQL parser.
7. **Sibling `.align`/`.sql`:** a path-free registered constructor maps the defining module's exact
   extension to `.sql`; exact bytes, logical path, kind, and digest are deterministic inputs.
8. **Module export:** `IStaticQuery` carries the public contract; `StaticQueryArtifact` carries SQL
   and implementation metadata. A private SQL-only edit rebuilds/relinks the producer without
   invalidating unchanged consumers.
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
    has no DB handle, and transitive effects reject hidden I/O.
14. **Compound Output builders:** L6 region-backed `RegionPlain` builder with chunk growth and one
    measured compacting pass. Builders stay separate mutable locals and are borrowed by the Pure
    step.
15. **Offline checked metadata:** explicit prepare/check tooling invokes real database engines;
    ordinary build consumes canonical artifacts and never connects.
16. **Native options:** distinct typed finite sums at all seven scopes, with separate common/native
    slices and no silent ignore.
17. **Roadmap:** move all language work before drivers; split Move work into L1a/L1b; prove fake
    Query artifacts before native code; put SQLite and PostgreSQL scalar verticals before
    streaming/transactions/compound output; schedule broad surfaces D11–D14.
18. **Minimum SQLite vertical:** in-memory connection, one scalar command, one sibling-file scalar
    Query, `execute`/`one`, cardinality/error/cleanup/execution-count tests, no text views.
19. **Minimum PostgreSQL vertical:** same common Query module, named-to-positional rewrite, scalar
    bind/decode, SQLSTATE error, driver restriction, explicit configured ephemeral/local server.
20. **Small PR order:** L1a, L1b, L2, L3, L4, L5, L6, D0, D1, D2, D3, D4, D5, D6, D7, D8, D9,
    D10, then D11–D14. Each owns only the tests and benchmark rail listed below.

## 4. Required specification revisions

The reviewed specification is acceptable only with these revisions, now incorporated in the design
documents:

1. Make L1a–L6 mandatory before a safe driver.
2. Replace builtin-handle enumeration with general package-defined/dependent resources.
3. Add borrowed parameters, imported return provenance, and the exclusive-input purity rule.
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

## 5. Revised implementation roadmap

| PR | Scope | Required focused tests | Benchmark/evidence |
|---|---|---|---|
| L1a | Recursive DropPlan; `Option<Move>` fields | `owned_tagged_payloads`, analysis coverage | tagged construct/pass/drop |
| L1b | Move sum/Option/Result completion | `?`/`else`/`match`/join cleanup | no-allocation `Ok`, error cleanup |
| L2 | `borrow`, `borrow mut`, effect/provenance summaries | same-unit/per-unit alias and escape parity | borrowed-call and interface-size cost |
| L3 | resource, resource_ref, dependent child, native view | exact Drop, invalid pointer, parent/child escape | resource/ref/view overhead and IR |
| L4 | named arena `region`, `clone_in` | all escape paths and module propagation | named versus anonymous arena |
| L5 | static inputs, Query/command artifacts, descriptor skeleton | cache matrix, path safety, reproducibility | cold/warm producer/consumer rebuild |
| L6 | region `RegionPlain` builder | copy count, no heap, current-row rejection | push/freeze throughput and bytes |
| D0 | SQLite/libpq capability probes only | native lifecycle/metadata observations | recorded driver evidence |
| D1 | fake-driver Query binder/decoder and scanner | artifact/cache/placeholder matrix | thunk overhead and warm cache |
| D2 | scalar SQLite Query vertical | cardinality, cleanup, execution count | package versus libsqlite3 |
| D3 | SQLite prepare/check metadata | stale/missing/policy/offline matrix | prepare/check time and artifact size |
| D4 | scalar PostgreSQL Query vertical | rewrite, SQLSTATE, mismatch, cleanup | package versus libpq |
| D5 | PostgreSQL checked metadata | recreated-schema reproducibility | describe/prepare time |
| D6 | dependent prepared statements | sequential reuse and child-before-parent Drop | prepare reuse/reprepare |
| D7 | tx plus common exec view | consume/commit/rollback/fail-safe Drop | tx/common-dispatch overhead |
| D8 | typed rows and row generations | old-view rejection and clone retention | row decode/iteration |
| D9 | all option scopes, timeout, cancellation | applied/unsupported/conflict matrix | option/cancellation overhead |
| D10 | one-pass compound Output | many-to-one/one-to-many, exactly one SQL | shaping allocation/copy/throughput |
| D11 | SQL migrations | checksum/order/transaction/status | migration startup/large history |
| D12 | category metadata and EXPLAIN | category isolation; ANALYZE executes visibly | catalog query count/latency |
| D13 | batch/SoA/native paths/pool | generation, native lifecycle, exact semantics | driver-specific throughput rails |
| D14 | dynamic rows and proved callback surfaces | allocation/lifetime/reentrancy/cleanup | dynamic decode/callback overhead |

## 6. Exact first implementation PR

The first implementation PR is **L1a only**. It must not contain database, resource, borrow, region,
or static-input code.

Scope:

- add one canonical recursive owned-value `DropPlan` after type definitions resolve;
- allow `Option<Move>` as a struct field and classify its enclosing struct as Move;
- emit tag-tested Drop on normal return, early return, and reassignment;
- move/null the live payload on supported whole-value moves;
- retain explicit diagnostics for recursive types, unsupported deep partial moves, and arbitrary
  Move collection elements.

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
- nested finite owned structs recurse through the same plan;
- malformed/unsupported types produce diagnostics rather than compiler panic;
- generated LLVM has a tag guard and introduces no allocation on `None`.

## 7. Required benchmark set

The delivery is not performance-complete without:

- L1a/L1b tagged-Move branch, allocation, and Drop counts;
- L2 borrowed call versus current builtin receiver plus interface summary size/time;
- L3 resource construction/ref/dependent Drop/native-view checks versus direct current handle path;
- L4 named/anonymous arena parity;
- L5 cold and warm rebuild matrix for unchanged, SQL-only, private, and public contract changes;
- L6 exact heap bytes, region bytes, push throughput, and one compacting pass;
- D1 generated binder/decoder versus hand-written field/ordinal code;
- D2 direct libsqlite3 comparison;
- D4 direct libpq comparison;
- D6 prepare reuse versus reprepare;
- D8 row iteration/decode and retained-copy costs;
- D10 one-pass one-to-many shaping, allocation/copy count, and exact one SQL execution;
- D12 category metadata query count and EXPLAIN latency;
- D13 batch/SoA/native throughput on each driver.

Benchmark results are evidence and regression anchors. They may not justify removing ownership,
runtime contract validation, explicit options, or one-statement semantics.
