//! Plan232: independent fixed-field values compose through ordinary Copy bindings.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
#[path = "helpers/indexed_array_field.rs"]
mod native;
use align_span::SourceMap;

const MODEL: &str = r#"module model
pub Point { x: i64, y: i64 }
pub Leaf { point: Point }
pub Shape { dims: [i64; 3] }
pub Row { shape: Shape, points: [Point; 2], nested: [Leaf; 1] }
pub Generic<T> { values: [T; 3] }
pub fn read<T>(rows: slice<Row>, i: i64, j: i64, marker: T) -> i64 = rows[i].shape.dims[j]
pub fn x(rows: slice<Row>, i: i64, j: i64) -> i64 = rows[i].points[j].x as i64
pub fn make() -> Row = Row { shape: Shape { dims: [2, 3, 4] }, points: [Point { x: 7, y: 12 }, Point { x: 20, y: 30 }], nested: [Leaf { point: Point { x: 50, y: 60 } }] }
pub fn retired() -> Point {
  arena scratch {
    mut b: array_builder<Row> := array_builder(scratch)
    b.push(make())
    rows := b.build()
    return rows[0].points[0]
  }
}
pub fn retired_control() -> Point {
  arena scratch {
    mut b: array_builder<Row> := array_builder(scratch)
    b.push(make())
    rows := b.build()
    fixed := rows[0].points
    return fixed[0]
  }
}
"#;
const MAIN: &str = r#"module main
import model
fn main() -> i32 {
  rows := [model.make()]
  fixed := rows[0].points
  direct := rows[0].points[1]
  control := fixed[1]
  if direct.x != control.x || rows[0].points[0].y != 12 { return 1 }
  if rows[0].nested[0].point.x != 50 || rows[0].shape.dims[2] != 4 { return 2 }
  if model.x(rows[..], 0, 1) != 20 { return 3 }
  ints: [model.Generic<i64>; 1] := [model.Generic { values: [1, 2, 3] }]
  floats: [model.Generic<f64>; 1] := [model.Generic { values: [1.5, 2.5, 3.5] }]
  if ints[0].values[2] != 3 || floats[0].values[1] != 2.5 { return 4 }
  if model.read(rows[..], 0, 2, true) != 4 || model.read(rows[..], 0, 0, 1) != 2 { return 7 }
  retired := model.retired()
  old := model.retired_control()
  if retired.x != old.x || retired.y != 12 { return 5 }
  arena out {
    mut b: array_builder<model.Row> := array_builder(out)
    b.push(model.make())
    values := b.build()
    if values[0].points[1].x != 20 || values[0].shape.dims[1] != 3 { return 6 }
  }
  return 0
}
"#;

#[test]
fn values_imports_and_cache() {
    owned_fixture::run("values_imports_and_cache", |stage| {
        assert!(align_driver::backend_available());
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        native::execute(stage, MAIN, "values").unwrap();
        let context = align_driver::CacheContext::at(stage.join("cache"));
        let entry = stage.join("main.align");
        let mut original = Vec::new();
        for pass in 0..4 {
            let module = if pass == 2 { MODEL.replace("x: i64", "x: i32") } else { MODEL.into() };
            std::fs::write(stage.join("model.align"), module).unwrap();
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(&mut sources, entry.to_str().unwrap(), MAIN,
                &context, align_driver::UnitReuse::Allowed);
            assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &built.diags));
            for unit in &built.units { assert_eq!(unit.frontend.as_ref().unwrap().hit, pass == 1 || pass == 3, "pass {pass}: {}", unit.unit); }
            let programs = (0..built.units.len()).map(|i| built.materialize(i).unwrap().clone()).collect::<Vec<_>>();
            let snapshot = programs.iter().map(|p| format!("{:?}\n{}", p.structs, align_mir::print::program_to_string(p))).collect::<Vec<_>>();
            if pass == 0 { original = snapshot; }
            else if pass == 2 { assert!(original != snapshot); }
            else { assert!(original == snapshot); }
            let output = native::run_programs(stage, &programs, &format!("cache-{pass}"), false).unwrap();
            assert!(output.status.success(), "{pass}: {output:?}");
        }
    });
}

