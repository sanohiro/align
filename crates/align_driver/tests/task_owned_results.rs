//! Plan228: owned task output transfers once and survives the group; untaken output drops.
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
    let output = std::process::Command::new(&binary).output()?;
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
fn task_owned_roots_execute() {
    owned_fixture::run("task_owned_roots_execute", |stage| {
        let result = run_both(
            stage,
            r#"module main
import core.bytes
Row { name:string, values:array<i64> }
Choice { Text(string), Off }
fn numbers()->array<i64> { mut b:array_builder<i64>:=array_builder(); b.push(17); b.push(29); b.build() }
fn make()->Row = Row {name:"row".clone(),values:numbers()}
fn bytes_out()->buffer {mut b:=buffer(2); b.append("GH".bytes()); b}
fn attempt()->Result<Row,Error> = Ok(make())
fn rows()->array<Row> {mut b:array_builder<Row>:=array_builder();b.push(make());b.build()}
fn pair_attempt()->Result<(string,i64),Error> = Ok(("fallible".clone(),7))
fn main()->Result<(),Error> {
  mut output:=task_group {
    t:=spawn(fn {make()})
    wait()
    t.get()
  }
  if output.name!="row" || output.values[1]!=29 {return Err(error(1))}
  output=make()
  task_group {
    text:=spawn(fn {"hello".clone()})
    bytes:=spawn(fn {bytes_out()})
    numbers_task:=spawn(fn {numbers()})
    pair:=spawn(fn {("tuple".clone(),19)})
    choice:=spawn(fn {Choice.Text("sum".clone())})
    fallible:=spawn(fn {attempt()})
    root_rows:=spawn(fn {rows()})
    fallible_pair:=spawn(fn {pair_attempt()})
    copy_pair:=spawn(fn {(11,true)})
    empty_sum:=spawn(fn {Choice.Off})
    wait()?
    text_value:=text.get()
    bytes_value:=bytes.get()
    mut numbers_value:=numbers_task.get()
    pair_value:=pair.get()
    choice_value:=choice.get()
    record:=fallible.get()
    rows_value:=root_rows.get()
    pair_attempt_value:=fallible_pair.get()
    copied:=copy_pair.get()
    copied_again:=copy_pair.get()
    empty_sum.get()
    if rows_value.len()!=1 || pair_attempt_value.0!="fallible" || copied.0!=copied_again.0 {return Err(error(5))}
    numbers_value[1]=31
    if text_value!="hello" || bytes_value.len()!=2 || numbers_value[0]!=17 || pair_value.0!="tuple" || pair_value.1!=19 || record.name!="row" {return Err(error(2))}
    match choice_value {Text(value)=>{if value!="sum" {return Err(error(3))}},Off=>{return Err(error(4))}}
  }
  Ok(())
}
"#,
            "owned-roots",
        );
        assert!(result.is_ok(), "{result:?}");
    });
}

