//! M11 std.process Slice 1 — `process.exit(code)` / `process.abort()` with the settled
//! cleanup-then-exit semantics (`docs/impl/std-design/process.md`):
//!
//! - `process.exit(code)` runs the current function's pending cleanup (Drops for live owned locals —
//!   so buffered writers flush + close — and arena / `task_group` ends, the exact emission a
//!   top-level `return` uses) THEN calls libc `exit(code)`. The default hard-exit is the *safe* one
//!   (Nothing-hidden: no silently lost buffered output).
//! - `process.abort()` is the named-dangerous escape hatch: immediate `_exit(1)`, running NO cleanup
//!   (the asymmetry — pending buffered output is intentionally lost).
//!
//! The headline is the P1 asymmetry proof: the SAME buffered write is flushed on `exit` and dropped
//! on `abort`. Also pins the exit-code low-byte truncation, dead-code-after-exit (parity with
//! code-after-`return`: no ICE, not emitted), the `import std.process` capability gate, and impurity
//! (rejected by `par_map`).

mod common;
use common::*;

/// `process.exit(3)` terminates the process with exit code 3 (low byte of the `i64`).
#[test]
fn exit_sets_the_process_exit_code() {
    if !backend_available() {
        return;
    }
    // `process.exit(3)` is a statement; the trailing `0` is dead (never runs) but makes `main`'s
    // return type `i32` (there is no `Never` type yet, so `exit` is typed `()` and cannot be the
    // tail value of a non-unit fn).
    let src = "import std.process\npub fn main() -> i32 {\n  process.exit(3)\n  0\n}\n";
    let out = build_and_run("proc-exit-code", src);
    assert_eq!(out.status.code(), Some(3), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stdout.is_empty(), "no output expected");
}

/// **P1 (the critical test)**: a buffered stdout writer with a pending (unflushed) write, then
/// `process.exit(0)` — the buffered bytes ARE flushed by the writer's `Drop`, which the exit
/// cleanup runs before terminating. Proves exit is not a naive libc `exit()` (that would silently
/// drop the buffer).
#[test]
fn exit_flushes_pending_buffered_writer_output() {
    if !backend_available() {
        return;
    }
    let src = "import std.io\nimport std.process\npub fn main() -> Result<(), Error> {\n  out := io.stdout.buffered()\n  out.write(\"flushed on exit\\n\")?\n  process.exit(0)\n  Ok(())\n}\n";
    let out = build_and_run("proc-exit-flush", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "flushed on exit\n",
        "the buffered write must be flushed by the exit cleanup"
    );
}

