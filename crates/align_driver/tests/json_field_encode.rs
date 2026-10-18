//! Plan227: stable selected JSON fields retain exact source and borrowing authority.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn execute(stage: &Path, programs: &[align_mir::Program], label: &str, rt_lto: bool) -> TestResult {
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{index}.o"));
        align_driver::emit_object_file(
            program,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            rt_lto,
        )?;
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
    }
    let binary = stage.join(label);
    align_driver::link_objects(
        &align_driver::CDriver::default(),
        &objects
            .iter()
            .map(|path| path.as_path())
            .collect::<Vec<_>>(),
        &binary,
        &libraries,
        Profile::Release,
    )?;
    let output = std::process::Command::new(binary).output()?;
    assert!(
        output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
    Ok(())
}
fn programs(stage: &Path, source: &str, per_unit: bool) -> TestResult<Vec<align_mir::Program>> {
    let path = stage.join("main.align");
    std::fs::write(&path, source)?;
    let entry = path.to_str().ok_or("non-UTF8 fixture path")?;
    let mut sources = SourceMap::new();
    if per_unit {
        let built = align_driver::build_per_unit(&mut sources, entry, source);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &built.diags)
        );
        Ok(built.units.into_iter().map(|unit| unit.mir).collect())
    } else {
        let checked = align_driver::check(&mut sources, entry, source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        Ok(vec![
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources))
                .map_err(|error| format!("{error:?}"))?,
        ])
    }
}

fn run_both(stage: &Path, source: &str, label: &str) -> TestResult {
    for per_unit in [false, true] {
        let mir = programs(stage, source, per_unit)?;
        execute(stage, &mir, &format!("{label}-{per_unit}"), per_unit)?;
    }
    Ok(())
}

#[test]
fn stable_field_schema_matrix() {
    owned_fixture::run("stable_field_schema_matrix", |stage| {
        let result = run_both(
            stage,
            r#"import core.json
Record { values: array<i64> }
Nested { record: Record, unrelated: buffer }
fn main() -> Result<(), Error> {
  mut values: array_builder<i64> := array_builder(2)
  values.push(1)
  values.push(2)
  record:=Nested {record:Record {values:values.build()},unrelated:buffer(8)}
  first:=json.encode(record.record.values)?
  second:=json.encode_bounded(record.record.values,5)?
  if first!="[1,2]" || second!=first || record.record.values.len()!=2 {return Err(Error.Invalid)}
  match json.encode_bounded(record.record.values,4) {Err(Invalid)=>{},_=>{return Err(Error.Invalid)}}
  if record.unrelated.len()!=0 {return Err(Error.Invalid)}
  Ok(())
}
"#,
            "scalar-array",
        );
        assert!(result.is_ok(), "{result:?}");
    });
}

