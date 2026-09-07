# XML producer validation: investigate, then repair

Status: RESOLVED implementation investigation, recorded 2026-09-05 and closed
2026-09-06 at the owner's request, then reopened and reclosed when the v0.7.0
release archive exercised the complete shipped-package corpus. This is an
implementation issue register, not a new language contract. It records the
complete finding set, classification, repair boundary, and owner evidence in
one place rather than as isolated review rounds.

## Scope and current evidence

The XML branch expanded shared MIR producer validation. Some findings are real
source-program regressions introduced by that expansion; others concern forged
internal graphs. These are different evidence classes, not equally demonstrated
runtime defects. The existing contract remains in
[`std-design/xml.md`](std-design/xml.md), especially the producer, native callback,
and reader-action closure matrices.

The reviewed predecessor is
`7a66c43d264ebdbf433bd231f862852b62a8b745`, based on
`718526e8f7605a4ecdc22314c2082b7aefc97b88`. The repair stays inside the shared
MIR producer validator and its direct codegen/driver owners. It changes no
language, XML, DB, ABI, or package contract.

The predecessor full review reported five P2 findings, and the changed-slice
review of the coherent repair reported one more P2 in the same malformed-MIR
closure. Earlier reader
use-after-free and native callback identity findings motivated the committed
redesign; this register does not claim those earlier fixes can be removed safely.

| ID | Classification | Resolution |
|---|---|---|
| X1 | Source regression introduced by producer validation. | Captured return roots now follow the authenticated callable environment through copied, stored, and joined closures. Missing fields, wrong field types, and forged capture indices fail closed. Direct parameter roots remain unchanged. |
| X2 | Source regression plus sibling malformed-MIR gaps. | AoS string-key group writers now preserve key provenance and own numeric results. The same inventory validates multi-aggregate, dictionary encode, numeric gather, dictionary lookup, output identity, length, field, nominal type, arity, and alias relations. The numeric gather mutation is owned by the numeric consumer instead of an unrelated key consumer. |
| X3 | Source regression introduced by producer validation. | `XmlParse` now certifies only its exact builtin `Error` payload in addition to the whole result and reader payload. Input ownership and cleanup identity checks remain mandatory. |
| X4 | Source regression plus type-class malformed-MIR gaps. | Numeric equations cover the source-admitted scalar and vector arithmetic, vector/scalar broadcasts, comparisons, lane masks, vector selection, scalar unary operators, and exact widths. Scalar-left arithmetic was rejected as an invalid fixture because sema does not admit it under the vector expectation; the validator was not widened to manufacture a new language rule. |
| X5 | Malformed-MIR contract gap. | `MaybeAbsent` becomes present only with the exact discriminator guard. Statically inactive guarded payloads remain absent. Option, Result, enum, and optional-reader joins have positive and guard-removal/wrong-predicate mutations. |
| X6 | Malformed-MIR allocation-safety gap. | Every caller-owned buffer passed to the direct group, dictionary, gather, and lookup runtime writers authenticates its `HeapAllocBuf` count against that writer's exact row, column, or explicit write bound. One parameterized mutation owner replaces each sibling allocation count with a smaller valid `i64`, for both whole-program and per-unit validation. |
| X7 | Source regression exposed by the release package corpus. | The validator now recognizes the settled one-way `http_request_ctx` to `http_headers` zero-cost view retype used by `ctx.headers()`. The reverse direction remains rejected, so a detached view cannot mint an owning request handle. |
| X8 | Source regression from a previously unconfirmed lowered shape. | Lifted-lambda capture roots become their exact trailing ABI parameter roots when HIR lowers the direct callable body. Closure construction reverses that representation using its checked capture inventory, preserving the existing environment-relative function-value contract. `pkg.web.group` and `group_with` own the source-reachable direct-call shape; malformed out-of-range roots still fail in whole-program and per-unit validation. |

## Consolidated DB and performance result

The failed DB batch had one shared producer cause, not a separate defect per
failing test. Generated query binders legitimately retype owned fields to views,
including `Option<string>` to `Option<str>` and
`Option<array<u8>>` to `Option<slice<u8>>`. The validator now permits those
recursive owner-to-view conversions while continuing to reject every reverse
view-to-owner conversion. The focused whole/per-unit `pkg_db_q4b` parity owner
then advanced past the former diagnostic.

