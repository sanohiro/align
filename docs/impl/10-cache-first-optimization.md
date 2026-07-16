# Cache-first compilation and output-code optimization

Status: **RECORDED 2026-07-12; artifact-collision C0 implemented 2026-07-13.** Private object/run/size
staging plus atomic executable publication closes §3; deterministic diagnostics, reproducibility,
and CAS work remain open. This is the durable source for the
cache-first audit requested after the LLVM 22 / macOS portability wave. It records confirmed
correctness defects, the required artifact-cache architecture, and new measure-first CPU-cache
candidates that are not already in the roadmap. Audit baseline: commit `ad7e4c8b57ad`, arm64 macOS,
LLVM 22.1.8, rustc 1.96.1.

The status labels in this document are deliberate:

- **CONFIRMED** — reproduced against the baseline, or directly proven from the implementation.
- **REQUIRED** — an architectural constraint for a sound cache, not an optional optimization.
- **PROPOSED** — the preferred design direction; settle its remaining details in the M15 review.
- **MEASURE-FIRST** — a plausible output/runtime optimization, not approved to ship until a written
  gate is met.
- **ALREADY PLANNED** — recorded only to draw the boundary; do not count it as a new finding here.

No language semantics or user-facing syntax are changed by this note.

---

## 1. Decision summary

The cache-first priority is sound, in both meanings of "cache":

1. **Build artifact reuse.** Make repeated builds skip work safely and make independent builds of
   the same input converge on one immutable artifact.
2. **CPU cache locality.** Reduce working-set size, memory traffic, active write streams, and
   cache-coherence traffic before pursuing instruction-level tricks.

The immediate order is:

1. Fix the predictable temporary-artifact collision. It is a correctness prerequisite for any
   parallel or cached build.
2. Pin reproducible artifacts and deterministic diagnostics with regression tests.
3. Add a whole-program content-addressed object cache without waiting for M15 unit boundaries.
4. In M15, split interface and implementation fingerprints so private edits do not invalidate
   consumers.
5. Repair the cache-sensitive benchmark methodology, then probe buffer donation, blocked AoS→SoA
   construction, and `task_group` batching. Add stack lifetime markers only after sound MIR liveness
   exists.

The existing M14 LTO ceiling probe and the existing Codex work queue remain independent. This note
does not reorder them except that the temporary-artifact race must be fixed before M15 parallel unit
compilation can be correct.

---

## 2. Boundary with already-planned work

The following are **ALREADY PLANNED** and are intentionally not presented as new findings:

- M14's runtime-bitcode / in-process-LTO ceiling probe, followed later by PGO and BOLT evaluation.
- M15 separate compilation, per-unit staleness/caching, parallel unit compilation, and ThinLTO once
  multiple Align units exist.
- Build-profile propagation into the `TargetMachine`, `optsize` / `minsize`, and a profile-aware
  runtime variant/cache key.
- Explicit benchmark export roots, the macOS binary-size script port, O(n²) sort replacement, the
  tiny-`par_map` pool-start fix, zero-size arena allocation, and attribute hardening.
- Measure-first JSON decode double-allocation and I/O zero-fill removal.
- Owned SoA columns, `soa_slice<T>`, packed-bool columns, hot/cold and useful-byte-ratio lints,
  non-temporal stores, cache-aware shaped operations, string blob/offset layouts, runtime SIMD,
  zero-copy I/O, and runtime static-library feature splitting.

Sources: the M14/M15 sections of `07-roadmap.md`, and the external binary-optimization / optimization
consultation adoption records in `open-questions.md`.

---

## 3. FIXED 2026-07-13: predictable temporary artifacts collided

### Audit-baseline behavior

`alignc build` and `alignc run` compile through a temporary object named only from the source
basename:

```text
<temp>/align-<stem>.o
```

`run` similarly uses `<temp>/align-<stem>` for the executable, while `size` uses
`<temp>/align-size-<stem>`. The relevant code is:

- [`build_per_unit_to` and `run`](../../crates/align_driver/src/main.rs) (the single per-unit build
  path since the M15 S2b flip — the old whole-program `build_to`/`main.rs#L307–346` helper is gone)
- [`size`](../../crates/align_driver/src/size.rs) (`run_size` → `build_per_unit_to`)

Two unrelated `/a/main.align` and `/b/main.align` therefore write the same object even when their
contents, target CPUs, profiles, or capability sets differ. One invocation can overwrite the object
between another invocation's codegen and link. `run` / `size` add a second collision on the
executable, and `size` may remove an executable another process is inspecting or about to execute.