#[test]
fn stable_field_route_matrix() {
    owned_fixture::run("stable_field_route_matrix", |stage| {
        let result = (|| -> TestResult {
            for (name, declarations, ty, setup, expected) in [
                (
                    "copy-record",
                    "Row { value:i64, label:str, maybe:Option<i64> }",
                    "Row",
                    "selected:=Row {value:3,label:\"view\",maybe:None}",
                    r#"{"value":3,"label":"view"}"#,
                ),
                (
                    "mixed-record",
                    "Row { value:string, label:str }",
                    "Row",
                    "selected:=Row {value:\"owned\".clone(),label:\"view\"}",
                    r#"{"value":"owned","label":"view"}"#,
                ),
                (
                    "sum",
                    "Choice { Number(i64), Text(str) }",
                    "Choice",
                    "selected:=Choice.Number(7)",
                    "7",
                ),
                (
                    "fixed",
                    "Row { value:i64 }",
                    "[Row;2]",
                    "selected:[Row;2]:=[Row {value:1},Row {value:2}]",
                    r#"[{"value":1},{"value":2}]"#,
                ),
                (
                    "fixed-empty",
                    "Row { value:i64 }",
                    "[Row;0]",
                    "selected:[Row;0]:=[]",
                    "[]",
                ),
                (
                    "copy-array",
                    "Row { value:i64 }",
                    "array<Row>",
                    "mut builder:array_builder<Row>:=array_builder(out)\nbuilder.push(Row {value:1})\nselected:=builder.build()",
                    r#"[{"value":1}]"#,
                ),
                (
                    "mixed-array",
                    "Row { value:string, label:str }",
                    "array<Row>",
                    r#"selected:array<Row>:=json.decode("[{\"value\":\"owned\",\"label\":\"view\"}]")?"#,
                    r#"[{"value":"owned","label":"view"}]"#,
                ),
                (
                    "owned-array",
                    "Row { value:string }",
                    "array<Row>",
                    "mut builder:array_builder<Row>:=array_builder()\nbuilder.push(Row {value:\"owned\".clone()})\nselected:=builder.build()",
                    r#"[{"value":"owned"}]"#,
                ),
                (
                    "string-array",
                    "",
                    "array<string>",
                    "mut builder:array_builder<string>:=array_builder()\nbuilder.push(\"a\".clone())\nselected:=builder.build()",
                    r#"["a"]"#,
                ),
                (
                    "str-array",
                    "",
                    "array<str>",
                    "mut builder:array_builder<str>:=array_builder(out)\nbuilder.push(\"a\")\nselected:=builder.build()",
                    r#"["a"]"#,
                ),
                (
                    "empty-array",
                    "",
                    "array<i64>",
                    "mut builder:array_builder<i64>:=array_builder()\nselected:=builder.build()",
                    "[]",
                ),
            ] {
                let source = format!(
                    r#"import core.json
{declarations}
InnerOwner {{ selected:{ty} }}
OuterOwner {{ inner:InnerOwner, unrelated:[i64;1] }}
fn main()->Result<(),Error> {{
  arena out {{
    {setup}
    control:=json.encode(selected)?
    holder:=OuterOwner {{inner:InnerOwner {{selected:selected}},unrelated:[0]}}
    encoded:=json.encode(holder.inner.selected)?
    limited:=json.encode_bounded(holder.inner.selected,{limit})?
    if encoded!={expected:?} || limited!=encoded || control!=encoded {{return Err(Error.Invalid)}}
    match json.encode_bounded(holder.inner.selected,{under}) {{Err(Invalid)=>{{}},_=>{{return Err(Error.Invalid)}}}}
    if holder.unrelated[0]!=0 {{return Err(Error.Invalid)}}
  }}
  Ok(())
}}
"#,
                    limit = expected.len(),
                    under = expected.len() - 1
                );
                run_both(stage, &source, name)?;
            }
            Ok(())
        })();
        assert!(result.is_ok(), "{result:?}");
    });
}

#[test]
fn borrowed_fixed_record_array_field_matrix() {
    owned_fixture::run("borrowed_fixed_record_array_field_matrix", |stage| {
        let result = run_both(
            stage,
            r#"import core.json
Inner { value: i64 }
Row { inner: Inner }
CopyCarrier { rows: [Row;2] }
Carrier { copy: CopyCarrier, owner: string }
Choice { Full(CopyCarrier,string), Empty }
fn make()->CopyCarrier=CopyCarrier {rows:[Row {inner:Inner {value:1}},Row {inner:Inner {value:2}}]}
fn shared(borrow value:Carrier)->Result<string,Error> {
  a:=json.encode(value.copy.rows)?
  b:=json.encode_bounded(value.copy.rows,a.len())?
  if a!=b || value.copy.rows[1].inner.value!=2 || value.owner!="owner" {return Err(Error.Invalid)}
  Ok(a)
}
fn option(borrow input:Option<CopyCarrier>)->Result<string,Error> {
  match input {Some(value)=>json.encode(value.rows),None=>Err(Error.Invalid)}
}
fn result(borrow input:Result<CopyCarrier,Error>)->Result<string,Error> {
  match input {Ok(value)=>json.encode_bounded(value.rows,100),Err(_)=>Err(Error.Invalid)}
}
fn choice(borrow input:Choice)->Result<string,Error> {
  match input {Full(value,text)=>{
    a:=json.encode(value.rows)?
    b:=json.encode_bounded(value.rows,a.len())?
    if a!=b || text!="owner" {return Err(Error.Invalid)}
    Ok(a)
  },Empty=>Err(Error.Invalid)}
}
fn main()->Result<(),Error> {
  a:=Carrier {copy:make(),owner:"owner".clone()}
  b:Option<CopyCarrier>:=Some(make())
  c:Result<CopyCarrier,Error>:=Ok(make())
  d:=Choice.Full(make(),"owner".clone())
  expected:="[{\"inner\":{\"value\":1}},{\"inner\":{\"value\":2}}]"
  if shared(a)?!=expected || option(b)?!=expected || result(c)?!=expected || choice(d)?!=expected || shared(a)?!=expected {return Err(Error.Invalid)}
  Ok(())
}

"#,
            "borrowed-fixed",
        );
        assert!(result.is_ok(), "{result:?}");
    });
}

