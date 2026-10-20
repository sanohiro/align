//! Plan236: explicit fallible allocation and coherent non-owning pointer transport.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
pub Holder { pointer:raw, optional:Option<raw>, result:Result<raw,Error>, pair:(raw,i64) }
pub Choice { Pointer(raw), Empty }
pub Generic<T> { Item(T), Empty }
pub fn identity<T>(value:T)->Result<T,Error> = Ok(value)
pub fn carry(value:raw)->raw = value
pub fn pointer(value:raw)->Choice = Choice.Pointer(value)
pub fn generic(value:raw)->Generic<raw> = Generic.Item(value)
pub fn maybe(value:raw, yes:bool)->Option<raw> = if yes {Some(value)} else {None}
pub fn outcome(value:raw, ok:bool)->Result<raw,raw> = if ok {Ok(value)} else {Err(value)}
pub fn fail(value:raw)->Result<raw,raw> = Err(value)
pub fn allocate(size:i64)->Result<raw,Error> = unsafe {raw.try_alloc(size)}
pub fn holder(value:raw)->Holder = Holder {pointer:value,optional:Some(value),result:Ok(value),pair:(value,37)}
"#;
const MAIN: &str = r#"module main
import model
fn read(pointer:raw)->i64 = unsafe {raw.load(pointer,0)}
fn size(pointer:raw)->i64 { unsafe {raw.store(pointer,0,read(pointer)+1)};return 8 }
fn diverge()->Result<raw,Error> = unsafe {raw.try_alloc({return Err(Error.Invalid)})}
fn main()->Result<(),Error> {
  unsafe {
    zero:=model.allocate(0)?
    if !zero.is_null() {return Err(Error.Code(1))}
    match model.allocate(-1) {Ok(_)=>{return Err(Error.Code(2))},Err(error)=>{
      match error {Invalid=>{},_=>{return Err(Error.Code(3))}}
    }}
    pointer:=model.allocate(16)?
    if pointer.is_null() {return Err(Error.Code(4))}
    raw.store(pointer,0,91 as i64)
    mut record:=model.holder(pointer)
    copied:=record
    record.pointer=zero
    from_result:=copied.result?
    from_option:=copied.optional else zero
    absent:=model.maybe(pointer,false) else zero
    if !absent.is_null() {return Err(Error.Code(8))}
    from_raw_ok:=match model.outcome(pointer,true) {Ok(value)=>value,Err(_)=>zero}
    pair:=copied.pair
    callback:=model.carry
    from_callback:=callback(pointer)
    from_generic:=model.identity(pointer)?
    from_sum:=match model.pointer(pointer) {Pointer(value)=>value,Empty=>zero}
    from_generic_sum:=match model.generic(pointer) {Item(value)=>value,Empty=>zero}
    from_error:=match model.fail(pointer).map_err(model.carry) {Ok(value)=>value,Err(value)=>value}
    from_else:=model.fail(pointer) else pointer
    from_loop:=loop {break pointer}
    if read(from_result)!=91 || read(from_option)!=91 || read(from_raw_ok)!=91
       || read(pair.0)!=91 || pair.1!=37
       || read(from_callback)!=91 || read(from_generic)!=91
       || read(from_sum)!=91 || read(from_generic_sum)!=91
       || read(from_error)!=91 || read(from_else)!=91
       || read(from_loop)!=91 {return Err(Error.Code(5))}
    raw.store(pointer,0,0 as i64)
    extra:=raw.try_alloc(size(pointer))?
    if read(pointer)!=1 {return Err(Error.Code(6))}
    raw.free(extra)
    match diverge() {Ok(_)=>{return Err(Error.Code(7))},Err(_)=>{}}
    raw.free(pointer)
    raw.free(zero)
  }
  return Ok(())
}
"#;

fn programs(stage: &Path, source: &str, per_unit: bool) -> Vec<align_mir::Program> {
    let entry = stage.join("main.align");
    std::fs::write(&entry, source).unwrap();
    let mut sources = SourceMap::new();
    if per_unit {
        let built = align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), source);
        assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &built.diags));
        built.units.into_iter().map(|unit| unit.mir).collect()
    } else {
        let checked = align_driver::check(&mut sources, entry.to_str().unwrap(), source);
        assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &checked.diags));
        vec![align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap()]
    }
}

