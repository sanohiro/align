# Session handoff (continue on another machine)

A living continuity note so a fresh Claude Code session — e.g. on a faster machine — can pick the
work up immediately. **If you are a new session: read this, then `CLAUDE.md`, then
`docs/impl/08-nested-structs.md`.** Everything durable is in this repo; the conversation history and
Claude's per-machine memory do not travel with `git clone` (see "Memory" below).

_Last updated: 2026-07-02 (main @ PR #290)._

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