#[test]
fn shared_move_fixed_array_formation_remains_rejected() {
    for (ty, body) in [
        (
            "Option<Carrier>",
            "match input {Some(value)=>json.encode(value.rows),None=>Err(Error.Invalid)}",
        ),
        (
            "Result<Carrier,Error>",
            "match input {Ok(value)=>json.encode(value.rows),Err(_)=>Err(Error.Invalid)}",
        ),
        (
            "Choice",
            "match input {Full(value)=>json.encode_bounded(value.rows,100),Empty=>Err(Error.Invalid)}",
        ),
    ] {
        let source = format!(
            "import core.json\nRow {{ value:i64 }}\nCarrier {{ rows:[Row;2],owner:string }}\nChoice {{ Full(Carrier),Empty }}\nfn bad(borrow input:{ty})->Result<string,Error> ={body}\nfn main()->i32=0\n"
        );
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, "fixed-formation.align", &source);
        let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
        assert!(
            checked.diags.has_errors() && diagnostics.contains("unsupported Move payload"),
            "{ty}: {diagnostics}"
        );
    }
}

#[test]
fn source_reservation_and_partial_move_matrix() {
    let mut failures = Vec::new();
    for (name, setup, expression, expected) in [
        (
            "moved-field",
            "consume_row(holder.selected)",
            "json.encode(holder.selected)",
            "moved",
        ),
        (
            "moved-root",
            "consume(holder)",
            "json.encode(holder.selected)",
            "moved",
        ),
        (
            "limit-consumes-root",
            "",
            "json.encode_bounded(holder.selected,{consume(holder);100})",
            "invalidated",
        ),
        (
            "limit-replaces-source",
            "",
            "json.encode_bounded(holder.selected,{holder.selected=Row {value:\"new\".clone()};100})",
            "invalidated",
        ),
        (
            "limit-mutates-sibling",
            "",
            "json.encode_bounded(holder.selected,{holder.other=\"new\".clone();100})",
            "invalidated",
        ),
        (
            "temporary",
            "",
            "json.encode(Row {value:\"new\".clone()})",
            "local binding or record field",
        ),
    ] {
        let source = format!(
            "import core.json\nRow {{value:string}}\nHolder {{selected:Row,other:string}}\nfn consume(value:Holder) {{}}\nfn consume_row(value:Row) {{}}\nfn bad()->Result<string,Error> {{mut holder:=Holder {{selected:Row {{value:\"owner\".clone()}},other:\"other\".clone()}};{setup};{expression}}}\nfn main() {{}}\n"
        );
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, "field-reservation.align", &source);
        let text = align_driver::format_diagnostics(&sources, &checked.diags);
        if !checked.diags.has_errors() || !text.contains(expected) {
            failures.push(format!("{name}: {text}"));
        }
    }
    for (name, action, limit) in [
        (
            "replace-backing",
            "",
            "{source=\"replacement\".clone();100}",
        ),
        ("consume-backing", "", "{consume_text(source);100}"),
        ("expired-backing", "consume_text(source)", "100"),
    ] {
        let source = format!(
            "import core.json\nRow {{value:string,text:str}}\nHolder {{selected:Row}}\nfn consume_text(value:string) {{}}\nfn bad()->Result<string,Error> {{mut source:=\"{{}}\".clone();row:Row:=json.decode(source)?;holder:=Holder {{selected:row}};{action};json.encode_bounded(holder.selected,{limit})}}\nfn main() {{}}\n"
        );
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, "field-backing.align", &source);
        let text = align_driver::format_diagnostics(&sources, &checked.diags);
        if !checked.diags.has_errors() || !(text.contains("invalidated") || text.contains("borrow"))
        {
            failures.push(format!("{name}: {text}"));
        }
    }
    for (ty, selected, diagnostic) in [
        ("i64", "holder.selected", "expects an encodable"),
        ("[i64;2]", "holder.selected", "expects an encodable"),
        ("Option<i64>", "holder.selected", "expects an encodable"),
        ("slice<i64>", "holder.selected", "expects an encodable"),
        (
            "array<i64>",
            "holder.selected[0]",
            "local binding or record field",
        ),
    ] {
        let source = format!(
            "import core.json\nHolder {{selected:{ty}}}\nfn bad(borrow holder:Holder)->Result<string,Error> =json.encode({selected})\nfn main() {{}}\n"
        );
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, "field-excluded.align", &source);
        let text = align_driver::format_diagnostics(&sources, &checked.diags);
        if !checked.diags.has_errors() || !text.contains(diagnostic) {
            failures.push(format!("{ty}: {text}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    owned_fixture::run("source_reservation_and_partial_move_matrix", |stage| {
        let result = run_both(
            stage,
            r#"import core.json
Row {value:string}
Holder {selected:Row,other:string}
Choice {Full(Holder),Empty}
fn consume(value:string) {}
Arrays {values:array<i64>}
ArrayChoice {Full(Arrays),Empty}
fn shared_array(borrow input:ArrayChoice)->Result<string,Error> {
  match input {Full(holder)=>{
    a:=json.encode(holder.values)?
    b:=json.encode_bounded(holder.values,a.len())?
    if a!=b || holder.values.len()!=1 {return Err(Error.Invalid)}
    Ok(a)
  },Empty=>Err(Error.Invalid)}
}
fn shared(borrow input:Choice)->Result<string,Error> {
  match input {Full(holder)=>{
    a:=json.encode(holder.selected)?
    b:=json.encode_bounded(holder.selected,a.len())?
    if a!=b || holder.selected.value!="owner" {return Err(Error.Invalid)}
    Ok(a)
  },Empty=>Err(Error.Invalid)}
}
fn main()->Result<(),Error> {
  holder:=Holder {selected:Row {value:"owner".clone()},other:"other".clone()}
  consume(holder.other)
  a:=json.encode(holder.selected)?
  b:=json.encode_bounded(holder.selected,a.len())?
  input:=Choice.Full(Holder {selected:Row {value:"owner".clone()},other:"other".clone()})
  mut builder:array_builder<i64>:=array_builder()
  builder.push(1)
  arrays:=ArrayChoice.Full(Arrays {values:builder.build()})
  if shared_array(arrays)?!="[1]" || shared_array(arrays)?!="[1]" {return Err(Error.Invalid)}
  if a!="{\"value\":\"owner\"}" || a!=b || shared(input)?!=a || shared(input)?!=a {return Err(Error.Invalid)}
  Ok(())
}
"#,
            "partial-and-shared",
        );
        assert!(result.is_ok(), "{result:?}");
    });
}

