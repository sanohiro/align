//! `std.xml` owner tests: the exact bounded XML profile and event stream, qualified surface,
//! cursor-scoped views, Move/Drop carriers, per-unit identity, and LLVM/runtime ABI selection.

mod common;
use common::*;

fn documented_first_key(path: &str) -> String {
    let document = std::fs::read_to_string(path).expect("std.xml design document");
    let marker = document.find("fn first_key").expect("first_key example");
    let start = document[..marker]
        .rfind("```align\n")
        .expect("opening Align fence")
        + "```align\n".len();
    let end = document[marker..]
        .find("\n```")
        .map(|offset| marker + offset)
        .expect("closing Align fence");
    document[start..end].to_string()
}

fn documented_http_integration(path: &str) -> String {
    let document = std::fs::read_to_string(path).expect("std.xml design document");
    let marker = document
        .find("response := http.parse(data)?")
        .expect("HTTP integration example");
    let start = document[..marker]
        .rfind("```align\n")
        .expect("opening Align fence")
        + "```align\n".len();
    let end = document[marker..]
        .find("\n```")
        .map(|offset| marker + offset)
        .expect("closing Align fence");
    document[start..end].to_string()
}

#[test]
fn english_and_japanese_designs_share_one_syntax_checked_example() {
    let root = env!("CARGO_MANIFEST_DIR");
    let english = documented_first_key(&format!("{root}/../../docs/impl/std-design/xml.md"));
    let japanese = documented_first_key(&format!("{root}/../../docs/impl/std-design/ja/xml.md"));
    assert_eq!(english, japanese, "the translated public example drifted");
    assert!(
        !check_errs("std-xml-documented-example", &english),
        "the documented std.xml example must type-check",
    );
}

#[test]
fn documented_http_body_conversion_is_synchronized_and_syntax_checked() {
    let root = env!("CARGO_MANIFEST_DIR");
    let english = documented_http_integration(&format!("{root}/../../docs/impl/std-design/xml.md"));
    let japanese =
        documented_http_integration(&format!("{root}/../../docs/impl/std-design/ja/xml.md"));
    assert_eq!(english, japanese, "the translated HTTP example drifted");
    let program = format!(
        "import std.http\nimport std.xml\nfn parse_response(data: slice<u8>) -> Result<(), Error> {{\n{english}\n  return Ok(())\n}}\nfn main() -> i32 = 0\n"
    );
    assert!(
        !check_errs("std-xml-http-integration", &program),
        "the documented HTTP body conversion must type-check",
    );
}

#[test]
fn xml_reader_emits_the_exact_normalized_event_stream() {
    if !backend_available() {
        return;
    }
    let source = r#"import std.xml
fn show(event: xml.event) {
  match event {
    Start => print("Start")
    End => print("End")
    Text => print("Text")
  }
}
pub fn main() -> Result<(), Error> {
  mut doc := xml.parse("<?xml version='1.0'?><!--pre--><a p:x='1\r\n&amp;&#13;' xmlns='u'>left&amp;y&#33;<!--gap--><![CDATA[<&\r]]><b/>right</a>".clone())?

  first := doc.next() else { return Err(Error.Invalid) }
  show(first)
  print(doc.name())
  print(doc.attribute_count())
  print(doc.attribute_name(0))
  print(doc.attribute_value(0))
  print(doc.attribute_name(1))
  print(doc.attribute_value(1))
  retained := doc.name().clone()

  second := doc.next() else { return Err(Error.Invalid) }
  show(second)
  print(doc.text())
  print(retained)

  third := doc.next() else { return Err(Error.Invalid) }
  show(third)
  print(doc.text())

  fourth := doc.next() else { return Err(Error.Invalid) }
  show(fourth)
  print(doc.name())
  print(doc.attribute_count())

  fifth := doc.next() else { return Err(Error.Invalid) }
  show(fifth)
  print(doc.name())

  sixth := doc.next() else { return Err(Error.Invalid) }
  show(sixth)
  print(doc.text())

  seventh := doc.next() else { return Err(Error.Invalid) }
  show(seventh)
  print(doc.name())

  match doc.next() { Some(_) => print("extra"), None => print("EOF") }
  match doc.next() { Some(_) => print("extra"), None => print("EOF") }
  return Ok(())
}
"#;
    let output = build_and_run("std-xml-events", source);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Start\na\n2\np:x\n1 &\r\nxmlns\nu\nText\nleft&y!\na\nText\n<&\n\nStart\nb\n0\nEnd\nb\nText\nright\nEnd\na\nEOF\nEOF\n",
    );
}

