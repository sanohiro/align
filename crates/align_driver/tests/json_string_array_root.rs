//! Independent JSON owned-text roots and canonical cleanup (Plans183/203).
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
import core.json
pub fn decode(input:str)->Result<array<string>,Error> = json.decode(input)
pub fn identity<T>(value:T)->T = value
pub fn map(error:Error)->Error = error
pub fn owned()->Result<array<string>,Error> {
  mut input:="[\"plain\",\"\\u304c\",\"\\u0000\",\"\"]".clone()
  value:array<string>:=json.decode(input)?
  input="replaced".clone()
  return Ok(value)
}
pub fn in_arena()->Result<array<string>,Error> {
  arena scratch {
    text:="[\"plain\",\"\\u304c\",\"\\u0000\",\"\"]".clone_in(scratch)
    value:array<string>:=json.decode(text)?
    return Ok(value)
  }
}
pub fn numeric()->Result<array<i64>,Error> {
  arena {numbers:array<i64>:=json.decode("[1,2]")?;return Ok(numbers)}
}
pub fn controls(way:i64)->Result<array<string>,Error> {
  if way<0 {discard:=decode("[\"lost\"]")?;return Err(Error.Invalid)}
  raw:=if way==0 {owned()} else {match way {1=>in_arena(),_=>loop {break owned()}}}
  return identity(raw.map_err(map))
}
pub fn direct()->array<string> = decode("[\"direct\"]") else {loop {}}
pub fn early()->Result<array<string>,Error> {
  value:array<string>:=json.decode({return Err(Error.Invalid); "[]"})?
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
fn verify(borrow values:array<string>)->bool = values.len()==4 && values[0]=="plain" && values[1]=="が" && values[2].len()==1 && values[2].bytes()[0]==0 && values[3]==""
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
  discarded:Result<array<string>,Error>:=model.decode("[\"discarded\"]")
  match model.decode("[\"partial\",false]") {Ok(unexpected)=>{return 9},Err(error)=>match error {Code(code)=>{if code!=1 {return 10}},_=>{return 11}}}
  match model.decode("[\"partial\"] x") {Ok(unexpected)=>{return 12},Err(error)=>{}}
  match model.early() {Ok(unexpected)=>{return 13},Err(error)=>{}}
  direct:=model.direct()
  if direct.len()!=1 || direct[0]!="direct" {return 16}
  numeric:=model.numeric() else {return 17}
  if numeric.len()!=2 || numeric[1]!=2 {return 18}
  empty:=model.decode("[]") else {return 14}
  if empty.len()!=0 {return 15}
  return 0
}
fn roundtrip()->i32 {
  values:=model.owned() else {return 6}
  encoded:=json.encode(values) else {return 7}
  decoded:=model.decode(encoded) else {return 8}
  if !verify(decoded) {return 19}
  return 0
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  before:=unsafe {align_rt_alloc_count()}
  free:=unsafe {align_rt_free_count()}
  status:=exercise()
  if status!=0 {return status}
  count:=unsafe {align_rt_alloc_count()}-before
  if count!=35 || unsafe {align_rt_free_count()}-free!=count || unsafe {align_rt_requested_live_bytes()}!=0 {print(count);print(unsafe {align_rt_free_count()}-free);print(unsafe {align_rt_requested_live_bytes()});return 20}
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

fn lifecycle_and_replay(name: &'static str, model: &str, main: &str) {
    owned_fixture::run(name, |stage| {
        std::fs::write(stage.join("model.align"), model).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, main).unwrap();
        let entry = entry.to_str().unwrap();
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, entry, main);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[program], "whole");
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry, main);
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
                main,
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
fn json_string_array_root_lifecycle_and_replay() {
    lifecycle_and_replay("json_string_array_root_lifecycle_and_replay", MODEL, MAIN);
}