#[test]
fn snapshot_effects_and_control() {
    owned_fixture::run("snapshot_effects_and_control", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        let source = r#"import model
fn observe(rows: slice<model.Row>, borrow mut count: i64) -> slice<model.Row> { count = count * 10 + 1; return rows }
fn main() -> i32 {
  mut rows := [model.make()]
  mut count := 0
  copied := observe(rows[..], count)[{ count = count * 10 + 2; 0 }].points[{
    count = count * 10 + 3
    rows = [model.Row { shape: model.Shape { dims: [9, 9, 9] }, points: [model.Point { x: 90, y: 91 }, model.Point { x: 92, y: 93 }], nested: [model.Leaf { point: model.Point { x: 94, y: 95 } }] }]
    0
  }].x
  if copied != 7 || count != 123 || rows[0].points[0].x != 90 { return 1 }
  mut reset := [model.make()]
  nested := reset[0].nested[{ reset = rows; 0 }].point.y
  if nested != 60 { return 2 }
  return 0
}
"#;
        native::execute(stage, source, "snapshot").unwrap();
        let expressions = ["if true { 0 } else { 1 }", "match 0 { 0 => 0, _ => 1 }",
            "none() else 0", "index_ok()?", "index_ok().map_err(fn e: i64 { e })?",
            "loop { break 0 }", "{ return Ok(7); 0 }"];
        let functions = expressions.iter().enumerate().map(|(i, expr)| format!(
            "fn f{i}(rows: slice<model.Row>) -> Result<i64, i64> = Ok(rows[0].points[{expr}].x)\n")).collect::<String>();
        let calls = (0..expressions.len()).map(|i| format!("if (f{i}(rows[..]) else -1) != 7 {{ return 1 }}\n")).collect::<String>();
        let source = format!("import model\nfn none() -> Option<i64> = None\nfn index_ok() -> Result<i64,i64> = Ok(0)\nfn index_err() -> Result<i64,i64> = Err(9)\nfn failed(rows: slice<model.Row>) -> Result<i64,i64> = Ok(rows[0].points[index_err()?].x)\n{functions}\nfn main() -> i32 {{ rows := [model.make()]; {calls} if (failed(rows[..]) else -9) != -9 {{ return 2 }}; return 0 }}");
        native::execute(stage, &source, "control").unwrap();
    });
}

#[test]
fn bounds_and_termination() {
    owned_fixture::run("bounds_and_termination", |stage| {
        for (label, length, values, outer, inner, expected) in [
            ("outer-negative", 2, "7, 12", "-1", "0", "1\n"),
            ("outer-high", 2, "7, 12", "1", "0", "1\n"),
            ("inner-negative", 2, "7, 12", "0", "-1", "1\n2\n"),
            ("inner-high", 2, "7, 12", "0", "2", "1\n2\n"),
            ("zero", 0, "", "0", "0", "1\n2\n"),
        ] {
            let source = format!("Row {{ values: [i64; {length}] }}\nfn main() {{ rows := [Row {{ values: [{values}] }}]; unused := rows[{{ print(1); {outer} }}].values[{{ print(2); {inner} }}] }}");
            for per_unit in [false, true] {
                let programs = native::programs(stage, &source, per_unit).unwrap();
                let output = native::run_programs(stage, &programs, &format!("{label}-{per_unit}"), false).unwrap();
                assert!(!output.status.success() && String::from_utf8_lossy(&output.stderr).contains("bounds"), "{label}: {output:?}");
                assert_eq!(output.stdout, expected.as_bytes(), "{label}");
            }
        }
        for (label, outer, inner) in [("outer-exit", "process.exit(0)", "{ print(9); 0 }"),
            ("inner-exit", "0", "process.exit(0)")] {
            let source = format!("import std.process\nRow {{ values: [i64; 1] }}\nfn main() {{ rows := [Row {{ values: [7] }}]; print(rows[{outer}].values[{inner}]) }}");
            native::execute(stage, &source, label).unwrap();
        }
    });
}

