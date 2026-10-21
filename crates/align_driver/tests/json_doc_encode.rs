//! R158: document tokens, bounded output and borrowed-source/owned-output lifecycle.
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

const HELPER: &str = r#"module helper
import core.json
pub Typed { value:i64 }
pub fn encode<T>(document:json.doc, cap:i64, marker:T)->Result<string,Error> = json.encode_bounded(document,cap)
pub fn fields(first:json.doc, second:json.doc)->Result<string,Error> {
  a:=json.encode(first)?
  b:=json.encode_bounded(second,100)?
  if a==b { return Err(Error.Invalid) }
  return Ok(a)
}
pub fn typed(value:i64)->Result<string,Error> {
  record:=Typed {value:value}
  json.encode(record)
}
"#;

const SOURCE: &str = r#"import core.json
import helper
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_requested_live_peak()->i64
fn require(ok:bool)->Result<(),Error> { if !ok { return Err(Error.Invalid) }; Ok(()) }
fn mapped(error:Error)->Error = error
fn same(a:string,b:string)->bool = a==b
fn verify(input:str, expected:str)->Result<(),Error> {
  arena {
    document:=json.doc(input)?
    encoded:=json.encode(document)?
    require(encoded==expected)?
    require(helper.encode(document,expected.len(),true)?==expected)?
    mut evaluated:i64:=0
    require((json.encode_bounded(document,{evaluated=evaluated+1;expected.len()})?).len()==expected.len())?
    require(evaluated==1)?
    match json.encode_bounded(document,expected.len()-1) { Err(Invalid)=>{}, _=>{return Err(Error.Invalid)} }
    match json.encode_bounded(document,0) { Err(Invalid)=>{}, _=>{return Err(Error.Invalid)} }
    match json.encode_bounded(document,-1) { Err(Invalid)=>{}, _=>{return Err(Error.Invalid)} }
    require(json.encode(document)?==expected)?
  }
  Ok(())
}
fn owned(input:str, mode:i64)->Result<string,Error> {
  source:=input.clone()
  arena {
    document:=json.doc(source)?
    if mode==0 { return json.encode(document) }
    chosen:=if mode==1 { json.encode(document)? } else { json.encode_bounded(document,1000)? }
    mut text:=chosen
    text=json.encode(document).map_err(mapped)?
    json.encode(document)?
    fallback:=json.encode_bounded(document,0) else "fallback".clone()
    require(fallback=="fallback")?
    mut i:i64:=0
    loop {
      if i==3 { break }
      text=match json.encode(document) { Ok(value)=>value, Err(error)=>{return Err(error)} }
      i=i+1
    }
    return Ok(text)
  }
}
fn divergent(document:json.doc)->Result<string,Error> = json.encode_bounded(document,{return Ok("early".clone()); 100})
fn exercise()->Result<(),Error> {
  verify(" { } ","{}")?
  verify(" [ ] ","[]")?
  verify(" null ","null")?
  verify("true","true")?
  verify("false","false")?
  verify("-0","-0")?
  verify("1.2300E+009","1.2300E+009")?
  verify("18446744073709551616","18446744073709551616")?
  verify("1e99999","1e99999")?
  verify("\"a\\u0000\\uD83D\\uDE00\\/\\\"\\\\日本語\"","\"a\\u0000\\uD83D\\uDE00\\/\\\"\\\\日本語\"")?
  verify(" { \"x\" : [ null, false, {\"\":-1e-9999} ], \"x\": 2, \"\\u0078\":3 } ","{\"x\":[null,false,{\"\":-1e-9999}],\"x\":2,\"\\u0078\":3}")?
  verify(" { \"type\":\"object\", \"properties\": {\"cmd\": {\"type\":\"string\",\"enum\":[\"a b\", \"\"]}}, \"required\": [\"cmd\"], \"maximum\":999999999999999999999 } ","{\"type\":\"object\",\"properties\":{\"cmd\":{\"type\":\"string\",\"enum\":[\"a b\",\"\"]}},\"required\":[\"cmd\"],\"maximum\":999999999999999999999}")?
  mut i:i64:=0
  loop { if i==4 {break}; require(owned("[ 1, 2 ]",i)?=="[1,2]")?; i=i+1 }
  independent:=arena { d:=json.doc(" [ 3 ] ")?; json.encode(d)? }
  require(independent=="[3]")?
  arena {
    document:=json.doc("{\"schema\":[null,1e999],\"tail\":9}")?
    subtree:=document.get("schema")
    number:=subtree.at(1)
    missing:=document.get("absent")
    require(helper.encode(subtree,100,true)?=="[null,1e999]")?
    require(json.encode(number)?=="1e999")?
    require(divergent(document)?=="early")?
    require(helper.fields(number,subtree)?=="1e999")?
    require(!same(json.encode(number)?,json.encode(subtree)?))?
    require(json.encode(number)?!=json.encode_bounded(subtree,100)?)?
    match json.encode(missing) {Err(Invalid)=>{},_=>{return Err(Error.Invalid)}}
    match json.encode_bounded(missing,100) {Err(Invalid)=>{},_=>{return Err(Error.Invalid)}}
    require(document.len()==2)?
    serialized:=json.encode(document)?
    parsed:=json.doc(serialized)?
    require((parsed.get("tail").as_i64() else 0)==9)?
  }
  require(helper.typed(42)?=="{\"value\":42}")?
  Ok(())
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  match exercise() {Ok(_)=>{}, Err(_)=>{return 1}}
  if unsafe {align_rt_requested_live_peak()}<=0 || unsafe {align_rt_requested_live_bytes()}!=0 {return 2}
  0
}
"#;

