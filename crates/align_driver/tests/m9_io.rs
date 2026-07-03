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

/// An owned I/O handle (`reader`/`writer`/`buffer`) cannot be an array/slice element — an element
/// read copies the handle by value, so a collection of handles would alias one fd/buffer across
/// copies → double close/free (UB). Rejected at construction (array literal) and at the type
/// (`array<T>` / `slice<T>`), matching struct fields / tuple elements.
#[test]
fn io_handles_cannot_be_array_or_slice_elements() {
    // Array literal of readers.
    assert!(check_errs(
        "m9-arr-lit-reader",
        "import std.fs\npub fn main(args: array<str>) -> Result<(), Error> {\n  arr := [fs.open(args[1])?, fs.open(args[1])?]\n  return Ok(())\n}\n",
    ), "an array literal of readers must be rejected");
    // `array<reader>` type annotation.
    assert!(check_errs(
        "m9-arr-ty-reader",
        "import std.io\nfn f(xs: array<reader>) -> i32 = 0\npub fn main() -> i32 { return 0 }\n",
    ), "`array<reader>` must be rejected");
    // `slice<writer>` type annotation.
    assert!(check_errs(
        "m9-slice-ty-writer",
        "import std.io\nfn f(xs: slice<writer>) -> i32 = 0\npub fn main() -> i32 { return 0 }\n",
    ), "`slice<writer>` must be rejected");
    // `array<buffer>` type annotation (buffer was accidentally safe via `ty_to_scalar`; now
    // rejected explicitly for consistency).
    assert!(check_errs(
        "m9-arr-ty-buffer",
        "import std.io\nfn f(xs: array<buffer>) -> i32 = 0\npub fn main() -> i32 { return 0 }\n",
    ), "`array<buffer>` must be rejected");
}

/// v1 restriction (until Move temporaries drop): a `buffer` **method** call requires a bound
/// receiver too — `buffer(n).bytes()` on an unbound temporary returns a `slice<u8>` into storage
/// that is leaked-but-valid today, but a dangling slice (UAF) the moment Move temporaries drop.
/// Bind the buffer first.
#[test]
fn buffer_temporary_as_method_receiver_is_rejected() {
    assert!(check_errs(
        "m9-buffer-bytes-temp",
        "import std.io\npub fn main() -> Result<(), Error> {\n  io.stdout.write(buffer(4).bytes())?\n  return Ok(())\n}\n",
    ), "`.bytes()` on an unbound buffer temporary must be rejected");
    assert!(check_errs(
        "m9-buffer-len-temp",
        "pub fn main() -> i32 {\n  return buffer(4).len() as i32\n}\n",
    ), "`.len()` on an unbound buffer temporary must be rejected");
    // A bound buffer's methods still work (regression: the restriction doesn't over-reach).
    assert!(!check_errs(
        "m9-buffer-bound-ok",
        "import std.io\npub fn main() -> Result<(), Error> {\n  b := buffer(4)\n  io.stdout.write(b.bytes())?\n  print(b.len())\n  return Ok(())\n}\n",
    ), "a bound buffer's .bytes()/.len() must stay allowed");
}