#[test]
fn xml_parse_rejects_invalid_documents_without_partial_reader_publication() {
    if !backend_available() {
        return;
    }
    let source = r#"import std.xml
fn invalid(source: string) {
  match xml.parse(source) {
    Ok(_) => print("unexpected")
    Err(_) => print("invalid")
  }
}
pub fn main() {
  invalid("".clone())
  invalid("<a>".clone())
  invalid("<a x='1' x='2'/>".clone())
  invalid("<!DOCTYPE a><a/>".clone())
  invalid("<a>&custom;</a>".clone())
  invalid("<a/><b/>".clone())
}
"#;
    let output = build_and_run("std-xml-invalid", source);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "invalid\ninvalid\ninvalid\ninvalid\ninvalid\ninvalid\n",
    );
}

#[test]
fn xml_parse_authenticates_owned_field_tagged_and_control_producers() {
    if !backend_available() {
        return;
    }
    let source = r#"import core.json
import std.fs
import std.xml
Holder { value: string }
Choice { Text(string), Empty }
Pair { Both(xml.reader, i64), Empty }
JsonRow { value: i64 }
fn keep_error(error: Error) -> Error = error
fn name(reader: xml.reader) -> Result<string, Error> {
  mut current := reader
  event := current.next() else { return Err(Error.Invalid) }
  return Ok(current.name().clone())
}
fn field(holder: Holder) -> Result<xml.reader, Error> = xml.parse(holder.value)
fn optional(value: Option<string>) -> Result<xml.reader, Error> {
  selected := value else { return Err(Error.Invalid) }
  return xml.parse(selected)
}
fn fallible(value: Result<string, Error>) -> Result<xml.reader, Error> {
  selected := value.map_err(keep_error)?
  return xml.parse(selected)
}
fn branch(flag: bool) -> Result<xml.reader, Error> = xml.parse(if flag { "<if/>".clone() } else { "<else/>".clone() })
fn selected(value: Choice) -> Result<xml.reader, Error> {
  source := match value {
    Text(text) => text
    Empty => { return Err(Error.Invalid) }
  }
  return xml.parse(source)
}
fn looped() -> Result<xml.reader, Error> = xml.parse(loop { break "<loop/>".clone() })
fn built() -> Result<xml.reader, Error> {
  output := builder()
  output.write("<builder/>")
  return xml.parse(output.to_string())
}
fn json_generated(row: JsonRow) -> Result<xml.reader, Error> {
  encoded := json.encode(row)
  return xml.parse(encoded.clone())
}
fn file_generated(path: str) -> Result<xml.reader, Error> = xml.parse(fs.read_file(path)?)
fn substring() -> Result<xml.reader, Error> {
  source := "xx<sub/>yy"
  return xml.parse(source[2..8].clone())
}
pub fn main() -> Result<(), Error> {
  print(name(field(Holder { value: "<field/>".clone() })?)?)
  print(name(optional(Some("<option/>".clone()))?)?)
  print(name(fallible(Ok("<result/>".clone()))?)?)
  print(name(branch(true)?)?)
  print(name(selected(Choice.Text("<match/>".clone()))?)?)
  print(name(looped()?)?)
  print(name(built()?)?)
  sibling := "ignored".clone()
  pair := Pair.Both(xml.parse("<sibling/>".clone())?, sibling.len())
  match pair {
    Both(reader, _) => print(name(reader)?)
    Empty => print("empty")
  }
  print(name(xml.parse("  <trim/>  ".trim().clone())?)?)
  print(name(substring()?)?)
  return Ok(())
}
"#;
    let output = build_and_run("std-xml-producer-paths", source);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "field\noption\nresult\nif\nmatch\nloop\nbuilder\nsibling\ntrim\nsub\n",
    );
}