// The counted allocator excludes realloc-grown JSON output, while free_count includes its
// release. Requested-live accounting tracks those payloads too; require a positive peak and
// zero retained bytes rather than equating these deliberately different event counters.
const LIFECYCLE: &str = r#"import core.json
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_requested_live_peak()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
Row {value:string,text:str}
Holder {selected:Row,other:buffer}
Numbers {values:array<f64>}
fn map(error:Error)->Error=error
fn detached()->Result<string,Error> {
  arena {
    source:="{\"value\":\"owner\",\"text\":\"borrow\"}".clone()
    row:Row:=json.decode(source)?
    holder:=Holder {selected:row,other:buffer(0)}
    return json.encode(holder.selected)
  }
}
fn diverge(borrow holder:Holder)->Result<string,Error> {
  json.encode_bounded(holder.selected,{return Ok("returned".clone());100})
}
fn exercise()->Result<(),Error> {
  holder:=Holder {selected:Row {value:"owner".clone(),text:"borrow"},other:buffer(0)}
  expected:="{\"value\":\"owner\",\"text\":\"borrow\"}"
  mut count:=0
  first:=json.encode_bounded(holder.selected,{count=count+1;expected.len()})?
  if count!=1 || first!=expected || detached()?!=expected || diverge(holder)?!="returned" {return Err(Error.Invalid)}
  mut mode:=0
  loop {
    if mode==3 {break}
    raw:=if mode==0 {json.encode(holder.selected)} else {match mode {1=>json.encode_bounded(holder.selected,expected.len()),_=>loop {break json.encode(holder.selected)}}}
    encoded:=raw.map_err(map)?
    if encoded!=expected {return Err(Error.Invalid)}
    mode=mode+1
  }
  fallback:=json.encode_bounded(holder.selected,-1) else {json.encode(holder.selected)?}
  if fallback!=expected {return Err(Error.Invalid)}
  mut replaced:=json.encode(holder.selected)
  replaced=json.encode_bounded(holder.selected,expected.len())
  if replaced?!=expected {return Err(Error.Invalid)}
  unused:=json.encode(holder.selected)
  unused_error:=json.encode_bounded(holder.selected,0)
  match json.encode_bounded(holder.selected,expected.len()-1) {Err(Invalid)=>{},_=>{return Err(Error.Invalid)}}
  mut builder:array_builder<f64>:=array_builder()
  builder.push(0.0/0.0)
  values:=Numbers {values:builder.build()}
  match json.encode(values.values) {Err(Invalid)=>{},_=>{return Err(Error.Invalid)}}
  match json.encode_bounded(values.values,100) {Err(Invalid)=>{},_=>{return Err(Error.Invalid)}}
  if values.values.len()!=1 || holder.selected.value!="owner" {return Err(Error.Invalid)}
  Ok(())
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  before:=unsafe {align_rt_alloc_count()}
  free:=unsafe {align_rt_free_count()}
  match exercise() {Ok(_)=>{},Err(_)=>{return 1}}
  allocated:=unsafe {align_rt_alloc_count()}-before
  if allocated<=0 || unsafe {align_rt_free_count()}-free<allocated || unsafe {align_rt_requested_live_peak()}<=0 || unsafe {align_rt_requested_live_bytes()}!=0 {print(allocated);print(unsafe {align_rt_free_count()}-free);print(unsafe {align_rt_requested_live_bytes()});return 2}
  0
}
"#;
#[test]
fn field_output_lifecycle_and_control() {
    owned_fixture::run("field_output_lifecycle_and_control", |stage| {
        let result = run_both(stage, LIFECYCLE, "lifecycle");
        assert!(result.is_ok(), "{result:?}");
    });
}

