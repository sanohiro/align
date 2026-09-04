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
fn xml_reader_crosses_generic_result_sum_and_per_unit_interfaces() {
    let files = &[
        (
            "xml_support.align",
            "module xml_support\nimport std.xml\npub Carrier<T> { Value(T), Empty }\npub fn parse(source: string) -> Result<xml.reader, Error> = xml.parse(source)\npub fn keep<T>(value: T) -> T = value\npub fn carry(reader: xml.reader) -> Carrier<xml.reader> = Carrier.Value(reader)\npub fn root_name(reader: xml.reader) -> Result<string, Error> {\n  mut current := reader\n  event := current.next() else { return Err(Error.Invalid) }\n  match event {\n    Start => { return Ok(current.name().clone()) }\n    End => { return Err(Error.Invalid) }\n    Text => { return Err(Error.Invalid) }\n  }\n}\n",
        ),
        (
            "main.align",
            "import std.xml\nimport xml_support\npub fn main() -> Result<(), Error> {\n  reader := xml_support.parse(\"<root/>\".clone())?\n  reader2 := xml_support.keep(reader)\n  match xml_support.carry(reader2) {\n    Value(reader3) => print(xml_support.root_name(reader3)?)\n    Empty => print(\"empty\")\n  }\n  return Ok(())\n}\n",
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

    if backend_available() {
        let output =
            build_per_unit_multi("std-xml-interface-run", files, "main.align").link_and_run();
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "root\n");
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
