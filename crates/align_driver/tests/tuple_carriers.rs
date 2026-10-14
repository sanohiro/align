//! Plan190: existing anonymous products compose with tagged carriers and records.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
pub Container<T> { value:T }
pub Record { pair:(i64,i64), wrapped:Result<(i64,i64),Error> }
pub Choice { Pair((i64,i64)), Empty }
pub fn choice()->Choice = Choice.Pair((9,6))
pub fn inspect(borrow value:Option<(i64,i64)>)->i64 = match value {Some(pair)=>pair.0+pair.1,None=>0}
pub fn maybe(value:bool)->Option<(i64,i64)> = if value {Some((8,2))} else {None}
pub fn nested()->Result<Option<(i64,i64)>,Error> = Ok(Some((6,2)))
pub fn borrowed(borrow value:string)->Result<(str,i64),Error> = Ok((value[0..value.len()],value.len()))
pub fn pair()->Result<(i64,i64),Error> = Ok((17,5))
pub fn owned()->Result<(string,array<i64>),Error> = Ok(("live".clone(),[10,20].to_array()))
pub fn identity<T>(value:T)->Result<T,Error> = Ok(value)
pub fn swapped(value:(i64,i64))->(i64,i64) = (value.1,value.0)
pub fn failure()->Result<i64,(i64,i64)> = Err((3,4))
pub fn contained()->Container<(i64,i64)> = Container {value:(11,12)}
pub fn view_array(value:str)->Result<(array<str>,i64),Error> = Ok(([value].to_array(),1))
pub fn borrowed_tuple(borrow value:(string,i64))->str = value.0[0..value.0.len()]
pub fn record()->Record = Record {pair:(1,2),wrapped:Ok((3,4))}
"#;
const MAIN: &str = r#"module main
import model
fn basic()->Result<(),Error> {
  (a,b):=model.pair()?
  if a!=17 || b!=5 {return Err(Error.Code(11))}
  (x,y):=model.identity((7,8))?
  if x!=7 || y!=8 {return Err(Error.Code(12))}
  r:=model.record()
  if r.pair.0!=1 || r.pair.1!=2 {return Err(Error.Code(13))}
  (c,d):=r.wrapped?
  if c!=3 || d!=4 {return Err(Error.Code(14))}
  match model.failure().map_err(model.swapped) {
    Ok(_)=>{return Err(Error.Code(15))},
    Err(pair)=>{if pair.0!=4 || pair.1!=3 {return Err(Error.Code(16))}},
  }
  match model.choice() {Pair(pair)=>{if pair.0!=9 || pair.1!=6 {return Err(Error.Code(18))}},Empty=>{return Err(Error.Code(19))}}
  copied:=model.maybe(true)
  if model.inspect(copied)!=10 {return Err(Error.Code(28))}
  (m,n):=model.maybe(false) else (3,5)
  (o,p):=model.maybe(true) else (0,0)
  if m+n!=8 || o+p!=10 {return Err(Error.Code(20))}
  (q,rn):=(model.nested()?) else (0,0)
  if q+rn!=8 {return Err(Error.Code(21))}
  function:=model.swapped
  (ix,iy):=function((9,10))
  if ix!=10 || iy!=9 {return Err(Error.Code(22))}
  match model.failure().map_err(fn v:(i64,i64) {(v.1,v.0)}) {Ok(_)=>{return Err(Error.Code(23))},Err(v)=>{if v.0!=4 {return Err(Error.Code(24))}}}
  source:="borrowed".clone()
  (view,length):=model.borrowed(source)?
  if view!="borrowed" || length!=8 {return Err(Error.Code(25))}
  generic_record:=model.contained()
  if generic_record.value.0+generic_record.value.1!=23 {return Err(Error.Code(26))}
  tuple_owner:=("callback".clone(),8)
  borrowed_callback:=model.borrowed_tuple
  borrowed_view:=borrowed_callback(tuple_owner)
  if borrowed_view!="callback" {return Err(Error.Code(27))}
  (views,view_count):=model.view_array(source)?
  if views[0]!="borrowed" || view_count!=1 {return Err(Error.Code(29))}
  (text,values):=model.owned()?
  if text!="live" || values.sum()!=30 {return Err(Error.Code(17))}
  return Ok(())
}
fn main()->Result<(),Error> = basic()
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
fn tuple_carrier_values_and_transport() {
    owned_fixture::run("tuple_carrier_values_and_transport", |stage| {
        let (model, main) = (MODEL, MAIN);
        std::fs::write(stage.join("model.align"), model).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, &main).unwrap();
        let entry = entry.to_str().unwrap();
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, entry, &main);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[program], "whole");
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry, &main);
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
                &main,
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

