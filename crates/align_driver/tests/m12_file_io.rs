//! M12 Slice A4 — `std.fs`/`std.io` offset-addressed file I/O: a new Move type `file` (an owned
//! read+write fd; `Drop` closes it), constructed by `fs.create_rw` (`O_RDWR|O_CREAT|O_TRUNC`) /
//! `fs.open_rw` (`O_RDWR`, must exist), with `f.pread(b: mut buffer, off)` / `f.pwrite(data, off)`
//! (loops to full; past-EOF extends) / `f.len()` (live fstat). `fs.open_ro` supplies the same
//! File with read-only access; pwrite returns Denied. **No cursor / no seek.** Negative offset aborts. The headline drives create_rw → pwrite at offsets (incl. a
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

/// Acquire fixture ownership with one exclusive mkdir; cleanup never touches an unowned path.
struct FileFixtures { dir: PathBuf }
impl FileFixtures {
    fn acquire(dir: PathBuf) -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        Ok(Self { dir })
    }
    fn new(tag: &str) -> Self {
        Self::acquire(std::env::temp_dir().join(format!("align-file-{}-{tag}-{}", std::process::id(), thin_nonce())))
            .expect("exclusive fixture directory")
    }
}
impl Drop for FileFixtures {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.dir); }
}

#[test]
fn file_fixture_directory_preserves_existing_entries() {
    let root = FileFixtures::new("ownership");
    let occupied = root.dir.join("occupied");
    std::fs::create_dir(&occupied).unwrap();
    std::fs::write(occupied.join("marker"), b"untouched").unwrap();
    assert!(FileFixtures::acquire(occupied.clone()).is_err());
    let link = root.dir.join("link");
    std::os::unix::fs::symlink(&occupied, &link).unwrap();
    assert!(FileFixtures::acquire(link).is_err());
    assert_eq!(std::fs::read(occupied.join("marker")).unwrap(), b"untouched");
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
  mut buf := buffer(6)
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
    if !backend_available() { return; }
    let fixtures = FileFixtures::new("negative-offset");
    let file = fixtures.dir.join("input");
    std::fs::write(&file, b"content").unwrap();
    for constructor in ["create_rw", "open_ro"] {
        for operation in ["f.pwrite(\"x\", -5)", "f.pwrite(\"\", -5)", "f.pread(buf, -5)"] {
            let source = format!("import std.fs\nfn main(args: array<str>) -> Result<(), Error> {{\n f := fs.{constructor}(args[1])?\n mut buf := buffer(2)\n {operation}?\n return Ok(())\n}}\n");
            let out = build_and_run_args(&format!("m12-negative-{constructor}-{}", operation.len()), &source, &[file.to_str().unwrap()]);
            assert_eq!(out.status.code(), None, "negative offset must abort before readonly denial: {}", String::from_utf8_lossy(&out.stderr));
        }
    }
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

/// `file` must be nameable as a surface type (gate F1 regression): a helper fn taking a `file`
/// parameter (by value — the file *moves* into the helper) `pwrite`s through it, called from
/// `main` with a freshly created file. Full compile+run round-trip, verified against the on-disk
/// bytes. A separate negative assert covers the move: using the original binding after the call
/// (which consumed it) must be rejected, mirroring the `reader`/`writer` param-threading gates.
#[test]
fn file_param_threading_through_helper_fn() {
    if !backend_available() {
        return;
    }
    let f = TempFile::out("param");
    let prog = "\
import std.fs
fn write_at(f: file, off: i64) -> Result<(), Error> {
  f.pwrite(\"hi\", off)?
  return Ok(())
}
pub fn main(args: array<str>) -> Result<(), Error> {
  f := fs.create_rw(args[1])?
  write_at(f, 3)?
  return Ok(())
}
";
    let out = build_and_run_args("m12-file-param-threading", prog, &[&f.str()]);
    assert_eq!(out.status.code(), Some(0), "program exits 0; stderr: {}", String::from_utf8_lossy(&out.stderr));
    let mut expected = vec![0u8; 3];
    expected.extend_from_slice(b"hi");
    assert_eq!(std::fs::read(&f.path).unwrap(), expected, "the helper's pwrite must land through the threaded param");

    // Use-after-call: `f` moved into `write_at`, so using it again in `main` must be rejected.
    assert!(check_errs(
        "m12-file-param-use-after-call",
        "import std.fs\nfn write_at(f: file, off: i64) -> Result<(), Error> {\n  f.pwrite(\"hi\", off)?\n  return Ok(())\n}\npub fn main(args: array<str>) -> Result<(), Error> {\n  f := fs.create_rw(args[1])?\n  write_at(f, 3)?\n  f.pwrite(\"x\", 0)?\n  return Ok(())\n}\n",
    ), "using a file after moving it into a helper fn's `file` parameter must be rejected");
}

/// `file` must be nameable as a surface *return* type (the gate's exact repro): a helper fn
/// returning `Result<file, Error>` from `fs.create_rw`, bound and used from `main`.
#[test]
fn file_returned_from_helper_fn() {
    if !backend_available() {
        return;
    }
    let f = TempFile::out("ret");
    let prog = "\
import std.fs
fn mk(p: str) -> Result<file, Error> = fs.create_rw(p)
pub fn main(args: array<str>) -> Result<(), Error> {
  f := mk(args[1])?
  f.pwrite(\"hello\", 0)?
  return Ok(())
}
";
    let out = build_and_run_args("m12-file-return", prog, &[&f.str()]);
    assert_eq!(out.status.code(), Some(0), "program exits 0; stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read(&f.path).unwrap(), b"hello", "the file returned from the helper must be usable in main");
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
        "import std.fs\nimport std.io\npub fn main(args: array<str>) -> Result<(), Error> {\n  f := fs.create_rw(args[1])?\n  f.pwrite(\"hi\", 0)?\n  mut buf := buffer(2)\n  n := f.pread(buf, 0)?\n  print(f.len()?)\n  return Ok(())\n}\n",
    ), "a bound file's pwrite/pread/len must stay allowed");
}

/// Read-only admission is observable on the same mode-0444 file that open_rw refuses.
#[test]
fn open_ro_permissions_windows_and_write_denial() {
    if !backend_available() { return; }
    use std::os::unix::fs::PermissionsExt;
    let fixtures = FileFixtures::new("open-ro-permissions");
    let readable = fixtures.dir.join("readable");
    let denied = fixtures.dir.join("denied");
    let missing = fixtures.dir.join("missing");
    std::fs::write(&readable, b"abcdef").unwrap();
    std::fs::write(&denied, b"secret").unwrap();
    std::fs::set_permissions(&readable, std::fs::Permissions::from_mode(0o444)).unwrap();
    std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert!(std::fs::OpenOptions::new().read(true).write(true).open(&readable).is_err(),
        "permission owner requires a non-root host where mode 0444 denies write admission");
    let source = r#"
import std.fs
fn main(args: array<str>) -> Result<(), Error> {
  f := fs.open_ro(args[1])?
  if f.len()? != 6 { return Err(Error.Invalid) }
  mut b := buffer(4)
  if f.pread(b, 2)? != 4 { return Err(Error.Invalid) }
  text := b.bytes().as_str()?
  if text != "cdef" { return Err(Error.Invalid) }
  if f.pread(b, 5)? != 1 { return Err(Error.Invalid) }
  if f.pread(b, 6)? != 0 { return Err(Error.Invalid) }
  if b.len() != 0 { return Err(Error.Invalid) }
  if f.pread(b, 100)? != 0 { return Err(Error.Invalid) }
  match f.pwrite("XYZ", 20) { Ok(_) => { return Err(Error.Invalid) }, Err(e) => match e { Denied => {}, _ => { return Err(Error.Invalid) } } }
  match f.pwrite("", 0) { Ok(_) => { return Err(Error.Invalid) }, Err(e) => match e { Denied => {}, _ => { return Err(Error.Invalid) } } }
  if f.len()? != 6 { return Err(Error.Invalid) }
  match fs.open_rw(args[1]) { Ok(_) => { return Err(Error.Invalid) }, Err(e) => match e { Denied => {}, _ => { return Err(Error.Invalid) } } }
  match fs.open_ro(args[2]) { Ok(_) => { return Err(Error.Invalid) }, Err(e) => match e { Denied => {}, _ => { return Err(Error.Invalid) } } }
  match fs.open_ro(args[3]) { Ok(_) => { return Err(Error.Invalid) }, Err(e) => match e { NotFound => {}, _ => { return Err(Error.Invalid) } } }
  match fs.open_ro("bad\0path") { Ok(_) => { return Err(Error.Invalid) }, Err(e) => match e { Invalid => {}, _ => { return Err(Error.Invalid) } } }
  return Ok(())
}
"#;
    let output = build_and_run_args("open-ro-permissions", source, &[
        readable.to_str().unwrap(), denied.to_str().unwrap(), missing.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(std::fs::read(&readable).unwrap(), b"abcdef");
    assert!(!missing.exists());
}

#[test]
fn open_ro_control_and_imported_generic_parity() {
    if !backend_available() { return; }
    let fixtures = FileFixtures::new("open-ro-control");
    let path = fixtures.dir.join("input");
    std::fs::write(&path, b"abcdef").unwrap();
    let library = r#"
module positional
import std.fs
pub fn open<T>(value: T, path: str) -> Result<file, Error> = fs.open_ro(path.clone())
pub fn read(f: file) -> Result<i64, Error> {
  mut b := buffer(8)
  return f.pread(b, 4)
}
"#;
    let source = format!(r#"
import std.fs
import positional
fn path_once(p: str) -> string {{ print(1); return p.clone() }}
fn early(p: str) -> i64 {{ f := fs.open_ro({{ return 17; p }}); return 0 }}
fn main() -> Result<(), Error> {{
  path := "{}".clone()
  f := positional.open(true, path)?
  if positional.read(f)? != 2 {{ return Err(Error.Invalid) }}
  print(path)
  mut g := fs.open_ro(if true {{ path_once(path) }} else {{ "missing".clone() }})?
  g = fs.open_ro(match 1 {{ 1 => path.clone(), _ => "missing".clone() }})?
  if g.len()? != 6 {{ return Err(Error.Invalid) }}
  h := fs.open_ro(loop {{ break path.clone() }}) else {{ return Err(Error.Invalid) }}
  if h.len()? != 6 {{ return Err(Error.Invalid) }}
  a := fs.open_ro(arena {{ path.clone() }})?
  if a.len()? != 6 {{ return Err(Error.Invalid) }}
  t := fs.open_ro(task_group {{ path.clone() }})?
  if t.len()? != 6 {{ return Err(Error.Invalid) }}
  match fs.open_ro(path).map_err(fn e: Error {{ e }}) {{ Ok(owner) => {{ if owner.len()? != 6 {{ return Err(Error.Invalid) }} }}, Err(e) => {{ return Err(e) }} }}
  if early(path) != 17 {{ return Err(Error.Invalid) }}
  rw := fs.open_rw(path)?
  mut b := buffer(4)
  if rw.pread(b, 2)? != 4 {{ return Err(Error.Invalid) }}
  if b.bytes().as_str()? != "cdef" {{ return Err(Error.Invalid) }}
  return Ok(())
}}
"#, path.display());
    let files = [("positional.align", library), ("main.align", source.as_str())];
    for output in [build_and_run_multi("open-ro-whole", &files, "main.align"),
        build_per_unit_multi("open-ro-units", &files, "main.align").link_and_run()] {
        assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), format!("{}\n1\n", path.display()));
    }
}

#[test]
fn open_ro_formation_and_move_diagnostics() {
    for constructor in ["open_ro", "create_rw_exclusive"] {
    for source in [
        "fn main() { f := fs.open_ro(\"x\") }",
        "import std.fs\nfn main() { f := fs.open_ro() }",
        "import std.fs\nfn main() { f := fs.open_ro(\"x\", \"y\") }",
        "import std.fs\nfn main() { f := fs.open_ro(1) }",
        "import std.fs\nfn main() { f: Result<i64, Error> := fs.open_ro(\"x\") }",
        "import std.fs\nfn main() -> Result<(), Error> { fs.open_ro(\"x\")?.len()?; return Ok(()) }",
        "import std.fs\nfn main() -> Result<(), Error> { f := fs.open_ro(\"x\")?; g := f; f.len()?; return Ok(()) }",
        "import std.fs\nfn main() -> Result<(), Error> { f := fs.open_ro(\"x\")?; print(f); return Ok(()) }",
        "import std.fs\nfn main() -> Result<(), Error> { f := fs.open_ro(\"x\")?; xs := [1,2].par_map(|n| { f.len(); n }); return Ok(()) }",
        "import std.fs\nfn main() { xs := [1,2].par_map(|n| { fs.open_ro(\"x\"); n }) }",
    ] {
        let source = source.replace("open_ro", constructor);
        assert!(check_errs("file-constructor-invalid", &source), "{source}");
        let mut sm = SourceMap::new();
        assert!(check_per_unit(&mut sm, "file-constructor-invalid.align", &source).diags.has_errors(), "per-unit: {source}");
    }
    }
}

#[test]
fn create_rw_exclusive_preserves_existing_entries_and_reads_offsets() {
    if !backend_available() { return; }
    let root = FileFixtures::new("exclusive-driver");
    let existing = root.dir.join("existing");
    std::fs::write(&existing, b"preserved").unwrap();
    let directory = root.dir.join("directory");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("marker"), b"marker").unwrap();
    let symlink = root.dir.join("symlink");
    std::os::unix::fs::symlink(&existing, &symlink).unwrap();
    for mode in ["whole", "units"] {
        let fresh = root.dir.join(mode);
        let source = format!(r#"
import std.fs
fn occupied(p: str) -> bool {{
  match fs.create_rw_exclusive(p) {{ Ok(f) => {{ return false }}, Err(e) => {{ match e {{ Code(n) => {{ return n > 0 }}, _ => {{ return false }} }} }} }}
}}
fn path_error(p: str) -> i64 {{
  match fs.create_rw_exclusive(p) {{ Ok(f) => {{ return 0 }}, Err(e) => {{ match e {{ NotFound => {{ return 1 }}, Invalid => {{ return 2 }}, _ => {{ return 3 }} }} }} }}
}}
fn main() -> Result<(), Error> {{
  f := fs.create_rw_exclusive("{}")?
  if f.pwrite("ab", 0)? != 2 {{ return Err(Error.Invalid) }}
  if f.pwrite("xy", 4)? != 2 {{ return Err(Error.Invalid) }}
  if f.len()? != 6 {{ return Err(Error.Invalid) }}
  mut b := buffer(8)
  if f.pread(b, 4)? != 2 {{ return Err(Error.Invalid) }}
  if b.bytes().as_str()? != "xy" {{ return Err(Error.Invalid) }}
  if f.pread(b, 6)? != 0 {{ return Err(Error.Invalid) }}
  if !occupied("{}") || !occupied("{}") || !occupied("{}") || !occupied("{}") {{ return Err(Error.Invalid) }}
  if path_error("{}") != 1 || path_error("") != 2 || path_error("bad\0path") != 2 {{ return Err(Error.Invalid) }}
  return Ok(())
}}
"#, fresh.display(), fresh.display(), existing.display(), directory.display(), symlink.display(), root.dir.join("absent-parent/file").display());
        let files = [("main.align", source.as_str())];
        let output = if mode == "whole" { build_and_run_multi("exclusive-whole", &files, "main.align") }
            else { build_per_unit_multi("exclusive-units", &files, "main.align").link_and_run() };
        assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(std::fs::read(fresh).unwrap(), b"ab\0\0xy");
    }
    assert_eq!(std::fs::read(&existing).unwrap(), b"preserved");
    assert_eq!(std::fs::read(directory.join("marker")).unwrap(), b"marker");
    assert_eq!(std::fs::read_link(symlink).unwrap(), existing);
}

#[test]
fn create_rw_exclusive_control_and_imported_generic_parity() {
    if !backend_available() { return; }
    let root = FileFixtures::new("exclusive-control");
    let library = r#"
module exclusive
import std.fs
pub fn create<T>(value: T, path: str) -> Result<file, Error> = fs.create_rw_exclusive(path.clone())
pub fn write(f: file) -> Result<(), Error> { f.pwrite("owned", 0)?; return Ok(()) }
"#;
    for mode in ["whole", "units"] {
        let dir = root.dir.join(mode);
        std::fs::create_dir(&dir).unwrap();
        let source = format!(r#"
import std.fs
import exclusive
fn once(p: str) -> string {{ print(1); return p.clone() }}
fn early(p: str) -> i64 {{ f := fs.create_rw_exclusive({{ return 17; p }}); return 0 }}
fn fail(p: str) -> Result<(), Error> {{ f := fs.create_rw_exclusive(p)?; f.pwrite("partial", 0)?; return Err(Error.Invalid) }}
fn main() -> Result<(), Error> {{
  path := "{0}/helper".clone()
  f := exclusive.create(true, path)?
  exclusive.write(f)?
  print(path)
  mut g := fs.create_rw_exclusive(if true {{ once("{0}/if") }} else {{ "missing".clone() }})?
  g = fs.create_rw_exclusive(match 1 {{ 1 => "{0}/match".clone(), _ => "missing".clone() }})?
  if g.len()? != 0 {{ return Err(Error.Invalid) }}
  h := fs.create_rw_exclusive(loop {{ break "{0}/loop".clone() }}) else {{ return Err(Error.Invalid) }}
  if h.len()? != 0 {{ return Err(Error.Invalid) }}
  a := fs.create_rw_exclusive(arena {{ "{0}/arena".clone() }})?
  if a.len()? != 0 {{ return Err(Error.Invalid) }}
  t := fs.create_rw_exclusive(task_group {{ "{0}/task".clone() }})?
  if t.len()? != 0 {{ return Err(Error.Invalid) }}
  match fs.create_rw_exclusive("{0}/map").map_err(fn e: Error {{ e }}) {{ Ok(owner) => {{ owner.pwrite("map", 0)? }}, Err(e) => {{ return Err(e) }} }}
  if early("{0}/never") != 17 {{ return Err(Error.Invalid) }}
  match fail("{0}/failure") {{ Err(e) => {{ match e {{ Invalid => {{}}, _ => {{ return Err(Error.Invalid) }} }} }}, _ => {{ return Err(Error.Invalid) }} }}
  return Ok(())
}}
"#, dir.display());
        let files = [("exclusive.align", library), ("main.align", source.as_str())];
        let output = if mode == "whole" { build_and_run_multi("exclusive-control-whole", &files, "main.align") }
            else { build_per_unit_multi("exclusive-control-units", &files, "main.align").link_and_run() };
        assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), format!("{}\n1\n", dir.join("helper").display()));
        assert_eq!(std::fs::read(dir.join("helper")).unwrap(), b"owned");
        assert_eq!(std::fs::read(dir.join("map")).unwrap(), b"map");
        assert_eq!(std::fs::read(dir.join("failure")).unwrap(), b"partial");
        for name in ["if", "match", "loop", "arena", "task"] { assert_eq!(std::fs::metadata(dir.join(name)).unwrap().len(), 0); }
        assert!(!dir.join("never").exists());
    }
}


#[test]
fn sync_file_and_buffered_writer_in_imported_helpers() {
    if !backend_available() { return; }
    let fixtures = FileFixtures::new("sync");
    let file_path = fixtures.dir.join("file");
    let writer_path = fixtures.dir.join("writer");
    let library = r#"
module synchronization
pub fn file_owner<T>(tag: T, f: file) -> Result<file, Error> { f.sync()?; return Ok(f) }
pub fn writer_owner<T>(tag: T, w: writer) -> Result<writer, Error> { w.sync()?; return Ok(w) }
"#;
    let source = format!(r#"
import std.fs
import synchronization
fn main() -> Result<(), Error> {{
  file_path := "{}"
  writer_path := "{}"
  f := fs.create_rw(file_path)?
  f.pwrite("file", 0)?
  mut g := synchronization.file_owner(true, f)?
  g.pwrite("!", 4)?
  match g.sync().map_err(fn e: Error {{ e }}) {{ Ok(value) => {{}}, Err(e) => {{ return Err(e) }} }}
  mut n := 0
  loop {{ if n == 2 {{ break }}; g.sync()?; n = n + 1 }}
  g.sync() else {{ return Err(Error.Invalid) }}
  g = fs.open_rw(file_path)?
  g.sync()?
  ro := fs.open_ro(file_path)?
  ro.sync()?
  print(fs.read_file(file_path)?)
  w := fs.create(writer_path)?
  w.write("buffered")?
  if fs.read_file(writer_path)? != "" {{ return Err(Error.Invalid) }}
  mut v := synchronization.writer_owner(1, w)?
  if fs.read_file(writer_path)? != "buffered" {{ return Err(Error.Invalid) }}
  v.write("!")?
  match v.sync().map_err(fn e: Error {{ e }}) {{ Ok(value) => {{}}, Err(e) => {{ return Err(e) }} }}
  n = 0
  loop {{ if n == 2 {{ break }}; v.sync()?; n = n + 1 }}
  v.sync() else {{ return Err(Error.Invalid) }}
  print(fs.read_file(writer_path)?)
  v = fs.create(writer_path)?
  v.sync()?
  return Ok(())
}}
"#, file_path.display(), writer_path.display());
    let files = [("synchronization.align", library), ("main.align", source.as_str())];
    for output in [build_and_run_multi("sync-whole", &files, "main.align"),
        build_per_unit_multi("sync-units", &files, "main.align").link_and_run()] {
        assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(output.stdout, b"file!\nbuffered!\n");
    }
}

#[test]
fn sync_formation_effect_and_move_diagnostics() {
    for (ty, constructor) in [("file", "fs.create_rw(\"x\")"), ("writer", "fs.create(\"x\")")] {
        let bind = format!("owner := {constructor}?");
        for body in [
            format!("{bind}; owner.sync(1)?"),
            format!("{bind}; wrong: Result<i64, Error> := owner.sync()"),
            format!("{constructor}?.sync()?"),
            format!("{bind}; moved := owner; owner.sync()?"),
            format!("{bind}; owner.sync()"),
            format!("{bind}; xs := [1,2].par_map(|n| {{ owner.sync(); n }})"),
        ] {
            let source = format!("import std.fs\nfn main() -> Result<(), Error> {{ {body}; return Ok(()) }}\n");
            assert!(check_errs(&format!("sync-invalid-{ty}"), &source), "{source}");
            let files = [("main.align", source.as_str())];
            assert!(check_per_unit_multi(&format!("sync-invalid-unit-{ty}"), &files, "main.align").diags.has_errors(), "{source}");
        }
    }
}

#[test]
fn sync_preserves_connection_writer_lifetime() {
    let valid = "import std.net\nfn operation() -> Result<(), Error> { conn := tcp.connect(\"127.0.0.1\", 80)?; w := conn.writer(); w.sync()?; w.write(\"still borrowed\")?; return Ok(()) }\nfn main() {}\n";
    assert!(!check_errs("sync-connection-valid", valid));
    let valid_files = [("main.align", valid)];
    assert!(!check_per_unit_multi("sync-connection-valid-unit", &valid_files, "main.align").diags.has_errors());
    for body in [
        "fn escape() -> Result<writer, Error> { conn := tcp.connect(\"127.0.0.1\", 80)?; w := conn.writer(); w.sync()?; return Ok(w) }",
        "fn retire(conn: tcp_conn) {}\nfn operation() -> Result<(), Error> { conn := tcp.connect(\"127.0.0.1\", 80)?; w := conn.writer(); w.sync()?; retire(conn); w.sync()?; return Ok(()) }",
        "fn operation() -> Result<(), Error> { conn := tcp.connect(\"127.0.0.1\", 80)?; w := conn.writer(); w.sync()?; xs := [1,2].par_map(|n| { w.sync(); n }); return Ok(()) }",
    ] {
        let source = format!("import std.net\n{body}\nfn main() {{}}\n");
        assert!(check_errs("sync-connection-invalid", &source), "{source}");
        let files = [("main.align", source.as_str())];
        assert!(check_per_unit_multi("sync-connection-invalid-unit", &files, "main.align").diags.has_errors(), "{source}");
    }
}
