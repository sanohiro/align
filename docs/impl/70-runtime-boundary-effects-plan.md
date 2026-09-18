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
VERIFIED   RuntimeAbiShapeSpec holds one effect bit (runtime_abi.rs:178-185).
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
           different scopes, different times and different owners; §3.7 R4 is
           the matrix cell that keeps them consistent under PR 1
CORRECTED  1072 proposal 4, "stop shipping an untuned runtime archive", is
           already satisfied for the shipped artifact: release.yml builds
           align_runtime with --profile dist (thin LTO, one codegen unit) in
           every release phase. The real gap is local; §5.4 records the decision
CORRECTED  1072 proposal 3, "extend the --rt-lto set to the per-element class",
           FAILS the admission predicate this plan states (§2.4). The per-element
           rows are allocator-reaching. §5.3 records the refusal and its reason
NARROWED   1071 proposal 2 asks for memory(argmem: readwrite) on
           align_rt_array_builder_push and align_rt_buffer_put, and
           memory(argmem: readwrite, inaccessiblemem: readwrite) on the free
           family. Both are UNSOUND as stated. LLVM's argmem location is
           "accesses to memory via pointer values BASED ON the function's
           arguments" (LangRef), and AAResults::getModRefInfo implements it by
           alias-querying the location against each pointer ARGUMENT, with no
           reachability step. A container mutator writes through a payload
           pointer LOADED OUT OF argument memory, which is not based on the
           argument, so the claim lets LLVM conclude NoModRef for that payload.
           Every argmem claim shipped today (A01, A26, A82) is a DIRECT reader,
           which is consistent with this rule and is why the existing curation
           never hit it. §2.2's invariant D and §3.3 record what the rows get
           instead, and §3.6 records the one experiment that could widen it
```

### 1.4 Which guarantee each PR closes

Plan 68 G5 reads: *"Every runtime primitive that can appear in a loop has an
effects record and either an inline fast path or a vector form"*, promising
*"each runtime ABI symbol carries a complete memory-effects attribute;
per-element primitives have a visible inline fast path and a visible slow path;
`Result`/`?` failure edges are cold with one model"*. PR 1 closes the first
clause, PR 3 the second, PR 2 the third.

No clause is widened. One is **narrowed**, and §7 records the edit that makes
plan 68 say so: "each runtime ABI symbol carries a complete memory-effects
*attribute*" is not achievable, because invariant D (§2.2) establishes that the
correct record for every indirect-storage row is to carry no memory attribute at
all. "A complete memory-effects *record*" — one per symbol, with no silent
default — is what PR 1 delivers, and it is what G5's exact statement already
says. The promised column is corrected to match the exact statement rather than
the plan quietly under-delivering against it.

## 2. Public-contract ledger

Every public surface this plan introduces, one row each. This ledger is
authoritative while drafting; §8 is the author-side consistency pass over it.

### 2.1 The effects record

```text
Surface           RuntimeEffects, one per RuntimeAbiId, in
                  crates/align_codegen_llvm/src/runtime_abi.rs

Exact type        struct RuntimeEffects {
                      class:    EffectClass,
                      argmem:   ArgMem,
                      params:   &'static [ParamEffect],
                      escapes:  &'static [u32],
                      releases: Release,
                      returns_fresh: bool,
                      diverges: bool,
                  }
                  enum ArgMem { Unstated, None, Read }
                  struct ParamEffect { ordinal: u32, mode: ParamMode }
                  enum ParamMode { Read, Write, ReadWrite, Opaque }
                  enum Release { None, HandleOnly, Indirect, Region }
                  EffectClass is the closed twelve-variant set of §2.2

                  `escapes` lists every pointer ordinal that may leave the call
                  by ANY route: retained in a handle, stored into another
                  argument, or RETURNED. `captures(none)` is emitted for a
                  pointer ordinal exactly when it is absent from `escapes`,
                  for every class. This is not a naming detail: with a
                  `retains`-only field, align_rt_realloc (which may return its
                  own argument), align_rt_array_builder_init_stack and
                  align_rt_builder_init_stack (which return `out`), and
                  align_rt_builder_new / align_rt_array_builder_new_in (which
                  store their arena argument into the constructed header) would
                  each receive an unsound `captures(none)`. None of those five
                  rows carries a parameter attribute today, so the record must
                  not introduce one.

                  `returns_fresh` and `diverges` are ROW bits, not class
                  consequences: `noalias` on the return and `noreturn` are
                  facts about a row, and deriving them from the class token
                  loses two the tree asserts today (§2.3 V7, V8).

                  `ArgMem` deliberately has no Write or ReadWrite variant. See
                  invariant D in §2.2: no row in this registry may soundly
                  claim to WRITE only argument-based memory

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
                  exactly once and then hits. No cache key COMPONENT is added.
                  The versioned test-control ABI fingerprint
                  (test_control_runtime_abi_fingerprint) serializes
                  memory_argmem_read today, must serialize the replacement, and
                  has its v1 tag bumped in the same change.
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

Exact schema      EffectClass, a closed set of exactly twelve tokens. "Direct"
                  below always means the LangRef sense of invariant D: memory
                  reached through a pointer BASED ON a parameter, never through
                  a pointer loaded out of one.

  PureScalar      no pointer parameter; touches no memory; terminates
  PureArgRead     reads only direct parameter memory; no allocator, no process
                  state; terminates
  ArgRead         reads only direct parameter memory but may allocate or free
                  through the allocator; terminates or aborts on OOM
  AllocNew        allocator-class constructor returning a fresh allocation that
                  aliases nothing the caller holds
  FreeLocal       null-safe deallocator whose released object IS the argument's
                  own allocation, exactly as libc `free` (align_rt_free is the
                  one shipped row that qualifies)
  IndirectStorage reads or writes storage reached by loading a pointer out of
                  parameter memory: every container mutator, every handle whose
                  payload is a separate allocation, and every deallocator that
                  releases one
  DispatchCache   otherwise PureArgRead, but reads a process-global CPU-feature
                  or dispatch cache
  HostState       reads or writes process/OS state: files, sockets, env, clock,
                  RNG, process control, threads, or a region teardown
  Callback        calls an Align function pointer the caller supplied
  Foreign         crosses into a third-party native library
  FailNoReturn    diverges through the runtime abort family
  ProcessExit     diverges by terminating the process

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

