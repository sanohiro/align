//! Plan242: retained read-only views of indexed inline and owning array fields.
mod common;
use common::*;

#[path = "helpers/indexed_array_field.rs"]
mod indexed_array_field;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

fn verdict(label: &str, source: &str, rejected: bool) {
    let result = diff_check_multi(label, &[("main.align", source)], "main.align");
    assert_eq!(result.whole_errors, rejected, "{source}\n{}", result.whole_diags);
    assert_eq!(result.per_unit_errors, rejected, "{source}\n{}", result.per_unit_diags);
}

#[test]
fn formation_and_values() {
    for field in ["[i64; 2]", "array<i64>"] {
        let init = if field.starts_with('[') { "[7, 12]" } else { "[7, 12].to_array()" };
        let source = format!("Row {{ data: {field} }}\nfn view(borrow rows: array<Row>, index: i64) -> slice<i64> = rows[index].data[..]\nfn read(v: slice<i64>) -> i64 = v[0] + v[1]\nfn main() {{ mut b: array_builder<Row> := array_builder(); b.push(Row {{ data: {init} }}); rows := b.build(); print(read(view(rows, 0))) }}");
        verdict("indexed-field-view-formation", &source, false);
        for parent in ["borrow rows: [Row; 1]", "rows: slice<Row>", "borrow rows: array<Row>"] {
            verdict("indexed-field-view-parent", &format!("Row {{ data: {field} }}\nfn view({parent}, index: i64) -> slice<i64> = rows[index].data[..]\nfn main() {{}}"), false);
        }
    }
    for element in ["str", "string", "slice<i64>"] {
        verdict("indexed-field-view-elements", &format!("Row {{ data: [{element}; 0] }}\nfn view(borrow rows: [Row; 1]) {{ value := rows[0].data[..] }}\nfn main() {{}}"), true);
    }
    for expr in ["fresh()[0].data[..]", "[Row { data: [1, 2] }][0].data[..]"] {
        verdict("indexed-field-view-temporary", &format!("Row {{ data: [i64; 2] }}\nfn fresh() -> [Row; 1] = [Row {{ data: [1, 2] }}]\nfn main() {{ view := {expr}; print(view.len()) }}"), true);
    }
    for count in [0, 2] {
        let init = if count == 0 { "[]" } else { "[7, 12]" };
        verdict("indexed-field-view-inline-escape", &format!("Row {{ data: [i64; {count}] }}\nfn escape() -> slice<i64> {{ rows := [Row {{ data: {init} }}]; rows[0].data[..] }}\nfn main() {{}}"), true);
    }
}

#[test]
fn inline_parent_replacement() {
    owned_fixture::run("inline_parent_replacement", |stage| {
        indexed_array_field::execute(stage, "Row { data: [i64; 2] }\nfn read(view: slice<i64>) -> i64 = view[0] + view[1]\nfn main() -> i32 { mut rows := [Row { data: [1, 2] }]; view := rows[0].data[..]; rows = [Row { data: [3, 4] }]; if view[0] == 3 && view[1] == 4 && read(view) == 7 { 0 } else { 1 } }", "inline").unwrap();
    });
    for (action, rejected) in [("rows = [Row { data: [3, 4] }]", false), ("", false)] {
        verdict("indexed-field-view-inline-replace", &format!("Row {{ data: [i64; 2] }}\nfn main() {{ mut rows := [Row {{ data: [1, 2] }}]; view := rows[0].data[..]; {action}; print(view[0]) }}"), rejected);
    }
}