fn lifecycle_source() -> String {
    let mut source = String::from(
        r#"module main
import pkg.probe
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
Direct { value:(pkg.probe.handle,pkg.probe.handle) }
Wrapped { value:Option<(pkg.probe.handle,pkg.probe.handle)> }
Choice { Pair((pkg.probe.handle,pkg.probe.handle)), Empty }
fn pair()->(pkg.probe.handle,pkg.probe.handle) = (pkg.probe.open(),pkg.probe.open())
fn produced()->Result<(pkg.probe.handle,pkg.probe.handle),i64> = Ok(pair())
fn failing()->Result<i64,(pkg.probe.handle,pkg.probe.handle)> = Err(pair())
fn consume(value:(pkg.probe.handle,pkg.probe.handle))->i64 = 7
fn map_failure(value:(pkg.probe.handle,pkg.probe.handle))->(pkg.probe.handle,pkg.probe.handle) = value
fn guard(stop:bool)->Result<i64,i64> = if stop {Err(4)} else {Ok(2)}
fn partial(stop:bool)->Result<i64,i64> {value:Option<(pkg.probe.handle,i64)>:=Some((pkg.probe.open(),guard(stop)?));return Ok(0)}
fn optional(stop:bool)->Option<(pkg.probe.handle,pkg.probe.handle)> {value:Option<(pkg.probe.handle,pkg.probe.handle)>:=if stop {None} else {Some(pair())};(a,b):=value else {return None};return Some((a,b))}
fn to_pair(error:i64)->(pkg.probe.handle,pkg.probe.handle) = pair()
fn partial_return()->i64 {value:Option<(pkg.probe.handle,i64)>:=Some((pkg.probe.open(),{return 3}));return 9}
fn propagate()->Result<i64,(pkg.probe.handle,pkg.probe.handle)> {value:=failing()?;return Ok(value)}
fn tuple_return()->Result<(pkg.probe.handle,pkg.probe.handle),i64> {return produced()}
"#,
    );
    let cases = [
        ("unused", "value:=Some(pair());return 0"),
        (
            "wildcard",
            "match produced() {Ok(_)=>{},Err(_)=>{return 1}};return 0",
        ),
        ("destructure", "(a,b):=produced() else {return 2};return 0"),
        (
            "indexed",
            "value:=produced() else {return 3};a:=value.0;b:=value.1;return 0",
        ),
        ("result_error", "value:=failing();return 0"),
        (
            "else_discards",
            "value:=failing() else 8;if value!=8 {return 4};return 0",
        ),
        (
            "map_error",
            "value:=failing().map_err(map_failure);return 0",
        ),
        (
            "map_ok",
            "value:Result<i64,(pkg.probe.handle,pkg.probe.handle)>:=Ok(3);mapped:=value.map_err(map_failure);unused:=pair();return 0",
        ),
        (
            "indirect",
            "callback:=consume;if callback(pair())!=7 {return 5};return 0",
        ),
        (
            "lambda",
            "callback:=fn value:(pkg.probe.handle,pkg.probe.handle) {consume(value)};if callback(pair())!=7 {return 6};return 0",
        ),
        (
            "nested",
            "value:Result<Option<(pkg.probe.handle,pkg.probe.handle)>,i64>:=Ok(Some(pair()));return 0",
        ),
        ("direct_field", "value:=Direct {value:pair()};return 0"),
        (
            "wrapped_field",
            "value:=Wrapped {value:Some(pair())};return 0",
        ),
        (
            "sum",
            "value:=Choice.Pair(pair());match value {Pair(payload)=>{if consume(payload)!=7 {return 7}},Empty=>{return 8}};return 0",
        ),
        (
            "replace",
            "mut value:Option<(pkg.probe.handle,pkg.probe.handle)>:=Some(pair());value=Some(pair());value=None;return 0",
        ),
        (
            "replace_field",
            "mut value:=Direct {value:pair()};value.value=pair();return 0",
        ),
        (
            "replace_wrapped_field",
            "mut value:=Wrapped {value:Some(pair())};value.value=Some(pair());value.value=None;return 0",
        ),
        (
            "joins",
            "mut iteration:=0;loop {if iteration==4 {break};value:Option<(pkg.probe.handle,pkg.probe.handle)>:=if iteration%2==0 {Some(pair())} else {None};other:Result<(pkg.probe.handle,pkg.probe.handle),i64>:=loop {break Ok(pair())};iteration=iteration+1};return 0",
        ),
        (
            "partial",
            "match partial(true) {Ok(_)=>{return 9},Err(_)=>{}};match partial(false) {Ok(_)=>{},Err(_)=>{return 10}};if partial_return()!=3 {return 11};return 0",
        ),
        (
            "propagate",
            "value:=propagate();other:=tuple_return();return 0",
        ),
        ("early", "value:=Some(pair());return 0"),
        (
            "resource_ref",
            "owner:=pkg.probe.open();(reference,n):=pkg.probe.view(owner) else {return 12};if n!=1 || !pkg.probe.present(reference) {return 13};return 0",
        ),
        (
            "optional",
            "none:=optional(true);some:=optional(false);return 0",
        ),
        (
            "map_to_pair",
            "value:Result<i64,i64>:=Err(3);mapped:=value.map_err(to_pair);return 0",
        ),
        (
            "generic",
            "value:=pkg.probe.identity(Some(pair()));return 0",
        ),
    ];
    for (name, body) in cases {
        source.push_str(&format!("fn case_{name}()->i32 {{{body}}}\n"));
    }
    source.push_str("fn main()->i32 {unsafe {align_rt_requested_live_reset()};\n");
    for (index, (name, _)) in cases.iter().enumerate() {
        let allocations = match *name {
            "replace" | "replace_field" | "replace_wrapped_field" | "propagate" => 4,
            "joins" => 12,
            "partial" => 3,
            "resource_ref" => 1,
            _ => 2,
        };
        source.push_str(&format!("{{before:=unsafe {{align_rt_alloc_count()}};before_free:=unsafe {{align_rt_free_count()}};status:=case_{name}();if status!=0 {{return status}};allocated:=unsafe {{align_rt_alloc_count()}}-before;freed:=unsafe {{align_rt_free_count()}}-before_free;if allocated!={allocations} || allocated!=freed || unsafe {{align_rt_requested_live_bytes()}}!=0 {{return {}}}}}\n", 40+index));
    }
    source.push_str("return 0}\n");
    source
}