#[test]
fn xml_surface_enforces_import_mutability_bound_receivers_and_cursor_views() {
    for (name, source) in [
        (
            "missing-import",
            "fn main() { doc := xml.parse(\"<a/>\".clone()) }\n",
        ),
        (
            "bare-types",
            "import std.xml\nfn bad(a: reader, b: event) {}\n",
        ),
        (
            "immutable-next",
            "import std.xml\nfn main() -> Result<(), Error> { doc := xml.parse(\"<a/>\".clone())?; event := doc.next(); return Ok(()) }\n",
        ),
        (
            "temporary-next",
            "import std.xml\nfn main() -> Result<(), Error> { event := xml.parse(\"<a/>\".clone())?.next(); return Ok(()) }\n",
        ),
        (
            "view-across-next",
            "import std.xml\nfn main() -> Result<(), Error> { mut doc := xml.parse(\"<a/>\".clone())?; event := doc.next(); name := doc.name(); event2 := doc.next(); print(name); return Ok(()) }\n",
        ),
        (
            "use-after-move",
            "import std.xml\nfn main() -> Result<(), Error> { mut doc := xml.parse(\"<a/>\".clone())?; moved := doc; event := doc.next(); return Ok(()) }\n",
        ),
        (
            "collection",
            "import std.xml\nfn bad(readers: array<xml.reader>) {}\n",
        ),
        (
            "parse-borrowed-input",
            "import std.xml\nfn main() { result := xml.parse(\"<a/>\") }\n",
        ),
        (
            "parse-consumes-input",
            "import std.xml\nfn main() { source := \"<a/>\".clone(); result := xml.parse(source); print(source) }\n",
        ),
        (
            "parse-arity",
            "import std.xml\nfn main() { result := xml.parse() }\n",
        ),
        (
            "next-arity",
            "import std.xml\nfn bad(reader: xml.reader) { event := reader.next(1) }\n",
        ),
        (
            "print",
            "import std.xml\nfn bad(reader: xml.reader) { print(reader) }\n",
        ),
        (
            "equality",
            "import std.xml\nfn bad(a: xml.reader, b: xml.reader) -> bool = a == b\n",
        ),
        (
            "global",
            "import std.xml\nDOC: xml.reader := 0\nfn main() -> i32 = 0\n",
        ),
        (
            "native",
            "import std.xml\nextern \"C\" fn expose(reader: xml.reader) -> i32\nfn main() -> i32 = 0\n",
        ),
        (
            "parallel-capture",
            "import std.xml\nfn inspect(borrow reader: xml.reader) -> i64 = 0\nfn bad(reader: xml.reader) { values := [1, 2].par_map(fn x { inspect(reader) + x }) }\n",
        ),
    ] {
        assert!(
            check_errs(&format!("std-xml-{name}"), source),
            "expected {name} diagnostic"
        );
    }
}

#[test]
fn xml_getters_abort_on_wrong_state_or_index() {
    if !backend_available() {
        return;
    }
    let before_next = build_and_run(
        "std-xml-name-before-next",
        "import std.xml\npub fn main() -> Result<(), Error> { mut doc := xml.parse(\"<a/>\".clone())?; print(doc.name()); return Ok(()) }\n",
    );
    assert!(
        !before_next.status.success(),
        "name before the first event must abort",
    );

    let invalid_index = build_and_run(
        "std-xml-invalid-index",
        "import std.xml\npub fn main() -> Result<(), Error> { mut doc := xml.parse(\"<a/>\".clone())?; event := doc.next(); print(doc.attribute_name(-1)); return Ok(()) }\n",
    );
    assert!(
        !invalid_index.status.success(),
        "a negative attribute index must abort",
    );
}