/// v1 restriction (until Move temporaries drop): a reader/writer **method** call requires a bound
/// receiver — an unbound owned-handle temporary (`fs.create(p)?.write(d)?`) is never `Drop`ped, so
/// its buffered output would be lost / its fd leaked. The borrowed `io.std*` streams are exempt.
#[test]
fn owned_handle_temporary_as_method_receiver_is_rejected() {
    // `fs.create(p)?.write(...)?` — the writer temp would never flush.
    assert!(check_errs(
        "m9-writer-temp",
        "import std.fs\npub fn main(args: array<str>) -> Result<(), Error> {\n  fs.create(args[1])?.write(\"x\")?\n  return Ok(())\n}\n",
    ), "a writer method on an unbound fs.create temporary must be rejected");
    // `fs.open(p)?.read(buf)?` — the reader temp would leak its fd.
    assert!(check_errs(
        "m9-reader-temp",
        "import std.fs\npub fn main(args: array<str>) -> Result<(), Error> {\n  buf := buffer(4)\n  n := fs.open(args[1])?.read(buf)?\n  return Ok(())\n}\n",
    ), "a reader method on an unbound fs.open temporary must be rejected");
    // The borrowed std streams are exempt — `io.stdout.write(x)?` inline still type-checks (a
    // regression guard so the restriction doesn't over-reach).
    assert!(!check_errs(
        "m9-stdout-inline-ok",
        "import std.io\npub fn main() -> Result<(), Error> {\n  io.stdout.write(\"ok\\n\")?\n  return Ok(())\n}\n",
    ), "io.stdout.write inline must stay allowed (borrowed, no owned fd)");
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

// --- Slice 2: io.copy -------------------------------------------------------------------------
// `io.copy(r: reader, w: writer) -> Result<i64, Error>` — stream all of `r` into `w` through a
// fixed 64 KiB buffer (memory O(buffer), never O(file size)), returning the byte count. It
// **borrows** both handles (fd ownership does not move — like `print`'s argument), so `r`/`w`
// remain usable afterward. (`docs/impl/07-roadmap.md` M9 Slice 2; `draft.md` §18.2.)

/// The runtime transfer buffer size (`BUF_WRITER_CAP` in `align_runtime`) — the boundary the
/// byte-exact test straddles (below / at / above one buffer).
const IO_COPY_BUF: usize = 64 * 1024;

/// The headline: `io.copy` is byte-exact at, below, and above the transfer-buffer boundary (so the
/// copy loop runs zero, one, and many times over the final partial chunk). `w` is bound in `main`
/// and not consumed by `io.copy`, so it flushes/closes on scope-exit drop.
#[test]
fn io_copy_is_byte_exact_across_the_buffer_boundary() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  w := fs.create(args[2])?
  n := io.copy(r, w)?
  print(n)
  return Ok(())
}
";
    // A deterministic, non-repeating-ish payload at each size around the 64 KiB buffer.
    for &len in &[10usize, IO_COPY_BUF - 1, IO_COPY_BUF, IO_COPY_BUF + 123, 3 * IO_COPY_BUF] {
        let content: Vec<u8> = (0..len).map(|i| (i * 31 + 7) as u8).collect();
        let src = TempFile::new(&format!("iocopy-src-{len}"), &content);
        let dst = TempFile::out(&format!("iocopy-dst-{len}"));
        let out = build_and_run_args("m9-copy", prog, &[&src.str(), &dst.str()]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "io.copy of {len} bytes exits 0; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("{len}\n"),
            "io.copy returns the transferred byte count ({len})"
        );
        let copied = std::fs::read(&dst.path).expect("read the copied file");
        assert_eq!(copied.len(), len, "copied length matches for {len} bytes");
        assert_eq!(copied, content, "the {len}-byte copy is byte-exact");
    }
}

/// `io.copy` of an empty file transfers `0` bytes and produces an empty (but created) destination.
#[test]
fn io_copy_empty_file_transfers_zero() {
    if !backend_available() {
        return;
    }
    let src = TempFile::new("copy-empty-src", b"");
    let dst = TempFile::out("copy-empty-dst");
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  w := fs.create(args[2])?
  n := io.copy(r, w)?
  print(n)
  return Ok(())
}
";
    let out = build_and_run_args("m9-copy-empty", prog, &[&src.str(), &dst.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n", "an empty source transfers 0 bytes");
    assert_eq!(std::fs::read(&dst.path).expect("read dst"), b"", "the destination is empty");
}

/// Non-consumption: `io.copy` **borrows** both handles, so after the call `r` still reads (now at
/// EOF → `0`) and `w` still writes (a trailing byte appends after the copied bytes). The MoveCheck
/// must not treat `io.copy` as consuming its Move-typed arguments (like `print`).
#[test]
fn io_copy_does_not_consume_reader_or_writer() {
    if !backend_available() {
        return;
    }
    let content = b"copy me, then keep using both handles\n";
    let src = TempFile::new("copy-reuse-src", content);
    let dst = TempFile::out("copy-reuse-dst");
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  w := fs.create(args[2])?
  n := io.copy(r, w)?
  print(n)
  buf := buffer(8)
  m := r.read(buf)?
  print(m)
  w.write(\"!\")?
  return Ok(())
}
";
    let out = build_and_run_args("m9-copy-reuse", prog, &[&src.str(), &dst.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}\n0\n", content.len()),
        "copy returns the count; the reused reader then hits EOF (0)"
    );
    let mut expected = content.to_vec();
    expected.push(b'!');
    assert_eq!(
        std::fs::read(&dst.path).expect("read dst"),
        expected,
        "the reused writer appended after the copied bytes — proof it was not consumed"
    );
}

/// `io.copy(io.stdin, io.stdout)` — the classic `cat`. The borrowed std streams (no owned fd) are
/// valid, un-bound `io.copy` arguments; feed a payload on stdin and expect it verbatim on stdout.
#[test]
fn io_copy_stdin_to_stdout_is_cat() {
    if !backend_available() {
        return;
    }
    use std::io::Write;
    let prog = "\
import std.io
pub fn main() -> Result<(), Error> {
  n := io.copy(io.stdin, io.stdout)?
  return Ok(())
}
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "m9-cat", prog);
    assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sm, &checked.diags));
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let obj = dir.join(format!("align-m9-cat-{pid}.o"));
    let exe = dir.join(format!("align-m9-cat-{pid}{}", std::env::consts::EXE_SUFFIX));
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
    let payload = b"a cat program: bytes in == bytes out, unbuffered stdout\n0123456789";
    let mut child = std::process::Command::new(&exe)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().unwrap().write_all(payload).expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, payload, "cat echoes stdin to stdout verbatim");
}

