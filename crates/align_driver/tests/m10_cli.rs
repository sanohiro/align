//! M10 Slice 3 — std.cli. A flag-registration parser over `main(args: array<str>)`'s `array<str>`
//! (the one argv source). `cli.command(name)` builds a Move `cli command`; `c.flag_bool/str/i64(...)`
//! register flags; `c.parse(args)` **borrows** the command and yields `Result<parsed, Error>`;
//! `c.usage()` stays callable after parse (incl. the `Err` path); `p.get_bool/i64/str(name)` are
//! total after a successful parse and **abort** on an unregistered / wrong-kind name (never a silent
//! default). The completion condition: bool present/absent, str/i64 default+override, both
//! `--name value` and `--name=value` forms, `Error.Invalid` on unknown/missing/malformed, the
//! `get_*` abort, a `get_str` view that cannot escape `parsed` (`.clone()` copies out), and the
//! import + bound-receiver + array-element gates. (`docs/impl/std-design/cli.md`; `draft.md` §18.2.)

mod common;
use common::*;

/// A bool flag is `true` when its bare `--name` is present, `false` (its default) when absent; a str
/// and an i64 flag report their default when not overridden. One program exercises all three.
#[test]
fn defaults_when_absent() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.cli
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"demo\")
  c.flag_bool(\"verbose\")
  c.flag_str(\"name\", \"world\")
  c.flag_i64(\"count\", 3)
  p := c.parse(args)?
  print(p.get_bool(\"verbose\"))
  io.stdout.write(p.get_str(\"name\"))?
  io.stdout.write(\"\\n\")?
  print(p.get_i64(\"count\"))
  return Ok(())
}
";
    let out = build_and_run_args("m10-cli-defaults", prog, &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "false\nworld\n3\n");
}

/// The `--name value` (space) form overrides each flag: a bare bool flag, and a str / i64 value taken
/// from the next token.
#[test]
fn overrides_space_form() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.cli
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"demo\")
  c.flag_bool(\"verbose\")
  c.flag_str(\"name\", \"world\")
  c.flag_i64(\"count\", 3)
  p := c.parse(args)?
  print(p.get_bool(\"verbose\"))
  io.stdout.write(p.get_str(\"name\"))?
  io.stdout.write(\"\\n\")?
  print(p.get_i64(\"count\"))
  return Ok(())
}
";
    let out = build_and_run_args("m10-cli-space", prog, &["--verbose", "--name", "Align", "--count", "42"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\nAlign\n42\n");
}

/// The `--name=value` (equals) form overrides str / i64 flags identically to the space form.
#[test]
fn overrides_equals_form() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.cli
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"demo\")
  c.flag_str(\"name\", \"world\")
  c.flag_i64(\"count\", 3)
  p := c.parse(args)?
  io.stdout.write(p.get_str(\"name\"))?
  io.stdout.write(\"\\n\")?
  print(p.get_i64(\"count\"))
  return Ok(())
}
";
    let out = build_and_run_args("m10-cli-equals", prog, &["--name=Align", "--count=42"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Align\n42\n");
}

/// An unknown flag is `Error.Invalid` — parse fails, so the `?` propagates it out of `main`.
#[test]
fn unknown_flag_is_invalid() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"demo\")
  c.flag_bool(\"verbose\")
  p := c.parse(args)?
  print(p.get_bool(\"verbose\"))
  return Ok(())
}
";
    let out = build_and_run_args("m10-cli-unknown", prog, &["--bogus"]);
    assert_ne!(out.status.code(), Some(0), "an unknown flag must fail parse");
}

/// A str/i64 flag whose value token is missing (end of argv) is `Error.Invalid`.
#[test]
fn missing_value_is_invalid() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"demo\")
  c.flag_i64(\"count\", 3)
  p := c.parse(args)?
  print(p.get_i64(\"count\"))
  return Ok(())
}
";
    let out = build_and_run_args("m10-cli-missing", prog, &["--count"]);
    assert_ne!(out.status.code(), Some(0), "a missing value must fail parse");
}

/// A non-numeric i64 value is `Error.Invalid`.
#[test]
fn malformed_i64_is_invalid() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"demo\")
  c.flag_i64(\"count\", 3)
  p := c.parse(args)?
  print(p.get_i64(\"count\"))
  return Ok(())
}
";
    let out = build_and_run_args("m10-cli-malformed", prog, &["--count", "abc"]);
    assert_ne!(out.status.code(), Some(0), "a malformed i64 value must fail parse");
}

/// `c.usage()` renders every registered flag, and stays callable **after** a parse failure (proving
/// `parse` borrows the command, never consumes it — the whole reason help is printable on error).
#[test]
fn usage_renders_all_flags_after_parse_err() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.cli
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"tool\")
  c.flag_bool(\"verbose\")
  c.flag_str(\"name\", \"world\")
  c.flag_i64(\"count\", 3)
  match c.parse(args) {
    Ok(p) => { print(p.get_i64(\"count\")) }
    Err(_) => { io.stdout.write(c.usage())? }
  }
  return Ok(())
}
";
    // Force the Err path with an unknown flag; usage must still render on the borrowed command.
    let out = build_and_run_args("m10-cli-usage", prog, &["--bogus"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("usage: tool"), "usage header missing: {s}");
    assert!(s.contains("--verbose"), "verbose flag missing: {s}");
    assert!(s.contains("--name"), "name flag missing: {s}");
    assert!(s.contains("--count"), "count flag missing: {s}");
}

/// `get_*` on an unregistered name aborts at runtime (a programmer error — no comptime check is
/// possible, so it aborts like an OOB index; never a silent default / `Result`).
#[test]
fn get_unregistered_aborts() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"t\")
  c.flag_bool(\"verbose\")
  p := c.parse(args)?
  print(p.get_bool(\"nope\"))
  return Ok(())
}
";
    let out = build_and_run_args("m10-cli-get-unreg", prog, &[]);
    assert!(!out.status.success(), "get on an unregistered flag must abort");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no such flag was registered"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `get_*` against a wrong flag kind aborts (reading a bool flag as an i64).
#[test]
fn get_wrong_kind_aborts() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"t\")
  c.flag_bool(\"verbose\")
  p := c.parse(args)?
  print(p.get_i64(\"verbose\"))
  return Ok(())
}
";
    let out = build_and_run_args("m10-cli-get-wrong", prog, &[]);
    assert!(!out.status.success(), "get with a wrong kind must abort");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not an i64"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A `get_str` view can be cloned out of `parsed` and returned — the owned copy escapes fine.
#[test]
fn get_str_clone_escapes() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.cli
import std.io
fn chosen(args: array<str>) -> Result<string, Error> {
  c := cli.command(\"t\")
  c.flag_str(\"name\", \"world\")
  p := c.parse(args)?
  return Ok(p.get_str(\"name\").clone())
}
pub fn main(args: array<str>) -> Result<(), Error> {
  n := chosen(args)?
  io.stdout.write(n)?
  io.stdout.write(\"\\n\")?
  return Ok(())
}
";
    let out = build_and_run_args("m10-cli-clone", prog, &["--name", "Align"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Align\n");
}

/// A `cli command` is a Move handle: reassigning a `mut` binding drops the old command exactly once
/// (its slot is nulled on move), and the new one plus the parsed result drop once at frame exit — no
/// double-free (the Gate-1 UAF/double-free check for the two new Move types).
#[test]
fn command_reassign_no_double_free() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  mut c := cli.command(\"a\")
  c.flag_bool(\"x\")
  c = cli.command(\"b\")
  c.flag_bool(\"y\")
  p := c.parse(args)?
  print(p.get_bool(\"y\"))
  return Ok(())
}
";
    let out = build_and_run_args("m10-cli-reassign", prog, &["--y"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

// --- compile-error gates ----------------------------------------------------------------------

/// `cli.command` requires `import std.cli`.
#[test]
fn import_required() {
    let src = "\
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"t\")
  return Ok(())
}
";
    assert!(check_errs("m10-cli-noimport", src), "cli.* without `import std.cli` must error");
}

/// A `cli command` cannot be an array/collection element (a copied handle would double-free).
#[test]
fn command_as_array_element_rejected() {
    let src = "\
import std.cli
pub fn main() -> i32 {
  c := cli.command(\"a\")
  xs := [c, c]
  return 0
}
";
    assert!(check_errs("m10-cli-array-elem", src), "a cli command as an array element must be rejected");
}

/// An unbound owned-handle temporary cannot be a method receiver (bind it to a local first) — the
/// chained `cli.command(...).flag_bool(...)` and `c.parse(args)?.get_bool(...)` forms.
#[test]
fn unbound_temporary_receiver_rejected() {
    let chained = "\
import std.cli
pub fn main() -> i32 {
  cli.command(\"t\").flag_bool(\"v\")
  return 0
}
";
    assert!(check_errs("m10-cli-chain", chained), "a chained command-temporary method must be rejected");
    let getter = "\
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"t\")
  c.flag_bool(\"v\")
  print(c.parse(args)?.get_bool(\"v\"))
  return Ok(())
}
";
    assert!(check_errs("m10-cli-temp-get", getter), "a getter on a parsed temporary must be rejected");
}

/// A `get_str` view borrows `parsed`; returning it past `parsed`'s drop is a compile error (it is a
/// `Frame` view — the #297 escape is caught, and `.clone()` is the escape hatch tested above).
#[test]
fn get_str_view_escape_rejected() {
    let src = "\
import std.cli
fn leak(args: array<str>) -> str {
  c := cli.command(\"t\")
  c.flag_str(\"name\", \"x\")
  match c.parse(args) {
    Ok(p) => { return p.get_str(\"name\") }
    Err(_) => { return \"\" }
  }
}
pub fn main() -> i32 = 0
";
    assert!(check_errs("m10-cli-escape", src), "returning a get_str view must be a compile error");
}

/// A cli method **name** on a non-cli receiver is not intercepted by the cli dispatch — it falls
/// through to normal method resolution (`unknown method` on that type), so a future same-named method
/// on another type resolves normally (the cli dispatch is type-guarded, like `trim`/`map_err`). A
/// generic word like `parse` on a `str` must report against `str`, not claim a cli command method.
#[test]
fn cli_method_name_on_other_type_falls_through() {
    let src = "\
pub fn main() -> i32 {
  s := \"hello\"
  x := s.parse(\"y\")
  return 0
}
";
    let diags = check_diagnostics("m10-cli-fallthrough", src);
    assert!(diags.contains("unknown method '.parse()' on str"), "expected a normal 'unknown method' error, got: {diags}");
    assert!(!diags.contains("cli command"), "must not claim a cli command method on a str: {diags}");
    // A cli method name on an erroring receiver reports the receiver error once — no cascade from
    // the (removed) eager cli error re-checking the receiver.
    let casc = "\
pub fn main() -> i32 {
  x := nope.parse(\"y\")
  return 0
}
";
    let cd = check_diagnostics("m10-cli-nocascade", casc);
    assert_eq!(cd.matches("error:").count(), 1, "an undefined receiver must report exactly one error: {cd}");
}

/// A `flag_i64` default (and the runtime ABI) is exactly `i64`; a narrower width is a type error.
#[test]
fn flag_i64_default_must_be_i64() {
    let src = "\
import std.cli
pub fn main() -> i32 {
  c := cli.command(\"t\")
  d: i32 := 3
  c.flag_i64(\"count\", d)
  return 0
}
";
    assert!(check_errs("m10-cli-i32-default", src), "a non-i64 flag_i64 default must be a type error");
}
