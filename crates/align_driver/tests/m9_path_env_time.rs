//! M9 Slice 4 — std.path / std.env / std.time. `path.join`/`base`/`dir`/`ext`/`normalize`
//! (pure lexical POSIX string ops; `base`/`dir`/`ext` are zero-copy `str` views of the input, so
//! their region is inherited from it), `env.get`/`env.set` (owned `Option<string>` / `Result`),
//! and `time.now`/`time.instant`/`time.sleep` (one `i64`-nanosecond timeline). The completion
//! condition: a round-trip per module — `path.join` then `dir`/`base`/`ext` recover the pieces;
//! `env.set` then `env.get` round-trips (and an unset name is `None`); `time.instant()` around a
//! `time.sleep(ns)` shows elapsed `ns` monotonically increasing. (`docs/impl/07-roadmap.md` M9
//! Slice 4; `draft.md` §18.2.)

mod common;
use common::*;

// --- path.join → dir/base/ext round-trip ------------------------------------------------------

/// The headline round-trip: `path.join` builds a path, then `dir`/`base`/`ext` recover its pieces.
#[test]
fn path_join_recover_round_trip() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.path
pub fn main() -> Result<(), Error> {
  j := path.join(\"dir/sub\", \"file.tar.gz\")
  print(j)
  print(path.dir(j))
  print(path.base(j))
  print(path.ext(j))
  return Ok(())
}
";
    let out = build_and_run("m9-path-roundtrip", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "dir/sub/file.tar.gz\ndir/sub\nfile.tar.gz\n.gz\n"
    );
}

