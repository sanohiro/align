This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.process — implementation design (M11)

> 🌐 **English** · [Japanese](./ja/process.md)

> **Status:** M11 core complete (exit/abort, spawn/wait/kill, exec shipped). **Extension DESIGNED
> 2026-07-24** (align-llm Request 1): captured output + cwd / env / timeout via a `process.command`
> builder + `run_output` handle — see "Extension" at the end of this file. **Slices 4–6 SHIPPED**
> (`process.command`/`c.cwd`/`c.run` capture; `c.timeout_ns` + the core `Error.Timeout` variant;
> `c.env`/`c.env_clear`). The bounded text/bytes extension is **SHIPPED** for align-llm Request 11.

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
process.cpu_count() -> i64            // parallelism available to THIS process (>= 1); see below
```

## `process.cpu_count()` — SHIPPED 2026-07-21

The parallelism available to **this process**: cores as limited by CPU affinity and cgroup quota,
never the raw machine core count (`std::thread::available_parallelism`). Always `>= 1` — the OS
failing to answer falls back to `1`, so there is no error path and no `Option`. **Impure** (it
observes the machine).

It lives here rather than in `std.env` because it is a property of the running process, not of the
environment block. Its reason to exist is that **it is the number a `task_group` worker count must
be sized against**: the runtime's task pool is sized from exactly this source and runs the group's
tasks on that pool PLUS the calling thread, so a group of never-returning tasks larger than
`cpu_count() + 1` would leave the extra tasks unstarted. `pkg.web.serve`'s `workers` parameter
aborts above that bound, and the recommended sizing (`workers = process.cpu_count()`) is only
writable because this exists.

**Deployment note.** It is quota-aware, which is the point — and it makes any bound derived from it
machine-dependent: a source line naming a fixed worker count can run on a 16-core box and abort in a
4-CPU container. Deriving the count from `cpu_count()` is the portable spelling.

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
- **Divergence typing (v1 limitation).** There is no `Never` type, so `exit`/`abort` retain a `()`
  surface result. They diverge in checked control flow and MIR (cleanup + call + `Unreachable`), and code
  after them is dead — not emitted (`lower_block` stops at `is_terminated`), parity with
  code-after-`return`, no ICE. The am-f return-completeness check therefore accepts either operation
  as a direct completion expression or non-fallthrough statement path completing a non-Unit
  function. This invents no trailing value and is not a general `Never` coercion through eager
  parents; a first-class diverging/`Never` type remains deferred.
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

---

# Extension — captured output + cwd / env / timeout (align-llm Request 1)

> **Status: SHIPPED (Slices 4–6, #630/#631/#632, 2026-07-24).** `process.command` + `cwd` +
> `timeout_ns` + `env`/`env_clear` + `run` capture are built end-to-end. The bytes tier and explicit
> cap now have a concrete consumer and are designed in the Request 11 extension below. Motivated by `align-llm` (a real client)
> whose core loop runs build/test/lint commands and **parses their output**. Source:
> `../align-llm/docs/align-requests.md` Request 1 (priority: critical — it blocked that loop).

## Why this is genuinely new

Slices 1–3 above ship `spawn`/`wait`/`kill`/`exec`, but `spawn` does a bare `fork` + `execvp` with
**no pipes and no `dup2`**: the child inherits the parent's fds and its output goes straight to the
terminal. The `child` handle is `{ pid, reaped }` only. So capturing `stdout`/`stderr` as strings,
running in a chosen working directory, passing a per-child environment, and bounding the run with a
timeout are all **impossible today** — and none of them are in this module's recorded design space
(the only prior deferrals were `detach()` and a `Never` type). This is a real-workload requirement,
not a planned gap.

## Shared prerequisite — the `Error.Timeout` variant (canonical definition; `std.http`/`std.net` reuse it)

> **SHIPPED (Slice 5).** The five-variant `Error` enum landed: sema registers `Timeout` between
> `Denied` and `Code` (`ERROR_VARIANT_CODE` is now `4`, `Code` stays last); the runtime carries
> `AL_TIMEOUT = 4` / `AL_CODE = 5`; and the MIR `make_error_from_status` branchless decode is
> `tag = min(status-1, 4)` / `Code = status-5`. The generic errno→`Error` mapping is unchanged — a
> timeout is surfaced ONLY by returning `AL_TIMEOUT` explicitly at a timeout site (an unrelated
> `ETIMEDOUT`/`EAGAIN` errno still maps to `Error.Code`). `Error.Timeout` is user-visible: it names a
> `match` arm alongside `NotFound`/`Invalid`/`Denied`/`Code(c)`.

A timeout must be **distinguishable** from a nonzero exit and from a transport error (align-llm must
tell "the test hung" apart from "the test failed"). The builtin `Error` enum has four variants
(`NotFound`, `Invalid`, `Denied`, `Code(i32)`) and no timeout. This extension adds a fifth,
**`Error.Timeout`** (no payload), shared with the `std.http`/`std.net` I/O-timeout work (http.md /
net.md G3-1). It is defined here because Request 1 lands first.

The five-variant enum and its branchless status↔variant mapping (the one non-mechanical part):

```text
variant order (must match ERROR_VARIANT_CODE):  NotFound, Invalid, Denied, Timeout, Code(i32)
AL_ status sentinels (align_runtime):            AL_NOT_FOUND=1  AL_INVALID=2  AL_DENIED=3
                                                 AL_TIMEOUT=4  (NEW)   AL_CODE=5  (was 4)
