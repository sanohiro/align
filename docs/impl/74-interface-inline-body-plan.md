# Interface-carried inline bodies

Status: design candidate for
[issue 1066](https://github.com/sanohiro/align/issues/1066) and G9 of the
[vectorization contract](68-vectorization-contract.md). This document is the
public artifact ledger and implementation closure matrix. It implements
nothing. Evidence baseline: Align
`2907d5e56b3bdbc4652787376ec268db5507bdc4`, LLVM 22.1.8, and align-llm
Request 95.

The accepted direction is issue 1066 proposal 2. Release builds do not enable
ThinLTO by default. Instead, a deliberately small non-generic `pub fn` may
publish a checked source body in its unit interface. A consuming unit lowers
that body as an LLVM `available_externally` definition: the ordinary optimizer
can inspect and inline it, while any residual call still resolves to the one
external definition in the producer object.

This is not the existing generic-template mechanism. `generic_body` means that
the importer must instantiate a type-parameterized declaration and recompute
its public facts. Treating presence of that field as "generic" is embedded in
interface validation, effect/provenance seeding, monomorphization and checked
HIR. Overloading it for a concrete function would make a concrete import look
like a template and would silently discard producer-certified facts. Format 15
therefore replaces the function's optional generic string with one explicit
body-kind record: absent, generic template, or concrete inline candidate.

## 1. Public artifact ledger

```text
Promise           Under per-unit compilation at release optimization, every
                  admitted concrete inline candidate is visible to LLVM in the
                  consuming unit as an `available_externally` definition. An
                  admitted direct call therefore has no "definition is
                  unavailable" refusal. LLVM retains its ordinary profitability
                  decision; Align does not promise that every visible body is
                  inlined at every call site or target.

Source surface     No keyword, attribute, annotation, profile default or source
                  convention is added. Admission is compiler-selected and has
                  no semantic effect. Whole-program compilation is unchanged.

Candidate          A candidate is a non-generic `pub fn` whose producer body
                  has passed ordinary semantic, checked-HIR and MIR validation.
                  Its parameters are ByValue Copy values or shared `borrow`
                  values; `out` and `borrow mut` reject. Its result has
                  `ReturnCleanupAbi::None`, no returned borrow or region root,
                  and no mutable-retention or parallel-transfer record. A
                  resource may appear behind shared borrow; no resource hook,
                  owned Move transfer, dynamic cleanup, closure environment or
                  function-valued parameter/result is admitted.

Budget             Admission uses one target-independent checked-HIR budget:
                  at most 24 nodes, counted as one for the function body, each
                  statement and each expression exactly once after checking.
                  Type records, spans and diagnostics cost zero. The exhaustive
                  node walker rejects rather than assigning a default cost to a
                  new statement or expression variant. Changing 24 or changing
                  the count definition changes the compiler build identity and
                  the interface body set.

Body domain        The body is straight-line: expression form or nested blocks,
                  `unsafe` blocks and one explicit return are allowed. `if`,
                  `match`, `else`, `?`, `map_err`, short-circuit control, loops,
                  `break`, early/multiple returns, arenas, tasks, pipelines,
                  lambdas, local function values, assignment, aggregate Move
                  construction, allocation and Drop reject. Scalar/raw
                  literals, parameter reads, arithmetic, comparisons, casts,
                  pure projections, builtin/raw operations and direct calls are
                  allowed. A direct call may target an imported public function,
                  a compiler builtin, or a producer-local C extern captured by
                  the record. A call to any ordinary same-unit function rejects;
                  there is no recursive body closure, cycle or hidden private
                  helper dependency.

Extern closure     Every producer-local extern referenced by the admitted body
                  is recorded structurally with its literal symbol, optional
                  link library, ordered parameter types and result type. Only
                  the existing `extern "C"` FFI-safe type domain is valid.
                  Records are sorted by `(link option, symbol)`, duplicate
                  symbols must be byte-identical and are encoded once, and the
                  imported body may reference no unrecorded extern. The final
                  link already contains the producer unit; repeating its
                  deduplicated link-library requirement in the consumer
                  preserves cached and uncached link parity.

Source closure     The body fragment is the producer's exact item-span source,
                  starting at `fn` and excluding `pub`, as for a generic
                  function. Import validation reconstructs `pub`, parses exactly
                  one function and compares name, empty type-parameter list,
                  ordered parameter modes/types and result against the
                  structured signature. The rechecked body must still satisfy
                  the candidate domain and budget and must infer the recorded
                  effect, return summaries, cleanup, drop-state, transfer and
                  retention facts. It may resolve only its parameters,
                  compiler builtins, imported public surface and its recorded
                  extern closure. A missing/private/extra dependency rejects.

Semantics           The producer object remains the sole external definition.
                  Each consumer gets the same canonical function name with
                  LLVM `available_externally` linkage. LLVM may inline it; if it
                  does not, the available body is discarded and the call binds
                  to the producer's external symbol. Address-taking, indirect
                  calls and debug names therefore retain ordinary external
                  identity. No body is emitted for an unused transitive
                  dependency after LLVM optimization, and no duplicate exported
                  symbol enters an object.

Effects/ownership  Rechecking is not authority to change facts. The consumer
                  compares all producer-certified signature, effect, borrow,
                  region, cleanup, drop-state, parallel-transfer and mutable-
                  retention fields before MIR publication. Calls, unsafe
                  operations, errors and side effects retain source order.
                  Candidate transport adds no allocation, copy, move, Drop,
                  cleanup or lifetime behavior.

Errors/precedence  Format header/version and byte-structural errors precede
                  semantic body validation. Within a body record: invalid body
                  kind, source length/UTF-8/truncation, extern sequence shape,
                  extern type shape/order/duplication, declaration syntax/header,
                  dependency closure, domain/budget, then inferred-fact mismatch.
                  Import failure occurs before consumer MIR, object, cache
                  publication or link side effects. A rejected candidate is not
                  serialized by a trusted producer; malformed serialized input
                  is rejected, never downgraded silently to a bodyless import.

Owner              align_interface owns selection input, format 15, canonical
                  bytes and import validation. align_sema owns exhaustive budget
                  and domain rechecking plus checked-HIR origin. align_mir owns
                  validated concrete-body lowering. align_codegen_llvm owns
                  `available_externally` linkage. align_driver owns transitive
                  reconstruction, external fact/extern closure injection,
                  cache identity, object/link parity and acceptance evidence.

Artifact/cache     Interface format 15 replaces format 14 outright. Function
                  body kind is encoded after `resource_hook_body`: u8 `0`
                  absent; u8 `1`, then the existing u32-length UTF-8 source for
                  a generic template; or u8 `2`, then the same source string,
                  u32 extern count, and canonical extern records. Each extern
                  record is option-link (`00`, or `01` + string), symbol string,
                  u32 parameter count, parameter ITypes in order, then result
                  IType. Integers are little-endian. Struct/sum generic-body
                  fields retain their existing option encoding. Unknown tags,
                  old format 14, invalid UTF-8, truncation, trailing bytes,
                  noncanonical extern order/duplicates and invalid nested IType
                  graphs reject before publication.

Identity           The complete body-kind record and the admission budget
                  version enter `interface_hash`; dependency interface hashes
                  already enter frontend and object keys. Editing an admitted
                  body, crossing the admission boundary, changing a referenced
                  extern signature/link requirement, or changing the budget
                  invalidates consumers. A private-body edit that remains
                  nonadmitted retains current interface-hash stability. Exact
                  edit-and-revert restores the prior hash and cache key. Target,
                  profile, runtime and producer `impl_hash` retain their existing
                  meanings; source bodies are target-independent.

Prerequisite       Issue 1070 is merged. No milestone, ThinLTO, runtime ABI or
                  native package prerequisite remains. Generic template import,
                  per-unit compilation, interface hashing, external effect and
                  provenance facts, and LLVM available-externally linkage are
                  existing foundations.

Acceptance         Section 3 owns format/malformed validation, eligibility,
                  producer/consumer semantic parity, direct and extern-wrapper
                  bodies, whole/per-unit results, release IR/remarks, symbols,
                  link behavior and cache invalidation. The provider target is
                  correctness. align-llm disassembly/site counts are external
                  adoption evidence and do not gate Align.

Measurement        On the pinned align-llm corpus, rebuild without ThinLTO and
                  report calls to `cached_f16`, `handle_absent`, `null_handle`,
                  `context_open` and `graph_new`, plus the total Align-to-Align
                  sites whose callee has at most eight native instructions.
                  The requested evidence target is fewer than 100 such sites
                  from 686. It is not a language-wide latency or code-size
                  promise and is not a correctness gate.

Mirrors            This plan, 17-library-boundary-prerequisites.md, the G9 row
                  and ownership map in 68-vectorization-contract.md, 10-cache-
                  first-optimization.md where cache behavior is recorded, and
                  HANDOFF.md. No language syntax or semantic contract changes,
                  so draft.md, language-spec.md, design-notes.md and
                  open-questions.md receive no restatement.
```

The canonical standalone function-body records use an empty source only to pin
the codec; semantic import validation rejects those empty declarations:

```text
Absent:                         00
Generic empty source:           01 00000000
Inline, empty source/externs:   02 00000000 00000000
Inline, one libc extern `f`:    02 00000000 01000000
                                00
                                01000000 66
                                01000000
                                00 03000000 693634 00000000
                                00 03000000 693634 00000000
```

Here `00 03000000 693634 00000000` is the format-15 encoding of
`IType::Named { path: "i64", args: [] }`. Codec owners construct the semantic
records and compare exact bytes, then
independently decode literal bytes and compare semantic records. Roundtrip alone
is insufficient.

## 2. Implementation closure matrix

The implementation updates this matrix before code review. One parameterized
owner may close several cells when it would fail for every listed defect.

| Cell | Implementation obligation | Owner evidence |
|---|---|---|
| Formation/selection | select only validated non-generic public bodies satisfying every signature, provenance, ownership, domain and 24-node rule; crossing each boundary removes the body without changing program semantics | interface/sema eligibility table with one positive and one negative per rule; exhaustive node-variant tripwire |
| Interface bytes | emit format 15 and exact 0/1/2 body kinds plus canonical extern closure; format 14 and every malformed ordering/tag/length/type combination reject | independent semantic-to-byte and byte-to-semantic goldens, mutation/depth/trailing corpus |
| Source reconstruction | parse exactly one reconstructed declaration, match its structured header and resolve only admitted public/builtin/extern dependencies | forged name/type/mode/result/body/dependency cases; private/same-unit helper negatives |
| Producer facts | rechecked concrete body agrees exactly with effect, return borrow/region/cleanup, drop-state, transfer, retention and resource-hook facts | one mutation per field; whole producer/importer twins |
| Construction/lowering | imported inline candidates become checked HIR/MIR definitions with an explicit imported-inline origin; ordinary bodyless imports and generic monomorphs retain their existing paths | checked-HIR and MIR structural owners plus generic/bodyless negative controls |
| Calls/function values | direct calls may see the body; residual and indirect/address uses retain the producer external symbol identity | optimized/unoptimized direct, function-value, callback and address-bearing object/IR owners |
| Extern/link closure | raw/builtin and captured C-extern wrappers compile in consumers; ABI, symbol and link library are exact and deduplicated | null/is-null, widening C-return and linked-library fixtures; forged/missing/conflicting extern negatives |
| Ownership/cleanup | shared resource borrow is admitted; ByValue Move, mutable/out, owned return, cleanup, returned view/region, replacement and Drop paths are rejected from transport and keep external-call behavior | parameter/result matrix and allocation/Drop counters |
| Control flow | straight-line expression/block/unsafe/return forms pass; if/match/else/?/map_err/short-circuit/loop/break/early return/arena/task/pipeline/lambda/assignment fail admission but still compile through the ordinary external path | parameterized syntax/HIR matrix with identical whole/per-unit runtime results |
| Generic separation | generic templates keep tag 1, monomorphization and recomputed facts; concrete inline tag 2 never enters generic worklists or disappears from external fact maps | generic/concrete sibling fixture and forged tag/type-parameter mismatches |
| LLVM/linkage | consumer definition is `available_externally`, producer definition is external, no consumer object exports/defines a duplicate, and a non-inlined call links to the producer | release/dev IR, `llvm-nm`, forced residual-call executable and multi-consumer link owners |
| Optimization evidence | a tiny scalar call is absent in release consumer IR/native output without ThinLTO; a stage call has no definition-unavailable remark; profitability negative remains legal | two-unit release, pipeline/explain-opt and deliberately non-inlined control |
| Cache/determinism | admitted edit, eligibility crossing, extern edit and budget-version change invalidate exact dependent keys; nonadmitted private edit does not; revert restores keys/bytes; warm/cold and jobs=1/N agree | unit-cache edit matrix, two-build byte identity and scheduling matrix |
| Malformed/recovery | every invalid interface or forged checked-HIR origin fails before MIR/object/cache/link publication without panic; no malformed candidate is downgraded to external silently | codec/import/HIR mutation corpus and no-artifact assertions |
| Whole/per-unit/profile | whole-program output is unchanged; per-unit dev/release/fast/small/tiny preserve results, with release body visibility independent of ThinLTO and target CPU | profile matrix on Linux x86_64, Linux ARM64 and macOS Apple Silicon |

This crosses interface, sema, HIR, MIR, LLVM and driver layers. It is one
capability PR after this reviewed design: splitting body publication from
consumer lowering would publish an unusable authority, while splitting extern
closure or linkage would make the first useful native wrapper either fail to
compile or define duplicate symbols. The implementation is expected to exceed
roughly 1,000 changed hand-written lines; the single boundary avoids duplicating
the same body-kind, fact-parity and cache proof across dormant intermediates.

## 3. Provider acceptance corpus

The focused owner is
`crates/align_driver/tests/interface_inline_bodies.rs`, supported by interface,
sema, MIR and LLVM unit owners.

1. A two-unit scalar fixture carries and inlines `pub fn tiny(x: i64) -> i64 =
   x + 7` in release without ThinLTO. A larger sibling remains an external call.
2. A shared-borrow resource query calling an imported public function, a
   `raw.null()` / `raw.is_null()` pair, and a direct C-extern wrapper cover the
   client shapes. An `i32` extern result widened to `i64` remains visible for
   tail-call and ordinary inlining inspection.
3. A pipeline-stage fixture has no definition-unavailable remark naming its
   admitted callee and retains identical output in whole/per-unit builds.
4. Producer and two consumers prove one external producer symbol, no exported
   consumer duplicate, valid function-value/address use and a residual call that
   links back to the producer.
5. The eligibility and malformed matrices exercise every row in §2. Format 15
   exact bytes, hash changes, cache cold/hit/edit/revert and deterministic
   parallel builds are mandatory.

The align-llm consumer then pins the merged Align revision, rebuilds its release
binary without ThinLTO, runs `scripts/bench-runtime-sampler` and
`scripts/bench-runtime-greedy`, inspects disassembly/remarks and reports the call
site census. That work remains consumer-owned.

## 4. Deliberate exclusions

No default ThinLTO, source `inline`/`always_inline` keyword, target-specific
budget, profile-dependent interface, arbitrary private-helper closure, recursive
body transport, generic-policy change, serialized HIR/MIR/LLVM bitcode,
cross-version reader, optimizer-result guarantee, new runtime ABI, native symbol
alias, reflection or dynamic loading is added. A body rejected by admission
continues to compile and call exactly as it does today.

## 5. Author-side consistency pass

- The ledger separates the promised available definition from LLVM's
  profitability decision and makes the client site count evidence only.
- Every transported scalar, tag, sequence and nested extern record has a fixed
  order, width, malformed-input rule and independent golden requirement.
- Concrete inline bodies retain producer-certified effects, ownership, cleanup,
  provenance and linkage; no body presence is treated as genericity.
- The 24-node rule is target-independent, exhaustive and versioned in identity.
- Direct native externs have a complete symbol/signature/link closure; private
  Align helper graphs and recursive closure are excluded.
- Whole-program, per-unit, every profile, cache edit/revert, function-value,
  residual-call and multi-consumer cases appear in the closure matrix.
- No later milestone or language change is consumed. Issue 1070 is already
  merged; the implementation needs no runtime ABI addition.
