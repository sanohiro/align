//! Plan172: field assignment publishes one complete RHS after eager evaluation.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{check, BuildTarget, Profile};
use align_sema::hir;
use align_span::SourceMap;
use std::path::Path;

fn assert_check(source: &str, valid: bool) {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "field-publication.align", source);
    let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
    assert_eq!(
        !checked.diags.has_errors(),
        valid,
        "{diagnostics}\n{source}"
    );
    if !valid {
        assert!(
            diagnostics.contains("moved value") || diagnostics.contains("snapshot"),
            "{diagnostics}"
        );
    }
}

#[test]
fn field_target_liveness_after_rhs() {
    for (target, result) in [
        ("value.n", "consume(value)"),
        ("value.inner.n", "consume(value)"),
        ("value.inner", "Inner {n: consume(value)}"),
        ("value.text", "{ consume(value); \"fresh\".clone() }"),
        ("value.values", "[consume(value)]"),
    ] {
        for nested in [false, true] {
            let assignment = format!("{target} = {result}");
            let body = if nested {
                format!("unused := {{ {assignment} }}")
            } else {
                assignment
            };
            assert_check(&format!("Inner {{ n: i64 }}\nState {{ text: string, n: i64, inner: Inner, values: [i64; 1] }}\nfn consume(value: State) -> i64 = 0\nfn main() {{ mut value := State {{text: \"old\".clone(), n: 1, inner: Inner {{n: 2}}, values: [3]}}; {body} }}"), false);
        }
    }
    for rhs in [
        "consume(value)",
        "{ consume(value) }",
        "if flag { consume(value) } else { 0 }",
        "match (if flag {1} else {0}) {1 => consume(value), _ => 0}",
        "loop { break consume(value) }",
        "maybe(value) else 0",
        "fallible(value)?",
        "fallible(value).map_err(fn error: i64 { error })?",
    ] {
        for nested in [false, true] {
            let assignment = format!("value.n = {rhs}");
            let assignment = if nested {
                format!("unused := {{ {assignment} }}")
            } else {
                assignment
            };
            assert_check(&format!("State {{ text: string, n: i64 }}\nfn consume(value: State) -> i64 = 0\nfn maybe(value: State) -> Option<i64> = Some(0)\nfn fallible(value: State) -> Result<i64, i64> = Ok(0)\nfn run(flag: bool) -> Result<(), i64> {{ mut value := State {{text: \"old\".clone(), n: 1}}; {assignment}; return Ok(()) }}\nfn main() {{}}"), false);
        }
    }
}

#[test]
fn retained_source_rebinding_stays_rejected() {
    for first in ["source", "{ source }", "unsafe { source }"] {
        for later in ["source = \"rebound\".clone(); 5", "take(source); 5"] {
            let source = format!("Inner {{text: string, n: i64}}\nOuter {{inner: Inner}}\nfn take(value: string) {{}}\nfn main() {{ mut value := Outer {{inner: Inner {{text: \"old\".clone(), n: 0}}}}; mut source := \"fresh\".clone(); value.inner = Inner {{text: {first}, n: {{ {later} }} }} }}");
            assert_check(&source, false);
        }
    }
}

const MODEL: &str = r#"module model
pub Inner { text: string, n: i64 }
pub Outer { inner: Inner, other: string }
pub fn make(number: i64) -> Inner = Inner {text: "old".clone(), n: number}
pub fn fresh() -> Outer = Outer {inner: make(1), other: "keep".clone()}
pub fn identity<T>(value: T) -> T = value
pub fn guard(fail: bool) -> Result<i64, i64> { if fail { return Err(7) }; return Ok(5) }
pub fn map_error(error: i64) -> i64 = error
"#;