That successful path exposed one performance defect: every protected access
root rebuilt the same validated producer subgraph. On the generated Q4b unit,
the focused owner exceeded the 15-minute binary budget. A per-function cache now
reuses only fully validated `Present` node results. It never caches absent,
maybe-absent, unresolved, or invalid states, and the graph is immutable for the
cache lifetime. The same owner completed in 102.52 seconds after the repair.

The repair is roughly 1,000 changed hand-written lines because the six findings
and the DB regression are one shared validator failure domain. Splitting its
producer equations from the cross-path malformed-MIR owners would create a
dormant producer/consumer chain and duplicate the same certification proof,
preflight, and DB boundary verification. Keeping one coherent repair has less
integration risk.

One observation remains explicitly unconfirmed and outside this repair:

- whether forged unsigned scalar negation or Bool-conditioned vector selection
  can be reached from source-admitted MIR.

No production surface is widened for either observation. If source reachability
is later demonstrated, reopen this record as one follow-up investigation rather
than appending a speculative patch to the XML release.

## Verification selection

The coherent worktree passed these focused owners before the final commit:

```text
align_codegen_llvm producer_*: 10 passed
align_codegen_llvm --lib: 134 passed, 5 ignored
align_driver resource_ownership: 23 passed
align_driver std_xml: 14 passed
pkg_db_q4b full_matrix_parity_is_exact_on_both_drivers: passed in 102.52s
```

The release follow-up additionally passes the direct lifted-capture mutation
owner, the one-way HTTP header-view owner, and the exact `pkg.web` prebuilt-cache
corpus that failed on all three v0.7.0 release platforms. Its exact committed
SHA passed its review and `scripts/pre-pr.sh`, then merged in PR #946. The
v0.7.1 tag did not publish because its release measurement used a stale corpus;
the shared-corpus v0.7.2 correction and release are the terminal workflow. This
record does not authorize another feature or consumer repository work.

## Evidence locations and focused commands

The local raw review is `.git/align-review-reader-action.log`; its complete final
verdict starts near line 16933. Local checkpoint evidence is retained under
`.git/align-xml-investigation-evidence/`. These paths are local evidence, not
portable or committed test results; the findings above remain readable without
them. `.git/align-resume-xml-release.md` owns operational recovery details.

Focused commands:

```bash
scripts/cargo.sh test -p align_codegen_llvm --lib producer_
scripts/cargo.sh test -p align_driver --test std_xml producer_certification_
scripts/cargo.sh test -p align_driver --test std_xml xml_parse_rejects_invalid_documents_without_partial_reader_publication -- --exact
scripts/build-prebuilt-cache.sh target/release/alignc target/release/prebuilt-cache
```

The required final DB gate uses `DOCKER_HOST=unix:///var/run/docker.sock` on this
host and `scripts/db-verify-local.sh`. The failed discovery run is preserved in
the evidence directory. Respect the existing thirty-minute run and fifteen-minute
binary budgets rather than increasing them.

## v0.7.2 owned `Result` loop-join regression

Status: candidate implemented. This is a post-release correction inside
the existing MIR producer-certification contract; it does not widen the Align
language, JSON surface, interface ABI, or XML surface.

Align v0.7.2 rejects an owned `string` moved through a loop-updated local into a
record field and then into `Result.Ok`. The whole-program native build and the
per-unit interface producer must accept the same source graph. Certification
must still reject any reaching borrowed, mixed, unresolved, or malformed
producer for that selected leaf.

Implementation closure matrix:

