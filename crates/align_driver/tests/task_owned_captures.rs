//! Plan259: task inputs transfer once; aliases expire and admitted files retain identity.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn execute(stage: &Path, programs: &[align_mir::Program], label: &str, rt_lto: bool, native: Option<&Path>) -> TestResult {
    let mut objects = native.into_iter().map(Path::to_path_buf).collect::<Vec<_>>();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{index}.o"));
        align_driver::emit_object_file(
            program,
            &object,
            BuildTarget::Native,
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
        execute(stage, &mir, &format!("{label}-{per_unit}"), per_unit, None)?;
    }
    Ok(())
}


const DATA: &str = r#"module main
import model
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_requested_live_peak()->i64
fn fail(value: model.Row)->Result<model.Row,Error> = Err(error(17))
fn early()->Result<(),Error> {
  first:=model.make()
  second:=model.make()
  task_group {spawn(fn {first});spawn(fn {fail(second)});return Err(error(19))}
}
fn run()->Result<(),Error> {
  mut value:=model.make()
  mut round:=0
  loop {
    if round==4 {break}
    task_group {
      t:=spawn(fn {model.identity(value)})
      wait()
      value=t.get()
    }
    if value.name!="row" || value.values[1]!=29 {return Err(error(1))}
    round=round+1
  }
  task_group {
    a:="text".clone()
    mut bytes:=buffer(2);bytes.append("GH".bytes())
    list:=[17,29].to_array()
    pair:=("pair".clone(),23)
    optional:Option<string>:=Some("optional".clone())
    result:Result<string,Error>:=Ok("result".clone())
    choice:=model.Choice.Text("sum".clone())
    first:=spawn(fn {a})
    second:=spawn(fn {mut local:=bytes;model.append(local);local})
    third:=spawn(fn {list})
    fourth:=spawn(fn {pair})
    fifth:=spawn(fn {optional else {"missing".clone()}})
    sixth:=spawn(fn {result})
    seventh:=spawn(fn {choice})
    wait()?
    text:=first.get();output:=second.get();numbers:=third.get();two:=fourth.get()
    opt:=fifth.get();res:=sixth.get();sum:=seventh.get()
    if text!="text" || output.len()!=3 || numbers[1]!=29 || two.1!=23 || opt!="optional" || res!="result" {return Err(error(2))}
    match sum {Text(v)=>{if v!="sum" {return Err(error(3))}},Off=>{return Err(error(4))}}
  }
  task_group {
    x:=model.make();y:=model.make()
    spawn(fn {x});spawn(fn {fail(y)});ignored:=wait();()
  }
  match early() {Err(_)=>{},Ok(_)=>{return Err(error(5))}}
  value=task_group {
    owned:=model.make()
    outer:=spawn(fn {local:=owned;task_group {inner:=spawn(fn {local});wait();inner.get()}})
    wait();outer.get()
  }
  if value.name!="row" {return Err(error(6))}
  task_group {
    one:=model.make();two:=model.make()
    task:=spawn(fn {if true {model.attempt(one)} else {fail(two)}})
    wait().map_err(fn e:Error {e})?
  }
  Ok(())
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  match run() {Err(e)=>{match e {Code(c)=>print(c),_=>print(-1)};return 1},Ok(_)=>{}}
  live:=unsafe {align_rt_requested_live_bytes()}
  if live!=0 || unsafe {align_rt_requested_live_peak()}<=0 {print(live);return 2}
  0
}
"#;
const MODEL: &str = r#"module model
pub Row {name:string,values:array<i64>}
pub Choice {Text(string),Off}
pub fn make()->Row=Row {name:"row".clone(),values:[17,29].to_array()}
pub fn identity<T>(value:T)->T=value
pub fn attempt(value:Row)->Result<Row,Error> = Ok(value)
pub fn append(borrow mut value:buffer) {value.append("!".bytes())}
"#;

#[test]
fn owned_capture_roots_cleanup_and_imports() {
    owned_fixture::run("owned_capture_roots_cleanup_and_imports", |stage| {
        let result = (|| -> TestResult {
            std::fs::write(stage.join("model.align"), MODEL)?;
            run_both(stage, DATA, "owned-inputs")
        })();
        assert!(result.is_ok(), "{result:?}");
    });
}

