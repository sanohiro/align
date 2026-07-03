# Session handoff (continue on another machine)

A living continuity note so a fresh Claude Code session — e.g. on a faster machine — can pick the
work up immediately. **If you are a new session: read this, then `CLAUDE.md`, then
`docs/impl/08-nested-structs.md`.** Everything durable is in this repo; the conversation history and
Claude's per-machine memory do not travel with `git clone` (see "Memory" below).

_Last updated: 2026-07-05 (M10 Slice 2 — std.rand — DONE)._

## M10 — std (encoding / rand / cli) — design settled (2026-07-04); Slices 1–2 (encoding, rand) DONE

**Slice 2 — std.rand — DONE.** Copy `rng` ([`Ty::Rng`] = Xoshiro256++ `[4 x i64]`, value not Move —
never on the move/drop/escape path); `rand.seed()` (OS `getrandom`, abort on the rare failure) /
`rand.seed_with(s)` (SplitMix64 deterministic); `r.next()`/`r.range(lo,hi)` (Lemire, `lo>=hi` aborts)
/`r.shuffle(out xs)` (in-place Fisher-Yates) /`r.sample(xs,k)` (partial Fisher-Yates → owned
`array<T>`, `k<0`/`k>len` aborts) take a **mut** receiver. All rand nodes Impure (excluded from
`par_map`). New `Ty::Rng` swept Copy/`Static` through every pass; HIR `Rand*` + MIR `Rvalue`s +
`align_rt_rng_*`; `tests/m10_rand.rs` (12) + runtime units (5). Only Slice 3 (std.cli) remains.


**Slice 1 — std.encoding — DONE.** `encoding.base64_encode`/`base64_decode`/`base64url_encode`/
`base64url_decode`/`hex_encode`/`hex_decode`/`utf8_valid`: encode (byte view) -> owned `string`,
decode (`str`) -> `Result<buffer, Error>` (invalid -> `Error.Invalid`), `utf8_valid(bytes)` -> `bool`
reusing the #344 validator. New `Scalar::Buffer` owned-Move payload (reader/writer precedent) carries
the decoded `buffer`; scalar reference impl (SIMD later, same signatures). sema dispatch + MIR
`Encoding*`/`Utf8Valid` + `align_rt_*` runtime; `tests/m10_encoding.rs` (11) + runtime units (7).