| Cell | Required closure | Owner |
|---|---|---|
| Formation and initial ownership | A recursively owned JSON manifest forms successfully; the selected local starts from an owned empty-string clone. | `producer_owned_result_loop_join.align` plus the driver regression owner |
| Loop and branch joins | Zero or more loop replacements from owned clones preserve `Owned`; every reaching replacement is included, and an invalid reaching store still poisons the selected leaf. | Source regression plus the existing malformed producer mutation owners |
| Record construction and move-in | Moving the local into `VerifiedBundle.backend_sha256` preserves the exact fourth-field path and does not certify sibling fields in its place. | MIR-shape assertion and whole-program native execution |
| Result construction and return | `ResultOk -> StructField(3)` reaches the owned producer and retains the existing dynamic cleanup companion; the moved source is nulled and dropped exactly once. | Whole-program native execution and the existing returned-cleanup owners |
| Interface publication | The exact source is accepted by both whole-program build and per-unit build/interface reconstruction. | One differential driver owner running both paths |
| Negative closure | Borrowed strings, mixed/unknown joins, duplicate definitions, malformed selected paths, and detached cleanup remain rejected. | Existing `align_codegen_llvm` `producer_*` invalid fixtures; extend the selected-path mutation owner only if the fix creates a new bypass shape |

The author-side matrix pass must trace every selected-path dependency added by
the repair and confirm that no validation-only sibling edge can manufacture
ownership. The reduced fixture must fail against v0.7.2 with the reported
`[ResultOk, StructField(3)]` diagnostic so a passing test distinguishes the
repair from a non-exercising fixture.

The defect was the pointer-based AoS field equation: source lowering exposes an
owned `string` field only as a borrowed `str`, while certification required the
result type to equal the stored field type. The repair admits that one-way view,
follows provenance using the stored `String` path, and rejects a forged
`IndexFieldPtr` that tries to mint a new owned `String`. The sibling fixed-array,
slice, SoA-column, and whole-element equations either already own their exact
conversion or cannot carry this source shape.

The published v0.7.2 compiler rejects the reduced fixture in both native build
and per-unit checking with the exact reported path. The candidate compiler
accepts `check`, `check-per-unit`, and native build/run; the zero-replacement and
replacement executions both complete. All 13 `align_codegen_llvm` `producer_`
owners and all three `align_driver` `std_xml` `producer_certification_` owners
pass, including the new reverse owner-minting mutation.

## Request 58 producer fixed-point termination

Status: candidate implemented and locally verified. This is an implementation
scaling correction inside the existing MIR producer-certification contract. It
adds no language, library, MIR, interface, or runtime ABI surface.

The unchanged align-llm client at `acfdd3b` checks successfully but its native
build does not finish in 60 seconds. A local 75-second run remained at 100% CPU.
Two debugger samples five seconds apart found the same 25-node producer graph for
`alignpack_read$read_append`, the same selected return root, and a nonempty
two-to-three-node worklist. A bounded diagnostic then recorded 100 updates of the
same `Value(87)` node; its last ten transitions alternate exactly between
`Present(Owned)` and `MaybeAbsent(Owned)`. This is a non-convergent fixed point,
not growth in the number of functions or graph nodes.

The repair separates facts that the old single state map conflated. First, a
monotone fixed point computes only reachable present producers and their access
capability. `Unknown` and `Mixed`, which both grant no capability, collapse to one
canonical rejecting lattice element so the access join is commutative,
associative, and idempotent. Second, with present reachability fixed, a monotone
boolean fixed point computes reachable absence. An authenticated discriminator
projects `present + absent` to present only after those raw facts have converged;
an unguarded presence requirement marks any reaching absence invalid. Explicit,
unresolved, capability, and validation-only failures then flow through the
existing fail-closed invalidity closure. Caching remains limited to capable
`Present` results after every phase completes.

Implementation closure matrix:

