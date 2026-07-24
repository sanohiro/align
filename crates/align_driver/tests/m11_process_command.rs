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
