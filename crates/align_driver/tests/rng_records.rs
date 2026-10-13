//! Plan175: Copy RNG fields, exact state mutation, recursive layout and cache replay.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile, check};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
import std.rand
pub State { random:rng }
pub Wrap<T> { value:T }
pub fn make(seed:i64)->State = State {random:rand.seed_with(seed)}
pub fn identity<T>(value:T)->T = value
pub fn wrap<T>(value:T)->Wrap<T> = Wrap {value:value}
pub fn advance(borrow mut value:rng)->i64 = value.next()
pub fn bounded(borrow mut value:rng)->i64 = value.range(5,6)
pub fn scramble(borrow mut value:rng, out values:slice<i64>) { value.shuffle(values) }
pub fn sample(borrow mut value:rng, values:slice<i64>)->array<i64> = value.sample(values,3)
pub fn pair(borrow mut left:rng, borrow mut right:rng)->bool = left.next()==right.next()
"#;

const MAIN: &str = r#"module main
import model
import std.rand
Pair {left:model.State, right:model.State}
Mixed {text:string, view:str, random:rng}
Choice {State(model.State), Empty}
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
fn live()->i64 = unsafe {align_rt_requested_live_bytes()}
fn allocations()->i64 = unsafe {align_rt_alloc_count()}
fn witness()->i64 { value:="positive witness".clone(); return value.len() }
fn copies()->i32 {
  mut generic:=model.wrap(rand.seed_with(42))
  if model.advance(generic.value)!=-3425465463722317665 {return 35}
  mut state:=model.make(42)
  mut copy:=model.identity(state)
  mut nested:=model.wrap(model.wrap(state))
  if model.advance(state.random)!=-3425465463722317665 {return 1}
  if model.advance(state.random)!=5881210131331364753 {return 2}
  if model.advance(copy.random)!=-3425465463722317665 {return 3}
  if model.advance(nested.value.value.random)!=-3425465463722317665 {return 4}
  mut extracted:=copy.random
  if extracted.next()!=5881210131331364753 {return 5}
  if model.advance(copy.random)!=5881210131331364753 {return 6}
  state.random=rand.seed_with(42)
  if model.advance(state.random)!=-3425465463722317665 {return 7}
  state=model.make(42)
  mut pair:=Pair {left:state,right:state}
  if !model.pair(pair.left.random,pair.right.random) {return 8}
  if model.advance(pair.left.random)!=5881210131331364753 {return 9}
  if model.advance(pair.right.random)!=5881210131331364753 {return 10}
  mut at:=0
  loop {
    if at>=8 {break}
    pair.left=model.make(42)
    if model.advance(pair.left.random)!=-3425465463722317665 {return 11}
    at=at+1
  }
  return 0
}
fn selected(flag:bool)->Result<model.State,i64> {
  mut state:=if flag {model.make(42)} else {model.make(123)}
  if model.advance(state.random)==0 {return Err(1)}
  return Ok(state)
}
fn controls(flag:bool)->Result<i32,i64> {
  mut first:=selected(flag)?
  mut reference:=model.make(if flag {42} else {123})
  _:=model.advance(reference.random)
  if model.advance(first.random)!=model.advance(reference.random) {return Ok(12)}
  optional:Option<model.State> := if flag {Some(model.make(42))} else {None}
  mut unwrapped:=optional else model.make(42)
  mut looped:=loop {break unwrapped}
  if model.advance(looped.random)!=model.advance(unwrapped.random) {return Ok(13)}
  tagged:=if flag {Choice.State(model.make(42))} else {Choice.Empty}
  mut matched:=match tagged {State(value)=>value, Empty=>model.make(42)}
  if model.advance(matched.random)!=-3425465463722317665 {return Ok(14)}
  return Ok(0)
}
fn containers()->i32 {
  fixed:=[model.make(42),model.make(123)]
  dynamic:=fixed[0..2].to_array()
  mut first:=dynamic[0]
  mut second:=dynamic[1]
  mut original:=fixed[0]
  if model.advance(first.random)!=model.advance(original.random) {return 15}
  mut reference:=model.make(123)
  if model.advance(second.random)!=model.advance(reference.random) {return 16}
  return 0
}
fn methods()->i32 {
  mut state:=model.make(123)
  mut reference:=rand.seed_with(123)
  mut expected:=[10,20,30,40,50][0..5]
  reference.shuffle(expected)
  mut xs:=[10,20,30,40,50][0..5]
  model.scramble(state.random,xs)
  if xs[0]!=10 || xs[1]!=30 || xs[2]!=20 || xs[3]!=50 || xs[4]!=40 {return 17}
  if model.bounded(state.random)!=reference.range(5,6) {return 18}
  if model.advance(state.random)!=reference.next() {return 33}
  state=model.make(123)
  values:=model.sample(state.random,[100,200,300,400,500,600][0..6])
  if values.len()!=3 || values[0]!=400 || values[1]!=600 || values[2]!=500 {return 19}
  reference=rand.seed_with(123)
  expected_sample:=reference.sample([100,200,300,400,500,600][0..6],3)
  if expected_sample.len()!=3 || model.advance(state.random)!=reference.next() {return 34}
  return 0
}
fn guard(fail:bool)->Result<(),i64> {if fail {return Err(7)};return Ok(())}
fn mixed(view:str,early:bool,fail:bool)->Result<i64,i64> {
  mut value:=Mixed {text:"owned".clone(),view:view,random:rand.seed_with(42)}
  if model.advance(value.random)!=-3425465463722317665 {return Err(20)}
  if early {return Ok(value.view.len()+value.text.len())}
  value.random=rand.seed_with(42)
  if model.advance(value.random)!=-3425465463722317665 {return Err(21)}
  guard(fail)?
  return Ok(value.view.len()+value.text.len())
}
fn cleanup()->i32 {
  owner:="borrowed".clone()
  c:=mixed(owner,false,false) else {return 26}
  d:=mixed(owner,true,false) else {return 27}
  if c!=13 || d!=13 || live()!=8 {return 28}
  match mixed(owner,false,true) {Ok(value)=>{return 30},Err(error)=>{if error!=7 {return 31}}}
  if live()!=8 {return 32}
  return 0
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  start:=allocations()
  if witness()!=16 || allocations()==start || live()!=0 {return 22}
  before:=allocations()
  result:=copies()
  if result!=0 {return result}
  if allocations()!=before || live()!=0 {return 23}
  left:=controls(true) else {return 24}
  right:=controls(false) else {return 25}
  if left!=0 {return left}; if right!=0 {return right}
  a:=containers(); if a!=0 {return a}
  b:=methods(); if b!=0 {return b}
  cleaned:=cleanup(); if cleaned!=0 {return cleaned}
  if live()!=0 {return 28}
  if allocations()!=unsafe {align_rt_free_count()} {return 29}
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
        &objects
            .iter()
            .map(|path| path.as_path())
            .collect::<Vec<_>>(),
        &binary,
        &libraries,
        Profile::Release,
    )
    .unwrap();
    // The parent-owned fixture bounds compilation and this entire descendant process group.
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(
        output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
}

