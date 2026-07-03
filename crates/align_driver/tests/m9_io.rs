//! M9 Slice 1 — std.io core: `reader` / `writer` (own an fd, `Drop` closes it) + `buffer` (an
//! owned growable byte sink), constructed by `fs.open` / `fs.create` / `io.stdin` / `io.stdout`,
//! with `r.read(b: mut buffer)` / `w.write(x)` / `w.flush()`. The headline is a byte-exact file
//! copy through a `read`/`write` loop, plus the errno→`Error` mapping and the ownership/region
//! guardrails. (`docs/impl/07-roadmap.md` M9 Slice 1; `draft.md` §18.2.)

mod common;
use common::*;

use std::path::PathBuf;

/// A temp file written with `content`, removed on scope exit (even on a panic).
struct TempFile {
    path: PathBuf,
}
impl TempFile {
    fn new(name: &str, content: &[u8]) -> TempFile {
        let path = std::env::temp_dir().join(format!("align-m9-{}-{name}", std::process::id()));
        std::fs::write(&path, content).expect("write temp file");
        TempFile { path }
    }
    /// A path for an output file (not created yet); still cleaned up on scope exit.
    fn out(name: &str) -> TempFile {
        let path = std::env::temp_dir().join(format!("align-m9-out-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&path);
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

/// The completion condition: a byte-exact file copy through `fs.open` → `reader.read(buf)` loop →
/// `fs.create` → `writer.write(buf.bytes())`. The buffer is deliberately tiny (4 bytes) so a
/// larger input drives many read/write iterations and the EOF (`read` returns 0) path — the loop
/// is expressed by tail recursion (Align has no loop keyword), threading the moved handles.
#[test]
fn file_copy_byte_exact_through_read_write_loop() {
    if !backend_available() {
        return;
    }
    // A payload that is not a multiple of the 4-byte window, so the last read is partial.
    let content = b"The quick brown fox jumps over the lazy dog.\n0123456789";
    let src = TempFile::new("copy-src", content);
    let dst = TempFile::out("copy-dst");

    let prog = "\
import std.fs
import std.io
fn copy_all(r: reader, w: writer, buf: buffer) -> Result<(), Error> {
  n := r.read(buf)?
  if n == 0 { return Ok(()) }
  w.write(buf.bytes())?
  return copy_all(r, w, buf)
}
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  w := fs.create(args[2])?
  buf := buffer(4)
  copy_all(r, w, buf)?
  return Ok(())
}
";
    let out = build_and_run_args("m9-file-copy", prog, &[&src.str(), &dst.str()]);
    assert_eq!(out.status.code(), Some(0), "copy program exits 0; stderr: {}", String::from_utf8_lossy(&out.stderr));
    let copied = std::fs::read(&dst.path).expect("read the copied file");
    assert_eq!(copied, content, "the copy is byte-exact");
}

/// A single large-window read fills the buffer with the whole (small) file; `w.flush()?` is
/// explicit (the writer is bound in `main`, not consumed). `buf.len()` reports the byte count.
#[test]
fn single_read_whole_file_then_explicit_flush() {
    if !backend_available() {
        return;
    }
    let content = b"hello, std.io\n";
    let src = TempFile::new("one-src", content);
    let dst = TempFile::out("one-dst");
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  w := fs.create(args[2])?
  buf := buffer(65536)
  n := r.read(buf)?
  print(n)
  w.write(buf.bytes())?
  w.flush()?
  return Ok(())
}
";
    let out = build_and_run_args("m9-single-read", prog, &[&src.str(), &dst.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{}\n", content.len()), "buf reports the read count");
    assert_eq!(std::fs::read(&dst.path).expect("read dst"), content);
}

/// `reader.read` on an empty file returns `0` (EOF) on the first read.
#[test]
fn read_empty_file_is_eof() {
    if !backend_available() {
        return;
    }
    let src = TempFile::new("empty-src", b"");
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  buf := buffer(16)
  n := r.read(buf)?
  print(n)
  return Ok(())
}
";
    let out = build_and_run_args("m9-eof", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n", "an empty file reads EOF (0)");
}

/// `fs.open` of a missing path maps `ENOENT` through the fixed table to `Error.NotFound` — the
/// program discriminates the category with `match` and returns a distinct code.
#[test]
fn open_missing_file_maps_to_not_found() {
    if !backend_available() {
        return;
    }
    // A path that reliably does not exist.
    let missing = std::env::temp_dir().join(format!("align-m9-absent-{}/nope", std::process::id()));
    // `?`-propagate the error to `main`'s boundary; the exit-code mapping sends a category to
    // `tag + 1`, so `Error.NotFound` (tag 0) exits 1 — proving `ENOENT` mapped to `NotFound`, not
    // the generic `Code`.
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  return Ok(())
}
";
    let out = build_and_run_args("m9-notfound", prog, &[&missing.display().to_string()]);
    assert_eq!(out.status.code(), Some(1), "a missing file is Error.NotFound (tag 0 -> exit 1)");
}

/// Opening a path whose directory denies access maps `EACCES` to `Error.Denied`. Skipped for root
/// (which bypasses permission checks) and where the sandbox can't create a 0-perm dir.
#[test]
fn open_permission_denied_maps_to_denied() {
    if !backend_available() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // root ignores permission bits — the mapping can't be exercised, so skip.
        let is_root = std::process::Command::new("id").arg("-u").output().ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim() == "0")
            .unwrap_or(false);
        if is_root {
            return;
        }
        let dir = std::env::temp_dir().join(format!("align-m9-denied-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let target = dir.join("secret");
        if std::fs::write(&target, b"x").is_err() {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        // Remove all access to the directory so opening a file inside it is EACCES.
        if std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).is_err() {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        // `Error.Denied` is tag 2 → the main boundary exits `tag + 1` = 3.
        let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  return Ok(())
}
";
        let out = build_and_run_args("m9-denied", prog, &[&target.display().to_string()]);
        // Restore permissions so the temp dir can be cleaned up.
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(out.status.code(), Some(3), "a permission-denied open is Error.Denied (tag 2 -> exit 3)");
    }
}

/// `io.stdin` is a `reader`: read a small window from stdin and echo the byte count, feeding the
/// child's stdin a known payload.
#[test]
fn stdin_reader_reads_from_fd_zero() {
    if !backend_available() {
        return;
    }
    use std::io::Write;
    let prog = "\
import std.io
pub fn main() -> Result<(), Error> {
  r := io.stdin
  buf := buffer(64)
  n := r.read(buf)?
  print(n)
  return Ok(())
}
";
    // Build the executable via the harness (it also runs once with empty stdin), then re-run with a
    // piped stdin. Simpler: compile+run through a dedicated spawn with stdin.
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "m9-stdin", prog);
    assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sm, &checked.diags));
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let obj = dir.join(format!("align-m9-stdin-{pid}.o"));
    let exe = dir.join(format!("align-m9-stdin-{pid}{}", std::env::consts::EXE_SUFFIX));
    struct Cleanup(Vec<PathBuf>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for p in &self.0 {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    let _g = Cleanup(vec![obj.clone(), exe.clone()]);
    emit_object_file(&mir, &obj, BuildTarget::Baseline).expect("codegen");
    link_executable(&obj, &exe, &mir.link_libs).expect("link");
    let mut child = std::process::Command::new(&exe)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().unwrap().write_all(b"hello").expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n", "stdin reader read the 5 piped bytes");
}

/// `Result<reader, Error>` carries the reader as an owned payload: `match` binds it in the `Ok`
/// arm (moving it out), reads through it, then the arm's end drops it (closes the fd). Exercises
/// the `Scalar::Reader` payload extraction + per-payload drop dtor.
#[test]
fn match_on_result_reader_binds_and_drops_the_handle() {
    if !backend_available() {
        return;
    }
    let src = TempFile::new("match-src", b"abcdefgh");
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  match fs.open(args[1]) {
    Ok(r) => {
      buf := buffer(4)
      n := r.read(buf)?
      print(n)
      return Ok(())
    }
    Err(e) => {
      print(99)
      return Ok(())
    }
  }
}
";
    let ok = build_and_run_args("m9-match-ok", prog, &[&src.str()]);
    assert_eq!(ok.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ok.stdout), "4\n", "Ok arm read 4 bytes through the bound reader");
    let missing = std::env::temp_dir().join(format!("align-m9-match-absent-{}", std::process::id()));
    let err = build_and_run_args("m9-match-err", prog, &[&missing.display().to_string()]);
    assert_eq!(err.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&err.stdout), "99\n", "Err arm ran");
}