#[test]
fn validation_and_disjoint_fields() {
    let prefix = "Row { bytes: [u8; 2] }\nfn view(rows: slice<Row>) -> slice<u8> = rows[0].bytes[..]\n";
    // A helper with a different input/result element shape has unresolved returned byte
    // backing under the existing summary contract. Its validations remain conservative.
    for selected in ["rows[0].bytes[..]", "view(rows)"] {
        for (target, rejected) in [("rows", true), ("other", selected == "view(rows)")] {
            verdict("indexed-view-byte-validation", &format!("{prefix}fn main() {{ mut rows := [Row {{ bytes: [65 as u8, 66 as u8] }}]; mut other := [Row {{ bytes: [67 as u8, 68 as u8] }}]; bytes := {selected}; text := bytes.as_str() else {{ return }}; {target} = [Row {{ bytes: [255 as u8, 0 as u8] }}]; print(text) }}"), rejected);
        }
        for writer in ["bytes.fill(0)", "bytes.set_u8(0, 0)", "bytes.copy_from(source.bytes())"] {
            verdict("indexed-view-native-writer", &format!("{prefix}fn main() {{ rows := [Row {{ bytes: [65 as u8, 66 as u8] }}]; source := buffer.filled(2, 0); mut bytes := {selected}; {writer} }}"), true);
        }
    }
    let prefix = "Row { data: array<i64> }\nOuter { rows: array<Row>, other: array<i64> }\nfn rows() -> array<Row> { mut b: array_builder<Row> := array_builder(); b.push(Row { data: [1, 2].to_array() }); b.build() }\nfn view(borrow outer: Outer) -> slice<i64> = outer.rows[0].data[..]\n";
    for selected in ["outer.rows[0].data[..]", "view(outer)"] {
        for (action, rejected) in [("outer.rows = rows()", true), ("outer.other = [3, 4].to_array()", selected == "view(outer)")] {
            verdict("indexed-view-sibling-replacement", &format!("{prefix}fn main() {{ mut outer := Outer {{ rows: rows(), other: [7, 8].to_array() }}; value := {selected}; {action}; print(value.len()) }}"), rejected);
        }
    }
}

#[test]
fn inline_validation_writes() {
    for statement in ["bytes = replacement", "ignored := { bytes = replacement; 0 }"] {
        verdict("inline-validation-replacement", &format!("fn main() {{ mut bytes := [65 as u8, 66 as u8]; replacement := [255 as u8, 0 as u8]; alias: slice<u8> := bytes; text := alias.as_str() else {{ return }}; {statement}; print(text) }}"), true);
    }
    for target in ["outer.bytes", "outer.other"] {
        for replacement in [format!("{target} = [255 as u8, 0 as u8]"), format!("ignored := {{ {target} = [255 as u8, 0 as u8]; 0 }}")] {
            verdict("inline-validation-field", &format!("Outer {{ bytes: [u8; 2], other: [u8; 2] }}\nfn main() {{ mut outer := Outer {{ bytes: [65 as u8, 66 as u8], other: [67 as u8, 68 as u8] }}; bytes: slice<u8> := outer.bytes; text := bytes.as_str() else {{ return }}; {replacement}; print(text) }}"), target == "outer.bytes");
        }
    }
    // Descriptor rebinding and copying inline data leave the old backing untouched.
    for action in ["alias = other", "mut copy := bytes; copy = [255 as u8, 0 as u8]", "bytes = bytes"] {
        verdict("inline-validation-preserved", &format!("fn main() {{ mut bytes := [65 as u8, 66 as u8]; other := [255 as u8, 0 as u8]; mut alias: slice<u8> := bytes; text := alias.as_str() else {{ return }}; {action}; print(text) }}"), false);
    }
}

