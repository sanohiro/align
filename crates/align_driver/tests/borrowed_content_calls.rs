//! Plan195: mutable calls retain readable content and exclusive storage separately.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn run_programs(
    stage: &Path,
    programs: &[align_mir::Program],
    label: &str,
    rt_lto: bool,
) -> TestResult<std::process::Output> {
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
    Ok(output)
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

fn execute(stage: &Path, source: &str, name: &str) -> TestResult {
    for per_unit in [false, true] {
        let built = programs(stage, source, per_unit)
            .map_err(|error| format!("{name}/{per_unit}: {error}"))?;
        let output = run_programs(stage, &built, &format!("{name}-{per_unit}"), false)
            .map_err(|error| format!("{name}/{per_unit}: {error}"))?;
        assert!(
            output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
            "{name}/{per_unit}: {output:?}"
        );
    }
    Ok(())
}

#[test]
fn values_and_storage() {
    owned_fixture::run("values_and_storage", |stage| {
        execute(stage, r#"
R {text:array<str>,numbers:array<i64>,fixed:[str;1],owner:string}
A {text:array<str>,numbers:array<i64>,fixed:[str;1]}
Outer {inner:A}
fn numbers(borrow mut a:array<i64>){a[0]=7}
fn texts(borrow mut a:array<str>){a[0]="new"}
fn row(borrow mut r:R){r.text[0]="new";r.numbers[0]=7;r.fixed[0]="new"}
fn nested(borrow mut r:Outer){r.inner.text[0]="new";r.inner.fixed[0]="new"}
fn forwarding(borrow mut r:R){row(r);row(r)}
fn arena_row(borrow mut r:A){r.text[0]="new";r.numbers[0]=7;r.fixed[0]="new"}
fn main()->i32 {
 mut a:=[1].to_array();numbers(a)
 mut b:=["old"].to_array();texts(b)
 mut r:=R{text:["old"].to_array(),numbers:[1].to_array(),fixed:["old"],owner:"kept".clone()}
 forwarding(r)
 if a[0]!=7 || b[0]!="new" || r.text[0]!="new" || r.numbers[0]!=7 || r.fixed[0]!="new" || r.owner!="kept" {return 1}
 arena {
  mut aa:=[1].to_array();numbers(aa);numbers(aa)
  mut bb:=["old"].to_array();texts(bb);texts(bb)
  mut rr:=A{text:["old"].to_array(),numbers:[1].to_array(),fixed:["old"]}
  arena_row(rr);arena_row(rr)
  if aa[0]!=7 || bb[0]!="new" || rr.text[0]!="new" || rr.numbers[0]!=7 || rr.fixed[0]!="new" {return 2}
  mut outer:=Outer{inner:A{text:["old"].to_array(),numbers:[1].to_array(),fixed:["old"]}};nested(outer)
  if outer.inner.text[0]!="new" || outer.inner.fixed[0]!="new" {return 3}
 }
 return 0
}"#, "values").unwrap();
    });
}

#[test]
fn control_and_lifecycle() {
    owned_fixture::run("control_and_lifecycle", |stage| {
        execute(stage, r#"
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
R {text:array<str>,numbers:array<i64>,owner:string}
A {text:array<str>,numbers:array<i64>}
fn fresh()->R=R{text:["old"].to_array(),numbers:[1].to_array(),owner:"kept".clone()}
fn write(borrow mut r:R){r.text[0]="new";r.numbers[0]=7}
fn replace(borrow mut r:R){r=fresh();write(r)}
fn arena_write(borrow mut r:A){r.text[0]="new";r.numbers[0]=7}
fn fail()->Result<i64,Error> = Err(Error.Invalid)
fn mapper(e:Error)->Error=e
fn failing(borrow mut r:R)->Result<(),Error>{write(r);unused:=fail().map_err(mapper)?;return Ok(())}
fn early(borrow mut r:R){write(r);return}
fn exercise()->Result<(),Error>{
 mut r:=fresh()
 before:=unsafe {align_rt_alloc_count()};freed:=unsafe {align_rt_free_count()}
 if true {write(r)} else {early(r)}
 match 1 {1=>write(r),_=>early(r)}
 mut i:=0
 loop {if i==3 {break};write(r);i=i+1}
 option:Option<i64>:=None
 unused:=option else {early(r);4}
 result:Result<i64,Error>:=Err(Error.Invalid)
 other:=result else {write(r);5}
 match failing(r) {Ok(_)=>{return Err(Error.Invalid)},Err(_)=>{}}
 if unsafe {align_rt_alloc_count()}!=before || unsafe {align_rt_free_count()}!=freed {return Err(Error.Invalid)}
 replace(r)
 moved:=r
 if moved.text[0]!="new" || moved.numbers[0]!=7 || moved.owner!="kept" {return Err(Error.Invalid)}
 arena {
  mut ar:=A{text:["old"].to_array(),numbers:[1].to_array()}
  arena_write(ar);arena_write(ar)
  if ar.text[0]!="new" || ar.numbers[0]!=7 {return Err(Error.Invalid)}
 }
 return Ok(())
}
fn main()->i32 {
 before:=unsafe {align_rt_alloc_count()};freed:=unsafe {align_rt_free_count()}
 live:=unsafe {align_rt_requested_live_bytes()}
 match exercise(){Err(_)=>{return 1},Ok(_)=>{}}
 if unsafe {align_rt_alloc_count()}-before!=unsafe {align_rt_free_count()}-freed || unsafe {align_rt_requested_live_bytes()}!=live {return 2}
 return 0
}"#, "control").unwrap();
    });
}
