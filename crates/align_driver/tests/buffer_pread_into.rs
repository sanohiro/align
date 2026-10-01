//! Direct bounded positional reads retain storage and publish only initialized byte prefixes.
mod common;
use common::*;

struct Fixtures(std::path::PathBuf);
impl Fixtures {
    fn new() -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let dir = std::env::temp_dir().join(format!("align-pread-into-{}-{}", std::process::id(), thin_nonce()));
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        Ok(Self(dir))
    }
}
impl Drop for Fixtures {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

#[test]
fn bounded_positional_reads_in_imported_helpers() -> Result<(), Box<dyn std::error::Error>> {
    if !backend_available() { return Ok(()); }
    let fixture = Fixtures::new()?;
    let input = fixture.0.join("input");
    std::fs::write(&input, b"0123456789ABCDEF")?;
    let path = input.to_str().ok_or("UTF-8 fixture path")?.replace('\\', "\\\\").replace('"', "\\\"");
    let helpers = r#"module windows
pub fn read(borrow f: file, borrow mut b: buffer, destination: i64, length: i64, offset: i64) -> Result<i64, Error> =
  f.pread_into(b, destination, length, offset)
pub fn scalar<T: Num>(value: T) -> T = value
pub fn exit_before_io(borrow f: file, borrow mut b: buffer) -> i64 {
  result := f.pread_into(b, { return 17; 0 }, 1, 0)
  return 0
}
pub fn early(borrow f: file, borrow mut b: buffer, flag: bool) -> Result<i64, Error> {
  if flag { return read(f, b, 0, 1, 0) }
  return read(f, b, 0, 1, 1)
}
"#;
    let main = r#"module main
import std.fs
import std.io
import windows
fn main() -> Result<(), Error> {
  f := fs.open_ro("INPUT")?
  mut b := buffer(16)
  print(b.len()); print(b.capacity())
  print(windows.read(f, b, 0, 4, 8)?)
  io.stdout.write(b.bytes())?; print("|")
  print(windows.read(f, b, 4, 4, windows.scalar(0))?)
  io.stdout.write(b.bytes())?; print("|")
  print(windows.read(f, b, 2, 4, 13)?)
  io.stdout.write(b.bytes())?; print("|")
  print(windows.read(f, b, 8, 5, 16)?)
  print(b.len()); print(b.capacity())
  print(windows.read(f, b, 8, 0, 0)?)
  print(match f.pread_into(b, 9, 0, 0) { Ok(n) => false, Err(e) => true })
  b = buffer.filled(16, 45)
  print(windows.read(f, b, 2, 3, 7)?)
  print(windows.read(f, b, 10, 2, 1)?)
  io.stdout.write(b.bytes())?; print("|")
  print(b.len()); print(b.capacity() >= b.len())
  selected := if true { windows.read(f, b, 0, 1, 0) } else { windows.read(f, b, 0, 1, 1) }
  print(selected?)
  print(match windows.read(f, b, 1, 1, 1) { Ok(n) => n, Err(e) => { return Err(e) } })
  propagated := windows.read(f, b, 2, 1, 2).map_err(fn e: Error { e })?
  print(propagated)
  discarded := windows.read(f, b, 3, 1, 3) else { return Err(Error.Invalid) }
  print(discarded)
  print(windows.early(f, b, true)?)
  mut i := 0
  loop { if i >= 2 { break }; print(windows.read(f, b, i, 1, i)?); i = i + 1 }
  print(f.len()?)
  print(windows.exit_before_io(f, b))
  print(b.len())
  print(f.pread_into(b, { print(20); 0 }, { print(21); 0 }, { print(22); 0 })?)
  return Ok(())
}
"#.replace("INPUT", &path);
    let files = [("main.align", main.as_str()), ("windows.align", helpers)];
    for output in [build_and_run_multi("pread-into-whole", &files, "main.align"),
        build_per_unit_multi("pread-into-unit", &files, "main.align").link_and_run()] {
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout),
            "0\n16\n4\n89AB|\n4\n89AB0123|\n3\n89DEF123|\n0\n8\n16\n0\ntrue\n3\n2\n--789-----12----|\n16\ntrue\n1\n1\n1\n1\n1\n1\n1\n16\n17\n16\n20\n21\n22\n0\n");
    }
    Ok(())
}