#[test]
fn returned_candidate_closure() {
    let provider = r#"module views
pub Row { data: array<i64> }
pub Outer { rows: array<Row>, other: array<i64> }
pub Holder { view: slice<i64> }
pub fn make() -> Outer {
 mut b: array_builder<Row> := array_builder()
 b.push(Row { data: [7, 12].to_array() })
 Outer { rows: b.build(), other: [19, 23].to_array() }
}
pub fn read<T>(borrow outer: Outer, marker: T) -> slice<i64> = outer.rows[0].data[..]
pub fn result(borrow outer: Outer) -> Result<Holder, Error> = Ok(Holder { view: outer.rows[0].data[..] })
pub fn parts(borrow outer: Outer) -> array<slice<i64>> = outer.rows[0].data[..].chunks(1)
pub fn error() -> Result<(), i64> = Err(0)
pub fn publish(borrow mut target: slice<i64>, borrow outer: Outer) { target = outer.rows[0].data[..] }
pub fn publish_holder(borrow mut target: Holder, borrow outer: Outer) { target.view = outer.rows[0].data[..] }
pub Bytes { numbers: array<i64>, other: array<u8> }
pub fn bytes(borrow outer: Bytes) -> slice<u8> { values: slice<i64> := outer.numbers; values.as_bytes() }
pub Flat { data: array<i64> }
pub fn flat(borrow value: Flat) -> slice<i64> = value.data[..]
"#;
    for (setup, use_value) in [
        ("value := views.read(outer, true)", "value"),
        ("value := views.result(outer) else { return }", "value.view"),
        ("value := views.result(outer).map_err(fn error: Error { error }) else { return }", "value.view"),
        ("parts := views.parts(outer); value := parts[0]", "value"),
        ("part := outer.rows[..]; other := outer.other[..]; mapped := views.error().map_err(fn n: i64 { if n == 0 { views.Holder { view: part[0].data[..] } } else { views.Holder { view: other } } }); value := match mapped { Err(held) => held.view, Ok(_) => { return } }", "value"),
        ("initial := [0]; mut value: slice<i64> := initial; views.publish(value, outer)", "value"),
        ("initial := [0]; mut value := views.Holder { view: initial[..] }; views.publish_holder(value, outer)", "value.view"),
    ] {
        for (action, rejected) in [("", false), ("outer = views.make()", true),
            ("outer.rows = views.make().rows", true), ("unrelated = views.make()", false)] {
            // A separate helper makes the replacement's array ownership explicit without
            // asking for a new owning field extraction from a temporary expression.
            let action = action.replace("views.make().rows", "replacement()");
            let source = format!("import views\nfn replacement() -> array<views.Row> {{ mut b: array_builder<views.Row> := array_builder(); b.push(views.Row {{ data: [0].to_array() }}); b.build() }}\nfn main() {{ mut outer := views.make(); mut unrelated := views.make(); {setup}; {action}; print({use_value}.len()) }}");
            let checked = diff_check_multi("indexed-return-candidates", &[("views.align", provider), ("main.align", &source)], "main.align");
            assert_eq!(checked.whole_errors, rejected, "{source}\n{}", checked.whole_diags);
            assert_eq!(checked.per_unit_errors, rejected, "{source}\n{}", checked.per_unit_diags);
        }
    }
    for (action, rejected) in [("", false), ("outer.numbers = [0].to_array()", true), ("unrelated = [0].to_array()", false)] {
        let source = format!("import views\nfn main() {{ mut outer := views.Bytes {{ numbers: [7].to_array(), other: [0 as u8].to_array() }}; mut unrelated := [1].to_array(); value := views.bytes(outer); {action}; print(value.len()) }}");
        let checked = diff_check_multi("typed-return-candidates", &[("views.align", provider), ("main.align", &source)], "main.align");
        assert_eq!(checked.whole_errors, rejected, "{source}\n{}", checked.whole_diags);
        assert_eq!(checked.per_unit_errors, rejected, "{source}\n{}", checked.per_unit_diags);
    }
    for (action, rejected) in [("", false), ("outer.data = [0].to_array()", true), ("unrelated.data = [0].to_array()", false)] {
        let source = format!("import views\nfn main() {{ mut outer := views.Flat {{ data: [7].to_array() }}; mut unrelated := views.Flat {{ data: [9].to_array() }}; value := views.flat(outer); {action}; print(value[0]) }}");
        let checked = diff_check_multi("closed-return-candidates", &[("views.align", provider), ("main.align", &source)], "main.align");
        assert_eq!(checked.whole_errors, rejected, "{source}\n{}", checked.whole_diags);
        assert_eq!(checked.per_unit_errors, rejected, "{source}\n{}", checked.per_unit_diags);
    }
}

#[test]
fn carriers_captures_and_joins() {
    let prefix = "Row { data: array<i64> }\nHolder { view: slice<i64> }\nfn fresh() -> array<Row> { mut b: array_builder<Row> := array_builder(); b.push(Row { data: [7, 12].to_array() }); b.build() }\nfn consume(rows: array<Row>) {}\n";
    for selected in [
        "if flag { rows[0].data[..] } else { other[0].data[..] }",
        "match (if flag { 0 } else { 1 }) { 0 => rows[0].data[..], _ => other[0].data[..] }",
        "{ rows[0].data[..] }",
    ] {
        for (action, rejected) in [("", false), ("consume(rows)", true)] {
            verdict("indexed-view-control-result", &format!("{prefix}fn inspect(flag: bool) {{ rows := fresh(); other := fresh(); view := {selected}; read := fn ignored: i64 {{ view[0] }}; {action}; print(read(0)) }}\nfn main() {{ inspect(true) }}"), rejected);
        }
        verdict("indexed-view-capture-write", &format!("{prefix}fn inspect(flag: bool) {{ rows := fresh(); other := fresh(); view := {selected}; read := fn ignored: i64 {{ mut alias := view; alias[0] = 0; alias[0] }}; print(read(0)) }}\nfn main() {{}}"), true);
    }
    for expression in [
        "rows[0].data[(Some(0) else { return Ok(0) })..]",
        "rows[0].data[zero()?..]",
        "rows[0].data[zero().map_err(fn e: Error { e })?..]",
        "rows[0].data[(loop { break 0 })..]",
    ] {
        verdict("indexed-view-control-bound", &format!("{prefix}fn zero() -> Result<i64, Error> = Ok(0)\nfn inspect() -> Result<i64, Error> {{ rows := fresh(); view := {expression}; Ok(view[0]) }}\nfn main() {{}}"), false);
    }
    verdict("indexed-view-loop-expiry", &format!("{prefix}fn main() {{ mut rows := fresh(); view := rows[0].data[..]; mut count := 0; loop {{ if count == 2 {{ break }}; rows = fresh(); count = count + 1 }}; print(view.len()) }}"), true);
}