The integration-test helper already includes the process id because it recognizes cross-process
temporary collisions, but the production driver does not apply that protection
([test helper](../../crates/align_driver/tests/common/mod.rs#L148-L161)). A process id alone would
still be insufficient for two concurrent builds inside one long-lived driver and would provide no
artifact identity.

### Reproduction

Two different programs were copied to separate directories under the same name `main.align` and
built concurrently for 40 pairs:

```text
build failures                                      0 / 40 pairs
the module-derived program linked the wrong object  6 / 40
the min-derived program linked the wrong object    34 / 40
pairs with at least one wrong executable            40 / 40
```

Concurrent `run` probes also produced wrong programs, signals, `ENOENT`, and "cannot execute binary
file" failures as the shared executable was replaced or removed. This is **CONFIRMED** and is not a
performance-only issue.

### Implemented first slice

Use the same artifact-identity mechanism for collision safety and caching:

```text
cache root / namespace / content key / artifact
                           ^
                           immutable after publication
```

For a missing key:

1. Create a private, unique staging directory under the cache root (`create_new`; never a predictable
   shared pathname).
2. Generate the artifact completely in staging.
3. Verify success and, where stored as CAS, verify the artifact digest.
4. Atomically rename/publish it at the content-key path. A per-key lock may suppress duplicate work;
   alternatively, racing producers may build independently and the losing publisher discards its
   byte-identical result.
5. Materialize the requested cwd output through its own temporary name + atomic rename. Never link or
   execute a partially written cache entry.
6. Remove staging data on failure; an interrupted build must not create a cache hit.

The first implementation uses atomically claimed private directories containing PID, time, and a
process-local nonce. Objects never leave private system-temp staging. Link output is created in a
private directory beside the requested executable and renamed atomically only after a complete
successful link, avoiding cross-filesystem publication. `run` and `size` each hold a separate
private directory through execution/reporting; Drop removes only the directory that invocation
claimed. This closes the race without pretending the later content-key CAS exists.

### Regression requirements

- [x] Two different same-basename programs build/run concurrently for many rounds; both always
  execute their own program (12 run pairs + 12 build pairs per gate).
- Same basename with different profile / target CPU never shares an object key.
- Many producers of one identical key all receive a complete byte-identical artifact.
- Kill the producer between codegen and publish; the next build reports a miss and rebuilds.
- [x] A `size` process cannot remove or inspect another process's executable (8 concurrent pairs).

The regression mutates object staging and run-executable staging independently back to shared
paths. The object mutation fails in round 0 with the wrong program; the run mutation fails with the
wrong program by round 6. Interrupted-publish fault injection and same-key CAS producer tests remain
for the content-key slice; private Drop cleanup and publish-after-link structure prevent failed
ordinary invocations from creating a partial final executable.

---

## 4. Reproducibility baseline: promising but unpinned

Normal location-free builds are already a good content-addressing substrate. Across three fresh
processes on the audit host, each of the following was byte-identical across all repetitions:

- `min.align`: raw LLVM IR, optimized LLVM IR, object, release executable, and tiny executable.
- `json_decode.align`: object.
- Executables built from different working directories.
- An object built through a differently named path/symlink to identical source.
- An object after a comment-only source change.

The audit also independently repeated `vec_sum.align` object and executable builds; each pair had
identical SHA-256 digests. No source path enters an ordinary build's MIR/codegen path. `explain-opt`
is intentionally different: its located MIR and debug metadata include file/line identity and must
live in another cache namespace.

This is empirical, not contractual. Existing profile tests prove that O0 and O3 objects differ, but
there is no test that identical inputs stay identical. Add a reproducibility suite before relying on
CAS:

- same process and fresh processes;
- relocated source trees and cwd;
- comment/format-only changes (canonical MIR/object identity, even if frontend diagnostics rerun);
- import graph ordering that is semantically equivalent;
- release / tiny and baseline / native namespaces;
- object and linked executable, on ELF and Mach-O;
- cold build output vs cache-hit output (bytes and execution result).

Do not promise cross-toolchain byte identity. A toolchain fingerprint deliberately creates a new
namespace.

---

## 5. Confirmed deterministic-diagnostic gap

Top-level constants are collected into a `HashMap`, then every independent constant is evaluated in
`decls.keys()` order ([constant evaluation](../../crates/align_sema/src/lib.rs#L1993-L2041)). Rust's
per-process randomized hash seed makes that order unstable.

One source containing four independent constant division-by-zero errors was checked in 24 fresh
processes. Its stderr produced **17 distinct SHA-256 digests**; observed line orders included
`2,4,3,1` and `3,1,2,4`.

This does not alter a valid machine artifact, but it breaks diagnostic/action caching, creates noisy
CI diffs, and makes the AI repair loop see a different problem order on identical input. It is
**CONFIRMED**.

Required correction:

- Evaluate independent roots in stable source-span order (dependency recursion remains memoized and
  naturally evaluates prerequisites first), or retain collection order explicitly.
- Render diagnostics in a stable final order as a defense-in-depth boundary.
- Add a subprocess test that repeats the same failing source and compares stderr bytes.

Do not cache failed frontend results until this is fixed and diagnostic cache identity includes the
compiler/frontend schema plus source-location identity.

---

## 6. Cache architecture

M15 currently says "per-unit staleness/caching" but does not define identity, invalidation, or
publication. The design review should settle those as first-class correctness rules, not driver
details.

### 6.1 Stage-separated action cache + CAS

Use action keys to map declared inputs to immutable result digests, and CAS to store the immutable
bytes:

| Stage | Action identity | Result |
|---|---|---|
| Frontend | source/import graph digests + frontend schema/options | checked HIR/MIR digest + diagnostics |
| Codegen | canonical location-free MIR digest + exact codegen/toolchain identity | object or bitcode digest |
| Link | ordered input-object digests + runtime/link environment identity | executable digest |

This separation matters:

- A no-op build can hit before parsing.
- A comment/format-only edit may rerun the frontend, produce the same canonical MIR digest, and hit
  codegen.
- Reverting source can recover an older CAS object without rebuilding it.
- `check`, normal build, and located `explain-opt` do not accidentally share incompatible results.
- Object caching remains useful even where non-hermetic system linking makes executable caching
  unsafe.

Do not use the formatter's current "significant token texts" helper directly as a semantic cache
key: it deliberately drops statement terminators and relies on reparsing as a second safety check.
Canonical checked HIR/MIR is the safer codegen identity. If a token-level frontend key is added, it
must retain normalized statement boundaries and every semantic token value.

### 6.2 Object/codegen key

At minimum, hash a canonical encoding of:

```text
cache-format/schema version
compiler build or codegen-schema identity
canonical location-free MIR
explicit export/root set
target triple + object format
resolved CPU + full feature set
profile + pass pipeline + TargetMachine optimization level
relocation/code model and codegen-affecting flags
exact LLVM build/version identity
LTO/runtime-bitcode mode and merged bitcode digest (when applicable)
instrument-PGO mode + profile-content digest (when applicable)
```

"LLVM major" is not a sufficient cache boundary. Minor/patch changes can alter pass pipelines,
instruction selection, textual attributes, or object bytes. Prefer an exact LLVM version/build id,
and include a compiler build/schema id so a codegen change cannot reuse an old object.

The key must use the **resolved** CPU/features. `native` as a string is not identity: two machines
resolve it differently.

> **ThinLTO S2 supersession (2026-07-16):** an earlier plan reserved a single *cross-unit-opt input
> digest* in this codegen key (empty in the non-ThinLTO build, to be populated once ThinLTO landed).
> ThinLTO instead composes as **two separate phase keys with their own CAS action namespaces** — a
> `prelink` bitcode key (this list MINUS the pure backend/target knobs) and a `thinbackend` object key
> (own prelink-bc digest ⊕ inbound imports ⊕ outbound exports ⊕ import-source prelink digests ⊕
> backend/target bits). Separate keys/namespaces give structurally stronger toggle isolation than one
> shared key discriminated by an empty-vs-non-empty digest (a `--thin-lto` object can never even
> address a non-ThinLTO action), so the reserved digest field was removed outright at S2 (no dead
> field). See `docs/impl/07-roadmap.md` "ThinLTO S2 SHIPPED".

> **Instrument-PGO S2 supersession (2026-07-17):** the codegen key gains a `pgo_mode` component of type
> `PgoKey { Off | Instrument | Use(Hash128) }` (the cache-side key type; the path-carrying CLI enum
> `PgoMode` is a distinct thing — `PgoKey` records the profile's content DIGEST, not its path), where
> `Use` carries the content digest of the merged `.profdata` BYTES (path-independent; computed once per
> invocation after the profile is validated, and those exact bytes are snapshotted so libLLVM optimizes
> with the digested profile — see doc note on `PgoKey::Use`).
> Unlike ThinLTO (two disjoint phase *namespaces*), PGO follows the `rt_lto`/`rt_lto_digest`
> precedent: **a component of the SAME codegen key**, not a separate CAS namespace — an instrumented /
> profile-use object is the same artifact KIND as an ordinary object (`.o` for the identical unit),
> just optimized differently, so distinct-tag key components (`Off`/`Instrument`/`Use(digest)`) give
> exactly the isolation needed while letting a use build re-hit and a profdata revert re-address its
> original CAS blob. The miss reason is `FirstDiff::PgoProfile` (a mode switch OR a profdata-bytes
> edit). `CACHE_KEY_FORMAT_VERSION` bumped 2→3, `MANIFEST_FORMAT_VERSION` 2→3. A PGO build now runs
> the NORMAL cached + parallel per-unit path (the S1 total bypass is gone); the only PGO-specific bits
> are the per-unit pipeline swap (`emit_object_pgo`) and the instrumented link. On an all-HIT
> profile-use build no LLVM runs, so no staleness diagnostics are (re)emitted — correct, because the
> staleness was already reported when each object was first built and is intrinsic to the cached
> bytes. See `docs/impl/07-roadmap.md` "Instrument-PGO S2".

### 6.3 Runtime and link key

The current runtime freshness check compares `libalign_runtime.a` mtime only with
`align_runtime/src/**/*.rs` ([implementation](../../crates/align_driver/src/lib.rs#L317-L396)). It
misses, among other inputs:

- `align_hash` changes (the runtime depends on it);
- `align_runtime/Cargo.toml` and `Cargo.lock`;
- rustc/toolchain and codegen flags;
- panic strategy, runtime profile, target, and native dependencies.

It can also false-miss on a content-preserving `touch` and false-hit when timestamps are restored.
Replace it with a runtime build manifest keyed by content/configuration. Independently, hash the
actual runtime archive bytes into every link action. If M14 merges runtime bitcode into the Align
module, its bitcode digest belongs in the object/codegen key instead.

A link key needs:

```text
ordered object/bitcode digests
runtime archive digest
object format + link flags + dead-strip policy
capability/user libraries in emitted order
profile strip decision
linker executable/version and target/sysroot identity
resolved search paths and other link-affecting environment
```

System `cc` + mutable system-library search paths are not hermetic today. Start the executable cache
as host-local. Object CAS is the portable/high-value layer; do not weaken its key in pursuit of a
cross-host link hit.

### 6.4 M15 interface vs implementation identity

Per-unit caching only has a high hit rate if a private implementation edit does not force every
consumer to rebuild. Each stable unit artifact should carry at least three independently hashed
parts:

1. **Public interface summary** — exported names/signatures, type layouts, generic body/instantiation
   information required by the chosen monomorphization model, and the conservative escape/region,
   effect/purity, and Move/Copy summaries required for sound cross-unit checking.
2. **Implementation body** — private + exported bodies needed to build this unit's object/bitcode.
3. **Link summary** — capability/native-library requirements and exported symbol/root information.

Consumers depend on the public-interface hash, not the implementation hash. Final link depends on
all implementation artifacts and the union of link summaries. Canonical encodings must not contain
process-local numeric ids or hash-map iteration order.

Soundness is fail-closed: an absent/unknown summary forces a conservative assumption or rebuild. It
must never recover the whole-program optimizer's former optimistic fact by guessing across a unit
boundary.

### 6.5 Observability

A cache that cannot explain misses will decay. Record, in a human and eventually machine-readable
form:

- hit/miss per frontend, unit, codegen, runtime, and link stage;
- the first differing key component / miss reason;
- stage wall time, bytes read/written, and peak RSS;
- cache corruption and forced-rebuild events.

The exact CLI surface is an M15 decision. The data model is required from the first slice so tests
can assert invalidation rather than infer it from elapsed time.

---

## 7. Cache validation matrix

The cache implementation is incomplete until this matrix is automated:

| Change/event | Expected result |
|---|---|
| no-op rebuild | frontend/unit/object/link hit |
| comment/format-only edit | diagnostics/frontend work as needed; canonical codegen hit |
| private function body edit | edited unit object miss; dependent-unit codegen hit; relink |
| public signature/layout/summary edit | reverse dependents miss |
| unused, unimported file edit | all existing build actions hit |
| profile/target/CPU/LLVM/compiler change | only the correct namespace misses |
| runtime source/dependency/flags change | runtime/link miss; no stale archive accepted |
| source edit then exact revert | old CAS artifact hit |
| many identical concurrent builds | one immutable result; no partial readers |
| different same-basename concurrent builds | distinct keys and correct executables |
| producer killed before publish | no cache entry; next build safely rebuilds |
| corrupted cache bytes | digest failure, eviction, automatic rebuild |
| cache hit vs cold build | byte-identical output and identical execution |

Add bounded cache eviction only after correctness and hit telemetry exist. Eviction policy is not an
artifact-identity concern and must not complicate the first slice.

**M15 SV status (2026-07-15): automated for the v1 object-cache boundary.** The frontend and link
always re-run by settled design, so their rows are safe-rerun requirements rather than cache-hit
claims. `cache_codegen.rs`, `cache_parallel.rs`, `artifact_staging.rs`, `per_unit*.rs`, and the
interface effect/codec tests jointly pin every applicable row: unimported edits, full key namespace
separation, transitive interface invalidation, runtime content freshness, exact revert, identical
and different same-basename concurrent producers, orphan staging after a killed producer,
corruption recovery, and cold-vs-hit byte identity. The interface reader also verifies its stored
public-surface hash before exposing effect bits. Future frontend/link cache layers must rerun this
matrix at their new stage boundary; this status does not pre-approve those deferred caches.

**ThinLTO stage status (2026-07-17): automated for the `prelink`/`thinbackend` phase boundaries.**
`--thin-lto` adds two cacheable phases (`CacheStage::ThinLtoPrelink` + `ThinLtoBackend`) with their
own CAS namespaces; the matrix rows that apply to those stages are pinned by `thin_lto_cache.rs` (S2)
and `thin_lto_sv.rs` (SV). `thin_lto_cache.rs`: private-body-edit precise backend invalidation,
public-signature transitive miss, import-sensitive precision, thin/non-thin namespace separation,
cold-vs-hit byte identity through both phases (same `-j`), parallel==`-j1`, cross-process all-hit,
and corrupted-prelink-blob digest-failure/eviction/rebuild. `thin_lto_sv.rs`: build-twice + cross-`-j`
cold determinism, a `-j2`-cold/`-j4`+`-j1`-hot byte-identical serve (the different-`-j` cold-vs-hit
row), a summary-level stale-summary mutation (a valid-but-different prelink `.bc` rejected on the
content digest — NOT merely on a parse failure), the profile/target/CPU/**LLVM/compiler** and
`--rt-lto` key-component rows at unit level (disjoint keys, never mixed), and an explicit
compile-time-regression bound. The serial thin-link (phase 2) is never cached (reruns every build),
so it has no cache-identity row. The ThinLTO arc (S0–SV) is CLOSED; see the roadmap
"ThinLTO SV SHIPPED" record.

---

## 8. CPU-cache candidates not already scheduled

These are **MEASURE-FIRST** candidates. Each changes implementation only; none creates a language
surface. Write the gate before prototyping and pin both a positive workload and a non-regression
control.

### 8.1 Donate a uniquely owned temporary buffer to a materializing pipeline

Current MIR explicitly recognizes a fresh unbound owned-array source as uniquely owned by the
consumer. For `make().map(f).to_array()`, `setup_source` returns `temp_free`; `lower_array_collect`
allocates a fresh output buffer, copies/filters into it, then frees the source
([collect lowering](../../crates/align_mir/src/lib.rs#L4234-L4418)).

For a heap-owned scalar source with compatible element size/alignment, the source buffer can become
the result buffer:

- `map`: load element `i`, compute, store element `i` back.
- `where`: stable compaction is safe because `out_index <= source_index`; the current element is
  loaded before its slot can be overwritten.
- `scan`: safe under the same compatible-layout rule; each source element is consumed before the
  running result is stored at or behind it.

Start with the mechanically safe subset:

```text
temp_free is present (fresh unbound heap ownership)
source and result scalar layouts are identical
no struct/string/Move payloads
no arena reuse in the first slice
no exposed alias/view of the source
```

The output remains a visible `.to_array()` materialization whose storage comes from a visible owned
input allocation; the optimization only removes redundant storage. A design review must still
confirm that this allocation elision is consistent with Nothing-hidden and the explicit
`map_into(out)` surface. Do not silently generalize donation to a bound local without real last-use
and borrow-liveness analysis.

Gate signals:

- one fewer allocator call and no source `DropValue` free/reallocate pair;
- lower peak RSS / resident bytes on a large owned temporary;
- lower wall time or cache/write-traffic counters on map and selective-where workloads;
- byte-for-byte result parity, drop/allocation-counter tests, and no regression for borrowed inputs.

### 8.2 Cache-block the AoS→SoA construction loop for wide structs

`transpose_to_soa` currently walks one row at a time and stores every field into a far-separated
column before advancing to the next row
([implementation](../../crates/align_mir/src/lib.rs#L4563-L4639)). Each column is individually
sequential, which is ideal for narrow structs, but a wide struct creates many simultaneous write
streams. Once the field count exceeds the store-buffer/cache-line working set, stores can thrash
active lines and page translations.

Probe a two-dimensional construction tile:

```text
for each small row block whose AoS bytes fit a chosen cache budget
  for each small field block
    transpose those rows × fields into the final plain-SoA columns
```

This is **not** the rejected automatic AoSoA/chunked-SoA representation. The final memory layout and
all later column scans remain ordinary SoA; only the one-time transpose loop order changes. A field
block may reread the row tile, so the row tile must fit cache and the threshold must be earned on
wide structs. Narrow structs are the non-regression control and should keep the current single loop
unless the probe proves otherwise.

Measure at multiple row sizes and field counts (for example 4/8/16/32 scalar fields), recording
wall time, stores, cache/TLB misses, and final column-scan parity. Ship only with a clear crossover
and a simple target-independent threshold or a settled target-data query.

### 8.3 Batch `task_group` claiming and completion

The parallel-runtime shape, newly confirmed nested-pool deadlock, and wider generated-IR audit are
recorded in [`11-parallel-execution-optimization.md`](11-parallel-execution-optimization.md). This
subsection remains the cache-locality gate; document 11 is the parallel correctness source.

`align_rt_tg_wait` performs one shared atomic `fetch_add` per task and, after every task, locks one
shared `TgBarrier`, updates it, and calls `notify_all`
([implementation](../../crates/align_runtime/src/lib.rs#L7318-L7395)). For many tiny tasks, the
task body can be cheaper than the cache-coherence and mutex traffic.

Probe runners claiming a small contiguous batch of indices at once and accumulating completion
locally. Merge one local result per batch/runner; lock only for a real error/panic or the final
completion transition. Preserve all existing semantics:

- caller participation and nested-group deadlock freedom;
- every task claimed exactly once;
- deterministic lowest-index error and panic selection;
- all tasks joined before region release.

This is distinct from the already-planned `n == 1` fast path. Gate across task count and body-cost
sweeps; heavy-task performance must stay flat while tiny-task throughput improves materially.

### 8.4 Emit stack lifetime markers after sound liveness exists

Codegen allocates every MIR slot in the function entry block and emits no
`llvm.lifetime.start/end` markers
([slot allocation](../../crates/align_codegen_llvm/src/lib.rs#L3380-L3410)). Entry allocation is
correct and prevents loop-growing stacks, but without lifetime intervals LLVM has less information
for stack coloring/reuse of large non-overlapping slots.

Do not derive markers from lexical scope alone while borrow liveness is incomplete. Sequence this
after the planned MIR-dataflow / borrow-liveness work and derive markers from proven last use,
including every branch, early return, `?`, cleanup, captured environment, and FFI escape. An early
`lifetime.end` is optimizer UB and can miscompile.

Gate on functions with disjoint large fixed arrays/struct temporaries: optimized frame size, stack
slot reuse, cache/TLB counters, and full behavior tests. Scalar allocas promoted by mem2reg are the
negative control; they should show no benefit.

### 8.5 MEASURED FUTURE — consumer-gated indexed-read locality

Sequential, known-stride, and data-dependent indexed access are different execution shapes, but this
does not justify source syntax or a general access-pattern optimizer. The current structural split is
the right default: ordinary array/pipeline nodes expose contiguous loops; SoA projections expose
contiguous columns; explicit index/gather nodes remain visibly indexed. When the first reusable
selection-vector, index-list, or `bitset` consumer lands, MIR must preserve two additional legality
facts rather than infer them late from pointer arithmetic:

```text
access order       contiguous / known-stride / data-dependent-index
reordering         forbidden / read-only-and-order-insensitive
```

Do not add a first-class `AccessPlan` type before that consumer. The facts may initially remain
properties of its dedicated MIR node. They must never authorize reordering an Impure callable,
ordered floating-point reduction, trapping operation, scatter with duplicate indices, or observable
output order.

**Ceiling probe (2026-07-14, Ryzen 9 5950X, LLVM 22, `-O3 -march=znver3`):** one wrapping `u32`
reduction performed 1,048,576 read-only indexed loads. Random indices were compared with natural
LLVM code, scalar code, explicit AVX2 gather, fixed-distance software prefetch, full sorting, and a
stable counting pass that grouped indices by 4 KiB source block. The construction timings below
include allocation and release of the reordered index buffer and counting scratch. Corrected
steady-state medians were repeated twice after a memory barrier was added between identical calls:

| source working set | random natural | block-grouped execution | block-plan construction | conservative reuse break-even |
|---:|---:|---:|---:|---:|
| 32 KiB | 0.206–0.216 ms | 0.192–0.193 ms | 2.53 ms | 109–179 uses |
| 512 KiB | 0.481–0.542 ms | 0.194–0.195 ms | 2.44–2.47 ms | 7–9 uses |
| 8 MiB | 0.874–1.232 ms | 0.268–0.269 ms | 1.32–1.39 ms | 2–3 uses |
| 128 MiB | 7.89–8.51 ms | 3.50–5.02 ms | 2.22 ms | 1 use in this probe |

The useful result is not “sort random access.” Full sorting cost 34–49 ms and needed roughly
9–2,100 reuses depending on the working set. Explicit AVX2 gather was generally slower than the
natural/scalar lowering. Per-element software prefetch regressed cache-resident cases; it helped the
128 MiB random case by roughly 1.2–1.4x at the best tested distance, which is target- and
latency-specific and does not earn a generic compiler mechanism.

Record one narrow future plan: if a visible operation constructs a read-only, order-insensitive
index/selection plan and reuses it across multiple large column reductions, probe cache-block
grouping behind the unchanged source operation. Keep direct indexed execution for short, cached,
single-use, ordered, trapping, or effectful cases. Adoption requires an end-to-end consumer on x86
and arm64, construction/allocation included, exact duplicate-index behavior, no float reassociation,
and a simple working-set/reuse gate. Otherwise record-and-close it. No new language surface is
recommended.

---

## 9. Cache-sensitive benchmark methodology

Before accepting any CPU-cache optimization, fix two measurement gaps.

### 9.1 Balance execution order

Several head-to-head harnesses say they alternate implementations, but every measured round is
fixed `Align → Rust`:

- [`bench/harness.rs`](../../bench/harness.rs#L72-L86)
- [`bench/group_by`](../../bench/group_by/src/main.rs#L75-L88)
- [`bench/json_decode`](../../bench/json_decode/src/main.rs#L106-L123)

With shared input, the second implementation may inherit cache warmth or settled frequency from the
first. Replace fixed AB rounds with balanced AB/BA (or a fixed-seed balanced order), keep separate
minima/distributions, and retain the existing correctness equality check. The sequence itself must
be reproducible.

### 9.2 Sweep cache hierarchy boundaries

Current sizes (`100k`, `1M`, `50M`, etc.) cover broad regimes but do not identify where performance
changes as the working set leaves L1, L2, LLC, and DRAM. For locality claims, record sizes around
`0.5× / 1× / 2×` each relevant cache capacity, expressed in bytes actually read/written rather than
only element count.

Record at least:

```text
ns/element and absolute wall time
useful bytes vs total bytes moved
allocations/materializations and peak RSS
cycles/instructions where available
L1/LLC/TLB misses where available
```

Hardware counters and cold/warm separation are already in the external audit checklist. The new
requirement here is balanced ordering plus the hierarchy-boundary working-set sweep. Cross-platform
tests must still run without counters; counters enrich a benchmark, never gate functional CI.

### 9.3 Sweep the relevant data and runtime state, not only byte size

**Cross-audit 2026-07-14:** the current records already cover most state-dependent crossovers, but
they were scattered across documents 10-13. The missing active case was presorted/ordered-run input
for the now-shipped stable merge sort; document 12 records its measured adaptive path. Use this matrix
when accepting any future fast path so a single uniform-random, warm-cache throughput number cannot
stand in for the operation's real state space:

| State axis | Required representative cases when relevant |
|---|---|
| order/locality | sorted, reverse, ordered runs, random; contiguous, known-stride, indexed |
| predicate distribution | 0/1/10/50/90/99/100% selectivity; clustered and random masks |
| key distribution | dense range, sparse range, low/high cardinality, all-unique, duplicates, skew and collision stress |
| value structure | ASCII/mixed UTF-8; invalid head/middle/tail; repetitive/incompressible bytes; zero/extreme values and float NaN/Inf where legal |
| reuse and warmth | one-shot/reused plan; cold/warm cache, allocator, CPU dispatch, thread pool, and file mapping |
| representation boundary | element width, alignment, legal alias/overlap, page boundary, cache/TLB working-set transitions |
| concurrency | uniform, one straggler, alternating and heavy-tailed cost; nested and blocking work; worker-count sweep |
| external capability/state | regular file/socket/pipe/special file, buffered lookahead, target ISA and portable fallback |

This is a selection checklist, not a Cartesian-product requirement. Each optimization names the axes
that can change its algorithm or cost class, includes adversarial negative controls, and preserves a
simple default when a reliable low-cost discriminator is unavailable. Runtime adaptation stays an
implementation detail only when results, error/effect order, allocation visibility, and worst-case
complexity remain within the source contract; otherwise require an explicit shaped operation or do
not adopt the transform.

---

## 10. Implementation sequence and stop conditions

### Slice C0 — artifact correctness and determinism

- [x] Replace predictable shared temporary paths with private staging + atomic publication (2026-07-13).
- [x] Add concurrent same-basename build/run/size regression tests (2026-07-13); retain explicit
  interrupted-publish fault injection for the content-key publication slice.
- Sort independent constant evaluation/diagnostic output deterministically.
- Add the byte-reproducibility suite for normal object/executable output.

**Completion:** no shared partial path exists; the race mutation is caught; identical cold builds
are byte-identical on the supported object formats.

### Slice C1 — whole-program object CAS

- Define cache schema and exact object key.
- Cache the current one-Program object before M15 changes the unit boundary.
- Hash the real runtime archive into link identity; replace the incomplete mtime freshness premise
  with a content/configuration manifest.
- Emit structured hit/miss reasons.

**Completion:** no-op and source-revert builds hit; comment-only changes can hit codegen; every key
mutation in the validation matrix causes the intended miss; cache-hit and cold outputs match.

### Slice C2 — M15 unit summaries and incremental invalidation

- [x] Settle the unit boundary/artifact format and generic strategy (M15 S2/S2b + the S3 design
  record in `07-roadmap.md`).
- [x] Canonicalize interface, implementation, and link summaries (`align_interface::codec`, M15 S2).
- [x] Rebuild only reverse dependencies whose interface inputs changed — **M15 S3a** (opt-in codegen
  cache): the per-unit codegen key folds the unit's own `impl_hash` + its transitive dependency
  interface hashes, so a private-body edit misses only the edited unit and every dependent hits
  (`crates/align_driver/src/cache.rs`; gates in `crates/align_driver/tests/cache_codegen.rs`).
- [x] Compile independent misses in parallel using the C0 publication mechanism — **M15 S3b**
  (`align_driver::codegen_units_parallel`): serial lookups, then `std::thread::scope` workers produce
  only the misses (fresh LLVM `Context` per unit; native target initialized once on the main thread
  before the scope); results stay DAG-ordered.

**S3a landed (2026-07-15):** the codegen-stage action cache + CAS substrate (`align_driver::cache`),
serial cache wiring for `build`/`run`/`size`/`emit-obj`, the `CacheOutcome`/`FirstDiff` observability
model, and the doc-10 §7 invalidation gates — **opt-in** via `ALIGNC_CACHE`.

**S3b landed (2026-07-15):** parallel codegen over cache MISSES (`-j`/`--jobs` + `ALIGNC_JOBS`, default
`available_parallelism`; `-j 1` byte-identical to `-j N`); `--cache-stats` (per-unit hit/miss + summary
on build/run/size); `alignc cache clear` (removes the `cas`/`actions`/`index` subtrees under the
resolved root); the runtime-archive freshness check moved **mtime → content digest** (§6.3) — a
source-content digest baked at build time (`build.rs`) is compared at link time, killing the false-stale
`touch`/checkout papercut while keeping the teeth, plus `runtime_archive_digest()` as the future
link-key input; and the **default-ON flip** — unset `ALIGNC_CACHE` now enables the cache at the XDG
root (`off` still disables). Test infrastructure isolates every `alignc`-binary-spawning test with an
explicit `ALIGNC_CACHE=off` so the default-ON cache never makes an assertion nondeterministic or
pollutes the user cache.

**Completion:** a private-body edit recompiles one unit plus relink; public soundness-summary changes
invalidate the right reverse dependencies; no missing summary is treated optimistically.

### CPU-cache probes

After benchmark export roots and balanced measurement work:

1. uniquely owned buffer donation;
2. wide AoS→SoA blocked construction;
3. `task_group` batch claiming/completion;
4. stack lifetimes only after MIR liveness.
5. reusable read-only indexed block plans only with a selection-vector/index-list/`bitset` consumer
   and the §8.5 working-set/reuse gate.

Record below-gate results and close them, following the arena-reuse and cold-edge precedents. Do not
keep an unearned second mechanism.

---

## 11. Claude Code handoff checklist

When resuming this work in a later session:

1. Read `HANDOFF.md`, `CLAUDE.md`, the M14/M15 sections of `07-roadmap.md`, then this document and
   the parallel companion `11-parallel-execution-optimization.md`.
2. Preserve the landed private-staging/atomic-publication C0 gate; never reintroduce a shared partial
   artifact path when adding content keys or parallel M15 compilation.
3. Re-run the confirmed reproductions on the current HEAD before editing; line numbers may have
   moved, but the linked functions are the authoritative sites.
4. Keep confirmed fixes separate from measure-first CPU candidates.
5. For code changes, follow the repository's self-review and adversarial gate workflow; cache-key
   omissions are correctness defects, not minor performance findings.
6. Update this document's status ledger and the short roadmap/handoff pointers as slices land. Do not
   duplicate the full design into `HANDOFF.md` or `open-questions.md`.
