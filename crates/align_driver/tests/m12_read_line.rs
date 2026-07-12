//! M12 Slice A7 — streaming line reads: the buffered `reader` (`r.buffered()`), `r.read_line(b: mut
//! buffer)` (terminator stripped, buffer grows, `0` = EOF), and the generic bytes→text VIEW
//! `bytes.as_str()` (validating, region-bound). The headline is the canonical per-line decode loop
//! `loop { n := r.read_line(buf)?; if n == 0 { break }; line := buf.bytes().as_str()?; … }`, plus
//! the documented edges (empty lines, CRLF, a lone `\r`, a final unterminated line, the interleaving
//! contract, the sema gate, and the escape/UTF-8 guards). (`docs/impl/07-roadmap.md` M12 Slice A7;
//! `draft.md` §18.2.)

mod common;
use common::*;

use std::path::PathBuf;

/// A temp file written with `content`, removed on scope exit (even on a panic).
struct TempFile {
    path: PathBuf,
}
impl TempFile {
    fn new(name: &str, content: &[u8]) -> TempFile {
        let path = std::env::temp_dir().join(format!("align-a7-{}-{name}", std::process::id()));
        std::fs::write(&path, content).expect("write temp file");
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

/// The canonical loop: open → `.buffered()` → `read_line` until EOF, validating each line with
/// `.as_str()` and accumulating a `.clone()` into an `array_builder<string>`. Emits `[body]\n` per
/// line to stdout for a content check, then the accumulated count. Exercises the whole documented
/// edge table in one file: an ordinary line, an empty line (`\n`), a CRLF line (`\r?\n` stripped),
/// and a final **unterminated** line — plus that empty lines round-trip as empty `str`s.
#[test]
fn canonical_loop_lines_edges_and_count() {
    if !backend_available() {
        return;
    }
    // "alpha\n" | "\n" (empty) | "beta\r\n" (CRLF) | "gamma" (final, unterminated).
    let content = b"alpha\n\nbeta\r\ngamma";
    let src = TempFile::new("canon", content);
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  base := fs.open(args[1])?
  r := base.buffered()
  buf := buffer(8)
  w := io.stdout
  mut acc: array_builder<string> := array_builder()
  loop {
    n := r.read_line(buf)?
    if n == 0 { break }
    line := buf.bytes().as_str()?
    w.write(\"[\")?
    w.write(line)?
    w.write(\"]\\n\")?
    acc.push(line.clone())
  }
  xs := acc.build()
  print(xs.len())
  return Ok(())
}
";
    let out = build_and_run_args("a7-canon", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "[alpha]\n[]\n[beta]\n[gamma]\n4\n",
        "each line body, terminator stripped; empty line round-trips empty; CRLF stripped; count = 4",
    );
}

/// The interleaving contract: after a `read_line` drains the first line, a following `r.read(buf)`
/// must return the **retained surplus** lookahead bytes (everything after the first `\n`), never
/// fd-fresh bytes. Prints the first line body then the surplus the `read` produced.
#[test]
fn read_after_read_line_drains_lookahead_surplus() {
    if !backend_available() {
        return;
    }
    let content = b"AB\nCDEFG"; // first line "AB"; surplus "CDEFG" (no trailing newline)
    let src = TempFile::new("interleave", content);
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  base := fs.open(args[1])?
  r := base.buffered()
  buf := buffer(64)
  w := io.stdout
  n := r.read_line(buf)?
  line := buf.bytes().as_str()?
  w.write(\"line=\")?
  w.write(line)?
  w.write(\"\\n\")?
  m := r.read(buf)?
  rest := buf.bytes().as_str()?
  w.write(\"rest=\")?
  w.write(rest)?
  w.write(\"\\n\")?
  print(m)
  return Ok(())
}
";
    let out = build_and_run_args("a7-interleave", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "line=AB\nrest=CDEFG\n5\n",
        "the read after read_line sees the retained surplus 'CDEFG', not fd-fresh bytes",
    );
}

/// `io.copy` is another reader consumer and must obey the same interleaving contract as `read`:
/// after `read_line` has filled the reader's lookahead past the newline, copy drains that retained
/// surplus before reading the fd (which is already at EOF for this short input).
#[test]
fn io_copy_after_read_line_drains_lookahead_surplus() {
    if !backend_available() {
        return;
    }
    let src = TempFile::new("copy-lookahead-src", b"AB\nCDEFG");
    let dst = TempFile::new("copy-lookahead-dst", b"");
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  base := fs.open(args[1])?
  r := base.buffered()
  buf := buffer(64)
  n := r.read_line(buf)?
  print(n)
  w := fs.create(args[2])?
  copied := io.copy(r, w)?
  print(copied)
    return Ok(())
}
";
    let out = build_and_run_args("a7-copy-lookahead", prog, &[&src.str(), &dst.str()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "3\n5\n",
        "line count then copied surplus count"
    );
    assert_eq!(
        std::fs::read(&dst.path).expect("read copied surplus"),
        b"CDEFG"
    );
}

/// A line longer than the internal 64 KiB refill chunk forces several refills across the boundary;
/// its body must come back whole. Prints the recovered body length (via `line.len()`) and the
/// consumed count (`n`, incl. the terminator).
#[test]
fn line_spanning_multiple_refills_reassembled_whole() {
    if !backend_available() {
        return;
    }
    // 200_000 'a' then '\n' then a short line — the first line spans ~3 refills.
    let mut content = vec![b'a'; 200_000];
    content.push(b'\n');
    content.extend_from_slice(b"tail\n");
    let src = TempFile::new("bigline", &content);
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  base := fs.open(args[1])?
  r := base.buffered()
  buf := buffer(16)
  n := r.read_line(buf)?
  line := buf.bytes().as_str()?
  print(line.len())
  print(n)
  return Ok(())
}
";
    let out = build_and_run_args("a7-bigline", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // body length 200_000, consumed 200_001 (body + '\n').
    assert_eq!(String::from_utf8_lossy(&out.stdout), "200000\n200001\n");
}

/// An empty file: the first `read_line` returns `0` immediately (EOF), so the loop makes no lines.
#[test]
fn empty_file_immediate_eof() {
    if !backend_available() {
        return;
    }
    let src = TempFile::new("empty", b"");
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  base := fs.open(args[1])?
  r := base.buffered()
  buf := buffer(8)
  mut count := 0
  loop {
    n := r.read_line(buf)?
    if n == 0 { break }
    count = count + 1
  }
  print(count)
  return Ok(())
}
";
    let out = build_and_run_args("a7-empty", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}

/// A lone `\r` (old-Mac) is **not** a terminator: "a\rb\n" is ONE line whose body is "a\rb"
/// (only the `\r\n` at the end would strip, and here the `\r` is mid-line). A trailing BOM is never
/// stripped. Emits the body length so the retained `\r` is visible (3, not 2).
#[test]
fn lone_cr_is_not_a_terminator() {
    if !backend_available() {
        return;
    }
    let src = TempFile::new("lonecr", b"a\rb\n");
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  base := fs.open(args[1])?
  r := base.buffered()
  buf := buffer(8)
  n := r.read_line(buf)?
  print(buf.len())
  print(n)
  return Ok(())
}
";
    let out = build_and_run_args("a7-lonecr", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // body "a\rb" = 3 bytes; consumed "a\rb\n" = 4.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n4\n");
}

/// JSONL integration (the Phase-2 shape): one JSON object per line, `json.decode`d per line and
/// aggregated. Sums the `n` field across three records.
#[test]
fn jsonl_decode_per_line_aggregated() {
    if !backend_available() {
        return;
    }
    let src = TempFile::new("jsonl", b"{\"n\": 10}\n{\"n\": 20}\n{\"n\": 12}\n");
    let prog = "\
import std.fs
import std.io
import core.json
Rec { n: i64 }
pub fn main(args: array<str>) -> Result<(), Error> {
  base := fs.open(args[1])?
  r := base.buffered()
  buf := buffer(8)
  mut total := 0
  loop {
    k := r.read_line(buf)?
    if k == 0 { break }
    line := buf.bytes().as_str()?
    rec: Rec := json.decode(line)?
    total = total + rec.n
  }
  print(total)
  return Ok(())
}
";
    let out = build_and_run_args("a7-jsonl", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

// --- guardrails --------------------------------------------------------------------------------

/// `read_line` on an **unbuffered** reader is a sema error (the receiver was never `.buffered()`).
#[test]
fn read_line_on_unbuffered_reader_is_sema_error() {
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  buf := buffer(8)
  n := r.read_line(buf)?
  print(n)
  return Ok(())
}
";
    let diags = check_diagnostics("a7-unbuffered", prog);
    assert!(
        diags.contains("read_line requires a buffered reader") && diags.contains(".buffered()"),
        "expected the buffered-receiver diagnostic, got:\n{diags}",
    );
}

/// `bytes.as_str()` on invalid UTF-8 is `Error.Invalid` — the program `?`-propagates it, exiting
/// non-zero (the `main`-returned `Err` is a non-zero exit, no successful stdout).
#[test]
fn as_str_on_invalid_utf8_errors() {
    if !backend_available() {
        return;
    }
    // 0xFF is never valid UTF-8; a single unterminated "line" of it.
    let src = TempFile::new("badutf8", &[0xFF, 0xFE, 0x00]);
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  base := fs.open(args[1])?
  r := base.buffered()
  buf := buffer(8)
  n := r.read_line(buf)?
  line := buf.bytes().as_str()?
  print(line.len())
  return Ok(())
}
";
    let out = build_and_run_args("a7-badutf8", prog, &[&src.str()]);
    assert_ne!(out.status.code(), Some(0), "invalid UTF-8 must propagate as Err (non-zero exit)");
    assert!(out.stdout.is_empty(), "no line length printed: as_str failed before the print");
}

/// The `as_str` VIEW is region-bound to its buffer: returning it past the buffer's `Drop` is an
/// escape error (mirrors the `buf.bytes()` escape guard, #297).
#[test]
fn as_str_view_escaping_its_buffer_is_rejected() {
    let prog = "\
import std.io
fn leak() -> Result<str, Error> {
  buf := buffer(8)
  s := buf.bytes().as_str()?
  return Ok(s)
}
pub fn main() -> Result<(), Error> {
  return Ok(())
}
";
    assert!(check_errs("a7-escape", prog), "an escaping as_str view must be rejected");
}

/// Twin-mirror: the reader-struct change must not disturb the existing reader path — a plain
/// unbuffered `r.read(buf)` loop (no `.buffered()`) still copies a file byte-exact.
#[test]
fn unbuffered_read_path_unchanged() {
    if !backend_available() {
        return;
    }
    let content = b"unchanged reader path 0123456789";
    let src = TempFile::new("plainread", content);
    let prog = "\
import std.fs
import std.io
fn drain(r: reader, buf: buffer, w: writer) -> Result<(), Error> {
  n := r.read(buf)?
  if n == 0 { return Ok(()) }
  w.write(buf.bytes())?
  return drain(r, buf, w)
}
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  buf := buffer(4)
  w := io.stdout
  drain(r, buf, w)?
  return Ok(())
}
";
    let out = build_and_run_args("a7-plainread", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(out.stdout, content, "the unbuffered read path is byte-identical to before");
}
