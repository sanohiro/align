//! Request134: borrowed inline Copy data beside fresh owned storage.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
pub Capacity { a:i64,b:i64,c:i64,d:i64 }
pub Plan { capacity:Capacity }
pub Owned { values:array<i64>,capacity:Capacity }
pub Reversed { capacity:Capacity,values:array<i64> }
pub Meta<T> { value:T,fixed:[i64;3] }
pub Nested { capacity:Meta<Capacity>,values:array<i64> }
pub fn identity<T>(value:T)->T = value
pub fn guard(way:i64)->Result<(),Error> {if way<0 {return Err(Error.Invalid)};return Ok(())}
pub fn map(error:Error)->Error = error
pub fn construct(borrow plan:Plan)->Result<Owned,Error> {
  capacity:=plan.capacity
  values:=[1,2].to_array()
  return Ok(Owned {values:values,capacity:capacity})
}
pub fn direct(borrow plan:Plan)->Reversed {
  capacity:=identity(plan.capacity)
  values:=[3,4].to_array()
  return Reversed {capacity:capacity,values:values}
}
pub fn nested(borrow plan:Meta<Capacity>,way:i64)->Result<Nested,Error> {
  capacity:=if way==0 {plan} else {identity(plan)}
  selected:=match way {0=>capacity,_=>plan}
  looped:=loop {break selected}
  values:=[5,6].to_array()
  if way == -1 {return Err(Error.Invalid)}
  guard(way)?
  return Ok(Nested {capacity:looped,values:values})
}
"#;

const MAIN: &str = r#"module main
import model
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
fn live()->i64 = unsafe {align_rt_requested_live_bytes()}
fn verify(borrow capacity:model.Capacity)->bool = capacity.a==11 && capacity.b==22 && capacity.c==33 && capacity.d==44
fn plan()->model.Plan = model.Plan {capacity:model.Capacity {a:11,b:22,c:33,d:44}}
fn meta()->model.Meta<model.Capacity> {local:=plan();return model.Meta {value:local.capacity,fixed:[7,8,9]}}
fn source_expiry()->Result<model.Owned,Error> {local:=plan();return model.construct(local)}
fn exact()->i32 {
  mut local:=plan()
  owned:=model.construct(local) else {return 1}
  local.capacity.a=99
  if !verify(owned.capacity) || owned.values.len()!=2 || owned.values[0]!=1 || owned.values[1]!=2 {return 2}
  later:=source_expiry() else {return 3}
  if !verify(later.capacity) || later.values[1]!=2 {return 4}
  return 0
}
fn direct()->i32 {
  local:=plan()
  mut owned:=model.direct(local)
  if !verify(owned.capacity) || owned.values[0]!=3 || owned.values[1]!=4 {return 5}
  owned=model.direct(local)
  discarded:=model.direct(local)
  if !verify(owned.capacity) || !verify(discarded.capacity) {return 6}
  return 0
}
fn fallible(way:i64)->Result<i32,Error> {
  local:=meta()
  owned:=model.nested(local,way).map_err(model.map)?
  if !verify(owned.capacity.value) || owned.capacity.fixed[0]!=7 || owned.capacity.fixed[2]!=9 || owned.values[1]!=6 {return Ok(7)}
  return Ok(0)
}
fn controls()->i32 {
  mut way:=-2
  loop {
    if way==3 {break}
    match fallible(way) {
      Ok(status)=>{if way<0 || status!=0 {return 8}},
      Err(error)=>{if way>=0 {return 9}}
    }
    way=way+1
  }
  return 0
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  {before:=unsafe {align_rt_alloc_count()};free:=unsafe {align_rt_free_count()};status:=exact();if status!=0 {return status};count:=unsafe {align_rt_alloc_count()}-before;if count!=2 || unsafe {align_rt_free_count()}-free!=count || live()!=0 {return 20}}
  {before:=unsafe {align_rt_alloc_count()};free:=unsafe {align_rt_free_count()};status:=direct();if status!=0 {return status};count:=unsafe {align_rt_alloc_count()}-before;if count!=3 || unsafe {align_rt_free_count()}-free!=count || live()!=0 {return 21}}
  {before:=unsafe {align_rt_alloc_count()};free:=unsafe {align_rt_free_count()};status:=controls();if status!=0 {return status};count:=unsafe {align_rt_alloc_count()}-before;if count!=5 || unsafe {align_rt_free_count()}-free!=count || live()!=0 {return 22}}
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
fn copy_record_lifecycle_and_replay() {
    owned_fixture::run("copy_record_lifecycle_and_replay", |stage| {
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
fn copy_values_do_not_extend_views_or_source_authority() {
    owned_fixture::run(
        "copy_values_do_not_extend_views_or_source_authority",
        |stage| {
            for (name, source) in [
                (
                    "shared",
                    "Capacity {n:i64}\nPlan {capacity:Capacity}\nfn write(borrow mut value:Capacity) {value.n=2}\nfn bad(borrow plan:Plan) {copy:=plan.capacity;write(plan.capacity)}\n",
                ),
                (
                    "view",
                    "View {text:str}\nOwned {view:View,values:array<i64>}\nfn bad()->Owned {text:=\"short\".clone();borrowed:str:=text;view:=View {text:borrowed};values:=[1,2].to_array();return Owned {view:view,values:values}}\n",
                ),
                (
                    "move",
                    "Plan {values:array<i64>}\nfn bad(borrow plan:Plan)->array<i64> = plan.values\n",
                ),
            ] {
                let entry = stage.join(format!("{name}.align"));
                std::fs::write(&entry, source).unwrap();
                for per_unit in [false, true] {
                    let mut sources = SourceMap::new();
                    let diags = if per_unit {
                        align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), source)
                            .diags
                    } else {
                        align_driver::check(&mut sources, entry.to_str().unwrap(), source).diags
                    };
                    let text = align_driver::format_diagnostics(&sources, &diags);
                    assert!(diags.has_errors(), "accepted {name}/{per_unit}");
                    let expected = match name {
                        "move" => text.contains("moving a nested struct field"),
                        "shared" => text.contains("borrow"),
                        _ => text.contains("cannot return a view that borrows local storage"),
                    };
                    assert!(expected, "{name}: {text}");
                    assert!(
                        !text.contains("parse error") && !text.contains("unknown"),
                        "{name}: {text}"
                    );
                }
            }
        },
    );
}