#[test]
fn owned_capture_rejects_aliases_borrows_and_non_send_graphs() {
    owned_fixture::run("owned_capture_rejects_aliases_borrows_and_non_send_graphs", |stage| {
        for (name, source, expected) in [
            ("caller-alias", "fn main(){owner:=[1,2].to_array();view:=owner[..];task_group{spawn(fn{owner.len()});print(view[0]);wait()}}", "invalidated borrow"),
            ("alias-first", "fn main(){owner:=[1,2].to_array();view:=owner[..];task_group{spawn(fn{print(view[0]);owner.len()});wait()}}", "invalidated borrow"),
            ("owner-first", "fn main(){owner:=[1,2].to_array();view:=owner[..];task_group{spawn(fn{print(owner.len());view[0]});wait()}}", "invalidated borrow"),
            ("nested-alias", "Row{values:array<i64>}\nfn main(){owner:=Row{values:[1,2].to_array()};view:=owner.values[..];task_group{spawn(fn{owner});print(view[0]);wait()}}", "invalidated borrow"),
            ("twice", "fn main(){owner:=\"owned\".clone();task_group{spawn(fn{owner.len()});spawn(fn{owner.len()});wait()}}", "moved"),
            ("after-wait", "fn main(){owner:=\"owned\".clone();task_group{spawn(fn{owner.len()});wait()};print(owner)}", "moved"),
            ("borrowed", "fn run(borrow owner:buffer){task_group{spawn(fn{owner.len()});wait()}}\nfn main(){}", "borrowed parameter"),
            ("exclusive", "fn run(borrow mut owner:buffer){task_group{spawn(fn{owner.len()});wait()}}\nfn main(){}", "borrowed parameter"),
            ("arena", "fn main(){arena{owner:=[1,2].to_array();task_group{spawn(fn{owner.len()});wait()}}}", "arena-owned"),
            ("fixed-owned", "fn main(){owner:=[\"owned\".clone()];task_group{spawn(fn{owner.len()});wait()}}", "non-transferable owned"),
            ("stored", "fn main(){owner:=\"owned\".clone();f:=fn{owner.len()};print(f())}", "a lambda cannot capture"),
            ("nested-stored", "fn main(){owner:=\"owned\".clone();task_group{spawn(fn{local:=owner;f:=fn{local.len()};f()});wait()}}", "a lambda cannot capture"),
            ("pipeline", "fn main(){owner:=\"owned\".clone();task_group{spawn(fn{local:=owner;[1,2].map(fn x {x+local.len()}).sum()});wait()}}", "a lambda cannot capture"),
            ("view-graph", "Row{text:string,view:str}\nfn main(){row:=Row{text:\"a\".clone(),view:\"b\"};task_group{spawn(fn{row});wait()}}", "non-transferable owned"),
            ("inactive-view", "Choice{Good(string),Bad(str)}\nfn main(){row:=Choice.Good(\"a\".clone());task_group{spawn(fn{match row{Good(v)=>v.len(),Bad(v)=>v.len()}});wait()}}", "non-transferable owned"),
            ("returned-view", "fn main(){owner:=\"owned\".clone();task_group{spawn(fn{view:str:=owner;view});wait()}}", "self-contained data"),
            ("pending-reader", "fn main(){owner:=[1,2].to_array();view:=owner[..];task_group{spawn(fn{view[0]});spawn(fn{owner.len()});wait()}}", "capture is modified"),
        ] {
            let path = stage.join(format!("{name}.align"));
            std::fs::write(&path, source).unwrap();
            for per_unit in [false, true] {
                let mut sources = SourceMap::new();
                let checked = if per_unit {
                    align_driver::check_per_unit(&mut sources, path.to_str().unwrap(), source).diags
                } else {
                    align_driver::check(&mut sources, path.to_str().unwrap(), source).diags
                };
                let diagnostics = align_driver::format_diagnostics(&sources, &checked);
                assert!(checked.has_errors() && diagnostics.contains(expected), "{name}/{per_unit}: expected {expected}\n{diagnostics}");
            }
        }
    });
}

const FILE_MODEL: &str = r#"module file_model
pub Window {pack:file,bytes:buffer,count:i64}
pub fn read(value:Window,offset:i64)->Result<Window,Error> {
  pack:=value.pack
  mut bytes:=value.bytes
  count:=pack.pread_into(bytes,0,4,offset)?
  Ok(Window {pack:pack,bytes:bytes,count:count})
}
pub fn fail(pack:file)->Result<file,Error> = Err(error(37))
pub fn identity<T>(value:T)->T=value
"#;