fn write_probe_package(stage: &Path) {
    std::fs::create_dir_all(stage.join("pkg/probe/internal")).unwrap();
    std::fs::write(stage.join("pkg/probe/internal/resource.align"), "module pkg.probe.internal.resource\npub fn close(handle:raw) {unsafe {raw.free(handle)}}\n").unwrap();
    std::fs::write(stage.join("pkg/probe.align"), "module pkg.probe\nimport pkg.probe.internal.resource\npub resource handle = pkg.probe.internal.resource.close\npub fn view(borrow owner:handle)->Result<(resource_ref<handle>,i64),i64> = Ok((resource.borrow(owner),1))\npub fn present(reference:resource_ref<handle>)->bool {unsafe {return !resource.raw(reference).is_null()}}\npub fn identity<T>(value:T)->T = value\npub fn open()->handle {unsafe {return resource.from_raw(raw.alloc(8))}}\n").unwrap();
}

#[test]
fn tuple_carrier_lifecycle() {
    owned_fixture::run("tuple_carrier_lifecycle", |stage| {
        write_probe_package(stage);
        let source = lifecycle_source();
        let entry = stage.join("main.align");
        std::fs::write(&entry, &source).unwrap();
        let entry = entry.to_str().unwrap();
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, entry, &source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[program], "lifecycle-whole");
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry, &source);
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
            "lifecycle-units",
        );
    });
}

