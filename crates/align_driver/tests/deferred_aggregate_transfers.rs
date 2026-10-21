//! Plan173: one successful action commits its evaluated source transfers.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile, check};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
pub fn identity<T>(value:T)->T = value
pub fn guard(stop:bool)->Result<i64,i64> {if stop {return Err(7)};return Ok(5)}
pub fn map(error:i64)->i64 = error
"#;

fn native_program() -> String {
    let mut source = String::from(
        r#"module main
import model
import core.json
Record {text:string,n:i64}
Pair {first:string,second:string}
JsonRecord {text:string}
JsonEnvelope {value:Option<JsonRecord>,digest:Option<string>}
Tagged {Text(string), Empty}
Choice {A,B}
Fixed {values:[string;1],n:i64}
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
fn live()->i64 = unsafe {align_rt_requested_live_bytes()}
fn consume(value:Option<string>,n:i64)->i32 {if n!=5 {return 1};return 0}
fn consume_text(value:string,n:i64)->i32 {if n!=5 {return 1};return 0}
"#,
    );
    let mut names = Vec::new();
    for (index, (ty, value)) in [
        ("string", "text"),
        ("Option<string>", "Some(text)"),
        ("Result<string,i64>", "Ok(text)"),
        ("Result<i64,string>", "Err(text)"),
        ("Tagged", "Tagged.Text(text)"),
        ("[string;1]", "[text]"),
        ("Record", "Record{text:text,n:5}"),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("construct_{index}");
        source.push_str(&format!("Wrap{index} {{value:{ty},n:i64}}\nfn {name}()->i32 {{text:=\"fresh\".clone(); value:=Wrap{index}{{value:{value},n:text.len()}}; if value.n!=5 {{return 1}}; return 0}}\n"));
        names.push(name);
    }
    for (index, value) in [
        "{text}",
        "unsafe {text}",
        "arena {text}",
        "task_group {text}",
        "if select {text} else {other}",
        "if select {if false {other} else {text}} else {other}",
        "match number {1 => text, _ => other}",
        "match letter {'a' => text, _ => other}",
        "match label {\"a\" => text, _ => other}",
        "match maybe {Some(x) => text, None => other}",
        "match result {Ok(x) => text, Err(error) => other}",
        "match choice {A => text, B => other}",
        "none else (if select {text} else {other})",
        "if select {text} else {\"fresh\".clone()}",
        "none else text",
        "fallible else text",
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("selected_{index}");
        source.push_str(&format!("fn {name}()->i32 {{mut iteration:=0; loop {{if iteration==4 {{break}}; select:=iteration%2==0; number:=iteration%2; result:Result<i64,i64>:=if select {{Ok(number)}} else {{Err(7)}};maybe:Option<i64>:=if select {{Some(number)}} else {{None}};letter:=if select {{'a'}} else {{'b'}};label:=if select {{\"a\"}} else {{\"b\"}};choice:=if select {{Choice.A}} else {{Choice.B}}; text:=\"fresh\".clone(); other:=\"other\".clone(); none:Option<string>:=if select {{None}} else {{Some(\"child\".clone())}}; fallible:Result<string,i64>:=if select {{Err(7)}} else {{Ok(\"child\".clone())}}; value:=Record{{text:{value},n:text.len()+other.len()}};if value.n!=10 {{return 2}};iteration=iteration+1}};return 0}}\n"));
        names.push(name);
    }
    source.push_str(r#"
fn tick(borrow mut count:i64)->string {count=count+1;return "tick".clone()}
fn effects()->i32 {mut count:=0;value:=Some(if true {tick(count)} else {tick(count)});a:=Some(arena {tick(count)});b:=Some(arena region {tick(count)});c:=Some(task_group {tick(count)});d:=Some(unsafe {tick(count)});if count!=5 {return 15};return 0}
fn inner_loop()->i32 {value:=Record{text:"fresh".clone(),n:loop {break 5}};if value.text!="fresh" || value.n!=5 {return 16};return 0}
fn fixed_return()->string {text:="fresh".clone();value:=Fixed{values:[text],n:{return text}};return "bad".clone()}
fn fixed_return_observation()->i32 {value:=fixed_return();if value!="fresh" {return 17};return 0}
fn named_scope()->i32 {text:="fresh".clone();value:=Record{text:arena region {text},n:text.len()};if value.n!=5 {return 14};return 0}
fn call_direct()->i32 {text:="fresh".clone();return consume(Some(text),text.len())}
fn call_indirect()->i32 {function:=consume_text;text:="fresh".clone();return function(text,text.len())}
fn tuple_construct()->i32 {text:="fresh".clone(); value:=(text,text.len());if value.1!=5 {return 12};return 0}
fn generic_record()->i32 {text:="fresh".clone(); value:=model.identity(Record{text:text,n:text.len()});if value.n!=5 {return 13};return 0}
fn clone_source()->i32 {text:="fresh".clone();value:=Pair{first:text,second:text.clone()};if value.first!="fresh" || value.second!="fresh" {return 3};return 0}
// R86: structural Some(value) retains its source until the enclosing action commits.
fn json_pending_source()->i32 {value:=JsonRecord{text:"owned".clone()};packet:=JsonEnvelope{value:Some(value),digest:Some(value.text.clone())};encoded:=json.encode(packet) else {return 18};if encoded!="{\"value\":{\"text\":\"owned\"},\"digest\":\"owned\"}" {return 19};return 0}
// Written initializer order still captures the clone before the reordered owner field.
fn json_reordered_source()->i32 {value:=JsonRecord{text:"owned".clone()};packet:=JsonEnvelope{digest:Some(value.text.clone()),value:Some(value)};encoded:=json.encode(packet) else {return 18};if encoded!="{\"value\":{\"text\":\"owned\"},\"digest\":\"owned\"}" {return 19};return 0}
fn projected()->i32 {root:=Pair{first:"fresh".clone(),second:"other".clone()}; value:=Record{text:root.first,n:root.first.len()};if value.n!=5 {return 4};return 0}
fn tuple_projected()->i32 {root:=("fresh".clone(),0); value:=Record{text:root.0,n:root.0.len()};if value.n!=5 {return 5};return 0}
fn retained_return(stop:bool)->string {text:="fresh".clone();value:=Record{text:if stop {text} else {"other".clone()},n:{if stop {return text};5}};return "ok".clone()}
fn retained_break()->i32 {text:="fresh".clone();value:=loop {unused:=Record{text:text,n:{break text}}};if value!="fresh" {return 6};return 0}
fn fresh_return()->i32 {value:=Record{text:"fresh".clone(),n:{return 0}};return 7}
fn fresh_break()->i32 {value:=loop {unused:=Record{text:"fresh".clone(),n:{break 0}}};return value}
fn partial_try(stop:bool)->Result<i64,i64> {value:=Record{text:"fresh".clone(),n:model.guard(stop)?};return Ok(value.n)}
fn exits()->i32 {a:=retained_return(true);b:=retained_return(false);if a!="fresh" || b!="ok" {return 8}; match partial_try(true) {Ok(x)=>{return 9},Err(e)=>{}};match partial_try(false) {Ok(x)=>{if x!=5 {return 10}},Err(e)=>{return 11}};return 0}
"#);
    names.extend(
        [
            "effects",
            "inner_loop",
            "fixed_return_observation",
            "named_scope",
            "call_direct",
            "call_indirect",
            "tuple_construct",
            "generic_record",
            "clone_source",
            "projected",
            "tuple_projected",
            "retained_break",
            "fresh_return",
            "fresh_break",
            "exits",
        ]
        .into_iter()
        .map(String::from),
    );
    source.push_str("fn main()->i32 {unsafe {align_rt_requested_live_reset()};\n");
    for (index, name) in names.iter().enumerate() {
        source.push_str(&format!("{{before:=unsafe {{align_rt_alloc_count()}}; before_free:=unsafe {{align_rt_free_count()}};status:={name}();if status!=0 {{return status}};allocated:=unsafe {{align_rt_alloc_count()}}-before;freed:=unsafe {{align_rt_free_count()}}-before_free;if allocated<=0 || allocated!=freed || live()!=0 {{return {}}}}}\n",40+index));
    }
    // As in json_field_encode, realloc-grown output is included in live bytes and
    // free_count but not alloc_count. Keep exact event parity for the owners above.
    for name in ["json_pending_source", "json_reordered_source"] {
        source.push_str(&format!("{{before:=unsafe {{align_rt_alloc_count()}};before_free:=unsafe {{align_rt_free_count()}};status:={name}();if status!=0 {{return status}};allocated:=unsafe {{align_rt_alloc_count()}}-before;freed:=unsafe {{align_rt_free_count()}}-before_free;if allocated<=0 || freed<allocated || live()!=0 {{return 20}}}}\n"));
    }
    source.push_str("return 0}\n");
    source
}

#[test]
fn optional_record_completed_moves_reject_later_reads() {
    // A completed block child and an earlier statement have already transferred value.
    for tail in [
        "packet:=Envelope{value:{Some(value)},digest:Some(value.text.clone())}",
        "packet:=Envelope{value:Some(value),digest:None}; unused:=value.text.clone()",
        "packet:=Envelope{value:Some(value),digest:Some(value.text.clone())}; unused:=value.text.clone()",
    ] {
        let input = format!(
            "Record {{text:string}}\nEnvelope {{value:Option<Record>,digest:Option<string>}}\nfn main() {{value:=Record{{text:\"owned\".clone()}};{tail}}}"
        );
        for per_unit in [false, true] {
            let mut sources = SourceMap::new();
            let diags = if per_unit {
                align_driver::check_per_unit(&mut sources, "completed-move.align", &input).diags
            } else {
                check(&mut sources, "completed-move.align", &input).diags
            };
            assert!(diags.has_errors(), "accepted (per_unit={per_unit}) {input}");
            let diagnostics = align_driver::format_diagnostics(&sources, &diags);
            assert!(diagnostics.contains("moved"), "{diagnostics}");
        }
    }
}

#[test]
fn deferred_fallback_moves_and_duplicates_reject() {
    for result in [false, true] {
        let declaration = if result {
            "absent:Result<string,i64>:=Err(7)"
        } else {
            "absent:Option<string>:=None"
        };
        for tail in [
            "value:=Pair{first:absent else text,second:text}",
            "value:=Record{text:absent else text,n:0}; unused:=text.len()",
        ] {
            let input = format!(
                "Record {{text:string,n:i64}}\nPair {{first:string,second:string}}\nfn main() {{text:=\"fresh\".clone();{declaration};{tail}}}"
            );
            let mut sources = SourceMap::new();
            let checked = check(&mut sources, "fallback.align", &input);
            assert!(checked.diags.has_errors(), "accepted {input}");
            let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
            assert!(
                diagnostics.contains("moved") || diagnostics.contains("owned source"),
                "{diagnostics}"
            );
        }
    }
}

#[test]
fn forged_fallback_transfer_rejects_in_checked_replay() {
    for container in ["Option<string>:=None", "Result<string,i64>:=Err(7)"] {
        let input = format!(
            "Record {{text:string,n:i64}}\nfn take(value:string) {{}}\nfn main() {{text:=\"fresh\".clone();other:=\"other\".clone();absent:{container};value:=Record{{text:absent else text,n:0}};take(other);return}}"
        );
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, "forged-fallback.align", &input);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let mut invalid = checked.hir.clone();
        let main = invalid
            .fns
            .iter_mut()
            .find(|function| function.name == "main")
            .unwrap();
        let text = u32::try_from(
            main.locals
                .iter()
                .position(|local| local.name == "text")
                .unwrap(),
        )
        .unwrap();
        let call = main
            .body
            .stmts
            .iter_mut()
            .find_map(|statement| match statement {
                align_sema::hir::Stmt::Expr(call)
                    if matches!(call.kind, align_sema::hir::ExprKind::Call { .. }) =>
                {
                    Some(call)
                }
                _ => None,
            })
            .expect("call statement");
        let align_sema::hir::ExprKind::Call { args, .. } = &mut call.kind else {
            panic!("call expression")
        };
        args[0].kind = align_sema::hir::ExprKind::Local(text);
        assert!(!align_sema::checked_hir_body_facts_are_valid(&invalid));
        for per_unit in [false, true] {
            assert!(align_mir::lower_program_checked(&invalid, per_unit, Some(&sources)).is_err());
        }
    }
}

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
fn deferred_aggregate_lifecycle_and_replay() {
    let valid = native_program();
    owned_fixture::run("deferred_aggregate_lifecycle_and_replay", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, &valid).unwrap();
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, entry.to_str().unwrap(), &valid);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[program], "whole");
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), &valid);
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
                &valid,
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
