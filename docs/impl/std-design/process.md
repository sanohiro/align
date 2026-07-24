This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.process — implementation design (M11)

> 🌐 **English** · [Japanese](./ja/process.md)

> **Status:** M11 core complete (exit/abort, spawn/wait/kill, exec shipped). **Extension DESIGNED
> 2026-07-24** (align-llm Request 1): captured output + cwd / env / timeout via a `process.command`
> builder + `run_output` handle — see "Extension" at the end of this file. Not yet implemented.

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

---

# Extension — captured output + cwd / env / timeout (align-llm Request 1)

> **Status:** DESIGNED 2026-07-24, not yet implemented. Motivated by `align-llm` (a real client)
> whose core loop runs build/test/lint commands and **parses their output**. Source:
> `../align-llm/docs/align-requests.md` Request 1 (priority: critical — it blocks that loop).

## Why this is genuinely new

Slices 1–3 above ship `spawn`/`wait`/`kill`/`exec`, but `spawn` does a bare `fork` + `execvp` with
**no pipes and no `dup2`**: the child inherits the parent's fds and its output goes straight to the
terminal. The `child` handle is `{ pid, reaped }` only. So capturing `stdout`/`stderr` as strings,
running in a chosen working directory, passing a per-child environment, and bounding the run with a
timeout are all **impossible today** — and none of them are in this module's recorded design space
(the only prior deferrals were `detach()` and a `Never` type). This is a real-workload requirement,
not a planned gap.

## Shared prerequisite — the `Error.Timeout` variant (canonical definition; `std.http`/`std.net` reuse it)

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
match out.code() {
  0 => parse_clean(out.stdout()),
  _ => report(out.stderr()),
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

Builder handle `Command { argv: Vec<CString>, cwd: Option<CString>, env: Vec<(CString,CString)>,
env_clear: bool, timeout_ns: i64 }` built by `align_rt_command_new(cmd, args)` (reuse
`marshal_cmd_argv` for the argv, same interior-NUL / empty-argv / non-UTF-8 rejection). `cwd` / `env`
/ `env_clear` / `timeout_ns` are thin setters (env pairs marshalled via the `*const AlignStr, len`
slice ABI, one pair per `env` call). `run_output` handle `RunOutput { code: i64, out: Vec<u8>, err:
Vec<u8> }`.

`align_rt_command_run(c, out: *mut *mut RunOutput) -> i32`:

1. Create two pipes (`stdout`, `stderr`), both ends `O_CLOEXEC` (P3 — no leak into the child, and the
   read ends never reach the exec'd image).
2. `fork`. **Child** (async-signal-safety caveats identical to `spawn` — `execvp`/`chdir`/`setenv` in
   a forked threaded parent are the documented existing hazard, `posix_spawn` is the recorded ideal
   fix): `chdir(cwd)` if set (fail → `_exit(127)`); if `env_clear` then `clearenv()`, then `setenv`
   each override; `dup2` the two pipe write-ends onto fds 1 and 2; close all pipe fds; `execvp`; on
   failure `_exit(127)`.
3. **Parent**: close the write ends. Set both read fds non-blocking. **`poll` BOTH read fds together**
   and drain into `out.out` / `out.err` as data arrives — draining both concurrently is mandatory, or
   a child that fills the stderr pipe while the parent reads stdout **deadlocks** (the classic
   two-pipe capture bug). Loop until both hit EOF.
4. **Timeout**: if `timeout_ns > 0`, `poll` with the remaining deadline (ns→ms, clamp ≥1). On expiry:
   `kill(pid, SIGKILL)`, keep draining until EOF (the child is dying — a bounded, non-blocking drain,
   so the pipes don't wedge the reap), `waitpid`, then return **`AL_TIMEOUT`** (partial output is
   discarded — "report timeout, don't return a half-answer"). `timeout_ns == 0` = no timeout (block).
   Negative `timeout_ns` is rejected at `c.timeout_ns()` build time (abort, like `kill`'s sig range).
5. `waitpid` the child (reaped here — no zombie); `out.code = decode_wait_status(status)`.
6. **UTF-8**: validate `out.out` / `out.err` as UTF-8. Invalid → free and return `AL_INVALID` (the
   `fs.read_file` precedent — a `string`-typed accessor cannot expose non-UTF-8 bytes). See below.
7. `*out = Box::into_raw(Box::new(RunOutput{...}))`; return `0`.

`.stdout()`/`.stderr()` return `AlignStr { ptr: out.out.as_ptr(), len }` — a borrowed view, exactly
like `align_rt_http_resp_body`.

## UTF-8 policy (decision + the deferred bytes tier)

v1 `run()` returns `str` accessors, so it **validates UTF-8 and errors (`Error.Invalid`) on invalid
bytes** — consistent with `fs.read_file` (string, validated) vs `read_bytes_view` (bytes). Build /
test / lint output is UTF-8 in practice, and the client parses it as text. The robustness escape
hatch for arbitrary binary tool output is a **deferred bytes tier** `c.run_bytes() -> Result<run_bytes,
Error>` whose `.stdout()`/`.stderr()` return `slice<u8>` (no validation) — mirroring
`read_file` vs `read_bytes_view` one-for-one. Deferred, not shipped in the first slices; designed so
it drops in without disturbing the string tier (a sibling handle + accessors). Ship it if non-UTF-8
tool output proves real for a consumer.

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
   (kill + `Err(Timeout)`).
6. `c.env(name,value)` + `c.env_clear()`.
7. *(deferred)* the bytes tier `c.run_bytes()` — ship on demand.

## Pitfalls

- **P7 (two-pipe deadlock)** — the #1 correctness point. `poll` **both** read fds and drain both, or
  a child filling one pipe while the parent reads the other deadlocks. Test: a child that writes
  >64 KiB to *both* streams and exits nonzero → both fully captured, code correct.
- **P8 (timeout must actually kill + reap)** — on expiry `SIGKILL` then keep draining to EOF and
  `waitpid`; do not leak a zombie or wedge on a full pipe. Test: `sleep 10` with a 100 ms timeout →
  `Err(Timeout)` within ~100 ms, no zombie.
- **P9 (view region, like http P3)** — `.stdout()`/`.stderr()` are views into `out`; `region_of =
  region_of(out)`. Escape past `out` Drop rejected.
- **P10 (new-Ty sweep)** — `Ty::Command`/`Ty::RunOutput` must hit every pass in New machinery; a
  skipped pass is a leak/double-free/UAF. Run `/align-self-review` gate 2.
- **P11 (child async-signal-safety)** — `chdir`/`clearenv`/`setenv`/`execvp` after `fork` in a
  threaded parent carry the existing `spawn` hazard (documented; `posix_spawn` is the deferred ideal).
- **P12 (unbounded capture)** — a runaway child (`yes`) grows the capture without bound. v1 is
  unbounded (matches `read_file` reading a whole file); a `max_capture` cap is a recorded future knob,
  not v1.

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