/// `path.join` collapses the boundary separator (a trailing `/` on `a` and/or a leading `/` on `b`
/// fold to one), and an empty fragment yields the other verbatim.
#[test]
fn path_join_separator_collapse() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.path
pub fn main() -> Result<(), Error> {
  print(path.join(\"a/\", \"/b\"))
  print(path.join(\"a\", \"b\"))
  print(path.join(\"\", \"b\"))
  print(path.join(\"a\", \"\"))
  print(path.join(\"/\", \"b\"))
  return Ok(())
}
";
    let out = build_and_run("m9-path-join-collapse", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a/b\na/b\nb\na\n/b\n");
}

// --- path.normalize ---------------------------------------------------------------------------

/// `path.normalize` lexically resolves `.` / `..` / redundant `/`, including the edge cases:
/// interior `..`, a leading `..` on a relative path (preserved), a `..` past the root on an
/// absolute path (dropped), the empty path (`.`), and the root (`/`).
#[test]
fn path_normalize_cases() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.path
pub fn main() -> Result<(), Error> {
  print(path.normalize(\"a/./b/../c\"))
  print(path.normalize(\"../a\"))
  print(path.normalize(\"/../a\"))
  print(path.normalize(\"a//b\"))
  print(path.normalize(\"\"))
  print(path.normalize(\"/\"))
  print(path.normalize(\"a/b/../..\"))
  print(path.normalize(\"/usr/./local/../bin\"))
  return Ok(())
}
";
    let out = build_and_run("m9-path-normalize", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "a/c\n../a\n/a\na/b\n.\n/\n.\n/usr/bin\n"
    );
}

// --- base / dir / ext edge cases --------------------------------------------------------------

/// `base`/`dir`/`ext` on the tricky, view-safe edges: a trailing slash, the root, a dotfile (no
/// ext), and a path with no separator (`dir` is the **empty** view, printed as a blank line).
#[test]
fn path_base_dir_ext_edges() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.path
pub fn main() -> Result<(), Error> {
  print(path.base(\"/usr/bin/\"))
  print(path.dir(\"/usr/bin/ls\"))
  print(path.dir(\"file\"))
  print(path.base(\"/\"))
  print(path.ext(\".bashrc\"))
  print(path.ext(\"noext\"))
  return Ok(())
}
";
    let out = build_and_run("m9-path-edges", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // bin / /usr/bin / (empty) / / / (empty) / (empty)
    assert_eq!(String::from_utf8_lossy(&out.stdout), "bin\n/usr/bin\n\n/\n\n\n");
}

/// `path.base`/`dir`/`ext` return a zero-copy `str` **view** of the input, so the result's region
/// is inherited from the input (the #297-class rule). A view of an arena-bound `str`
/// (`fs.read_file_view`) must not escape the arena — a compile error (were the region mis-inferred
/// as `Static`, this would compile and dangle after `munmap`).
#[test]
fn path_view_region_bound_to_input() {
    let prog = "\
import std.fs
import std.path
fn load(p: str) -> Result<str, Error> {
  arena {
    v := fs.read_file_view(p)?
    return Ok(path.base(v))
  }
}
pub fn main() -> Result<(), Error> {
  return Ok(())
}
";
    assert!(
        check_errs("m9-path-view-escape", prog),
        "a path view of an arena str must not escape the arena (region inherited from the input)"
    );
}

// --- env.set / env.get round-trip -------------------------------------------------------------

/// `env.set` then `env.get` round-trips the value; an unset name yields `None`.
#[test]
fn env_set_get_round_trip() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.env
pub fn main() -> Result<(), Error> {
  env.set(\"ALIGN_M9_SLICE4\", \"value-123\")?
  match env.get(\"ALIGN_M9_SLICE4\") {
    Some(v) => print(v),
    None => print(\"none\"),
  }
  match env.get(\"ALIGN_M9_DEFINITELY_UNSET_ZZZ\") {
    Some(v) => print(v),
    None => print(\"none\"),
  }
  return Ok(())
}
";
    let out = build_and_run("m9-env-roundtrip", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "value-123\nnone\n");
}

/// `env.set` with an invalid name (containing `=`) is `Error.Invalid` (tag 1 → main exits 1).
#[test]
fn env_set_invalid_name_rejected() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.env
pub fn main() -> Result<(), Error> {
  env.set(\"BAD=NAME\", \"x\")?
  return Ok(())
}
";
    let out = build_and_run("m9-env-invalid", prog);
    // Error.Invalid (tag 1) propagates out of `main` as a nonzero exit (the runtime reports the
    // Err and exits with a nonzero code — NotFound/tag 0 → 1, Invalid/tag 1 → 2, etc.).
    assert_eq!(out.status.code(), Some(2), "an invalid env name is Error.Invalid");
}

// --- time.now / instant / sleep ---------------------------------------------------------------

/// `time.instant()` is monotonic: a reading after `time.sleep(ns)` is at least `ns` greater than a
/// reading before it. `time.now()` returns a positive wall-clock nanosecond count.
#[test]
fn time_instant_monotonic_across_sleep() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.time
pub fn main() -> Result<(), Error> {
  t0 := time.instant()
  time.sleep(5000000)
  t1 := time.instant()
  if t1 - t0 >= 5000000 { print(1) } else { print(0) }
  if time.now() > 0 { print(1) } else { print(0) }
  return Ok(())
}
";
    let out = build_and_run("m9-time-monotonic", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n1\n");
}

/// `time.sleep(ns)` with a non-positive `ns` is a no-op (it returns immediately, no hang).
#[test]
fn time_sleep_negative_is_noop() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.time
pub fn main() -> Result<(), Error> {
  time.sleep(-1)
  time.sleep(0)
  print(1)
  return Ok(())
}
";
    let out = build_and_run("m9-time-sleep-noop", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
}

// --- import required --------------------------------------------------------------------------

/// Each module builtin requires its `import` — using `path.join` without `import std.path` is an
/// error (the `core.json` / std.fs precedent).
#[test]
fn path_requires_import() {
    let prog = "\
pub fn main() -> Result<(), Error> {
  print(path.join(\"a\", \"b\"))
  return Ok(())
}
";
    assert!(check_errs("m9-path-no-import", prog), "path.join without `import std.path` is an error");
}

// --- shadowing: a value named like the module takes precedence over the builtin ----------------

/// A parameter (or local) named `path`/`env`/`time`/`fs` **shadows** the std module, so
/// `path.base(...)` on such a value routes to normal value-method resolution — never silently
/// swallowing the receiver into the builtin. `path` is a classic parameter name, so this matters:
/// the diagnostic must be "unknown method on <type>" (proving value dispatch), not a spurious
/// `import`/argument error from the builtin. (PR #340 fallback-review CONFIRMED finding.)
#[test]
fn path_receiver_shadows_module() {
    // `path` is a `str` parameter; `path.base(...)` must resolve as a method on that `str`.
    let prog = "\
import std.path
fn f(path: str) -> str {
  return path.base(\"ignored\")
}
pub fn main() -> Result<(), Error> {
  return Ok(())
}
";
    let d = check_diagnostics("m9-path-shadow", prog);
    assert!(d.contains("unknown method '.base()' on str"), "shadowed `path` routes to value dispatch, not the builtin; got: {d}");
}

#[test]
fn env_receiver_shadows_module() {
    let prog = "\
import std.env
fn f(env: str) -> bool {
  return env.get(\"x\")
}
pub fn main() -> Result<(), Error> {
  return Ok(())
}
";
    let d = check_diagnostics("m9-env-shadow", prog);
    // `.get()` is also the `box<T>`/`Task<R>` method, so value dispatch resolves it there and
    // reports the receiver type (`str`) — the point is it routed to value dispatch (the `env`
    // receiver was not swallowed into the `env.get` builtin), never a spurious `import std.env`.
    assert!(d.contains("got str"), "shadowed `env` routes to value dispatch (receiver kept); got: {d}");
    assert!(!d.contains("import std.env"), "no spurious import diagnostic when `env` is shadowed; got: {d}");
}

#[test]
fn time_receiver_shadows_module() {
    // A shadowing `time` local (an i64) — `time.sleep(...)` must report "no method on int",
    // not a spurious `import std.time` error (finding (b)).
    let prog = "\
pub fn main() -> Result<(), Error> {
  time := 5
  time.sleep(10)
  return Ok(())
}
";
    let d = check_diagnostics("m9-time-shadow", prog);
    assert!(d.contains("unknown method '.sleep()'"), "shadowed `time` routes to value dispatch; got: {d}");
    assert!(!d.contains("import std.time"), "no spurious import diagnostic when `time` is shadowed; got: {d}");
}

#[test]
fn fs_receiver_shadows_module() {
    let prog = "\
import std.fs
fn f(fs: str) -> bool {
  return fs.exists(\"ignored\")
}
pub fn main() -> Result<(), Error> {
  return Ok(())
}
";
    let d = check_diagnostics("m9-fs-shadow", prog);
    assert!(d.contains("unknown method '.exists()' on str"), "shadowed `fs` routes to value dispatch, not the builtin; got: {d}");
}