**M10 std-2 design settled: `std.encoding` / `std.rand` / `std.cli`** — all three close over
existing mechanisms (`str`/`bytes`/`buffer`, `mut` slice, `main(args: array<str>)`'s `array<str>`)
with zero new Move types, zero new effects, and no FFI engine; `rand.seed`'s OS-getrandom call is
the only new runtime primitive. Full signatures in `draft.md` §18.2; slice breakdown + completion
conditions in `docs/impl/07-roadmap.md` M10; scope rationale in `docs/open-questions.md` Settled →
"M10 scope decision". **`std.net`/`std.http`/`std.process`/`std.compress`/`std.crypto` → explicitly
M11+** (each needs a new Move type, an FFI engine, or an unsettled design question — recorded per-
module in the roadmap's M10 deferral list); `process.exit`'s Drop/arena-cleanup semantics is a new
Open item to settle when `std.process` is designed. Implementation (M10 Slices 1–3) has not started.

## M9 — std (I/O, filesystem, path, env, time) — formally closed (2026-07-04)

**M0–M9 COMPLETE** (language core + tooling/FFI + std phase 1). All four M9 slices are done and
their completion conditions independently met (design settled #336; shipped #337–#340): **`std.io`**
core (`reader`/`writer`/`buffer` Move types + a shared errno→`Error` table), **`io.copy`** (a
non-consuming portable fixed-buffer transfer, `Result<i64, Error>`), **`std.fs`** complete
(`write_file`/`exists`/`remove`/`read_dir` plus the arena-scoped `mmap` view `read_file_view`), and
**`std.path`/`std.env`/`std.time`** (`path.join`/`normalize`/`base`/`dir`/`ext` views, `env.get`/
`env.set`, `time.now`/`instant`/`sleep`). See the M9 section of `docs/impl/07-roadmap.md` for the
full shipped-feature summary. **Not blockers, deferred as post-M9 backlog** (own labeled subsection
right after M9 in the roadmap): `io.copy` syscall fast paths (`sendfile`/`splice`/`io_uring`), no
`SIGBUS` handler on post-`mmap` truncation, non-UTF-8 filenames from `read_dir` (a raw-byte caveat
for `string` ops that assume UTF-8), dropping unbound Move temporaries (a one-shot
`io.stdout.write(x)?` leaks its writer handle today), streaming×pipeline integration, and the M10+
module set (`std.net`/`std.http`/`std.cli`/`std.process`/`std.encoding`/`std.compress`/`std.rand`/
`std.crypto`).

## M8 — Tooling and Quality — formally closed (2026-07-03)

All four completion conditions are met: the formatter (#233, `align_fmt`); `unsafe`/`raw.*`
(#262–264); `extern "C"` FFI v1 (#265–269) plus by-value struct passing shipped beyond the v1
boundary (x86-64 SysV only, #329); and the lint suite's full **profile-independent** slice —
unhandled-`Result` (#138), huge-struct-copy (#234), lossy-cast + wasteful-default-element (#313),
unnecessary-heap narrow form (#323) — five lints that never need runtime/profile evidence. See the
M8 section of the roadmap for the full shipped-feature summary. **Not blockers, deferred as post-M8
backlog** (own labeled section right after M8 in the roadmap, and `docs/open-questions.md` → "M8
lint candidates"): the frequency-dependent lints (allocation-in-loop, the broader
unnecessary-clone/unnecessary-heap forms, branch-in-hot-loop, string re-scan, implicit copy),
`prefer-pipeline-over-vecN` (no firing surface — no loop construct exists yet), and the hot/cold
field-split suggestion (heuristic design needed).

## M6 — SIMD / vec / mask — formally closed (2026-07-03)

Both completion conditions in `docs/impl/07-roadmap.md` are met and re-verified: `emit-llvm` on a
`vecN<T>` program shows real `<N x T>` IR, and `where(p).<reducer>()` is branch-free for every
reducer (`sum`/`count`/`min`/`max`/`any`/`all`/`reduce`), same as PR #303 established. See the M6
section of the roadmap for the full shipped-feature summary. **Not blockers, deferred as post-M6
backlog** (own labeled section right after M6 in the roadmap, and `docs/open-questions.md`): owned
SoA columns, `soa_slice<T>`, packed-bool columns, dynamic/arena over-aligned arrays.

## Internal review (2026-07-02)

A same-day multi-agent internal review (4 parallel deep-dive tracks — frontend soundness / MIR+LLVM
codegen / runtime+library / language-design evaluation — plus the design-evaluation question put to
Opus and Codex **independently**, which converged on the same conclusions). Distinct from, and no
overlap with, the external soundness audit below. Findings recorded in `docs/open-questions.md` under
"2026-07-02 internal review" (Open, near the external-audit section). **All bug findings were fixed
the same day, PRs #293–#297**: the `AssignField` MoveCheck gap + `arena_alloc` raw cast (#293), the
division-by-zero/`INT_MIN÷-1` LLVM-UB guard — zero aborts via `align_rt_div_fail`, `INT_MIN/-1`
wraps, constant divisors skip the guard (#294), `json.decode` out-of-range integers — sign bit added
to the field tag, an ABI change across codegen+runtime (#295), the expression-depth ICE — post-parse
`cap_expr_depths` with a measured 128 ceiling (#296), and the `chunks`-over-local-array region hole,
including the str-array storage-vs-element-region case Gemini caught on the PR (#297). **Still open:
the perf backlog** — led by missing LLVM no-alias metadata on fused loops and `task_group` spawning a
thread per task instead of reusing `ParPool` — and the design-decision Open items (out-of-range
literals, `main() -> Result<(), E>` exit mapping). The
design-facing conclusions from the same review (MIR must carry vectorization-enabling *properties*
and never bake in a fixed vector width — vector width stays a permanent backend decision; a two-tier
SIMD story where `vecN<T>`/`maskN<T>` stay the fixed-width kernel escape hatch and the pipeline is the
width-agnostic main path where scalable ISAs live; `str + str` is now a hard error, not a lint
candidate; unconstrained-literal defaults and `&&`/`||` short-circuit order are now explicit in the
spec) were landed the same day in `draft.md` / `docs/design-notes.md` / `docs/impl/*` and recorded as
Settled/Future entries in `docs/open-questions.md`. **The perf backlog from this review is now done
(PRs #300–#303, same day):** alias-scope metadata was investigated and **deferred with the sound
encoding documented** — the mechanism proven but no source construct generates an aliasing-ambiguous
loop today (belongs with the future `map_into(out)` slice); the investigation surfaced and #302
fixed a real soundness hole (`out` no-alias check blind to sub-slices); `task_group` now reuses
`ParPool` via a caller-participating claim loop (nesting-deadlock-free by construction, #301);
allocator declarations carry verified `noalias`/`nounwind`/split-`nofree` attributes and `emit-llvm`
output is self-describing (triple, #301); and the branchless identity-select `where` now covers
every reducer — the M6 completion criterion — with `where`+`min` demonstrably vectorizing (#303).
**Remaining from the review: the design-decision Open items** (out-of-range literals,
`main() -> Result<(), E>` exit mapping) and the deferred noalias emission gated on `map_into(out)`.

## Improvement wave (2026-07-02–03, PRs #306–#314)

A queue-driven improvement wave off the same review backlog, all merged same-day(-ish): **#306** closed
the json speculative-path duplicate-key gap (zero new state; cost confined to records with undeclared
colons); **#307** made default struct layout **unspecified order** — fields now reorder by descending
alignment (Rust-style) while `layout(C)` stays fixed, sema/codegen parity pinned by a mutation-checked
test; **#308** restricted `main`'s error type to the builtin `Error` and fixed a real ICE along the way
(user-`E` `main` was reaching codegen's hard-coded `Error` layout); **#309** made out-of-range integer
literals a hard error, including nested-negation effective values, while pattern literals keep wrap per
spec; **#310** confirmed `str` search was already memchr-SIMD (AVX2+NEON+scalar) and added a
differential SIMD-vs-scalar oracle; **#311** made json `u64` fields accept the full `u64` range (three
write sites unified through one dispatcher); **#312** evaluated `Option` niche optimization and deferred
it — no expressible target type today, since `Option` payloads are scalar-only — with the revisit plan
recorded; **#313** shipped M8 lints batch 1 (lossy `as` conversions, unconstrained-default large array
literals, both post-inference classification); **#314** was a clippy sweep, 50 warnings → 0 with zero
`allow`s, and unified the runtime's byte-copy loops on `copy_nonoverlapping`. **Held/deferred** (reasons
recorded in `docs/open-questions.md`): hot/cold field-split lint (heuristics need design), buffered
`print` (deliberate), escape→MIR dataflow + purity-as-effect-bit (structural, big), relative pointers
(no recursive types yet), `f16`/`bf16` (arithmetic semantics decision needed). Tests grew ~1047 → ~1103;
clippy is clean at `-D warnings`. **Next:** continue roadmap work (M6 is now formally closed, see
above; M8 remainders) — the queue is derived from `docs/impl/07-roadmap.md`.

## Roadmap-remainder wave (2026-07-03, PRs #316–#324)

Continuing the same queue, all merged with reviews reflected (#317 is absent below — closed
unmerged after a branch mishap and re-landed as #318): **#316** added dynamic `array<Struct>`
element-field write (`StoreElemFieldPtr`, the write dual of `IndexFieldPtr`); **#318** shipped
lane-wise `%` for `vecN<T>` and closed the unguarded vec integer-division UB residual alongside it (lane-wise
zero-abort + `INT_MIN`/`-1` wrap, plus a broadcast-constant fast path); **#319** gave over-aligned
struct arrays a padded stride (`round_up(size, align)`, C-style tail padding; dynamic
`array<align(N)S>` stays rejected pending aligned heap alloc); **#320** added the `align(N)` binding
form for numeric scalar arrays — the aligned-vector-load enabler (a proven-or-nothing aligned-load
switch); **#321** converged canonical hashing into a new `align_hash` crate (wyhash) so the JSON PHF
byte-match is now structural, with `group_by`/`dict_encode` 1.4–1.8× faster; **#322** shipped
str-bearing `soa` element writes (read columns had already landed; the write path was the real gap);
**#323** shipped M8 lint batch 2 (`unnecessary-heap`, narrow single-node form; `prefer-pipeline-over-vecN`
held — no loop syntax exists yet to fire on); **#324** fixed a **class-closing miscompile**: `check_expr`
now reconciles every value's type with its expected context at a single reconciliation point (found via
#323's side discovery), surfacing ~10 latent silent-truncation spots in the test corpus, now explicit
`as` casts. Tests grew ~1103 → ~1147. M6/M8 roadmap remainders are now essentially consumed;
deferred-with-record: owned `soa` columns, dynamic over-aligned arrays, cross-function aligned slice
loads, the `prefer-pipeline` lint (needs a kernel surface).

## Third wave (2026-07-03, PRs #326–#330)

**#326** extended the differential fuzzer to pipeline reducers and `vecN` lane arithmetic
(mutation-checked, no miscompiles found); **#327** formally closed M6 with both completion conditions
re-verified; **#328** shipped the `map_into(out)` pipeline terminal with the thrice-deferred
out-slice noalias emission — overlap guards went 3→0 at `-O2` — and the Claude-side fallback review
caught a CONFIRMED false-noalias miscompile (call-laundered aliasing args) pre-merge, fixed by
conservatizing the caller-side out-disjointness check; **#329** shipped FFI by-value structs
(x86-64 SysV, ≤16B register class) with clang-verified flattening, and the fallback review caught a
CONFIRMED SysV atomicity miscompile under register pressure, resolved by a compile-time GP/SSE budget
walk after aggregate-coerce was empirically disproven; **#330** decided the `soa_slice<T>` repr
(windowed 4-word soa view, unification not a new type) with implementation deferred pending a
concrete consumer. **Review process note:** `gemini-code-assist` ran out of daily quota mid-wave; per
owner instruction the review gate switched to Claude-side fallback reviews (deep-reasoner adversarial
pass + re-verification of fixes by the original reviewer). This caught two real miscompiles pre-merge
(#328/#329) — the gate is load-bearing. Tests grew ~1147 → ~1190.

## Latest (2026-07-02, PRs #262–#290)

Since the #183 snapshot below: **M8 unsafe/`raw.*` + `extern "C"` FFI v1 shipped** (#262–#269:
`extern "C"` decls + unsafe-gated foreign calls, `layout(C)` struct-by-pointer ABI, `str`/`slice`/
`bytes` → C data pointer, `link("name")`; deferred: by-value struct return, `bool`/`char` args,
`ptr_cast`). The **2026-07-02 external soundness audit is fully addressed** (#270–#277: escape/effect/
move coverage holes, `&&`/`||` short-circuit, arena double-free, negate-unsigned sign loss, parser/
diagnostic papercuts). Owned-array materialization `.to_array()` + a clean error for bare-literal-in-
owned-array-context (#283–#285). A **dependency-free fuzz + property suite** now locks the invariants
(#286–#290): `fuzz_frontend.rs` (front end never panics), `fuzz_fmt.rs` (formatter idempotent +
parse-preserving), `fuzz_differential.rs` (generate-program-with-oracle differential fuzzer over
scalars / all widths + casts / call ABI / struct+array aggregates — **no miscompile found**). `cargo
test` ≈ 990+ green. Remaining audit items are the structural refactors (escape→MIR-dataflow,
purity-as-effect-bit) tracked **open** in `docs/open-questions.md`. See memory
`fuzzing-infrastructure.md`, `audit-2026-07-02-fixes.md`, `m8-unsafe-raw-started.md`.

## Setup on the new machine

```bash
git clone https://github.com/sanohiro/align            # ideally into /home/<user>/project/align
cd align
# Toolchain: Rust 1.96 + LLVM 19 (inkwell llvm19-1). Debian: apt install llvm-19 llvm-19-dev
# .cargo/config.toml already sets LLVM_SYS_191_PREFER_DYNAMIC=1 (Debian llvm-19 is shared-only).
cargo build && cargo test       # expect all green (~994 tests)
```

The compiler is `./target/debug/alignc` (or `./target/release/alignc` after `--release`) — not on
`PATH`. `./target/debug/alignc run examples/min.align` compiles `.align` → native. Subcommands:
`check` / `emit-mir` / `emit-llvm` / `emit-obj` / `build` / `run`. (Or just drive it via `cargo run
-p align_driver -- run <file>`.)

## Where we are (as of main @ commit for PR #183)

The **language core is essentially complete**: types/struct/sum-type/tuple, if/match, Option/Result/
`?`, ownership (value/move/arena/box), strings/template/JSON, the data-oriented array/slice pipeline
(map/where/reduce/sum/scan/sort/partition/chunks), lambdas/closures, task_group/par_map, generics,
numeric casts, multi-file modules, named constants, bitwise/shift, LLVM -O2 (real SIMD). All run
end-to-end to native.

**M6 data-oriented perf is well underway and validated** (see `bench/`): `soa<T>` column scan beats
Rust ~8–10×; `group_by(.key).sum/min/max/.count()` beats the default `std::HashMap` 1.4–4.2×;
`par_map` uses a persistent worker pool; flat pipelines match idiomatic Rust (shared LLVM).

**Perf profiling snapshot (2026-06-29):** benchmark harnesses now support
`ALIGN_BENCH_PROFILE=1 .../run.sh native` decomposition output. The important measured bottlenecks:
JSON decode is parser/decoder-bound (`bench/json_decode`: 1M full decode-only ≈91 ms vs
decode+aggregate ≈92 ms); JSON→SoA **now beats serde** at 1M after the direct-decode work below
(`bench/json_soa`: ≈1.03× of serde); `group_by_reuse` now has a fused one-pass `a3` (below) that beats
the naive 4× group_by 3.2–3.7× but still trails smart single-pass Rust; `string_builder` is
call-count/itoa-bound, not capacity-bound (the `literal + int + literal` batch lowering below now
removes two of three per-row calls); cheap `par_map` loses to Align's own sequential/vectorized
`map().sum()` because every element crosses an indirect thunk. See `bench/README.md` and the
per-benchmark READMEs before changing perf code.

**Direct SoA JSON decode DONE (2026-06-29):** `json.decode → soa<Struct>` parses straight into
arena-allocated columns — no AoS intermediate, no transpose. New runtime `align_rt_json_decode_soa`
(count rows → arena-allocate columns via the `soa_column_offset` layout → fill in one value-parse
pass, sharing the AoS Mison speculation through a generic `FieldDst`); new `Rvalue::JsonDecodeSoa`;
`lower_json_decode_soa` rewritten (no more `transpose_to_soa` for json — `.to_soa()` still uses it).
At 1M rows the SoA path went ≈0.82× → **≈1.03× of serde_json** (~104 → ~83.5 ms), even edging the
AoS decode-only path (which still heap-materializes). See `bench/json_soa/README.md`.

**Fused multi-aggregate `group_by` DONE — first cut (2026-06-29):** `xs.group_by(.name).agg(sum(.a),
max(.b), count(), …)` over an AoS str key computes all K aggregates in **one pass** (intern key once,
fold K accumulators — the `HashMap<&str,[i64;K]>` shape), instead of one group_by per aggregate. New
surface (`.agg(...)`, sema `check_group_agg_multi` → `hir::ArrayGroupAggMulti`), MIR
`Rvalue::GroupAggMultiStr`, runtime `align_rt_group_multi_str` (with a fast FxHash-class hasher, not
SipHash). Bench `a3` beats naive `a1` 3.2–3.7× and beats `a2` (dict_encode reuse); still loses to smart
Rust 1.3–2.4×. **Measured (corrects an earlier guess):** right-sizing the output buffers is a *no-op*
(over-allocation is lazily paged); the real lever is the **hasher** (`ahash` moved `smart/a3` 0.77×→0.92×
at 632k), but it's a new runtime dependency; and the bench's smart baseline reads pre-extracted columns
(a3 reads AoS strided). Deferred: i64-key soa / `dict_encoded` sources. See
`bench/group_by_reuse/README.md` + `docs/open-questions.md`.

**Builder batch lowering DONE (2026-06-29):** the compiler lowers `b.write("lit"); b.write_int(x);
b.write("lit")` in a builder-reduce body to one `align_rt_builder_write_str_int_str` call — a MIR
peephole (`fuse_builder_writes` in `align_mir`), narrow to exactly the `str,int,str` shape on one
builder. Same-host before/after at 100k rows: generated `build` ~1.65 → ~1.30 ms (≈21%), within ~0.19
ms of the direct batch probe and now beating Rust `naive`. A general builder-chain batcher (other
shapes) is the recorded follow-up. See `bench/string_builder/README.md`.

**Active feature: nested struct fields** (`docs/impl/08-nested-structs.md`), the last big language gap:
- **Slice 1 DONE** (PR #182): plain-data (scalar-only, acyclic) nested struct fields — `Line { a: Point }`,
  depth-N read/write (`l.a.x`), nested-literal construction.
- **Slice 2 DONE** (PR #183): whole-struct value semantics (read `p := l.a`, struct-by-value
  params/returns, struct-to-struct assign) — was already working once Slice 1 generalized
  Field/Load/Store; locked in by `tests/struct_by_value.rs`.
- **Slice 3 DONE** (this branch): owned (`string`-bearing) struct fields → the struct becomes a
  **Move** type with a recursive **Drop**; whole-struct move (return/pass/assign) nulls the source.
  Closed the Move-vs-Copy soundness seams (array-of / Option-Result-enum-payload-of a Move struct
  rejected). `tests/owned_structs.rs`. Deferred: owned-field read-out (`u.name.len()`), `array<T>`
  fields, reassign-drops-old (a pre-existing gap for all owned types).

## Next action

**Recently DONE (perf):** builder batch lowering (`fuse_builder_writes`), direct SoA JSON decode
(`align_rt_json_decode_soa`), **and** the fused multi-aggregate `group_by(.key).agg(...)`
(`align_rt_group_multi_str`) — all in the snapshot above, all with new tests, `cargo test` green.

**Best next action: the remaining perf follow-ups**, in measured priority order: a **cross-cutting
"beat smart Rust" pass** (deferred on purpose — we trail smart in several benches, best decided once):
the hash strategy (`ahash` dep vs hand-rolled AES, applied across **all** str group paths incl.
`dict_encode`), an inline-value accumulator layout, and possibly a fair AoS-reading smart baseline — the
right-size-the-output-buffers idea was probed and is a **no-op** (lazy paging), so don't re-try it in
isolation. Also extend the fused `.agg(...)` to i64-key soa / `dict_encoded` sources. Then: cheap
`par_map` sequential fallback or thunk specialization; a SIMD/structural JSON parser (decode is still
value-parse-bound, the lever for both `json_decode` and `json_soa`). Smaller recorded follow-ups: a
general builder-chain batcher; fold
the SoA decode's count pass into the structural-index build. Re-run any perf change with:

```bash
ALIGN_BENCH_PROFILE=1 bench/json_soa/run.sh native
cargo test -q
```

Continue `docs/impl/08-nested-structs.md`:
- **Slice 4** — arrays/soa × nesting (`arr[i].a.x`, nested soa column) **and arrays of Move structs**
  (`[User{…}]` — needs per-element drop; Slice 3 rejects it for now). Risk: medium–high.
- **Slice 5 DONE** — cross-module field types (`f: geom.Point`): an imported `pub` type may be a
  struct field / enum payload / template member. `tests/cross_module_types.rs`.
- **Partial owned-field move DONE** — `n := u.name` (depth-1 `string` field) moves the buffer out,
  nulls the struct field, struct Drop frees null. Deeper paths / Move-struct fields still deferred.
- **Slice 4 `arr[i].a.x` read DONE** — nested field of a struct-array element (`ElemField.field` →
  `path`; first field loaded via the single-field seam, remainder projected from a temp slot — the
  pipeline seam untouched). Deferred: nested element *write* (`arr[i].a.x = v`), nested soa column,
  and **arrays of Move structs** (`[User{name}]`, per-element drop). `tests/struct_index.rs`.
- Smaller follow-up unblocked by Slice 3: owned `array<T>` struct fields.
- **DONE (this branch): borrowing an owned field out** — `u.name.len()` / `str` arg / `s: str :=
  u.name` now read a `string` field as a zero-copy `str` view (non-consuming, `Frame`-regioned so it
  can't escape the struct). Moving the field out stays deferred. `tests/owned_structs.rs`.
- **DONE (this branch): reassign-drops-old** — `mut s := …; s = …` no longer leaks the old buffer
  (all owned types). Sema's `MoveCheck` sets `Stmt::Assign::drop_old` (a `Cell<bool>`) iff the RHS
  doesn't move the old value out; MIR drops the slot before the store. No double-free (`s = f(s)`
  emits no drop). Still deferred: owned **field**/**element** reassign (`u.name = …`, `a[i] = …`).
  `crates/align_driver/tests/reassign_drop.rs`.

Or pause: this is a natural milestone (language core + S1/S2/S3 done, M6 perf validated).

## This session's PRs (#174–#183)

Gap A leak fix (#174); match-on-owned-payload double-free fix (#175); Gemini bench Part 3 record
(#176); builder itoa Gap D + string_builder bench (#177); `builder(capacity)` Gap C — measured *not*
the lever (#178); par_map persistent worker pool (#179); group_by table-interleave negative result
(#180); group_by min/max/count (#181); nested struct fields Slice 1 (#182); struct-by-value Slice 2
(#183).

## Process rules (do not skip — see `CLAUDE.md` + memory)

- **MANDATORY: reflect the `gemini-code-assist` PR review before merging any code PR** (until its
  2026-07-17 sunset). Open PR → poll until the review lands → scrutinize each finding (verify against
  code, don't blind-apply) → reflect valid ones / reject invalid with reason → merge. This lapsed
  once and the user called it out; do not repeat.
- **Benchmark-driven**: measure before claiming a win; if a change doesn't help (e.g. the group_by
  interleave, `builder(capacity)`), don't ship it — record the finding.
- **Ideal form, or defer**: ship only the ideal/unified form; defer rather than compromise.
- **English only** in the repo; **no backward-compat shims** (pre-release — change outright).

## Memory (does NOT travel with `git clone`)

Claude's cross-session memory lives at `~/.claude/projects/-home-hiro-project-align/memory/` (13
files: PR-review workflow, perf model, benchmark findings, language-completion status, etc.). The
repo is self-sufficient without it, but to carry it over:

```bash
# old machine (note the leading ./ — the dir name starts with '-', which tar would else read as flags):
tar czf align-memory.tgz -C ~/.claude/projects ./-home-hiro-project-align
# new machine:
tar xzf align-memory.tgz -C ~/.claude/projects
```
The project key (`-home-hiro-project-align`) is derived from the clone path. Clone to the **same**
path (`/home/<user>/project/align`) so it matches. If the new machine's user/path differs, the key
changes (e.g. `-home-bob-project-align`) — rename the extracted folder to that new key, or Claude
Code won't pick the memory up.