/// O(buffer) guarantee: copying a large file (many times the transfer buffer) keeps the process's
/// peak resident memory bounded — it must not scale with the file size. Linux-only (reads
/// `/proc/<pid>/status` `VmHWM`). The child signals "copy done" on stdout, then blocks reading
/// stdin so the parent can sample its stable peak RSS before letting it exit — deterministic, no
/// sleeps. A copy that buffered the whole file would show `VmHWM` ≳ the file size.
#[test]
#[cfg(target_os = "linux")]
fn io_copy_rss_stays_bounded_on_large_input() {
    if !backend_available() {
        return;
    }
    use std::io::{BufRead, BufReader, Write};

    // 64 MiB — 1024× the transfer buffer, so an O(file) copy would be unmistakable in VmHWM.
    const FILE_LEN: usize = 64 * 1024 * 1024;
    let content: Vec<u8> = (0..FILE_LEN).map(|i| (i * 131 + 17) as u8).collect();
    let src = TempFile::new("copy-big-src", &content);
    let dst = TempFile::out("copy-big-dst");

    // Copy, flush the destination, print the count (the "done" handshake), then block on stdin so
    // the process stays alive at its peak RSS while the parent samples it.
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  w := fs.create(args[2])?
  n := io.copy(r, w)?
  w.flush()?
  print(n)
  sin := io.stdin
  b := buffer(4)
  k := sin.read(b)?
  return Ok(())
}
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "m9-copy-rss", prog);
    assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sm, &checked.diags));
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let obj = dir.join(format!("align-m9-copy-rss-{pid}.o"));
    let exe = dir.join(format!("align-m9-copy-rss-{pid}{}", std::env::consts::EXE_SUFFIX));
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
        .args([src.str(), dst.str()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    let child_pid = child.id();
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("child stdout"));

    // Block until the child reports the copy is done (it then blocks on stdin, alive at peak RSS).
    let mut done = String::new();
    lines.read_line(&mut done).expect("read done handshake");
    assert_eq!(done.trim(), FILE_LEN.to_string(), "child copied the whole file");

    // Sample the peak resident set from /proc while the child is blocked.
    let vm_hwm_kb = read_vm_hwm_kb(child_pid).expect("read VmHWM");
    // Release the child (close its stdin → its `sin.read` returns EOF → it exits).
    let _ = stdin.flush();
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0), "child exits cleanly after the handshake");

    // O(buffer), not O(file): peak RSS must be a small fraction of the 64 MiB file. 32 MiB is a
    // generous ceiling (the whole-file copy would be ≥ 64 MiB); the real figure is a few MiB.
    let ceiling_kb = 32 * 1024;
    assert!(
        vm_hwm_kb < ceiling_kb,
        "io.copy peak RSS {vm_hwm_kb} kB must stay bounded (< {ceiling_kb} kB) for a {} MiB file — \
         O(buffer), not O(file size)",
        FILE_LEN / (1024 * 1024)
    );

    // Byte-exact even at this size.
    assert_eq!(std::fs::read(&dst.path).expect("read big dst").len(), FILE_LEN, "big copy length matches");
}

