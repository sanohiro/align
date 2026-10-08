//! Request 135: inline owned siblings survive an exclusive call through another field.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{build_per_unit, check, emit_object_file, link_objects, lower_to_mir, BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
pub Session {
  label: string,
  bytes: buffer,
  graph_counts: [i64; 5],
  graph_widths: [i64; 5],
  graph_phases: [i64; 5],
  graph_emits: [i64; 5],
  n: i64,
}
pub fn create(label: string, bytes: buffer) -> Session {
  return Session {
    label: label,
    bytes: bytes,
    graph_counts: [1, 2, 3, 4, 5], graph_widths: [1, 2, 3, 4, 5],
    graph_phases: [1, 2, 3, 4, 5], graph_emits: [1, 2, 3, 4, 5],
    n: 0,
  }
}
"#;
const OPERATIONS: &str = r#"module operations
import model
pub fn identity<T>(value: T) -> T = value
fn read(borrow values: [i64; 5]) -> i64 = values[0]
fn write(borrow mut bytes: slice<u8>) { bytes.set_u8(0, 1) }
pub fn advance(borrow mut session: model.Session, stop: bool) {
  if stop { return }
  before := read(session.graph_counts)
  if session.n == 0 {
    { mut bytes := session.bytes.bytes(); write(bytes) }
  } else {
    mut bytes := session.bytes.bytes()
    write(bytes)
  }
  session.n = session.n + identity(before)
  if read(session.graph_counts) != before { session.n = -100 }
  if session.graph_widths[4] != 5 || session.graph_phases[2] != 3 || session.graph_emits[1] != 2 {
    session.n = -200
  }
}
"#;
const MAIN: &str = r#"module main
import model
import operations
fn main() -> i32 {
  mut session := model.create("cuda".clone(), buffer.filled(1, 0))
  mut count := 0
  loop {
    if count == 2 { break }
    operations.advance(session, false)
    count = count + 1
  }
  operations.advance(session, true)
  if session.n != 2 || session.bytes.bytes().u8(0) != 1 || session.label != "cuda" { return 1 }
  if session.graph_counts[4] != 5 { return 2 }
  return 0
}
"#;

fn execute(stage: &Path, programs: &[align_mir::Program], label: &str) {
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{index}.o"));
        emit_object_file(program, &object, BuildTarget::Baseline, Profile::Release, &[], false)
            .unwrap_or_else(|error| panic!("{error}\n{}", align_mir::print::program_to_string(program)));
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) { libraries.push(library.clone()); }
        }
    }
    let executable = stage.join(format!("run-{label}"));
    link_objects(&align_driver::CDriver::default(), &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        &executable, &libraries, Profile::Release).unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{label}: {output:?}");
    assert!(output.stdout.is_empty() && output.stderr.is_empty(), "{output:?}");
}

#[test]
fn exact_request_and_dynamic_control_build_in_both_modes() {
    owned_fixture::run("exact_request_and_dynamic_control_build_in_both_modes", |stage| {
        let fixed = "module main\nSession { bytes: buffer, fixed: [i64; 5], n: i64 }\nfn write(borrow mut bytes: slice<u8>) { bytes.set_u8(0, 1) }\nfn advance(borrow mut session: Session) { { mut bytes := session.bytes.bytes(); write(bytes) }; session.n = session.n + 1 }\npub fn main() {}\n";
        let nested = fixed.replace("Session { bytes: buffer, fixed: [i64; 5], n: i64 }",
            "Row { value: i64 }\nNested { values: [i64; 5], rows: [Row; 2] }\nSession { fixed: Nested, bytes: buffer, n: i64 }");
        for (index, source) in [fixed.to_owned(), fixed.replace("[i64; 5]", "array<i64>"), nested].into_iter().enumerate() {
            let entry = stage.join("main.align");
            std::fs::write(&entry, &source).unwrap();
            for per_unit in [false, true] {
                let mut sources = SourceMap::new();
                let programs = if per_unit {
                    let built = build_per_unit(&mut sources, entry.to_str().unwrap(), &source);
                    assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &built.diags));
                    built.units.into_iter().map(|unit| unit.mir).collect::<Vec<_>>()
                } else {
                    let checked = check(&mut sources, entry.to_str().unwrap(), &source);
                    assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &checked.diags));
                    vec![lower_to_mir(&checked.hir)]
                };
                execute(stage, &programs, &format!("exact-{index}-{per_unit}"));
            }
        }
    });
}