#[test]
fn rng_records_execute_and_replay() {
    owned_fixture::run("rng_records_execute_and_replay", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, MAIN).unwrap();
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, entry.to_str().unwrap(), MAIN);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[program], "whole");
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), MAIN);
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
                entry.to_str().unwrap(),
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
                .map(|index| built.materialize(index).unwrap().clone())
                .collect::<Vec<_>>();
            if round == 1 {
                execute(stage, &programs, "cache-hit");
            }
        }
    });
}

#[test]
fn rng_record_placement_and_authority_refusals() {
    let prefix = "import std.rand\nState {random:rng}\nfn advance(borrow mut value:rng)->i64 = value.next()\nfn pair(borrow mut left:rng, borrow mut right:rng) {}\n";
    for (name, body, expected) in [
        (
            "immutable",
            "fn main(){state:=State{random:rand.seed_with(42)};advance(state.random)}",
            "must be rooted in mutable storage",
        ),
        (
            "shared",
            "fn use(borrow state:State){advance(state.random)}\nfn main(){}",
            "must be rooted in mutable storage",
        ),
        (
            "overlap",
            "fn main(){mut state:=State{random:rand.seed_with(42)};pair(state.random,state.random)}",
            "aliases argument",
        ),
        (
            "ancestor",
            "fn both(borrow mut state:State,borrow mut random:rng){}\nfn main(){mut state:=State{random:rand.seed_with(42)};both(state,state.random)}",
            "aliases argument",
        ),
        (
            "intrinsic",
            "fn main(){mut state:=State{random:rand.seed_with(42)};state.random.next()}",
            "needs a `mut` rng local",
        ),
        (
            "direct-array",
            "fn main(){values:=[rand.seed_with(42)]}",
            "array element must be a scalar",
        ),
        (
            "direct-slice",
            "fn use(values:slice<rng>){}\nfn main(){}",
            "slice element must be a scalar",
        ),
        (
            "direct-option",
            "fn main(){value:Option<rng> := Some(rand.seed_with(42))}",
            "Option payload must be a scalar",
        ),
        (
            "c-layout",
            "layout(C) Native {random:rng}\nfn main(){}",
            "a `layout(C)` struct field must be an integer",
        ),
        (
            "ffi",
            "extern \"C\" fn use(value:State)\nfn main(){}",
            "an extern struct parameter must be `layout(C)`",
        ),
        (
            "soa",
            "fn main(){arena{values:=[State{random:rand.seed_with(42)}].to_soa()}}",
            "'to_soa' needs a non-empty struct",
        ),
        (
            "json",
            "import core.json\nfn main(){state:=State{random:rand.seed_with(42)};encoded:=json.encode(state)}",
            "'json.encode' field 'random' has unsupported type rng",
        ),
        (
            "region",
            "fn keep<T:RegionPlain>(value:T){}\nfn main(){keep(State{random:rand.seed_with(42)})}",
            "does not satisfy the `RegionPlain` bound",
        ),
        (
            "builder",
            "fn main(){mut values:array_builder<State> := array_builder()}",
            "field 'random' has excluded type rng",
        ),
    ] {
        let source = if name == "json" {
            format!(
                "import core.json\n{prefix}{}\n",
                body.trim_start_matches("import core.json\n")
            )
        } else {
            format!("{prefix}{body}\n")
        };
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, "rng-refusal.align", &source);
        let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
        assert!(
            checked.diags.has_errors() && !diagnostics.contains("internal error"),
            "{name}: {diagnostics}"
        );
        assert!(
            diagnostics.contains(expected) && !diagnostics.contains("expected `fn`"),
            "{name}: {diagnostics}"
        );
    }
}

#[test]
fn rng_record_guide_recipes_check_and_lower() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for language in ["", "ja/"] {
        let guide = std::fs::read_to_string(
            root.join(format!("docs/guide/{language}14-std-encoding-rand-cli.md")),
        )?;
        let recipe = guide
            .split("```align\n")
            .skip(1)
            .filter_map(|block| block.split_once("```"))
            .map(|(source, _)| source)
            .find(|source| source.contains("State { random: rng }"))
            .ok_or("RNG record recipe")?;
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, "rng-guide.align", recipe);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        for per_unit in [false, true] {
            assert!(
                align_mir::lower_program_checked(&checked.hir, per_unit, Some(&sources)).is_ok()
            );
        }
    }
    Ok(())
}