#[test]
fn lifetime_and_authority() {
    for field in ["[i64; 2]", "array<i64>"] {
        let init = if field.starts_with('[') { "[7, 12]" } else { "[7, 12].to_array()" };
        let prefix = format!("Row {{ data: {field} }}\nHolder {{ view: slice<i64> }}\nfn fresh() -> array<Row> {{ mut b: array_builder<Row> := array_builder(); b.push(Row {{ data: {init} }}); b.build() }}\nfn consume(rows: array<Row>) {{}}\nfn replace(borrow mut rows: array<Row>) {{ rows = fresh() }}\nfn writer(borrow mut view: slice<i64>) {{ view[0] = 0 }}\nfn returned(borrow rows: array<Row>) -> slice<i64> = rows[0].data[..]\n");
        for selected in ["rows[0].data[..]", "returned(rows)"] {
            for (action, tail, rejected) in [
                ("", "print(view[0])", false),
                ("view[0] = 0", "", true),
                ("writer(view)", "", true),
                ("rows = fresh()", "print(view.len())", true),
                ("consume(rows)", "print(view.len())", true),
                ("replace(rows)", "print(view.len())", true),
                ("other = fresh()", "print(view[0])", false),
                ("replace(other)", "print(view[0])", false),
                ("copy := view[0]; consume(rows)", "print(copy)", false),
            ] {
                let source = format!("{prefix}fn main() {{ mut rows := fresh(); mut other := fresh(); mut view := {selected}; {action}; {tail} }}");
                verdict("indexed-field-view-lifetime", &source, rejected);
            }
            for (carry, use_view) in [
                ("holder := Holder { view: view }", "holder.view"),
                ("part := view[0..1]", "part"),
            ] {
                for (action, tail, rejected) in [
                    ("", "print(alias.len())", false),
                    ("", "alias[0] = 0", true),
                    ("consume(rows)", "print(alias.len())", true),
                ] {
                    let source = format!("{prefix}fn main() {{ rows := fresh(); view := {selected}; {carry}; {action}; mut alias := {use_view}; {tail} }}");
                    verdict("indexed-field-view-carrier", &source, rejected);
                }
            }
        }
        for (body, rejected) in [
            ("rows := fresh(); rows[0].data[..]", true),
            ("rows := fresh(); returned(rows)", true),
        ] {
            verdict("indexed-field-view-local-escape", &format!("{prefix}fn escape() -> slice<i64> {{ {body} }}\nfn main() {{}}"), rejected);
        }
    }
}

#[test]
fn evaluation_and_control() {
    let prefix = "Row { data: array<i64> }\nfn fresh() -> array<Row> { mut b: array_builder<Row> := array_builder(); b.push(Row { data: [7, 12].to_array() }); b.build() }\nfn consume(rows: array<Row>) -> i64 = 0\nfn replace(borrow mut rows: array<Row>) -> i64 { rows = fresh(); 0 }\n";
    for receiver in ["rows[{ACTION}].data[..]", "rows[0].data[{ACTION}..]", "rows[0].data[..{ACTION}]"] {
        for (action, rejected) in [("consume(rows)", true), ("replace(rows)", true),
            ("{ rows = fresh(); 0 }", true), ("replace(other)", false),
            ("{ return; 0 }", false)] {
            let selected = receiver.replace("{ACTION}", action);
            verdict("indexed-field-view-eager-order", &format!("{prefix}fn main() {{ mut rows := fresh(); mut other := fresh(); view := {selected}; print(view.len()) }}"), rejected);
        }
    }
    let source = format!("{prefix}fn main() -> i32 {{ rows := fresh(); mut count := 0; view := rows[{{ count = count + 1; 0 }}].data[{{ count = count * 10 + 2; 0 }}..{{ count = count * 10 + 3; 2 }}]; if count != 123 || view[0] != 7 || view[1] != 12 {{ return 1 }}; 0 }}");
    owned_fixture::run("evaluation_and_control", |stage| {
        indexed_array_field::execute(stage, &source, "order").unwrap();
    });
}