#[test]
fn json_string_array_root_rejects_invalid_source_and_view_escape() {
    owned_fixture::run(
        "json_string_array_root_rejects_invalid_source_and_view_escape",
        |stage| {
            let cases = [
                (
                    "fn bad()->Result<str,Error> {values:array<string>:=json.decode(\"[\\\"x\\\"]\")?;return Ok(values[0])}",
                    Some("cannot return"),
                ),
                (
                    "fn bad()->Result<(),Error> {mut values:array<string>:=json.decode(\"[\\\"x\\\"]\")?;view:=values[0];values=json.decode(\"[]\")?;print(view);return Ok(())}",
                    Some("invalidated borrow"),
                ),
                (
                    "fn bad()->Result<array<str>,Error> = json.decode(\"[]\")",
                    Some("not supported yet"),
                ),
                (
                    "fn bad()->Result<array<char>,Error> = json.decode(\"[]\")",
                    Some("not supported yet"),
                ),
                (
                    "fn bad()->Result<str,Error> = json.decode(\"\\\"x\\\"\")",
                    None,
                ),
                (
                    "fn bad()->Result<array<string>,Error> = json.decode(17)",
                    None,
                ),
                (
                    "fn bad()->Result<array<string>,Error> = json.decode(missing())",
                    Some("undefined function"),
                ),
                (
                    "fn bad()->Result<array<string>,Error> = json.decode(\"[]\",\"[]\")",
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

const STRING_MODEL: &str = r#"module model
import core.json
pub fn decode(input:str)->Result<string,Error> = json.decode(input)
pub fn identity<T>(value:T)->T = value
pub fn map(error:Error)->Error = error
pub fn owned()->Result<string,Error> {
  mut input:="\"plain\"".clone()
  value:string:=json.decode(input)?
  input="replaced".clone()
  return Ok(value)
}
pub fn in_arena()->Result<string,Error> {
  arena scratch {
    text:="\"\\u304c\\u0000\"".clone_in(scratch)
    value:string:=json.decode(text)?
    return Ok(value)
  }
}
pub fn controls(way:i64)->Result<string,Error> {
  if way<0 {discard:=decode("\"lost\"")?;return Err(Error.Invalid)}
  raw:=if way==0 {owned()} else {match way {1=>in_arena(),_=>loop {break owned()}}}
  return identity(raw.map_err(map))
}
pub fn direct()->string = decode("\"direct\"") else {loop {}}
pub fn early()->Result<string,Error> {
  value:string:=json.decode({return Err(Error.Invalid); "\"never\""})?
  return Ok(value)
}
pub fn once()->Result<string,Error> {
  mut count:=0
  value:string:=json.decode({count=count+1; "\"once\""})?
  if count!=1 {return Err(Error.Invalid)}
  return Ok(value)
}
"#;

const STRING_MAIN: &str = r#"module main
import model
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
fn equal(borrow value:string, expected:str)->bool = value[..]==expected
fn exercise()->i32 {
  mut way:=-1
  loop {
    if way==3 {break}
    match model.controls(way) {
      Ok(value)=>{
        if way<0 {return 1}
        if way==1 {
          if value.len()!=4 || value[0..3]!="が" || value.bytes()[3]!=0 {return 2}
        } else {if !equal(value,"plain") {return 3}}
        if unsafe {align_rt_requested_live_bytes()}<=0 {return 4}
      },
      Err(error)=>{if way>=0 {return 5}}
    }
    way=way+1
  }
  mut value:=model.owned() else {return 6}
  value=model.in_arena() else {return 7}
  moved:=model.identity(value)
  if moved.len()!=4 {return 8}
  discarded:Result<string,Error>:=model.decode("\"discarded\"")
  match model.decode("\"partial\" x") {
    Ok(unexpected)=>{return 9},
    Err(error)=>match error {Code(code)=>{if code!=1 {return 10}},_=>{return 11}}
  }
  match model.decode("false") {Ok(unexpected)=>{return 12},Err(error)=>{}}
  match model.early() {Ok(unexpected)=>{return 13},Err(error)=>{}}
  direct:=model.direct()
  if !equal(direct,"direct") {return 14}
  once:=model.once() else {return 15}
  if !equal(once,"once") {return 16}
  empty:=model.decode("\"\"") else {return 17}
  if empty.len()!=0 {return 18}
  fallback:=model.decode("invalid") else {"fallback".clone()}
  if !equal(fallback,"fallback") {return 19}
  if moved[0..3]!="が" || moved.bytes()[3]!=0 {return 20}
  return 0
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  before:=unsafe {align_rt_alloc_count()}
  free:=unsafe {align_rt_free_count()}
  status:=exercise()
  if status!=0 {return status}
  count:=unsafe {align_rt_alloc_count()}-before
  if count<=0 || unsafe {align_rt_free_count()}-free!=count || unsafe {align_rt_requested_live_bytes()}!=0 {
    print(count);print(unsafe {align_rt_free_count()}-free);print(unsafe {align_rt_requested_live_bytes()});return 25
  }
  return 0
}
"#;

#[test]
fn json_string_root_lifecycle_and_replay() {
    lifecycle_and_replay(
        "json_string_root_lifecycle_and_replay",
        STRING_MODEL,
        STRING_MAIN,
    );
}

#[test]
fn json_string_root_refusals() {
    owned_fixture::run("json_string_root_refusals", |stage| {
        for (index, (body, expected)) in [
            ("fn bad()->Result<str,Error> {value:string:=json.decode(\"\\\"x\\\"\")?;return Ok(value[..])}", "cannot return"),
            ("fn bad()->Result<(),Error> {mut value:string:=json.decode(\"\\\"x\\\"\")?;view:=value[..];value=json.decode(\"\\\"next\\\"\")?;print(view);return Ok(())}", "invalidated borrow"),
            ("fn bad()->Result<string,Error> = json.decode(missing())", "undefined function"),
            ("fn bad()->Result<string,Error> = json.decode(17)", "str"),
            ("fn bad()->Result<string,Error> = json.decode(\"\\\"x\\\"\",\"extra\")", "expects 1 argument"),
            ("fn bad()->Result<char,Error> = json.decode(\"\\\"x\\\"\")", "not supported yet"),
        ].iter().enumerate() {
            let source=format!("import core.json\n{body}\nfn main() {{}}\n");
            let entry=stage.join(format!("bad-{index}.align"));
            std::fs::write(&entry,&source).unwrap();
            for per_unit in [false,true] {
                let mut sources=SourceMap::new();
                let diags=if per_unit {
                    align_driver::build_per_unit(&mut sources,entry.to_str().unwrap(),&source).diags
                } else {
                    align_driver::check(&mut sources,entry.to_str().unwrap(),&source).diags
                };
                let rendered=align_driver::format_diagnostics(&sources,&diags);
                assert!(diags.has_errors() && rendered.contains(expected),"{index}/{per_unit}: {rendered}");
            }
        }
    });
}