const FILE_SOURCE: &str = r#"module main
import std.fs
import file_model
extern "C" fn r41_replace_name()->i32
extern "C" fn r41_descriptor_count()->i32
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_requested_live_peak()->i64
fn early(path:str)->Result<(),Error> {
  pack:=fs.open_ro(path)?
  task_group {spawn(fn {pack});return Err(error(19))}
}
fn failed(path:str)->Result<(),Error> {
  first:=fs.open_ro(path)?
  second:=fs.open_ro(path)?
  task_group {spawn(fn {file_model.fail(first)});spawn(fn {second});wait()?}
  Ok(())
}
fn run()->Result<(),Error> {
  admitted:=fs.open_ro("@INPUT@")?
  if unsafe {r41_replace_name()}!=0 {return Err(error(1))}
  mut window:=file_model.Window {pack:admitted,bytes:buffer(4),count:0}
  mut index:=0
  loop {
    if index==4 {break}
    task_group {
      task:=spawn(fn {file_model.read(window,index*4)})
      wait().map_err(fn e:Error {e})?
      window=task.get()
    }
    expected:=match index {0=>4,1=>4,2=>2,_=>0}
    if window.count!=expected {return Err(error(2))}
    if index<3 && window.bytes.bytes().u8(0)!=(97+index*4) as u8 {return Err(error(3))}
    index=index+1
  }
  // Return the same descriptor directly, then transfer it through a tagged capture.
  pack:=window.pack
  returned:=task_group {t:=spawn(fn {pack});wait();t.get()}
  tagged:Result<file,Error>:=Ok(returned)
  final_file:=task_group {t:=spawn(fn {tagged});wait()?;t.get()}
  mut bytes:=buffer(2)
  count:=final_file.pread_into(bytes,0,2,0)?
  if count!=2 || bytes.bytes().u8(0)!=97 {return Err(error(4))}
  match early("@INPUT@") {Err(_)=>{},Ok(_)=>{return Err(error(5))}}
  match failed("@INPUT@") {Err(_)=>{},Ok(_)=>{return Err(error(6))}}
  task_group {
    first:=fs.open_ro("@INPUT@")?
    second:=fs.open_ro("@INPUT@")?
    spawn(fn {file_model.fail(first)});spawn(fn {second});ignored:=wait();()
  }
  // Actual pread failure and success cleanup share the ordinary file/buffer owners.
  directory:=fs.open_ro("@DIR@")?
  input:=file_model.Window {pack:directory,bytes:buffer(4),count:0}
  task_group {spawn(fn {file_model.read(input,0)});match wait(){Err(_)=>{},Ok(_)=>{return Err(error(7))}}}
  Ok(())
}
fn main()->i32 {
  task_group {spawn(fn {1});wait()}
  before:=unsafe {r41_descriptor_count()}
  unsafe {align_rt_requested_live_reset()}
  match run(){Err(e)=>{match e{Code(c)=>print(c),_=>print(-1)};return 1},Ok(_)=>{}}
  if unsafe {r41_descriptor_count()}!=before {return 2}
  live:=unsafe {align_rt_requested_live_bytes()}
  if live!=0 || unsafe {align_rt_requested_live_peak()}<=0 {print(live);return 3}
  0
}
"#;

#[test]
fn file_capture_identity_errors_and_cleanup() {
    owned_fixture::run("file_capture_identity_errors_and_cleanup", |stage| {
        let result = (|| -> TestResult {
            std::fs::write(stage.join("file_model.align"), FILE_MODEL)?;
            let input = stage.join("admitted");
            let replacement = stage.join("replacement");
            let source = FILE_SOURCE
                .replace("@INPUT@", input.to_str().ok_or("input path")?)
                .replace("@DIR@", stage.to_str().ok_or("directory path")?);
            // C owns only the test's pathname substitution and fd observation, never task work.
            let native_source = stage.join("probe.c");
            std::fs::write(&native_source, format!(r#"
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
int32_t r41_replace_name(void) {{ return rename({:?}, {:?}); }}
int32_t r41_descriptor_count(void) {{
    int32_t count = 0;
    for (int fd = 0; fd < 4096; ++fd) {{ if (fcntl(fd, F_GETFD) >= 0) ++count; }}
    return count;
}}
"#, replacement.to_str().ok_or("replacement path")?, input.to_str().ok_or("input path")?))?;
            let native = stage.join("probe.o");
            let mut compiler = std::process::Command::new(align_driver::CDriver::default().program());
            compiler.args(["-O2", "-c"]);
            if cfg!(target_arch = "x86_64") { compiler.arg("-march=native"); }
            if cfg!(target_arch = "aarch64") { compiler.arg("-mcpu=native"); }
            let compiled = compiler
                .arg(&native_source).arg("-o").arg(&native).output()?;
            assert!(compiled.status.success(), "{compiled:?}");
            for per_unit in [false, true] {
                let mir = programs(stage, &source, per_unit)?;
                std::fs::write(&input, b"abcdefghij")?;
                std::fs::write(&replacement, b"ZZ")?;
                execute(stage, &mir, &format!("file-{per_unit}"), per_unit, Some(&native))?;
            }
            let cache = align_driver::CacheContext::at(stage.join("cache"));
            for round in 0..2 {
                let mut sources = SourceMap::new();
                let mut built = align_driver::build_package(&mut sources,
                    stage.join("main.align").to_str().ok_or("entry path")?, &source,
                    &cache, align_driver::UnitReuse::Allowed);
                assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &built.diags));
                assert!(built.units.iter().all(|unit| unit.frontend.as_ref()
                    .is_some_and(|frontend| frontend.hit == (round == 1))));
                let mut mir = Vec::new();
                for index in 0..built.units.len() {
                    mir.push(built.materialize(index).map_err(|error| format!("{error:?}"))?.clone());
                }
                std::fs::write(&input, b"abcdefghij")?;
                std::fs::write(&replacement, b"ZZ")?;
                execute(stage, &mir, &format!("cached-{round}"), false, Some(&native))?;
            }
            Ok(())
        })();
        assert!(result.is_ok(), "{result:?}");
    });
}