| Cell | Required closure | Owner |
|---|---|---|
| Finite convergence | Present access and absence each grow in a finite-height lattice. Seeded cyclic value/slot equations terminate after bounded monotone widenings; unseeded cycles terminate as invalid. | A reduced `align_codegen_llvm` equation owner pins the separated guarded-cycle semantics and the existing seeded/unseeded cycle owner remains green. The whole-program/per-unit borrowed-reader fixture is the pre-fix discriminator: the old compiler does not terminate on it. |
| Presence joins | Loop, `if`, `match`, `else`, and `?` joins retain every reachable raw present and absent alternative. Guard projection runs only after raw present convergence, so an exact discriminator may select present without feeding a transient narrowing back into the producer graph. | Existing guarded-absence positive and guard-removal/wrong-predicate mutations plus reduced guarded-input and loop-join regressions. |
| Capability joins | Owned, shared, exclusive, unreadable, and the canonical no-capability element form an explicit conservative semilattice; only an all-owned reaching set certifies a returned owned leaf. | Exhaustive commutativity, associativity, and idempotence owner plus existing owned/shared selected-alternative and malformed producer owners. |
| Construction, move, replacement, and return | Value construction, slot store/load, replacement, source nulling, Drop, record/sum construction, and returned cleanup keep their current equations and exactly-once ownership result. | Existing owned `Result` loop fixture, returned-cleanup owners, and the reduced borrowed-reader pipeline. |
| Calls and control exits | Direct/imported/generic calls, branch joins, loop backedges, early `return`, `break`, `else`, `?`, and `map_err` cannot erase a reaching invalid or shorter-lived producer. | Whole-program/per-unit reduced pipeline plus the same shorter-lived-view negative in whole-program and per-unit checking; existing call-contract and Request 37 long-function/loop-`match` owners. |
| Validation separation and cache | Check-only edges still validate total typed states and poison dependent nodes; absent, conditional, unresolved, and invalid results are not cached or published as certified access. | Existing check-cycle, detached cleanup, malformed type/path, and cache-sensitive producer mutations. |
| End-to-end bound | The exact published align-llm `runtime_qwen_load_smoke.align` build completes below the requested 60-second owner limit without bypassing producer validation. | Align-owned reduced fixture locally; align-llm runs `gmake gpu-qwen-load-smoke` on the shipped release and records the named Apple M1 measurement. |

The author-side matrix pass must show that each raw phase is inflationary in its
own finite lattice, that no guard projection participates in present propagation,
that lattice height bounds updates per node independently of graph cycles, and
that all acceptance decisions occur only after present, absence, and
validation-only invalidity propagation complete.

The pre-implementation adversarial review found two P1 strategy gaps and one P2
evidence gap. The two-phase raw solver closes the non-monotone guarded-presence
gap; canonicalizing the rejecting access element plus the exhaustive algebra
owner closes the non-semilattice gap; the recorded 100-update alternating trace
closes the evidence gap. No public contract or producer capability is widened.

The candidate passes all 138 `align_codegen_llvm` library owners, all 16
`align_driver` `std_xml` owners, and all 23 `resource_ownership` owners. The
exact published align-llm source at `acfdd3b` reaches native linking in 15.52
seconds with the compiler cache disabled instead of remaining in MIR resource
validation. Its `gmake gpu-qwen-load-smoke` invocation also completes compiler
validation and linking, then the generated program independently exits with
`Error` code 2 inside the client-owned runtime plan path. Release verification
therefore remains with align-llm after it updates `.align-revision`; the Align
regression owner is the reduced whole-program/per-unit pipeline above.

## Request 59 canonical Copy call operands

Status: implemented and owner-verified; closure prepared before repair. Issue #966's
unchanged client at align-llm `1a37b5b` fails on Align `f291700f`. The failing
return field is not the original invalid equation: a sibling prompt depends on
`run_spec` called with `BorrowedPlace` projections through an optional borrowed
record. Those projections are the settled physical representation of Copy
arguments (`str`, `slice<str>`, and `i64`) passed by value. `add_call_result`
defers canonical Borrow/BorrowMut/Out arguments to the complete call validator,
but mistakenly sends their ByValue Copy siblings through an operand equation
that rejects every borrowed descriptor. Whole-result validation then propagates
that failure to the builder-owned string field.

The repair follows the existing validation split, not a new ownership rule:
only the already-admitted `BorrowedPlace` Copy-by-value descriptors gain a typed
READ requirement on their translated storage path in the existing producer
worklist, in addition to call-site validation. `operands_match_modes`
still requires absent cleanup and excludes Move-by-value descriptors; ByValue
`BorrowedElementPlace` and `BorrowedFixedElementPlace` remain rejected. The latter
families must not be admitted by this repair: their element/index/guard validation
is not the same publication boundary as a traversable `BorrowedPlace` path.
A returned Copy view rooted in such an argument propagates the grounded storage
provenance, exactly like the existing ByValue SSA representation;
the worklist must authenticate every later store and selected/sibling producer,
and the call validator must authenticate its descriptor and declared mode before
producer certification succeeds. Do not use `xml_borrowed_access`'s parameter
shortcut for this new producer edge: entry authority alone omits later stores.
Exact same-element array-to-slice retyping retains the actual storage path/type.
No equation constructor may invoke a nested producer analysis. Owned return leaves still
require the existing certified callee and dynamic cleanup contract.