The `memory` and flag columns are fixed by the class. The `return` and
parameter columns are the row's own bits, applied uniformly: `noalias` on the
return exactly when `returns_fresh`, `noreturn` exactly when `diverges`,
`readonly`/`writeonly` per `params`, and `captures(none)` on every pointer
ordinal absent from `escapes`. The parameter columns below therefore say only
which rows may record `params` at all.

| class | `memory(...)` | function attributes | may record `params` |
|---|---|---|---|
| `PureScalar` | `memory(none)` | `nounwind nofree nosync willreturn` | no pointer exists |
| `PureArgRead` | `memory(argmem: read)` | `nounwind nofree nosync willreturn` | `Read` only |
| `ArgRead` | `memory(argmem: read, inaccessiblemem: readwrite)` | `nounwind` | `Read` only |
| `AllocNew` | `memory(argmem: read, inaccessiblemem: readwrite)`, or `memory(inaccessiblemem: readwrite)` when `argmem == None` | `nounwind nofree` | `Read` only |
| `FreeLocal` | `memory(argmem: readwrite, inaccessiblemem: readwrite)` | `nounwind` | none |
| `IndirectStorage` | withheld | `nounwind` | `Read` only, and only for a directly-read ordinal |
| `DispatchCache` | withheld | `nounwind nofree nosync willreturn` | `Read` only |
| `HostState` | withheld | `nounwind` | `Read` only, and only for a directly-read ordinal |
| `Callback` | withheld | `nounwind` | none |
| `Foreign` | withheld | `nounwind` | none |
| `FailNoReturn` | `memory(inaccessiblemem: readwrite)` | `nounwind cold` | none |
| `ProcessExit` | `memory(inaccessiblemem: readwrite)` | `nounwind` | none |

*Withheld* means no `memory` attribute is emitted, which is LLVM's most
conservative state: the call may read and write all memory. Withholding is the
fail-open-into-correctness direction, and it is what makes an over-conservative
classification cost optimization rather than soundness.

**Precedence among the withheld classes is total and ordered**, because they
overlap on real rows and do not emit identical declarations: a row that is more
than one of them takes the FIRST that applies, in the order `FailNoReturn`,
`ProcessExit`, `Foreign`, `Callback`, `HostState`, `IndirectStorage`,
`DispatchCache`. A generated-SQLite-callback registration is `Foreign` and a
`par_map` row is `Callback`; neither may record `params`, so the class token is
determined by the row's properties and the golden cannot drift.

Seven facts in that table need their reason on the record.

**Invariant D — `argmem` is direct-access only, and nothing in this registry
writes only direct-access memory.** LLVM's `argmem` location means *"accesses to
memory via pointer values based on the function's arguments"*, and `based on`
excludes a pointer obtained by loading through another. `AAResults::getModRefInfo`
implements exactly that: for an `onlyAccessesArgPointees` call it alias-queries
the queried location against each pointer argument with
`MemoryLocation::getForArgument`, with no reachability step. A container mutator
writes through the payload pointer it loads out of its handle, which `BasicAA`
will report `NoAlias` against the handle, so `memory(argmem: readwrite)` on such
a row entitles LLVM to conclude `NoModRef` for that payload — and to sink a store
to it past the call. This is not hypothetical for this wave: PR 3's fast path
emits exactly `%data = load ptr, %b` followed by a store through `%data`, around
slow-path calls to the same handle.

Consequently `ArgMem` has no `Write` variant, no class emits `argmem: readwrite`
except `FreeLocal`, and every mutator, every payload-owning deallocator, and
every builder writer is `IndirectStorage` with memory withheld. Every `argmem`
claim shipped today — A01, A26, A82 — is a direct reader of a `{ptr, len}`
argument, which is why the 2026-07-11 curation never met this and why the
existing rows are preserved unchanged.

`FreeLocal` is therefore not a family and not a shape. `align_rt_free(ptr)`
releases exactly the argument's own allocation, precisely as libc `free`, whose
LLVM declaration is `memory(argmem: readwrite, inaccessiblemem: readwrite)`.
Every other deallocator releases a payload reached by a load and is
`IndirectStorage`, and a region teardown is `HostState`. In particular
`align_rt_free_string_array` and the `array_builder_free_strings` pair walk an
array and free each element pointer loaded from argument memory, and those
element buffers came from `align_rt_alloc`, which is declared `noalias ptr`: a
`FreeLocal` claim on them is the same store-after-free hazard as the arena one.

**A62 is a shape, not a class.** The `void @SYM(ptr)` shape carries 43 registry
rows and spans at least four classes: `align_rt_free` is `FreeLocal`,
`align_rt_buffer_free` and `align_rt_array_builder_free` are `IndirectStorage`,
`align_rt_arena_end`, `align_rt_tg_end`, `align_rt_io_file_free`,
`align_rt_tcp_conn_free`, `align_rt_child_free`, `align_rt_crypto_random` and
`align_rt_rng_seed_os` are `HostState`, and `align_rt_builder_pop_comma` frees
nothing. Naming an A-shape as a class family would be exactly the
signature-keyed effects that invariant I1 forbids and that 1071 identifies as
the root cause.

**`nounwind` is universal.** Every base row is a Rust `extern "C"` function, and
since Rust 1.81 an unwind out of `extern "C"` aborts. That is an ABI property of
the toolchain, not of the edition, so the invariant's stated basis is the
workspace's rustc floor, which is the thing a version bump could move. No
runtime export declares `extern "C-unwind"`. The class table therefore emits
`nounwind` for all twelve classes, and an owner asserts that no `align_rt_*`
export uses `C-unwind`, so adding one fails the gate rather than silently
invalidating 464 declarations.