// --- Ownership / region guardrails (compile-time) ---------------------------------------------

/// A `reader` is a Move handle: using it after it is moved into another function is rejected.
#[test]
fn reusing_a_moved_reader_is_rejected() {
    let prog = "\
import std.fs
import std.io
fn count(r: reader) -> Result<i64, Error> {
  buf := buffer(8)
  return r.read(buf)
}
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  a := count(r)?
  b := count(r)?
  return Ok(())
}
";
    assert!(check_errs("m9-moved-reader", prog), "using a reader after it is moved must be rejected");
}

/// `buf.bytes()` borrows the buffer's frame-local storage, so returning it (a `slice<u8>` that
/// would dangle once the buffer drops) is rejected — the same escape rule as a slice of a local
/// array. Region-tracked, buffer-owned; the caller must copy to escape.
#[test]
fn returning_buffer_bytes_is_rejected() {
    let prog = "\
import std.io
fn leak(buf: buffer) -> slice<u8> {
  return buf.bytes()
}
pub fn main() -> i32 {
  return 0
}
";
    assert!(check_errs("m9-bytes-escape", prog), "returning a slice into a local buffer must be rejected");
}

/// `w.write(x)` returns `Result<(), Error>`; discarding it (no `?` / `match` / bind) is the
/// unhandled-`Result` error — a write failure must not be silently dropped.
#[test]
fn discarding_a_writer_write_result_is_rejected() {
    let prog = "\
import std.io
pub fn main() -> Result<(), Error> {
  w := io.stdout.buffered()
  w.write(\"x\")
  return Ok(())
}
";
    assert!(check_errs("m9-write-unhandled", prog), "an unhandled writer.write Result must be rejected");
}

/// A `reader` is not region-tracked (it is an owned Move handle, like a `box`/`string`), so it may
/// be returned out of an `arena {}` — reading from a file has nothing to do with arena storage.
#[test]
fn reader_is_owned_not_region_tracked() {
    if !backend_available() {
        return;
    }
    let src = TempFile::new("region-src", b"abcdef");
    // Open the reader inside an arena and return the read count through the arena value — the
    // reader is owned (not arena-bound), so this type-checks and runs.
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  n := arena {
    r := fs.open(args[1])?
    buf := buffer(3)
    r.read(buf)?
  }
  print(n)
  return Ok(())
}
";
    let out = build_and_run_args("m9-region", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
}
