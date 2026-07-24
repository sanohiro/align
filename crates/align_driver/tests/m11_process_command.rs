//! std.process Slice 4 — `process.command(cmd, args)` + `c.cwd(dir)` + `c.run()` captured output
//! (`docs/impl/std-design/process.md` "Extension — captured output + cwd"):
//!
//! - `c := process.command(cmd, args)` builds a Move `command` builder handle (the captured-run dual
//!   of `spawn`, modeled on `http.request`).
//! - `c.cwd(dir)` sets the working directory in place (returns `()`); `c.run()` forks a child with
//!   BOTH stdout and stderr captured, drains both pipes to EOF (the P7 two-pipe drain), reaps the
//!   child, and yields `Result<run_output, Error>`.
//! - `out.code()` is the exit code; `out.stdout()` / `out.stderr()` are `str` VIEWS region-bound to
//!   `out` (an escape past `out`'s `Drop` is a compile error, P9).
//!
//! The headline is the Request-1 acceptance gate: capture stdout, stderr, and the exit code of a
//! child in one run. Also pins the cwd effect, the import gate, the view-escape rejection (P9), and
//! the Move-handle array-element rejection (P10).

mod common;
use common::*;

/// The Request-1 acceptance gate: run a child that writes to stdout AND stderr and exits nonzero,
/// then recover all three (the exit code, the full stdout, the full stderr).
#[test]
fn command_captures_stdout_stderr_and_code() {
    if !backend_available() {
        return;
    }
    if !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let src = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf HELLO; printf OOPS 1>&2; exit 7\"])\n\
  out := c.run()?\n\
  print(out.code())\n\
  print(out.stdout())\n\
  print(out.stderr())\n\
  Ok(())\n\
}\n";
    let out = build_and_run("cmd-capture", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains('7'), "exit code 7 captured; got: {s:?}");
    assert!(s.contains("HELLO"), "full stdout captured; got: {s:?}");
    assert!(s.contains("OOPS"), "full stderr captured; got: {s:?}");
}

