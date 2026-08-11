# Build performance plan (dedicated improvement track)

This is a dedicated improvement track outside the milestone roadmap
(`docs/impl/07-roadmap.md`). It consumes no mainline milestone. Each item
passes the normal design and implementation gates when work on it starts.

## Principle

Owner-settled 2026-08-12: **Output is always fully optimized. Build speed
comes from reuse and parallelism, never from lowering optimization.** A
reduced-optimization dev profile (`-O0` or equivalent) is a non-goal.

## Track items

Order is priority.

| # | Item | Status |
|---|------|--------|
| 1 | Persistent unit cache v1 | In flight — in-process memoization shipped as #757; ledger lands in `docs/impl/10-cache-first-optimization.md` §6.7 with the v1 PR |
| 2 | mold/lld linking for local builds | Next after v1; output-identical, link-stage only |
| 3 | Pipelined compilation | Start a dependent unit's frontend as soon as the dependency's interface summary exists; same work, shorter wall clock |
| 4 | Prebuilt optimized cache distribution | Ship warmed std/pkg cache entries with releases, once the v1 persistent format is settled |
| 5 | Daemon / watch mode | Keep the in-process memo alive across builds; the main lever for AI-agent edit-compile loops |
| 6 | Function-level incremental compilation | Heaviest; requires its own design ledger before any implementation |

## Background

Measured on `pkg.db`: cold per-unit codegen ~9s versus frontend ~3.4s.
Caching (items 1–4) removes re-paying, parallelism (3) and residency (5)
shorten the wall clock, and granularity (6) shrinks the unit of re-payment.