#[test]
fn imports_and_cache() {
    owned_fixture::run("imports_and_cache", |stage| {
        let module = r#"module views
pub Row { data: [i64; 2] }
pub fn make() -> array<Row> {
 mut b: array_builder<Row> := array_builder(2)
 b.push(Row { data: [7, 9] })
 b.push(Row { data: [11, 13] })
 b.build()
}
pub fn prefix<T>(borrow rows: array<Row>, index: i64, marker: T) -> slice<i64> = rows[index].data[..2]
pub fn plain(borrow rows: array<Row>) -> slice<i64> = rows[1].data[..]
pub fn consume(rows: array<Row>) {}
"#;
        let source = "import views\nfn first_value(value: slice<i64>) -> i64 = value[0]\nfn read(borrow value: slice<i64>) -> i64 = value[0] + value[1]\nfn main() -> i32 { rows := views.make(); first := views.prefix(rows, 0, true); second := views.prefix(rows, 1, 2); get := first_value; third := get(views.plain(rows)); if read(first) != 16 || read(second) != 24 || third != 11 { return 1 }; saved := second[1]; views.consume(rows); if saved == 13 { 0 } else { 2 } }\n";
        std::fs::write(stage.join("views.align"), module).unwrap();
        indexed_array_field::execute(stage, source, "imported").unwrap();
        let edited = module.replace("[i64; 2]", "[i64; 3]")
            .replace("[7, 9]", "[7, 9, 0]").replace("[11, 13]", "[11, 13, 0]");
        let cache = align_driver::CacheContext::at(stage.join("cache"));
        let entry = stage.join("main.align");
        let mut first = Vec::new();
        for (round, module) in [module, module, edited.as_str(), module].into_iter().enumerate() {
            std::fs::write(stage.join("views.align"), module).unwrap();
            let mut sources = align_span::SourceMap::new();
            let mut package = align_driver::build_package(&mut sources, entry.to_str().unwrap(), source,
                &cache, align_driver::UnitReuse::Allowed);
            assert!(!package.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &package.diags));
            let helper = package.units.iter().find(|unit| unit.unit == "views").unwrap();
            assert_eq!(helper.frontend.as_ref().unwrap().hit, matches!(round, 1 | 3));
            let programs = (0..package.units.len()).map(|index| package.materialize(index).unwrap().clone()).collect::<Vec<_>>();
            let printed = programs.iter().map(align_mir::print::program_to_string).collect::<Vec<_>>();
            match round { 0 => first = printed, 1 | 3 => assert_eq!(first, printed), _ => assert_ne!(first, printed) }
            let output = indexed_array_field::run_programs(stage, &programs, &format!("cached-{round}"), false).unwrap();
            assert!(output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(), "{output:?}");
        }
        for selected in ["views.prefix(rows, 0, true)", "views.plain(rows)"] {
            let main = format!("import views\nfn first_value(value: slice<i64>) -> i64 = value[0]\nfn main() {{ rows := views.make(); mut bytes := {selected}; bytes[0] = 0 }}");
            let checked = diff_check_multi("indexed-view-imported-authority", &[("main.align", main.as_str()), ("views.align", module)], "main.align");
            assert!(checked.whole_errors && checked.per_unit_errors);
        }
    });
}

