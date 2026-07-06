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
fn exit_after_arena_use_runs_clean() {
    if !backend_available() {
        return;
    }
    // Allocate a box in an arena, read it back (prints 42), then exit(0). The arena end is part of
    // the exit cleanup; the program must run to completion with code 0.
    let src = "import std.process\npub fn main() -> i32 {\n  arena {\n    b := heap.new(42)\n    print(b.get())\n  }\n  process.exit(0)\n  0\n}\n";
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
