# Session handoff (continue on another machine)

A living continuity note so a fresh Claude Code session — e.g. on a faster machine — can pick the
work up immediately. **If you are a new session: read this, then `CLAUDE.md`, then
`docs/impl/08-nested-structs.md`.** Everything durable is in this repo; the conversation history and
Claude's per-machine memory do not travel with `git clone` (see "Memory" below).

_Last updated: 2026-06-27._

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

**Active feature: nested struct fields** (`docs/impl/08-nested-structs.md`), the last big language gap:
- **Slice 1 DONE** (PR #182): plain-data (scalar-only, acyclic) nested struct fields — `Line { a: Point }`,
  depth-N read/write (`l.a.x`), nested-literal construction.
- **Slice 2 DONE** (PR #183): whole-struct value semantics (read `p := l.a`, struct-by-value
  params/returns, struct-to-struct assign) — was already working once Slice 1 generalized
  Field/Load/Store; locked in by `tests/struct_by_value.rs`.

## Next action

Continue `docs/impl/08-nested-structs.md`:
- **Slice 3 — owned (`str`-bearing) nested fields + struct `Drop`** (highest value, **highest risk**:
  the double-free class fixed in #175; also stops the current `str`-by-value leak). Best done fresh.
- **Slice 4** — arrays/soa × nesting (`arr[i].a.x`, nested soa column).
- **Slice 5** — cross-module field types (`f: other.T`, the module B3 leftover, now unblocked).

Or pause: this is a natural milestone (language core + S1/S2 done, M6 perf validated).

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
