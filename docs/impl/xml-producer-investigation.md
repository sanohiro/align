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
