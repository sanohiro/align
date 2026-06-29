# Session handoff (continue on another machine)

A living continuity note so a fresh Claude Code session — e.g. on a faster machine — can pick the
work up immediately. **If you are a new session: read this, then `CLAUDE.md`, then
`docs/impl/08-nested-structs.md`.** Everything durable is in this repo; the conversation history and
Claude's per-machine memory do not travel with `git clone` (see "Memory" below).

_Last updated: 2026-06-29._

## Setup on the new machine

```bash
git clone https://github.com/sanohiro/align            # ideally into /home/<user>/project/align
cd align
# Toolchain: Rust 1.96 + LLVM 19 (inkwell llvm19-1). Debian: apt install llvm-19 llvm-19-dev
# .cargo/config.toml already sets LLVM_SYS_191_PREFER_DYNAMIC=1 (Debian llvm-19 is shared-only).
cargo build && cargo test       # expect all green (~676 tests)
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
decode+aggregate ≈92 ms); JSON→SoA still pays a real AoS→SoA materialization cost at 1M rows
(`bench/json_soa`: AoS decode-only ≈88 ms, SoA decode+transpose ≈102–114 ms); `group_by_reuse` is
dominated by high-cardinality string `dict_encode` (≈260 ms at 1M groups, with four reused
aggregates adding ≈75 ms); `string_builder` is call-count/itoa-bound, not capacity-bound (the
`literal + int + literal` batch lowering below now removes two of three per-row calls); cheap
`par_map` loses to Align's own sequential/vectorized `map().sum()` because every element crosses an
indirect thunk. See `bench/README.md` and the per-benchmark READMEs before changing perf code.

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

**Builder batch lowering is DONE** (see the snapshot above + `bench/string_builder/README.md`): the
`fuse_builder_writes` MIR peephole collapses `b.write("lit"); b.write_int(x); b.write("lit")` to one
`align_rt_builder_write_str_int_str` call (narrow `str,int,str` shape only). `cargo test` green; new
`align_mir` tests `builder_str_int_str_is_fused` / `builder_int_str_str_is_not_fused`.

**Best next action: pick up the secondary perf follow-ups**, in measured priority order: JSON direct
SoA decode / count-then-direct column fill (`bench/json_soa` measured a 10–25 ms AoS→SoA
materialization penalty at 1M); `group_by_reuse` multi-aggregate one-pass fuse plus faster string
`dict_encode`; cheap `par_map` sequential fallback or thunk specialization. A general builder-chain
batcher (append shapes beyond `str,int,str`) is a smaller recorded follow-up. Re-run any perf change
with:

```bash
ALIGN_BENCH_PROFILE=1 bench/string_builder/run.sh native
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
