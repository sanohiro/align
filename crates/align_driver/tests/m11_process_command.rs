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

#[test]
fn command_capture_bound_formation_and_state() {
    let bad_type = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\"])\n\
  c.max_capture_bytes(\"four\")\n\
  Ok(())\n\
}\n";
    assert!(check_errs("command-capture-bound-type", bad_type));
    let temporary = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  process.command(\"/bin/echo\", [\"/bin/echo\"]).max_capture_bytes(4)\n\
  Ok(())\n\
}\n";
    assert!(check_errs("command-capture-bound-temporary", temporary));
    let wrong_arity = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\"])\n\
  c.max_capture_bytes()\n\
  Ok(())\n\
}\n";
    assert!(check_errs("command-capture-bound-arity", wrong_arity));
    let temporary_run = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  out := process.command(\"/bin/echo\", [\"/bin/echo\"]).run_bytes()?\n\
  print(out.code())\n\
  Ok(())\n\
}\n";
    assert!(check_errs("command-run-bytes-temporary", temporary_run));
}

#[test]
fn command_capture_exact_limit_and_reuse() {
    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let src = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  empty := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \":\"])\n\
  empty.max_capture_bytes(0)\n\
  empty_out := empty.run()?\n\
  print(empty_out.stdout().len() + empty_out.stderr().len())\n\
  stdout := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf HELLO\"])\n\
  stdout.max_capture_bytes(5)\n\
  stdout_out := stdout.run()?\n\
  print(stdout_out.stdout())\n\
  stderr := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf WORLD 1>&2\"])\n\
  stderr.max_capture_bytes(5)\n\
  stderr_out := stderr.run()?\n\
  print(stderr_out.stderr())\n\
  both := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf HELLO; printf WORLD 1>&2\"])\n\
  both.max_capture_bytes(4)\n\
  both.max_capture_bytes(5)\n\
  first := both.run()?\n\
  second := both.run()?\n\
  print(first.stdout())\n\
  print(first.stderr())\n\
  print(second.stdout().len() + second.stderr().len())\n\
  Ok(())\n\
}\n";
    let out = build_and_run("command-capture-exact", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\nHELLO\nWORLD\nHELLO\nWORLD\n10\n");
}

#[test]
fn command_capture_overflow_discards_partial() {
    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let src = "import std.process\n\
pub fn main() -> i32 {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf HELLO\"])\n\
  c.max_capture_bytes(4)\n\
  match c.run() {\n\
    Ok(_) => 1,\n\
    Err(e) => match e { Invalid => 42, _ => 2 },\n\
  }\n\
}\n";
    let out = build_and_run("command-capture-overflow", src);
    assert_eq!(out.status.code(), Some(42), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn command_run_bytes_preserves_arbitrary_output() {
    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let src = "import std.process\n\
import std.encoding\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf '\\\\377\\\\000A'\"])\n\
  c.max_capture_bytes(3)\n\
  out := c.run_bytes()?\n\
  print(out.code())\n\
  print(encoding.hex_encode(out.stdout()))\n\
  Ok(())\n\
}\n";
    let out = build_and_run("command-run-bytes", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\nff0041\n");
}

#[test]
fn command_timeout_covers_post_eof_wait() {
    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let src = "import std.process\n\
pub fn main() -> i32 {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"exec 1>&- 2>&-; sleep 10\"])\n\
  c.timeout_ns(100_000_000)\n\
  match c.run() {\n\
    Ok(_) => 1,\n\
    Err(e) => match e { Timeout => 42, _ => 2 },\n\
  }\n\
}\n";
    let out = build_and_run("command-timeout-post-eof", src);
    assert_eq!(out.status.code(), Some(42), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn run_bytes_view_cannot_escape_the_handle() {
    let leak = "import std.process\n\
fn leak() -> Result<slice<u8>, Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"hi\"])\n\
  out := c.run_bytes()?\n\
  Ok(out.stdout())\n\
}\n\
pub fn main() -> i32 { 0 }\n";
    assert!(check_errs("run-bytes-view-escape", leak));

    let borrowed = "import std.process\n\
fn stdout(borrow out: run_bytes) -> slice<u8> = out.stdout()\n\
fn stderr(borrow mut out: run_bytes) -> slice<u8> = out.stderr()\n\
pub fn main() -> i32 { 0 }\n";
    assert!(
        !check_errs("run-bytes-borrowed-view-return", borrowed),
        "views of caller-owned borrowed run_bytes parameters must remain returnable"
    );

    let by_value = "import std.process\n\
fn stdout(out: run_bytes) -> slice<u8> = out.stdout()\n\
pub fn main() -> i32 { 0 }\n";
    assert!(
        check_errs("run-bytes-by-value-view-return", by_value),
        "a by-value run_bytes parameter is dropped by the callee, so its view must not escape"
    );
}

#[test]
fn command_capture_overflow_kills_group_and_discards_partial() {
    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let marker = std::env::temp_dir().join(format!(
        "align-command-overflow-marker-{}-{}",
        std::process::id(),
        thin_nonce()
    ));
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        "(sleep 0.2; printf alive > '{}') & head -c 65537 /dev/zero & head -c 65537 /dev/zero 1>&2 & wait",
        marker.display()
    );
    let src = format!(
        "import std.process\n\
pub fn main() -> i32 {{\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"{script}\"])\n\
  c.max_capture_bytes(65536)\n\
  match c.run() {{\n\
    Ok(_) => 1,\n\
    Err(e) => match e {{ Invalid => 42, _ => 2 }},\n\
  }}\n\
}}\n"
    );
    let out = build_and_run("command-capture-overflow-group", &src);
    assert_eq!(out.status.code(), Some(42), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!marker.exists(), "the overflow path must kill the owned descendant group");
}

#[test]
fn command_capture_error_precedence() {
    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let cases = [
        ("sleep 0.1; printf 12345", 4, 20_000_000, "Timeout", 42),
        ("printf 12345; sleep 0.1", 4, 1_000_000_000, "Invalid", 43),
        ("printf 12345; exit 7", 4, 0, "Invalid", 43),
        ("printf '\\\\377'", 4, 0, "Invalid", 43),
    ];
    for (index, (script, limit, timeout, expected, code)) in cases.into_iter().enumerate() {
        let timeout_setter = if timeout == 0 {
            String::new()
        } else {
            format!("  c.timeout_ns({timeout})\n")
        };
        let src = format!(
            "import std.process\n\
pub fn main() -> i32 {{\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"{script}\"])\n\
  c.max_capture_bytes({limit})\n\
{timeout_setter}\
  match c.run() {{\n\
    Ok(_) => 1,\n\
    Err(e) => match e {{ {expected} => {code}, _ => 2 }},\n\
  }}\n\
}}\n"
        );
        let out = build_and_run(&format!("command-capture-precedence-{index}"), &src);
        assert_eq!(out.status.code(), Some(code), "case {index}; stderr: {}", String::from_utf8_lossy(&out.stderr));
    }
}

#[test]
fn command_run_bytes_nonzero_exit_and_both_streams() {
    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let src = "import std.process\n\
import std.encoding\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf '\\\\377\\\\000A'; printf '\\\\000E' 1>&2; exit 7\"])\n\
  c.max_capture_bytes(3)\n\
  out := c.run_bytes()?\n\
  print(out.code())\n\
  print(encoding.hex_encode(out.stdout()))\n\
  print(encoding.hex_encode(out.stderr()))\n\
  Ok(())\n\
}\n";
    let out = build_and_run("command-run-bytes-nonzero", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\nff0041\n0045\n");
}

#[test]
fn run_bytes_type_classification_tripwire() {
    for (name, declaration) in [
        (
            "run-bytes-struct-field",
            "import std.process\nHolder { out: run_bytes }\npub fn main() -> i32 { 0 }\n",
        ),
        (
            "run-bytes-option",
            "import std.process\nfn bad() -> Option<run_bytes> = None\npub fn main() -> i32 { 0 }\n",
        ),
        (
            "run-bytes-error-payload",
            "import std.process\nfn bad() -> Result<i64, run_bytes> = Ok(0)\npub fn main() -> i32 { 0 }\n",
        ),
        (
            "run-bytes-nested-result",
            "import std.process\nfn bad() -> Result<Result<run_bytes, Error>, Error> = Err(Error.Invalid)\npub fn main() -> i32 { 0 }\n",
        ),
        (
            "run-bytes-bare-return",
            "import std.process\nfn bad(out: run_bytes) -> run_bytes = out\npub fn main() -> i32 { 0 }\n",
        ),
    ] {
        assert!(check_errs(name, declaration), "{name} must stay outside the closed run_bytes carrier set");
    }
    let aggregate = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"x\"])\n\
  out := c.run_bytes()?\n\
  xs := [out]\n\
  Ok(())\n\
}\n";
    assert!(check_errs("run-bytes-array", aggregate));
    let capture = "import std.process\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"x\"])\n\
  out := c.run_bytes()?\n\
  f := fn unused: i64 { out.code() }\n\
  print(f(0))\n\
  Ok(())\n\
}\n";
    assert!(check_errs("run-bytes-capture", capture));
    let generic_return = "import std.process\n\
fn identity<T>(value: T) -> T = value\n\
pub fn main() -> Result<(), Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"x\"])\n\
  out := c.run_bytes()?\n\
  returned := identity(out)\n\
  print(returned.code())\n\
  Ok(())\n\
}\n";
    let diagnostics = check_diagnostics("run-bytes-generic-bare-return", generic_return);
    assert!(
        diagnostics.contains("generic substitution cannot produce a bare run_bytes return"),
        "generic substitution must fail at the closed return-carrier gate:\n{diagnostics}"
    );
    let return_move = "import std.process\n\
fn capture() -> Result<run_bytes, Error> {\n\
  c := process.command(\"/bin/echo\", [\"/bin/echo\", \"x\"])\n\
  c.run_bytes()\n\
}\n\
pub fn main() -> i32 { 0 }\n";
    let diagnostics = check_diagnostics("run-bytes-return", return_move);
    assert!(diagnostics.is_empty(), "a run_bytes Result may return by move:\n{diagnostics}");

    let moved_source = "import std.process\n\
fn make() -> Result<run_bytes, Error> {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \":\"])\n\
  c.run_bytes()\n\
}\n\
pub fn main() -> Result<(), Error> {\n\
  first := make()?\n\
  second := first\n\
  print(first.code())\n\
  print(second.code())\n\
  Ok(())\n\
}\n";
    assert!(check_errs("run-bytes-moved-source", moved_source), "moving a run_bytes local must null and invalidate its source");

    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let ownership_paths = "import std.process\n\
fn make() -> Result<run_bytes, Error> {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \":\"])\n\
  c.max_capture_bytes(0)\n\
  c.run_bytes()\n\
}\n\
fn keep_error(e: Error) -> Error = e\n\
fn through_try() -> Result<i64, Error> {\n\
  result := make()\n\
  out := result?\n\
  Ok(out.code())\n\
}\n\
fn through_else() -> i64 {\n\
  result := make()\n\
  out := result else { return 90 }\n\
  out.code()\n\
}\n\
fn through_match() -> i64 = match make() {\n\
  Ok(out) => out.code(),\n\
  Err(_) => 91,\n\
}\n\
fn through_map_err() -> Result<i64, Error> {\n\
  out := make().map_err(keep_error)?\n\
  Ok(out.code())\n\
}\n\
fn through_replacement() -> Result<i64, Error> {\n\
  mut out := make()?\n\
  out = make()?\n\
  Ok(out.code())\n\
}\n\
fn through_return() -> Result<run_bytes, Error> = make()\n\
fn through_early_exit() -> Result<i64, Error> {\n\
  out := make()?\n\
  if out.code() == 0 { return Ok(1) }\n\
  Ok(92)\n\
}\n\
pub fn main() -> Result<(), Error> {\n\
  returned := through_return()?\n\
  total := through_try()? + through_else() + through_match() + through_map_err()? + through_replacement()? + returned.code() + through_early_exit()?\n\
  print(total)\n\
  Ok(())\n\
}\n";
    let out = build_and_run("run-bytes-ownership-matrix", ownership_paths);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
}

#[test]
fn command_capture_negative_bound_aborts_before_child() {
    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let marker = std::env::temp_dir().join(format!(
        "align-command-negative-marker-{}-{}",
        std::process::id(),
        thin_nonce()
    ));
    let _ = std::fs::remove_file(&marker);
    let src = format!(
        "import std.process\n\
pub fn main() -> i32 {{\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf child > '{}'\"])\n\
  c.max_capture_bytes(-1)\n\
  match c.run() {{ Ok(_) => 1, Err(_) => 2 }}\n\
}}\n",
        marker.display()
    );
    let out = build_and_run("command-capture-negative", &src);
    assert!(!out.status.success(), "a negative bound is an abort-class programmer error");
    assert!(!marker.exists(), "negative setter must abort before the child starts");
}

#[test]
fn command_run_bytes_matches_whole_program_and_per_unit_abi() {
    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let files = &[
        (
            "capture.align",
            r#"
module capture
import std.process
pub fn run() -> Result<run_bytes, Error> {
  c := process.command("/bin/sh", ["/bin/sh", "-c", "printf imported; exit 6"])
  c.max_capture_bytes(8)
  c.run_bytes()
}
"#,
        ),
        (
            "main.align",
            r#"
import capture
fn main() -> i32 {
  match capture.run() {
    Ok(out) => if out.stdout().len() == 8 { out.code() as i32 } else { 90 },
    Err(_) => 91,
  }
}
"#,
        ),
    ];
    let whole = build_and_run_multi("command-run-bytes-whole", files, "main.align");
    let per_unit = build_per_unit_multi("command-run-bytes-per-unit", files, "main.align");
    assert_eq!(whole.status.code(), Some(6), "stderr: {}", String::from_utf8_lossy(&whole.stderr));
    let linked = per_unit.link_and_run();
    assert_eq!(linked.status.code(), Some(6), "stderr: {}", String::from_utf8_lossy(&linked.stderr));
}

#[test]
fn command_run_mode_edit_and_revert_has_exact_cache_identity() {
    if !backend_available() || !std::path::Path::new("/bin/sh").exists() {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "align-command-cache-{}-{}",
        std::process::id(),
        thin_nonce()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(dir.clone());
    let source_path = dir.join("main.align");
    let cache = dir.join("cache");
    let text = "import std.process\n\
fn main() -> i32 {\n\
  c := process.command(\"/bin/sh\", [\"/bin/sh\", \"-c\", \"printf cache\"])\n\
  c.max_capture_bytes(5)\n\
  out := c.run() else { return 90 }\n\
  out.stdout().len() as i32\n\
}\n";
    let bytes = text.replace("c.run()", "c.run_bytes()");
    let build = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
            .args(["build", "main.align", "--cache-stats"])
            .current_dir(&dir)
            .env("ALIGNC_CACHE", &cache)
            .output()
            .expect("run alignc")
    };

    std::fs::write(&source_path, text).unwrap();
    let cold = build();
    assert!(cold.status.success());
    assert!(String::from_utf8_lossy(&cold.stderr).contains("0 hit, 1 miss"));

    std::fs::write(&source_path, bytes).unwrap();
    let edited = build();
    assert!(edited.status.success());
    assert!(String::from_utf8_lossy(&edited.stderr).contains("0 hit, 1 miss"));

    std::fs::write(&source_path, text).unwrap();
    let reverted = build();
    assert!(reverted.status.success());
    assert!(
        String::from_utf8_lossy(&reverted.stderr).contains("1 hit, 0 miss"),
        "an exact source revert must select the original cached object: {}",
        String::from_utf8_lossy(&reverted.stderr)
    );
}
