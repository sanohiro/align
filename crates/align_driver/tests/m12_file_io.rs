//! M12 Slice A4 — `std.fs`/`std.io` offset-addressed file I/O: a new Move type `file` (an owned
//! read+write fd; `Drop` closes it), constructed by `fs.create_rw` (`O_RDWR|O_CREAT|O_TRUNC`) /
//! `fs.open_rw` (`O_RDWR`, must exist), with `f.pread(b: mut buffer, off)` / `f.pwrite(data, off)`
//! (loops to full; past-EOF extends) / `f.len()` (live fstat). **No cursor / no seek / no read-only
//! constructor.** Negative offset aborts. The headline drives create_rw → pwrite at offsets (incl. a
//! past-EOF hole) → pread back → and verifies the bytes end-to-end, plus the Move/consume guardrails
//! and the import gate. (`docs/impl/07-roadmap.md` M12 Slice A4; `draft.md` §18.2.)

mod common;
use common::*;

use std::path::PathBuf;

/// A temp path (not created yet), removed on scope exit (even on a panic).
struct TempFile {
    path: PathBuf,
}
impl TempFile {
    fn out(name: &str) -> TempFile {
        let path = std::env::temp_dir().join(format!("align-m12-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        TempFile { path }
    }
    fn with(name: &str, content: &[u8]) -> TempFile {
        let path = std::env::temp_dir().join(format!("align-m12-{}-{name}", std::process::id()));
        std::fs::write(&path, content).expect("seed temp file");
        TempFile { path }
    }
    fn str(&self) -> String {
        self.path.display().to_string()
    }
}
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The completion condition: `fs.create_rw` a fresh file, `pwrite` at explicit offsets (contiguous
/// then a hole past EOF), `pread` a region back, and verify both the read-back bytes (via stdout)
/// and the on-disk contents (via the Rust side). Exercises create_rw / pwrite (incl. past-EOF
/// extension) / pread / Drop-close, all on one bound `file`.
#[test]
fn create_pwrite_at_offsets_then_pread_back() {
    if !backend_available() {
        return;
    }
    let f = TempFile::out("rw");
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  f := fs.create_rw(args[1])?
  f.pwrite(\"Hello, \", 0)?
  f.pwrite(\"World!\", 7)?
  f.pwrite(\"Z\", 20)?
  buf := buffer(6)
  n := f.pread(buf, 7)?
  io.stdout.write(buf.bytes())?
  return Ok(())
}
";
    let out = build_and_run_args("m12-create-pwrite-pread", prog, &[&f.str()]);
    assert_eq!(out.status.code(), Some(0), "program exits 0; stderr: {}", String::from_utf8_lossy(&out.stderr));
    // pread returned the region at offset 7.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "World!");
    // On disk: "Hello, World!" (0..13), a zero hole (13..20), 'Z' at 20 — length 21.
    let mut expected = b"Hello, World!".to_vec();
    expected.extend_from_slice(&[0u8; 7]);
    expected.push(b'Z');
    assert_eq!(std::fs::read(&f.path).unwrap(), expected, "pwrite must land the bytes (with a past-EOF hole)");
}

/// `f.len()` is a **live** fstat (`Result<i64, Error>`), tracking the file's growth as `pwrite`
/// extends it — never a cached count.
#[test]
fn file_len_tracks_growth() {
    if !backend_available() {
        return;
    }
    let f = TempFile::out("len");
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  f := fs.create_rw(args[1])?
  f.pwrite(\"abcde\", 0)?
  print(f.len()?)
  f.pwrite(\"0123456789\", 5)?
  print(f.len()?)
  return Ok(())
}
";
    let out = build_and_run_args("m12-file-len", prog, &[&f.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n15\n");
}

/// `fs.open_rw` requires the file to exist (no create) — a missing path is `Err` (observed via
/// `match`); an existing file reopens O_RDWR for an in-place update without truncation.
#[test]
fn open_rw_missing_is_err_existing_updates_in_place() {
    if !backend_available() {
        return;
    }
    // Missing → Err.
    let missing = TempFile::out("missing");
    let prog_missing = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  match fs.open_rw(args[1]) {
    Ok(f) => { print(1) }
    Err(e) => { print(0) }
  }
  return Ok(())
}
";
    let out = build_and_run_args("m12-open-rw-missing", prog_missing, &[&missing.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n", "open_rw on a missing file is Err");

    // Existing → in-place region update (no truncate).
    let existing = TempFile::with("inplace", b"aaaaaa");
    let prog_update = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  f := fs.open_rw(args[1])?
  f.pwrite(\"XY\", 2)?
  return Ok(())
}
";
    let out = build_and_run_args("m12-open-rw-update", prog_update, &[&existing.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read(&existing.path).unwrap(), b"aaXYaa", "open_rw does not truncate; the region update lands");
}

/// A **negative** offset is a programmer bug — the runtime aborts (`SIGABRT` via `panic_abort`),
/// never a silent clamp / `Err`. An abort is a signal death, not a normal exit code.
#[test]
fn negative_offset_aborts() {
    if !backend_available() {
        return;
    }
    let f = TempFile::out("neg");
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  f := fs.create_rw(args[1])?
  f.pwrite(\"x\", -5)?
  return Ok(())
}
";
    let out = build_and_run_args("m12-neg-offset", prog, &[&f.str()]);
    assert_eq!(out.status.code(), None, "a negative offset must abort (signal death), not exit normally");
}

/// The Move/consume guardrails, all compile-time rejections (twin-mirror with reader/writer):
/// an unbound owned-file temporary as a method receiver, use-after-move, capture into `par_map`,
/// and `print`/`==` on a `file`.
#[test]
fn file_move_and_consume_gates_are_rejected() {
    // Unbound owned-file temporary as a method receiver (`fs.create_rw(p)?.pwrite(...)` leaks its fd).
    assert!(check_errs(
        "m12-file-temp-recv",
        "import std.fs\npub fn main(args: array<str>) -> Result<(), Error> {\n  fs.create_rw(args[1])?.pwrite(\"x\", 0)?\n  return Ok(())\n}\n",
    ), "a file method on an unbound fs.create_rw temporary must be rejected");
    // Use-after-move: moving the file into another binding then using the original.
    assert!(check_errs(
        "m12-file-use-after-move",
        "import std.fs\npub fn main(args: array<str>) -> Result<(), Error> {\n  f := fs.create_rw(args[1])?\n  g := f\n  f.pwrite(\"x\", 0)?\n  return Ok(())\n}\n",
    ), "using a file after it was moved must be rejected");
    // Capturing a Move file handle into a par_map closure is rejected (ty_capture_is_move / impurity).
    assert!(check_errs(
        "m12-file-par-map-capture",
        "import std.fs\npub fn main(args: array<str>) -> Result<(), Error> {\n  f := fs.create_rw(args[1])?\n  xs := [1, 2, 3]\n  ys := xs.par_map(|x| { x + (f.len()? as i32) })\n  return Ok(())\n}\n",
    ), "capturing a file into a par_map closure must be rejected");
    // `print` on a file (only scalars/strings are printable).
    assert!(check_errs(
        "m12-file-print",
        "import std.fs\npub fn main(args: array<str>) -> Result<(), Error> {\n  f := fs.create_rw(args[1])?\n  print(f)\n  return Ok(())\n}\n",
    ), "print on a file must be rejected");
    // `==` on files (structural equality is scalars/strings only).
    assert!(check_errs(
        "m12-file-eq",
        "import std.fs\npub fn main(args: array<str>) -> Result<(), Error> {\n  f := fs.create_rw(args[1])?\n  g := fs.create_rw(args[2])?\n  if f == g { print(1) }\n  return Ok(())\n}\n",
    ), "comparing files with == must be rejected");
}

/// The import gate: `fs.create_rw` / `fs.open_rw` require `import std.fs`.
#[test]
fn file_constructors_require_std_fs_import() {
    assert!(check_errs(
        "m12-no-import-create-rw",
        "pub fn main(args: array<str>) -> Result<(), Error> {\n  f := fs.create_rw(args[1])?\n  return Ok(())\n}\n",
    ), "fs.create_rw without `import std.fs` must be rejected");
    assert!(check_errs(
        "m12-no-import-open-rw",
        "pub fn main(args: array<str>) -> Result<(), Error> {\n  f := fs.open_rw(args[1])?\n  return Ok(())\n}\n",
    ), "fs.open_rw without `import std.fs` must be rejected");
    // With the import, a bound file's methods type-check (regression: the gates don't over-reach).
    assert!(!check_errs(
        "m12-file-bound-ok",
        "import std.fs\nimport std.io\npub fn main(args: array<str>) -> Result<(), Error> {\n  f := fs.create_rw(args[1])?\n  f.pwrite(\"hi\", 0)?\n  buf := buffer(2)\n  n := f.pread(buf, 0)?\n  print(f.len()?)\n  return Ok(())\n}\n",
    ), "a bound file's pwrite/pread/len must stay allowed");
}