Implementation closure matrix:

| Cell | Required closure | Owner |
|---|---|---|
| Source discriminator | Optional borrowed record fields passed as Copy values feed an owned prompt beside an initialized/replaced builder string and multiple early record returns. The reduced fixture must fail on `f291700f`, not merely resemble the source. | New `std_xml` whole/per-unit producer fixture; unchanged public `verification_loop.align` check-per-unit. |
| Call forms and projections | Direct/imported/generic and indirect calls share `add_call_result`; already-admitted root/tagged `BorrowedPlace` Copy projections retain exact type/mode/cleanup checks. ByValue element/fixed-element descriptors remain rejected. Returned Copy views propagate existing source authority and presence without a new seed. | Parameterized codegen producer owner plus existing indexed-call and callable-fact mutation owners. |
| Negative admission | ByValue Move descriptors, wrong slots/paths/types, unreadable Out sources, shared mutable arguments, uncertified callees, and detached cleanup remain rejected before publication and emission. | Parameterized malformed-MIR owner in whole/per-unit modes and existing `producer_*` owners. |
| Construction, replacement, move, Drop, return | Empty owned initialization, builder replacement, record move-in, source nulling, early exits and returned cleanup retain the unchanged lowering and exactly-once lifecycle. | New source fixture executes each return branch in whole/per-unit builds; existing `resource_ownership` suite. |
| Control joins and graph termination | `if`, `match`, loop, `else`, `?`, and `map_err` keep existing equations. Canonical call arguments do not initiate a recursive solver or seed a new intra-function cycle. | Existing guarded/seeded/unseeded/check-cycle `producer_*` owners and Request 58 fixture. |
| Publication and allocation | Whole-program, per-unit interface publication, and instantiated generic bodies use the same validator. No serialized record, runtime ABI, allocation, or language contract changes. | `std_xml` producer owners, codegen mutation parity, local PostgreSQL owner gate if classified in scope. |

The author-side extraction binds the existing obligations for every actual call
edge, exact types/modes/cleanup, selected-path provenance, whole-result validity,
and same-worklist dependencies to this matrix. No performance promise is added,
so a benchmark is not a correctness gate. Consumer revision adoption and its
build/smoke commands remain align-llm-owned; no versioned release is requested.

The independent pre-implementation review found two boundary gaps: admitting
indexed/fixed ByValue descriptors, and trusting parameter entry authority without
later-store proof. The matrix now keeps those descriptor modes rejected and
requires same-worklist storage dependencies. The malformed owner must include
a wrong callable stored after parameter entry beside indexed/fixed rejection.

Author-side closure: `check_copy_place` validates the precise stored projection
and attaches its leaf READ requirements; `add_call_result` retains them for
owned results and propagates those same source dependencies for rooted views.
`producer_copy_place_calls_preserve_storage_proof`
crosses direct/indirect/generic/view calls with bad slot/path/type/cleanup/Out
and unsupported descriptor mutations in publication, whole, and per-unit modes.
`producer_copy_place_checks_callable_stores_after_parameter_entry` rejects the
bad later callable store; deleting the new READ requirement makes that owner
fail, so parameter-entry authority cannot silently replace this proof.
`producer_certification_accepts_owned_builder_record_after_copy_projection_call`
reproduces field 7 on the pre-fix compiler, executes all four return paths in
whole/per-unit builds, and rejects a borrowed `str` in the same owned field.
All 17 `producer_*`, 17 `std_xml`, and 23 `resource_ownership` owners pass.
The unchanged client at `1a37b5b` now passes all five per-unit modules.