#[test]
fn tuple_carrier_rejects_invalid_authority() {
    owned_fixture::run("tuple_carrier_rejects_invalid_authority", |stage| {
        write_probe_package(stage);
        let cases = [
            (
                "use_after_move",
                "fn main() {value:=(\"owned\".clone(),1);carrier:=Some(value);print(value.0)}",
                "moved",
            ),
            (
                "owner_replacement",
                "fn main() {mut owner:=\"owned\".clone();view:str:=owner;carrier:=Some((view,1));owner=\"new\".clone();(text,n):=carrier else (\"\",0);print(text)}",
                "borrow",
            ),
            (
                "arena_escape",
                "fn bad(x:str)->Result<(str,i64),i64> {arena {text:=template \"{x}y\";return Ok((text,1))}}\nfn main() {}",
                "arena",
            ),
            (
                "local_escape",
                "fn bad()->Option<(str,i64)> {owner:=\"owned\".clone();view:str:=owner;return Some((view,1))}\nfn main() {}",
                "local",
            ),
            (
                "borrowed_move_payload",
                "fn inspect(borrow value:Option<(string,i64)>)->i64 = match value {Some(pair)=>pair.0.len(),None=>0}\nfn main() {}",
                "borrow",
            ),
            (
                "nested_owned_field",
                "Record {pair:(string,i64)}\nOuter {inner:Record}\nfn main() {value:=Outer {inner:Record {pair:(\"owned\".clone(),1)}};other:=value.inner.pair}",
                "owned",
            ),
            (
                "nested_element",
                "fn main() {value:=Some(((1,2),3))}",
                "tuple elements",
            ),
            (
                "record_element",
                "Record {n:i64}\nfn main() {value:=Some((Record {n:1},2))}",
                "tuple elements",
            ),
            (
                "array_literal",
                "fn main() {values:=[(1,2),(3,4)]}",
                "tuple",
            ),
            (
                "array_annotation",
                "fn use(values:array<(i64,i64)>) {}\nfn main() {}",
                "tuple",
            ),
            (
                "array_builder",
                "fn main() {builder:=array_builder<(i64,i64)>.new()}",
                "array_builder",
            ),
            (
                "pipeline_array",
                "fn main() {values:=[1,2].map(fn n:i64 {(n,n)}).to_array()}",
                "to_array",
            ),
            (
                "box_annotation",
                "fn use(value:box<(i64,i64)>) {}\nfn main() {}",
                "tuple",
            ),
            (
                "box_inference",
                "fn main() {arena {value:=heap.new((1,2))}}",
                "tuple",
            ),
            (
                "tuple_by_value_escape",
                "fn bad(value:(string,i64))->str = value.0[0..value.0.len()]\nfn main() {}",
                "local storage",
            ),
            (
                "tuple_direct_call_escape",
                "fn read(borrow value:(string,i64))->str = value.0[0..value.0.len()]\nfn bad()->str {value:=(\"owned\".clone(),1);return read(value)}\nfn main() {}",
                "local storage",
            ),
            (
                "tuple_indirect_call_escape",
                "fn read(borrow value:(string,i64))->str = value.0[0..value.0.len()]\nfn bad()->str {value:=(\"owned\".clone(),1);callback:=read;return callback(value)}\nfn main() {}",
                "local storage",
            ),
            (
                "generic_array",
                "fn collect<T>(value:T) {values:=[value]}\nfn main() {collect((1,2))}",
                "tuple",
            ),
            (
                "generic_box",
                "fn boxed<T>(value:T) {arena {box_value:=heap.new(value)}}\nfn main() {boxed((1,2))}",
                "tuple",
            ),
            (
                "resource_ref_escape",
                "import pkg.probe\nfn bad()->Result<(resource_ref<pkg.probe.handle>,i64),i64> {owner:=pkg.probe.open();return pkg.probe.view(owner)}\nfn main() {}",
                "local storage",
            ),
            (
                "array_view_escape",
                "fn bad()->Result<(array<str>,i64),i64> {owner:=\"owned\".clone();view:str:=owner;return Ok(([view].to_array(),1))}\nfn main() {}",
                "local storage",
            ),
            (
                "global",
                "VALUE := (1,2)\nfn main() {}",
                "constant initializer",
            ),
            (
                "foreign",
                "extern \"C\" fn foreign(value:(i64,i64))\nfn main() {}",
                "extern",
            ),
            (
                "branch_type",
                "fn bad(flag:bool)->Option<(i64,i64)> = if flag {Some((1,2))} else {Some((\"a\",2))}\nfn main() {}",
                "type mismatch",
            ),
        ];
        for (name, body, expected) in cases {
            let source = format!("module main\n{body}\n");
            let entry = stage.join(format!("{name}.align"));
            std::fs::write(&entry, &source).unwrap();
            let entry = entry.to_str().unwrap();
            for per_unit in [false, true] {
                let mut sources = SourceMap::new();
                let diagnostics = if per_unit {
                    align_driver::build_per_unit(&mut sources, entry, &source).diags
                } else {
                    align_driver::check(&mut sources, entry, &source).diags
                };
                let rendered = align_driver::format_diagnostics(&sources, &diagnostics);
                assert!(
                    diagnostics.has_errors() && rendered.contains(expected),
                    "{name} per_unit={per_unit}: {rendered}"
                );
                assert!(
                    !rendered.contains("expected expression")
                        && !rendered.contains("expected `fn`"),
                    "fixture syntax: {rendered}"
                );
            }
        }
    });
}
