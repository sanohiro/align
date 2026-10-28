//! R131: imported owned records retain complete producers across scalar CFGs.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
pub Source { first: i64, second: i64, third: i64, text: string }
pub Host { text: string, values: array<i64> }
pub Session {
  source: Source, work: buffer, host: Option<Host>,
  limit: i64, context: i64, last: i64,
}
pub Trace { value: i64 }

fn observe(borrow mut trace: Trace, digit: i64) -> bool {
  trace.value = trace.value * 10 + digit
  return digit > 0
}
fn guard(borrow mut trace: Trace, fail: bool) -> Result<i64, i64> {
  observe(trace, 3)
  if fail { return Err(17) }
  return Ok(7)
}
pub fn make(source: Source, host: Option<Host>, flag: bool, fail: bool,
  reordered: bool, borrow mut trace: Trace) -> Result<Session, i64> {
  if reordered {
    return Ok(Session {
      last: guard(trace, fail)?,
      context: if observe(trace, 2) && flag { 64 } else { 32 },
      limit: if observe(trace, 1) && flag { 8 } else { 4 },
      host: host, work: buffer.filled(9, 23 as u8), source: source,
    })
  }
  return Ok(Session {
    source: source, work: buffer.filled(9, 23 as u8), host: host,
    limit: if observe(trace, 1) && flag { 8 } else { 4 },
    context: if observe(trace, 2) && flag { 64 } else { 32 },
    last: guard(trace, fail)?,
  })
}
"#;

const MAIN: &str = r#"import model
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64

fn create(flag: bool, fail: bool, reordered: bool,
  borrow mut trace: model.Trace) -> Result<model.Session, i64> {
  source := model.Source { first: 1, second: 2, third: 3, text: "source".clone() }
  host: Option<model.Host> := if flag {
    Some(model.Host { text: "host".clone(), values: [11, 22].to_array() })
  } else { None }
  return model.make(source, host, flag, fail, reordered, trace)
}
fn valid(borrow session: model.Session, flag: bool) -> bool {
  if unsafe { align_rt_requested_live_bytes() } <= 0 { return false }
  if session.source.text != "source" || session.source.third != 3 { return false }
  if session.work.len() != 9 || session.work.bytes()[8] != 23 as u8 { return false }
  if session.last != 7 { return false }
  if session.limit != if flag { 8 } else { 4 } { return false }
  if session.context != if flag { 64 } else { 32 } { return false }
  return match session.host {
    Some(host) => flag && host.text == "host" && host.values[1] == 22,
    None => !flag,
  }
}
fn exercise(flag: bool, fail: bool, reordered: bool) -> i32 {
  mut trace := model.Trace { value: 0 }
  result := create(flag, fail, reordered, trace)
  expected := if reordered { if fail { 3 } else { 321 } } else { 123 }
  if trace.value != expected { return 1 }
  return match result {
    Err(error) => if fail && error == 17 { 0 } else { 2 },
    Ok(value) => {
      if fail || !valid(value, flag) { return 3 }
      mut current := value
      mut at := 0
      loop {
        if at == 4 { break }
        trace.value = 0
        replacement := create(!flag, false, !reordered, trace) else { return 4 }
        if trace.value != if reordered { 123 } else { 321 } { return 5 }
        current = replacement
        if !valid(current, !flag) { return 6 }
        at = at + 1
      }
      0
    },
  }
}
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  mut index := 0
  loop {
    if index == 8 { break }
    allocated := unsafe { align_rt_alloc_count() }
    freed := unsafe { align_rt_free_count() }
    status := exercise(index % 2 == 0, (index / 2) % 2 == 0, index >= 4)
    if status != 0 { return status }
    count := unsafe { align_rt_alloc_count() } - allocated
    if count <= 0 || count != unsafe { align_rt_free_count() } - freed { return 7 }
    if unsafe { align_rt_requested_live_bytes() } != 0 { return 8 }
    index = index + 1
  }
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
    // The parent fixture owns the process group and the compile/link/run deadline.
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(
        output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
}

#[test]
fn imported_conditional_records_preserve_values_order_and_cleanup() {
    owned_fixture::run(
        "imported_conditional_records_preserve_values_order_and_cleanup",
        |stage| {
            std::fs::write(stage.join("model.align"), MODEL).unwrap();
            let entry = stage.join("main.align");
            std::fs::write(&entry, MAIN).unwrap();
            let mut sources = SourceMap::new();
            let checked = align_driver::check(&mut sources, entry.to_str().unwrap(), MAIN);
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
        },
    );
}