#[test]
fn xml_name_views_are_capped_by_owned_reader_storage_and_preserve_borrowed_roots() {
    for (name, source) in [
        (
            "direct",
            "import std.xml\nfn bad(reader: xml.reader) -> str = reader.name()\n",
        ),
        (
            "control",
            "import std.xml\nfn bad(reader: xml.reader) -> str { if true { return reader.name() } else { return reader.name() } }\n",
        ),
        (
            "carrier",
            "import std.xml\nView { Name(str), Empty }\nfn bad(reader: xml.reader) -> View = View.Name(reader.name())\n",
        ),
    ] {
        assert!(
            check_errs(&format!("std-xml-view-escape-{name}"), source),
            "an owned reader's {name} view escaped its Drop boundary"
        );
    }

    let borrowed = "import std.xml\npub View { Name(str), Empty }\npub fn direct(borrow reader: xml.reader) -> str = reader.name()\npub fn control(borrow reader: xml.reader) -> str { if true { return reader.name() } else { return reader.name() } }\npub fn carrier(borrow reader: xml.reader) -> View = View.Name(reader.name())\n";
    assert!(
        !check_errs("std-xml-view-borrowed-root", borrowed),
        "a name view rooted in a borrowed reader parameter must remain returnable"
    );
}

#[test]
fn xml_reader_crosses_generic_result_sum_and_per_unit_interfaces() {
    let files = &[
        (
            "xml_support.align",
            "module xml_support\nimport std.xml\npub Carrier<T> { Value(T), Empty }\npub Concrete { Value(xml.reader), Empty }\npub Paired { Both(xml.reader, str), Empty }\npub OwnedChoice { Text(string), Empty }\npub fn source() -> string = \"<branch/>\".clone()\npub fn substring() -> Result<xml.reader, Error> { source := \"xx<unit/>yy\"; return xml.parse(source[2..9].clone()) }\npub fn take_option(value: Option<string>) -> i64 = 1\npub fn take_result(value: Result<string, Error>) -> i64 = 2\npub fn take_choice(value: OwnedChoice) -> i64 = 3\npub fn parse(source: string) -> Result<xml.reader, Error> = xml.parse(source)\npub fn keep<T>(value: T) -> T = value\npub fn carry(reader: xml.reader) -> Carrier<xml.reader> = Carrier.Value(reader)\npub fn carry_concrete(reader: xml.reader) -> Concrete = Concrete.Value(reader)\npub fn paired(reader: xml.reader, borrow peer: xml.reader) -> Paired = Paired.Both(reader, peer.name())\npub fn current_name(borrow reader: xml.reader) -> str = reader.name()\npub fn root_name(reader: xml.reader) -> Result<string, Error> {\n  mut current := reader\n  event := current.next() else { return Err(Error.Invalid) }\n  match event {\n    Start => { return Ok(current.name().clone()) }\n    End => { return Err(Error.Invalid) }\n    Text => { return Err(Error.Invalid) }\n  }\n}\n",
        ),
        (
            "main.align",
            "import std.xml\nimport xml_support\npub fn main() -> Result<(), Error> {\n  reader := xml_support.parse(\"<root/>\".clone())?\n  reader2 := xml_support.keep(reader)\n  match xml_support.carry(reader2) {\n    Value(reader3) => print(xml_support.root_name(reader3)?)\n    Empty => print(\"empty\")\n  }\n  concrete := xml_support.parse(\"<leaf/>\".clone())?\n  match xml_support.carry_concrete(concrete) {\n    Value(reader4) => print(xml_support.root_name(reader4)?)\n    Empty => print(\"empty\")\n  }\n  imported_source := xml.parse(xml_support.source())?\n  print(xml_support.root_name(imported_source)?)\n  print(xml_support.root_name(xml_support.substring()?)?)\n  print(xml_support.take_option(None))\n  print(xml_support.take_result(Err(Error.Invalid)))\n  print(xml_support.take_choice(xml_support.OwnedChoice.Empty))\n  mut peer := xml.parse(\"<peer/>\".clone())?\n  peer_event := peer.next() else { return Err(Error.Invalid) }\n  match xml_support.paired(xml.parse(\"<sum/>\".clone())?, peer) {\n    Both(reader5, _) => {\n      mut sum_reader := reader5\n      sum_event := sum_reader.next() else { return Err(Error.Invalid) }\n      print(sum_reader.name())\n    }\n    Empty => print(\"empty\")\n  }\n  return Ok(())\n}\n",
        ),
    ];
    let differential = diff_check_multi("std-xml-interface", files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    let summary = differential
        .per_unit
        .summaries
        .iter()
        .find(|summary| summary.unit == "xml_support")
        .expect("xml_support summary");
    let parse = summary
        .fns
        .iter()
        .find(|function| function.name == "parse")
        .expect("parse interface");
    assert_eq!(
        parse.ret,
        align_interface::IType::Named {
            path: "Result".to_string(),
            args: vec![
                align_interface::IType::Named {
                    path: "xml.reader".to_string(),
                    args: Vec::new(),
                },
                align_interface::IType::Named {
                    path: "Error".to_string(),
                    args: Vec::new(),
                },
            ],
        },
    );
    let current_name = summary
        .fns
        .iter()
        .find(|function| function.name == "current_name")
        .expect("borrowed XML name interface");
    assert_eq!(
        current_name.return_borrow,
        align_sema::hir::ReturnBorrowSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        },
    );
    assert_eq!(
        current_name.return_region,
        align_sema::hir::ReturnRegionSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        },
    );

    if backend_available() {
        let output =
            build_per_unit_multi("std-xml-interface-run", files, "main.align").link_and_run();
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "root\nleaf\nbranch\nunit\n1\n2\n3\nsum\n"
        );
    }
}