**`nosync` is never emitted with the allocator.** `malloc`/`free` synchronize,
so `ArgRead`, `AllocNew`, `FreeLocal` and `IndirectStorage` carry neither
`nosync` nor `willreturn` — OOM aborts, which is divergence.

**A reallocating row is `IndirectStorage`, never `AllocNew`.** `AllocNew` fixes
`nofree`, and the class definition requires a fresh allocation aliasing nothing
the caller holds. Both are false for `align_rt_realloc(ptr, i64)`: it frees its
argument and may return that same pointer. It therefore records
`escapes: [0]`, so it never receives `captures(none)`, and `releases: HandleOnly`.

**A constructor that returns an argument-derived pointer is not `AllocNew`.**
`align_rt_array_builder_init_stack(out, …)` and `align_rt_builder_init_stack(out, …)`
return `out`; `align_rt_builder_init_bounded_stack` writes into caller storage.
All three are `IndirectStorage` with `returns_fresh: false` and `escapes: [0]`.
`align_rt_builder_new(arena, …)` and `align_rt_array_builder_new_in(arena, …)`
are `AllocNew` for their return but store the arena argument into the header
they construct, so both record `escapes: [0]`, and `new_in` records no `Read`
mode for that ordinal because it bump-allocates out of it.

**`FailNoReturn` gets `memory(inaccessiblemem: readwrite)`.** The fail family
takes no pointer argument and writes a message through Rust's global stderr
machinery, which is exactly what LLVM's own `inaccessiblemem` models for
`abort`/`exit`/stdio. This is the half of 1074 whose loss is measured: a kernel
with a surviving bounds check collapses to `{ nounwind }` today because the
declaration states nothing.

**`ProcessExit` is a separate class from `FailNoReturn`.** The `noreturn`
attribute group `#1` has eight members today, not seven:
`align_rt_process_exit(i64)` is a deliberate program termination, not an abort.
It keeps `noreturn` through the row's `diverges` bit and gets the same
`inaccessiblemem` model, but it must not be `cold` — `process.exit` on a
success path is ordinary control flow, and marking it cold would misweight a
normal program's exit. Folding it into the abort family would have silently
dropped a `noreturn` the tree asserts today.

**`HostState` keeps memory withheld in this wave.** An I/O row could in
principle carry LLVM's `memory(argmem: ..., inaccessiblemem: readwrite)` stdio
model — file and socket *contents* are not LLVM memory, so only process memory
is at issue. It is withheld anyway because a host-state body may reach `std`
machinery whose process-global state the program can also read through another
row (`env_set`/`env_get` is the clearest pair), and no measured evidence in
1071, 1072, 1073 or 1074 comes from an I/O row. Narrowing `HostState` per symbol
later is a follow-up under this same machinery, not a reopening of the schema.

**A region teardown is `HostState`, not a deallocator class.**
`align_rt_arena_alloc` and `align_rt_tg_alloc` are declared `noalias ptr` (A45),
so an arena-allocated object is, by declaration, *not* derived from the arena
argument. If `align_rt_arena_end(arena)` claimed any `argmem`-limited
deallocation, LLVM would be entitled to sink a store to that `noalias` object
past the teardown: a store after free. `align_rt_arena_end`,
`align_rt_arena_reset` and `align_rt_tg_end` are therefore `HostState` with
`releases: Region` — `tg_end` additionally joins threads — and §2.3 rejects
`Region` and `Indirect` on any class but `HostState` and `IndirectStorage`.

