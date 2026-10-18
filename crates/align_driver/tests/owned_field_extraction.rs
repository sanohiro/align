//! Plan192: direct owning fields transfer complete values and exact cleanup/storage authority.
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

const VALUES: &str = r#"
Entry { key:string, value:i64 }
Empty {values:[Entry;0]}
Inner { name:string, marker:i64 }
Outer { entries:array<Entry>, inner:Inner, pair:(string,i64), fixed:[string;2], marker:i64 }
fn take(values:array<Entry>)->i64 = values[0].value + values[0].key.len()
fn pair(value:(string,i64))->i64 = value.0.len()+value.1
fn make()->Outer {
  mut builder:array_builder<Entry>:=array_builder()
  builder.push(Entry {key:"key".clone(),value:7})
  return Outer {entries:builder.build(),inner:Inner {name:"inner".clone(),marker:11},pair:("pair".clone(),13),fixed:["a".clone(),"bc".clone()],marker:19}
}
fn nested(owner:Outer)->Inner = owner.inner
fn returned(owner:Outer)->array<Entry> {return owner.entries}
fn exercise(flag:bool)->i32 {
  empty:={
    empty_owner:=Empty {values:[]}
    empty_owner.values
  }
  if empty.len()!=0 {return 12}
  owner:=make()
  if take(owner.entries)!=10 {return 1}
  if owner.inner.name!="inner" || owner.inner.marker!=11 {return 2}
  selected:=owner.inner
  if selected.name!="inner" || owner.marker!=19 {return 3}
  if pair(owner.pair)!=17 {return 4}
  fixed:=owner.fixed
  if fixed[0]!="a" || fixed[1]!="bc" || owner.marker!=19 {return 5}
  branch:=make()
  branch_value:=if flag {branch.inner} else {branch.inner}
  if branch_value.marker!=11 {return 13}
  matched:=make()
  match_value:=match (if flag {1} else {0}) {1=>matched.inner,_=>matched.inner}
  if match_value.name!="inner" {return 14}
  joined:=if flag {nested(make())} else {nested(make())}
  if joined.name!="inner" {return 6}
  moved:=returned(make())
  if moved[0].key!="key" {return 7}
  other:=make()
  wrapped:=make()
  optional:=Some(wrapped.entries)
  from_option:=optional else {return 9}
  if take(from_option)!=10 {return 10}
  child:=make()
  aggregate:=Outer {entries:child.entries,inner:child.inner,pair:child.pair,fixed:child.fixed,marker:child.marker}
  if take(aggregate.entries)!=10 {return 11}
  callback:=take
  if callback(other.entries)!=10 {return 8}
  return 0
}
fn main()->i32 {a:=exercise(true); if a!=0 {return a}; return exercise(false)}
"#;
#[test]
fn field_extraction_values_and_control() {
    owned_fixture::run("field_extraction_values_and_control", |stage| {
        let result = (|| -> TestResult {
            let built = programs(stage, VALUES, false)?;
            execute(stage, &built, "values", false)
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}

#[test]
fn field_extraction_authority_matrix() {
    owned_fixture::run("field_extraction_authority_matrix", |stage| {
        let cases = [
            (
                "dynamic-alias",
                true,
                "R { values:array<i64>, sibling:array<i64> } fn main()->i32 { r:=R {values:[1,2].to_array(),sibling:[3].to_array()}; alias:slice<i64>:=r.values; sibling:slice<i64>:=r.sibling; taken:=r.values; if alias[0]!=1 || sibling[0]!=3 || taken[1]!=2 {return 1}; return 0 }",
            ),
            (
                "destination-replacement",
                false,
                "R { values:array<i64> } fn main()->i32 { r:=R {values:[1].to_array()}; alias:slice<i64>:=r.values; mut taken:=r.values; taken=[2].to_array(); return alias[0] as i32 }",
            ),
            (
                "contained-view",
                false,
                r#"R { values:array<str> } fn main()->i32 { mut text:="old".clone(); view:str:=text; r:=R {values:[view].to_array()}; taken:=r.values; text="new".clone(); return taken[0].len() as i32 }"#,
            ),
            (
                "borrowed",
                false,
                "R { values:array<i64> } fn take(borrow r:R)->array<i64> = r.values fn main()->i32=0",
            ),
            (
                "borrowed-mut",
                false,
                "R { values:array<i64> } fn take(borrow mut r:R)->array<i64> = r.values fn main()->i32=0",
            ),
            (
                "reuse-field",
                false,
                "R { values:array<i64> } fn main()->i32 {r:=R {values:[1].to_array()}; a:=r.values; return r.values.len() as i32}",
            ),
            (
                "reuse-parent",
                false,
                "R { values:array<i64> } fn take(r:R)->i64=r.values.len() fn main()->i32 {r:=R {values:[1].to_array()}; a:=r.values; return take(r) as i32}",
            ),
            (
                "duplicate",
                false,
                "R { values:array<i64> } fn take(a:array<i64>,b:array<i64>)->i64=a.len()+b.len() fn main()->i32 {r:=R {values:[1].to_array()}; return take(r.values,r.values) as i32}",
            ),
            (
                "deep",
                false,
                "I { values:array<i64> } R { inner:I } fn main()->i32 {r:=R {inner:I {values:[1].to_array()}}; a:=r.inner.values; return a.len() as i32}",
            ),
            (
                "refill",
                false,
                "R { values:array<i64> } fn main()->i32 {mut r:=R {values:[1].to_array()}; a:=r.values; r.values=[2].to_array(); return 0}",
            ),
            (
                "inline-backing",
                false,
                r#"I { fixed:[i64;2], text:string } R { inner:I } fn main()->i32 {r:=R {inner:I {fixed:[1,2],text:"a".clone()}}; alias:slice<i64>:=r.inner.fixed; moved:=r.inner; return alias[0] as i32}"#,
            ),
            (
                "inline-sibling",
                true,
                r#"I { fixed:[i64;2], text:string } R { inner:I, other:array<i64> } fn main()->i32 {r:=R {inner:I {fixed:[1,2],text:"a".clone()},other:[3].to_array()}; alias:slice<i64>:=r.inner.fixed; moved:=r.other; if alias[0]!=1 || r.inner.text!="a" || moved[0]!=3 {return 1}; return 0}"#,
            ),
            (
                "later-replacement",
                false,
                r#"R { values:array<i64> } fn take(a:array<i64>,b:i64)->i64=a.len()+b fn main()->i32 {mut r:=R {values:[1].to_array()}; return take(r.values,{r.values=[2].to_array();0}) as i32}"#,
            ),
            (
                "duplicate-constructor",
                false,
                r#"R { values:array<i64> } Pair {a:array<i64>,b:array<i64>} fn main()->i32 {r:=R {values:[1].to_array()}; p:=Pair {a:r.values,b:r.values}; return 0}"#,
            ),
            (
                "unsupported-field",
                false,
                r#"R { values:array<slice<i64>> } fn main()->i32=0"#,
            ),
            (
                "descendant",
                false,
                r#"I { name:string } R { inner:I } fn main()->i32 {r:=R {inner:I {name:"a".clone()}}; moved:=r.inner; return r.inner.name.len() as i32}"#,
            ),
            (
                "dynamic-scope",
                true,
                r#"R { values:array<i64>, sibling:array<i64> } fn main()->i32 {moved:={r:=R {values:[1].to_array(),sibling:[2].to_array()};r.values}; if moved[0]!=1 {return 1};return 0}"#,
            ),
            (
                "borrowed-match",
                false,
                r#"R { values:array<i64> } fn bad(borrow maybe:Option<R>)->array<i64> = match maybe {Some(r)=>r.values,None=>[0].to_array()} fn main()->i32=0"#,
            ),
            (
                "mixed-dynamic-alias",
                true,
                r#"I {values:array<i64>,text:string} R {value:I} fn main()->i32 {owner:=R {value:I {values:[1].to_array(),text:"live".clone()}}; alias:slice<i64>:=owner.value.values; moved:=owner.value; if alias[0]!=1 || moved.text!="live" {return 1};return 0}"#,
            ),
            (
                "tuple-dynamic-alias",
                true,
                r#"fn main()->i32 {owner:=([1].to_array(),"live".clone());alias:slice<i64>:=owner.0;moved:=owner.0;if alias[0]!=1 || moved[0]!=1 || owner.1!="live" {return 1};return 0}"#,
            ),
        ];
        legacy_mixed_field_authority(stage);
        for (name, accepted, source) in cases {
            let path = stage.join(format!("{name}.align"));
            std::fs::write(&path, source).unwrap();
            let mut sources = SourceMap::new();
            let checked = align_driver::check(&mut sources, path.to_str().unwrap(), source);
            let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
            assert_eq!(
                !checked.diags.has_errors(),
                accepted,
                "{name}: {diagnostics}"
            );
            if accepted {
                let mir =
                    align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
                execute(stage, &[mir], name, false).unwrap();
            } else {
                assert!(
                    !diagnostics.contains("expected") && !diagnostics.contains("unknown type"),
                    "invalid witness {name}: {diagnostics}"
                );
            }
        }
    });
}

// Allocated siblings never certify the release lifetime of an inline owner. Every source is
// check-only: the rejected alternatives deliberately describe views of released storage.
fn legacy_mixed_field_authority(stage: &Path) {
    let mut failures = Vec::new();
    for (family, inner, view) in [
        (
            "string",
            "I {values:array<i64>, text:string}",
            "owner.value.text",
        ),
        (
            "buffer",
            "I {values:array<i64>, data:buffer}",
            "owner.value.data.bytes()",
        ),
        (
            "nested",
            "Text {name:string} I {values:array<i64>, text:Text}",
            "owner.value.text.name",
        ),
        (
            "fixed",
            "I {values:array<i64>, texts:[string;1]}",
            "owner.value.texts[0]",
        ),
        (
            "fixed-record",
            "Text {name:string} I {values:array<i64>, texts:[Text;1]}",
            "owner.value.texts[0].name",
        ),
    ] {
        for (action, transfer) in [
            ("bind-replace", "mut taken:=owner.value; taken=replacement"),
            ("direct-call", "consume(owner.value)"),
            ("indirect-call", "callback:=consume; callback(owner.value)"),
            ("constructor", "{wrapped:=Wrap {value:owner.value}; ()}"),
        ] {
            let view_type = if family == "buffer" {
                "slice<u8>"
            } else {
                "str"
            };
            let source = format!(
                "{inner}\nR {{value:I}}\nWrap {{value:I}}\nfn consume(value:I) {{}}\nfn probe(owner:R,replacement:I)->i64 {{view:{view_type}:={view};{transfer};return view.len()}}\nfn main()->i32=0\n"
            );
            let path = stage.join(format!("legacy-{family}-{action}.align"));
            std::fs::write(&path, &source).unwrap();
            let mut sources = SourceMap::new();
            let checked = align_driver::check(&mut sources, path.to_str().unwrap(), &source);
            let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
            if !checked.diags.has_errors()
                || !diagnostics.contains("use of invalidated borrow 'view'")
            {
                failures.push(format!("{family}/{action}: {diagnostics}"));
            }
        }
    }
    for (name, source) in [
        (
            "tuple-field",
            r#"R {value:(string,array<i64>)} fn consume(value:(string,array<i64>)) {} fn text(borrow value:(string,array<i64>))->str=value.0[0..value.0.len()] fn probe(owner:R)->i64 {view:=text(owner.value);consume(owner.value);return view.len()} fn main()->i32=0"#,
        ),
        (
            "tuple-direct",
            r#"fn probe()->i64 {owner:=("old".clone(),[1].to_array());view:str:=owner.0;mut taken:=owner.0;taken="new".clone();return view.len()} fn main()->i32=0"#,
        ),
        (
            "tuple-deferred",
            r#"fn consume(value:string) {} fn probe()->i64 {owner:=("old".clone(),[1].to_array());view:str:=owner.0;consume(owner.0);return view.len()} fn main()->i32=0"#,
        ),
    ] {
        let mut sources = SourceMap::new();
        let path = stage.join(format!("legacy-{name}.align"));
        std::fs::write(&path, source).unwrap();
        let checked = align_driver::check(&mut sources, path.to_str().unwrap(), source);
        let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
        if !checked.diags.has_errors() || !diagnostics.contains("use of invalidated borrow 'view'")
        {
            failures.push(format!("{name}: {diagnostics}"));
        }
    }
    assert!(
        failures.is_empty(),
        "legacy mixed ownership: {}",
        failures.join("\n")
    );
}

const LIFECYCLE: &str = r#"
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
R { values:array<i64>, sibling:array<i64> }
I { name:string, marker:i64 }
N { inner:I, sibling:string }
fn make()->R = R {values:[7,8].to_array(),sibling:[9].to_array()}
fn nested()->N = N {inner:I {name:"owned".clone(),marker:33},sibling:"other".clone()}
fn consume(values:array<i64>, n:i64)->i64 = values.sum()+n
fn early(stop:bool)->i32 {
  owner:=make()
  _:=consume(owner.values,{if stop {return 0};owner.sibling[0]})
  return 0
}
fn failed(stop:bool)->Result<(),Error> {
  owner:=make()
  _:=consume(owner.values,{if stop {return Err(Error.Invalid)};1})
  return Ok(())
}
fn through_result()->Result<array<i64>,Error> {owner:=make();return Ok(owner.values)}
fn through_loop()->array<i64> {owner:=make();return loop {break owner.values}}
fn nested_join(heap:bool)->i32 {
  arena {
    mut owner:=N {inner:I {name:"arena".clone(),marker:33},sibling:"other".clone()}
    if heap {owner=nested()}
    selected:=owner.inner
    if selected.name.len()!=5 || selected.marker!=33 || owner.sibling!="other" {return 1}
  }
  return 0
}
fn joined(heap:bool)->i32 {
  arena {
    mut owner:=R {values:[1,2].to_array(),sibling:[3].to_array()}
    if heap {owner=make()}
    before:=unsafe {align_rt_alloc_count()}
    selected:=owner.values
    if unsafe {align_rt_alloc_count()}!=before {return 2}
    if selected.len()!=2 || owner.sibling.len()!=1 {return 3}
    mut replacement:=[99].to_array()
    other:=make()
    replacement=other.values
    if replacement[0]!=7 {return 4}
  }
  return nested_join(heap)
}
fn run()->Result<(),Error> {
  mut turn:=0
  loop {
    if turn==8 {break}
    if joined(turn%2==0)!=0 {return Err(Error.Code(1))}
    if early(turn%2==0)!=0 {return Err(Error.Code(2))}
    match failed(turn%2==0).map_err(fn e:Error {e}) {Ok(_)=>{},Err(_)=>{}}
    values:=through_result()?
    if values.sum()!=15 {return Err(Error.Code(3))}
    looped:=through_loop()
    if looped.sum()!=15 {return Err(Error.Code(4))}
    optional:=Some(through_loop())
    picked:=optional else {return Err(Error.Code(5))}
    if picked.sum()!=15 {return Err(Error.Code(6))}
    turn=turn+1
  }
  return Ok(())
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  _:=run() else {return 7}
  if unsafe {align_rt_requested_live_bytes()}!=0 {return 8}
  if unsafe {align_rt_alloc_count()}!=unsafe {align_rt_free_count()} {return 9}
  return 0
}
"#;
#[test]
fn field_extraction_allocation_lifecycle() {
    owned_fixture::run("field_extraction_allocation_lifecycle", |stage| {
        let result = (|| -> TestResult {
            let built = programs(stage, LIFECYCLE, false)?;
            execute(stage, &built, "lifecycle", false)
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}

#[test]
fn field_extraction_imports_and_cache() {
    owned_fixture::run("field_extraction_imports_and_cache", |stage| {
        let result = (|| -> TestResult {
            let model = r#"module model
pub Container<T> { value:T, marker:i64 }
pub Inner { name:string }
pub fn extract<T>(owner:Container<T>)->T = owner.value
pub fn make()->Container<array<i64>> = Container {value:[4,5].to_array(),marker:9}
pub fn inner()->Container<Inner> = Container {value:Inner {name:"inner".clone()},marker:8}
"#;
            let source = r#"module main
import model
fn main()->i32 {
  first:=model.make()
  a:=first.value
  if a.sum()!=9 || first.marker!=9 {return 1}
  b:=model.extract(model.make())
  if b.sum()!=9 {return 2}
  owner:=model.inner()
  nested:=owner.value
  if nested.name!="inner" || owner.marker!=8 {return 3}
  c:=model.extract(model.inner())
  if c.name!="inner" {return 4}
  return 0
}
"#;
            std::fs::write(stage.join("model.align"), model)?;
            for per_unit in [false, true] {
                let built = programs(stage, source, per_unit)?;
                execute(stage, &built, &format!("imports-{per_unit}"), false)?;
            }
            let cache = align_driver::CacheContext::at(stage.join("cache"));
            let path = stage.join("main.align");
            let entry = path.to_str().ok_or("non-UTF8 path")?;
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
                        .is_some_and(|state| state.hit == (round == 1))
                }));
                let mut materialized = Vec::new();
                for index in 0..built.units.len() {
                    materialized.push(
                        built
                            .materialize(index)
                            .map_err(|error| format!("{error:?}"))?
                            .clone(),
                    );
                }
                if round == 1 {
                    execute(stage, &materialized, "cache-hit", false)?;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}

// Request 153: one operation transfers a direct field while a sibling operand conditionally
// transfers or replaces another field of the same owner. Each branch, operand order, direct and
// imported helper, and whole/per-unit build releases every transferred or retained field once.
const DISJOINT_MODEL: &str = r#"module model
pub Source { first:string, second:string, ok:bool }
pub Output { first:string, second:Option<string> }
pub fn make(ok:bool)->Source = Source {first:"first".clone(),second:"second".clone(),ok:ok}
pub fn forward(source:Source)->Output {
  return Output {first:source.first, second:if source.ok {Some(source.second)} else {None}}
}
pub fn reverse(source:Source)->Output {
  return Output {second:if source.ok {Some(source.second)} else {None}, first:source.first}
}
"#;
const DISJOINT_MAIN: &str = r#"module main
import model
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
S { first:string, second:string, ok:bool }
O { first:string, second:Option<string> }
fn make(ok:bool)->S = S {first:"first".clone(),second:"second".clone(),ok:ok}
fn forward(source:S)->O = O {first:source.first, second:if source.ok {Some(source.second)} else {None}}
fn reverse(source:S)->O = O {second:if source.ok {Some(source.second)} else {None}, first:source.first}
fn matched(source:S)->O = O {first:source.first, second:match (if source.ok {1} else {0}) {1=>Some(source.second), _=>None}}
fn replaced(ok:bool)->O {
  mut source:=make(ok)
  return O {first:source.first, second:if ok {source.second="third".clone(); Some(source.second)} else {None}}
}
I { name:string, values:array<i64> }
P { inner:I, second:string, ok:bool }
Q { inner:I, second:Option<string> }
fn headers(ok:bool)->bool {
  source:=P {inner:I {name:"first".clone(), values:[1,2].to_array()}, second:"second".clone(), ok:ok}
  output:=Q {inner:source.inner, second:if source.ok {Some(source.second)} else {None}}
  inner:=output.inner
  if inner.values.len()!=2 || inner.values[1]!=2 {return false}
  return verify(inner.name, output.second, ok, "second")
}
fn take(a:string, b:Option<string>)->i64 = a.len() + (b else {return a.len()}).len()
fn call(source:S)->i64 = take(source.first, if source.ok {Some(source.second)} else {None})
fn tuple(ok:bool)->i64 {
  owner:=("first".clone(), "second".clone())
  pair:=(owner.0, if ok {owner.1} else {"".clone()})
  return pair.0.len() + pair.1.len()
}
fn verify(first:string, second:Option<string>, ok:bool, expected:str)->bool {
  if first!="first" {return false}
  return match second {Some(text)=>ok && text==expected, None=>!ok}
}
fn local(output:O, ok:bool, expected:str)->bool = verify(output.first, output.second, ok, expected)
fn imported(output:model.Output, ok:bool)->bool = verify(output.first, output.second, ok, "second")
fn round(ok:bool)->i32 {
  if !local(forward(make(ok)), ok, "second") {return 1}
  if !local(reverse(make(ok)), ok, "second") {return 2}
  if !local(matched(make(ok)), ok, "second") {return 3}
  if !local(replaced(ok), ok, "third") {return 4}
  if call(make(ok))!=(if ok {11} else {5}) {return 5}
  if tuple(ok)!=(if ok {11} else {5}) {return 6}
  if !imported(model.forward(model.make(ok)), ok) {return 7}
  if !imported(model.reverse(model.make(ok)), ok) {return 8}
  if !headers(ok) {return 9}
  return 0
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  a:=round(true)
  if a!=0 {return a}
  b:=round(false)
  if b!=0 {return b+20}
  if unsafe {align_rt_requested_live_bytes()}!=0 {return 40}
  if unsafe {align_rt_alloc_count()}!=unsafe {align_rt_free_count()} {return 41}
  return 0
}
"#;
#[test]
fn field_extraction_disjoint_conditional_siblings() {
    owned_fixture::run("field_extraction_disjoint_conditional_siblings", |stage| {
        let result = (|| -> TestResult {
            std::fs::write(stage.join("model.align"), DISJOINT_MODEL)?;
            for per_unit in [false, true] {
                let built = programs(stage, DISJOINT_MAIN, per_unit)?;
                execute(stage, &built, &format!("disjoint-{per_unit}"), false)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}
