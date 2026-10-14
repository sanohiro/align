//! Request133: independent JSON owned-record-array roots and canonical deep cleanup.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
import core.json
pub Case {id:string,prompt:string,text:string,ids:array<i64>}
pub fn decode(input:str)->Result<array<Case>,Error> = json.decode(input)
pub Child {text:string}
pub Deep {child:Child,maybe:Option<Child>,children:array<Child>,notes:Option<array<string>>,nums:array<f32>,flag:bool}
pub fn deep(input:str)->Result<array<Deep>,Error> = json.decode(input)
pub Wrapped<T> {value:T}
pub fn generic(input:str)->Result<array<Wrapped<string>>,Error> = json.decode(input)

pub fn identity<T>(value:T)->T = value
pub fn map(error:Error)->Error = error
pub fn document()->str = "[{\"id\":\"first\",\"prompt\":\"\\u304c\",\"text\":\"\\u0000\",\"ids\":[1,-2]},{\"id\":\"\",\"prompt\":\"\",\"text\":\"\",\"ids\":[]}]"
pub fn owned()->Result<array<Case>,Error> {
  mut input:=document().clone()
  value:array<Case>:=json.decode(input)?
  input="replaced".clone()
  return Ok(value)
}
pub fn in_arena()->Result<array<Case>,Error> {
  arena scratch {
    text:=document().clone_in(scratch)
    value:array<Case>:=json.decode(text)?
    return Ok(value)
  }
}
pub fn controls(way:i64)->Result<array<Case>,Error> {
  if way<0 {discard:=decode(document())?;return Err(Error.Invalid)}
  raw:=if way==0 {owned()} else {match way {1=>in_arena(),_=>loop {break owned()}}}
  return identity(raw.map_err(map))
}
pub fn direct()->array<Case> = decode(document()) else {loop {}}
pub fn early()->Result<array<Case>,Error> {
  value:array<Case>:=json.decode({return Err(Error.Invalid); "[]"})?
  return Ok(value)
}
"#;

const MAIN: &str = r#"module main
import model
import core.json
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
fn first(borrow value:model.Case)->bool = value.id=="first" && value.prompt=="が" && value.text.len()==1 && value.text.bytes()[0]==0 && value.ids.len()==2 && value.ids[1]==-2
fn empty(borrow value:model.Case)->bool = value.id=="" && value.prompt=="" && value.text=="" && value.ids.len()==0
fn verify(borrow values:array<model.Case>)->bool = values.len()==2 && first(values[0]) && empty(values[1])
fn exercise()->i32 {
  mut way:=-1
  loop {
    if way==3 {break}
    match model.controls(way) {
      Ok(values)=>{if way<0 || !verify(values) {return 1}},
      Err(error)=>{if way>=0 {return 2}}
    }
    way=way+1
  }
  mut values:=model.owned() else {return 3}
  values=model.in_arena() else {return 4}
  if !verify(values) {return 5}
  discarded:Result<array<model.Case>,Error>:=model.decode(model.document())
  match model.decode("[{\"id\":\"a\",\"prompt\":\"b\",\"text\":\"c\",\"ids\":[1]},{\"id\":\"d\",\"prompt\":\"e\",\"text\":\"f\",\"ids\":false}]") {Ok(unexpected)=>{return 9},Err(error)=>match error {Code(code)=>{if code!=1 {return 10}},_=>{return 11}}}
  match model.decode("[{}] x") {Ok(unexpected)=>{return 12},Err(error)=>{}}
  match model.early() {Ok(unexpected)=>{return 13},Err(error)=>{}}
  direct:=model.direct()
  if !verify(direct) {return 16}
  none:=model.decode("[]") else {return 14}
  if none.len()!=0 {return 15}
  return 0
}
fn roundtrip()->i32 {
  values:=model.owned() else {return 6}
  encoded:=json.encode(values) else {return 7}
  decoded:=model.decode(encoded) else {return 8}
  if !verify(decoded) {return 19}
  deep:=model.deep("[{\"child\":{\"text\":\"one\"},\"maybe\":{\"text\":\"two\"},\"children\":[{\"text\":\"three\"}],\"notes\":[\"four\",\"\"],\"nums\":[1.5],\"flag\":true},{\"child\":{\"text\":\"\"},\"children\":[],\"notes\":null,\"nums\":[],\"flag\":false}]") else {return 22}
  deep_text:=json.encode(deep) else {return 23}
  if deep_text!="[{\"child\":{\"text\":\"one\"},\"maybe\":{\"text\":\"two\"},\"children\":[{\"text\":\"three\"}],\"notes\":[\"four\",\"\"],\"nums\":[1.5],\"flag\":true},{\"child\":{\"text\":\"\"},\"children\":[],\"nums\":[],\"flag\":false}]" {return 24}
  generic:array<model.Wrapped<string>>:=model.generic("[{\"value\":\"generic\"}]") else {return 25}
  generic_text:=json.encode(generic) else {return 26}
  if generic_text!="[{\"value\":\"generic\"}]" {return 27}
  return 0
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  before:=unsafe {align_rt_alloc_count()}
  free:=unsafe {align_rt_free_count()}
  status:=exercise()
  if status!=0 {return status}
  count:=unsafe {align_rt_alloc_count()}-before
  if count<=0 || unsafe {align_rt_free_count()}-free!=count || unsafe {align_rt_requested_live_bytes()}!=0 {print(count);print(unsafe {align_rt_free_count()}-free);print(unsafe {align_rt_requested_live_bytes()});return 20}
  result:=roundtrip()
  if result!=0 {return result}
  if unsafe {align_rt_requested_live_bytes()}!=0 {return 21}
  return 0
}
"#;