#[test]
fn document_encoding_lifecycle_import_and_cache() {
    owned_fixture::run("document_encoding_lifecycle_import_and_cache", |stage| {
        let result = (|| -> TestResult {
            std::fs::write(stage.join("helper.align"), HELPER)?;
            run_both(stage, SOURCE, "document")?;
            let path = stage.join("main.align");
            let entry = path.to_str().ok_or("fixture path")?;
            let cache = align_driver::CacheContext::at(stage.join("cache"));
            for round in 0..3 {
                let source = if round == 2 {
                    SOURCE
                        .replace(" [ 3 ] ", " [ 4 ] ")
                        .replace("\"[3]\"", "\"[4]\"")
                } else {
                    SOURCE.to_owned()
                };
                if round == 2 {
                    assert_ne!(source, SOURCE, "cache mutation must change the input");
                }
                std::fs::write(&path, &source)?;
                let mut sources = SourceMap::new();
                let mut built = align_driver::build_package(
                    &mut sources,
                    entry,
                    &source,
                    &cache,
                    align_driver::UnitReuse::Allowed,
                );
                assert!(
                    !built.diags.has_errors(),
                    "{}",
                    align_driver::format_diagnostics(&sources, &built.diags)
                );
                if round == 1 {
                    assert!(
                        built
                            .units
                            .iter()
                            .all(|unit| unit.frontend.as_ref().unwrap().hit)
                    );
                } else {
                    assert!(built.units.iter().any(|unit| !unit.frontend.as_ref().unwrap().hit));
                }
                let programs = (0..built.units.len())
                    .map(|i| built.materialize(i).unwrap().clone())
                    .collect::<Vec<_>>();
                execute(stage, &programs, &format!("cached-{round}"), false)?;
            }
            Ok(())
        })();
        assert!(result.is_ok(), "{result:?}");
    });
}

#[test]
fn document_encoding_rejects_invalid_sources() {
    owned_fixture::run("document_encoding_rejects_invalid_sources", |stage| {
        for (name, source) in [
            (
                "expired-arena",
                "fn bad()->Result<string,Error> { d:=arena { json.doc(\"{}\")? }; json.encode(d) }",
            ),
            (
                "expired-source",
                "fn consume(value:string) {}\nfn bad()->Result<string,Error> { input:=\"{}\".clone(); arena {d:=json.doc(input)?; consume(input); json.encode(d)} }",
            ),
            (
                "drop-in-limit",
                "fn consume(value:string)->i64 = value.len()\nfn bad()->Result<string,Error> = arena { input:=\"{}\".clone(); d:=json.doc(input)?; json.encode_bounded(d,consume(input)) }",
            ),
            (
                "replace-in-limit",
                "fn bad()->Result<string,Error> = arena { mut input:=\"{}\".clone(); d:=json.doc(input)?; json.encode_bounded(d,{ input=\"[]\".clone(); 10 }) }",
            ),
            (
                "doc-in-record",
                "Holder { value:json.doc }\nfn bad(d:json.doc)->Result<string,Error> { h:=Holder {value:d}; json.encode(h) }",
            ),
            (
                "temporary",
                "fn bad(d:json.doc)->Result<string,Error> = json.encode(d.get(\"x\"))",
            ),
            (
                "wrong-limit",
                "fn bad(d:json.doc,n:i32)->Result<string,Error> = json.encode_bounded(d,n)",
            ),
        ] {
            let source = format!("import core.json\n{source}\nfn main() {{}}\n");
            let entry = stage.join("negative.align");
            std::fs::write(&entry, &source).unwrap();
            for per_unit in [false, true] {
                let mut sources = SourceMap::new();
                let diags = if per_unit {
                    align_driver::check_per_unit(&mut sources, entry.to_str().unwrap(), &source)
                        .diags
                } else {
                    align_driver::check(&mut sources, entry.to_str().unwrap(), &source).diags
                };
                assert!(diags.has_errors(), "accepted {name}, per_unit={per_unit}");
                let text = align_driver::format_diagnostics(&sources, &diags);
                assert!(
                    !text.contains("cannot certify") && !text.contains("unknown module"),
                    "{name}: {text}"
                );
            }
        }
    });
}