#[test]
fn bounded_positional_read_formation_and_loans() {
    let cases = [
        ("arity", "f.pread_into(b, 0, 1)", "4 arguments"),
        ("destination", "f.pread_into(b, true, 1, 0)", "i64"),
        ("length", "f.pread_into(b, 0, true, 0)", "i64"),
        ("offset", "f.pread_into(b, 0, 1, true)", "i64"),
        ("buffer-type", "f.pread_into(0, 0, 1, 0)", "buffer"),
        ("temporary-buffer", "f.pread_into(buffer(16), 0, 1, 0)", "mut buffer local"),
        ("temporary-file", "fs.open_ro(\"input\")?.pread_into(b, 0, 1, 0)", "bind the file"),
        ("undefined", "f.pread_into(b, missing, 1, 0)", "undefined"),
        ("later-buffer-move", "f.pread_into(b, 0, consume_buffer(b), 0)", "borrow"),
        ("later-file-move", "f.pread_into(b, 0, 1, consume_file(f))", "borrow"),
        ("later-buffer-replace", "f.pread_into(b, 0, { b = buffer(16); 1 }, 0)", "borrow"),
        ("legacy-buffer-move", "f.pread(b, consume_buffer(b))", "borrow"),
        ("legacy-file-move", "f.pread(b, consume_file(f))", "borrow"),
        ("legacy-write-file-move", "f.pwrite(\"bytes\", consume_file(f))", "borrow"),
    ];
    for (name, expression, diagnostic) in cases {
        let source = format!("module main\nimport std.fs\nfn consume_buffer(b: buffer) -> i64 = 1\nfn consume_file(f: file) -> i64 = 0\nfn main() -> Result<(), Error> {{ f := fs.open_ro(\"input\")?; mut b := buffer(16); {expression}?; return Ok(()) }}\n");
        let checked = diff_check_multi(&format!("pread-into-{name}"), &[("main.align", source.as_str())], "main.align");
        assert!(checked.whole_errors && checked.per_unit_errors, "{name}: {}\n{}", checked.whole_diags, checked.per_unit_diags);
        assert!(checked.whole_diags.contains(diagnostic) && checked.per_unit_diags.contains(diagnostic), "{name}: {}\n{}", checked.whole_diags, checked.per_unit_diags);
        if name == "undefined" { assert_eq!(checked.whole_diags.matches("error:").count(), 1); assert_eq!(checked.per_unit_diags.matches("error:").count(), 1); }
    }
    for (name, source, diagnostic) in [
        ("immutable", "fn bad(borrow f: file, b: buffer) { f.pread_into(b, 0, 1, 0) }", "immutable"),
        ("shared", "fn bad(borrow f: file, borrow b: buffer) { f.pread_into(b, 0, 1, 0) }", "immutable"),
        ("field", "S { b: buffer }\nfn bad(borrow f: file, borrow mut s: S) { f.pread_into(s.b, 0, 1, 0) }", "mut buffer local"),
        ("old-view", "fn bad(borrow f: file) { mut b := buffer.filled(8, 0); old := b.bytes(); f.pread_into(b, 0, 0, 0); print(old.len()) }", "borrow"),
        ("result", "fn bad(borrow f: file) { mut b := buffer(16); n: bool := f.pread_into(b, 0, 1, 0) }", "type mismatch"),
    ] {
        let source = format!("module main\n{source}\nfn main() {{}}\n");
        let checked = diff_check_multi(&format!("pread-into-{name}"), &[("main.align", source.as_str())], "main.align");
        assert!(checked.whole_errors && checked.per_unit_errors, "{name}: {}\n{}", checked.whole_diags, checked.per_unit_diags);
        assert!(checked.whole_diags.contains(diagnostic) && checked.per_unit_diags.contains(diagnostic), "{name}: {}\n{}", checked.whole_diags, checked.per_unit_diags);
    }
}