fn execute(stage: &Path, programs: &[align_mir::Program], label: &str) {
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
            false,
        )
        .unwrap();
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
        &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        &binary,
        &libraries,
        Profile::Release,
    )
    .unwrap();
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(
        output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
}

#[test]
fn json_owned_record_array_lifecycle_and_replay() {
    owned_fixture::run("json_owned_record_array_lifecycle_and_replay", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, MAIN).unwrap();
        let entry = entry.to_str().unwrap();
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, entry, MAIN);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[program], "whole");
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry, MAIN);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &built.diags)
        );
        execute(
            stage,
            &built
                .units
                .into_iter()
                .map(|unit| unit.mir)
                .collect::<Vec<_>>(),
            "units",
        );
        let cache = align_driver::CacheContext::at(stage.join("cache"));
        for round in 0..2 {
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sources,
                entry,
                MAIN,
                &cache,
                align_driver::UnitReuse::Allowed,
            );
            assert!(
                !built.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &built.diags)
            );
            assert_eq!(built.units.len(), 2);
            assert!(
                built
                    .units
                    .iter()
                    .all(|unit| unit.frontend.as_ref().unwrap().hit == (round == 1))
            );
            let programs = (0..built.units.len())
                .map(|i| built.materialize(i).unwrap().clone())
                .collect::<Vec<_>>();
            if round == 1 {
                execute(stage, &programs, "cache-hit");
            }
        }
    });
}

#[test]
fn json_owned_record_array_rejects_invalid_source_and_view_escape() {
    owned_fixture::run(
        "json_owned_record_array_rejects_invalid_source_and_view_escape",
        |stage| {
            let cases = [
                (
                    "R{text:string}\nfn bad()->Result<str,Error>{values:array<R>:=json.decode(\"[]\")?;return Ok(values[0].text)}",
                    Some("cannot return"),
                ),
                (
                    "R{text:string}\nfn bad()->Result<(),Error>{mut values:array<R>:=json.decode(\"[]\")?;view:str:=values[0].text;values=json.decode(\"[]\")?;print(view);return Ok(())}",
                    Some("invalidated borrow"),
                ),
                (
                    "R{text:string,view:str}\nfn bad()->Result<array<R>,Error> = json.decode(\"[]\")",
                    None,
                ),
                (
                    "R{text:string}\nfn bad()->Result<soa<R>,Error> = json.decode(\"[]\")",
                    None,
                ),
                (
                    "layout(C) R{text:string}\nfn bad()->Result<array<R>,Error> = json.decode(\"[]\")",
                    None,
                ),
                (
                    "R{text:string}\nfn bad()->Result<array<R>,Error> = json.decode(17)",
                    None,
                ),
                (
                    "R{text:string}\nfn bad()->Result<array<R>,Error> = json.decode(missing())",
                    Some("undefined function"),
                ),
                (
                    "R{text:string}\nfn bad()->Result<array<R>,Error> = json.decode(\"[]\",\"[]\")",
                    Some("expects 1 argument"),
                ),
            ];
            for (index, (source, expected)) in cases.iter().enumerate() {
                let source = format!("import core.json\n{source}\nfn main() {{}}\n");
                let path = stage.join(format!("bad-{index}.align"));
                std::fs::write(&path, &source).unwrap();
                for per_unit in [false, true] {
                    let mut sources = SourceMap::new();
                    let diagnostics = if per_unit {
                        align_driver::build_per_unit(&mut sources, path.to_str().unwrap(), &source)
                            .diags
                    } else {
                        align_driver::check(&mut sources, path.to_str().unwrap(), &source).diags
                    };
                    let rendered = align_driver::format_diagnostics(&sources, &diagnostics);
                    assert!(
                        diagnostics.has_errors(),
                        "invalid source {index}/{per_unit}"
                    );
                    if let Some(expected) = expected {
                        assert!(
                            rendered.contains(expected),
                            "{index}/{per_unit}: {rendered}"
                        );
                    }
                }
            }
        },
    );
}