#[test]
fn nested_parents_and_cleanup() {
    owned_fixture::run("nested_parents_and_cleanup", |stage| {
        let source = r#"
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
Point { x: i64, y: i64 }
Inner { fixed: [i64; 2], owned: array<i64>, points: array<Point> }
Row { inner: Inner }
Outer { rows: array<Row> }
fn fresh() -> Outer {
 mut b: array_builder<Row> := array_builder()
 b.push(Row { inner: Inner { fixed: [3, 5], owned: [7, 11].to_array(), points: [Point { x: 13, y: 17 }].to_array() } })
 Outer { rows: b.build() }
}
fn prefix(borrow outer: Outer) -> slice<i64> = outer.rows[0].inner.owned[..]
MatchRow { owned: array<i64> }
fn matched(borrow value: Option<array<MatchRow>>) -> i64 = match value {
 Some(rows) => { view := rows[0].owned[..]; view[1] }, None => 0
}
fn consume(outer: Outer) {}
fn exercise() -> i64 {
 mut outer := fresh()
 if unsafe { align_rt_requested_live_bytes() } <= 0 { return -1 }
 before := unsafe { align_rt_alloc_count() }
 freed := unsafe { align_rt_free_count() }
 fixed := outer.rows[0].inner.fixed[..]
 owned := prefix(outer)
 points := outer.rows[0].inner.points[..]
 saved := points[0]
 mut index := 0
 mut sum := 0
 loop { if index == 8 { break }; sum = sum + fixed[0] + owned[1] + points[0].y; index = index + 1 }
 if unsafe { align_rt_alloc_count() } != before || unsafe { align_rt_free_count() } != freed { return -2 }
 outer = fresh()
 consume(outer)
 if saved.x != 13 || saved.y != 17 { return -3 }
 mut matched_builder: array_builder<MatchRow> := array_builder()
 matched_builder.push(MatchRow { owned: [7, 11].to_array() })
 maybe := Some(matched_builder.build())
 sum = sum + matched(maybe)
 arena { rows := [Row { inner: Inner { fixed: [1, 2], owned: [3, 4].to_array(), points: [Point { x: 5, y: 6 }].to_array() } }]; view := rows[0].inner.owned[..]; sum = sum + view[0] }
 sum
}
fn main() -> i32 {
 unsafe { align_rt_requested_live_reset() }
 before := unsafe { align_rt_requested_live_bytes() }
 if exercise() != 262 { return 1 }
 if unsafe { align_rt_requested_live_bytes() } != before { return 2 }
 0
}
"#;
        indexed_array_field::execute(stage, source, "cleanup").unwrap();
    });
}

#[test]
fn bounds_and_termination() {
    owned_fixture::run("bounds_and_termination", |stage| {
        for (label, expression, expected) in [
            ("outer-low", "rows[-1].data[{ print(99); 0 }..]", b"".as_slice()),
            ("outer-high", "rows[1].data[..{ print(99); 0 }]", b"".as_slice()),
            ("range-low", "rows[0].data[-1..2]", b"".as_slice()),
            ("range-high", "rows[0].data[..3]", b"".as_slice()),
            ("range-order", "rows[0].data[{ print(1); 2 }..{ print(2); 1 }]", b"1\n2\n".as_slice()),
        ] {
            let source = format!("Row {{ data: [i64; 2] }}\nfn main() {{ rows := [Row {{ data: [7, 12] }}]; unused := {expression} }}");
            for per_unit in [false, true] {
                let programs = indexed_array_field::programs(stage, &source, per_unit).unwrap();
                let output = indexed_array_field::run_programs(stage, &programs, &format!("{label}-{per_unit}"), false).unwrap();
                assert!(!output.status.success(), "{label}/{per_unit}");
                assert_eq!(output.stdout, expected, "{label}/{per_unit}");
            }
        }
        let source = "Row { data: [i64; 0] }\nfn empty(borrow rows: array<Row>) -> slice<i64> = rows[0].data[..]\nfn early() -> i64 { rows := [Row { data: [] }]; value := rows[0].data[..{ return 7; 0 }]; 0 }\nfn main() -> i32 { mut b: array_builder<Row> := array_builder(); b.push(Row { data: [] }); rows := b.build(); view := empty(rows); if view.len() == 0 && early() == 7 { 0 } else { 1 } }";
        indexed_array_field::execute(stage, source, "empty-and-return").unwrap();
        let empty = "Row { data: [i64; 2] }\nfn main() { rows: [Row; 0] := []; unused := rows[0].data[..{ print(99); 0 }] }";
        for per_unit in [false, true] {
            let programs = indexed_array_field::programs(stage, empty, per_unit).unwrap();
            let output = indexed_array_field::run_programs(stage, &programs, &format!("empty-parent-{per_unit}"), false).unwrap();
            assert!(!output.status.success() && output.stdout.is_empty(), "{output:?}");
        }
        let zero_record = "Zero { empty: [i64; 0] }\nRow { data: [Zero; 1] }\nfn main() -> i32 { mut rows := [Row { data: [Zero { empty: [] }] }]; view := rows[0].data[..]; rows = [Row { data: [Zero { empty: [] }] }]; if view.len() == 1 { 0 } else { 1 } }";
        indexed_array_field::execute(stage, zero_record, "zero-record").unwrap();
    });
}