#[test]
fn task_owned_rejects_double_take_borrows_and_invalid_waits() {
    owned_fixture::run(
        "task_owned_rejects_double_take_borrows_and_invalid_waits",
        |stage| {
            let result = (|| -> TestResult {
                for (name, declarations, body, expected) in [
                    (
                        "twice",
                        "",
                        "t:=spawn(fn {make()});wait();a:=t.get();b:=t.get()",
                        "moved",
                    ),
                    (
                        "borrowed-twice",
                        "fn size(borrow x:array<i64>)->i64=x.len()",
                        "t:=spawn(fn {make()});wait();size(t.get());size(t.get())",
                        "moved",
                    ),
                    (
                        "before-wait",
                        "",
                        "t:=spawn(fn {make()});t.get();wait()",
                        "wait",
                    ),
                    (
                        "inner-wait",
                        "",
                        "t:=spawn(fn {make()});task_group {wait();t.get()};wait()",
                        "wait",
                    ),
                    (
                        "failed-wait",
                        "fn attempt()->Result<array<i64>,Error> = Ok(make())",
                        "t:=spawn(fn {attempt()});wait();t.get()",
                        "successful",
                    ),
                    (
                        "view",
                        "",
                        "spawn(fn {\"view\"});wait()",
                        "self-contained data",
                    ),
                    (
                        "nested-view",
                        "Bad {value:str}",
                        "spawn(fn {Bad {value:\"view\"}});wait()",
                        "self-contained data",
                    ),
                    (
                        "empty-view",
                        "Bad {value:[str;0]}",
                        "spawn(fn {Bad {value:[]}});wait()",
                        "self-contained data",
                    ),
                    (
                        "inactive-view",
                        "Choice {Good, Bad(str)}",
                        "spawn(fn {Choice.Good});wait()",
                        "self-contained data",
                    ),
                    (
                        "worker-arena",
                        "",
                        "spawn(fn {arena {make_local:=[1,2].to_array();make_local}});wait()",
                        "arena",
                    ),
                    (
                        "handle-escape",
                        "",
                        "escape:=task_group {t:=spawn(fn {make()});wait();t}",
                        "escape",
                    ),
                ] {
                    let source = format!(
                        "module main\n{declarations}\nfn make()->array<i64> = [1,2].to_array()\nfn main()->Result<(),Error>{{task_group{{{body}}};Ok(())}}\n"
                    );
                    let path = stage.join(format!("{name}.align"));
                    std::fs::write(&path, &source)?;
                    for per_unit in [false, true] {
                        let mut sources = SourceMap::new();
                        let entry = path.to_str().ok_or("non-UTF8 path")?;
                        let diagnostics = if per_unit {
                            align_driver::check_per_unit(&mut sources, entry, &source).diags
                        } else {
                            align_driver::check(&mut sources, entry, &source).diags
                        };
                        let text = align_driver::format_diagnostics(&sources, &diagnostics);
                        assert!(
                            diagnostics.has_errors() && text.contains(expected),
                            "{name}/{per_unit}: expected {expected}\n{text}"
                        );
                    }
                }
                Ok(())
            })();
            assert!(result.is_ok(), "{result:?}");
        },
    );
}

const LIFECYCLE: &str = r#"module main
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_requested_live_peak()->i64
Row {name:string}
Choice {Text(string), Off}
Record {text:string, bytes:buffer, names:array<string>, rows:array<Row>, optional:Option<string>, choice:Choice, fixed:[string;2]}
fn make(n:i64)->Record {
  mut names:array_builder<string>:=array_builder()
  names.push("first".clone()); names.push("second".clone())
  mut rows:array_builder<Row>:=array_builder()
  rows.push(Row {name:"row".clone()})
  mut bytes:=buffer(1);bytes.append("buffer payload".bytes())
  Record {text:"text".clone(),bytes:bytes,names:names.build(),rows:rows.build(),optional:Some("option".clone()),choice:Choice.Text("choice".clone()),fixed:["a".clone(),"b".clone()]}
}
fn check(borrow value:Record)->bool = value.text=="text" && value.bytes.len()==14 && value.names.len()==2 && value.rows.len()==1 && value.fixed[1]=="b"
fn attempt(n:i64)->Result<Record,Error> {value:=make(n);if n<0 {return Err(error(37))};Ok(value)}
fn fail()->Result<Record,Error> = Err(error(43))
fn consume(a:Record,b:Record) {}
fn early()->Result<Record,Error> {
 task_group {t:=spawn(fn {attempt(1)});wait().map_err(fn e:Error {e})?;return Ok(t.get())}
}
fn failed()->Result<(),Error> {
 task_group {a:=spawn(fn {attempt(-1)});b:=spawn(fn {attempt(2)});c:=spawn(fn {fail()});wait()?;a.get();b.get();c.get()}
 Ok(())
}
fn eager_failure()->Result<(),Error> {
 task_group {t:=spawn(fn {make(0)});wait();consume(t.get(),fail()?)}
 Ok(())
}
fn ignored_wait() {task_group {spawn(fn {attempt(-1)});spawn(fn {attempt(2)});ignored:=wait();()}}
fn exercise()->Result<(),Error> {
 task_group {
   ignored:=spawn(fn {make(1)})
   taken:=spawn(fn {make(2)})
   wait()
   value:=taken.get()
   if !check(value) {return Err(error(1))}
   spawn(fn {make(3)})
   wait()
   spawn(fn {make(4)}); ()
 }
 task_group {
   t:=spawn(fn {make(1)});u:=spawn(fn {make(2)});wait()
   t.get()
   value:=u.get();if !check(value) {return Err(error(2))}
   text:=spawn(fn {"temporary".clone()});wait();if text.get().len()!=9 {return Err(error(12))}
 }
 task_group {
   mut replaced:=spawn(fn {make(1)})
   replaced=spawn(fn {make(2)})
   moved:=replaced
   wait()
   wrapped:=Some(moved.get())
 }
 mut output:=arena {task_group {t:=spawn(fn {make(1)});wait();t.get()}}
 if !check(output) {return Err(error(3))}
 output=early()?
 if !check(output) {return Err(error(4))}
 mut source:="captured".clone()
 view:str:=source
 independent:=task_group {t:=spawn(fn {make(view.len())});wait();t.get()}
 source="replacement".clone()
 if !check(independent) {return Err(error(5))}
 task_group {
   t:=spawn(fn {task_group {inner:=spawn(fn {make(1)});wait();inner.get()}})
   wait();value:=t.get();if !check(value) {return Err(error(6))}
 }
 mut index:=0
 loop {
   if index==3 {break}
   value:=task_group {t:=spawn(fn {match index {0=>make(0),_=>loop {break make(index)}}});wait();t.get()}
   if !check(value) {return Err(error(7))}
   index=index+1
 }
 match failed() {Err(e)=>{match e {Code(code)=>{if code!=37 {return Err(error(8))}},_=>{return Err(error(9))}}},_=>{return Err(error(9))}}
 match eager_failure() {Err(e)=>{match e {Code(code)=>{if code!=43 {return Err(error(10))}},_=>{return Err(error(11))}}},_=>{return Err(error(11))}}
 ignored_wait()
 Ok(())
}
fn main()->i32 {
 unsafe {align_rt_requested_live_reset()}
 match exercise() {Err(e)=>{match e {Code(c)=>print(c),_=>print(-1)};return 1},Ok(_)=>{}}
 live:=unsafe {align_rt_requested_live_bytes()}
 if live!=0 || unsafe {align_rt_requested_live_peak()}<=0 {print(live);return 2}
 0
}
"#;