MIR status→Error decode (make_error_from_status): tag = min(status-1, 4);  Code payload = status-5
```

Touch points (each already handles four variants — extend, do not restructure):
- `crates/align_sema/src/lib.rs` ~`:2795` — add the `Timeout` variant (payload `Vec::new()`,
  `field_base: 1`) **between `Denied` and `Code`** so `Code` stays last.
- `crates/align_runtime/src/lib.rs` ~`:6803` — insert `AL_TIMEOUT = 4`, bump `AL_CODE` 4→5, and
  update the `AL_CODE.saturating_add(errno)` base in `io_error_to_status`. Generic errno mapping is
  unchanged; a timeout is surfaced by returning `AL_TIMEOUT` **explicitly at the timeout sites**, not
  by classifying an errno (an `EAGAIN`/`ETIMEDOUT` from an unrelated site still means `Code`).
- `crates/align_mir/src/lib.rs` ~`:8117` `make_error_from_status` — change the branchless clamp from
  `min(status-1, 3)` / `Code(status-4)` to `min(status-1, 4)` / `Code(status-5)`.

This is a language-core change (the `Error` enum is core, not std). It is intentionally small and
one-directional (a new variant is added; nothing is renamed or removed).

## Surface

Optional per-run configuration (cwd / env / timeout) cannot be a trailing `opts?` argument — **Align
has no optional / default / named arguments**. The one existing idiom for optional configuration is
the `std.http` request builder (`r := http.request(...)`; `r.header(...)`; `r.body(...)` — each an
in-place mutation of a bound-local Move handle that returns `()`, *not* a chained fluent call). This
extension follows that idiom exactly, so it is the same "one way," not a second mechanism:

```text
c := process.command(cmd: str, args: array<str>) -> command   // Move handle (opaque, Ty::Command)
c.cwd(dir: str)                    // set working directory       -> ()   (in-place, bound-local)
c.env(name: str, value: str)       // add/override one variable    -> ()
c.env_clear()                      // start the child env empty    -> ()
c.timeout_ns(ns: i64)              // kill + Err(Timeout) past ns   -> ()
out := c.run() -> Result<run_output, Error>   // fork+capture; borrows c (re-runnable)

