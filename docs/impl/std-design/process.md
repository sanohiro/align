This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.process — implementation design (M11)

> 🌐 **English** · [Japanese](./ja/process.md)

## Overview

spawn, exec, exit (draft §18.2). Fork/exec/waitpid + a child Move handle. **This module settles
the `process.exit` Drop-semantics Open question** (open-questions).

## Signatures

```text
ch := process.spawn(cmd: str, args: array<str>) -> Result<child, Error>   // fork+exec, child owns pid
ch.wait() -> Result<i64, Error>       // reap, return exit code (consumes the child's reapable state)
ch.kill(sig: i64) -> Result<(), Error>
process.exec(cmd: str, args: array<str>) -> Result<(), Error>   // replace current image (execvp; returns only on error)
process.exit(code: i64)               // run cleanup, then exit — see below
process.abort()                        // immediate _exit, NO cleanup
```

## Type & ownership classification

`child` is a **Move type** owning a pid. Drop = if already waited → no-op; if not waited →
**reap it** with a blocking `waitpid` (discarding the exit code) so it cannot become a zombie.
Explicit `wait()` is encouraged and returns the exit code; Drop-without-wait is safe (no zombie)
but loses the code and may block until the child exits.

**Why not `SA_NOCLDWAIT`** (a rejected alternative): setting `SA_NOCLDWAIT` globally on `SIGCHLD`
at init would auto-reap zombies, but under POSIX it makes a subsequent `waitpid` for a specific
child fail with `ECHILD` — which directly breaks `ch.wait() -> Result<i64, Error>` (explicit wait
could no longer retrieve the exit status). So v1 keeps the default `SIGCHLD` disposition and reaps
per-child in Drop instead. If a caller wants to drop a long-lived child *without* blocking, it
should `kill()` first (or use a future explicit `detach()` API — recorded, not in v1).

## `process.exit` Drop-semantics decision (SETTLED here)

`process.exit(code)` runs like a normal return to the top — it **unwinds and runs all pending
Drops / arena ends / buffered-writer flushes**, THEN calls libc `exit(code)`. This honors
Nothing-hidden (no silently-lost buffered output — the exact hazard the io.md buffered-writer
restriction warns about). The immediate hard-exit that skips all cleanup is a SEPARATE explicit
API, `process.abort()` (→ `_exit`), for when the program must die now. Rationale: the default must
be the safe one (cleanup runs); the dangerous one must be named. (Resolves the open-questions
"process.exit Drop semantics" item — run-Drops-then-exit as default, `abort()` as the escape
hatch.)

## Effect classification

All impure.

## Error policy

fork/exec/wait failures → errno→Error table (M9). `exec` returning at all = it failed (errno).
`exit`/`abort` don't return.

## New machinery required

`child` Move type + runtime fork/execvp/waitpid/kill wrappers; **child Drop reaps via blocking
`waitpid`** (no `SA_NOCLDWAIT` — it would break explicit `wait()` with `ECHILD`); **the
exit-runs-cleanup path** — `process.exit` must hook the same
unwind/cleanup emission that a top-level return uses (emit_exit_cleanup for all open arenas +
drop_locals + writer flush), then call `exit()`. This is the one non-trivial codegen piece: exit
is not a plain runtime call, it must run the function's (and ideally the stack's) pending cleanup
first. v1 pragmatic scope: run the CURRENT function's cleanup + a registered atexit-style flush of
std handles, then exit — full multi-frame unwind is documented as the ideal, v1 runs
current-frame + global flush. (Record the gap honestly.)

## Slice breakdown

1. `process.exit`/`abort` + the cleanup-then-exit path (the settled semantics) + global std-handle
   flush registration.
2. `child` Move type + `spawn` + `wait` + Drop-reaps-via-waitpid (no `SA_NOCLDWAIT`).
3. `kill` + `exec`.

## Pitfalls

- **P1 (exit skips cleanup = the hazard)**: the WHOLE point is exit runs cleanup. A naive
  `process.exit` = libc `exit()` would silently drop buffered writer output and skip arena frees —
  exactly the bug. Must emit cleanup first. Highest-value correctness point.
- **P2 (zombie children)**: Drop-without-wait must not zombie — reap per-child with a blocking
  `waitpid` in Drop. Do NOT use a global `SA_NOCLDWAIT`: it auto-reaps but makes explicit
  `ch.wait()` fail with `ECHILD`, breaking the exit-code contract. The tradeoff is that dropping a
  still-running child blocks until it exits (documented; `kill()` first to avoid). Test: spawn 100
  short-lived, drop all without wait, assert no zombies (ps/proc) and that a separate explicit
  `wait()` still returns a code.
- **P3 (fork+exec fd leak)**: child inherits fds; set CLOEXEC on Align-owned fds
  (readers/writers/sockets) so they don't leak into the child. Or document the inheritance. v1:
  CLOEXEC on all Align fd-owning handles.
- **P4 (child Move sweep + bound-receiver)**: Gate-1 sweep; unbound-temp receiver rejected.
- **P5 (exec argv[0])**: execvp convention — args includes argv[0] or the runtime supplies cmd as
  argv[0]. Pick one (v1: caller's args is the full argv incl. [0]; cmd is the lookup path),
  document it.

## Test checklist

- spawn `true`/`false` → wait returns 0/1
- spawn + drop without wait → no zombie (P2)
- exec replaces image (child prints, parent never continues past exec-on-success)
- `process.exit(3)` after a buffered stdout write → the write IS flushed (P1 — the critical test)
  + exit code 3
- `process.abort()` → exit without flush
- kill sends signal
- child as array element rejected
- CLOEXEC prevents fd leak into child (P3)
- import-required
