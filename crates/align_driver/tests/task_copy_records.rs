//! Plan207: independent Copy record results use the structured task slot and wait proof.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::{Path, PathBuf};

fn compile(stage: &Path, programs: &[align_mir::Program], label: &str) -> PathBuf {
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
    binary
}

const MODEL: &str = r#"module model
pub Timing { started: i64, finished: i64 }
pub Empty { values: [i64; 0] }
pub align(32) Aligned { value: i64 }
pub Choice { Number(i64), Off }
pub fn empty() -> Empty { value := Empty { values: [] }; return value }
pub fn aligned() -> Aligned = Aligned { value: 41 }
pub Wide {
  marker: u8, flag: bool, weight: f64, letter: char,
  timing: Timing, values: [i64; 8], times: [Timing; 2],
  pair: (i64, bool), optional: Option<i64>, choice: Choice, empty: [i64; 0]
}
pub fn identity<T>(value: T) -> T = value
pub fn make(n: i64) -> Timing {
  scratch := "worker-local".clone()
  return Timing { started: n, finished: n + scratch.len() }
}
pub fn attempt(n: i64) -> Result<Timing, Error> {
  if n < 0 { return Err(error(37)) }
  return Ok(identity(make(n)))
}
pub fn wide(n: i64) -> Wide = Wide {
  marker: 241, flag: true, weight: 2.5, letter: 'z',
  timing: make(n), values: [1,2,3,4,5,6,7,n], times: [make(n+1),make(n+2)],
  pair: (n+3, false), optional: Some(n+4), choice: Choice.Number(n+5), empty: []
}
pub fn wide_result(n: i64) -> Result<Wide, Error> = Ok(wide(n))
pub fn escaped(text: str) -> Wide = task_group {
  worker := spawn(fn { wide(text.len()) })
  wait()
  worker.get()
}
pub fn early(text: str) -> Result<Timing, Error> {
  task_group {
    worker := spawn(fn { attempt(text.len()) })
    wait().map_err(fn e: Error { e })?
    return Ok(worker.get())
  }
}
"#;

const MAIN: &str = r#"module main
import model
fn failed() -> Result<(), Error> {
  task_group {
    first := spawn(fn { model.attempt(-1) })
    second := spawn(fn { model.attempt(4) })
    third := spawn(fn { later_error() })
    wait()?
    unreachable_result := first.get()
    print(unreachable_result.finished)
  }
  return Ok(())
}
fn later_error() -> Result<model.Timing, Error> = Err(error(43))
pub fn main() -> Result<(), Error> {
  mut text := "captured".clone()
  mut escaped := model.escaped(text)
  early := model.early(text)?
  text = "replacement".clone()
  if early.started != 8 || early.finished != 20 { return Err(error(1)) }
  if escaped.marker != 241 || !escaped.flag || escaped.weight != 2.5 || escaped.letter != 'z' { return Err(error(2)) }
  if escaped.timing.started != 8 || escaped.timing.finished != 20 || escaped.values[7] != 8 { return Err(error(3)) }
  if escaped.times[1].started != 10 || escaped.pair.0 != 11 || escaped.pair.1 { return Err(error(4)) }
  if (escaped.optional else { 0 }) != 12 || escaped.empty.len() != 0 { return Err(error(5)) }
  if (match escaped.choice { Number(n) => n, Off => 0 }) != 13 { return Err(error(6)) }
  escaped = model.wide(17)
  if escaped.values[7] != 17 { return Err(error(7)) }
  task_group {
    first := spawn(fn { model.identity(model.make(1)) })
    second := spawn(fn { model.attempt(2) })
    empty := spawn(fn { model.empty() })
    aligned := spawn(fn { model.aligned() })
    large := spawn(fn { model.wide_result(29) })
    wait()?
    first_value := first.get()
    second_value := second.get()
    aligned_value := aligned.get()
    if first_value.started != 1 || second_value.finished != 14 || aligned_value.value != 41 { return Err(error(8)) }
    empty.get()
    empty.get()
    large_first := large.get()
    large_second := large.get()
    if large_first.values[7] != 29 || large_second.times[0].started != 30 { return Err(error(9)) }
    task_group {
      inner := spawn(fn { model.make(3) })
      wait()
      outer_value := first.get()
      inner_value := inner.get()
      if outer_value.finished != 13 || inner_value.started != 3 { return Err(error(10)) }
    }
    third := spawn(fn { if true { model.make(4) } else { model.make(5) } })
    wait()?
    old_value := first.get()
    new_value := third.get()
    if old_value.started != 1 || new_value.finished != 16 { return Err(error(11)) }
    wait()?
  }
  mut count := 0
  loop {
    if count == 3 { break }
    output := task_group {
      worker := spawn(fn { match count { 0 => model.make(10), _ => loop { break model.make(count) } } })
      wait()
      worker.get()
    }
    if output.started != (if count == 0 { 10 } else { count }) { return Err(error(12)) }
    count = count + 1
  }
  match failed() {
    Err(failure) => match failure { Code(code) => { if code != 37 { return Err(error(13)) } }, _ => { return Err(error(14)) } },
    Ok(_) => { return Err(error(15)) }
  }
  return Ok(())
}
"#;