The one full-diff code review found a P2 malformed-MIR gap in the candidate's
rooted-view seed: a call with a check-only input edge could seed its own otherwise
uninitialized slot cycle. The correction uses the translated storage nodes as
ordinary provenance dependencies, matching the existing ByValue SSA path without
changing the solver or admitting any new descriptor. The READ checks remain.
`producer_copy_place_return_cycles_require_a_grounded_source` owns both the
unseeded rejection and seeded acceptance in publication and whole/per-unit modes;
restoring the candidate's unconditional seed makes its publication negative fail.
This closes the finding with a local producer-edge correction and does not
change the language, IR shape, capability boundary, or existing solver strategy.

## Canonical producer return type identity

Current-source reproduction at `3051b98c` confirms the deferred Request 52
supporting-owner failure: `origin_specific_generic_instances_share_one_tagged_llvm_type`
fails in `pick` before LLVM emission with `producer return operand disagrees
with its declared result type`. The program returns `Result<Option<Holder<fn(i64)
-> i64>>, Error>` from branches containing distinct pure/impure noncapturing
function origins. These are one source type, but origin-specific internal table
IDs differ. The source and existing canonical type machinery already permit
that representation; producer return validation must not redefine identity as
raw table-ID equality.

The repair reuses the existing `source_ty_matches`/`CanonicalTy` authority.
It does not equate arbitrary equal-layout types, loosen callable contracts,
invent provenance, change an IR variant, or alter the worklist solver. Every
returned protected leaf still needs its own complete producer proof and every
cleanup relation stays exact. An invalid canonical graph must fail closed.

| Closure axis | Implementation requirement and owner |
|---|---|
| Formation, generic origins and source identity | Compare actual and declared return types through the existing canonical source-type authority, not raw origin IDs or LLVM body layout. The existing source regression must discriminate the pre-fix gate and keep its single tagged LLVM type and runtime result assertions. |
| Construction, move-in/out, nulling, Drop, replacement, returned cleanup | No lowering or lifecycle changes. Cross Copy callable records and an owned-string sibling with both direct and nested tagged returns; retain existing `resource_ownership` owners and malformed cleanup owners. |
| Branches and control joins | The source owner executes pure/impure/absent branches. Existing producer owners retain if/match/else/?/map_err, loop joins, guards and early-exit equations. Inspect sibling producer type comparisons for this same canonical-identity defect rather than changing unrelated exact slot/index/descriptor comparisons. |
| Whole-program, per-unit and generic publication | The shared validator must accept canonical aliases before both interface publication and executable emission. Add imported whole/per-unit execution coverage to the existing source witness; generic source instantiation retains existing owner coverage. |
| Malformed types and producers | A parameterized codegen owner must reject different nominal types with equal layout, changed fields, malformed table IDs, wrong callable signatures/modes, and invalid return cleanup in publication and whole/per-unit emission. Canonical identity never substitutes for protected-leaf provenance or actual producer construction validation. |
| Allocation, runtime ABI, artifacts and performance | Unchanged ownership, allocation strategy, cache formats and ABI. Compiler-source identity remains the cache invalidator. No performance/resource claim and no benchmark requirement. |

The author-side matrix follows the existing reviewed validation strategy; its
boundary check belongs in the one fresh preflight review. Before requesting
that review, map the exact/canonical identity, every protected leaf, malformed
graph rejection and whole/per-unit obligations above to the diff and owner
results. Do not claim closure from accepting the original positive alone.

Author-side closure: `validate_resource_rvalues_component` uses the existing
`source_ty_matches` authority for the value/declared return pair. Raw-call and
direct/indirect callable return consumers already use that authority; exact
slot, descriptor, discriminator and cleanup-SSA relations remain unchanged.
`producer_canonical_return_types_preserve_nominal_and_callable_checks` proves
the accepted pair has distinct internal IDs, then mutates nominal identity,
fields, both declared and actual table IDs, signature, mode, cleanup and the
callable producer across publication and whole/per-unit validation. Restoring
the old raw-ID guard makes this owner's positive publication assertion fail.
The existing tagged-type source owner retains its exact single-LLVM-type and
runtime assertions. `origin_specific_return_types_preserve_imported_copy_and_owned_values`
crosses direct/nested-tagged and Copy/owned-string returns in whole/per-unit
imported builds with bounded child execution. All 62 owned-tagged, 18 producer,
23 resource-ownership and 17 XML owners pass. This maps the extracted canonical,
exact, every-leaf and malformed-graph obligations to executable owners without
a new safety strategy or a performance claim.

