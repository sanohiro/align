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

## Slice 1 — SHIPPED (2026-07-06, branch `feat/m11-process-slice1-exit`)

`process.exit(code)` / `process.abort()` are built end-to-end (sema → HIR `ProcessExit`/`ProcessAbort`
→ MIR → runtime `align_rt_process_exit`/`align_rt_process_abort`):

- **exit = cleanup-then-exit.** MIR lowering runs the current function's `emit_exit_cleanup` (the same
  helper a `return` uses — drops for live owned locals, `task_group`/arena ends) *before* the runtime
  call, then terminates the block `Unreachable`. So a buffered writer flushes + closes in its `Drop`
  and an arena is freed before the process dies. The runtime side is just `std::process::exit(code)`.
- **abort = named escape hatch.** A bare `align_rt_process_abort()` with **no** preceding cleanup, i.e.
  libc `_exit(1)` — no Drops, no flushes, no `atexit`. Distinct from the compiler's `panic_abort`
  (`SIGABRT`, reserved for arithmetic-trap / invariant violations); `abort()` is a user-requested
  signal-free immediate exit, as specified below (`_exit`, not `abort`). Exit status `1` (abort takes
  no code; a deliberate abnormal exit is a failure).
- **"Global flush" turned out to require nothing.** The runtime owns no process-wide output buffer:
  `print` flushes `stdout` on every call (generated `main` returns straight to crt0, so it can't rely
  on an `atexit` hook), and every `writer` / buffered sink is an Align **Move** value flushed by its
  `Drop` in the caller's cleanup. So there is no atexit-style registration to build — recorded here as
  unneeded-today. If a runtime-owned global buffer is ever introduced, `align_rt_process_exit` is where
  its flush would hook.
- **Exit-code truncation.** `i64 -> i32`, observed as the low 8 bits on a Unix `wait`
  (`WEXITSTATUS`): `exit(256)` → `0`, `exit(-1)` → `255`. Documented, matches `exit(3)`.
- **Divergence typing (v1 limitation).** There is no `Never` type, so `exit`/`abort` are typed `()`.
  They diverge in MIR (cleanup + call + `Unreachable`), and code after them is dead — not emitted
  (`lower_block` stops at `is_terminated`), parity with code-after-`return`, no ICE. But because the
  type system does not model the divergence, `process.exit` cannot be the **tail value** of a
  non-unit-returning function — use it as a statement (e.g. `process.exit(3)` then a trailing `0`).
  A proper diverging/`Never` type is the ideal, deferred.
- **v1 multi-frame gap (recorded honestly).** Only the CURRENT function's cleanup runs. A full
  multi-frame stack unwind — running *every* caller's Drops on the way out — is the documented ideal,
  deferred. For a program whose owned resources all live in the frame that calls `exit` (or in an
  arena / buffered writer bound there), current-frame cleanup already covers everything expressible;
  the gap bites only when a caller up the stack owns a resource whose `Drop` has an observable effect.

Slice 2 (`child` / `spawn` / `wait`) SHIPPED (2026-07-06, `feat/m11-process-slice2-*`, PR #377).

## Slice 3 — SHIPPED (2026-07-06, branch `feat/m11-process-slice3-kill-exec`)

`ch.kill(sig)` / `process.exec(cmd, args)` are built end-to-end (sema → HIR `ChildKill`/`ProcessExec`
→ MIR → runtime `align_rt_child_kill`/`align_rt_process_exec`):

- **`ch.kill(sig: i64) -> Result<(), Error>`** — libc `kill(pid, sig)`. Borrows the child (like
  `wait`, non-consuming; bound-receiver gated) and guards the `reaped` flag *before* signalling: killing
  an already-reaped child is a clean `Err` (`AL_INVALID`), never a stray signal to a possibly-recycled
  pid. **`sig == 0` is ALLOWED** — the standard POSIX liveness/permission probe (no signal sent, just an
  existence check); a negative or out-of-range `sig` (`> 64`, the Linux `SIGRTMAX`) is `Error.Invalid`
  *before* the syscall (so the `i64 → i32` narrow is always sound). `EPERM`/`ESRCH` surface via the
  shared errno table. A signal-killed child then `wait()`s as `128 + sig`.
- **`process.exec(cmd, args) -> Result<(), Error>`** — `execvp(cmd, argv)` **in the current process**
  (no `fork`). `args` is the new image's FULL argv incl. `argv[0]` (P5 — same convention as `spawn`;
  `cmd` is the independent lookup path). **On success it REPLACES the process image and NEVER RETURNS**,
  so the `Result` is only ever observed as its `Err` arm (a mapped `execvp` errno; `AL_INVALID` for a
  bad `cmd`/`argv`). **⚠️ NO CLEANUP RUNS on the success path — this is loud and deliberate:** `execvp`
  discards the entire address space, so pending `Drop`s / arena ends / **buffered-writer flushes DO NOT
  RUN** (buffered bytes still sitting in user space are LOST — flush before `exec` if they matter). This
  is inherent to `execvp` and makes `exec` **abort-class** in cleanup terms — the mirror image of
  `process.exit` (which runs cleanup first) and closer to `process.abort` (no cleanup). Unlike
  `process.exit`/`abort`, `exec` does NOT diverge in the type system (it returns `Result` on failure);
  the MIR is a plain fallible builtin call whose success path simply never returns from the runtime, so
  no cleanup is emitted (nor could it run). **CLOEXEC interaction:** Align-owned fds (readers / writers /
  sockets / children) are `CLOEXEC` (Slice 2's P3 sweep), so the exec'd image does NOT inherit them;
  only the inherited standard streams (fds 0/1/2, not `CLOEXEC`) survive — the normal contract.
- **Marshalling shared with `spawn`.** `cmd` + argv → C strings (interior-NUL / empty-argv / non-UTF-8
  rejection) is a single runtime helper `marshal_cmd_argv`, used by both `spawn` (in the parent, pre-
  `fork`) and `exec` (in the process about to be replaced). No duplication. The three argv source forms
  (`array<str>` / `slice<str>` / fixed-array-literal via `ArrayToSlice`) share one sema helper too.

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