fn execute(binary: &Path) {
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stdout.is_empty() && output.stderr.is_empty(),
        "{output:?}"
    );
}

#[test]
fn task_records_execute_and_replay() {
    owned_fixture::run("task_records_execute_and_replay", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, MAIN).unwrap();
        let mut sources = SourceMap::new();
        eprintln!("task-record owner: whole checking");
        let checked = align_driver::check(&mut sources, entry.to_str().unwrap(), MAIN);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        eprintln!("task-record owner: whole lowering");
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        eprintln!("task-record owner: whole emission");
        execute(&compile(stage, &[program], "whole"));
        eprintln!("task-record owner: whole executed");
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), MAIN);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &built.diags)
        );
        execute(&compile(
            stage,
            &built
                .units
                .into_iter()
                .map(|unit| unit.mir)
                .collect::<Vec<_>>(),
            "units",
        ));
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
                execute(&compile(stage, &programs, "cache-hit"));
            }
        }
    });
}

#[test]
fn task_records_reject_authority_and_invalid_waits() {
    owned_fixture::run("task_records_reject_authority_and_invalid_waits", |stage| {
        let mut cases = Vec::new();
        for (name, declaration, value) in [
            ("view", "Bad { value: str }", "Bad { value: \"borrowed\" }"),
            ("empty-view", "Bad { value: [str; 0] }", "Bad { value: [] }"),
            (
                "inactive-view",
                "Choice { Good, Borrowed(str) }\nBad { value: Choice }",
                "Bad { value: Choice.Good }",
            ),
            (
                "callable",
                "Bad { value: fn() -> i64 }\nfn number() -> i64 = 1",
                "Bad { value: number }",
            ),
            (
                "raw",
                "layout(C) Bad { value: raw }",
                "unsafe { Bad { value: raw.null() } }",
            ),
        ] {
            cases.push((name, format!("{declaration}\nfn main() {{ task_group {{ spawn(fn {{ {value} }}); wait() }} }}"), "self-contained data"));
        }
        for (name, body, diagnostic) in [
            ("early", "t:=spawn(fn { make() }); t.get(); wait()", "wait"),
            (
                "ignored",
                "t:=spawn(fn { attempt() }); wait(); t.get()",
                "successful",
            ),
            (
                "failed",
                "t:=spawn(fn { attempt() }); match wait() { Ok(_) => (), Err(_) => { t.get(); () } }",
                "successful",
            ),
            (
                "stale",
                "t:=spawn(fn { make() }); wait(); spawn(fn { make() }); t.get(); wait()",
                "wait",
            ),
            (
                "inner",
                "t:=spawn(fn { make() }); task_group { wait(); t.get() }; wait()",
                "wait",
            ),
        ] {
            cases.push((name, format!("Timing {{ started:i64, finished:i64 }}\nfn make()->Timing=Timing {{ started:1,finished:2 }}\nfn attempt()->Result<Timing,Error> = Ok(make())\nfn main()->Result<(),Error>{{task_group{{{body}}};return Ok(())}}"), diagnostic));
        }
        cases.push(("escaped-handle", "Timing { started:i64, finished:i64 }\nfn main() { value:=task_group { t:=spawn(fn { Timing {started:1,finished:2} }); wait(); t } }".into(), "escape"));
        for (name, source, expected) in cases {
            let entry = stage.join(format!("{name}.align"));
            std::fs::write(&entry, &source).unwrap();
            for per_unit in [false, true] {
                let mut sources = SourceMap::new();
                let diags = if per_unit {
                    align_driver::check_per_unit(&mut sources, entry.to_str().unwrap(), &source)
                        .diags
                } else {
                    align_driver::check(&mut sources, entry.to_str().unwrap(), &source).diags
                };
                let diagnostics = align_driver::format_diagnostics(&sources, &diags);
                assert!(
                    diags.has_errors() && diagnostics.contains(expected),
                    "{name}/{per_unit}: expected {expected}\n{diagnostics}"
                );
            }
        }
    });
}
