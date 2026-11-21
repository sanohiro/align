//! Literal split field bytes, source lifetime and native header ownership.
mod common;
use common::*;

#[test]
fn literal_fields_in_whole_and_imported_generic_code() {
    if !backend_available() { return; }
    let helpers = r#"module fields
pub fn words<T>(tag:T, text:str, separator:str)->array<str> = text.split(separator)
"#;
    let main = r#"import fields
fn show(fields:array<str>) {
  print(fields.len())
  mut i:=0
  loop {
    if i==fields.len() { break }
    print(fields[i])
    i=i+1
  }
}
fn source()->string { print(101);return "a::界🚀::".clone() }
fn delimiter()->string { print(102);return "::".clone() }
fn keep(word:str)->bool = word != "L"
fn early()->i64 = source().split({return 7;":"}).len()
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
fn pass_error(e:Error)->Error = e
fn choose(which:i64)->Result<array<str>,Error> {
  result:Result<array<str>,Error>:=if which<0 { Err(Error.Invalid) } else { Ok(match which {0=>"a,b".split(","), _=>loop {break "c,d".split(",")} }) }
  return result.map_err(pass_error)
}
fn expect(which:i64)->Result<str,Error> {
  chosen:=choose(which)?
  return Ok(chosen[1])
}
fn lifecycle() {
  print(expect(0) else "missing")
  print(expect(1) else "missing")
  print(expect(-1) else "missing")
  "discarded".split(",")
  print("one,two".split(",").len())
  mut fields:="before".split(",")
  fields="after,replace".split(",")
  print(fields[1])
  arena {
    print("a,b".clone().split(",").where(keep).count())
    copied:="x,y".clone().split(",").to_array()
    print(copied[1])
  }
}
fn exercise() {
  values:=source().split(delimiter())
  show(values)
  print(early())
  show(fields.words(3,"aaaaa","aa"))
  show("é🚀".split(""))
  show("".split(""))
  show("".split(","))
  show("a\0b".split("\0"))
  show("one".split("longer"))
  mut i:=0
  loop {
    if i==2 {break}
    collected:="L,R".clone().split(",").where(keep).to_array()
    print(collected[0])
    i=i+1
  }
  lifecycle()
}
fn main() {
  allocated:=unsafe {align_rt_alloc_count()}
  freed:=unsafe {align_rt_free_count()}
  exercise()
  print(unsafe {align_rt_alloc_count()}-allocated == unsafe {align_rt_free_count()}-freed)
}
"#;
    let files = [("fields.align", helpers), ("main.align", main)];
    let mir = whole_mir_multi("literal-split-mir", &files, "main.align");
    assert!(mir.contains("str_split("), "{mir}");
    for out in [
        build_and_run_multi("literal-split-whole", &files, "main.align"),
        build_per_unit_multi("literal-split-units", &files, "main.align").link_and_run(),
    ] {
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout),
            "101\n102\n3\na\n界🚀\n\n101\n7\n3\n\n\na\n4\n\né\n🚀\n\n2\n\n\n1\n\n2\na\nb\n1\none\nR\nR\nb\nd\nmissing\n2\nreplace\n2\ny\ntrue\n");
    }
}

#[test]
fn split_views_retain_only_source_and_its_validation() {
    let prefix = "owner := [(65 as u8)].to_array(); mut alias: slice<u8> := owner; text := alias.as_str() else { return };";
    let mut cases = Vec::new();
    for (body, error) in [
        ("value := text.split()", true),
        ("value := text.split(1)", true),
        ("value := text.split({ alias[0] = 255; \",\" }); print(value.len())", true),
        ("value := text.split(\",\"); alias[0] = 255; print(value[0])", true),
        ("value := \"abc\".split(text); alias[0] = 255; print(value[0])", false),
        ("value := text.split(\",\"); print(value[0]); alias[0] = 255", false),
    ] {
        cases.push((format!("fn main() {{ {prefix} {body} }}\n"), error));
    }
    cases.push(("fn bad()->array<str> { s:=\"owned\".clone(); return s.split(\",\") }\nfn main() {}\n".into(), true));
    cases.push(("fn bad()->str { s:=\"owned\".clone(); fields:=s.split(\",\"); return fields[0] }\nfn main() {}\n".into(), true));
    cases.push(("fn ok()->array<str> = \"static\".split(\",\")\nfn main() {}\n".into(), false));
    cases.push(("fn bad()->array<str> = \"short\".clone().split(\",\")\nfn main() {}\n".into(), true));
    cases.push(("fn bad()->str {arena a {s:=\"x,y\".clone_in(a); values:=s.split(\",\"); return values[0]}}\nfn main() {}\n".into(), true));
    cases.push(("fn fields(text:str,sep:str)->array<str> = text.split(sep)\nfn good()->array<str> {sep:=\",\".clone(); return fields(\"a,b\",sep)}\nfn main() {}\n".into(), false));
    for (src, error) in cases {
        let result = diff_check_multi("literal-split-check", &[("main.align", &src)], "main.align");
        assert_eq!(result.whole_errors, error, "{src}\n{}", result.whole_diags);
        assert_eq!(result.per_unit_errors, error, "{src}\n{}", result.per_unit_diags);
    }
}