#[test]
fn llvm_selects_only_the_typed_xml_runtime_surface_and_drop() {
    if !backend_available() {
        return;
    }
    let mut sources = SourceMap::new();
    let checked = check(
        &mut sources,
        "std-xml-abi",
        r#"import std.xml
pub fn main() -> Result<(), Error> {
  mut doc := xml.parse("<a x='v'>t</a>".clone())?
  event := doc.next() else { return Err(Error.Invalid) }
  print(doc.name())
  print(doc.attribute_count())
  print(doc.attribute_name(0))
  print(doc.attribute_value(0))
  event2 := doc.next() else { return Err(Error.Invalid) }
  print(doc.text())
  return Ok(())
}
"#,
    );
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags),
    );
    let mir = align_driver::try_lower_to_mir(&checked.hir).expect("checked XML HIR must validate");
    let llvm = emit_llvm_ir(&mir, BuildTarget::Baseline, false, &[], false).expect("LLVM IR");
    for fragment in [
        "switch i32 %xml.parse.status, label %xml.parse.abort",
        "i32 0, label %xml.parse.ok",
        "i32 -1, label %xml.parse.invalid",
        "%xml.parse.cleanup = phi i1 [ true, %xml.parse.ok ], [ false, %xml.parse.invalid ]",
        "switch i32 %xml.next.status, label %xml.next.abort",
        "i32 0, label %xml.next.none",
        "i32 1, label %xml.next.start",
        "i32 2, label %xml.next.end",
        "i32 3, label %xml.next.text",
        "[ { i8 1, %xml.event zeroinitializer }, %xml.next.start ]",
        "[ { i8 1, %xml.event { i32 1 } }, %xml.next.end ]",
        "[ { i8 1, %xml.event { i32 2 } }, %xml.next.text ]",
        "%xml.attribute_count.nonnegative = icmp sge i64 %xml.attribute_count, 0",
        "%xml.attribute_count.bounded = icmp sle i64 %xml.attribute_count, 256",
    ] {
        assert!(
            llvm.contains(fragment),
            "missing atomic XML status/result fragment `{fragment}`:\n{llvm}"
        );
    }
    for operation in [
        "xml_name",
        "xml_attribute_name",
        "xml_attribute_value",
        "xml_text",
    ] {
        assert!(
            llvm.contains(&format!("%{operation}.valid = icmp eq i32 %{operation}.status, 0")),
            "{operation} must accept only zero:\n{llvm}"
        );
        assert!(
            llvm.contains(&format!("{operation}.invalid:")),
            "{operation} must have one invalid-status abort edge:\n{llvm}"
        );
    }
    for symbol in [
        "align_rt_xml_parse",
        "align_rt_xml_next",
        "align_rt_xml_name",
        "align_rt_xml_attribute_count",
        "align_rt_xml_attribute_name",
        "align_rt_xml_attribute_value",
        "align_rt_xml_text",
        "align_rt_xml_free",
    ] {
        assert!(llvm.contains(symbol), "missing {symbol}:\n{llvm}");
    }
}