/// The asymmetry: the SAME buffered write followed by `process.abort()` is NOT flushed (abort runs
/// no cleanup), and the process exits non-zero (`_exit(1)`).
#[test]
fn abort_skips_cleanup_and_loses_buffered_output() {
    if !backend_available() {
        return;
    }
    let src = "import std.io\nimport std.process\npub fn main() -> Result<(), Error> {\n  out := io.stdout.buffered()\n  out.write(\"NOT flushed\\n\")?\n  process.abort()\n  Ok(())\n}\n";
    let out = build_and_run("proc-abort-noflush", src);
    assert_eq!(out.status.code(), Some(1), "abort exits non-zero via _exit(1)");
    assert!(
        out.stdout.is_empty(),
        "abort must NOT flush the buffered writer, got: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// `process.exit` after an `arena` use runs clean: the arena is allocated in, then `exit` runs the
/// arena-end cleanup before terminating (no leak / no crash). A companion to the buffered-writer P1
/// test on the arena-cleanup path.
#[test]
fn exit_inside_arena_runs_the_pending_arena_end() {
    if !backend_available() {
        return;
    }
    // Exit from INSIDE the arena block: the pending arena end must be emitted by the exit
    // cleanup itself — the block's normal close is never reached (the exit terminates it).
    // Prints 42, then exits 0.
    let src = "import std.process\npub fn main() -> i32 {\n  arena {\n    b := heap.new(42)\n    print(b.get())\n    process.exit(0)\n  }\n  0\n}\n";
    let out = build_and_run("proc-exit-arena", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

/// The exit code is narrowed `i64 -> i32` and observed as the low 8 bits on a Unix `wait`
/// (`WEXITSTATUS`): `exit(256)` is observed as `0`. Pins the documented truncation.
#[test]
fn exit_code_is_truncated_to_the_low_byte() {
    if !backend_available() {
        return;
    }
    let src = "import std.process\npub fn main() -> i32 {\n  process.exit(256)\n  0\n}\n";
    let out = build_and_run("proc-exit-trunc", src);
    assert_eq!(out.status.code(), Some(0), "256 & 0xff == 0");
}

/// Code after `process.exit(...)` compiles without ICE (parity with code after `return`) and is NOT
/// executed — the diverging call terminates the block, so the trailing `print` is never emitted.
#[test]
fn code_after_exit_is_dead_and_not_run() {
    if !backend_available() {
        return;
    }
    let src = "import std.process\npub fn main() -> i32 {\n  process.exit(7)\n  print(999)\n  0\n}\n";
    let out = build_and_run("proc-exit-dead", src);
    assert_eq!(out.status.code(), Some(7));
    assert!(out.stdout.is_empty(), "the post-exit print must not run, got: {:?}", String::from_utf8_lossy(&out.stdout));
}

/// `process.abort()` takes no arguments; passing one is a compile error.
#[test]
fn abort_rejects_arguments() {
    let src = "import std.process\npub fn main() -> i32 {\n  process.abort(1)\n  0\n}\n";
    assert!(check_errs("proc-abort-args", src));
}

/// Using `process.exit` / `process.abort` without `import std.process` is a compile error (the
/// capability gate).
#[test]
fn process_requires_import() {
    assert!(check_errs("proc-noimport-exit", "pub fn main() -> i32 {\n  process.exit(0)\n  0\n}\n"));
    assert!(check_errs("proc-noimport-abort", "pub fn main() -> i32 {\n  process.abort()\n  0\n}\n"));
}

/// A missing `import std.process` names the capability in the diagnostic.
#[test]
fn missing_import_diagnostic_names_the_capability() {
    let diags = check_diagnostics("proc-diag", "pub fn main() -> i32 {\n  process.exit(0)\n  0\n}\n");
    assert!(diags.contains("import std.process"), "diagnostic should name the capability: {diags}");
}

/// `process.exit` / `process.abort` are Impure (they terminate the process — an external effect), so
/// a `par_map` closure that calls one is rejected by the Pure requirement.
#[test]
fn exit_in_par_map_is_rejected() {
    // `kill` calls `process.exit` → impure → `par_map(kill)` rejected. (The trailing `x` gives the
    // impure helper its `i64` return.)
    let src = "import std.process\nfn kill(x: i64) -> i64 {\n  process.exit(x)\n  x\n}\nfn main() -> Result<(), Error> {\n  ys := [1, 2].par_map(kill)\n  print(ys.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("proc-exit-parmap", src));
}

/// Likewise `process.abort()` inside a `par_map` closure is rejected (impure).
#[test]
fn abort_in_par_map_is_rejected() {
    let src = "import std.process\nfn boom(x: i64) -> i64 {\n  process.abort()\n  x\n}\nfn main() -> Result<(), Error> {\n  ys := [1, 2].par_map(boom)\n  print(ys.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("proc-abort-parmap", src));
}

// --- Slice 2 — `child` / `process.spawn` / `ch.wait()` + Drop-reaps-via-waitpid ----------------
//
// `process.spawn(cmd, args) -> Result<child, Error>` forks + `execvp`s (an owned Move handle owning
// the pid; `Drop` reaps it via a blocking `waitpid`, so a dropped-without-`wait()` child can't zombie
// — P2, no `SA_NOCLDWAIT`). `args` is the child's FULL argv incl. `argv[0]` (P5). `ch.wait()` blocks
// in `waitpid` and returns the exit code (`WEXITSTATUS`, or `128 + signal` for a signal-killed child —
// the shell convention); it borrows the child (never consumed — mirrors `l.accept()`) and flips its
// reaped state so the later `Drop` is a no-op. A double-`wait()` is a clean `Err`. The CLOEXEC (P3)
// and rigorous no-zombie (`ECHILD` after the reap) proofs are runtime unit tests in `align_runtime`.

/// A program that spawns `args[1]` with the child argv `args[1..]` (the full argv incl. `argv[0]`),
/// waits, and prints the exit code — the harness supplies the command + its argv as `prog_args`.
const SPAWN_WAIT_PRINT: &str = "\
import std.process
pub fn main(args: array<str>) -> Result<(), Error> {
  ch := process.spawn(args[1], args[1..])?
  code := ch.wait()?
  print(code)
  return Ok(())
}";

/// `/bin/true` exits 0 → `wait()` returns 0. The child argv is a single element (`argv[0]` only).
#[test]
fn spawn_true_waits_zero() {
    if !backend_available() {
        return;
    }
    let out = build_and_run_args("m11proc-true", SPAWN_WAIT_PRINT, &["/bin/true"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0", "spawn /bin/true → wait 0");
}

/// `/bin/false` exits 1 → `wait()` returns 1.
#[test]
fn spawn_false_waits_one() {
    if !backend_available() {
        return;
    }
    let out = build_and_run_args("m11proc-false", SPAWN_WAIT_PRINT, &["/bin/false"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1", "spawn /bin/false → wait 1");
}

/// An exec-not-found cannot be reported synchronously (the fork already happened): the forked child
/// `_exit(127)`s (the shell convention), so `wait()` returns 127 (P5).
#[test]
fn spawn_nonexistent_waits_127() {
    if !backend_available() {
        return;
    }
    let out = build_and_run_args("m11proc-missing", SPAWN_WAIT_PRINT, &["/nonexistent/definitely-not-a-real-binary"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "127", "exec-not-found → wait 127");
}

/// A signal-killed child yields `128 + signal`: `sh -c 'kill -9 $$'` sends itself `SIGKILL` (9), so
/// `wait()` returns `128 + 9 = 137` (the shell convention; `ch.kill` is a Slice-3 API, so the child
/// kills itself here).
#[test]
fn spawn_signal_killed_child_is_128_plus_sig() {
    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let out = build_and_run_args("m11proc-signal", SPAWN_WAIT_PRINT, &["/bin/sh", "-c", "kill -9 $$"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "137", "SIGKILL-ed child → 128+9");
}

/// A `child` dropped without `wait()` is reaped by its `Drop` (a blocking `waitpid`) — no zombie, no
/// hang. Here `ch` drops at function end after `print`; `/bin/true` has already exited, so the reap is
/// immediate. (The rigorous no-zombie proof — the reap makes a later `waitpid` return `ECHILD` — is a
/// runtime unit test.)
#[test]
fn spawn_then_drop_without_wait_reaps_cleanly() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.process
pub fn main(args: array<str>) -> Result<(), Error> {
  ch := process.spawn(args[1], args[1..])?
  print(\"spawned\")
  return Ok(())
}";
    let out = build_and_run_args("m11proc-drop", prog, &["/bin/true"]);
    assert!(out.status.success(), "drop-without-wait exits cleanly (the reap does not hang)");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "spawned");
}

/// A second `wait()` on an already-reaped child is a clean `Err` (detected via the reaped flag, not an
/// `ECHILD` race). The first wait succeeds (prints the code); the second's `else`-unwrap runs.
#[test]
fn double_wait_second_is_err() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.process
pub fn main(args: array<str>) -> Result<(), Error> {
  ch := process.spawn(args[1], args[1..])?
  first := ch.wait()?
  print(first)
  match ch.wait() {
    Ok(code) => {
      print(code)
    }
    Err(_) => {
      print(\"second-err\")
    }
  }
  return Ok(())
}";
    let out = build_and_run_args("m11proc-double-wait", prog, &["/bin/true"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0\nsecond-err", "double-wait → clean Err");
}

/// A `child` threads through a function parameter (a Move handle passed by value) — but cannot be
/// collected into an array (a copied handle would double-reap its pid), rejected at construction like
/// `reader`/`writer`/`tcp_conn`.
#[test]
fn child_rejected_as_array_element() {
    let src = "\
import std.process
fn f(a: child, b: child) -> i64 {
  xs := [a, b]
  return xs.len()
}
pub fn main() -> i32 {
  return 0
}
";
    assert!(check_errs("m11proc-child-array", src), "a child cannot be an array element");
}

/// `process.spawn` is a fork+exec syscall — impure. A closure that spawns is never `Pure`, so `par_map`
/// (which requires a Pure closure) rejects it (the `tcp.connect` / `fs` / `io` impurity precedent).
#[test]
fn spawn_rejected_by_par_map() {
    let src = "\
import std.process
fn f(x: i64) -> i64 {
  ch := process.spawn(\"/bin/true\", [\"/bin/true\"][0..1]) else { return x }
  code := ch.wait() else { return x }
  return code
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m11proc-parmap", src), "a process.spawn-using (impure) closure must be rejected by par_map");
}

/// P4: a `child` is an owned Move handle — an unbound temporary (`process.spawn(...)?.wait()`) is not
/// dropped yet, so it cannot be a `wait()` receiver in v1 (its pid would never be reaped). Bind it
/// first. (Mirrors the `l.accept()` / reader/writer bound-receiver restriction.)
#[test]
fn child_wait_unbound_temporary_receiver_rejected() {
    let src = "\
import std.process
pub fn main(args: array<str>) -> Result<(), Error> {
  code := process.spawn(args[1], args[1..])?.wait()?
  print(code)
  return Ok(())
}
";
    assert!(check_errs("m11proc-unbound-recv", src), "a wait on an unbound child temporary must be rejected (bind first)");
}

/// A `child` moved by value into a function is reaped **exactly once**: the move nulls the caller's
/// slot (`null_moved_source`), so `main`'s exit `Drop` is a no-op and only the callee's param `Drop`
/// (after its `wait()`) reaps. Without the move-nulling this would double-reap (a recycled pid hazard).
#[test]
fn child_moved_into_function_is_reaped_once() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.process
fn run(c: child) -> Result<i64, Error> {
  code := c.wait()?
  return Ok(code)
}
pub fn main(args: array<str>) -> Result<(), Error> {
  ch := process.spawn(args[1], args[1..])?
  code := run(ch)?
  print(code)
  return Ok(())
}";
    let out = build_and_run_args("m11proc-move-fn", prog, &["/bin/true"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0", "child moved into run(), waited once");
}

/// `process.spawn` requires `import std.process` (the capability-header rule), like every other `std`
/// surface.
#[test]
fn process_spawn_requires_import() {
    let src = "\
pub fn main(args: array<str>) -> Result<(), Error> {
  ch := process.spawn(args[1], args[1..])?
  code := ch.wait()?
  print(code)
  return Ok(())
}
";
    assert!(check_errs("m11proc-noimport-spawn", src), "process.spawn without `import std.process` must error");
}