/// Read `VmHWM` (peak resident set size, in kB) from `/proc/<pid>/status`. `None` if the field is
/// absent (e.g. the process already exited).
#[cfg(target_os = "linux")]
fn read_vm_hwm_kb(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            let kb = rest.split_whitespace().next()?;
            return kb.parse().ok();
        }
    }
    None
}

// --- Slice 2: io.copy ownership guardrails (compile-time) -------------------------------------

/// v1 restriction (until Move temporaries drop): an owned handle passed to `io.copy` must be a
/// bound local — an unbound temporary (`io.copy(fs.open(p)?, w)`) is never `Drop`ped, so its fd
/// leaks / its buffered output is lost. The borrowed `io.std*` streams are exempt.
#[test]
fn io_copy_temporary_handle_argument_is_rejected() {
    // A temporary reader argument.
    assert!(check_errs(
        "m9-copy-temp-reader",
        "import std.fs\nimport std.io\npub fn main(args: array<str>) -> Result<(), Error> {\n  w := fs.create(args[2])?\n  n := io.copy(fs.open(args[1])?, w)?\n  return Ok(())\n}\n",
    ), "a temporary reader argument to io.copy must be rejected");
    // A temporary writer argument.
    assert!(check_errs(
        "m9-copy-temp-writer",
        "import std.fs\nimport std.io\npub fn main(args: array<str>) -> Result<(), Error> {\n  r := fs.open(args[1])?\n  n := io.copy(r, fs.create(args[2])?)?\n  return Ok(())\n}\n",
    ), "a temporary writer argument to io.copy must be rejected");
    // Bound reader + writer: allowed (regression — the restriction doesn't over-reach).
    assert!(!check_errs(
        "m9-copy-bound-ok",
        "import std.fs\nimport std.io\npub fn main(args: array<str>) -> Result<(), Error> {\n  r := fs.open(args[1])?\n  w := fs.create(args[2])?\n  n := io.copy(r, w)?\n  return Ok(())\n}\n",
    ), "io.copy of two bound handles must be allowed");
    // Borrowed std streams: allowed (no owned fd — the `cat` form).
    assert!(!check_errs(
        "m9-copy-std-ok",
        "import std.io\npub fn main() -> Result<(), Error> {\n  n := io.copy(io.stdin, io.stdout)?\n  return Ok(())\n}\n",
    ), "io.copy(io.stdin, io.stdout) must be allowed");
}

/// `io.copy` returns `Result<i64, Error>`; discarding it (no `?` / `match` / bind) is the
/// unhandled-`Result` error — a copy failure must not be silently dropped.
#[test]
fn io_copy_discarding_the_result_is_rejected() {
    assert!(check_errs(
        "m9-copy-unhandled",
        "import std.io\npub fn main() -> Result<(), Error> {\n  io.copy(io.stdin, io.stdout)\n  return Ok(())\n}\n",
    ), "an unhandled io.copy Result must be rejected");
}

/// `io.copy` type-checks its arguments: a non-reader / non-writer is a clear error.
#[test]
fn io_copy_argument_types_are_checked() {
    assert!(check_errs(
        "m9-copy-bad-reader",
        "import std.io\npub fn main() -> Result<(), Error> {\n  n := io.copy(42, io.stdout)?\n  return Ok(())\n}\n",
    ), "a non-reader first argument must be rejected");
    assert!(check_errs(
        "m9-copy-bad-writer",
        "import std.io\npub fn main() -> Result<(), Error> {\n  n := io.copy(io.stdin, 42)?\n  return Ok(())\n}\n",
    ), "a non-writer second argument must be rejected");
    assert!(check_errs(
        "m9-copy-arity",
        "import std.io\npub fn main() -> Result<(), Error> {\n  n := io.copy(io.stdin)?\n  return Ok(())\n}\n",
    ), "the wrong argument count must be rejected");
}
