//! Plan193: one mixed JSON graph preserves borrowed content and independent owners.
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
        Ok(vec![align_mir::lower_program_checked(
            &checked.hir,
            false,
            Some(&sources),
        )
        .map_err(|error| format!("{error:?}"))?])
    }
}

// Unchanged media AM-R02 witness, copied into the provider owner.
const MEDIA_PROBE: &str = r#"module main
import core.json
ResultRecord { label: str, owned_label: string, values: array<i64> }
pub fn main() -> Result<(), Error> {
  mut values: array_builder<i64> := array_builder()
  values.push(1)
  result := ResultRecord { label: "image", owned_label: "model".clone(), values: values.build() }
  encoded := json.encode(result)?
  return Ok(())
}
"#;

const VALUES: &str = r#"
import core.json
Mixed {label:str,owned:string,maybe:Option<str>,tags:array<str>}
fn encode_after_scope()->Result<string,Error> {
  arena {
    source:="{\"label\":\"a\\nb\",\"owned\":\"o\\u0000w\",\"maybe\":\"\\u304c\",\"tags\":[\"x\\ty\",\"\"]}".clone()
    row:Mixed:=json.decode(source)?
    if row.label!="a\nb" || row.owned!="o\0w" || (row.maybe else "")!="が" || row.tags[0]!="x\ty" {return Err(Error.Invalid)}
    full:=json.encode(row)?
    limited:=json.encode_bounded(row,full.len())?
    if full!=limited || row.label!="a\nb" {return Err(Error.Invalid)}
    return Ok(full)
  }
}
fn exercise()->Result<(),Error> {
  source:="{\"label\":\"image\",\"owned\":\"model\",\"maybe\":\"yes\",\"tags\":[\"a\",\"b\"]}".clone()
  row:Mixed:=json.decode(source)?
  if row.label!="image" || row.owned!="model" || row.tags[1]!="b" {return Err(Error.Invalid)}
  full:=json.encode(row)?
  if full!=source {return Err(Error.Invalid)}
  limited:=json.encode_bounded(row,full.len())?
  if full!=limited {return Err(Error.Invalid)}
  rows:array<Mixed>:=json.decode("[{\"label\":\"first\",\"owned\":\"owner\",\"tags\":[]},{\"label\":\"second\",\"owned\":\"two\",\"maybe\":null,\"tags\":[\"tag\"]}]")?
  text:=json.encode(rows)?
  if text!="[{\"label\":\"first\",\"owned\":\"owner\",\"tags\":[]},{\"label\":\"second\",\"owned\":\"two\",\"tags\":[\"tag\"]}]" {return Err(Error.Invalid)}
  escaped:=encode_after_scope()?
  if escaped!="{\"label\":\"a\\nb\",\"owned\":\"o\\u0000w\",\"maybe\":\"が\",\"tags\":[\"x\\ty\",\"\"]}" {return Err(Error.Invalid)}
  return Ok(())
}
fn main()->i32 = match exercise() {Ok(_)=>0,Err(_)=>1}
"#;
#[test]
fn mixed_json_values_and_output_independence() {
    owned_fixture::run("mixed_json_values_and_output_independence", |stage| {
        let result = (|| -> TestResult {
            for per_unit in [false, true] {
                let built = programs(stage, VALUES, per_unit)?;
                execute(
                    stage,
                    &built,
                    if per_unit { "units" } else { "whole" },
                    false,
                )?;
                let media = programs(stage, MEDIA_PROBE, per_unit)?;
                execute(
                    stage,
                    &media,
                    if per_unit {
                        "media-units"
                    } else {
                        "media-whole"
                    },
                    false,
                )?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}

// Check only: these programs deliberately try to retain released input/arena storage.
#[test]
fn mixed_json_retains_input_and_arena_in_every_leaf_position() {
    let mut failures = Vec::new();
    for (name, definitions, ty) in [
        ("direct", "M {owned:string,text:str}", "M"),
        ("optional", "M {owned:string,text:Option<str>}", "M"),
        ("array", "M {owned:string,text:array<str>}", "M"),
        (
            "optional-array",
            "M {owned:string,text:Option<array<str>>}",
            "M",
        ),
        ("nested", "Leaf {text:str} M {owned:string,leaf:Leaf}", "M"),
        (
            "nested-optional",
            "Leaf {text:str} M {owned:string,leaf:Option<Leaf>}",
            "M",
        ),
        (
            "nested-array",
            "Leaf {text:str} M {owned:string,leaves:array<Leaf>}",
            "M",
        ),
        ("array-root", "M {owned:string,text:str}", "array<M>"),
    ] {
        for (scope, body) in [
            (
                "input-return",
                format!(
                    "fn bad()->Result<{ty},Error> {{source:=\"{{}}\".clone();value:{ty}:=json.decode(source)?;return Ok(value)}}"
                ),
            ),
            (
                "input-replace",
                format!(
                    "fn bad()->Result<string,Error> {{mut source:=\"{{}}\".clone();value:{ty}:=json.decode(source)?;source=\"replacement\".clone();return json.encode(value)}}"
                ),
            ),
            (
                "input-consume",
                format!(
                    "fn consume(s:string) {{}} fn bad()->Result<string,Error> {{source:=\"{{}}\".clone();value:{ty}:=json.decode(source)?;consume(source);return json.encode(value)}}"
                ),
            ),
            (
                "arena-return",
                format!(
                    "fn bad(source:str)->Result<{ty},Error> {{arena {{value:{ty}:=json.decode(source)?;return Ok(value)}}}}"
                ),
            ),
            (
                "named-arena-return",
                format!(
                    "fn bad(source:str)->Result<{ty},Error> {{arena region {{value:{ty}:=json.decode(source)?;return Ok(value)}}}}"
                ),
            ),
        ] {
            let source = format!("import core.json\n{definitions}\n{body}\nfn main()->i32=0\n");
            let mut sources = SourceMap::new();
            let checked = align_driver::check(&mut sources, "mixed-lifetime.align", &source);
            let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
            if !checked.diags.has_errors()
                || !(diagnostics.contains("cannot return")
                    || diagnostics.contains("invalidated borrow")
                    || diagnostics.contains("borrowed"))
            {
                failures.push(format!("{name}/{scope}: {diagnostics}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn mixed_json_bounded_reserves_transitive_text_until_cap_completes() {
    let mut failures = Vec::new();
    for (name, definitions, ty) in [
        ("direct", "M {owned:string,text:str}", "M"),
        ("optional", "M {owned:string,text:Option<str>}", "M"),
        ("array", "M {owned:string,text:array<str>}", "M"),
        (
            "optional-array",
            "M {owned:string,text:Option<array<str>>}",
            "M",
        ),
        ("nested", "Leaf {text:str} M {owned:string,leaf:Leaf}", "M"),
        (
            "nested-array",
            "Leaf {text:str} M {owned:string,leaves:array<Leaf>}",
            "M",
        ),
        ("array-root", "M {owned:string,text:str}", "array<M>"),
    ] {
        for (action, cap) in [
            ("replace", "{source=\"replacement\".clone();1000}"),
            ("consume", "{consume(source);1000}"),
        ] {
            let source = format!(
                "import core.json\n{definitions}\nfn consume(s:string) {{}}\nfn bad()->Result<string,Error> {{mut source:=\"{{}}\".clone();value:{ty}:=json.decode(source)?;return json.encode_bounded(value,{cap})}}\nfn main()->i32=0\n"
            );
            let mut sources = SourceMap::new();
            let checked = align_driver::check(&mut sources, "mixed-cap.align", &source);
            let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
            if !checked.diags.has_errors()
                || !(diagnostics
                    .contains("value snapshot was invalidated before the enclosing operation"))
            {
                failures.push(format!("{name}/{action}: {diagnostics}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const MODEL: &str = r#"module model
import core.json
pub Row {owned:string,text:str}
pub Generic<T> {owned:string,text:T}
pub Deep {row:Row,maybe:Option<Row>,rows:array<Row>,tags:Option<array<str>>}
pub fn decode(input:str)->Result<Row,Error> = json.decode(input)
pub fn deep(input:str)->Result<Deep,Error> = json.decode(input)
pub fn generic(input:str)->Result<Generic<str>,Error> = json.decode(input)
pub fn identity<T>(value:T)->T = value
pub fn map(error:Error)->Error = error
pub fn controls(input:str,way:i64)->Result<Row,Error> {
  if way<0 {discard:=decode(input)?;return Err(Error.Invalid)}
  raw:=if way==0 {decode(input)} else {match way {1=>decode(input),_=>loop {break decode(input)}}}
  return identity(raw.map_err(map))
}
"#;
const IMPORTED_MAIN: &str = r#"module main
import model
import core.json
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
fn consume(value:model.Row) {}
fn later()->Result<i64,Error> = Err(Error.Invalid)
fn pair(value:model.Row,count:i64) {}
fn unwind(input:str)->Result<(),Error> {pair(model.decode(input)?,later()?);return Ok(())}
fn exercise()->Result<(),Error> {
  source:="{\"owned\":\"owner\",\"text\":\"borrow\"}".clone()
  mut way:=-1
  loop {
    if way==3 {break}
    match model.controls(source,way) {Ok(row)=>{if row.text!="borrow" {return Err(Error.Invalid)}},Err(_)=>{if way>=0 {return Err(Error.Invalid)}}}
    way=way+1
  }
  mut row:=model.decode(source)?
  row=model.decode(source)?
  if row.text!="borrow" {return Err(Error.Invalid)}
  consume(row)
  discarded:Result<model.Row,Error>:=model.decode(source)
  match unwind(source) {Ok(_)=>{return Err(Error.Invalid)},Err(_)=>{}}
  from_else:=model.decode("{}") else {model.decode(source)?}
  if from_else.text!="borrow" {return Err(Error.Invalid)}
  generic:model.Generic<str>:=model.generic(source)?
  if generic.text!="borrow" {return Err(Error.Invalid)}
  arena scratch {
    deep:model.Deep:=json.decode("{\"row\":{\"owned\":\"a\",\"text\":\"x\\ny\"},\"maybe\":{\"owned\":\"b\",\"text\":\"z\"},\"rows\":[{\"owned\":\"c\",\"text\":\"q\"}],\"tags\":[\"u\\tv\"]}")?
    if deep.row.text!="x\ny" || deep.rows.len()!=1 {return Err(Error.Invalid)}
  }
  return Ok(())
}
fn roundtrip()->Result<(),Error> {
  source:="{\"owned\":\"owner\",\"text\":\"borrow\"}".clone()
  value:=model.decode(source)?
  text:=json.encode(value)?
  bounded:=json.encode_bounded(value,text.len())?
  if text!=source || text!=bounded {return Err(Error.Invalid)}
  return Ok(())
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  before:=unsafe {align_rt_alloc_count()}
  free:=unsafe {align_rt_free_count()}
  match exercise() {Ok(_)=>{},Err(_)=>{return 1}}
  count:=unsafe {align_rt_alloc_count()}-before
  if count<=0 || unsafe {align_rt_free_count()}-free!=count || unsafe {align_rt_requested_live_bytes()}!=0 {print(count);print(unsafe {align_rt_free_count()}-free);print(unsafe {align_rt_requested_live_bytes()});return 2}
  match roundtrip() {Ok(_)=>{},Err(_)=>{return 3}}
  if unsafe {align_rt_requested_live_bytes()}!=0 {return 4}
  return 0
}
"#;
#[test]
fn mixed_json_import_generic_lifecycle_and_cache_identity() {
    owned_fixture::run(
        "mixed_json_import_generic_lifecycle_and_cache_identity",
        |stage| {
            let result = (|| -> TestResult {
                let model = stage.join("model.align");
                std::fs::write(&model, MODEL)?;
                for per_unit in [false, true] {
                    let built = programs(stage, IMPORTED_MAIN, per_unit)?;
                    execute(
                        stage,
                        &built,
                        if per_unit {
                            "import-units"
                        } else {
                            "import-whole"
                        },
                        false,
                    )?;
                }
                // Imported return facts, including concrete generics and loop/map_err joins,
                // must not erase the input owner when the call itself succeeds.
                for (ty, call) in [
                    ("model.Row", "model.decode(source)"),
                    ("model.Generic<str>", "model.generic(source)"),
                    ("model.Row", "model.controls(source,2)"),
                ] {
                    let source = format!("module main\nimport model\nfn bad()->Result<{ty},Error> {{source:=\"{{}}\".clone();value:={call}?;return Ok(value)}}\nfn main()->i32=0\n");
                    let entry = stage.join("main.align");
                    std::fs::write(&entry, &source)?;
                    for per_unit in [false, true] {
                        let mut sources = SourceMap::new();
                        let path = entry.to_str().ok_or("fixture path")?;
                        let diagnostics = if per_unit {
                            align_driver::build_per_unit(&mut sources, path, &source).diags
                        } else {
                            align_driver::check(&mut sources, path, &source).diags
                        };
                        let text = align_driver::format_diagnostics(&sources, &diagnostics);
                        assert!(
                            diagnostics.has_errors() && text.contains("cannot return"),
                            "{call}/{per_unit}: {text}"
                        );
                    }
                }
                std::fs::write(stage.join("main.align"), IMPORTED_MAIN)?;
                let entry = stage.join("main.align");
                let cache = align_driver::CacheContext::at(stage.join("cache"));
                let mut original_keys = Vec::new();
                for (round, owned, hit) in [
                    (0, false, false),
                    (1, false, true),
                    (2, true, false),
                    (3, true, true),
                    (4, false, true),
                ] {
                    let source = if owned {
                        MODEL.replace(
                            "pub Row {owned:string,text:str}",
                            "pub Row {owned:string,text:string}",
                        )
                    } else {
                        MODEL.into()
                    };
                    std::fs::write(&model, source)?;
                    let mut sources = SourceMap::new();
                    let mut built = align_driver::build_package(
                        &mut sources,
                        entry.to_str().ok_or("entry path")?,
                        IMPORTED_MAIN,
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
                        built.units.iter().all(|unit| unit
                            .frontend
                            .as_ref()
                            .is_some_and(|frontend| frontend.hit == hit)),
                        "round {round}: expected hit={hit}"
                    );
                    let lowered = (0..built.units.len())
                        .map(|index| built.materialize(index).map(Clone::clone))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| format!("{error:?}"))?;
                    let keys = lowered
                        .iter()
                        .map(align_interface::codegen_impl_hash)
                        .collect::<Vec<_>>();
                    if round == 0 {
                        original_keys = keys;
                    } else if owned {
                        assert_ne!(keys, original_keys, "Str/String must alter canonical MIR");
                    } else {
                        assert_eq!(
                            keys, original_keys,
                            "restored schema must restore canonical MIR"
                        );
                    }
                    execute(stage, &lowered, &format!("cache-{round}"), false)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                panic!("{error}");
            }
        },
    );
}

#[test]
fn mixed_json_loop_transfer_keeps_arena_ownership_rejection() {
    for expression in [
        "loop {break M {owned:\"owner\".clone(),text:\"borrow\",values:[1].to_array()}}",
        "loop {break Ok(M {owned:\"owner\".clone(),text:\"borrow\",values:[1].to_array()})}",
    ] {
        let wrapped = expression.contains("break Ok");
        let ty = if wrapped { "Result<M,Error>" } else { "M" };
        let source = format!(
            "M {{owned:string,text:str,values:array<i64>}} fn take(value:{ty}) {{}} fn bad() {{arena {{value:{ty}:={expression};take(value)}}}} fn main()->i32=0"
        );
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, "mixed-loop-arena.align", &source);
        let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
        assert!(
            checked.diags.has_errors() && diagnostics.contains("arena"),
            "{expression}: {diagnostics}"
        );
    }
}