/// `c.cwd(dir)` → the child observes `dir` as its working directory (`pwd` prints it).
#[test]
fn command_cwd_changes_working_directory() {
    if !backend_available() {
        return;
    }
    if !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let src = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"pwd\"])\n\
  c.cwd(\"/\")\n\
  out := c.run()?\n\
  print(out.stdout())\n\
  Ok(())\n\
}\n";
    let out = build_and_run("cmd-cwd", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let s = String::from_utf8_lossy(&out.stdout);
    assert_eq!(s.trim_end(), "/", "the child's pwd is the set cwd; got: {s:?}");
}

/// Slice 6 — `c.env(name, value)` → the child observes the added/overridden variable.
#[test]
fn command_env_is_observed_by_the_child() {
    if !backend_available() {
        return;
    }
    if !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let src = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf %s \\\"$ALIGN_E2E_V\\\"\"])\n\
  c.env(\"ALIGN_E2E_V\", \"hello-env\")\n\
  out := c.run()?\n\
  print(out.stdout())\n\
  Ok(())\n\
}\n";
    let out = build_and_run("cmd-env", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let s = String::from_utf8_lossy(&out.stdout);
    assert_eq!(s.trim_end(), "hello-env", "the child sees the env var set by c.env; got: {s:?}");
}

/// Slice 6 — `c.env_clear()` starts the child environment empty, then an `env` pair applied AFTER the
/// clear still survives (order: wipe first, then apply pairs). `HOME` is inherited from the parent and
/// (unlike `PATH`) is NOT re-synthesized by `sh` when unset, so it is the reliable "was it wiped?"
/// probe: a normal child would see it; after `env_clear` it is gone.
#[test]
fn command_env_clear_wipes_then_env_survives() {
    if !backend_available() {
        return;
    }
    if !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    if std::env::var_os("HOME").is_none() {
        return; // the contrast relies on an inherited HOME being present to wipe
    }
    let src = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf '%s|%s' \\\"${HOME:-EMPTY}\\\" \\\"${KEPT:-NONE}\\\"\"])\n\
  c.env_clear()\n\
  c.env(\"KEPT\", \"yes\")\n\
  out := c.run()?\n\
  print(out.stdout())\n\
  Ok(())\n\
}\n";
    let out = build_and_run("cmd-env-clear", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let s = String::from_utf8_lossy(&out.stdout);
    assert_eq!(s.trim_end(), "EMPTY|yes", "env_clear wipes inherited HOME, then the env pair survives; got: {s:?}");
}

/// Slice 6 — `c.env` / `c.env_clear` require a BOUND receiver (the v1 Move-temporary gate), like the
/// other command setters. Calling them on a temporary `process.command(...)` must error.
#[test]
fn command_env_requires_a_bound_receiver() {
    let temp_env = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  process.command(\"/bin/echo\", [\"/bin/echo\", \"hi\"]).env(\"K\", \"V\")\n\
  Ok(())\n\
}\n";
    assert!(check_errs("command-temp-env", temp_env), "env() on a temporary command must error (bind it first)");
    let temp_clear = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  process.command(\"/bin/echo\", [\"/bin/echo\", \"hi\"]).env_clear()\n\
  Ok(())\n\
}\n";
    assert!(check_errs("command-temp-env-clear", temp_clear), "env_clear() on a temporary command must error (bind it first)");
}

/// Slice 6 — `.env()` takes exactly two `str` arguments; a wrong arity or a non-`str` arg is a
/// diagnostic (mirrors `.timeout_ns()` requiring an `i64`).
#[test]
fn command_env_requires_two_str_args() {
    let one_arg = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"hi\"])\n\
  c.env(\"K\")\n\
  out := c.run()?\n\
  Ok(())\n\
}\n";
    assert!(check_errs("cmd-env-arity", one_arg), "env with one argument must error");
    let bad_arg = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"hi\"])\n\
  c.env(\"K\", 42)\n\
  out := c.run()?\n\
  Ok(())\n\
}\n";
    assert!(check_errs("cmd-env-badarg", bad_arg), "env with a non-str value must error");
}

/// Slice 5 — `c.timeout_ns(ns)` bounds the run: a child that overruns is killed and `c.run()` yields
/// `Err(Error.Timeout)`, matchable as the `Timeout` variant DISTINCTLY from `Ok`/a nonzero exit. The
/// `sleep 10` child with a 100 ms timeout returns the `Timeout` arm's exit code (42), not `Ok` (1).
#[test]
fn command_timeout_yields_err_timeout() {
    if !backend_available() {
        return;
    }
    if !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let src = "import std.process\n\
pub fn main() -> i32 {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"sleep 10\"])\n\
  c.timeout_ns(100_000_000)\n\
  match c.run() {\n\
    Ok(out) => out.code() as i32,\n\
    Err(e) => match e {\n\
      Timeout => 42,\n\
      _       => 2,\n\
    },\n\
  }\n\
}\n";
    let out = build_and_run("cmd-timeout", src);
    assert_eq!(out.status.code(), Some(42), "a hung child past its timeout → Err(Error.Timeout); stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// A `Timeout` set but NOT exceeded is inert: a quick child finishes and `c.run()` is `Ok`, with the
/// captured output and exit code intact (the timeout path is not taken). Also proves `Error.Timeout`
/// is a user-nameable variant usable in an ordinary `match` arm alongside the other categories.
#[test]
fn command_timeout_not_triggered_and_variant_is_nameable() {
    if !backend_available() {
        return;
    }
    if !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let src = "import std.process\n\
pub fn main() -> i32 {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf hi; exit 5\"])\n\
  c.timeout_ns(30_000_000_000)\n\
  match c.run() {\n\
    Ok(out) => out.code() as i32,\n\
    Err(e)  => match e {\n\
      NotFound => 90,\n\
      Invalid  => 91,\n\
      Denied   => 92,\n\
      Timeout  => 93,\n\
      Code(n)  => n,\n\
    },\n\
  }\n\
}\n";
    let out = build_and_run("cmd-timeout-inert", src);
    assert_eq!(out.status.code(), Some(5), "finished in time → Ok(out), exit code 5; stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// `.timeout_ns()` requires an `i64` argument — a non-integer is a type error.
#[test]
fn command_timeout_requires_i64() {
    let bad = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"hi\"])\n\
  c.timeout_ns(\"soon\")\n\
  out := c.run()?\n\
  print(out.code())\n\
  Ok(())\n\
}\n";
    assert!(check_errs("cmd-timeout-badarg", bad), "timeout_ns with a non-i64 argument must error");
}

/// `process.command` requires `import std.process`; without it the call is a diagnostic.
#[test]
fn command_requires_the_import() {
    let missing = "pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"hi\"])\n\
  out := c.run()?\n\
  print(out.code())\n\
  Ok(())\n\
}\n";
    assert!(check_errs("cmd-no-import", missing), "process.command without `import std.process` must error");
    let present = format!("import std.process\n{missing}");
    assert!(!check_errs("cmd-with-import", &present), "the imported form must type-check");
}

/// **P9**: a `run_output`'s `.stdout()`/`.stderr()` view is region-bound to `out`; returning it past
/// `out`'s `Drop` (the handle is a function local) is a compile error — the view would read freed
/// memory. (The `http response` body-view / `cli parsed` get_str precedent, #297.)
#[test]
fn run_output_view_cannot_escape_the_handle() {
    let leak = "import std.process\n\
fn leak() -> Result<str, Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"hi\"])\n\
  out := c.run()?\n\
  Ok(out.stdout())\n\
}\n\
pub fn main() -> i32 { 0 }\n";
    assert!(check_errs("run-output-view-escape", leak), "returning a stdout view past the run_output's Drop must error");
}

/// **P10**: `command` / `run_output` are Move handles bound to one local — never collected into an
/// array (an element read would copy + double-free the handle). Both are rejected as array elements.
#[test]
fn command_and_run_output_reject_array_elements() {
    let cmd_elem = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"hi\"])\n\
  xs := [c]\n\
  Ok(())\n\
}\n";
    assert!(check_errs("command-array-elem", cmd_elem), "a command as an array element must error");
    let out_elem = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"hi\"])\n\
  out := c.run()?\n\
  xs := [out]\n\
  Ok(())\n\
}\n";
    assert!(check_errs("run-output-array-elem", out_elem), "a run_output as an array element must error");
}

/// A temporary (unbound) command / run_output receiver is rejected — the handle is not dropped yet,
/// so its method must go through a bound local (the v1 Move-temporary gate).
#[test]
fn command_and_run_output_require_a_bound_receiver() {
    let temp_run = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  out := process.command(\"/bin/echo\", [\"/bin/echo\", \"hi\"]).run()?\n\
  print(out.code())\n\
  Ok(())\n\
}\n";
    assert!(check_errs("command-temp-run", temp_run), "run() on a temporary command must error (bind it first)");
}