#[test]
fn primitive_fields_and_owned_parent() {
    owned_fixture::run("primitive_fields_and_owned_parent", |stage| {
        let leaves = [("i8", "-7"), ("u8", "251"), ("i16", "-300"), ("u16", "60000"),
            ("i32", "-70000"), ("u32", "4000000000"), ("i64", "-9000000000"),
            ("u64", "18000000000000000000"), ("f32", "1.5"), ("f64", "-2.5"),
            ("bool", "true"), ("char", "'界'")];
        let fields = leaves.iter().enumerate().map(|(i, (ty, _))| format!("v{i}: [{ty}; 1]")).collect::<Vec<_>>().join(", ");
        let values = leaves.iter().enumerate().map(|(i, (_, v))| format!("v{i}: [{v}]")).collect::<Vec<_>>().join(", ");
        let checks = leaves.iter().enumerate().map(|(i, (_, v))| format!("if rows[0].v{i}[0] != {v} {{ return 1 }}")).collect::<Vec<_>>().join("\n");
        let many = (0..37).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
        let source = format!(r#"Row {{ name: string, {fields}, many: [i64; 37] }}
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn make() -> array<Row> {{ mut b: array_builder<Row> := array_builder(); b.push(Row {{ name: "owned".clone(), {values}, many: [{many}] }}); return b.build() }}
fn take(rows: array<Row>) {{}}
fn main() -> i32 {{
  initial := unsafe {{ align_rt_alloc_count() }}
  mut rows := make()
  allocated := unsafe {{ align_rt_alloc_count() }}
  freed := unsafe {{ align_rt_free_count() }}
  if allocated <= initial {{ return 2 }}
  {checks}
  copied := rows[0].many[36]
  if unsafe {{ align_rt_alloc_count() }} != allocated || unsafe {{ align_rt_free_count() }} != freed {{ return 3 }}
  rows = make()
  after_drop := rows[0].many[{{ take(rows); 36 }}]
  if copied != 36 || after_drop != 36 {{ return 4 }}
  return 0
}}
"#);
        native::execute(stage, &source, "primitive-owned").unwrap();
    });
}

#[test]
fn receiver_completion() {
    owned_fixture::run("receiver_completion", |stage| {
        let prefix = r#"Row { name: string, values: [i64; 2], scalar: i64 }
Wrapper { rows: array<Row> }
fn fresh() -> array<Row> { mut b: array_builder<Row> := array_builder(); b.push(Row { name: "x".clone(), values: [7, 12], scalar: 7 }); return b.build() }
fn take(rows: array<Row>) -> i64 = 0
fn take_wrapper(owner: Wrapper) -> i64 = 0
fn replace(borrow mut rows: array<Row>) -> i64 { rows = fresh(); return 0 }
fn identity(rows: slice<Row>) -> slice<Row> = rows
"#;
        for (i, body) in [
            "mut rows := fresh(); return rows[take(rows)].values[0]",
            "mut rows := fresh(); fixed := rows[take(rows)].values; return fixed[0]",
            "mut rows := fresh(); return rows[take(rows)].scalar",
            "mut rows := fresh(); return rows[{ rows = fresh(); 0 }].values[0]",
            "mut rows := fresh(); return rows[replace(rows)].values[0]",
            "mut rows := fresh(); view := rows[..]; return view[take(rows)].values[0]",
            "mut rows := fresh(); return identity(rows[..])[take(rows)].values[0]",
            "owner := Wrapper { rows: fresh() }; return owner.rows[take_wrapper(owner)].values[0]",
            "mut rows := fresh(); return rows[if true { take(rows) } else { 0 }].values[0]",
            "mut rows := fresh(); return rows[loop { break take(rows) }].values[0]",
        ].iter().enumerate() {
            let source = format!("{prefix}\nfn bad() -> i64 {{ {body} }}\nfn main() {{ print(bad()) }}");
            let entry = stage.join(format!("receiver-{i}.align"));
            std::fs::write(&entry, &source).unwrap();
            for per_unit in [false, true] {
                let mut sources = SourceMap::new();
                let diags = if per_unit { align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), &source).diags }
                    else { align_driver::check(&mut sources, entry.to_str().unwrap(), &source).diags };
                let text = align_driver::format_diagnostics(&sources, &diags);
                assert!(diags.has_errors() && (text.contains("invalidated") || text.contains("moved")), "accepted/bad fixture {i}/{per_unit}: {text}");
            }
        }
        let source = format!(r#"{prefix}
Plain {{ values: [i64; 1] }}
fn ended() -> i64 {{ rows := fresh(); return rows[{{ unused := take(rows); return 0; 0 }}].values[0] }}
fn main() -> i32 {{
  if ended() != 0 {{ return 1 }}
  mut rows := [Plain {{ values: [7] }}]
  current := rows[{{ rows = [Plain {{ values: [9] }}]; 0 }}].values[0]
  if current != 9 {{ return 2 }}
  return 0
}}
"#);
        native::execute(stage, &source, "receiver-controls").unwrap();
    });
}