// run_output — an opaque Move handle (Ty::RunOutput), NOT a by-value struct (see below).
out.code() -> i64        // exit code (decode_wait_status: WEXITSTATUS, or 128+signal)
out.stdout() -> str      // captured stdout, zero-copy view into out (region-bound to out)
out.stderr() -> str      // captured stderr, zero-copy view into out (region-bound to out)
```

Usage (align-llm's verify loop):

```align
c := process.command("git", ["git", "status", "--porcelain"])
c.cwd(repo_dir)
c.timeout_ns(30_000_000_000)          // 30 s
out := c.run()?                        // Err(Timeout) if it overruns
if out.code() == 0 {
  parse_clean(out.stdout())
} else {
  report(out.stderr())
}
```

### Why `run_output` is a handle, not `{ code, stdout, stderr }`

The request sketched `output = { code, stdout, stderr }`. A by-value builtin struct owning **two**
heap strings is exactly the "first-class builtin-struct return" that net.md (`datagram { n, peer }`)
and http.md (`response_builder` deep-owns to sidestep it) both record as **deferred** — a `Result`
`Ok` payload is a single `Scalar`, and there is no machinery to return a value aggregating multiple
owned allocations. The realized Align idiom for "a returned thing that owns heap" is a single opaque
`*mut Handle` owning its allocations internally, read through accessors — which is precisely how
`http.response` works (`resp.status()` / `resp.header()` / `resp.body()`). So `run_output` mirrors
`response`: one Move handle, `.code()` / `.stdout()` / `.stderr()` accessors, the string accessors
returning region-bound zero-copy views. This is the **ideal form within Align's current coherent
design**, not a compromise; the by-value-struct spelling would require building the separate,
larger deferred feature first (and would then be a second way to do the same thing).

## Type & ownership classification

- `command` — **Move** type (`Ty::Command`), owns cmd + full argv + optional cwd + an env override
  list + a timeout. Modeled on `Ty::HttpRequest` (a builder Move handle with owned internals). Drop =
  free. Config methods borrow it (bound-receiver, in-place), `run()` borrows it (re-runnable).
- `run_output` — **Move** type (`Ty::RunOutput`), owns the exit code + two owned byte buffers.
  Modeled on `Ty::HttpResponse`. Rides `Result<run_output, Error>`'s `Ok` position
  (`Scalar::RunOutput`). `.stdout()`/`.stderr()` views are `region_of(out)` (P3-style escape gate:
  a view must not escape past `out`'s Drop). Drop = free both buffers.
- Both are rejected as aggregate elements except their own-constructor `Result` Ok slot — the
  standard Move-handle restriction. Both need the **full new-Ty sweep** (see New machinery).

## Runtime design (`align_rt_command_*` + `align_rt_command_run`)

This subsection records the **currently shipped Slices 4–7**. The Request 11 ledger below owns the
complete bounded parent capture/reap lifecycle.

Builder handle `Command { argv: Vec<CString>, cwd: Option<CString>, env: Vec<(CString,CString)>,
env_clear: bool, timeout_ns: i64, max_capture_bytes: Option<i64> }` built by
`align_rt_command_new(cmd, args)` (reuse
`marshal_cmd_argv` for the argv, same interior-NUL / empty-argv / non-UTF-8 rejection). `cwd` / `env`
/ `env_clear` / `timeout_ns` are thin setters (env pairs marshalled via the `*const AlignStr, len`
slice ABI, one pair per `env` call). `run_output` handle `RunOutput { code: i64, out: Vec<u8>, err:
Vec<u8> }`.

`align_rt_command_run(c, out: *mut *mut RunOutput) -> i32`:

1. Validate the optional bound and allocate its two exact capture layouts plus the output shell.
   Make both `O_CLOEXEC` pipes and both nonblocking read ends before fork.
2. `fork`. **Child** (async-signal-safety caveats identical to `spawn` — `execvp`/`chdir`/`setenv` in
   a forked threaded parent are the documented existing hazard, `posix_spawn` is the recorded ideal
   fix): `chdir(cwd)` if set (fail → `_exit(127)`); if `env_clear` then `clearenv()`, then `setenv`
   each override; `dup2` the two pipe write-ends onto fds 1 and 2; close all pipe fds; `execvp`; on
   failure `_exit(127)`.
3. **Parent**: close the write ends, **`poll` BOTH read fds together**, and drain stdout then stderr
   into the selected stores as data arrives. Draining both concurrently is mandatory, or
   a child that fills the stderr pipe while the parent reads stdout **deadlocks** (the classic
   two-pipe capture bug). Loop until both hit EOF.
4. The monotonic deadline covers pipe drain through direct-child reap. Timed EOF/live-child state
   uses `waitpid(WNOHANG)` plus zero-fd `poll`; untimed EOF may block in `waitpid`. Every timeout,
   overflow, or hard capture/wait error preserves its first status, signals the owned group when
   present and the direct pid while it remains waitable, closes both reads, and reaps the direct
   child. A wait that already consumed the child never signals its potentially recycled pid.
5. Success requires both EOF and direct-child reap, then sets `out.code = decode_wait_status(status)`.
6. **UTF-8**: validate `out.out` / `out.err` as UTF-8. Invalid → free and return `AL_INVALID` (the
   `fs.read_file` precedent — a `string`-typed accessor cannot expose non-UTF-8 bytes). See below.
7. Transfer the two stores into the preallocated `RunOutput` or `RunBytes` shell, publish it, and
   return `0`.

`.stdout()`/`.stderr()` return `AlignStr { ptr: out.out.as_ptr(), len }` — a borrowed view, exactly
like `align_rt_http_resp_body`.

## UTF-8 policy (decision + the bounded bytes tier)

v1 `run()` returns `str` accessors, so it **validates UTF-8 and errors (`Error.Invalid`) on invalid
bytes** — consistent with `fs.read_file` (string, validated) vs `read_bytes_view` (bytes). Build /
test / lint output is UTF-8 in practice, and the client parses it as text. The robustness escape
hatch for arbitrary binary tool output is the Request 11 bytes tier `c.run_bytes() ->
Result<run_bytes, Error>` whose `.stdout()`/`.stderr()` return `slice<u8>` (no validation) — mirroring
`read_file` vs `read_bytes_view` one-for-one. It is a sibling handle + accessors over the same
capture engine, so it does not disturb the string tier. The exact bound and ownership contract is
in the extension ledger below.

## Effect classification

All impure (they fork a process / observe the machine).

## Error policy

fork/pipe/dup2/waitpid failures → the errno→`Error` table (M9). A timeout is `Error.Timeout`
(explicit `AL_TIMEOUT` at the timeout site — never inferred from an errno). Non-UTF-8 captured output
→ `Error.Invalid`. A `chdir`/`exec` failure in the child surfaces as the child's `_exit(127)` → exit
code 127 in `out.code()` (same convention as `spawn`), **not** an `Err` — the fork itself succeeded.

## New machinery required

- `Error.Timeout` + `AL_TIMEOUT` (the shared prerequisite above).
- Two new opaque Move-handle types `Ty::Command` / `Ty::RunOutput` (+ `Scalar::Command` /
  `Scalar::RunOutput`). Each must run the **full new-Ty sweep** a new Move handle must not skip
  (modeled on the recent `Ty::Captures` / existing `Ty::HttpResponse`): sema `scalar_of`/inverse,
  `needs_drop` / the four move-classifier `matches!` lists, `tracks_region`, `region_of`, the
  element-borrow intercept, `ty_name`, the `scalar_arg` Move-rejection choke points; MIR
  move-classifier / owning-expr set / display name / `new_slot`; codegen LLVM pointer type,
  `handle_free_fn` (`Ty::Command => "command_free"`, `Ty::RunOutput => "run_output_free"`),
  zero-init-on-move set, and the runtime free-fn extern decls. `handle_free_fn` MUST match
  `is_field_ok`'s admitted set. This sweep is where a soundness hole hides — `/align-self-review`
  gate 2 ("a new IR variant skips an analysis pass").
- Runtime: `align_rt_command_new/cwd/env/env_clear/timeout_ns/run/free` +
  `align_rt_run_output_code/stdout/stderr/free`.

## Slice breakdown

4. `process.command` + `c.cwd(dir)` + `run()` — **both must-haves** (captured output + working
   directory). The `Command`/`RunOutput` handles + the full new-Ty sweep (the bulk of the machinery),
   the pipe+fork+dup2+two-pipe-`poll`-drain, the child `chdir`, `.code()`/`.stdout()`/`.stderr()`,
   UTF-8 validation. No timeout/env yet. `cwd` is folded in here because it is a must-have and the
   child-side `chdir` is trivial, so S4 is a complete must-have delivery (a `command` with a real
   setter, not empty scaffolding).
5. `c.timeout_ns(ns)` + the `Error.Timeout` core change — the "a hung test freezes the loop" fix
   (kill + `Err(Timeout)`). **SHIPPED.** `c.timeout_ns(ns: i64)` is an in-place bound-local setter
   (`()`); `ns == 0` = no timeout (the Slice-4 default), a negative `ns` aborts at build. `c.run()`
   threads the deadline through the two-pipe drain (`poll` with the remaining ns→ms, clamped `>= 1`).
   Past the deadline the runtime targets the owned **process group** when present and always the
   direct pid, closes both reads, reaps the direct child, and returns `Err(Error.Timeout)` with
   partial output discarded. The deadline remains active after pipe EOF.
   `timeout_ns == 0` keeps the blocking behavior.
6. `c.env(name,value)` + `c.env_clear()` — **SHIPPED.** Both are in-place bound-local setters (`()`).
   `c.env(name, value)` records a `(name, value)` override; `c.env_clear()` marks the child to start
   from an empty environment. The runtime `Command` gains `env: Vec<(CString, CString)>` + `env_clear:
   bool`; in the forked child, AFTER `chdir` and before the `dup2`s, it runs `if env_clear { clearenv()
   }` then `setenv(name, value, 1)` for each recorded pair (overwrite=1 — a later `env` for the same
   name wins; an `env` after an `env_clear` survives). Names/values marshal to C strings in the PARENT,
   so the child adds no per-pair marshalling allocation; `clearenv`/`setenv`/`execvp` retain the P11
   child-side allocation and async-signal-safety caveat. An interior-NUL / non-UTF-8 name or value
   aborts, and a name containing `=` aborts (`setenv` would reject it).
7. **SHIPPED:** the bytes tier `c.run_bytes()` and command-local `max_capture_bytes` bound —
   align-llm Request 11.

## Shared timeout-budget hardening (`pkg.kv` prerequisite 1 — ACCEPTED DESIGN 2026-09-02)

> **Status:** accepted design; inactive until implementation. The shipped `process.command`
> surface and behavior remain active until then. This prerequisite changes no public signature,
> compiler operation, runtime symbol, ABI shape, registry key, or row count.

The runtime's `poll_timeout_ms` helper is shared by TCP connect and command capture, so the checked
timeout prerequisite also replaces command capture's absolute `Instant::checked_add` deadline with
a monotonic start and positive `Duration` budget at the existing post-fork timeout anchor before the
first parent poll. It never forms `start + budget`; every checkpoint subtracts `start.elapsed()`
from the budget. Thus the complete positive i64 range remains bounded instead of degrading to an
unbounded run when an absolute deadline is unrepresentable.

Before each timed parent poll, including the allocation-free zero-fd wait after pipe EOF, a positive
remainder is rounded up to the next millisecond and saturated at `i32::MAX`; the existing post-EOF
one-millisecond sleep cap is applied afterward. An exhausted remainder returns `Error.Timeout`
before another poll. `EINTR` and a zero result recompute the monotonic remainder; zero re-polls only
when time remains. There is no final timeout-zero probe. `timeout_ns == 0` keeps the shipped
blocking behavior and a negative value still aborts in the setter.

This changes only deadline representation and wait quantization. The shipped post-syscall
checkpoint order remains exact: a timeout observed after `poll` or `read` wins before that result is
interpreted; after `waitpid`, consumption of a reaped child is recorded first and then an observed
timeout wins before result classification. Otherwise the existing stdout-before-stderr
hard-error/cap order, child-exit handling, cleanup, and UTF-8 precedence are unchanged. Scheduler
delay may make a return late, but rounding never expires a logical timeout early.

## Pitfalls

- **P7 (two-pipe deadlock)** — the #1 correctness point. `poll` **both** read fds and drain both, or
  a child filling one pipe while the parent reads the other deadlocks. Test: a child that writes
  >64 KiB to *both* streams and exits nonzero → both fully captured, code correct.
- **P8 (timeout must actually kill + reap)** — the deadline covers pipe drain **and the wait after
  pipe EOF**. On expiry `SIGKILL` the process group and direct pid, close both capture reads, and
  `waitpid` the direct child; do not leak a zombie or wedge on a child that closes fd 1/2 and keeps
  running. Acceptance test:
  both `sleep 10` and `exec 1>&- 2>&-; sleep 10` with a 100 ms timeout produce
  `Err(Timeout)` after the logical deadline and before the controlled test's generous stall-detector
  ceiling, with no direct-child zombie. That detector ceiling is not a public wall-clock promise.
- **P9 (view region, like http P3)** — `.stdout()`/`.stderr()` are views into `out`; `region_of =
  region_of(out)`. Escape past `out` Drop rejected.
- **P10 (new-Ty sweep)** — `Ty::Command`/`Ty::RunOutput` must hit every pass in New machinery; a
  skipped pass is a leak/double-free/UAF. Run `/align-self-review` gate 2.
- **P11 (child async-signal-safety)** — `chdir`/`clearenv`/`setenv`/`execvp` after `fork` in a
  threaded parent carry the existing `spawn` hazard (documented; `posix_spawn` is the deferred ideal).
- **P12 (unbounded capture)** — a runaway child (`yes`) grows the capture without bound. Existing
  callers remain unbounded, while the Request 11 extension below supplies an explicit command-local
  bound and the binary-output tier.

## Test checklist / gate

- child writes to **both** stdout and stderr and exits nonzero → caller recovers the full stdout
  string, the full stderr string, and the exit code (the Request-1 acceptance gate).
- `c.cwd(dir)` → the child observes `dir` as its working directory.
- a command exceeding `timeout_ns` → `Err(Timeout)` (distinct from a nonzero exit), killed, no zombie.
- `c.env(n,v)` overrides / `c.env_clear()` starts empty → child sees the expected environment.
- non-UTF-8 output → `Error.Invalid` (string tier).
- two-pipe >64 KiB each → no deadlock (P7).
- `.stdout()` view escaping past `out` Drop → rejected (P9).
- `command` / `run_output` as an array element → rejected.
- import-required.
- Planned shared-timeout owners: exact millisecond, the next nanosecond, and maximum-positive i64
  budgets; `EINTR` remainder recomputation; an early zero poll result with time remaining; exhausted
  remainder before another poll; and no early expiry. These activate only with the accepted-design
  prerequisite above.

# Extension — bounded text and byte capture (align-llm Request 11)

> **Status: SHIPPED (2026-08-14).** This extension closes P12 for
> callers that select a bound and ships the previously deferred binary-output tier. Source:
> `../align-llm/docs/align-requests.md` Request 11.

## Public-contract ledger

The ledger below is authoritative for the extension. The prose and implementation must not broaden
it independently.

| Surface | Exact contract |
|---|---|
| `c.max_capture_bytes(limit: i64) -> ()` | Bound-local, in-place `command` setter. `limit >= 0`; a negative value is a programmer error and aborts before a child or allocation. The latest call overwrites the previous bound. An explicit `0` permits only empty stdout and empty stderr. A command with no call retains the shipped unbounded behavior. The same bound applies independently to each stream and to every later `run()` or `run_bytes()` on that command. |
| `c.run() -> Result<run_output, Error>` | Existing borrowed, re-runnable text capture. With a bound, each stream may contain at most `limit` bytes. Exact-limit output succeeds; the first observed byte beyond either stream's limit signals the owned child process group when present, kills and reaps the direct child, discards both partial streams, and returns `Error.Invalid`. Completed in-bound output is UTF-8 validated exactly as today; invalid UTF-8 returns `Error.Invalid`. Nonzero exit remains `Ok(run_output)`. |
| `c.run_bytes() -> Result<run_bytes, Error>` | New borrowed, re-runnable binary capture over the same command configuration, drain, cap, timeout, kill, and reap path as `run()`. It performs no UTF-8 validation. `run_bytes` is one opaque Move handle owning the exit code and both byte buffers. |
| `out.code() -> i64` | Both `run_output` and `run_bytes` expose the shipped decoded exit code: `WEXITSTATUS`, `128 + signal`, or `127` when child-side `chdir`/`execvp` fails. Pure Copy read. |
| `out.stdout()`, `out.stderr()` | `run_output` returns zero-copy `str` views; `run_bytes` returns zero-copy `slice<u8>` views. Empty output is an empty view. Each view is region-bound to its owning output handle and cannot escape its Drop. Embedded NUL is ordinary data in the byte tier; the text tier accepts it when the complete stream is valid UTF-8. |

The cap is **per stream**, not combined. A selected limit `L` therefore permits at most `L` bytes of
stdout and `L` bytes of stderr, for at most `2L` retained capture bytes. This preserves useful
diagnostics when stdout uses its complete consumer-declared response budget and gives each pipe one
stable rule independent of scheduling. C6 sets `65_536` for its helper command and `262_144` for its
measurement command; it does not perform an unbounded run followed by a length check.

### Inputs, defaults, errors, and precedence

- `max_capture_bytes` takes an `i64` byte count. There is no ambient environment variable, global
  default, or package setting. Unset is the only unbounded state; `0` is a real zero-byte bound.
- Negative `limit` aborts at the setter before allocation or process creation. A bounded run first
  validates its output slot, converts the non-negative limit to the platform allocation size, and
  creates both capture stores plus the mode-specific empty output-handle shell. An unrepresentable
  layout returns `Error.Invalid`. Physical capture/output allocation failure follows Align's locked fatal-OOM
  policy: it aborts immediately without unwinding or a recoverable `Error` value. No child has
  started in either case; process teardown reclaims any earlier preallocation after fatal OOM.
- Pipe/fork/nonblocking-setup failures retain the fixed errno mapping. Pipe creation and both read-fd
  `fcntl` operations complete before fork; a hard setup error closes every opened fd, frees bounded
  preallocation, and starts no child. Child-side `chdir`/`execvp` failure remains `Ok` with code
  `127`. A nonzero child exit never outranks a capture failure.
- The shared post-fork engine owns one state machine from the first pipe poll until **both** streams
  reached EOF and the direct child was reaped. It checks the monotonic deadline before every `poll`,
  before interpreting each descriptor/read result in stdout-then-stderr order, and before every
  `waitpid(WNOHANG)` checkpoint after pipe EOF. If expired, `Error.Timeout` wins. Otherwise a positive
  read that would cross either cap produces `Error.Invalid`; `POLLNVAL`, a non-`EINTR` poll failure,
  or a non-`EINTR`/non-`EAGAIN` read failure returns the fixed `Error.Code(errno)` mapping. `POLLNVAL`
  maps deterministically to `EBADF`. Thus an already-observable timeout wins, otherwise the first
  stdout-before-stderr hard pipe error or overflow wins. Any capture error outranks later exit status
  or UTF-8 validation.
- Once both pipes reach EOF, an untimed run may use blocking `waitpid`; a timed run repeatedly checks
  the deadline, calls `waitpid(WNOHANG)`, and sleeps without allocation using zero-fd `poll` for at
  most `min(remaining, 1 ms)`. It therefore returns promptly when a child closes stdout/stderr and
  continues running, instead of entering an unbounded wait. `EINTR` retries from the deadline
  checkpoint. A hard `waitpid` error uses its fixed errno mapping; `ECHILD` means the direct child is
  already reaped and never authorizes a partial successful result.
- After both streams reach EOF, text `run()` validates stdout first and stderr second; either invalid
  stream returns the same `Error.Invalid`, so no partial output or stream identity is exposed.
- Every post-fork timeout/overflow/hard-pipe/hard-wait failure snapshots its winning status and,
  while the direct child remains waitable, sends `SIGKILL` to the owned process group when this run
  created one and to the direct pid. It closes both read fds, retries direct-child `waitpid` on
  `EINTR` (`ECHILD` is already-reaped), frees capture/output state, and returns the original status
  with the result slot null. A successful wait or `ECHILD` marks the pid consumed before later
  deadline/error cleanup, so a potentially recycled pid is never signalled; `ECHILD` remains a hard
  `Error.Code`, not a synthesized exit status. Only the direct child is reaped by this caller; in-group
  descendants are signalled, and `setsid` descendants remain outside the contract. Fatal OOM has no
  cleanup or unwind path and, for a bounded run, occurs before pipe creation/fork. No recoverable
  error returns partial bytes, an exit code, or a truncation marker.

### Ownership, lifetime, allocation, and concurrency

`command` remains a single-owner Move handle and `run()`/`run_bytes()` borrow it. The optional bound
persists across repeated runs and both run modes. `run_bytes` follows the complete new-Move-type
sweep: it may occupy only its constructor's `Result` Ok slot, cannot be an aggregate/array element or
captured task value, zeroes its source on move, and drops its two buffers exactly once. Its views
carry `region_of(run_bytes)` just as text views carry `region_of(run_output)`.

For a bounded run, the runtime allocates one exact `L`-byte capture layout per stream and the empty
mode-specific output-handle shell **before** pipe creation and fork; `L == 0` allocates neither byte
layout. No capture allocation or reserved capture capacity is larger than `L`, and the two live
capture layouts total exactly `2L`. The existing fixed 64 KiB stack read scratch, the command's
argv/cwd/environment storage, two pipe descriptors, a fixed stack `[PollFd; 2]`, and the small output
handle are outside that declared capture-store bound. The **parent bounded capture/reap state
machine** performs no heap allocation after fork: the poll descriptor array is filled in place, the
post-EOF wait uses zero-fd `poll`, and reads land in the fixed scratch then copy only when the complete
chunk fits the selected stream's remaining capacity. Success fills the existing shell and transfers
the two layouts without another allocation; every recoverable error frees all three preallocated
objects. Fatal OOM for those capture/output allocations therefore terminates before pipe creation and
fork under the process-wide allocation rule. The forked child remains on the existing P11 launch
path: `clearenv`/`setenv` and `execvp` may allocate, and this capture contract neither strengthens nor
hides that caveat. Both read ends are made nonblocking before fork; a failed `F_GETFL`/`F_SETFL` is a
recoverable setup error, never a best-effort post-fork assumption.
Unbounded callers retain the existing growable `Vec` behavior and make no memory-bound claim.

A command with either a positive timeout or an explicit capture bound creates the child as a process
group leader. Timeout, overflow, or a hard post-fork capture/wait error sends `SIGKILL` to that owned
group when present and to the direct pid while it remains waitable, closes both read ends without
another EOF drain, then waits for the direct child with `EINTR` retry. A wait result that already
consumed the child suppresses later signalling of that pid. Descendants that deliberately escape with `setsid` are
outside the process-group contract; closing the read ends ensures they cannot keep the caller
blocked. The caller never claims to reap descendants. Distinct commands share no mutable state, so
concurrent runs are independent. The same `command` cannot be mutably configured during a run in safe
Align, and one synchronous run completes before that command is reused.

### Runtime and cache identity

The implementation adds these ABI entries without changing an existing signature:

```text
align_rt_command_max_capture(command: ptr, limit: i64) -> void
align_rt_command_run_bytes(command: ptr, out: ptr<ptr>) -> i32
align_rt_run_bytes_code(out: ptr) -> i64
align_rt_run_bytes_stdout(out: ptr) -> { ptr, i64 }
align_rt_run_bytes_stderr(out: ptr) -> { ptr, i64 }
align_rt_run_bytes_free(out: ptr) -> void
```

`align_rt_command_run` keeps its ABI and consumes the new bound stored in `Command`. Both run entries
delegate to one runtime capture engine; the only mode-specific step is completed-buffer UTF-8
validation and output-handle construction.

`run_bytes` has one exact append-only encoding in each compiler-owned type format:

- canonical type codec version 3 remains unchanged; `Ty::RunBytes` is the new leaf tag `60` and
  `Scalar::RunBytes` is the new leaf tag `36`, after the existing `59` and `35` maxima. The root
  semantic-to-byte and byte-to-semantic golden is `RunBytes <-> [3, 0, 0, 0, 0, 60]`; the scalar
  field golden is `RunBytes <-> [36]`. Tags are never inserted or renumbered. Unknown tags `61` and
  `37`, and a truncated root `[3, 0, 0, 0, 0]`, reject before cache publication.
- current interface summary format 7 represents every source type as `IType::Named`; `run_bytes`
  added neither a type discriminator nor its own version bump. The Request 9 codec-wide format-7
  bump preserves this named-type record. `run_bytes` uses existing type tag `0`, UTF-8 path
  `run_bytes`, and zero type arguments. Its field-local golden is
  `[0, 9, 0, 0, 0, 114, 117, 110, 95, 98, 121, 116, 101, 115, 0, 0, 0, 0]` in both directions.
  Unknown type tag `3`, a truncated name/argument count, invalid UTF-8, or trailing bytes rejects
  before semantic import.

HIR, MIR, LLVM lowering, and checked-HIR validation use the same new closed builtin type. There is
no runtime reflection, callback, separate user wire format, or ambient cache input; the canonical
type bytes and versioned interface summary are the persisted compiler records. Whole-program and
per-unit compilation of the same source must select the same run mode and output type. Changing
between `run` and `run_bytes`, or adding/changing the bound call in source, invalidates the ordinary
source-derived frontend/object cache entry.

## Implementation closure matrix

| Cell | Owner and required evidence |
|---|---|
| Setter formation and validation | Sema/HIR/MIR/codegen/runtime route `max_capture_bytes(i64)` only on a bound `command`; wrong arity/type and temporary receivers diagnose; negative aborts before side effects; `0`, overwrite, and unset states are distinct. Owner: `m11_process_command::command_capture_bound_formation_and_state`. |
| Exact-limit text success | Shared drain admits empty, `L`, stdout-only, stderr-only, and simultaneous `L`/`L`; `run_output` retains existing code/text views. Owner: `command_capture_exact_limit_and_reuse`. |
| One-byte overflow | Either stream at `L + 1`, including simultaneous pipe pressure, returns `Error.Invalid`, exposes no output, kills the group/direct pid, closes fds, and reaps the direct child once. Owner: `command_capture_overflow_kills_group_and_discards_partial`. |
| Timeout/cap/exit/UTF-8 precedence | Checkpoint order above is exercised for timeout-before-overflow, overflow-before-timeout, nonzero-overflow, in-bound invalid UTF-8, and a child that closes both streams then remains alive beyond the deadline. Owner: `command_capture_error_precedence` plus `command_timeout_covers_post_eof_wait`. |
| Planned timeout-budget quantization | At the unchanged post-fork anchor, exact-ms, next-ns, and maximum-positive i64 budgets use monotonic start+duration subtraction; `EINTR` and early-zero results recompute, exhaustion returns before another poll, and no case expires early or performs a final timeout-zero probe. Existing post-syscall timeout/error precedence remains green. Owner: `command_timeout_budget_quantization` (activates with the accepted-design shared prerequisite). |
| Hard pipe/wait errors | Inject non-`EINTR` poll, `POLLNVAL`, stdout/stderr hard read, and post-EOF `waitpid` errors. Timeout wins when already observable; otherwise stdout precedes stderr, the original fixed errno survives cleanup, no partial result escapes, an owned group (if present) and direct pid are killed, fds close, and the direct child is reaped or already `ECHILD`. Owner: `command_capture_hard_io_errors_are_terminal`. |
| Post-fork lifecycle | Parameterize `{pipes open/EOF} × {child live/exited} × {untimed/timed/bounded}`. Success requires both EOF and a reaped direct child; a timed EOF/live child uses WNOHANG plus allocation-free zero-fd poll until exit/deadline. Owner: `command_capture_lifecycle_state_matrix`. |
| Binary tier | `run_bytes` preserves invalid UTF-8 and embedded NUL byte-for-byte, exposes region-bound byte views, supports nonzero exit, and shares exact-cap behavior. Owner: `command_run_bytes_preserves_arbitrary_output`. |
| Move and Drop | Formation, construction, `Result` move-in/out, `?`, `else`, `match`, `map_err`, replacement, return, source nulling, and early exit drop each output once; aggregate/capture/temporary and escaped views reject. Owners: `m11_process_command` ownership matrix plus the checked-HIR variant tripwire. |
| Allocation, descriptor setup, and malformed limits | Exact layouts/shell are allocated before pipes/fork; zero allocates no byte stores; unrepresentable layout is `Invalid`; physical capture/output allocation failure aborts without unwind before a child exists; both read fds become nonblocking before fork or setup fails cleanly; fixed poll/scratch/wait storage gives the parent bounded capture/reap state machine zero post-fork heap allocations; capture capacity never exceeds `L`. Owners: subprocess fatal-OOM failpoints for first/second/shell allocation, `fcntl` failpoints, and `command_capture_allocation_bound`; child-side markers prove fork was never reached. |
| Child launch boundary | Bounded terminals reuse the existing post-fork child `chdir`/environment/`execvp` path and introduce no new child-side operation. `clearenv`/`setenv`/`execvp` may allocate and retain P11; the parent capture bound and fatal-OOM-before-fork claim do not apply to them. Owners: existing cwd/env/env_clear/exit-127 regressions within `m11_process_command`. |
| Reuse and concurrency | One command repeats text and byte runs with the persisted/overwritten bound; two independent commands run concurrently without shared state. Owner: `command_capture_reuse_and_independent_concurrency`. |
| Generic/interface/per-unit/cache parity | A function returning `Result<run_bytes, Error>` has identical whole-program and per-unit type/ABI; interface round-trip and exact edit/revert cache identity agree. Owners: process interface/per-unit/cache tests. |
| Existing behavior | No-setter `run()` remains unbounded and byte-for-byte compatible; cwd/env/env_clear/timeout, large dual-pipe, nonzero exit, and text view owners remain green. Owner: complete `m11_process_command` target. |

The explicit memory promise also requires a local `bench/process_capture` measurement. It records
bounded text/byte throughput and the maximum live capture-layout bytes for the 65,536 and 262,144
consumer limits versus the existing unbounded path. It is evidence for the resource contract, not a
correctness gate.

### Closure matrix reopened: post-fork lifecycle

The second review found that the first matrix bounded pipe capture but stopped before direct-child
termination. A later projection review found that the target-facing overview still described the old
unconditional post-EOF wait and treated the existing child launcher as part of the parent's
no-allocation promise. A status audit then separated that corrected target from the still-shipped
Slice-5 behavior instead of describing the pending replacement as already implemented.
The reopened axis is `{parent capture/child launch} × {pipe state} × {direct-child state} × {deadline
state} × {terminal trigger}`. The shared parent engine is now one indivisible capability from
pre-fork setup through EOF, direct-child wait, and terminal cleanup; the existing P11 child launcher
is an explicit adjacent boundary. Splitting the new producer/type work from this runtime consumer
would leave the existing timeout hang and truncated-success path reachable, so the design and
implementation remain one mergeable capability.

## Design-review finding closure

| Finding | Ledger-first closure |
|---|---|
| P1 recoverable OOM contradicted the locked allocation model | The error/allocation rows now retain fatal OOM with no unwind; only unrepresentable layouts return `Error.Invalid`, and subprocess failpoints prove abort-before-fork. |
| P1 HIR and native owner ledgers omitted the new surface | `docs/impl/19-hir-validation-ledger.md` reserves the five exact expression rows and malformed fixtures; `docs/impl/20-runtime-abi-ledger.md` reserves all six keyed symbols, declarations, attributes, counts, and registry owners. |
| P2 compiler type encodings were implementation-defined | The canonical codec keeps version 3 and appends exact `Ty`/`Scalar` tags 60/36 with bidirectional and malformed vectors; current interface format 7 preserves format 6's existing named-type record and exact byte vector. |
| P2 the external request register remained proposed | The sibling register records the accepted per-stream/text/bytes/ownership/error contract and the final reviewed design commit; the edit remains uncommitted in that repository as required. |
| P1 deadline ended at pipe EOF | The reopened lifecycle keeps the deadline active until direct-child reap; EOF/live uses `waitpid(WNOHANG)` plus allocation-free zero-fd `poll`, with its own owner. |
| P1 bounded poll allocated after fork | The allocation row requires exact stores/shell before pipes, fixed stack poll/scratch state, pre-fork nonblocking setup, and no parent-side bounded post-fork heap allocation. |
| P2 hard poll/read errors became partial success | The precedence and hard-I/O rows map the first deterministic errno, run the same kill/close/direct-reap cleanup, preserve the original status, and expose no output. |
| P2 normative group reaping was overstated | Specifications now say the process group is signalled while only the direct child is reaped; escaped descendants remain outside the contract. |
| P1 target overview still ended the deadline at pipe EOF | The Request 11 lifecycle keeps the deadline active through direct-child reap: timed EOF/live-child state uses `waitpid(WNOHANG)` plus zero-fd `poll` until exit or expiry. |
| P1 no-allocation promise included the existing child launcher | The allocation promise and owner row now cover only the parent bounded capture/reap state machine; a separate child-launch row retains P11 for `clearenv`/`setenv`/`execvp`. |
| P2 target timeout overview omitted the direct-pid fallback | Every Request 11 description snapshots the status, signals an owned group when present and the direct pid while it remains waitable, closes both reads, and reaps only the direct child with `EINTR` retry. A successful wait or `ECHILD` suppresses later signalling of a potentially recycled pid. |
| P1 pending lifecycle was attributed to shipped Slice 5 | The design-time status split kept the old Slice-5 behavior distinct until the complete Request 11 state machine activated atomically. |
| P2 pending direct-pid fallback was attributed to shipped Slice 5 | The design-time ledger reserved the direct-pid fallback while the child remains waitable; the shipped runtime and owner now exercise it. |
| P2 `code()` was described as a region-bound view | The condensed specification now identifies `code()` as a Copy `i64` and limits region-bound zero-copy views to `stdout()`/`stderr()`. |

## Acceptance gate

Implementation acceptance and consumer adoption require:

1. every matrix row points to implementation and a regression owner, with the new type included in
   the exhaustive variant tripwire;
2. the English and Japanese process designs, `draft.md`, the condensed language specification,
   design notes, Settled decisions, checked-HIR ledger, runtime ABI ledger, and align-llm request
   register agree;
3. the focused process owner, bounded PR gate, library/binary Clippy, whole/per-unit/cache owners,
   allocation failpoint, and local resource measurement pass on the final candidate; and
4. align-llm may advance the request only after it pins the named merged implementation commit and
   its focused helper/adapter target proves the 65,536/262,144 bounds, timeout/cap precedence,
   process-group cleanup, and arbitrary-byte tier before the capability wave's final `make ci`.