#[test]
fn imported_generic_field_whole_per_unit_cache() {
    owned_fixture::run("imported_generic_field_whole_per_unit_cache", |stage| {
        let result = (|| -> TestResult {
            std::fs::write(
                stage.join("model.align"),
                r#"module model
import core.json
pub Container<T> {selected:T,other:buffer}
pub Row {value:string}
pub fn encode_ints(borrow input:Container<array<i64>>)->Result<string,Error> = json.encode(input.selected)
pub fn bounded_ints(borrow input:Container<array<i64>>,cap:i64)->Result<string,Error> = json.encode_bounded(input.selected,cap)
pub fn encode_row<T>(borrow input:Container<Row>,marker:T)->Result<string,Error> = json.encode(input.selected)
pub fn bounded_row(borrow input:Container<Row>,cap:i64)->Result<string,Error> = json.encode_bounded(input.selected,cap)
"#,
            )?;
            let source = r#"module main
import model
fn main()->Result<(),Error> {
  mut builder:array_builder<i64>:=array_builder()
  builder.push(1)
  ints:model.Container<array<i64>>:=model.Container {selected:builder.build(),other:buffer(0)}
  row:model.Container<model.Row>:=model.Container {selected:model.Row {value:"owner".clone()},other:buffer(0)}
  if model.encode_ints(ints)?!="[1]" || model.bounded_ints(ints,3)?!="[1]" || model.encode_row(row,1)?!="{\"value\":\"owner\"}" || model.bounded_row(row,17)?!="{\"value\":\"owner\"}" {return Err(Error.Invalid)}
  if ints.selected.len()!=1 || row.selected.value!="owner" {return Err(Error.Invalid)}
  Ok(())
}
"#;
            run_both(stage, source, "import")?;
            let entry = stage.join("main.align");
            let cache = align_driver::CacheContext::at(stage.join("cache"));
            let mut first = Vec::new();
            for (round, hit) in [(0, false), (1, true)] {
                let mut sources = SourceMap::new();
                let mut built = align_driver::build_package(
                    &mut sources,
                    entry.to_str().ok_or("fixture path")?,
                    source,
                    &cache,
                    align_driver::UnitReuse::Allowed,
                );
                assert!(
                    !built.diags.has_errors(),
                    "{}",
                    align_driver::format_diagnostics(&sources, &built.diags)
                );
                assert_eq!(built.units.len(), 2);
                assert!(built.units.iter().all(|unit| {
                    unit.frontend
                        .as_ref()
                        .is_some_and(|frontend| frontend.hit == hit)
                }));
                let lowered = (0..built.units.len())
                    .map(|index| built.materialize(index).cloned())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| format!("{error:?}"))?;
                let keys = lowered
                    .iter()
                    .map(align_interface::codegen_impl_hash)
                    .collect::<Vec<_>>();
                if hit {
                    assert_eq!(keys, first)
                } else {
                    first = keys
                }
                execute(stage, &lowered, &format!("cache-{round}"), false)?;
            }
            Ok(())
        })();
        assert!(result.is_ok(), "{result:?}");
    });
}