#[test]
fn task_owned_lifecycle_every_exit() {
    owned_fixture::run("task_owned_lifecycle_every_exit", |stage| {
        let result = run_both(stage, LIFECYCLE, "lifecycle");
        assert!(result.is_ok(), "{result:?}");
    });
}

#[test]
fn task_owned_imported_generics_and_cache() {
    owned_fixture::run("task_owned_imported_generics_and_cache", |stage| {
        let result = (|| -> TestResult {
            std::fs::write(
                stage.join("model.align"),
                r#"module model
pub Row {value:string}
pub fn identity<T>(value:T)->T=value
pub fn make()->Row=Row {value:"imported".clone()}
pub fn attempt()->Result<Row,Error> = Ok(identity(make()))
"#,
            )?;
            let source = r#"module main
import model
fn main()->Result<(),Error> {
 result:=task_group {t:=spawn(fn {model.identity(model.attempt())});wait()?;t.get()}
 if result.value!="imported" {return Err(error(1))}
 Ok(())
}
"#;
            run_both(stage, source, "imports")?;
            let path = stage.join("main.align");
            let entry = path.to_str().ok_or("non-UTF8 path")?;
            let cache = align_driver::CacheContext::at(stage.join("cache"));
            for round in 0..2 {
                let mut sources = SourceMap::new();
                let mut built = align_driver::build_package(
                    &mut sources,
                    entry,
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
                        .is_some_and(|frontend| frontend.hit == (round == 1))
                }));
                let mut programs = Vec::new();
                for index in 0..built.units.len() {
                    programs.push(
                        built
                            .materialize(index)
                            .map_err(|error| format!("{error:?}"))?
                            .clone(),
                    );
                }
                execute(stage, &programs, &format!("cache-{round}"), false)?;
            }
            Ok(())
        })();
        assert!(result.is_ok(), "{result:?}");
    });
}

#[test]
fn task_owned_bands_match_every_sequential_byte() {
    owned_fixture::run("task_owned_bands_match_every_sequential_byte", |stage| {
        let result = (|| -> TestResult {
            std::fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../bench/task_owned_results/kernel.align"),
                stage.join("kernel.align"),
            )?;
            run_both(
                stage,
                r#"module main
import kernel
fn main()->i32 {
 mut input:=buffer(8197)
 mut i:=0
 loop {if i==8197 {break};input.put_u8((i%251) as u8);i=i+1}
 expected:=kernel.sequential(input.bytes())
 actual:=kernel.parallel(input.bytes())
 if actual.len()!=expected.len() {return 1}
 mut j:=0
 loop {if j==actual.len() {break};if actual.bytes()[j]!=expected.bytes()[j] {return 2};j=j+1}
 0
}
"#,
                "band-parity",
            )
        })();
        assert!(result.is_ok(), "{result:?}");
    });
}
