# Runtime boundary effects: per-symbol memory model, cold paths, per-element primitives

Status: plan of record for issues
[1071](https://github.com/sanohiro/align/issues/1071),
[1072](https://github.com/sanohiro/align/issues/1072) and
[1074](https://github.com/sanohiro/align/issues/1074). It folds
[1073](https://github.com/sanohiro/align/issues/1073) part 2 (the `BufferPut`
effects row) and [1069](https://github.com/sanohiro/align/issues/1069) part 4
(the rt-LTO guarded-set admission criterion), per those issues' 2026-09-18
triage comments. Nothing here is implemented.

[Plan 68](68-vectorization-contract.md) is the public-contract ledger and names
this document as the implementing plan for guarantee G5. Unlike
[plan 69](69-loop-facts-plan.md), which implements G1–G3 without adding a public
surface, **this plan does add one**: a per-symbol memory-effects contract for
every runtime ABI symbol, recorded in
[plan 20](20-runtime-abi-ledger.md), which is that document's declaration
authority. The large design authoring gate therefore applies in full, and §2 is
its public-contract ledger.

[Plan 20](20-runtime-abi-ledger.md) is the primary source of truth. Every row
this plan adds lands there; this document fixes the schema, the closed class
set, the derivation, the admission predicate, and the machinery that makes a
wrong row fail rather than miscompile.

Evidence baseline is plan 68's: Align `8c8bfbc7a3169e84ecc8415f5149ab8c61afe863`
(alignc 0.7.5, LLVM 22.1.8), Apple M1, `--profile release`, default
`--target-cpu baseline`. Code locations and counts below were re-verified
against `956f3869`, after PR #1091.

## 1. Scope

### 1.1 What this plan owns

```text
PR 1   1071 + 1073 part 2 + 1069 part 4
       one per-symbol effects record for all 464 runtime ABI rows, derived from
       a closed classification; the exact attribute set each class emits; the
       rt-LTO admission predicate stated over those classes; the fail-closed
       machinery that detects a wrong row

PR 2   1074 Tiers 1-3
       one cold-path model for Result/?: a MIR ExceptionalEdge record, !prof
       weights on every edge it names, cold inference over the MIR call graph,
       and the fail family's effects row from PR 1. Metadata only

PR 3   1072
       an inline fast path for the per-element array_builder primitives with a
       visible slow path, the exact runtime layout pins that make it sound, and
       the release-profile decision for the runtime archive
```

The three land in that order. PR 2 Tier 3 consumes PR 1's record. PR 3's fast
path is measurable only once PR 1 lets LLVM hoist its guard loads out of the
push loop, so PR 3 follows PR 1; PR 3 does not depend on PR 2.

### 1.2 Non-goals

```text
no new language surface  no annotation, keyword, attribute or pragma reaches
                         source. Every fact is a property of a Rust function
                         the compiler ships or of the compiler's own lowering
no source-minted facts   63-codegen-performance-audit.md:216-219 forbids facts
                         derived from source spelling; nothing here is
                         (docs/impl/63 §compliance, and §6 below)
no llvm.assume           and no custom pass order
                         (docs/open-questions.md, "attributes and flags first")
no alwaysinline          and no inlinehint, on any row or any Align function
no allocator change      libc stays (12-pipeline-...-audit.md:912)
no diagnostic change     PR 2 changes no Err value and no trap text, byte for
                         byte. PR 3 changes no observable container behaviour
no reduced optimization  21-build-perf-plan.md's principle is preserved: PR 3
                         raises the local runtime artifact to the shipped
                         artifact's level and lowers nothing
no PGO dependency        PR 2's model works with no profile data
no new ABI symbol        PR 1 and PR 2 add none. PR 3 adds none: the fast path
                         is codegen, the slow path is the existing symbol
```

### 1.3 What the issues claim, cross-checked against the tree

Four claims are restated by the issues and are re-verified here, because three
of them have moved since the issues were written.

```text
VERIFIED   RuntimeAbiShapeSpec holds one effect bit (runtime_abi.rs:177-184).
           At 956f3869 there are 141 shapes, 104 with fn_attrs: &[], 3 with
           memory_argmem_read: true (A01, A26, A82), 131 with no read_ptr_params
VERIFIED   the registry is 446 keyed + 18 unkeyed = 464 base rows over those
           141 shapes (validate_registry, runtime_abi.rs:2330), and the golden
           at tests/golden/runtime_abi_declarations.txt carries 464 declare
           lines over exactly five attribute groups #0-#4
CORRECTED  1069 proposal 2 asked for remove_attributes to cover string
           attributes. PR #1091 did not do that. remove_attributes
           (runtime_abi.rs:313) still owns exactly the row's *enum* contract and
           still runs POST-merge, from normalize_linked_rt_lto_guarded_
           definitions (lib.rs:7482, called at lib.rs:7686). The string half
           became a separate module-scoped sweep, shed_rt_lto_target_bound_
           attributes + verify_rt_lto_target_independence (lib.rs:7529, :7546),
           which runs PRE-merge on the incoming artifact. The two halves have
           different scopes, different times and different owners; §3.6 R4 is
           the matrix cell that keeps them consistent under PR 1
CORRECTED  1072 proposal 4, "stop shipping an untuned runtime archive", is
           already satisfied for the shipped artifact: release.yml builds
           align_runtime with --profile dist (thin LTO, one codegen unit) in
           every release phase. The real gap is local; §5.4 records the decision
CORRECTED  1072 proposal 3, "extend the --rt-lto set to the per-element class",
           FAILS the admission predicate this plan states (§2.4). The per-element
           rows are allocator-reaching. §5.3 records the refusal and its reason
```

### 1.4 Which guarantee each PR closes

Plan 68 G5 reads: *"Every runtime primitive that can appear in a loop has an
effects record and either an inline fast path or a vector form"*, promising
*"each runtime ABI symbol carries a complete memory-effects attribute;
per-element primitives have a visible inline fast path and a visible slow path;
`Result`/`?` failure edges are cold with one model"*. PR 1 closes the first
clause, PR 3 the second, PR 2 the third. No clause is widened and none is
reinterpreted.

## 2. Public-contract ledger

Every public surface this plan introduces, one row each. This ledger is
authoritative while drafting; §6 is the author-side consistency pass over it.

### 2.1 The effects record

```text
Surface           RuntimeEffects, one per RuntimeAbiId, in
                  crates/align_codegen_llvm/src/runtime_abi.rs

Exact type        struct RuntimeEffects {
                      class:   EffectClass,
                      argmem:  ArgMem,
                      params:  &'static [ParamEffect],
                      retains: &'static [u32],
                      releases: Release,
                  }
                  enum ArgMem { Unstated, None, Read, Write, ReadWrite }
                  struct ParamEffect { ordinal: u32, mode: ParamMode }
                  enum ParamMode { Read, Write, ReadWrite, Opaque }
                  enum Release { NotAFree, HandleOnly, Region }
                  EffectClass is the closed eleven-variant set of §2.2

Inputs, defaults  none and none. The record is produced by a total match over
                  RuntimeAbiId with no `_` arm, so a new RuntimeKey does not
                  compile until it is classified. "Unclassified" is not
                  expressible; there is no default row
Errors            validate_registry returns the existing Err(String) for every
                  structural violation of §2.3. Codegen has no runtime failure
                  mode: the record is a `&'static` table
Ownership         'static. No allocation, no interior mutability, Copy
Allocation        none, at any point
Owner             compiler (align_codegen_llvm). The *facts* are properties of
                  align_runtime's Rust bodies; align_runtime owns the
                  machine controls of §3.5
Artifact/cache    compiler_build_id (cache.rs:135, the hash of the alignc
                  binary bytes) changes, so every codegen-family entry misses
                  exactly once and then hits. No key COMPONENT is added.
                  rt_lto_digest (cache.rs:177) is unchanged: it hashes
                  build.rs's baked bitcode, and PR 1 admits no new guarded row.
                  No unit interface field changes, so no importer is
                  invalidated beyond that one build-id miss
Prerequisite      none. M0-M15 are complete and plan 68 §3.1 names the
                  existing plan 20 inventory as G5's only prerequisite
Acceptance        runtime_effects_registry_is_total_and_structurally_valid;
                  the extended declaration golden; the §3.5 machine controls
Benchmark         none. PR 1 makes no performance or resource promise. The
                  client numbers in 1071 are evidence, not a gate
Mirrors           20-runtime-abi-ledger.md gains the classification column and
                  the replacement attribute-group table (primary).
                  68-vectorization-contract.md already names this plan for G5
                  and needs no edit. 07-roadmap.md Slice 5A gains one pointer.
                  HANDOFF.md gains one paragraph. draft.md and
                  docs/language-spec.md are unchanged: no language surface
```

### 2.2 The classification column

```text
Surface           one token per row in plan 20's declaration tables, and the
                  fourth |-separated field of each golden line

Exact schema      EffectClass, a closed set of exactly eleven tokens:

  PureScalar      no pointer parameter; touches no memory; terminates
  PureArgRead     reads only through pointer parameters; no allocator, no
                  process state; terminates
  ArgWrite        reads and writes only through pointer parameters; never
                  reaches the allocator; terminates
  ArgWriteAlloc   reads and writes through pointer parameters and may reach the
                  allocator; every object it can free is argument-derived
  AllocNew        allocator-class constructor returning a fresh owned handle
  FreeLocal       null-safe deallocator; every object it releases is
                  argument-derived and its Drop touches no process state
  DispatchCache   otherwise PureArgRead, but reads a process-global CPU-feature
                  or dispatch cache
  HostState       reads or writes process/OS state: files, sockets, env, clock,
                  RNG, process control, threads, or a region teardown
  Callback        calls an Align function pointer the caller supplied
  Foreign         crosses into a third-party native library
  FailNoReturn    diverges through the runtime abort family

Inputs, defaults  none and none; see §2.1
Errors            §2.3's structural rejections
Ownership         'static token; no value crosses any boundary
Allocation        none
Owner             compiler for the token, align_runtime for the property it
                  asserts
Artifact/cache    as §2.1. The token is part of the golden, which is a test
                  artifact and not a cache input
Prerequisite      none
Acceptance        the golden (one line per row, class included), plus
                  runtime_effects_class_mutation_changes_the_golden
Benchmark         none
Mirrors           20-runtime-abi-ledger.md (primary)
```

The class fixes everything except the argument-memory component and the
per-ordinal detail, which the row supplies. This is the whole point of keying
by class rather than by attribute tuple: the unit a human reviews against a
Rust body is one token, and 464 tokens cannot drift into 464 different attribute
conventions.

**Exact derivation.** `apply_attributes` emits exactly this and nothing else:

| class | `memory(...)` | function attributes | return | pointer parameters |
|---|---|---|---|---|
| `PureScalar` | `memory(none)` | `nounwind nofree nosync willreturn` | — | none exist |
| `PureArgRead` | `memory(argmem: read)` | `nounwind nofree nosync willreturn` | — | `readonly captures(none)` per `params` |
| `ArgWrite` | `memory(argmem: <argmem>)` | `nounwind nofree nosync willreturn` | — | `readonly`/`writeonly` per `params`, `captures(none)` unless in `retains` |
| `ArgWriteAlloc` | `memory(argmem: <argmem>, inaccessiblemem: readwrite)` | `nounwind` | — | `readonly`/`writeonly` per `params`, `captures(none)` unless in `retains` |
| `AllocNew` | `memory(argmem: <argmem>, inaccessiblemem: readwrite)`, and `memory(inaccessiblemem: readwrite)` when `argmem == None` | `nounwind nofree` | `noalias` | `readonly captures(none)` per `params` |
| `FreeLocal` | `memory(argmem: readwrite, inaccessiblemem: readwrite)` | `nounwind` | — | none |
| `DispatchCache` | withheld | `nounwind nofree nosync willreturn` | — | `readonly captures(none)` per `params` |
| `HostState` | withheld | `nounwind` | — | `readonly captures(none)` only for a `params` ordinal explicitly recorded `Read` |
| `Callback` | withheld | `nounwind` | — | none |
| `Foreign` | withheld | `nounwind` | — | none |
| `FailNoReturn` | `memory(inaccessiblemem: readwrite)` | `nounwind noreturn cold` | — | none |

*Withheld* means no `memory` attribute is emitted, which is LLVM's most
conservative state: the call may read and write all memory. Withholding is the
fail-open-into-correctness direction, and it is what makes an over-conservative
classification cost optimization rather than soundness.

Five facts in that table need their reason on the record.

**`nounwind` is universal.** Every base row is a Rust `extern "C"` function, and
in edition 2024 an unwind out of `extern "C"` aborts. No runtime export declares
`extern "C-unwind"`. The class table therefore emits `nounwind` for all eleven
classes, and an owner asserts that no `align_rt_*` export uses `C-unwind`, so
adding one fails the gate rather than silently invalidating 464 declarations.

**`nosync` is never emitted with the allocator.** `malloc`/`free` synchronize,
so `ArgWriteAlloc`, `AllocNew` and `FreeLocal` carry neither `nosync` nor
`willreturn` — OOM aborts, which is divergence.

**A reallocating row is `ArgWriteAlloc`, never `AllocNew`.** `AllocNew` fixes
`nofree` and a `noalias` return, and both are false for
`align_rt_realloc(ptr, i64)`: it frees its argument and its result may be that
same pointer. V7 enforces the `noalias` half structurally, and the class
definition — *constructor returning a fresh owned handle* — excludes it.

**`FailNoReturn` gets `memory(inaccessiblemem: readwrite)`.** The fail family
takes no pointer argument and writes a message through Rust's global stderr
machinery, which is exactly what LLVM's own `inaccessiblemem` models for
`abort`/`exit`/stdio. This is the half of 1074 whose loss is measured: a kernel
with a surviving bounds check collapses to `{ nounwind }` today because the
declaration states nothing.

**`HostState` keeps memory withheld in this wave.** An I/O row could in
principle carry LLVM's `memory(argmem: ..., inaccessiblemem: readwrite)` stdio
model — file and socket *contents* are not LLVM memory, so only process memory
is at issue. It is withheld anyway because a host-state body may reach `std`
machinery whose process-global state the program can also read through another
row (`env_set`/`env_get` is the clearest pair), and no measured evidence in
1071, 1072, 1073 or 1074 comes from an I/O row. Narrowing `HostState` per symbol
later is a follow-up under this same machinery, not a reopening of the schema.

**A region teardown is `HostState`, not `FreeLocal`.** `align_rt_arena_alloc`
and `align_rt_tg_alloc` are declared `noalias ptr` (A45), so an arena-allocated
object is, by declaration, *not* derived from the arena argument. If
`align_rt_arena_end(arena)` claimed `memory(argmem: readwrite,
inaccessiblemem: readwrite)`, LLVM would be entitled to sink a store to that
`noalias` object past the teardown: a store after free. `align_rt_arena_end`,
`align_rt_arena_reset` and `align_rt_tg_end` are therefore `HostState` with
`releases: Region`, and §2.3 rejects `FreeLocal` combined with `Region`
structurally. `align_rt_buffer_free` and the rest of the A62 family are
`FreeLocal` with `releases: HandleOnly`, because every view they invalidate
(`align_rt_buffer_bytes`, A72) was produced from the same handle through a
non-`noalias` return.

### 2.3 Structural validity

```text
Surface           validate_registry (runtime_abi.rs:2330), extended

Exact signature   unchanged: fn validate_registry() -> Result<(), String>
Inputs, defaults  none; it reads only the 'static tables
Errors            one Err(String) per violated rule, in this deterministic
                  order, reporting the first violation with its symbol.
                  V12 is checked after V1-V11 because it reads a record V1
                  must already have proved present:

  V1  every RuntimeAbiId has a RuntimeEffects record       (totality)
  V2  a params ordinal names a NativeType::Ptr of that row's shape
  V3  a retains ordinal names a NativeType::Ptr of that row's shape
  V4  a params mode of Write or ReadWrite requires argmem in {Write, ReadWrite}
  V5  a params mode of Read requires argmem in {Read, ReadWrite}
  V6  a withheld class (DispatchCache, HostState, Callback, Foreign) requires
      argmem == Unstated; a memory-claiming class forbids Unstated
  V7  return_noalias is set for AllocNew rows and for no other class
  V8  FailNoReturn forbids willreturn; it is the only class that emits noreturn
  V9  releases == NotAFree for every class but FreeLocal and HostState;
      FreeLocal forbids releases == Region
  V10 a row with no pointer parameter forbids a nonempty params or retains,
      and PureScalar requires exactly that shape
  V11 the existing key-count, base-count, bijection and key-symbol rules
  V12 every row listed in the --rt-lto guarded set satisfies P1-P5 of §2.4,
      and no row satisfying none of them is listed

Ownership         none; no value escapes
Allocation        the existing error String only, on failure
Owner             compiler
Artifact/cache    none; it runs before any LLVM value exists
Prerequisite      none
Acceptance        runtime_effects_registry_is_total_and_structurally_valid,
                  one negative per rule V1-V10 built by mutating a copied row
Benchmark         none
Mirrors           20-runtime-abi-ledger.md "Machine gates"
```

### 2.4 The rt-LTO admission predicate

```text
Surface           RuntimeAbi::is_rt_lto_guarded (runtime_abi.rs:337), restated
                  as a predicate over the effects record rather than a
                  four-variant match

Exact predicate   a row is admissible to the --rt-lto guarded set exactly when
                  ALL of:

  P1  class in {PureScalar, PureArgRead, ArgWrite}
  P2  retains is empty
  P3  its Rust body's call graph is closed inside the baked artifact:
      no cross-crate call, no allocator, no panic/abort machinery
  P4  the baked definition carries no semantic string attribute
      (verify_rt_lto_target_independence already rejects every string
      attribute, so P4 is machine-checked as a byproduct)
  P5  the post-`rustc -O -Ccodegen-units=1` body is at most 200 LLVM
      instructions, measured from the baked artifact
  P6  a recorded paired measurement on both supported architectures shows no
      regression at the default --target-cpu

Inputs, defaults  none. P1-P5 are static and machine-checked; P6 is a recorded
                  measurement, and a row without one is not admitted
Errors            a row failing P1-P5 while listed as guarded fails
                  validate_registry (rule V12). P6 has no compile-time form:
                  it gates the PR that adds the row, and its measurement is
                  recorded in plan 20 beside the row
Ownership         none
Allocation        none
Owner             compiler for P1-P5, the admitting PR for P6
Artifact/cache    rt_lto_digest changes exactly when the guarded set or the
                  baked source changes. This wave admits no row, so it is
                  unchanged. build.rs bakes str_prims.bc with its own
                  `rustc -O -Ccodegen-units=1` invocation (build.rs:86-101),
                  independent of any cargo profile, so §5.4's decision does not
                  touch it either
Prerequisite      1069 parts 1-3, shipped in PR #1091. Without target
                  independence a merged body does not inline at the default
                  --target-cpu on aarch64, so P6 could not be measured honestly
Acceptance        rt_lto_admission_predicate_matches_the_guarded_set, plus one
                  negative per P1-P5
Benchmark         P6 is the benchmark, and it is an admission gate for a row,
                  never a correctness gate
Mirrors           20-runtime-abi-ledger.md, whose rt-LTO paragraph currently
                  names the four symbols as a list
```

One line: **a runtime row may join the `--rt-lto` guarded set exactly when its
effect class is `PureScalar`, `PureArgRead` or `ArgWrite`, it retains no pointer
argument, its baked body is crate-closed, carries no string attribute and stays
within the instruction budget, and a recorded paired measurement shows no
regression at the default `--target-cpu` on both architectures.**

Applied to the current set: `StrEq`, `StrStartsWith`, `StrEndsWith` and
`StrEqIgnoreCase` are `PureArgRead` and pass every clause. `StrCmp` is also
`PureArgRead` and passes P1–P5; it is excluded by **P6**, its measured 0.72×
regression, which is exactly the outcome 1069 asks for — a row that failed the
criterion, not evidence against having one.

Why the allocator classes are excluded, stated once. A merged definition is set
`internal` and has its curated attributes shed, on the theory that LLVM
re-derives them from the now-visible body. That holds for a leaf predicate. It
is false for an allocator-reaching body: the growth path calls
`align_rt_realloc` and the runtime's abort machinery, neither of which is in the
baked artifact, so the merged body keeps an opaque external call and LLVM
re-derives *less* than the declaration promised. Merging such a row therefore
trades a precise declaration for an imprecise definition. `array_builder_push`
is `ArgWriteAlloc` and is excluded for exactly this reason; §5.3 records the
resulting refusal of 1072 proposal 3 and what replaces it.

### 2.5 The `ExceptionalEdge` record (PR 2)

```text
Surface           MirFn::exceptional_edges, in crates/align_mir

Exact type        struct ExceptionalEdge {
                      block: BlockId,
                      unlikely: Successor,     // Then | Else
                      kind: ExceptionalKind,
                  }
                  enum ExceptionalKind {
                      ResultPropagate, BoundsCheck, RangeCheck,
                      Utf8Boundary, DivByZero, LenMismatch,
                  }
                  MirFn gains `exceptional_edges: Vec<ExceptionalEdge>`

Inputs, defaults  recorded by the lowering that CREATES the edge, at the six
                  sites that exist today: the `?` desugaring
                  (align_mir/src/lib.rs:22022, whose NOTE this replaces),
                  emit_bounds_check (:11889), emit_range_bounds_check (:12627)
                  and the A-range arm (:12055, :12105), emit_vec_bounds_check
                  (:12612), the UTF-8 boundary arm (:12745), the runtime
                  divide-guard arm, and the length-mismatch arm (:15215).
                  It is never inferred from a terminator's shape afterwards.
                  The default is an empty vector: an edge nobody recorded is
                  an ordinary branch
Errors            MIR validation rejects an ExceptionalEdge whose block is not
                  a Term::Branch, whose block id is out of range, or that
                  duplicates an earlier entry for the same block
Ownership         owned by the MirFn; Copy elements; no borrow escapes
Allocation        one Vec per function, sized by the number of recorded edges
Owner             align_mir produces it, align_codegen_llvm consumes it
Artifact/cache    object content changes; compiler_build_id covers it. The
                  unit INTERFACE does not carry it, so no importer is
                  invalidated and no key component is added
Prerequisite      none
Acceptance        one codegen owner per ExceptionalKind asserting the exact
                  printed metadata; one MIR owner per recording site; negative
                  owners for an ordinary if/match branch and for an else-unwrap
Benchmark         none. PR 2 promises layout and metadata, not throughput
Mirrors           04-mir.md §2.1, whose `?` pseudocode currently ends with the
                  sentence "codegen can place the cold edge in a
                  separate/low-priority section" and now names the record
```

The emitted metadata is exactly LLVM's `__builtin_expect` pair:

```llvm
br i1 %is_ok, label %ok, label %err, !prof !0
!0 = !{!"branch_weights", i32 2000, i32 1}
```

`2000`/`1` are LLVM's `LikelyBranchWeight`/`UnlikelyBranchWeight`, with the
unlikely weight on the successor the record names. The printed form is pinned
the way the `memory(...)` bitmask is pinned, so an LLVM change to those
constants fails an owner rather than silently producing a different layout.

### 2.6 `cold` inference (PR 2 Tier 2)

```text
Surface           the "cold" function attribute on Align-generated definitions

Exact rule        a function is cold when BOTH hold:
                  (a) every one of its return paths constructs an Err of the
                      function's return type, or diverges; and
                  (b) every call site that can reach it is dominated by the
                      unlikely successor of an ExceptionalEdge.
                  Computed as a least fixed point over the MIR call graph, so a
                  helper's helper and the error-formatting chain below it are
                  classified by the same rule rather than by a list

Inputs, defaults  the MIR call graph of the compilation unit set, plus §2.5's
                  records. The default is NOT cold
Errors            none; the analysis is total and fails closed to not-cold
Ownership         a bool per MIR function; nothing escapes
Allocation        one worklist and one bitset per compilation
Owner             align_mir computes, align_codegen_llvm emits
Artifact/cache    object content changes; compiler_build_id covers it. Not an
                  interface field. Whole-program and per-unit builds MAY
                  disagree: condition (b) is unprovable for a `pub fn` that
                  another unit can call, so per-unit fails closed and emits
                  nothing for it. cold is a hint with no semantic content, so
                  the two builds differ in object bytes and in nothing else.
                  This is stated because plan 20's machine gates otherwise
                  require whole-program and per-unit agreement on attributes,
                  and this is the one deliberate exception
Prerequisite      §2.5
Acceptance        a positive owner on a fixture whose error-only helper is
                  reached solely from an Err arm; a negative owner on a helper
                  also reachable from a success path; a negative owner on a
                  cross-unit `pub fn` in per-unit mode
Benchmark         none
Mirrors           04-mir.md §2.1; 05-backend-llvm.md is unchanged, because the
                  attribute is emitted through the existing add_enum_attr path
```

### 2.7 The inline fast-path contract (PR 3)

```text
Surface           codegen for Rvalue::ArrayBuilderPush with a statically sized
                  scalar element

Exact contract    for an element of statically known width W in the scalar set,
                  codegen emits

                    if arena == null && elem_size == W && len < cap {
                        store v, data + len * W
                        len = len + 1
                    } else {
                        call align_rt_array_builder_push(b, bits)
                    }

                  The slow path is exactly one call to the existing symbol and
                  is the only path that can grow, use arena chunks, or handle a
                  string element. No new runtime symbol, no alwaysinline, no
                  source annotation

Inputs, defaults  the builder handle and the value, as today. There is no
                  option, flag or environment input; the fast path is emitted
                  whenever the element is a statically sized scalar
Errors            none at compile time. Behaviour is identical to the call on
                  every input, including a null handle, which fails the
                  `arena == null && elem_size == W` guard only if the load
                  itself is legal — so the fast path is emitted only where the
                  handle is already known non-null by the existing lowering,
                  and otherwise the call is emitted unchanged
Ownership         unchanged. The builder is borrowed; the element is copied by
                  value; no element is retained by the fast path that the slow
                  path would not retain
Allocation        the fast path allocates nothing; that is its definition. The
                  slow path allocates exactly as today
Owner             align_codegen_llvm emits, align_runtime owns the layout
Layout pins       ArrayBuilder is already #[repr(C)] with raw fields
                  (align_runtime/src/lib.rs:17582). Its existing assertion is a
                  BOUND (size <= 64, align <= 16, :17594), which does not pin an
                  offset. PR 3 replaces it with exact pins:
                    offset_of!(ArrayBuilder, data)      == 0
                    offset_of!(ArrayBuilder, len)       == 8
                    offset_of!(ArrayBuilder, cap)       == 16
                    offset_of!(ArrayBuilder, elem_size) == 24
                    offset_of!(ArrayBuilder, arena)     == 32
                    size_of::<ArrayBuilder>()           == 64
                  as const assertions, plus a compiler-side constant table
                  carrying the same five offsets and one owner asserting the
                  two agree. A field reorder then fails the runtime build, and
                  a compiler-side drift fails the owner
Artifact/cache    object content changes; compiler_build_id covers it. No key
                  component, no interface field, no ABI row
Prerequisite      PR 1. Without the effects row on the slow-path call, the
                  guard's arena/elem_size/cap loads are not loop-invariant to
                  LLVM and the fast path measures as little as the call it
                  replaces
Acceptance        the scalar set owner (i64, f32, f64, bool, char); a
                  bounded-capacity loop emitting zero calls; an unbounded loop
                  emitting exactly one call on the growth edge; an arena-mode
                  builder taking the slow path; a zero-stride builder taking
                  the slow path; runtime equivalence of both paths over the
                  plan 13 §4.2 length matrix
Benchmark         REQUIRED, because this row makes an explicit performance
                  promise. The plan 13 §8.1 matrix rerun (elements 1, 4, 16,
                  1024, 100000; push versus one append versus direct fill) with
                  the push/append ratio recorded before and after. It is a
                  measurement, not a correctness gate
Mirrors           13-string-array-allocation-short-input-audit.md §8.1, whose
                  CONFIRMED P1 entry this closes; 20-runtime-abi-ledger.md,
                  where the layout pin is recorded beside the Issue batch
                  constructor ABI section
```

`buffer.put_u8` gets **no** fast path. `Buffer` is not `#[repr(C)]` and its
storage is `BufferStorage { bytes: Vec<u8>, writable: *mut u8 }`
(`align_runtime/src/buffer_storage.rs`), whose `Vec` layout Rust does not
guarantee and whose `writable` pointer must be refreshed after every
reallocation. An inline fast path would have to reproduce `truncate(len)`,
`extend_from_slice`, the refresh and `cap = cap.max(len)` against an unpinned
layout. Making `Buffer` a `repr(C)` raw triple is a larger change with its own
ownership proof, and the shape that motivated it — a per-byte fill loop — is
already served by `buffer.append_filled`, merged as 1073 part 1. `BufferPut`
and `BufferAppendFilled` therefore receive their effects rows from PR 1 and
nothing else; a `Buffer` layout restructure is recorded here as deliberately
deferred, not forgotten.

### 2.8 The runtime archive profile (PR 3)

```text
Surface           [profile.release.package.align_runtime] in the root
                  Cargo.toml

Exact schema      [profile.release.package.align_runtime]
                  codegen-units = 1

Inputs, defaults  none. It applies to every `cargo build --release` of the
                  workspace, including the align-llm batch build
                  (`cargo build --release --workspace`) CLAUDE.md fixes
Errors            none; it is a cargo profile override
Ownership         n/a
Allocation        n/a
Owner             the workspace manifest; 21-build-perf-plan.md records the
                  decision
Artifact/cache    libalign_runtime.a bytes change. The archive is NOT a cache
                  key input: it participates only at final link, and no cached
                  artifact is derived from it, so no entry can go stale.
                  rt_lto_digest is unaffected (build.rs bakes with its own
                  rustc invocation). [profile.dist] is unchanged and already
                  carries lto = "thin" and codegen-units = 1
Prerequisite      none
Acceptance        an owner asserting the shipped `_align_rt_array_builder_push`
                  contains no `bl <ArrayBuilder::reserve>`, run against the
                  archive the driver actually links
Benchmark         the §2.7 benchmark covers it; the build-time delta is
                  recorded in 21-build-perf-plan.md's item ledger
Mirrors           21-build-perf-plan.md (a new item recording the decision and
                  its compliance with that document's Principle); Cargo.toml's
                  existing [profile.dist] comment, which gains one sentence
```

## 3. PR 1 — the per-symbol effects model

### 3.1 Layers and exact owners

```text
align_codegen_llvm/src/runtime_abi.rs
  RuntimeEffects, EffectClass and the total runtime_effects(RuntimeAbiId)
  match; apply_attributes derives from the class table of §2.2 instead of the
  shape spec's four effect fields; remove_attributes sheds exactly what
  apply_attributes added; validate_registry gains V1-V12; the test-control
  ABI digest serializes the record in place of memory_argmem_read

align_codegen_llvm/src/lib.rs
  the MEM_ARGMEM_READ constant is joined by the three further MemoryEffects
  bitmasks the classes need, each with its own textual pin in
  rt_contract_attrs_pin_encoding_and_curation

align_runtime
  the machine controls of §3.5: the allocation-parity owner, the fd/env
  observation owner, the C-unwind absence owner

crates/align_codegen_llvm/tests/golden/runtime_abi_declarations.txt
  one class token per line and the replacement attribute-group table
```

`RuntimeAbiShapeSpec` loses `return_noalias`, `fn_attrs`, `memory_argmem_read`
and `read_ptr_params`: a shape keeps the *type* and nothing else. That is the
structural half of 1071's root cause — effects keyed by C signature — and
removing the fields is what makes the old keying unrepresentable rather than
merely discouraged. Align is pre-release; there is no transitional shape spec
carrying both.

### 3.2 The four new bitmask pins

```text
memory(none)                                       MemoryEffects::none()
memory(argmem: read)                               existing MEM_ARGMEM_READ
memory(argmem: readwrite)
memory(argmem: readwrite, inaccessiblemem: readwrite)
memory(argmem: read, inaccessiblemem: readwrite)
memory(inaccessiblemem: readwrite)
```

Each is emitted through `add_valued_enum_attr(..., "memory", <mask>)` and each
gets an assertion on its exact printed form, for the reason the existing
`MEM_ARGMEM_READ` comment gives: the packed `MemoryEffects` encoding is
version-sensitive, so an LLVM upgrade that shifts a location's bits must fail
loudly at the pin rather than silently emit a different claim. The emitted
module continues to round-trip through `llvm-as-22`
(`emitted_ir_round_trips_through_llvm_as`), which rejects a malformed payload.

### 3.3 What each measured symptom becomes

```text
1071 (a)  align_rt_buffer_free becomes FreeLocal:
          memory(argmem: readwrite, inaccessiblemem: readwrite) nounwind.
          A non-escaping entry alloca never passed to the call is then provably
          untouched, and DSE removes its dead memset
1071 (b)  align_rt_buffer_put becomes ArgWriteAlloc:
          memory(argmem: readwrite, inaccessiblemem: readwrite) nounwind.
          The borrowed view header is loop-invariant across the call
1071 (c)  align_rt_array_builder_push becomes ArgWriteAlloc, so the builder
          handle is no longer reloaded from the frame before each push
1073 (2)  align_rt_buffer_put and align_rt_buffer_append_filled are classified
          by the same rule; this issue's half (2) is closed by that, not by a
          second shape variant
1074 T3   align_rt_bounds_fail, range_fail, utf8_boundary_fail,
          len_mismatch_fail, div_fail, alloc_size_fail and process_abort become
          FailNoReturn: noreturn cold nounwind
          memory(inaccessiblemem: readwrite). A caller with a surviving check
          keeps its inferred memory(read, ...) instead of collapsing to
          { nounwind }
withheld  align_rt_utf8_valid, str_find, str_rfind and str_finder_find become
          DispatchCache and keep claiming no memory effect. The withholding is
          now a named class with a stated reason rather than an empty cell
```

### 3.4 Invariants

```text
I1  a row's class is a property of the Rust body align_runtime ships,
    verifiable by reading that function and its callees. No class is derived
    from the row's C signature, its symbol spelling, or its shape
I2  nounwind is universal, and holds because every export is extern "C" in
    edition 2024. A row that ever needs extern "C-unwind" must first change
    this invariant
I3  a memory-claiming class is emitted only when the claim is true for EVERY
    input, including a null handle, an invalid width, and an error return. A
    row whose fast path is argmem-only but whose error path touches process
    state is HostState
I4  argmem-limited deallocation requires that every object the row frees is
    argument-derived AT THE LLVM LEVEL. A row that frees memory handed out
    through a noalias return is excluded; releases: Region records that, and
    V9 enforces it
I5  the withheld direction is always safe. A row classified more conservatively
    than its body warrants costs optimization; a row classified less
    conservatively is a miscompile. Every ambiguity resolves toward HostState
I6  a guarded rt-LTO row's declaration attributes are withheld before the merge
    and its merged definition carries none afterwards, at any attribute
    location. What apply_attributes adds, remove_attributes removes; the two
    are derived from one record so they cannot disagree
I7  an indirect call never carries a row's attributes. Attributes live on the
    declaration; a call through a function pointer has none, which is correct
    and must stay asserted
```

I4 and I6 carry the soundness of this PR. I4's hazard is reachable today:

```align
fn double(x: i32) -> i32 = x * 2

fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4, 5].map(double).to_array()
    return ys.sum()
  }
}
```

`to_array()` bump-allocates `ys` through `align_rt_arena_alloc`, whose A45
return is declared `noalias`, and writes the elements into that storage. The
`arena` block's end releases the whole region through `align_rt_arena_end`,
which receives the arena handle and not `ys`. Under a `FreeLocal` claim on
`align_rt_arena_end`, LLVM is entitled to conclude the teardown cannot touch the
`noalias` storage and to sink an element store past it: a store after free.

I6's hazard is the cross-check of §1.3: `remove_attributes` currently sheds
`spec.fn_attrs`, `spec.memory_argmem_read`, `spec.read_ptr_params` and
`spec.return_noalias` — the four fields PR 1 deletes. If it is not rewritten
against the record in the same change, a merged `PureArgRead` body keeps a stale
`memory(argmem: read)` that describes the *declaration's* promise rather than
the now-visible body's behaviour.

### 3.5 How a wrong row is caught

Five controls. The first three are inspection-time; the last two are the
machinery this repository's "recurred classes are closed by machinery, not
prose" rule requires, and they are what make the design fail-closed.

```text
C1  totality        a new RuntimeKey does not compile until it is classified
                    (total match, no `_` arm). runtime_export_source_inventory_
                    matches_registry independently forces a new
                    #[unsafe(no_mangle)] export into the registry at all
C2  structure       V1-V12 reject every internally inconsistent record, one
                    negative owner per rule
C3  golden          every declaration line carries its class token, so a class
                    change is a one-line diff a reviewer must approve, and a
                    mutation owner proves the golden is sensitive to it. The
                    emitted module still round-trips through llvm-as-22
C4  allocation      THE PRIMARY MACHINE CONTROL. align_runtime already exports
    parity          align_rt_alloc_count / align_rt_free_count under the
                    alloc-count feature. A parameterized owner calls every row
                    classified PureScalar, PureArgRead or ArgWrite with a valid
                    minimal input and asserts a zero allocation delta. A row
                    that actually reaches the allocator while claiming a
                    non-allocator class fails immediately, by test, for the
                    single most dangerous misclassification in the table
C5  behavioural     for every row in a memory-claiming class, a driver fixture
    control         writes a local aggregate, calls the row without passing the
                    local, reads the local back and checks its bytes at -O2. A
                    false argmem-only claim that lets DSE delete a live store
                    is caught as a value mismatch, not by inspection. The same
                    fixture shape, run under an fd-count and environment
                    observation, catches a row classified as anything but
                    HostState, Foreign or Callback that touches process state
```

C4 and C5 together close the two failure modes that inspection cannot: "this
body allocates and I did not notice" and "this claim is false in a way that
changes the caller's observable values". Neither is a benchmark and neither is
a client measurement; both are ordinary owner tests over the whole table.

### 3.6 Implementation closure matrix (PR 1)

Every class × every build mode × every declaration site. A cell whose behaviour
is identical across an axis is stated once for that axis rather than repeated.

**Class axis** — the required attribute set and the detector for a wrong row.

| Class | Required declaration | Failure mode if wrong | Detector |
| --- | --- | --- | --- |
| `PureScalar` | `memory(none) nounwind nofree nosync willreturn` | a hidden global read is optimized away | C4 zero-allocation; C5 value control; V10 |
| `PureArgRead` | `memory(argmem: read)` + pure-finite flags + `readonly captures(none)` | a hidden write is dropped | C4; C5; the existing `rt_contract_attrs_pin_encoding_and_curation` rows |
| `ArgWrite` | `memory(argmem: <argmem>)` + pure-finite flags | an allocator call is hidden, so a caller's freed object survives | **C4** — the discriminating control for this class |
| `ArgWriteAlloc` | `memory(argmem: rw, inaccessiblemem: rw) nounwind` | a store to a non-argument object is sunk past the call | C5 value control; I4 review; V4/V5 |
| `AllocNew` | `noalias` return + `memory(..., inaccessiblemem: rw) nounwind nofree` | a caller assumes non-aliasing that the body does not provide | the existing `align_rt_array_builder_new`/`str_finder_new` allocator-attribute pins; V7 |
| `FreeLocal` | `memory(argmem: rw, inaccessiblemem: rw) nounwind` | store-after-free when a released object is not argument-derived | **I4 + V9** (`releases: Region` forced to `HostState`); C5 |
| `DispatchCache` | flags only, memory withheld | none: strictly conservative | the existing `utf8_valid` / `str_find` / `str_finder_find` negatives, extended to the class |
| `HostState` | `nounwind` only | none: strictly conservative | C4 is not applicable; the class is the fail-closed sink |
| `Callback` | `nounwind` only | a callback's effects are assumed away | a par-thunk owner asserting the declaration carries no `memory` |
| `Foreign` | `nounwind` only | none: strictly conservative | — |
| `FailNoReturn` | `noreturn cold nounwind memory(inaccessiblemem: rw)` | `willreturn` on a diverging row | V8; the existing abort-family negative that forbids `willreturn` |

**Build-mode axis.**

| Cell | Required behaviour | Owner |
| --- | --- | --- |
| whole-program | every declared row carries its class's exact attributes | the extended declaration golden |
| per-unit | every unit declares the same rows with byte-identical attributes | the existing "trivial whole-program and per-unit-shaped emitted IR with identical alphabetical runtime declarations" owner, extended to the attribute groups |
| ThinLTO | the same declaration appears in every partition with identical attributes; no partition disagrees | `thin_lto_sv`, plus a new cross-partition attribute-identity assertion |
| rt-LTO merged (R4) | a guarded row's declaration attributes are withheld before the merge, and the merged definition carries none of them afterwards. `remove_attributes` is rewritten against the record, not against the deleted shape fields | the existing rt-LTO off/on XOR owner extended to every new attribute, plus a new assertion that a merged definition carries no enum attribute at any location |
| rt-LTO merged, string half | unchanged from PR #1091: `shed_rt_lto_target_bound_attributes` + `verify_rt_lto_target_independence` run pre-merge, module-scoped | `test-review-bounded`-independent existing rt-LTO owners; no change |
| `--no-rt-lto` | guarded rows keep their full class attributes | the existing off/on XOR owner |
| rt-LTO artifact defect | every existing fallback path re-applies the *class* attributes through `restore_rt_lto_guarded_attributes` | the existing missing/declaration-only/wrong-type/internal/private/available-externally/non-C negatives |

**Declaration-site axis.**

| Cell | Required behaviour | Owner |
| --- | --- | --- |
| declaration site (`declare`) | attributes applied through the typed row handle, never by a symbol-prefix scan, so a same-spelled program claimant cannot receive them | the existing plan 20 machine gate for claimant uniquification, extended to the new attributes |
| imported-fn declaration | a per-unit importer derives identical attributes from the same table with no interface field | `imports`, `interface_param_modes`, plus the per-unit golden comparison |
| indirect call | a call through an Align function pointer (`$parkernel`, `tg_register`'s thunk, a generated SQLite callback) inherits no row's attributes | a negative owner over the par-thunk and `tg_register` IR (I7) |
| compatible source extern | a source extern reusing a row's handle receives the row's class attributes and mints none of its own | the existing "compatible reuse representatives for each checked-in attribute class" owner, re-expressed over the class set |
| probe-feature rows | the eleven probe exports gain no class and no compiler handle | the existing probe-presence owners |

### 3.7 Acceptance corpus

```text
language-level, new
  dead store across a runtime call: a function writes a local aggregate, never
  passes it to a runtime call, frees an unrelated handle, and returns. Zero
  llvm.memset and zero dead stores for that local (1071 criterion 3)
  LICM across a runtime call: a loop loads a loop-invariant local that is not
  passed to an in-loop runtime call; the load is hoisted to the preheader
  (1071 criterion 4)
  no regression in the withheld set: utf8_valid and str_find still claim no
  memory effect, and the guarded rows still have theirs withheld before body
  linking (1071 criterion 5)

table-level, new
  runtime_effects_registry_is_total_and_structurally_valid, with one negative
  per V1-V12
  rt_lto_admission_predicate_matches_the_guarded_set, with one negative per
  P1-P5
  runtime_effects_class_mutation_changes_the_golden
  no_runtime_export_uses_extern_c_unwind

machine controls, new
  C4 allocation parity over every non-allocator-class row
  C5 value and process-state controls over every memory-claiming row

extended, existing
  rt_contract_attrs_pin_encoding_and_curation: one textual pin per bitmask
  emitted_ir_round_trips_through_llvm_as: unchanged, now exercising the new
    attribute payloads
  the declaration golden: 464 lines each gaining a class token, plus a
    replacement attribute-group table
  scripts/test-runtime-abi-exports.sh: unchanged. It compares normalized
    signatures, not attributes, so the classification does not reach it
```

## 4. PR 2 — one cold-path model for `Result`/`?`

### 4.1 The three tiers

Tier 1 records §2.5's edge at each of the six MIR sites that create one and
attaches `!prof` from one place in codegen. The `?` desugaring's standing NOTE
(`align_mir/src/lib.rs:22022`) is deleted by the change that closes it.

Tier 2 computes §2.6's fixed point and emits `cold`.

Tier 3 is a consumer of PR 1: the fail family's class is `FailNoReturn`, which
already carries `cold`. Tier 3 adds no pass and no attribute of its own.

### 4.2 Invariants

```text
J1  an edge is exceptional only because MIR SYNTHESIZED it. An `if`, a `match`
    arm, an `else` unwrap and a user-written early return are ordinary branches
    and are never recorded. This is what keeps the fact structural rather than
    minted from source spelling
J2  the record names the unlikely successor explicitly. It is never re-derived
    from block order, terminator shape or successor index conventions, both of
    which a later MIR transform may change
J3  no Err value, no diagnostic byte and no trap message changes. Bounds text
    keeps its exact (index, len) form
J4  cold fails closed. An unprovable call site, a cross-unit `pub fn`, an
    indirect call and an unanalyzable cycle all yield not-cold
J5  a block that is recorded exceptional and then deleted by an existing MIR
    pass takes its record with it. Validation rejects a record whose block is
    not a live Term::Branch, so a stale record is a compiler error rather than
    a weight on an unrelated branch
```

### 4.3 Implementation closure matrix (PR 2)

| Cell | Required behaviour | Owner |
| --- | --- | --- |
| `?` on `Result` | `!prof 2000/1` with the unlikely weight on the `Err` successor | new codegen owner; the MIR owner at the desugaring site |
| `?` early-return fast path | the pre-existing diverging-inner arm records no edge, because it emits no `Branch` | negative MIR owner |
| bounds check | `!prof` on the failing successor of `emit_bounds_check` | new owner; trap text parity owner unchanged |
| range check | both range sites recorded once each | new owner |
| vec bounds check | recorded | new owner |
| UTF-8 boundary | recorded | new owner |
| divide-by-zero guard | recorded on the runtime-guard arm only; the constant-divisor fold emits no branch | positive and negative owners |
| length mismatch | recorded | new owner |
| ordinary `if`/`match`/`else` | no record, no `!prof` | negative owner (J1) |
| deleted exceptional block | validation error, not a misplaced weight | MIR validation owner (J5) |
| `cold`, whole-program | an error-only helper reached solely from `Err` arms is `cold`; its callees are too | positive owner over a two-level helper chain |
| `cold`, mixed reachability | a helper also reachable from a success path is not `cold` | negative owner |
| `cold`, per-unit | a `pub fn` is never `cold` in per-unit mode | negative owner (J4) |
| `cold`, indirect | a function whose address is taken is never `cold` | negative owner |
| `cold`, recursion | a recursive error-only cycle terminates the fixed point and is `cold` | owner on a self-recursive error helper |
| fail family (Tier 3) | `noreturn cold nounwind memory(inaccessiblemem: readwrite)` | PR 1's `FailNoReturn` owners; the existing abort-family `willreturn` negative |
| caller effect recovery | a kernel with a surviving bounds check keeps an inferred `memory(read, ...)` rather than collapsing to `{ nounwind }` | new driver IR owner (1074's Tier 3 criterion) |
| diagnostics | every trap message and `Err` value byte-identical | existing trap-parity owners, unchanged (J3) |

### 4.4 Acceptance and measurement

Acceptance is the matrix above plus 1074's stated layout contract: *for a
function using `?`, the emitted layout places the `Err` continuation after the
function's `ret`, and a helper whose every return path constructs an `Err` is
`cold`* — with no profile data and no source annotation. The client positional
measurements in 1074 are evidence for that contract, not gates; PR 2 promises
metadata and layout, so it carries no benchmark.

## 5. PR 3 — per-element primitives

### 5.1 Invariants

```text
K1  the fast path and the slow path are observationally identical on every
    input: same published length, same bytes, same growth behaviour, same Drop
    eligibility, same initialized prefix
K2  the fast path never grows, never touches an arena chunk, and never handles
    a string element. Every one of those is a guard failure, and a guard
    failure is one call to the existing symbol
K3  the fast path is emitted only for an element of statically known scalar
    width in the existing scalar set, and only where the existing lowering
    already proves the handle non-null
K4  the layout dependency is pinned on BOTH sides by exact offsets. A runtime
    field reorder fails the runtime build; a compiler-side drift fails an owner
K5  no new runtime symbol, no alwaysinline, no inlinehint, no source
    annotation, no allocator substitution
```

### 5.2 Implementation closure matrix (PR 3)

| Cell | Required behaviour | Owner |
| --- | --- | --- |
| scalar set | `i64`, `f32`, `f64`, `bool`, `char`: one typed store and one `add`, zero calls, zero `memcpy` | one owner per element type |
| reserved capacity | a bounded-capacity push loop emits zero calls | new owner (1072 criterion 1) |
| growth edge | an unbounded loop emits exactly one call, on the growth edge | new owner (1072 criterion 3) |
| arena mode | an `array_builder` created with `new_in` takes the slow path every time | new owner |
| zero stride | a zero-`elem_size` builder takes the slow path | new owner |
| string element | `push_str` is untouched | existing owners |
| record element | `push_bytes` is untouched by this PR; 1072 criterion 2's record half is deferred with its reason recorded here | existing owners |
| stack-header builder | `init_stack` builders use the same fast path, because the header layout is the same | new owner |
| freeze and Drop | `build()` still transfers the payload without copying; the unfinished-Drop element sweep still sees the same initialized prefix | existing `array_builder` freeze and Drop owners, which fail for a fast path that mis-updates `len` |
| whole-program / per-unit / ThinLTO | identical emitted fast path in all three | the existing per-unit and `thin_lto_sv` comparison owners |
| runtime equivalence | both paths produce identical arrays over the plan 13 §4.2 length matrix | new differential owner |
| shipped archive | `_align_rt_array_builder_push` contains no `bl <reserve>` | new owner over the linked archive (§2.8) |
| no new attribute | no `alwaysinline` or `inlinehint` anywhere in the emitted module | a grep-shaped IR negative (K5) |

### 5.3 What this plan refuses from 1072, and why

1072 proposal 3 asks to extend the `--rt-lto` guarded set from the four string
predicates to the per-element class and its growth helpers. The admission
predicate of §2.4 refuses it: `array_builder_push` is `ArgWriteAlloc`, failing
P1, and its growth path calls `align_rt_realloc` and the abort machinery,
failing P3. Merging it would hand LLVM an `internal` body containing an opaque
external call in place of a precise declaration.

The inline fast path replaces it and is strictly better for the measured
symptom: the fast path removes the call entirely for the common case, whereas a
merged body would still be a call that LLVM may decline to inline. The slow path
stays one call to a symbol whose declaration now carries a complete effects
record. 1072's proposal 3 is therefore recorded as refused with its replacement,
not deferred.

This also removes the dependency 1072 assumed: with proposal 3 refused, PR 3
does not depend on 1069 at all.

### 5.4 The archive-profile decision, as recorded

**Decision.** The shipped artifact is already tuned, and the defect is local.
`release.yml` builds `align_runtime` with `--profile dist` in every phase —
`lto = "thin"`, `codegen-units = 1` — and deliberately keeps that archive
outside PGO instrumentation, so the `libalign_runtime.a` a user links from a
release archive already has `reserve` inlined into `push`. 1072's disassembly
was taken from `target/release/libalign_runtime.a`, the untuned local artifact.

The real gap is that the local archive differs from the shipped one, so local
iteration and local evidence do not represent what users link. PR 3 closes it
with a per-package profile override:

```toml
[profile.release.package.align_runtime]
codegen-units = 1
```

Rationale, recorded so it is not re-litigated:

```text
sufficient      intra-crate inlining is what reserve-into-push needs, and
                codegen-units = 1 supplies it. lto is not expressible in a
                cargo per-package override and is not required for one crate
cheap           align_runtime is 1 of 15 crates and rarely changes, so the
                serialized codegen cost is paid on a runtime edit only. The
                fourteen compiler crates keep parallel codegen
compliant       21-build-perf-plan.md's Principle — "Output is always fully
                optimized. Build speed comes from reuse and parallelism, never
                from lowering optimization" — is preserved: this RAISES the
                local artifact and lowers nothing
bounded         [profile.dist] is unchanged. `--release` for the compiler
                crates stays untuned, which is what Cargo.toml's existing
                comment settled
rejected        making --release inherit dist: pays thin-LTO link time on every
                compiler iteration, the exact cost that comment rejects
rejected        building the runtime with LTO under release: cargo does not
                accept lto in a per-package profile override
identity        libalign_runtime.a bytes change. It is not a cache-key input —
                it participates only at final link and no cached artifact
                derives from it — so no entry can go stale. rt_lto_digest is
                unaffected: build.rs bakes str_prims.bc with its own
                `rustc -O -Ccodegen-units=1`, independent of any cargo profile
```

`21-build-perf-plan.md` gains one item recording this decision and its measured
build-time delta. It is a release-profile decision on the record, not a silent
manifest edit.

## 6. Scalar ABI facts at call boundaries (issue 1075)

**Recommendation: keep 1075 separate. It does not belong in this plan.**

```text
different owner     1075 attributes ALIGN PROGRAM functions at declare_fn
                    (lib.rs:7077) and declare_imported_fn (:7164). This plan
                    attributes RUNTIME ABI ROWS through the registry. Different
                    table, different producer, different reviewers
no shared mechanism it needs no effects record, no class, no ledger row, no
                    golden line and no rt-LTO interaction. Nothing in §2 is
                    reused by it except the general shape of "a total function
                    over a type produces the attribute set"
different failure   a wrong zeroext is a miscompile at an Align-to-Align
                    boundary, detected by value controls over Align fixtures.
                    None of §3.5's controls apply: C4's allocation parity and
                    C5's runtime-row fixtures are meaningless for a bool
                    parameter
different identity  1075 changes per-function parameter attributes, which the
                    Slice 5 roadmap deferral explicitly covers. Folding it here
                    would put two independent public contracts under one review
                    and one closure matrix, which the cross-cutting gate warns
                    against
independent         1074 records 1075 as independent of every tier here, and
                    plan 68 assigns it to no G-guarantee
```

Where it should go: `07-roadmap.md` Slice 5's *"type-derived per-program-fn
param attributes"* deferral, which plan 69 already retracted in part for the
borrowed view header. 1075 is the second partial retraction of that same
paragraph, and it needs its own plan with its own closure matrix over the six
scalar rows its table names.

One shared note for whoever writes it: both plans state a fact at a call
boundary, so if 1075 lands after this one it should reuse the shape *class fixes
the attribute set, row fixes the per-ordinal detail* rather than inventing a
second mechanism. That is guidance, not a dependency in either direction.

## 7. Documents this plan edits

```text
20-runtime-abi-ledger.md   PRIMARY. Every declaration table gains the
                           classification column; the rt-LTO paragraph is
                           restated as the admission predicate rather than a
                           list of four symbols; "Machine gates" gains V1-V12,
                           the class-derivation table, and the replacement
                           attribute-group inventory; the Issue batch
                           constructor ABI section gains the ArrayBuilder
                           offset pins. Edited by the PR that implements each
                           part, atomically with it
04-mir.md                  §2.1's `?` pseudocode names the ExceptionalEdge
                           record in place of "codegen can place the cold edge
                           in a separate/low-priority section". PR 2
21-build-perf-plan.md      one new item recording §5.4's decision, its
                           rationale, its rejected alternatives and its
                           measured build-time delta. PR 3
07-roadmap.md              Slice 5A's curated-attribute-table paragraph gains
                           one pointer here, the way 2425 points at plan 69
13-...-audit.md            §8.1's CONFIRMED P1 entry is closed by PR 3, with
                           the measured matrix rerun recorded beside it
HANDOFF.md                 one paragraph, recording this plan as planned
68-vectorization-contract.md
                           UNCHANGED. It already names "plan 70" as G5's
                           implementing plan and needs no edit
draft.md, docs/language-spec.md, docs/open-questions.md
                           UNCHANGED. No language surface, no settled decision
                           reopened, no new syntax, no new diagnostic
05-backend-llvm.md         UNCHANGED. Every attribute is emitted through the
                           existing add_enum_attr / add_valued_enum_attr path
16-test-policy.md          UNCHANGED. Every owner named here is a leaf owner of
                           an existing target; none enters the bounded gate
```

## 8. Author-side ledger-to-prose consistency pass

- Every normative promise in §§3–5 appears in the §2 ledger, and every §2 public
  field has specified semantics: `class`, `argmem`, `params`, `retains` and
  `releases` each have a stated meaning, a validity rule (V1–V12) and an effect
  on the emitted declaration.
- The Cartesian product is exhaustive where it is externally meaningful. §2.2's
  derivation table has one row per class with no gap; §3.6 crosses all eleven
  classes with five build modes and five declaration sites and states, for each
  axis, the cells where behaviour is uniform rather than repeating them.
- Every argument and result has a concrete type, ownership, lifetime and
  allocation rule: the effects record is `'static` `Copy` and allocates nothing;
  `ExceptionalEdge` is owned by its `MirFn`; the fast path copies its scalar by
  value and retains nothing.
- No text or view crosses a native or wire boundary in this plan. No canonical
  persisted or exchanged format is added, so no golden vectors are required
  beyond the declaration golden, which is a textual IR pin and already has a
  mutation owner in both directions.
- Multi-invalid input has a deterministic order: V1–V12 are stated in evaluation
  order and report the first violation with its symbol, matching
  `validate_registry`'s existing single-`Err(String)` contract.
- Every CLI and build input is explicit. The fast path has no flag; the effects
  record has no environment input; `--rt-lto` and `--no-rt-lto` keep their exact
  current meaning; §2.8 is a manifest constant, not an ambient setting.
- Every fingerprint is stated: `compiler_build_id` is the only cache identity
  that changes for PR 1 and PR 2; `rt_lto_digest` is unchanged and its
  independence from §2.8 is proved from `build.rs`'s own `rustc` invocation; the
  runtime archive is stated to be outside every cache key with the reason.
- Every runtime inspection field is producer-owned: the classification is a
  `'static` table in the compiler and the layout pins are `const` assertions in
  the runtime. Nothing is read from an artifact or from source at run time.
- Every operation changing process-global native state is classified `HostState`
  and claims no memory effect, so no overlap-exclusion or restoration-order
  question arises from an effects claim. §2.2 records the one region-teardown
  exclusion (I4) and V9 enforces it.
- Every normative `align` example is one fenced block (§3.4) using constructs
  with repository precedent; declarations are shown separately from the
  positional call expressions in the fenced `text` and `llvm` blocks.
- No PR consumes a capability scheduled for a later PR. PR 2 Tier 3 consumes
  PR 1, which ships first; PR 3 consumes PR 1's effects row, which ships first;
  PR 3 consumes nothing from PR 2 and, after §5.3's refusal, nothing from 1069
  beyond what PR #1091 already merged. No milestone is consumed: plan 68 §3.1
  names the existing plan 20 inventory as G5's only prerequisite.
- `draft.md`, `docs/language-spec.md`, the implementation plans and the package
  designs agree, because none of them states a runtime effects contract today
  and only plan 20 gains one. There is no `ja/` mirror for an `docs/impl` plan,
  so no language mirror is affected.
- Acceptance tests cover each ledger invariant: I1–I7 map to §3.5's C1–C5 and
  §3.6's detector column, J1–J5 to §4.3, K1–K5 to §5.2. The only benchmark is
  §2.7's, attached to the only explicit performance promise in the plan, and it
  is stated as a measurement rather than a correctness gate.
- Three issue claims were found stale against the tree and are corrected in
  §1.3 rather than repeated: `remove_attributes`' post-#1091 scope, the shipped
  archive's profile, and the admissibility of the per-element rows to the
  rt-LTO set.