#[test]
fn excluded_neighbors() {
    owned_fixture::run("excluded_neighbors", |stage| {
        let mut cases = vec![
            "Row { values: [i64; 2] }\nfn read(rows: slice<Row>) -> slice<i64> = rows[0].values[..]\nfn main() {}".to_owned(),
            "Row { values: [i64; 2] }\nfn read(rows: slice<Row>) -> i64 = rows[0].values.sum()\nfn main() {}".to_owned(),
            "fn make() -> [i64; 2] = [7, 12]\nfn main() { print(make()[0]) }".to_owned(),
            "Row { values: [i64; 2] }\nfn main() { mut rows := [Row { values: [7, 12] }]; rows[0].values[0] = 9 }".to_owned(),
            "Leaf { value: i64, text: str }\nRow { values: [Leaf; 1] }\nfn read(rows: slice<Row>) -> i64 = rows[0].values[0].value\nfn main() {}".to_owned(),
            "layout(C) Pointer { address: raw }\nRow { values: [Pointer; 1] }\nfn read(rows: slice<Row>) -> raw = rows[0].values[0].address\nfn main() {}".to_owned(),
            "Row { name: string, values: [i64; 2] }\nfn take(rows: array<Row>) -> i64 = 0\nfn read(rows: array<Row>) -> i64 = rows[take(rows)].values[0]\nfn main() {}".to_owned(),
        ];
        for element in ["str", "string", "fn(i64) -> i64"] {
            for length in [0, 2] {
                cases.push(format!("Row {{ values: [{element}; {length}] }}\nfn read(rows: slice<Row>) {{ unused := rows[0].values[0] }}\nfn main() {{}}"));
            }
        }
        for (i, source) in cases.iter().enumerate() {
            let entry = stage.join(format!("reject-{i}.align"));
            std::fs::write(&entry, source).unwrap();
            for per_unit in [false, true] {
                let mut sources = SourceMap::new();
                let diags = if per_unit { align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), source).diags }
                    else { align_driver::check(&mut sources, entry.to_str().unwrap(), source).diags };
                let text = align_driver::format_diagnostics(&sources, &diags);
                assert!(diags.has_errors(), "accepted {i}/{per_unit}: {source}");
                assert!(!text.contains("internal error") && !text.contains("expected expression") && !text.contains("undefined name"), "bad fixture {i}: {text}");
            }
        }
    });
}
