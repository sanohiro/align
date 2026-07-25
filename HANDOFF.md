# Session handoff

Current continuity note for a fresh Claude Code or Codex session. Keep this file
about the present state, the next decision, and operational facts. The former
per-PR journal is preserved in
[`docs/archive/HANDOFF-2026-07-25.md`](docs/archive/HANDOFF-2026-07-25.md).

_Last updated: 2026-07-25. `main` includes the shipped wave through #640.
Removing obsolete JSON schema summaries from MIR is the current work._

## Start here

1. Read `CLAUDE.md` for repository rules, sources of truth, and the required
   review flow. `AGENTS.md` is the Codex compatibility link to the same file.
2. Read the design or audit directly governing the requested work.
3. Use the archive only when historical implementation detail is material.

Do not rely on Claude's per-machine memory or a previous conversation. Durable
facts must live in this repository.

## Current baseline

- **Release:** v0.4.0 was tagged at `88ee798` and published for the completed
  align-llm R1/R2/R3 batch. `RELEASE_NOTES_0.4.0.md` is the release record.
- **Post-release portability:** v0.4.0's macOS artifact failed to link because
  `clearenv(3)` is unavailable on macOS/BSD. #636 fixed this on `main` with a
  platform-specific portable implementation; macOS CI is green and Linux
  behavior is unchanged. The v0.4.0 tag predates the fix, so the next versioned
  release must be cut from current `main`.
- **Compiler roadmap:** M0-M15, the LLVM 19-to-22 checkpoint, separate
  compilation, the default-on per-unit object cache, parallel codegen,
  ThinLTO, and instrumented PGO are complete. The roadmap retains the
  implementation evidence; it is not the live backlog.
- **pkg.web:** F0-F3 and W1-W7 are complete. The current contract is
  `docs/impl/pkg-design/web.md`; `docs/impl/15-pkg-web-plan.md` is the completed
  execution record. The framework is general-purpose REST infrastructure, not
  an LLM-gateway-specific subset.
- **align-llm requests:** all filed requests are complete and answered in
  `../align-llm/docs/align-requests.md`.

## Latest shipped wave

```text
#630  std.process captured output + cwd
#631  process timeout + Error.Timeout
#632  process env / env_clear
#633  std.net connect/read/write timeout substrate
#634  std.http client/request I/O timeouts, including TLS and pooled reuse
#635  core.json array<str> struct-field decode
#636  portable env_clear for macOS/BSD
#637  unified Claude/Codex guidance + compact handoff
#638  Copy-struct array materialization
#639  Unit-call values + aggregate call-ownership hardening
#640  cold/cache build-result parity via complete structural MIR identity
```

#639 fixes Unit-call values across direct, indirect, pipeline, and per-unit
lowering. Its final review also hardened call ownership: temporary aggregate
arguments keep per-member cleanup until the call is reached, and arena-owned
Move values cannot transfer to a callee. Bound aggregates require one uniform
owned allocation mode so their single path-local cleanup bit remains exact.
The same transfer rule covers `Result.map_err`; fused pipeline functions reject
Move source and result elements until explicit per-iteration cleanup exists.
The review follow-up carries `map_err` and one-owner struct runtime provenance
through their result slots, guards partial direct struct and fixed-array initialization, and
rejects mutation when aggregate ownership is path-dependent. `map_err` also
retains its receiver during mapper evaluation and tracks mapper-capture borrows.
Move values leaving a `task_group` forward the tail local's cleanup bit and
clear that inner source before return or call transfer.
Early exits join open task groups before dropping captured frame or arena storage.
The task-group runtime region is reserved for spawned environments and result slots:
ordinary owned values inside the block retain individual cleanup, and general arena-only
allocation still requires an explicit nested `arena {}`.
Both `reduce` and materializing `scan` require a Copy accumulator until MIR has
explicit per-iteration transfer and error-path cleanup for Move values.

#640 replaced the function-only implementation hash with the complete structural
per-unit MIR program consumed by codegen. Type tables, declarations, linkage,
alignment, and located metadata now participate automatically, so a warm cache
hit cannot skip a cold codegen failure caused by an omitted backend input.

The last recorded full workspace run before #636 was 2748 passed / 0 failed,
with clippy clean. #636 then passed focused Linux runtime/process tests, clippy,
and the macOS release-build CI path. A local `cargo build --release --workspace`
was rerun after #636.

## Next work

Remove the obsolete recursive JSON schema strings from MIR. #640 made the
per-unit implementation hash cover the exact structural MIR program consumed by
codegen, including its type tables, so the copied summaries no longer carry
cache identity. JSON MIR nodes should retain only their target ids. Then select
the next task from an owner request, a real consumer, or the current **Open**
section of `docs/open-questions.md`; do not resurrect a superseded `NEXT` item
from the archived journal.

Consumer-gated deferrals that remain intentional:

- Fully escaping function values wait for a consumer and a settled heap-owned
  environment/drop model.
- `std.process` binary capture (`run_bytes`) waits for a binary-output consumer;
  see `docs/impl/std-design/process.md`.
- Top-level `array<str> := json.decode(...)` waits for a result representation
  that carries the input region. Struct fields of `array<str>` already ship;
  see `docs/impl/core-design/json.md`.
- The first pkg.web consumer application remains a separate, owner-scheduled
  task.

## Build and test notes

On this Apple Silicon machine, use:

```bash
export LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm
export LLVM_CONFIG=/opt/homebrew/opt/llvm/bin/llvm-config
export LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/openssl@3/lib

cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Operational rules:

- After modifying `align_runtime`, run a plain workspace build before driver
  tests or `alignc run`; user programs link the runtime static archive.
- Do not edit runtime sources while a workspace test run is in progress; that
  can produce a stale-archive cascade in driver tests.
- Do not pipe test output through a command that hides the original exit code.
- Use `ALIGNC_CACHE=off` when a test specifically requires a cold build.
- Network, TLS, filesystem, and fd tests may need an unrestricted local
  environment rather than a sandbox.

## Durable records

```text
Language semantics and surface       draft.md
Current decisions and open items     docs/open-questions.md
Milestone implementation evidence    docs/impl/07-roadmap.md
Current pkg.web contract             docs/impl/pkg-design/web.md
Cache architecture and parity resolution    docs/impl/10-cache-first-optimization.md
Closure/memory/I/O/SIMD audit        docs/impl/12-pipeline-closure-memory-io-simd-audit.md
Allocation and short-input audit     docs/impl/13-string-array-allocation-short-input-audit.md
Source-correctness fixes             docs/impl/source-correctness-fixes-2026-07-13.md
Historical session journal           docs/archive/HANDOFF-2026-07-25.md
```

## Maintaining this handoff

- Update the current baseline and next-work sections in place.
- Do not append a full PR narrative. Put durable design facts in the relevant
  spec/audit, and rely on the PR and Git history for implementation chronology.
- When historical context is still worth retaining, add a dated archive rather
  than growing the live handoff indefinitely.
- Keep release and review procedures in `CLAUDE.md`; link to them instead of
  duplicating them here.
