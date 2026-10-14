//! Media AM-R06: existing AoS subslices beside fallible owned returns.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
pub Row<T> { key:i64,value:T }
pub Stage { data:buffer,total:i64 }
pub fn identity<T>(value:T)->T = value
pub fn map(error:Error)->Error = error
pub fn copy()->Result<array<Row<i64>>,Error> {
  mut output:array_builder<Row<i64>>:=array_builder(2)
  output.push(Row {key:2,value:3})
  output.push(Row {key:5,value:7})
  return Ok(output.build())
}
pub fn owned(fail:bool)->Result<array<Row<string>>,Error> {
  mut output:array_builder<Row<string>>:=array_builder(2)
  output.push(Row {key:2,value:"abc".clone()})
  if fail {return Err(Error.Invalid)}
  output.push(Row {key:5,value:"defghij".clone()})
  return Ok(output.build())
}
pub fn inspect(view:slice<Row<i64>>,fail:bool)->Result<i64,Error> {
  if fail || view.len()==0 {return Err(Error.Invalid)}
  return Ok(view[0].key+view[0].value)
}
pub fn inspect_owned(view:slice<Row<string>>,fail:bool)->Result<i64,Error> {
  if fail || view.len()==0 {return Err(Error.Invalid)}
  return Ok(view[0].key+view[0].value.len())
}
pub fn stage(way:i64)->Result<Stage,Error> {
  cache:=identity(copy()?)
  text:=owned(way==-2)?
  mut total:=0
  mut i:=0
  loop {
    if i==2 {break}
    span:=if i==0 {cache[..]} else {cache[1..2]}
    total=total+inspect(span[..],way==-1)?
    i=i+1
  }
  view:=match way {0=>text[..],_=>text[1..2]}
  total=total+inspect_owned(view[..],false).map_err(map)?
  data:=buffer.try_new(total)?
  return Ok(Stage {data:data,total:total})
}
"#;

const MAIN: &str = r#"module main
import model
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
fn exercise()->i32 {
  mut way:=-2
  loop {
    if way==2 {break}
    match model.stage(way) {
      Ok(value)=>{if way<0 || value.total!=if way==0 {22} else {29} {return 1}},
      Err(error)=>{if way>=0 {return 2}}
    }
    way=way+1
  }
  mut value:=model.stage(0) else {return 3}
  value=model.stage(1) else {return 4}
  discarded:=model.stage(0)
  rows:=model.copy() else {return 5}
  before:=unsafe {align_rt_alloc_count()}
  freed:=unsafe {align_rt_free_count()}
  view:=rows[0..2]
  shortened:=view[1..]
  if (model.inspect(shortened,false) else -1)!=12 {return 6}
  match model.inspect(rows[0..0],false) {Ok(unexpected)=>{return 7},Err(error)=>{}}
  if unsafe {align_rt_alloc_count()}!=before || unsafe {align_rt_free_count()}!=freed {return 8}
  return 0
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  status:=exercise()
  if status!=0 {return status}
  if unsafe {align_rt_requested_live_bytes()}!=0 {return 20}
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
fn record_subslice_lifecycle_and_replay() {
    owned_fixture::run("record_subslice_lifecycle_and_replay", |stage| {
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