#[test]
fn owned_sources_keep_distinct_eager_identity() {
    owned_fixture::run("owned_sources_keep_distinct_eager_identity", |stage| {
        let result = (|| -> TestResult {
            for array in [false, true] {
                let ty = if array { "array<Row>" } else { "Row" };
                let make = if array {
                    "mut builder:array_builder<Row>:=array_builder();builder.push(Row {value:text.clone()});builder.build()"
                } else {
                    "Row {value:text.clone()}"
                };
                let checks = |left: &str, right: &str| {
                    let mut result = String::new();
                    for a in [false, true] {
                        for b in [false, true] {
                            let call = |source: &str, bounded| {
                                if bounded {
                                    format!("json.encode_bounded({source},100)")
                                } else {
                                    format!("json.encode({source})")
                                }
                            };
                            let a = call(left, a);
                            let b = call(right, b);
                            result.push_str(&format!(
                                "if {a}?=={b}? {{return Err(Error.Invalid)}}\n"
                            ));
                            result.push_str(&format!(
                                "if !distinct({a}?,{b}?) {{return Err(Error.Invalid)}}\n"
                            ));
                        }
                    }
                    result
                };
                let source = format!(
                    r#"import core.json
Row {{value:string}}
Holder {{left:{ty},right:{ty}}}
Choice {{Full(Holder),Empty}}
fn make(text:str)->{ty} {{{make}}}
fn distinct(left:string,right:string)->bool=left!=right
fn shared(borrow input:Choice)->Result<(),Error> {{
  match input {{Full(holder)=>{{
    {field_checks}
    Ok(())
  }},Empty=>Err(Error.Invalid)}}
}}
fn main()->Result<(),Error> {{
  left:=make("left")
  right:=make("right")
  {local_checks}
  holder:=Holder {{left:left,right:right}}
  {field_checks}
  choice:=Choice.Full(holder)
  shared(choice)?
  shared(choice)?
  Ok(())
}}
"#,
                    field_checks = checks("holder.left", "holder.right"),
                    local_checks = checks("left", "right")
                );
                run_both(
                    stage,
                    &source,
                    if array { "eager-array" } else { "eager-record" },
                )?;
            }
            Ok(())
        })();
        assert!(result.is_ok(), "{result:?}");
    });
}
