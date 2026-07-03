This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.process — implementation design (M11)

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
**detach** (do not block; leave the OS to reap via a SIGCHLD-ignore disposition set at startup, or
double-fork-style reaping — v1: set SIGCHLD to SA_NOCLDWAIT at runtime init so un-waited children
don't become zombies). Explicit `wait()` is encouraged; Drop-without-wait is safe (no zombie) but
loses the exit code.

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

`child` Move type + runtime fork/execvp/waitpid/kill wrappers; SIGCHLD SA_NOCLDWAIT init (so
Drop-detach doesn't zombie); **the exit-runs-cleanup path** — `process.exit` must hook the same
unwind/cleanup emission that a top-level return uses (emit_exit_cleanup for all open arenas +
drop_locals + writer flush), then call `exit()`. This is the one non-trivial codegen piece: exit
is not a plain runtime call, it must run the function's (and ideally the stack's) pending cleanup
first. v1 pragmatic scope: run the CURRENT function's cleanup + a registered atexit-style flush of
std handles, then exit — full multi-frame unwind is documented as the ideal, v1 runs
current-frame + global flush. (Record the gap honestly.)

## Slice breakdown

1. `process.exit`/`abort` + the cleanup-then-exit path (the settled semantics) + global std-handle
   flush registration.
2. `child` Move type + `spawn` + `wait` + Drop-detach + SIGCHLD init.
3. `kill` + `exec`.

## Pitfalls

- **P1 (exit skips cleanup = the hazard)**: the WHOLE point is exit runs cleanup. A naive
  `process.exit` = libc `exit()` would silently drop buffered writer output and skip arena frees —
  exactly the bug. Must emit cleanup first. Highest-value correctness point.
- **P2 (zombie children)**: Drop-without-wait must not zombie. SA_NOCLDWAIT at init, or reap.
  Test: spawn 100, drop all without wait, assert no zombies (ps/proc).
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
