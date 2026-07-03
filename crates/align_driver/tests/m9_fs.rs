//! M9 Slice 3 — std.fs complete: `fs.write_file` / `fs.exists` / `fs.remove` / `fs.read_dir`
//! (owned `array<string>`), and the headline `fs.read_file_view` — an mmap view that requires an
//! enclosing `arena {}` (region bound to the arena, `munmap` at arena end). The completion
//! condition is the `draft.md` §19 type program running end to end (read_file → json.decode →
//! pipeline → builder → stdout), plus a byte-exact view read, the view escape rejection, the
//! write/exists/remove/read_dir round-trip, and the errno mapping. (`docs/impl/07-roadmap.md` M9
//! Slice 3; `draft.md` §18.2/§19.)

mod common;
use common::*;

use std::path::PathBuf;

/// A temp file with `content`, removed on scope exit (even on panic).
struct TempFile {
    path: PathBuf,
}
impl TempFile {
    fn new(name: &str, content: &[u8]) -> TempFile {
        let path = std::env::temp_dir().join(format!("align-m9fs-{}-{name}", std::process::id()));
        std::fs::write(&path, content).expect("write temp file");
        TempFile { path }
    }
    /// A path for an output file (not created yet); still cleaned up on scope exit.
    fn out(name: &str) -> TempFile {
        let path = std::env::temp_dir().join(format!("align-m9fs-out-{}-{name}", std::process::id()));
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

/// A temp directory, removed recursively on scope exit.
struct TempDir {
    path: PathBuf,
}
impl TempDir {
    fn new(name: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!("align-m9fs-dir-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        TempDir { path }
    }
    fn str(&self) -> String {
        self.path.display().to_string()
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// --- write_file / exists / remove round-trip --------------------------------------------------

/// The write/exists/read/remove round-trip: `write_file` a payload, `exists` reports it present,
/// `read_file` reads back the exact byte count, `remove` deletes it, `exists` reports it gone.
#[test]
fn write_exists_read_remove_round_trip() {
    if !backend_available() {
        return;
    }
    let f = TempFile::out("roundtrip");
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  fs.write_file(args[1], \"roundtrip payload\\n\")?
  if fs.exists(args[1]) { print(1) } else { print(0) }
  data := fs.read_file(args[1])?
  print(data.len())
  fs.remove(args[1])?
  if fs.exists(args[1]) { print(1) } else { print(0) }
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-roundtrip", prog, &[&f.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // exists=1, len("roundtrip payload\n")=18, exists=0.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n18\n0\n");
    assert!(!f.path.exists(), "the file was removed");
}

/// `fs.write_file` accepts a `builder`'s bytes (the third `str | bytes | builder` form), written
/// directly without materializing a string.
#[test]
fn write_file_from_builder() {
    if !backend_available() {
        return;
    }
    let f = TempFile::out("builder");
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  arena {
    b := builder()
    b.write(\"from \")
    b.write(\"builder\")
    fs.write_file(args[1], b)?
  }
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-wbuilder", prog, &[&f.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read(&f.path).expect("read written file"), b"from builder");
}

/// `fs.write_file` accepts `bytes` (a `slice<u8>`) — here a `buffer` filled from a `reader`, then
/// its `.bytes()` view written straight to a file (no string materialization).
#[test]
fn write_file_from_bytes_slice() {
    if !backend_available() {
        return;
    }
    let src = TempFile::new("bytes-src", b"raw bytes payload");
    let dst = TempFile::out("bytes-dst");
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  r := fs.open(args[1])?
  buf := buffer(64)
  n := r.read(buf)?
  fs.write_file(args[2], buf.bytes())?
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-wbytes", prog, &[&src.str(), &dst.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read(&dst.path).expect("read dst"), b"raw bytes payload");
}

/// `fs.remove` of a missing file maps `ENOENT` to `Error.NotFound` (tag 0 → main exits 1).
#[test]
fn remove_missing_maps_to_not_found() {
    if !backend_available() {
        return;
    }
    let missing = std::env::temp_dir().join(format!("align-m9fs-absent-{}/nope", std::process::id()));
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  fs.remove(args[1])?
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-rm-notfound", prog, &[&missing.display().to_string()]);
    assert_eq!(out.status.code(), Some(1), "removing a missing file is Error.NotFound (tag 0 -> exit 1)");
}

// --- read_dir ---------------------------------------------------------------------------------

/// `fs.read_dir` returns an owned `array<string>` of the directory's entry names; `.len()` reports
/// the count (`.`/`..` excluded). The whole owned array is deep-`Drop`-freed at scope exit.
#[test]
fn read_dir_counts_entries() {
    if !backend_available() {
        return;
    }
    let dir = TempDir::new("entries");
    for i in 0..3 {
        std::fs::write(dir.path.join(format!("file{i}.txt")), b"x").expect("write entry");
    }
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  names := fs.read_dir(args[1])?
  print(names.len())
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-readdir", prog, &[&dir.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n", "three entries");
}

/// `fs.read_dir` of a missing directory maps `ENOENT` to `Error.NotFound` (tag 0 → main exits 1).
#[test]
fn read_dir_missing_maps_to_not_found() {
    if !backend_available() {
        return;
    }
    let missing = std::env::temp_dir().join(format!("align-m9fs-nodir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&missing);
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  names := fs.read_dir(args[1])?
  print(names.len())
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-readdir-notfound", prog, &[&missing.display().to_string()]);
    assert_eq!(out.status.code(), Some(1), "a missing dir is Error.NotFound (tag 0 -> exit 1)");
}

/// An owned `array<string>` from `fs.read_dir` returns out of `main` as a whole (Move), then is
/// deep-freed by the boundary — exercises the `DynArray(String)` payload move + deep drop without a
/// leak (an empty dir yields a `{null,0}` array, dropped harmlessly).
#[test]
fn read_dir_empty_dir_is_zero() {
    if !backend_available() {
        return;
    }
    let dir = TempDir::new("empty");
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  names := fs.read_dir(args[1])?
  print(names.len())
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-readdir-empty", prog, &[&dir.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n", "empty dir has 0 entries");
}

// --- read_file_view (mmap) --------------------------------------------------------------------

/// The headline: `fs.read_file_view` mmaps the file into the enclosing arena; the returned `str`
/// view reads byte-exact (`.len()` + `io.stdout.write(view)` reproduces the file). The mapping is
/// `munmap`ped when the arena block ends.
#[test]
fn read_file_view_is_byte_exact() {
    if !backend_available() {
        return;
    }
    let content = b"mmap view bytes\n";
    let src = TempFile::new("view-src", content);
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  arena {
    v := fs.read_file_view(args[1])?
    print(v.len())
    io.stdout.write(v)?
  }
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-view", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let expected = format!("{}\n{}", content.len(), String::from_utf8_lossy(content));
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected, "the view reads byte-exact");
}

/// A larger-than-a-page file (so mmap spans multiple pages) still reads byte-exact through the view.
#[test]
fn read_file_view_multi_page() {
    if !backend_available() {
        return;
    }
    // > 100 KiB of valid multibyte UTF-8 (well over a 4 KiB page) — `read_file_view` returns a `str`,
    // so the content must be valid UTF-8 (draft §7/§12); binary reads use `reader.read(buffer)`. Whole
    // units are appended (never a truncated multibyte char), so the buffer stays well-formed.
    let unit = "café 日本語 test 😀 multi-page line\n";
    let mut text = String::new();
    while text.len() < 100 * 1024 {
        text.push_str(unit);
    }
    let content = text.into_bytes();
    let src = TempFile::new("view-big", &content);
    let dst = TempFile::out("view-big-out");
    let prog = "\
import std.fs
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  arena {
    v := fs.read_file_view(args[1])?
    w := fs.create(args[2])?
    w.write(v)?
  }
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-view-big", prog, &[&src.str(), &dst.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read(&dst.path).expect("read dst"), content, "multi-page view is byte-exact");
}

/// A zero-length file is never `mmap`ped (mmap of length 0 is `EINVAL`); `read_file_view` returns an
/// empty view (`len() == 0`) via the copy fallback — no error.
#[test]
fn read_file_view_empty_file_is_empty_view() {
    if !backend_available() {
        return;
    }
    let src = TempFile::new("view-empty", b"");
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  arena {
    v := fs.read_file_view(args[1])?
    print(v.len())
  }
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-view-empty", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n", "empty file yields an empty view");
}

/// A special file whose `st_size` is 0 or a lie (here `/proc/self/stat`, a regular-looking file
/// with size 0 but real content) is *not* mmap'd — the copy fallback reads its true bytes, so the
/// view has nonzero length. Guards the "avoid /proc / character devices" mmap guardrail.
#[test]
#[cfg(target_os = "linux")]
fn read_file_view_proc_file_falls_back_to_copy() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  arena {
    v := fs.read_file_view(args[1])?
    if v.len() > 0 { print(1) } else { print(0) }
  }
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-view-proc", prog, &["/proc/self/stat"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n", "/proc file read real bytes via the fallback");
}

/// `read_file_view` of a missing path maps `ENOENT` to `Error.NotFound` (tag 0 → main exits 1).
#[test]
fn read_file_view_missing_maps_to_not_found() {
    if !backend_available() {
        return;
    }
    let missing = std::env::temp_dir().join(format!("align-m9fs-view-absent-{}", std::process::id()));
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  arena {
    v := fs.read_file_view(args[1])?
    print(v.len())
  }
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-view-notfound", prog, &[&missing.display().to_string()]);
    assert_eq!(out.status.code(), Some(1), "a missing file is Error.NotFound (tag 0 -> exit 1)");
}

// --- read_file_view region / arena guardrails (compile-time) ----------------------------------

/// `read_file_view` requires an enclosing `arena {}` (the view is `munmap`ped at arena end) — using
/// it outside any arena is a compile error, exactly like `heap.new`.
#[test]
fn read_file_view_outside_arena_is_rejected() {
    let prog = "\
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  v := fs.read_file_view(args[1])?
  print(v.len())
  return Ok(())
}
";
    assert!(check_errs("m9fs-view-no-arena", prog), "read_file_view outside an arena must be rejected");
}

/// The view is bound to its arena's region: returning it out of the arena (where the mapping is
/// unmapped) is an escape and rejected at compile time. `.clone()` is the way to copy it out.
#[test]
fn read_file_view_escaping_its_arena_is_rejected() {
    let prog = "\
import std.fs
fn leak(p: str) -> Result<str, Error> {
  arena {
    v := fs.read_file_view(p)?
    return Ok(v)
  }
}
pub fn main() -> i32 { return 0 }
";
    assert!(check_errs("m9fs-view-escape", prog), "returning an arena-bound view must be rejected");
}

/// A view `.clone()`d escapes its arena as an owned `string` — the sanctioned copy-out path.
#[test]
fn read_file_view_clone_escapes() {
    if !backend_available() {
        return;
    }
    let content = b"cloned out of the arena";
    let src = TempFile::new("view-clone", content);
    let prog = "\
import std.fs
import std.io
fn load(p: str) -> Result<string, Error> {
  arena {
    v := fs.read_file_view(p)?
    return Ok(v.clone())
  }
}
pub fn main(args: array<str>) -> Result<(), Error> {
  s := load(args[1])?
  io.stdout.write(s)?
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-view-clone", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(out.stdout, content, "the cloned string escapes and prints");
}

// --- §19 completion condition -----------------------------------------------------------------

/// The `draft.md` §19 type program: `fs.read_file` → `json.decode<array<User>>` → a fused pipeline
/// (`.where(.active).score.sum()`) → a `builder` → `io.stdout`. End to end.
#[test]
fn draft_section_19_program_runs_end_to_end() {
    if !backend_available() {
        return;
    }
    let json = br#"[{"id":1,"name":"a","active":true,"score":10},{"id":2,"name":"b","active":false,"score":5},{"id":3,"name":"c","active":true,"score":7}]"#;
    let src = TempFile::new("s19", json);
    let prog = "\
import core.json
import std.fs
import std.io
User { id: i64, name: str, active: bool, score: i32 }
pub fn main(args: array<str>) -> Result<(), Error> {
  arena {
    data := fs.read_file(args[1])?
    users: array<User> := json.decode(data)?
    total := users.where(.active).score.sum()
    out := builder()
    out.write(\"active score: \")
    out.write_int(total)
    out.write(\"\\n\")
    io.stdout.write(out)?
  }
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-s19", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "active score: 17\n", "10 + 7 for the active users");
}

/// The same §19 shape, but the JSON is sourced from a **read_file_view** (mmap) instead of an owned
/// `read_file` string — proving the zero-copy view feeds `json.decode` and the whole pipeline while
/// staying arena-bound.
#[test]
fn read_file_view_feeds_json_decode_pipeline() {
    if !backend_available() {
        return;
    }
    let json = br#"[{"id":1,"name":"a","active":true,"score":40},{"id":2,"name":"b","active":true,"score":2}]"#;
    let src = TempFile::new("s19-view", json);
    let prog = "\
import core.json
import std.fs
import std.io
User { id: i64, name: str, active: bool, score: i32 }
pub fn main(args: array<str>) -> Result<(), Error> {
  arena {
    view := fs.read_file_view(args[1])?
    users: array<User> := json.decode(view)?
    total := users.where(.active).score.sum()
    out := builder()
    out.write(\"active score: \")
    out.write_int(total)
    out.write(\"\\n\")
    io.stdout.write(out)?
  }
  return Ok(())
}
";
    let out = build_and_run_args("m9fs-s19-view", prog, &[&src.str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "active score: 42\n");
}