## Request 59 aggregate shell ownership follow-up

Status: candidate implemented and owner-verified. The unchanged align-llm client on Align
`882ea5d1` passes the original Copy-call regression but rejects `stats_for` when
it returns a Move record containing owned arrays beside Copy scalar fields read
from a borrowed input. The scalar reads are valid `Shared` producers; merging
that access into the record shell incorrectly downgrades the shell from
`Owned`. This is an implementation correction inside the existing producer
contract, with no language, MIR, ABI, allocation, or package-format change.

| Cell | Required closure | Owner |
|---|---|---|
| Formation and transfer | A materialized local-record shell, including a selected nested record, starts owned only after the field-store graph completely initializes every declared field and every stored field is readable. A direct field store proves that subtree; descendant stores must recursively cover the complete selected record. Move fields contribute their owned/shared access to the shell; Copy fields validate their source but do not define shell ownership. | Reduced `std_xml` fixture with a borrowed-array calculation plus `array_builder.build()` fields, in whole/per-unit execution; the codegen owner also passes a recursively field-built Copy record to `BorrowMut`, while malformed-MIR removal of the Move-field store must fail publication/whole/per-unit validation. |
| Branch, replacement, and return | Root stores preserve their source access; field stores, early returns, and joined replacements retain all reaching validation. A valid field store cannot hide an invalid sibling or later store. | Existing producer equation and record-return mutation owners, plus the unchanged align-llm `stats_for` consumer. |
| Negative ownership | A missing Move field leaves the aggregate shell uninitialized, while a shared Move field makes it shared rather than owned; unreadable, unresolved, duplicated, or malformed Move fields still poison it. One present owned Move field cannot authenticate an incomplete sibling, and an exact Move aggregate store cannot ground its own source cycle. Only Copy values are validation-only inputs. Borrowed destination shells retain their entry authority only while every later Move-field store preserves it. | Focused malformed-MIR mutations remove the sole Move-field store, remove one of two owned Move-field stores, form an exact nested Move self-cycle, or replace the Move field of both an owning local and a `BorrowMut` destination with a shared argument, with publication/whole/per-unit rejection, beside existing `producer_*` owners. |
| Drop and cleanup | Construction keeps current source nulling, hidden owners, recursive Drop, and return cleanup. The validator change neither synthesizes cleanup nor changes lowering. | Whole/per-unit runtime fixture and existing `resource_ownership` suite. |
| Publication | Whole-program validation, per-unit interface publication, and imported consumption use the same aggregate rule. | Reduced differential driver owner and align-llm Request 59 targets. |

The author pass must bind shell seeding to a complete valid aggregate field-store cover relative to the queried local subobject,
propagate only recursively Move field access into the shell, retain READ checks
for every stored field, and preserve selected-field provenance unchanged. No
benchmark is required because this follow-up makes no performance claim.

Author-side closure: a complete Copy local field-store aggregate seeds only its
shell as `Owned`; Move aggregates derive access from their ordinary recursively
Move field dependencies, so an exact Move self-store cannot ground its own
cycle. Incomplete descendant-only construction is explicitly invalid unless a
whole root, out producer, or ancestor field store already initialized the
selected value. All fields retain their existing READ validation. Borrow/BorrowMut/Out parameter
slots manufacture no owned seed, but their recursively Move field stores remain
provenance dependencies so entry authority cannot hide a weaker replacement;
root stores continue to propagate their source access unchanged. The reduced fixture passes native
whole/per-unit execution, and its malformed-MIR mutation replaces the owned
array field with the borrowed input and is rejected in publication, whole, and
per-unit modes. All 19 `producer_*`, 18 `std_xml`, and 23
`resource_ownership` owners pass. The unchanged align-llm
`verification_loop.align` and `alignpack.align` pass their five-unit and
three-unit per-unit checks, respectively. The comprehensive review found the
borrowed-destination dependency gap; the correction and second mutation above
close it without changing the aggregate-shell strategy.
