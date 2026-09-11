//! Retained owner leaves share the existing indexed-place borrowing contract.
mod common;
use common::*;

const HELPER: &str = r#"module helper
import std.fs
pub Pending<T> { directory: fs.directory, cursor: fs.dir_cursor, path: string, marker: T }
pub fn observe(borrow directory: fs.directory) -> Result<i64,Error> {
  metadata := directory.metadata()?
  return Ok(1)
}
pub fn cursor_marker(borrow cursor: fs.dir_cursor) -> i64 = 2
pub fn inspect<T>(borrow item: Pending<T>) -> Result<i64,Error> {
  return Ok(observe(item.directory)? + cursor_marker(item.cursor))
}
pub fn text<T>(borrow item: Pending<T>) -> str { value: str := item.path; return value }
pub fn maybe(borrow item: Option<Pending<i64>>) -> i64 {
  return match item { Some(value) => cursor_marker(value.cursor), None => 0 }
}
pub fn inspect_i64(borrow item: Pending<i64>) -> Result<i64,Error> = inspect(item)
pub fn make() -> Result<array<Pending<i64>>,Error> {
  directory := fs.open_directory(".")?
  cursor := directory.cursor()?
  mut builder: array_builder<Pending<i64>> := array_builder()
  builder.push(Pending { directory: directory, cursor: cursor, path: "retained".clone(), marker: 1 })
  return Ok(builder.build())
}
"#;

#[test]
fn indexed_shared_owners_and_returned_views() {
    let main=r#"import helper
import std.fs
fn main() -> Result<(),Error> {
  rows := helper.make()?
  view: slice<helper.Pending<i64>> := rows
  print(helper.inspect(view[0])?)
  print(helper.text(rows[0]))
  again := helper.inspect_i64
  print(again(view[0])?)
  print(helper.text(view[0]))
  directory := fs.open_directory(".")?
  cursor := directory.cursor()?
  optional := Some(helper.Pending { directory: directory, cursor: cursor, path: "optional".clone(), marker: 1 })
  print(helper.maybe(optional))
  return Ok(())
}
"#;
    let files=&[("helper.align",HELPER),("main.align",main)];
    let checked=diff_check_multi("fs-indexed",files,"main.align");
    assert!(!checked.whole_errors && !checked.per_unit_errors,"{} {}",checked.whole_diags,checked.per_unit_diags);
    if backend_available() {
        for unit in [false,true] {
            let out=if unit { build_per_unit_multi("fs-indexed-unit",files,"main.align").link_and_run() } else { build_and_run_multi("fs-indexed-whole",files,"main.align") };
            assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
            assert_eq!(out.stdout,b"3\nretained\n3\nretained\n2\n");
        }
    }
}

#[test]
fn shared_retained_owners_do_not_grant_exclusive_or_escaping_authority() {
    for source in [
        "import std.fs\nfn bad(borrow c: fs.dir_cursor) -> Result<Option<fs.dir_entry>,Error> = c.next()\nfn main() {}",
        "import std.fs\nRow { directory: fs.directory }\nfn consume(row: Row) {}\nfn bad(rows: slice<Row>) { consume(rows[0]) }\nfn main() {}",
    ] {
        let checked=diff_check_multi("fs-indexed-invalid",&[("main.align",source)],"main.align");
        assert!(checked.whole_errors && checked.per_unit_errors,"accepted {source}");
    }
}

#[test]
fn retained_record_view_escape_reaches_lifetime_check() {
    let source = r#"import std.fs
Row { directory: fs.directory, text: string }
fn text(borrow row: Row) -> str { value: str := row.text; return value }
fn observe() -> Result<RETURN_TYPE,Error> {
  rows := [Row { directory: fs.open_directory(".")?, text: "local".clone() }]
  view: slice<Row> := rows
  FINISH
}
fn main() {}
"#;
    let local = source.replace("RETURN_TYPE", "()")
        .replace("FINISH", "print(text(view[0])); return Ok(())");
    let good = diff_check_multi("fs-view-local", &[("main.align", &local)], "main.align");
    assert!(!good.whole_errors && !good.per_unit_errors, "{} {}", good.whole_diags, good.per_unit_diags);
    let escaping = source.replace("RETURN_TYPE", "str")
        .replace("FINISH", "return Ok(text(view[0]))");
    let bad = diff_check_multi("fs-view-escape", &[("main.align", &escaping)], "main.align");
    for diagnostics in [&bad.whole_diags, &bad.per_unit_diags] {
        assert!(diagnostics.contains("cannot return a view that borrows local storage"), "{diagnostics}");
    }
}