fn execute(stage: &Path, programs: &[align_mir::Program], label: &str) {
    let mut objects = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{index}.o"));
        align_driver::emit_object_file(program, &object, BuildTarget::Baseline,
            Profile::Release, &[], false).unwrap();
        objects.push(object);
    }
    let binary = stage.join(label);
    align_driver::link_objects(&align_driver::CDriver::default(),
        &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(), &binary, &[], Profile::Release).unwrap();
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(), "{label}: {output:?}");
}

#[test]
fn fallible_raw_values_and_transport() {
    owned_fixture::run("fallible_raw_values_and_transport", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        for per_unit in [false, true] {
            let programs = programs(stage, MAIN, per_unit);
            execute(stage, &programs, if per_unit {"units"} else {"whole"});
        }
    });
}

#[test]
fn fallible_raw_rejects_invalid_domains() {
    owned_fixture::run("fallible_raw_rejects_invalid_domains", |stage| {
        let sources = [
            "fn bad() { raw.try_alloc(8) }",
            "fn bad() { unsafe { raw.try_alloc() } }",
            "fn bad() { unsafe { raw.try_alloc(1,2) } }",
            "fn bad(value:i32) { unsafe { raw.try_alloc(value) } }",
            "fn bad(value:u64) { unsafe { raw.try_alloc(value) } }",
            "fn bad() { unsafe { raw.try_alloc(true) } }",
            "fn bad() { unsafe { raw.try_alloc(missing) } }",
            "fn bad()->(raw,str) {text:=\"gone\".clone();return unsafe {(raw.null(),text[0..text.len()])}}",
            "fn bad(value:raw) {task_group {t:=spawn(fn {value});wait()}}",
            "fn bad(value:array<raw>) {}",
            "fn bad(value:slice<raw>) {}",
            "fn bad(value:[raw;2]) {}",
            "fn bad(value:box<raw>) {}",
            "fn bad(value:vec2<raw>) {}",
            "fn bad(value:mask2<raw>) {}",
            "extern \"C\" fn bad(value:Option<raw>)",
            "extern \"C\" fn bad()->Result<raw,Error>",
            "layout(C) Bad { value:Option<raw> }",
            "Record { value:raw }\nfn bad(pointer:raw)->Record = unsafe { raw.load(pointer,0) }",
            "import core.json\nRecord { value:raw }\nfn bad(value:Record) {json.encode(value)}",
        ];
        let entry = stage.join("bad.align");
        for source in sources {
            let source = format!("module main\n{source}\nfn main() {{}}\n");
            std::fs::write(&entry, &source).unwrap();
            for per_unit in [false, true] {
                let mut map = SourceMap::new();
                let diags = if per_unit {
                    align_driver::build_per_unit(&mut map, entry.to_str().unwrap(), &source).diags
                } else {
                    align_driver::check(&mut map, entry.to_str().unwrap(), &source).diags
                };
                assert!(diags.has_errors(), "accepted invalid raw domain ({per_unit}): {source}");
            }
        }
    });
}

