# XML producer validation: investigate, then repair

Status: RESOLVED implementation investigation, recorded 2026-09-05 and closed
2026-09-06 at the owner's request. This is an implementation issue register,
not a new language contract. It records the complete finding set, classification,
repair boundary, and owner evidence in one place rather than as isolated review
rounds.

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

The latest full review reported the following five P2 findings. Earlier reader
use-after-free and native callback identity findings motivated the committed
redesign; this register does not claim those earlier fixes can be removed safely.

| ID | Classification | Resolution |
|---|---|---|
| X1 | Source regression introduced by producer validation. | Captured return roots now follow the authenticated callable environment through copied, stored, and joined closures. Missing fields, wrong field types, and forged capture indices fail closed. Direct parameter roots remain unchanged. |
| X2 | Source regression plus sibling malformed-MIR gaps. | AoS string-key group writers now preserve key provenance and own numeric results. The same inventory validates multi-aggregate, dictionary encode, numeric gather, dictionary lookup, output identity, length, field, nominal type, arity, and alias relations. The numeric gather mutation is owned by the numeric consumer instead of an unrelated key consumer. |
| X3 | Source regression introduced by producer validation. | `XmlParse` now certifies only its exact builtin `Error` payload in addition to the whole result and reader payload. Input ownership and cleanup identity checks remain mandatory. |
| X4 | Source regression plus type-class malformed-MIR gaps. | Numeric equations cover the source-admitted scalar and vector arithmetic, vector/scalar broadcasts, comparisons, lane masks, vector selection, scalar unary operators, and exact widths. Scalar-left arithmetic was rejected as an invalid fixture because sema does not admit it under the vector expectation; the validator was not widened to manufacture a new language rule. |
| X5 | Malformed-MIR contract gap. | `MaybeAbsent` becomes present only with the exact discriminator guard. Statically inactive guarded payloads remain absent. Option, Result, enum, and optional-reader joins have positive and guard-removal/wrong-predicate mutations. |

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

The repair is roughly 1,000 changed hand-written lines because the five findings
and the DB regression are one shared validator failure domain. Splitting its
producer equations from the cross-path malformed-MIR owners would create a
dormant producer/consumer chain and duplicate the same certification proof,
preflight, and DB boundary verification. Keeping one coherent repair has less
integration risk.

Two observations remain explicitly unconfirmed and outside this repair:

- whether forged unsigned scalar negation or Bool-conditioned vector selection
  can be reached from source-admitted MIR; and
- whether a legitimate lowered direct program call can carry capture roots.

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

The exact committed SHA still requires the changed-slice review, full local DB
verification, and `scripts/pre-pr.sh`. Merge and versioned release remain the
terminal workflow; this record does not authorize another feature or consumer
repository work.

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
```

The required final DB gate uses `DOCKER_HOST=unix:///var/run/docker.sock` on this
host and `scripts/db-verify-local.sh`. The failed discovery run is preserved in
the evidence directory. Respect the existing thirty-minute run and fifteen-minute
binary budgets rather than increasing them.