#[test]
fn imported_fixed_siblings_check_execute_and_replay() {
    owned_fixture::run("imported_fixed_siblings_check_execute_and_replay", |stage| {
        assert!(align_driver::backend_available(), "native owner requires LLVM");
        for (name, source) in [("model.align", MODEL), ("operations.align", OPERATIONS), ("main.align", MAIN)] {
            std::fs::write(stage.join(name), source).unwrap();
        }
        let entry = stage.join("main.align");
        let entry = entry.to_str().unwrap();
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, entry, MAIN);
        assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &checked.diags));
        execute(stage, &[lower_to_mir(&checked.hir)], "whole");
        let mut sources = SourceMap::new();
        let built = build_per_unit(&mut sources, entry, MAIN);
        assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &built.diags));
        execute(stage, &built.units.into_iter().map(|unit| unit.mir).collect::<Vec<_>>(), "units");
        let context = align_driver::CacheContext::at(stage.join("cache"));
        let mut snapshots = Vec::new();
        for round in 0..2 {
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(&mut sources, entry, MAIN, &context, align_driver::UnitReuse::Allowed);
            assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &built.diags));
            assert_eq!(built.units.len(), 3);
            for unit in &built.units { assert_eq!(unit.frontend.as_ref().unwrap().hit, round == 1, "{}", unit.unit); }
            let programs = (0..built.units.len()).map(|i| built.materialize(i).unwrap().clone()).collect::<Vec<_>>();
            let snapshot = programs.iter().map(align_mir::print::program_to_string).collect::<Vec<_>>();
            if round == 0 { snapshots = snapshot; } else {
                assert_eq!(snapshots, snapshot);
                execute(stage, &programs, "replay");
            }
        }
    });
}

#[test]
fn overlapping_and_contained_views_still_reject() {
    owned_fixture::run("overlapping_and_contained_views_still_reject", |stage| {
        for (name, source) in [
            ("buffer-alias", "Session { bytes: buffer, fixed: [i64; 5] }\nfn write(borrow mut bytes: slice<u8>) { bytes.set_u8(0, 1) }\nfn bad(borrow mut s: Session) -> i32 { old := s.bytes.bytes(); { mut bytes := s.bytes.bytes(); write(bytes) }; return old.u8(0) as i32 }\nfn main() {}\n"),
            ("fixed-alias", "Session { bytes: buffer, fixed: [i64; 5] }\nfn write(borrow old: slice<i64>, borrow mut values: slice<i64>) { values[0] = old[0] }\nfn bad(borrow mut s: Session) -> i64 { old := s.fixed[..]; mut values := s.fixed[..]; write(old, values); return old[0] }\nfn main() {}\n"),
            ("contained-view", "Session { owners: array<string>, fixed: [str; 1] }\nfn bad(borrow mut s: Session) -> i64 { s.owners.truncate(0); return s.fixed[0].len() }\nfn main() {}\n"),
            ("escaped-view", "fn bad() -> slice<i64> { values := [1, 2]; return values[..] }\nfn main() {}\n"),
        ] {
            let entry = stage.join(format!("{name}.align"));
            std::fs::write(&entry, source).unwrap();
            for per_unit in [false, true] {
                let mut sources = SourceMap::new();
                let diagnostics = if per_unit {
                    build_per_unit(&mut sources, entry.to_str().unwrap(), source).diags
                } else { check(&mut sources, entry.to_str().unwrap(), source).diags };
                let text = align_driver::format_diagnostics(&sources, &diagnostics);
                assert!(diagnostics.has_errors(), "{name}/{per_unit}: expected rejection");
                assert!(text.contains("invalidated") || text.contains("outlive") || text.contains("escape") || text.contains("borrow"), "{name}: {text}");
                assert!(!text.contains("parse error") && !text.contains("unknown"), "unrelated rejection: {text}");
            }
        }
    });
}