const VALID: &str = r#"module main
import model
CopyInner { a: i64, b: i64 }
Shell { text: string, inner: CopyInner }
Pair { left: string, right: string }
Pairs { pair: Pair }
Many { first: string, values: [string; 2], n: i64 }
ManyHolder { inner: Many }
Wrapped { optional: Option<string>, n: i64 }
WrappedHolder { inner: Wrapped }
ArenaInner { values: array<i64>, n: i64 }
ArenaHolder { inner: ArenaInner }
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn live() -> i64 = unsafe { align_rt_requested_live_bytes() }
fn fresh_shell() -> Shell = Shell {text: "shell".clone(), inner: CopyInner {a: 42, b: 43}}
fn consume_shell(value: Shell) -> i64 = 3
fn text_is(value: string, expected: str) -> bool = value == expected
fn copy_publication() -> i32 {
  mut root := fresh_shell()
  root.inner = CopyInner {a: 3, b: root.inner.a}
  if root.inner.a != 3 || root.inner.b != 42 { return 1 }
  unused := { root.inner = CopyInner {a: consume_shell(root), b: { root = fresh_shell(); 7 }} }
  if root.inner.a != 3 || root.inner.b != 7 { return 2 }
  root.inner = CopyInner {a: 9, b: { root = fresh_shell(); 11 }}
  if root.inner.a != 9 || root.inner.b != 11 { return 3 }
  return 0
}
fn source_observation() -> i32 {
  mut root := model.fresh()
  source := "fresh".clone()
  root.inner = model.Inner {text: source, n: source.len()}
  if root.inner.text != "fresh" || root.inner.n != 5 || live() != 9 { return 4 }
  root.inner = model.Inner {text: "fresh".clone(), n: {root = model.fresh(); 7}}
  if root.inner.text != "fresh" || root.inner.n != 7 || live() != 9 { return 5 }
  return 0
}
fn source_clone() -> i32 {
  mut root := Pairs {pair: Pair {left: "a".clone(), right: "b".clone()}}
  source := "fresh".clone()
  root.pair = Pair {left: source, right: source.clone()}
  if root.pair.left != "fresh" || root.pair.right != "fresh" || live() != 10 { return 6 }
  return 0
}
fn source_return(stop: bool) -> string {
  mut root := model.fresh()
  source := "fresh".clone()
  root.inner = model.Inner {text: source, n: if stop { return source } else { 5 }}
  return root.inner.text.clone()
}
fn retained_sources() -> i32 {
  if !text_is(source_return(true), "fresh") || !text_is(source_return(false), "fresh") { return 7 }
  mut root := model.fresh()
  source := "fresh".clone()
  output := loop { root.inner = model.Inner {text: source, n: {break source; 5}} }
  if root.inner.text != "old" || root.inner.n != 1 || !text_is(output, "fresh") { return 8 }
  return 0
}
fn partial(fail: bool) -> Result<(), i64> {
  mut root := model.fresh()
  root.inner = model.Inner {text: "fresh".clone(), n: model.guard(fail).map_err(model.map_error)?}
  if root.inner.text != "fresh" || root.inner.n != 5 || live() != 9 { return Err(9) }
  return Ok(())
}
fn partial_break(stop: bool) -> i32 {
  mut root := model.fresh()
  loop {
    root.inner = model.Inner {text: "fresh".clone(), n: if stop {break} else {5}}
    break
  }
  expected := if stop {"old"} else {"fresh"}
  if root.inner.text != expected || root.other != "keep" { return 10 }
  return 0
}
fn replace_borrowed(borrow mut root: model.Outer, stop: bool) {
  root.inner = model.Inner {text: "fresh".clone(), n: if stop { return } else {5}}
}
fn borrowed_and_controls() -> i32 {
  mut root := model.fresh()
  replace_borrowed(root, true)
  if root.inner.text != "old" || live() != 7 { return 11 }
  replace_borrowed(root, false)
  if root.inner.text != "fresh" || live() != 9 { return 12 }
  root.inner = if true { model.make(3) } else { model.make(4) }
  root.inner = match root.inner.n {3 => model.make(5), _ => model.make(6)}
  root.inner = loop { break model.identity(model.make(7)) }
  root.inner = Some(model.make(8)) else model.make(9)
  if root.inner.n != 8 || live() != 7 { return 13 }
  return 0
}
fn sibling_and_self() -> i32 {
  mut pair := Pair {left: "old".clone(), right: "fresh".clone()}
  pair.left = pair.left
  unused := { pair.left = pair.right }
  if pair.left != "fresh" || live() != 5 { return 14 }
  return 0
}
fn nested_arrays(stop: bool) -> i32 {
  mut root := ManyHolder {inner: Many {first: "a".clone(), values: ["b".clone(), "c".clone()], n: 0}}
  loop {
    root.inner = Many {first: "fresh".clone(), values: ["x".clone(), "yz".clone()], n: if stop { break } else {5}}
    break
  }
  if stop {
    if root.inner.first != "a" || root.inner.values[1] != "c" || live() != 3 { return 15 }
  } else {
    if root.inner.first != "fresh" || root.inner.values[1] != "yz" || live() != 8 { return 16 }
  }
  return 0
}
fn wrapped_cleanup(stop: bool) -> i32 {
  mut root := WrappedHolder {inner: Wrapped {optional: Some("old".clone()), n: 0}}
  source := "fresh".clone()
  loop {
    root.inner = Wrapped {optional: Some(source), n: if stop { break } else {5}}
    break
  }
  if !stop && root.inner.n != 5 {return 17}
  return 0
}
fn arena_control() -> i32 {
  arena {
    mut allocated := ArenaHolder {inner: ArenaInner {values: [1, 2].to_array(), n: 1}}
    allocated.inner = ArenaInner {values: [3, 4, 5].to_array(), n: allocated.inner.values.len()}
    if allocated.inner.values[0] != 3 || allocated.inner.values.len() != 3 || allocated.inner.n != 2 { return 20 }
    mut root := model.Outer {inner: model.Inner {text: "old".clone(), n: 1}, other: "keep".clone()}
    root.inner = model.Inner {text: "fresh".clone(), n: 5}
    if root.inner.text != "fresh" || root.other != "keep" { return 19 }
  }
  return 0
}
fn exercise() -> i32 {
  first := copy_publication(); if first != 0 { return first }; if live() != 0 {return 31}
  second := source_observation(); if second != 0 {return second}; if live() != 0 {return 32}
  third := source_clone(); if third != 0 {return third}; if live() != 0 {return 33}
  fourth := retained_sources(); if fourth != 0 {return fourth}; if live() != 0 {return 34}
  match partial(true) {Err(error) => {if error != 7 {return 35}}, Ok(_) => {return 35}}
  match partial(false) {Ok(_) => {}, _ => {return 36}}
  if live() != 0 {return 37}
  fifth := partial_break(true); if fifth != 0 {return fifth}
  sixth := partial_break(false); if sixth != 0 {return sixth}; if live() != 0 {return 38}
  seventh := borrowed_and_controls(); if seventh != 0 {return seventh}; if live() != 0 {return 39}
  eighth := sibling_and_self(); if eighth != 0 {return eighth}; if live() != 0 {return 40}
  ninth := nested_arrays(true); if ninth != 0 {return ninth}
  tenth := nested_arrays(false); if tenth != 0 {return tenth}; if live() != 0 {return 41}
  eleventh := wrapped_cleanup(true); if eleventh != 0 {return eleventh}
  twelfth := wrapped_cleanup(false); if twelfth != 0 {return twelfth}; if live() != 0 {return 42}
  thirteenth := arena_control(); if thirteenth != 0 {return thirteenth}; if live() != 0 {return 43}
  return 0
}
fn main() -> i32 {
  unsafe {align_rt_requested_live_reset()}
  before_alloc := unsafe {align_rt_alloc_count()}
  before_free := unsafe {align_rt_free_count()}
  status := exercise()
  if status != 0 { return status }
  allocated := unsafe {align_rt_alloc_count()} - before_alloc
  freed := unsafe {align_rt_free_count()} - before_free
  if allocated == 0 || allocated != freed || live() != 0 { return 44 }
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
fn field_publication_lifecycle_and_replay() {
    owned_fixture::run("field_publication_lifecycle_and_replay", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, VALID).unwrap();
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, entry.to_str().unwrap(), VALID);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[program], "whole");
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), VALID);
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
                VALID,
                &cache,
                align_driver::UnitReuse::Allowed,
            );
            assert!(
                !built.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &built.diags)
            );
            assert_eq!(built.units.len(), 2);
            assert!(built
                .units
                .iter()
                .all(|unit| unit.frontend.as_ref().unwrap().hit == (round == 1)));
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
fn forged_field_assignment_replay_rejects() {
    for target in ["value.n", "value.inner.n"] {
        let source = format!("Inner {{n: i64}}\nState {{text: string, n: i64, inner: Inner}}\nfn consume(value: State) -> i64 = 0\nfn run() {{ mut value := State {{text: \"old\".clone(), n: 1, inner: Inner {{n: 2}}}}; {target} = 3; consume(value); return }}\nfn main() {{}}");
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, "forged-field.align", &source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        assert!(align_sema::checked_hir_body_facts_are_valid(&checked.hir));
        let mut bad = checked.hir.clone();
        let function = bad
            .fns
            .iter_mut()
            .find(|function| function.name == "run")
            .unwrap();
        let hir::Stmt::Expr(call) = function.body.stmts.remove(2) else {
            panic!("fixture lost call")
        };
        let hir::Stmt::AssignField { value, .. } = &mut function.body.stmts[1] else {
            panic!("fixture lost field assignment")
        };
        *value = call;
        assert!(!align_sema::checked_hir_body_facts_are_valid(&bad));
        for per_unit in [false, true] {
            assert!(align_mir::lower_program_checked(&bad, per_unit, Some(&sources)).is_err());
        }
    }
}