The shape of that hazard is worth stating once, because it is the same one
invariant D states for writes: whenever LLVM can prove `NoAlias` between a
pointer the caller holds and the pointer the row receives, an `argmem`-limited
claim removes that row from the caller's mod/ref picture for that pointer. That
is correct only when the released or written object genuinely is the argument's
own allocation.

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
  V2  a params ordinal names a NativeType::Ptr of that row's shape, and no
      ordinal appears twice
  V3  an escapes ordinal names a NativeType::Ptr of that row's shape
  V4  a params mode of Write or ReadWrite is rejected outright. Invariant D
      leaves no sound way to state a direct-only write, so the mode exists in
      the type only so that a future widening (§3.6) has somewhere to land,
      and until then it fails closed
  V5  a memory-claiming class (PureScalar, PureArgRead, ArgRead, AllocNew,
      FreeLocal, FailNoReturn, ProcessExit) requires argmem to match its row
      in the §2.2 table exactly: None for PureScalar, FreeLocal, FailNoReturn
      and ProcessExit; Read or None for ArgRead and AllocNew; Read for
      PureArgRead
  V6  a withheld class (IndirectStorage, DispatchCache, HostState, Callback,
      Foreign) requires argmem == Unstated. Parameter attributes are NOT
      coupled to argmem: a withheld row may still record a Read ordinal and
      receive readonly captures(none), which is what align_rt_utf8_valid,
      str_find, str_rfind and str_finder_find carry today with no memory
      attribute (golden attribute group #4)
  V7  returns_fresh requires the row's NativeReturn to be Ptr, and is rejected
      for FreeLocal, FailNoReturn and ProcessExit. It is a ROW bit, not an
      AllocNew consequence: align_rt_par_map is declared `noalias ptr` today
      and is a Callback row
  V8  diverges is set exactly for FailNoReturn and ProcessExit, and a row with
      diverges set forbids willreturn
  V9  releases == None for every class but FreeLocal, IndirectStorage and
      HostState; FreeLocal requires HandleOnly; Indirect and Region are
      permitted only on IndirectStorage and HostState respectively. "Releases"
      names what the row does to a CALLER-VISIBLE object, so a row that only
      recycles its own internal storage through the allocator records None
  V10 a row with no pointer parameter forbids a nonempty params or escapes,
      and PureScalar requires exactly that shape
  V11 the existing key-count, base-count, bijection and key-symbol rules
  V12 every row listed in the --rt-lto guarded set satisfies P1 and P2 of §2.4.
      P3, P4 and P5 are properties of the baked artifact, which does not exist
      at this point; §2.4 gives them their own check and error path

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

  P1  class in {PureScalar, PureArgRead}
  P2  escapes is empty and releases == None
  P3  its Rust body's call graph is closed inside the baked artifact:
      no cross-crate call, no allocator, no panic/abort machinery
  P4  the baked definition carries no semantic string attribute
      (verify_rt_lto_target_independence already rejects every string
      attribute, so P4 is machine-checked as a byproduct)
  P5  the post-`rustc -O -Ccodegen-units=1` body is at most 200 LLVM
      instructions, measured from the baked artifact
  P6  a recorded paired measurement on both supported architectures shows no
      regression at the default --target-cpu

Inputs, defaults  none. P1-P2 are registry properties; P3-P5 are properties of
                  the baked artifact; P6 is a recorded measurement, and a row
                  without one is not admitted
Errors            THREE checks with three error paths, because the three groups
                  are knowable at three different times:
                  - P1-P2 are checked by validate_registry (V12), before any
                    LLVM value exists, and fail the compiler build
                  - P3-P5 are checked by an artifact-time owner over the parsed
                    str_prims.bc, per guarded row, and fail that owner. They
                    deliberately do NOT run inside link_in_rt_lto: the existing
                    verify_rt_lto_target_independence sweeps every definition
                    with a body and falls back for the whole artifact, so it
                    can never exclude one row, and a per-row admission question
                    must not be able to disable rt-LTO on a user's build
                  - P6 gates the PR that admits the row; its measurement is
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
Acceptance        rt_lto_admission_predicate_matches_the_guarded_set (P1-P2,
                  one negative each) and rt_lto_guarded_bodies_meet_the_artifact
                  _budget (P3-P5, one negative each, over the baked bitcode)
Benchmark         P6 is the benchmark, and it is an admission gate for a row,
                  never a correctness gate
Mirrors           20-runtime-abi-ledger.md, whose rt-LTO paragraph currently
                  names the four symbols as a list
```

One line: **a runtime row may join the `--rt-lto` guarded set exactly when its
effect class is `PureScalar` or `PureArgRead`, no pointer argument escapes and
it releases nothing, its baked body is crate-closed, carries no string attribute
and stays within the instruction budget, and a recorded paired measurement shows
no regression at the default `--target-cpu` on both architectures.**

Applied to the current set: `StrEq`, `StrStartsWith`, `StrEndsWith` and
`StrEqIgnoreCase` are `PureArgRead` and pass every clause. `StrCmp` is also
`PureArgRead` and passes P1–P5; it is excluded by **P6**, its measured 0.72×
regression, which is exactly the outcome 1069 asks for — a row that failed the
criterion, not evidence against having one.

Why the allocator and indirect-storage classes are excluded, stated once. A
merged definition is set `internal` and has its curated attributes shed, on the
theory that LLVM re-derives them from the now-visible body. That holds for a
leaf predicate. It is false for an allocator-reaching body: the growth path
calls `align_rt_realloc` and the runtime's abort machinery, neither of which is
in the baked artifact, so the merged body keeps an opaque external call and LLVM
re-derives *less* than the declaration promised. Merging such a row therefore
trades a precise declaration for an imprecise definition. `array_builder_push`
is `IndirectStorage` and is excluded for exactly this reason; §5.3 records the
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

Inputs, defaults  recorded by the lowering that CREATES the edge. Six KINDS
                  over nine recording SITES, which are not the same count:
                    ResultPropagate  the `?` desugaring (align_mir/src/lib.rs
                                     :22022, whose NOTE this replaces)
                    BoundsCheck      emit_bounds_check (:11889),
                                     emit_vec_bounds_check (:12612)
                    RangeCheck       emit_range_bounds_check (:12627) and the
                                     two A-range arms (:12055, :12105)
                    Utf8Boundary     the boundary arm (:12745)
                    DivByZero        the runtime divide guards (:11673, :11836)
                    LenMismatch      the length-mismatch arm (:15215)
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

**Deliberately not recorded, with the reason.** MIR synthesizes further branches
whose unlikely successor calls a diverging runtime row and which none of the six
kinds names: the raw-call guard (`align_mir/src/lib.rs:11119`), the `impossible`
arm (`:15730`), the exceeded arm (`:19556`), and the capacity guards (`:21475`,
`:21593`). They are left unweighted in this wave because each reaches
`align_rt_process_abort`, whose `noreturn` already makes LLVM's own
`unreachable` heuristic sink the block — the one mechanism 1074 measures as
working. Adding a seventh `Guard` kind for them would duplicate that heuristic.
`ExceptionalKind` is closed, so a later capability that needs one adds the
variant and its recording site together; the omission is a decision here, not a
gap the closed enum hides.

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
                  Plan 20's agreement gate covers "identical alphabetical
                  runtime DECLARATIONS"; cold lands on Align-generated
                  DEFINITIONS, which that gate does not reach, so no exception
                  to it is needed or claimed and it stays absolute for runtime
                  declarations
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
                  BOUND (size <= 64, align <= 16, :17593), which does not pin an
                  offset. PR 3 ADDS exact pins beside it, keeping the existing
                  alignment bound, which is load-bearing for
                  align_rt_array_builder_init_stack's documented "at least 64
                  writable bytes aligned to 16" precondition and for the
                  companion size_of::<ArrayBuilder>() + size_of::<Buffer>()
                  <= 128 assertion:
                    offset_of!(ArrayBuilder, data)      == 0
                    offset_of!(ArrayBuilder, len)       == 8
                    offset_of!(ArrayBuilder, cap)       == 16
                    offset_of!(ArrayBuilder, elem_size) == 24
                    offset_of!(ArrayBuilder, arena)     == 32
                    size_of::<ArrayBuilder>()           == 64
                    align_of::<ArrayBuilder>()          <= 16   (retained)
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
Acceptance        a SYMBOL-level owner over the release archive: `llvm-nm
                  --defined-only` on libalign_runtime.a must report no
                  `ArrayBuilder::reserve` symbol reachable as an external
                  relocation target from align_rt_array_builder_push's section.
                  Deliberately not a disassembly grep for `bl <reserve>`: `bl`
                  is AArch64 only, the leading underscore is Mach-O only, and
                  the memory note "aarch64 shape gates are x86-only" records
                  exactly this trap. The owner skips, loudly, when no `--release`
                  archive is present, because no test target builds one
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
  the MEM_ARGMEM_READ constant is joined by the four further MemoryEffects
  bitmasks §3.2 enumerates, each with its own textual pin in
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

### 3.2 The complete bitmask set

The class × `ArgMem` product is small and closed, because `ArgMem` has three
variants and only seven classes claim memory at all. This is the exhaustive
list; nothing else is reachable, and each line gets its own textual pin:

```text
memory(none)                                  PureScalar             (existing:
                                                                  none emitted)
memory(argmem: read)                          PureArgRead      MEM_ARGMEM_READ,
                                                                       existing
memory(inaccessiblemem: readwrite)            ArgRead / AllocNew with
                                              argmem == None; FailNoReturn;
                                              ProcessExit                   NEW
memory(argmem: read, inaccessiblemem: readwrite)
                                              ArgRead / AllocNew with
                                              argmem == Read                NEW
memory(argmem: readwrite, inaccessiblemem: readwrite)
                                              FreeLocal                     NEW
```

Four new masks, one existing, one (`memory(none)`) new in emission though
trivially encoded. `memory(argmem: readwrite)` alone and every
`argmem: write` form are unreachable by construction: invariant D removes the
`Write` variant from `ArgMem`, and V4 rejects a write-claiming parameter mode.

Each is emitted through `add_valued_enum_attr(..., "memory", <mask>)` and each
gets an assertion on its exact printed form, for the reason the existing
`MEM_ARGMEM_READ` comment gives: the packed `MemoryEffects` encoding is
version-sensitive, so an LLVM upgrade that shifts a location's bits must fail
loudly at the pin rather than silently emit a different claim. The emitted
module continues to round-trip through `llvm-as-22`
(`emitted_ir_round_trips_through_llvm_as`), which rejects a malformed payload.

### 3.3 What each measured symptom becomes

Invariant D narrows this substantially against what 1071 proposal 2 asks for,
and the narrowing is stated rather than quietly absorbed.

```text
1071 (a)  align_rt_buffer_free becomes IndirectStorage, NOT a FreeLocal claim:
          it releases the inner payload through a loaded pointer. Memory stays
          withheld; the row gains nounwind and, through `escapes`, an honest
          capture record. 1071's criterion 3 (zero memsets for a dead local) is
          therefore NOT promised by effects alone in this wave; BasicAA's
          existing non-escaping-allocation reasoning is what can reach it, and
          §3.6 records the experiment that would tell us whether anything more
          is available
1071 (b)  align_rt_buffer_put becomes IndirectStorage for the same reason
1071 (c)  align_rt_array_builder_push becomes IndirectStorage for the same
          reason. Criterion 4's LICM claim is likewise not promised by effects
          alone
1073 (2)  align_rt_buffer_put and align_rt_buffer_append_filled are classified
          by the same rule, which is what this issue's half (2) asks for: one
          model, not a second shape variant. The classification is
          IndirectStorage, not the argmem claim the issue proposes
1074 T3   align_rt_bounds_fail, range_fail, utf8_boundary_fail,
          len_mismatch_fail, div_fail, alloc_size_fail and process_abort become
          FailNoReturn: noreturn cold nounwind
          memory(inaccessiblemem: readwrite). align_rt_process_exit becomes
          ProcessExit: noreturn nounwind memory(inaccessiblemem: readwrite),
          keeping the noreturn it carries today and deliberately not cold.
          A caller with a surviving check keeps its inferred memory(read, ...)
          instead of collapsing to { nounwind }: THIS is the measured effect
          this wave does deliver
align_rt_alloc   AllocNew with argmem == None: noalias return,
          memory(inaccessiblemem: readwrite) nounwind nofree. align_rt_free is
          the one FreeLocal row, releasing exactly its argument's allocation
align_rt_par_map Callback, memory withheld, returns_fresh: true — it keeps the
          noalias return the golden asserts today
withheld  align_rt_utf8_valid, str_find, str_rfind and str_finder_find become
          DispatchCache and keep claiming no memory effect and their existing
          readonly captures(none) parameters. The withholding is now a named
          class with a stated reason rather than an empty cell
```

What PR 1 therefore delivers, stated honestly: complete classification with no
silent default, universal `nounwind` across 464 rows, `noreturn` preserved on
all eight rows that carry it, `cold` and an explicit memory effect on the abort
family (1074 Tier 3, the one measured effect-attribute win in the four issues),
`memory(none)` and `memory(argmem: read)` on the genuinely direct rows,
honest `captures(none)` coverage including the five rows §2.2 names as needing
an `escapes` record, and the admission predicate. It does **not** deliver
1071's criteria 3 and 4 by attribute alone. PR 3's fast path, which removes the
call rather than describing it, is what reaches the per-element measurements.

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
I4  an argmem claim of ANY kind — read, write or deallocation — requires that
    every object the row touches is based on a pointer argument AT THE LLVM
    LEVEL, in the LangRef sense that excludes a pointer obtained by loading
    through another. This covers writes, not only frees: §2.2's invariant D is
    the statement, ArgMem's missing Write variant and V4/V5 are the
    enforcement, and releases: Indirect / Region record the deallocation half
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

I4 and I6 carry the soundness of this PR. I4's hazard is reachable today, in
both of its halves. The write half is reachable from PR 3's own codegen: the
fast path loads `data` out of the builder and stores through it, around
slow-path calls to the same handle, so an `argmem` write claim on
`align_rt_array_builder_push` would let LLVM reorder those. The deallocation
half is reachable from ordinary source:

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
C1  totality        a new RuntimeKey does not compile until it is classified:
                    the record is a total match over RuntimeAbiId with no `_`
                    arm. That is the whole of C1, and the plan does not lean on
                    more. runtime_export_source_inventory_matches_registry is
                    NOT a general backstop: it include_str!s six of
                    align_runtime/src's twenty files and asserts 379 symbols
                    against 464 registry rows, so a new export added in
                    process_live.rs, crypto_digest.rs, fs_*.rs, os_host.rs,
                    json_number.rs or buffer_storage.rs is outside its reach
                    (align_rt_child_poll is a shipped example). PR 1 therefore
                    also makes that test's file list DERIVED from a directory
                    read rather than hand-maintained, so the backstop the plan
                    wants actually exists
C2  structure       V1-V12 reject every internally inconsistent record, one
                    negative owner per rule
C3  golden          every declaration line carries its class token, so a class
                    change is a one-line diff a reviewer must approve, and a
                    mutation owner proves the golden is sensitive to it. The
                    emitted module still round-trips through llvm-as-22
C4  allocation      THE PRIMARY MACHINE CONTROL, and it needs new machinery to
    parity          be one. align_rt_alloc_count / align_rt_free_count exist
                    under the alloc-count feature, but ALLOC_CALLS is
                    incremented at exactly one site — inside align_rt_alloc —
                    and align_runtime has no #[global_allocator]. Almost every
                    row allocates through Rust's global allocator (Box, Vec,
                    String, format!, extend_from_slice), which those counters
                    never see, so a row that allocates while claiming a
                    non-allocator class would show a ZERO delta and pass.
                    PR 1 therefore adds a counting #[global_allocator] behind
                    the existing alloc-count feature, leaving the two existing
                    counters and their consumers untouched, and only then is
                    the parameterized owner meaningful: call every row
                    classified PureScalar, PureArgRead or DispatchCache with a
                    valid minimal input and assert a zero global-allocation
                    delta. Without that shim C4 detects nothing, and the plan
                    must not claim it does
C5  behavioural     the discriminating fixture is INDIRECT REACHABILITY, not
    control         absence. A caller that never passes the local to the row
                    cannot be affected under any classification, and reading
                    the local back keeps its store live, so that shape passes
                    identically for a correct and an incorrect claim. Instead,
                    for every row in a memory-claiming class, the fixture holds
                    a pointer the row DOES reach indirectly — a buffer.bytes()
                    view, an arena object, a pushed element buffer — writes
                    through it, calls the row, and checks the bytes at -O2. A
                    false argmem-limited claim that lets LLVM reorder or delete
                    that write is caught as a value mismatch. A second fixture
                    shape, under fd-count and environment observation, catches
                    a row classified as anything but HostState, Foreign or
                    Callback that touches process state
```

C4 and C5 together close the two failure modes inspection cannot: "this body
allocates and I did not notice" and "this claim is false in a way that changes
the caller's observable values". Both require machinery this PR adds — the
allocator shim and the indirect-reachability fixtures — and neither is a
benchmark or a client measurement.

### 3.6 The one experiment that could widen invariant D

Invariant D is what removes 1071's headline `argmem` claims, so the plan states
the single question that could restore them and refuses to guess the answer.

**Question.** Does LLVM treat memory reached by loading a pointer out of
argument memory as part of the `argmem` location, or only memory based on the
argument itself?

**What the plan currently assumes, and why.** Only the latter. The LangRef
defines `argmem` as *"accesses to memory via pointer values based on the
function's arguments"*, `based on` in the pointer-aliasing rules excludes a
pointer obtained by a load, and `AAResults::getModRefInfo` alias-queries the
argument locations with no reachability step.

**The experiment.** A standalone module declaring a helper
`memory(argmem: readwrite)` that a definition calls between a store to and a
load from a pointer obtained by loading through the helper's argument, run
through the same `default<O2>` pipeline the driver uses. If the load is
forwarded across the call, the strict reading is confirmed and invariant D
stands. It is a one-module experiment, not a benchmark and not a client build.

**If the strict reading is confirmed**, `IndirectStorage` is permanent, and the
route to 1071's criteria 3 and 4 is not an attribute at all: it is making the
caller's objects provably non-escaping so BasicAA can answer, which is plan 69's
territory, plus PR 3's removal of the call. That would be recorded here and in
plan 68 as a narrowing of G5's promised column.

**If it is refuted**, `ArgMem` regains its `Write` variant, V4 stops rejecting
write modes, `IndirectStorage` splits back into direct and indirect halves, and
the rows §3.3 lists gain the claims 1071 proposes. The record's shape does not
change, which is why the type carries the unreachable `ParamMode::Write` today.

This experiment is a PR 1 precondition, not a follow-up: the classification of
roughly a third of the table depends on its outcome.

### 3.7 Implementation closure matrix (PR 1)

Every class × every build mode × every declaration site. A cell whose behaviour
is identical across an axis is stated once for that axis rather than repeated.

**Class axis** — the required attribute set and the detector for a wrong row.

| Class | Required declaration | Failure mode if wrong | Detector |
| --- | --- | --- | --- |
| `PureScalar` | `memory(none) nounwind nofree nosync willreturn` | a hidden global read or allocation is optimized away | **C4** (needs the §3.5 allocator shim); V10 |
| `PureArgRead` | `memory(argmem: read)` + pure-finite flags + `readonly captures(none)` | a hidden write or allocation is dropped | **C4**; C5 indirect-reachability control; the existing `rt_contract_attrs_pin_encoding_and_curation` rows |
| `ArgRead` | `memory(argmem: read, inaccessiblemem: rw) nounwind` | a write through argument-reachable storage is reordered | C5; V5 |
| `AllocNew` | `memory(…, inaccessiblemem: rw) nounwind nofree`, `noalias` return via `returns_fresh` | a caller assumes non-aliasing the body does not provide, or a captured argument gets `captures(none)` | the existing `align_rt_array_builder_new`/`str_finder_new` allocator pins; V7; the `escapes` records §2.2 names for `builder_new` and `array_builder_new_in` |
| `FreeLocal` | `memory(argmem: rw, inaccessiblemem: rw) nounwind` | store-after-free when the released object is not the argument's own allocation | **I4 + V9**; C5. Only `align_rt_free` qualifies, so the cell is one row |
| `IndirectStorage` | `nounwind` only, memory withheld, `escapes`/`releases` recorded | a row is mistakenly promoted out of this class and gains an unsound `argmem` claim | **C5** indirect-reachability control; V4 rejecting a write mode; §3.6's experiment |
| `DispatchCache` | flags only, memory withheld, `readonly captures(none)` retained | a dispatch-cache row silently gains `memory(...)` | the existing `utf8_valid` / `str_find` / `str_finder_find` negatives, extended to the class; V6 |
| `HostState` | `nounwind` only | none: strictly conservative. It is the fail-closed sink | the fd/environment half of C5, which catches a row misclassified *out* of it |
| `Callback` | `nounwind` only, `returns_fresh` where the row has it | a callback's effects are assumed away, or `par_map` loses its `noalias` return | a par-thunk owner asserting no `memory` on the declaration; the golden's `declare noalias ptr @align_rt_par_map` line; V7 |
| `Foreign` | `nounwind` only | none: strictly conservative | — |
| `FailNoReturn` | `noreturn cold nounwind memory(inaccessiblemem: rw)` | `willreturn` on a diverging row | V8; the existing abort-family negative that forbids `willreturn` |
| `ProcessExit` | `noreturn nounwind memory(inaccessiblemem: rw)`, never `cold` | `noreturn` silently dropped, or a success-path exit marked cold | V8; the golden's eight-member `noreturn` group; a negative asserting `cold`'s absence |

**Build-mode axis.**

| Cell | Required behaviour | Owner |
| --- | --- | --- |
| whole-program | every declared row carries its class's exact attributes | the extended declaration golden |
| per-unit | every unit declares the same rows with byte-identical attributes | the existing "trivial whole-program and per-unit-shaped emitted IR with identical alphabetical runtime declarations" owner, extended to the attribute groups |
| ThinLTO | the same declaration appears in every partition with identical attributes; no partition disagrees | `thin_lto_sv`, plus a new cross-partition attribute-identity assertion |
| rt-LTO merged (R4) | a guarded row's declaration attributes are withheld before the merge, and the merged definition carries none of **that row's curated** attributes afterwards. `remove_attributes` is rewritten against the record, not against the deleted shape fields | the existing rt-LTO off/on XOR owner extended to every new attribute. The assertion is scoped to the row's own curated set, **not** "no enum attribute at any location": a `rustc`-compiled body legitimately carries its own `nounwind`, `noundef`, `nonnull`, `readonly`, `captures` and `uwtable`, and keeping them is the whole premise that LLVM re-derives the contract from the visible body |
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

### 3.8 Acceptance corpus

```text
language-level, new
  caller effect recovery: a kernel with a surviving bounds check keeps an
  inferred memory(read, ...) rather than collapsing to { nounwind }. This is
  the one language-level effect win PR 1 promises (1074 Tier 3)
  no regression in the withheld set: utf8_valid, str_find, str_rfind and
  str_finder_find still claim no memory effect and keep their readonly
  captures(none) parameters; the guarded rows still have theirs withheld
  before body linking (1071 criterion 5)

  NOT owners of this PR: 1071 criteria 3 (zero memsets for a dead local) and
  4 (LICM across a runtime call). Invariant D removes the attribute that would
  have delivered them; §3.3 records the narrowing and §3.6 the one experiment
  that could restore them

table-level, new
  runtime_effects_registry_is_total_and_structurally_valid, with one negative
  per V1-V12
  rt_lto_admission_predicate_matches_the_guarded_set (P1-P2) and
  rt_lto_guarded_bodies_meet_the_artifact_budget (P3-P5), one negative each
  runtime_effects_class_mutation_changes_the_golden
  no_runtime_export_uses_extern_c_unwind
  the derived-file-list form of runtime_export_source_inventory_matches_registry

machine controls, new
  the counting #[global_allocator] behind the alloc-count feature, plus C4
  allocation parity over every PureScalar / PureArgRead / DispatchCache row
  C5 indirect-reachability value controls over every memory-claiming row, and
  the fd/environment control over every row not classified HostState, Foreign
  or Callback

extended, existing
  rt_contract_attrs_pin_encoding_and_curation: one textual pin per §3.2 mask
  emitted_ir_round_trips_through_llvm_as: unchanged, now exercising the new
    attribute payloads
  the declaration golden: 464 lines each gaining a class token, plus a
    replacement attribute-group table
  scripts/test-runtime-abi-exports.sh: unchanged. It compares normalized
    signatures, not attributes, so the classification does not reach it
```

**Gate cost.** These owners are not leaf tests. `scripts/test-pr.sh` builds
`-p align_codegen_llvm -p align_mir -p align_driver … --lib`, so every
registry, golden, mutation and codegen owner above lands in a bounded-gate
binary under the hard 30-minute budget; only C4, which lives in
`align_runtime`, sits outside it. The parameterized controls are therefore
written as **one** table-driven owner each rather than one test per row, and
C5's fixtures reuse the existing driver IR-assertion harness instead of
compiling a new program per class. PR 1 records the measured gate delta and
ejects to the nightly suite if it does not fit.

## 4. PR 2 — one cold-path model for `Result`/`?`

### 4.1 The three tiers

Tier 1 records §2.5's edge at each of the nine recording sites that create one,
under the six kinds §2.5 names, and
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
predicate of §2.4 refuses it: `array_builder_push` is `IndirectStorage`, failing
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
                    (lib.rs:7136) and declare_imported_fn (:7223). This plan
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
                           TWO edits, both in PR 1. §3.1's G5 line reads "every
                           effects record added there is part of the runtime
                           artifact's identity"; §2.1 here finds that no cache
                           key component is added, because the record lives in
                           the compiler rather than in the runtime artifact and
                           compiler_build_id already covers it. That clause is
                           clarified to name the two identities that do apply,
                           compiler_build_id and rt_lto_digest. G5's promised
                           column is narrowed to match §3.3: "each runtime ABI
                           symbol carries a complete memory-effects record"
                           stands, "a complete memory-effects ATTRIBUTE" does
                           not, because invariant D makes withholding the
                           correct record for every indirect-storage row. It
                           already names "plan 70" as G5's implementing plan,
                           and that needs no edit
draft.md, docs/language-spec.md, docs/open-questions.md
                           UNCHANGED. No language surface, no settled decision
                           reopened, no new syntax, no new diagnostic
05-backend-llvm.md         UNCHANGED. Every attribute is emitted through the
                           existing add_enum_attr / add_valued_enum_attr path
16-test-policy.md          UNCHANGED as policy, but §3.8's gate-cost paragraph
                           is written against it: most of PR 1's and PR 2's
                           owners land in align_codegen_llvm / align_mir /
                           align_driver `--lib` binaries, which scripts/test-pr.sh
                           builds, so they ARE bounded-gate content under the
                           hard 30-minute budget. Only C4 sits outside it. Each
                           PR records its measured gate delta
crates/align_runtime       PR 1 adds a counting #[global_allocator] behind the
                           existing alloc-count feature (§3.5 C4) and derives
                           runtime_export_source_inventory_matches_registry's
                           file list from a directory read (§3.5 C1). Both are
                           test-only machinery, not ABI
```

## 8. Author-side ledger-to-prose consistency pass

- Every normative promise in §§3–5 appears in the §2 ledger, and every §2 public
  field has specified semantics: `class`, `argmem`, `params`, `escapes`,
  `releases`, `returns_fresh` and `diverges` each have a stated meaning, a
  validity rule (V1–V12) and an effect on the emitted declaration.
- The Cartesian product is exhaustive where it is externally meaningful. §2.2's
  derivation table has one row per class with no gap, §3.2 enumerates the
  complete class × `ArgMem` mask product and names the two forms that are
  unreachable by construction, and §3.7 crosses all twelve classes with five
  build modes and five declaration sites, stating for each axis the cells where
  behaviour is uniform rather than repeating them.
- The withheld classes have a total precedence order (§2.2), so a row that is
  more than one of them has exactly one class token and the golden cannot drift.
- Every argument and result has a concrete type, ownership, lifetime and
  allocation rule: the effects record is `'static` `Copy` and allocates nothing;
  `ExceptionalEdge` is owned by its `MirFn`; the fast path copies its scalar by
  value and retains nothing.
- No text or view crosses a native or wire boundary in this plan. One existing
  persisted identity is touched and is named rather than elided: the
  test-control ABI fingerprint `b"align-test-control-runtime-abi-v1\0"`
  (`runtime_abi.rs`, exported as `test_control_runtime_abi_fingerprint`)
  serializes `memory_argmem_read` today and must serialize the replacement
  record. PR 1 bumps its version tag to `v2` in the same change, because a
  fingerprint whose inputs change without its tag is the failure that tag
  exists to prevent. No new format is added, and the declaration golden — a
  textual IR pin with a mutation owner in both directions — remains the only
  other checked-in artifact.
- Multi-invalid input has a deterministic order: V1–V12 are stated in evaluation
  order and report the first violation with its symbol, matching
  `validate_registry`'s existing single-`Err(String)` contract.
- Every CLI and build input is explicit. The fast path has no flag; the effects
  record has no environment input; `--rt-lto` and `--no-rt-lto` keep their exact
  current meaning; §2.8 is a manifest constant, not an ambient setting.
- Every fingerprint is stated: `compiler_build_id` is the only *cache* identity
  that changes for PR 1 and PR 2; the test-control ABI fingerprint is a
  versioned identity that changes and is bumped with it; `rt_lto_digest` is
  unchanged and its independence from §2.8 is proved from `build.rs`'s own
  `rustc` invocation; the runtime archive is stated to be outside every cache
  key with the reason. §7 records the one plan 68 clause this reconciles.
- Every runtime inspection field is producer-owned: the classification is a
  `'static` table in the compiler and the layout pins are `const` assertions in
  the runtime. Nothing is read from an artifact or from source at run time.
- Every operation changing process-global native state is classified `HostState`
  and claims no memory effect, so no overlap-exclusion or restoration-order
  question arises from an effects claim. §2.2 records the region-teardown and
  indirect-release exclusions, and V9 enforces both.
- The one open technical question is named rather than assumed away. §3.6 states
  the LLVM `argmem` semantics question invariant D depends on, what the plan
  assumes, the one-module experiment that settles it, and what changes in each
  direction. It is a PR 1 precondition, not a follow-up, because roughly a third
  of the table's classification turns on it.
- Where the plan cannot keep an issue's promise it says so instead of restating
  it. §3.3 lists 1071 criteria 3 and 4 as not delivered by PR 1, §3.8 repeats
  that in the corpus, and §7 narrows plan 68's G5 promised column to match.
- Where a named control does not yet detect what it is assigned, the machinery
  is added rather than the claim softened: §3.5 C4 requires a counting
  `#[global_allocator]` (the existing counters see only `align_rt_alloc`), C1
  requires a derived file list (the existing inventory test reaches 379 of 464
  rows), and C5 requires indirect-reachability fixtures (an absent-pointer
  fixture passes for a correct and an incorrect claim alike).
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
  §3.7's detector column, J1–J5 to §4.3, K1–K5 to §5.2. The only benchmark is
  §2.7's, attached to the only explicit performance promise in the plan, and it
  is stated as a measurement rather than a correctness gate.
- Four issue claims were found wrong against the tree and are corrected in §1.3
  rather than repeated: `remove_attributes`' post-#1091 scope, the shipped
  archive's profile, the admissibility of the per-element rows to the rt-LTO
  set, and the `memory(argmem: ...)` claims 1071 proposes for the container and
  free rows.