#[test]
fn fallible_raw_collection_producers_reject() {
    owned_fixture::run("fallible_raw_collection_producers_reject", |stage| {
        let cases = [
            ("fn bad(pointer:raw) {values:=[1,2].scan(pointer,fn prior:raw,x:i64 {prior})}", "'scan' accumulator must be a materializable"),
            ("fn bad(pointer:raw) {values:=[pointer]}", "raw"),
            ("fn bad(pointer:raw) {values:=[1,2].map(fn x:i64 {pointer}).to_array()}", "'to_array' needs a scalar"),
            ("fn bad(pointer:raw,out destination:slice<i64>) {[1,2].map(fn x:i64 {pointer}).map_into(destination)}", "'map_into' needs a scalar"),
            ("fn bad(pointer:raw) {value:=heap.new(pointer)}", "raw"),
            ("fn bad() {mut builder:array_builder<raw>:=array_builder()}", "array_builder<T> element"),
            ("fn bad(pointer:raw) {value:=[1,2].map(fn x:i64 {pointer}).any(fn p:raw {p.is_null()})}", "'any' needs a scalar"),
            ("fn bad(pointer:raw) {value:=[1,2].map(fn x:i64 {pointer}).all(fn p:raw {p.is_null()})}", "'all' needs a scalar"),
        ];
        let entry = stage.join("bad.align");
        for (source, expected) in cases {
            let source = format!("module main\n{source}\nfn main() {{}}\n");
            std::fs::write(&entry, &source).unwrap();
            for per_unit in [false, true] {
                let mut map = SourceMap::new();
                let diags = if per_unit {
                    align_driver::build_per_unit(&mut map, entry.to_str().unwrap(), &source).diags
                } else {
                    align_driver::check(&mut map, entry.to_str().unwrap(), &source).diags
                };
                let rendered = align_driver::format_diagnostics(&map, &diags);
                assert!(diags.has_errors() && rendered.contains(expected),
                    "raw producer ({per_unit}): {source}\n{rendered}");
            }
        }
        // A scalar reduction transports an existing pointer without collecting it.
        let source = "module main\nfn keep(pointer:raw)->raw = [1,2].reduce(pointer,fn prior:raw,x:i64 {prior})\nfn main() {}\n";
        for per_unit in [false, true] { let _ = programs(stage, source, per_unit); }
    });
}

#[test]
fn fallible_raw_generic_cache() {
    owned_fixture::run("fallible_raw_generic_cache", |stage| {
        let cache = align_driver::CacheContext::at(stage.join("cache"));
        let entry = stage.join("main.align");
        for (round, marker) in [37, 37, 38, 37].into_iter().enumerate() {
            std::fs::write(stage.join("model.align"), MODEL.replace("pair:(value,37)", &format!("pair:(value,{marker})"))).unwrap();
            let source = MAIN.replace("pair.1!=37", &format!("pair.1!={marker}"));
            std::fs::write(&entry, &source).unwrap();
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(&mut sources, entry.to_str().unwrap(), &source,
                &cache, align_driver::UnitReuse::Allowed);
            assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &built.diags));
            for unit in &built.units {
                assert_eq!(unit.frontend.as_ref().unwrap().hit, round == 1 || round == 3);
                let bytes = align_interface::serialize(&unit.summary);
                assert_eq!(bytes, align_interface::serialize(&align_interface::deserialize(&bytes).unwrap()));
            }
            let programs = (0..built.units.len()).map(|i| built.materialize(i).unwrap().clone()).collect::<Vec<_>>();
            execute(stage, &programs, &format!("cache-{round}"));
        }
    });
}

const OBSERVER: &str = r#"
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
extern int32_t align_rt_raw_try_alloc(int64_t, void **);
extern void align_rt_free(void *);
static void *live[32];
static int acquired, freed, refused;
static void finish(void) {
    if (acquired != 8 || freed != 8 || refused != 2) {
        fprintf(stderr,"raw ownership acquired=%d freed=%d refused=%d\n",acquired,freed,refused);
        abort();
    }
    for (int i=0;i<32;i++) if (live[i]) abort();
}
int32_t probe_try_alloc(int64_t size, void **out) {
    if (!out || *out) abort();
    /* ABI refusal owns source rollback; the runtime unit owner injects real allocator refusal. */
    if (size==12345) { refused++; return 17; }
    int32_t status=align_rt_raw_try_alloc(size,out);
    if (status || size==0) { if (*out) abort(); return status; }
    if (!*out || (uintptr_t)*out%8) abort();
    if (!acquired && atexit(finish)) abort();
    acquired++;
    for (int i=0;i<32;i++) if (!live[i]) {live[i]=*out;return 0;}
    abort();
}
void probe_free(void *pointer) {
    if (!pointer) return;
    for (int i=0;i<32;i++) if (live[i]==pointer) {
        live[i]=0;freed++;align_rt_free(pointer);return;
    }
    abort();
}
"#;

