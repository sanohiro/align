//! `std.xml` owner tests: the exact bounded XML profile and event stream, qualified surface,
//! cursor-scoped views, Move/Drop carriers, per-unit identity, and LLVM/runtime ABI selection.

mod common;
use common::*;

#[test]
fn producer_certification_preserves_non_xml_container_interfaces() {
    if !backend_available() {
        return;
    }
    let support = r#"module container_support
import core.json
import core.codec
pub fn codec_name(batch: codec.batch) -> Option<str> = batch.name(0)
pub fn codec_text(column: codec.str_column) -> Option<str> = column.at(0)
pub fn captured(value: str) -> str {
  callback := fn { value }
  return callback()
}
fn describe_vector(value: vec4<i32>) -> str = if value.sum() == 12 { "twelve" } else { "wrong" }
pub fn vector_producer() -> string {
  value: vec4<i32> := [1, 2, 3, 4]
  return describe_vector(select(value > 2, value + 2, value - 1)).clone()
}
pub Choice { A(string), B(string) }
pub fn absent() -> string { value: Option<string> := None; return value else "fallback".clone() }
pub fn alternate() -> string {
  value: Result<string, string> := Err("error".clone())
  return match value { Ok(text) => text, Err(error) => error }
}
pub fn selected() -> string {
  value := Choice.A("alpha".clone())
  return match value { A(text) => text, B(text) => text }
}
pub Row { name: str, age: i64 }
pub Carrier { owned: string, view: str }
pub fn consume(value: Carrier) -> i64 = value.owned.len() + value.view.len()
pub fn tail(values: slice<str>) -> str { rest := values[1..]; return rest[0] }
pub Point { key: i64, value: i64 }
fn describe(value: i64) -> str = if value == 2 { "two" } else { "other" }
pub fn transposed() -> string {
  arena {
    rows := [Point { key: 1, value: 2 }].to_soa()
    return describe(rows[0].value).clone()
  }
}
pub fn grouped(data: str) -> Result<string, Error> {
  arena {
    rows: soa<Point> := json.decode(data)?
    groups := rows.group_by(.key).sum(.value)
    return Ok(describe(groups.1[0]).clone())
  }
}
pub fn string_groups(data: str) -> Result<string, Error> {
  arena {
    rows: soa<Row> := json.decode(data)?
    groups := rows.group_by(.name).count()
    return Ok(groups.0[0].clone())
  }
}
pub fn aos_groups(data: str) -> Result<string, Error> {
  rows: array<Row> := json.decode(data)?
  groups := rows.group_by(.name).sum(.age)
  return Ok(groups.0[0].clone())
}
pub fn aos_multi(data: str) -> Result<string, Error> {
  rows: array<Row> := json.decode(data)?
  groups := rows.group_by(.name).agg(sum(.age), max(.age), count())
  return Ok(groups.0[0].clone())
}
pub fn aos_encoded(data: str) -> Result<string, Error> {
  rows: array<Row> := json.decode(data)?
  encoded := rows.dict_encode(.name)
  groups := encoded.group_by(.name).sum(.age)
  return Ok(groups.0[0].clone())
}
pub fn transposed_strings() -> string {
  arena {
    rows := [Row { name: "text", age: 1 }].to_soa()
    return rows[0].name.clone()
  }
}
fn length(row: Row) -> i64 = row.name.len()
pub fn summarize(data: str) -> Result<i64, Error> {
  rows: array<Row> := json.decode(data)?
  mapped := rows.map(length).sum()
  arena {
    columns: soa<Row> := json.decode(data)?
    names := columns.name
    copied := columns.to_array()
    return Ok(mapped + names[0].len() + copied[0].name.len())
  }
}
"#;
    let main = r#"import container_support
fn main() -> Result<(), Error> {
  owner := "view".clone()
  view: str := owner
  print(container_support.consume(container_support.Carrier { owned: "owned".clone(), view: view }))
  values := ["first", "second"]
  print(container_support.tail(values))
  print(container_support.summarize("[{\"name\":\"abc\",\"age\":1}]")?)
  print(container_support.transposed())
  print(container_support.grouped("[{\"key\":1,\"value\":2}]")?)
  print(container_support.string_groups("[{\"name\":\"abc\",\"age\":1}]")?)
  print(container_support.transposed_strings())
  print(container_support.absent())
  print(container_support.alternate())
  print(container_support.selected())
  print(container_support.captured(view))
  print(container_support.vector_producer())
  print(container_support.aos_groups("[{\"name\":\"abc\",\"age\":1}]")?)
  print(container_support.aos_multi("[{\"name\":\"abc\",\"age\":1}]")?)
  print(container_support.aos_encoded("[{\"name\":\"abc\",\"age\":1}]")?)
  return Ok(())
}
"#;
    let files = &[("container_support.align", support), ("main.align", main)];
    let whole = build_and_run_multi("producer-container-whole", files, "main.align");
    assert_eq!(whole.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&whole.stdout),
        "9\nsecond\n9\ntwo\ntwo\nabc\ntext\nfallback\nerror\nalpha\nview\ntwelve\nabc\nabc\nabc\n"
    );
    let output =
        build_per_unit_multi("producer-container-interface", files, "main.align").link_and_run();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "9\nsecond\n9\ntwo\ntwo\nabc\ntext\nfallback\nerror\nalpha\nview\ntwelve\nabc\nabc\nabc\n"
    );
}

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
fn producer_certification_preserves_generated_streaming_decoder_observations() {
    if !backend_available() { return; }
    let resource = r#"module pkg.db.internal.resource
pub fn drop_rows(state: raw) { unsafe { if !state.is_null() { raw.free(state) } } }
"#;
    let db = r#"module pkg.db
import pkg.db.internal.resource
pub BorrowedRow { value: str }
pub resource rows<R> = pkg.db.internal.resource.drop_rows
"#;
    let sqlite = r#"module pkg.db.internal.sqlite
import pkg.db
import pkg.db.internal.descriptor
pub fn decode_current(borrow mut stream: pkg.db.rows<pkg.db.BorrowedRow>, context: raw) -> pkg.db.BorrowedRow {
  unsafe {
    reference := resource.borrow(stream)
    return pkg.db.internal.descriptor.decode_current_row(reference, context)
  }
}
"#;
    let files = &[
        ("pkg/db/internal/resource.align", resource),
        ("pkg/db/internal/descriptor.align", "module pkg.db.internal.descriptor\n"),
        ("pkg/db/internal/sqlite.align", sqlite),
        ("pkg/db.align", db),
        ("pkg/db/api.align", "module pkg.db.api\nimport pkg.db\nimport pkg.db.internal.sqlite\npub fn exercise(run: bool) { if run { unsafe { state := raw.alloc(1); mut stream: pkg.db.rows<pkg.db.BorrowedRow> := resource.from_raw(state); row := pkg.db.internal.sqlite.decode_current(stream, state); print(row.value) } } }\n"),
        ("main.align", "import pkg.db.api\nfn main() -> i32 { pkg.db.api.exercise(false); return 0 }\n"),
    ];
    let llvm = emit_llvm_multi("streaming-decoder-producer", files, "main.align");
    assert!(llvm.contains("call %\"pkg.db$BorrowedRow\" %rawptrval("),
        "the generated indirect row decoder must be emitted");
    let output = build_per_unit_multi("streaming-decoder-interface", files, "main.align")
        .link_and_run();
    assert_eq!(output.status.code(), Some(0));
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
    let diagnostics = check_diagnostics("std-xml-documented-example", &english);
    assert!(
        diagnostics.is_empty(),
        "the documented std.xml example must type-check: {diagnostics}"
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
fn describe(error: Error) -> str = match error { Invalid => "invalid", _ => "unexpected" }
fn invalid(source: string) -> string {
  return match xml.parse(source) {
    Ok(_) => "unexpected".clone()
    Err(error) => describe(error).clone()
  }
}
pub fn main() {
  print(invalid("".clone()))
  print(invalid("<a>".clone()))
  print(invalid("<a x='1' x='2'/>".clone()))
  print(invalid("<!DOCTYPE a><a/>".clone()))
  print(invalid("<a>&custom;</a>".clone()))
  print(invalid("<a/><b/>".clone()))
}
"#;
    for output in [
        build_and_run("std-xml-invalid", source),
        build_per_unit_multi("std-xml-invalid-interface", &[("main.align", source)], "main.align")
            .link_and_run(),
    ] {
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "invalid\ninvalid\ninvalid\ninvalid\ninvalid\ninvalid\n",
        );
    }
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
fn xml_reader_actions_reserve_exact_observations_through_eager_indices() {
    let helpers = r#"import std.xml
fn discard(reader: xml.reader) -> i64 = 0
fn step(borrow mut reader: xml.reader) -> i64 { reader.next(); return 0 }
fn name(borrow reader: xml.reader) -> str = reader.name()
fn fresh(borrow mut reader: xml.reader) -> str { reader.next(); reader.next(); return reader.name() }
fn noop(borrow mut reader: xml.reader) {}
fn show(value: str, index: i64) { print(value) }
Holder { first: xml.reader, second: xml.reader, values: array<i64> }
fn discard_holder(holder: Holder) -> i64 = 0
"#;
    for getter in ["attribute_name", "attribute_value"] {
        for index in [
            "discard(doc)",
            "{ moved := doc; 0 }",
            "step(doc)",
            "{ doc.next(); 0 }",
            "{ doc = xml.parse(\"<b/>\".clone())?; 0 }",
            "{ call := discard; call(doc) }",
        ] {
            let source = format!(
                "{helpers}\nfn main() -> Result<(), Error> {{ mut doc := xml.parse(\"<a x='v'/>\".clone())?; doc.next(); value := doc.{getter}({index}); return Ok(()) }}"
            );
            let diagnostics = check_diagnostics("xml-reserved-reader", &source);
            assert!(
                diagnostics.contains("invalidated") || diagnostics.contains("moved"),
                "{getter}({index}) must reject the invalidating index: {diagnostics}"
            );
        }
    }
    for body in [
        "old := doc.name(); doc.next(); print(old)",
        "old := name(doc); step(doc); print(old)",
        "old := fresh(doc); discard(doc); print(old)",
        "old := fresh(doc); doc.next(); print(old)",
        "old := doc.name(); if true { event := doc.next() }; print(old)",
        "old := doc.name(); loop { doc.next(); break }; print(old)",
        "old := doc.name(); if true { doc = xml.parse(\"<c/>\".clone())? }; print(old)",
        "show(doc.name(), step(doc))",
        "call := show; call(doc.name(), step(doc))",
        "show(if true { doc.name() } else { \"\" }, step(doc))",
    ] {
        let source = format!(
            "{helpers}\nfn main() -> Result<(), Error> {{ mut doc := xml.parse(\"<a><b/></a>\".clone())?; doc.next(); {body}; return Ok(()) }}"
        );
        let diagnostics = check_diagnostics("xml-old-observation", &source);
        assert!(diagnostics.contains("invalidated"), "{body}: {diagnostics}");
    }
    for body in [
        "doc.next(); doc.next(); print(doc.name())",
        "owned := doc.attribute_value(0); doc.next(); print(owned)",
        "old := doc.name(); noop(doc); print(old)",
        "print(fresh(doc))",
        "old := doc.name(); if true { doc = xml.parse(\"<c/>\".clone())? }; print(doc.name())",
        "old := doc.name(); if true { event := doc.next() }; print(doc.name())",
        "loop { doc.next(); if true { break }; doc.next() }; print(doc.name())",
        "doc.attribute_value({ return Ok(()); 0 })",
    ] {
        let source = format!(
            "{helpers}\nfn main() -> Result<(), Error> {{ mut doc := xml.parse(\"<a x='v'><b/></a>\".clone())?; doc.next(); {body}; return Ok(()) }}"
        );
        let diagnostics = check_diagnostics("xml-fresh-observation", &source);
        assert!(diagnostics.is_empty(), "{body}: {diagnostics}");
    }
    for (action, accepted) in [("discard_holder(holder)", false), ("step(peer)", true)] {
        let source = format!(
            "{helpers}\nfn main() -> Result<(), Error> {{ mut peer := xml.parse(\"<p/>\".clone())?; holder := Holder {{ first: xml.parse(\"<a/>\".clone())?, second: xml.parse(\"<b/>\".clone())?, values: [1].to_array() }}; old := name(holder.first); {action}; print(old); return Ok(()) }}"
        );
        let diagnostics = check_diagnostics("xml-projected-observation", &source);
        assert_eq!(diagnostics.is_empty(), accepted, "{action}: {diagnostics}");
    }
    for (body, accepted) in [
        ("old := reader.name(); reader.next(); print(old)", false),
        ("reader.next(); reader.next(); print(reader.name())", true),
        ("old := fresh(reader); step(reader); print(old)", false),
        ("print(fresh(reader)); print(fresh(reader))", true),
    ] {
        let source = format!("{helpers}\nfn inspect(borrow mut reader: xml.reader) {{ {body} }}");
        let diagnostics = check_diagnostics("xml-borrowed-observation", &source);
        assert_eq!(diagnostics.is_empty(), accepted, "{body}: {diagnostics}");
    }
}

#[test]
fn xml_advanced_borrowed_readers_keep_interface_roots_and_owned_getter_results() {
    if !backend_available() {
        return;
    }
    let support = r#"module reader_support
import std.xml
pub fn next_name(borrow mut reader: xml.reader) -> str {
  reader.next()
  return reader.name()
}
pub fn twice(borrow mut reader: xml.reader) -> str {
  first := next_name(reader)
  return next_name(reader)
}
"#;
    let main = r#"import std.xml
import reader_support
fn main() -> Result<(), Error> {
  mut reader := xml.parse("<root x='value'><leaf/></root>".clone())?
  reader.next()
  value := reader.attribute_value(0)
  print(reader_support.twice(reader))
  print(reader_support.next_name(reader))
  print(value)
  return Ok(())
}
"#;
    let files = &[("reader_support.align", support), ("main.align", main)];
    let checked = diff_check_multi("xml-advanced-reader-summary", files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole: {}\nper-unit: {}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    let summary = checked
        .per_unit
        .summaries
        .iter()
        .find(|summary| summary.unit == "reader_support")
        .expect("reader support summary");
    for function in summary
        .fns
        .iter()
        .filter(|function| matches!(function.name.as_str(), "next_name" | "twice"))
    {
        assert_eq!(
            function.return_borrow,
            align_sema::hir::ReturnBorrowSummary::Roots {
                params: vec![0],
                captures: vec![],
            }
        );
        assert_eq!(
            function.return_region,
            align_sema::hir::ReturnRegionSummary::Roots {
                params: vec![0],
                captures: vec![],
            }
        );
    }
    let invalid = main
        .replace(
            "print(reader_support.twice(reader))",
            "old := reader_support.twice(reader)",
        )
        .replace("print(value)", "print(value); print(old)");
    let rejected = diff_check_multi(
        "xml-advanced-reader-expired",
        &[("reader_support.align", support), ("main.align", &invalid)],
        "main.align",
    );
    assert!(
        rejected.whole_errors
            && rejected.per_unit_errors
            && rejected.whole_diags.contains("invalidated")
            && rejected.per_unit_diags.contains("invalidated"),
        "whole: {}\nper-unit: {}",
        rejected.whole_diags,
        rejected.per_unit_diags
    );
    for output in [
        build_and_run_multi("xml-advanced-reader-whole", files, "main.align"),
        build_per_unit_multi("xml-advanced-reader-units", files, "main.align").link_and_run(),
    ] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "leaf\nroot\nvalue\n"
        );
    }
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
            llvm.contains(&format!(
                "%{operation}.valid = icmp eq i32 %{operation}.status, 0"
            )),
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
