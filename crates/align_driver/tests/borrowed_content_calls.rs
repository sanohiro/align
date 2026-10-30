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
fn mixed_numeric_text_publications_keep_conservative_dependencies() {
    // An ordinal-only self-source cannot distinguish an independently stored string from
    // text validated over a numeric sibling's bytes. Keep that limitation explicit while
    // the lifecycle positives below exercise independent numeric and text domains.
    for action in ["set(row);set(row)", "publish(row);row.numbers[0]=-1"] {
        let source = format!(r#"
R {{text:array<str>,numbers:array<i64>}}
fn set(borrow mut row:R){{row.text[0]="new";row.numbers[0]=7}}
fn publish(borrow mut row:R){{values:slice<i64>:=row.numbers;row.text[0]=values.as_bytes().as_str() else ""}}
fn main(){{mut row:=R{{text:["old"].to_array(),numbers:[0].to_array()}};{action};print(row.text[0])}}
"#);
        for per_unit in [false, true] {
            let mut sources = SourceMap::new();
            let checked = if per_unit {
                align_driver::check_per_unit(&mut sources, "mixed-publication.align", &source).diags
            } else {
                align_driver::check(&mut sources, "mixed-publication.align", &source).diags
            };
            let diagnostics = align_driver::format_diagnostics(&sources, &checked);
            assert!(checked.has_errors() && diagnostics.contains("validated bytes were modified"),
                "{action}/{per_unit}: {diagnostics}");
        }
    }
}

#[test]
fn values_and_storage() {
    owned_fixture::run("values_and_storage", |stage| {
        execute(stage, r#"
R {text:array<str>,fixed:[str;1],owner:string}
A {text:array<str>,fixed:[str;1]}
Outer {inner:A}
fn numbers(borrow mut a:array<i64>){a[0]=7}
fn texts(borrow mut a:array<str>){a[0]="new"}
fn row(borrow mut r:R){r.text[0]="new";r.fixed[0]="new"}
fn nested(borrow mut r:Outer){r.inner.text[0]="new";r.inner.fixed[0]="new"}
fn forwarding(borrow mut r:R){row(r);row(r)}
fn arena_row(borrow mut r:A){r.text[0]="new";r.fixed[0]="new"}
fn main()->i32 {
 mut a:=[1].to_array();numbers(a)
 mut b:=["old"].to_array();texts(b)
 mut r:=R{text:["old"].to_array(),fixed:["old"],owner:"kept".clone()}
 forwarding(r)
 if a[0]!=7 || b[0]!="new" || r.text[0]!="new" || r.fixed[0]!="new" || r.owner!="kept" {return 1}
 arena {
  mut aa:=[1].to_array();numbers(aa);numbers(aa)
  mut bb:=["old"].to_array();texts(bb);texts(bb)
  mut rr:=A{text:["old"].to_array(),fixed:["old"]}
  arena_row(rr);arena_row(rr)
  if aa[0]!=7 || bb[0]!="new" || rr.text[0]!="new" || rr.fixed[0]!="new" {return 2}
  mut outer:=Outer{inner:A{text:["old"].to_array(),fixed:["old"]}};nested(outer)
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
R {text:array<str>,owner:string}
A {text:array<str>}
fn fresh()->R=R{text:["old"].to_array(),owner:"kept".clone()}
fn write(borrow mut r:R){r.text[0]="new"}
fn replace(borrow mut r:R){r=fresh();write(r)}
fn arena_write(borrow mut r:A){r.text[0]="new"}
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
 if moved.text[0]!="new" || moved.owner!="kept" {return Err(Error.Invalid)}
 arena {
  mut ar:=A{text:["old"].to_array()}
  arena_write(ar);arena_write(ar)
  if ar.text[0]!="new" {return Err(Error.Invalid)}
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

#[test]
fn arena_decoded_storage() {
    owned_fixture::run("arena_decoded_storage", |stage| {
        std::fs::write(
            stage.join("decoded.align"),
            r#"module decoded
pub Row {values:array<str>,tags:array<str>,text:str}
pub fn change(borrow mut row:Row) {
 if row.values.len()>0 {row.values[0]="seven"}
 if row.tags.len()>0 {row.tags[0]="new"}
 row.text="changed"
}
pub fn forward(borrow mut row:Row){change(row);change(row)}
pub fn inspect<T>(borrow mut value:T){}
"#,
        )
        .unwrap();
        execute(stage, r#"
import core.json
import decoded
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
fn fail()->Result<i64,Error> = Err(Error.Invalid)
fn mapper(e:Error)->Error=e
fn exercise(bad:bool)->Result<(),Error>{
 input:="{\"values\":[\"one\",\"two\"],\"tags\":[\"old\",\"\\u0061\"],\"text\":\"old\"}".clone()
 arena outer {
  mut row:decoded.Row:=json.decode(input)?
  if unsafe {align_rt_requested_live_bytes()}<=0 {return Err(Error.Invalid)}
  before:=unsafe {align_rt_alloc_count()};freed:=unsafe {align_rt_free_count()}
  decoded.forward(row)
  decoded.inspect(row)
  mut n:=0
  loop {if n==3 {break};decoded.change(row);n=n+1}
  if row.values[0]!="seven" || row.values[1]!="two" || row.tags[0]!="new" || row.tags[1]!="a" || row.text!="changed" {return Err(Error.Invalid)}
  if unsafe {align_rt_alloc_count()}!=before || unsafe {align_rt_free_count()}!=freed {return Err(Error.Invalid)}
  mut empty:decoded.Row:=json.decode("{\"values\":[],\"tags\":[],\"text\":\"empty\"}")?
  decoded.forward(empty)
  if empty.values.len()!=0 || empty.tags.len()!=0 || empty.text!="changed" {return Err(Error.Invalid)}
  if bad {unused:=fail().map_err(mapper)?}
  row=json.decode(input)?
  mut moved:=row
  decoded.forward(moved)
  if moved.values[0]!="seven" || moved.tags[1]!="a" {return Err(Error.Invalid)}
  malformed:Result<decoded.Row,Error>:=json.decode("{\"values\":[\"one\"],\"tags\":[\"ok\",false],\"text\":\"bad\"}")
  match malformed {Ok(_)=>{return Err(Error.Invalid)},Err(_)=>{}}
 }
 return Ok(())
}
fn main()->i32 {
 unsafe {align_rt_requested_live_reset()}
 before:=unsafe {align_rt_alloc_count()};freed:=unsafe {align_rt_free_count()}
 live:=unsafe {align_rt_requested_live_bytes()}
 match exercise(false){Err(_)=>{return 1},Ok(_)=>{}}
 match exercise(true){Ok(_)=>{return 2},Err(_)=>{}}
 if unsafe {align_rt_alloc_count()}-before!=unsafe {align_rt_free_count()}-freed || unsafe {align_rt_requested_live_bytes()}!=live {print(unsafe {align_rt_alloc_count()}-before);print(unsafe {align_rt_free_count()}-freed);print(live);print(unsafe {align_rt_requested_live_bytes()});return 3}
 return 0
}"#, "arena-decoded").unwrap();
    });
}

#[test]
fn arena_decoded_shapes() {
    owned_fixture::run("arena_decoded_shapes", |stage| {
        let package = stage.join("pkg/csv/internal");
        std::fs::create_dir_all(&package).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/csv/pkg");
        std::fs::copy(root.join("csv.align"), stage.join("pkg/csv.align")).unwrap();
        std::fs::copy(
            root.join("csv/internal/descriptor.align"),
            package.join("descriptor.align"),
        )
        .unwrap();
        execute(stage, r#"
import core.json
import pkg.csv
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
Inner {values:array<i64>}
Row {inner:Inner,text:str}
Cell {value:i64,text:str}
Choice {Record(Row),Number(i64)}
fn nested(borrow mut row:Row){row.inner.values[0]=9}
fn rows(borrow mut values:array<Row>){}
fn columns(borrow values:soa<Cell>){}
fn choice(borrow mut value:Choice){}
fn exercise()->Result<(),Error>{
 arena {
  mut row:Row:=json.decode("{\"inner\":{\"values\":[1]},\"text\":\"live\"}")?
  nested(row)
  if row.inner.values[0]!=9 || row.text!="live" {return Err(Error.Invalid)}
  mut list:array<Row>:=json.decode("[{\"inner\":{\"values\":[2]},\"text\":\"live\"}]")?
  rows(list)
  if list.len()!=1 || list[0].text!="live" {return Err(Error.Invalid)}
  mut table:soa<Cell>:=json.decode("[{\"value\":3,\"text\":\"live\"}]")?
  columns(table)
  if table.value[0]!=3 || table.text[0]!="live" {return Err(Error.Invalid)}
  mut tagged:Choice:=json.decode("{\"inner\":{\"values\":[4]},\"text\":\"live\"}")?
  choice(tagged)
  if unsafe {align_rt_requested_live_bytes()}<=0 {return Err(Error.Invalid)}
 }
 return Ok(())
}
fn main()->i32 {
 unsafe {align_rt_requested_live_reset()}
 before:=unsafe {align_rt_alloc_count()};freed:=unsafe {align_rt_free_count()}
 match exercise(){Ok(_)=>{},Err(_)=>{return 1}}
 arena out {
  options:=pkg.csv.DecodeOptions {header:pkg.csv.Header.Absent,line_ending:pkg.csv.LineEnding.Lf,max_rows:2}
  result:Result<soa<Cell>,pkg.csv.Error>:=pkg.csv.decode("5,live",out,options)
  mut table:=result else {return 2}
  columns(table)
  if table.value[0]!=5 || table.text[0]!="live" {return 3}
 }
 if unsafe {align_rt_alloc_count()}-before!=unsafe {align_rt_free_count()}-freed || unsafe {align_rt_requested_live_bytes()}!=0 {return 4}
 return 0
}"#, "arena-decoded-shapes").unwrap();
    });
}

#[test]
fn arena_decoder_borrowed_text_keeps_its_lifetime() {
    owned_fixture::run("arena_decoder_borrowed_text_keeps_its_lifetime", |stage| {
        for (declaration, ty, json) in [
            (
                "R {values:array<i64>,text:str}",
                "R",
                r#"{"values":[1],"text":"escaped\\u0061"}"#,
            ),
            (
                "R {values:array<i64>,text:str}",
                "array<R>",
                r#"[{"values":[1],"text":"escaped\\u0061"}]"#,
            ),
            (
                "R {values:array<i64>,text:str} Choice {One(R),Number(i64)}",
                "Choice",
                r#"{"values":[1],"text":"escaped\\u0061"}"#,
            ),
        ] {
            let literal = format!("\"{}\"", json.replace('\\', "\\\\").replace('"', "\\\""));
            for body in [
                format!(
                    "fn escaped()->Result<{ty},Error>{{arena {{value:{ty}:=json.decode({literal})?;return Ok(value)}}}}"
                ),
                format!(
                    "fn invalidated()->Result<(),Error>{{mut input:={literal}.clone();arena {{value:{ty}:=json.decode(input)?;input=\"other\".clone();observe(value)}};return Ok(())}}"
                ),
            ] {
                let source = format!(
                    "import core.json\n{declaration}\nfn observe(borrow value:{ty}){{}}\n{body}\nfn main(){{}}"
                );
                let path = stage.join("lifetime.align");
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
                        "{ty}/per-unit={per_unit}: {source}"
                    );
                    assert!(
                        rendered.contains("arena")
                            || rendered.contains("outlive")
                            || rendered.contains("invalidat")
                            || rendered.contains("borrow"),
                        "{rendered}"
                    );
                }
            }
        }
    });
}

#[test]
fn arena_union_error_cleanup() {
    owned_fixture::run("arena_union_error_cleanup", |stage| {
        let base = r#"
import core.json
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_requested_live_peak()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
Part {value:i64}
Row {text:str,values:array<i64>,required:i64}
Choice {Many(array<Part>),One(Row)}
Outer {choice:Choice,tail:i64}
fn exercise()->Result<(),Error>{
 arena {
  partial:Result<Outer,Error>:=json.decode("{\"choice\":{\"text\":\"borrowed\"}}")
  match partial {Ok(_)=>{return Err(Error.Invalid)},Err(_)=>{}}
  trailing:Result<Choice,Error>:=json.decode("{\"text\":\"borrowed\",\"values\":[1],\"required\":2} trailing")
  match trailing {Ok(_)=>{return Err(Error.Invalid)},Err(_)=>{}}
  sibling:Result<Outer,Error>:=json.decode("{\"choice\":{\"text\":\"borrowed\",\"values\":[1],\"required\":2}}")
  match sibling {Ok(_)=>{return Err(Error.Invalid)},Err(_)=>{}}
  rows:Result<array<Outer>,Error>:=json.decode("[{\"choice\":{\"text\":\"borrowed\",\"values\":[1],\"required\":2},\"tail\":3},{\"choice\":{\"text\":\"borrowed\",\"values\":[1]}}]")
  match rows {Ok(_)=>{return Err(Error.Invalid)},Err(_)=>{}}
 }
 return Ok(())
}
fn main()->i32 {
 unsafe {align_rt_requested_live_reset()}
 before:=unsafe {align_rt_alloc_count()};freed:=unsafe {align_rt_free_count()}
 match exercise(){Ok(_)=>{},Err(_)=>{return 1}}
 if unsafe {align_rt_requested_live_peak()}<=0 {return 2}
 if unsafe {align_rt_requested_live_bytes()}!=0 || unsafe {align_rt_alloc_count()}-before!=unsafe {align_rt_free_count()}-freed {return 3}
 return 0
}"#;
        for record_first in [false, true] {
            let source = if record_first {
                base.replace(
                    "Choice {Many(array<Part>),One(Row)}",
                    "Choice {One(Row),Many(array<Part>)}",
                )
            } else {
                base.into()
            };
            execute(stage, &source, &format!("union-errors-{record_first}")).unwrap();
        }
    });
}