#[test]
fn fallible_raw_partial_cleanup_and_retry() {
    owned_fixture::run("fallible_raw_partial_cleanup_and_retry", |stage| {
        std::fs::create_dir_all(stage.join("pkg/raw/internal")).unwrap();
        std::fs::write(stage.join("pkg/raw/internal.align"), "module pkg.raw.internal\npub fn close(pointer:raw) {unsafe {raw.free(pointer)}}\n").unwrap();
        std::fs::write(stage.join("pkg/raw.align"), r#"module pkg.raw
import pkg.raw.internal
pub resource Handle = pkg.raw.internal.close
pub Mixed { pointer:raw, owner:Handle }
pub fn open(size:i64)->Result<Handle,Error> {unsafe {
  pointer:=raw.try_alloc(size)?
  return Ok(resource.from_raw(pointer))
}}
pub fn pair(fail:bool)->Result<(Handle,Handle),Error> {
  first:=open(104)?
  second:=open(if fail {12345} else {152})?
  return Ok((first,second))
}
pub fn eager()->Result<(Handle,Handle),Error> = Ok((open(104)?,open(12345)?))
"#).unwrap();
        let source = r#"module main
import pkg.raw
fn main()->Result<(),Error> {
  match pkg.raw.pair(true) {Ok(_)=>{return Err(Error.Code(1))},Err(error)=>{
    match error {Code(code)=>{if code!=12 {return Err(Error.Code(2))}},_=>{return Err(Error.Code(3))}}
  }}
  match pkg.raw.eager() {Ok(_)=>{return Err(Error.Code(4))},Err(_)=>{}}
  pair:=pkg.raw.pair(false)?
  unsafe {
    pointer:=raw.try_alloc(32)?
    {
      mixed:=pkg.raw.Mixed {pointer:pointer,owner:pkg.raw.open(24)?}
      raw.store(mixed.pointer,0,77 as i64)
      carried:Result<(raw,pkg.raw.Handle),Error>:=Ok((pointer,pkg.raw.open(24)?))
      extracted:=carried?
      raw.store(extracted.0,8,55 as i64)
    }
    value:i64:=raw.load(pointer,0)
    if value!=77 {return Err(Error.Code(5))}
    raw.free(pointer)
    retried:=raw.try_alloc(8)?
    raw.free(retried)
  }
  return Ok(())
}
"#;
        std::fs::write(stage.join("probe.c"), OBSERVER).unwrap();
        assert!(std::process::Command::new("cc").arg("-c").arg(stage.join("probe.c"))
            .arg("-o").arg(stage.join("probe.o")).status().unwrap().success());
        let llc = align_driver::llvm_tool("llc").unwrap();
        for per_unit in [false, true] {
            let programs = programs(stage, source, per_unit);
            let mut objects = vec![stage.join("probe.o")];
            for (index, program) in programs.iter().enumerate() {
                let ir = align_driver::emit_llvm_ir(program, BuildTarget::Baseline, Profile::Release,
                    false, &[], false).unwrap()
                    .replace("@align_rt_raw_try_alloc(", "@probe_try_alloc(")
                    .replace("@align_rt_free(", "@probe_free(");
                let input = stage.join(format!("cleanup-{index}.ll"));
                let object = stage.join(format!("cleanup-{index}.o"));
                std::fs::write(&input, ir).unwrap();
                assert!(std::process::Command::new(&llc).args(["-filetype=obj","-relocation-model=pic"])
                    .arg(input).arg("-o").arg(&object).status().unwrap().success());
                objects.push(object);
            }
            let binary = stage.join("cleanup");
            align_driver::link_objects(&align_driver::CDriver::default(),
                &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(), &binary, &[], Profile::Release).unwrap();
            let output = std::process::Command::new(binary).output().unwrap();
            assert!(output.status.success() && output.stderr.is_empty(), "cleanup ({per_unit}): {output:?}");
        }
    });
}
