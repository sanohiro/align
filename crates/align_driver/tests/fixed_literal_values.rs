//! Fixed-array literals use ordinary inline value construction at admitted consumers.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{
    BuildTarget, Profile, backend_available, build_per_unit, check, emit_object_file, link_objects,
    lower_to_mir,
};
use align_span::SourceMap;
use std::path::Path;

const _: extern "C" fn() -> i64 = align_runtime::align_rt_alloc_count;
const _: extern "C" fn() -> i64 = align_runtime::align_rt_free_count;

// Called only inside owned_fixture::run, including every transitive native wait.
fn execute(stage: &Path, files: &[(&str, &str)], per_unit: bool) -> Option<std::process::Output> {
    for (name, source) in files {
        std::fs::write(stage.join(name), source).unwrap();
    }
    let entry = stage.join("main.align");
    let source = std::fs::read_to_string(&entry).unwrap();
    let mut sources = SourceMap::new();
    let programs = if per_unit {
        let walk = build_per_unit(&mut sources, entry.to_str().unwrap(), &source);
        assert!(
            !walk.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &walk.diags)
        );
        assert_eq!(walk.units.len(), files.len());
        walk.units
            .into_iter()
            .map(|unit| unit.mir)
            .collect::<Vec<_>>()
    } else {
        let checked = check(&mut sources, entry.to_str().unwrap(), &source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        vec![lower_to_mir(&checked.hir)]
    };
    if !backend_available() {
        return None;
    }
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{per_unit}-{index}.o"));
        emit_object_file(
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
    let executable = stage.join(format!("program-{per_unit}"));
    link_objects(
        &align_driver::CDriver::default(),
        &objects
            .iter()
            .map(|path| path.as_path())
            .collect::<Vec<_>>(),
        &executable,
        &libraries,
        Profile::Release,
    )
    .unwrap();
    Some(std::process::Command::new(executable).output().unwrap())
}

#[test]
fn owned_elements_and_later_failures() {
    owned_fixture::run("owned_elements_and_later_failures", |stage| {
        for per_unit in [false, true] {
            if let Some(output) = execute(stage, &[("main.align", OWNED)], per_unit) {
                assert!(
                    output.status.success(),
                    "mode={per_unit}, status={}, stdout={}, stderr={}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    });
}

#[test]
fn copy_literals_at_named_calls_and_builder_pushes() {
    owned_fixture::run("copy_literals_at_named_calls_and_builder_pushes", |stage| {
        let source = r#"Row { number: i64, text: str }
Nested { values: [i64; 2] }
fn numeric(values: [i64; 2]) -> i64 = values[0] + values[1]
fn views(values: [str; 2]) -> i64 = values[0].len() + values[1].len()
fn records(values: [Row; 1]) -> i64 = values[0].number + values[0].text.len()
fn nested(values: [Nested; 1]) -> i64 { row := values[0]; return row.values[1] }
fn empty(values: [i64; 0]) -> i64 = values.len()
fn reached(value: i64) -> i64 { print(value); return value }
fn early(stop: bool) -> i64 = numeric([reached(1), if stop { return 7 } else { reached(2) }])
fn main() {
  print(numeric([4, 5]))
  text := "a".clone()
  view: str := text
  print(views([view, "é"]))
  print(records([Row { number: 7, text: "row" }]))
  print(empty([]))
  print(nested([Nested { values: [4, 5] }]))
  print(early(true)); print(early(false))
  arena out {
    mut numbers: array_builder<[i64; 2]> := array_builder(out)
    numbers.push([4, 5])
    bound := [6, 7]
    numbers.push(bound)
    values := numbers.build()
    print(values.len())
    first := values[0]
    second := values[1]
    print(numeric(first)); print(numeric(second))
    mut rows: array_builder<[Row; 1]> := array_builder(out)
    rows.push([Row { number: 8, text: "builder" }])
    built := rows.build()
    print(built.len())
    row := built[0]
    print(row[0].number); print(row[0].text)
  }
}
"#;
        for per_unit in [false, true] {
            if let Some(output) = execute(stage, &[("main.align", source)], per_unit) {
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    String::from_utf8_lossy(&output.stdout),
                    "9\n3\n10\n0\n5\n1\n7\n1\n2\n3\n2\n9\n13\n1\n8\nbuilder\n"
                );
            }
        }
    });
}

const OWNED: &str = r#"Row { text: string }
fn consume(rows: [Row; 2], extra: i64) -> i64 {
  return rows[0].text.len() + rows[1].text.len() + extra
}
fn ordinary() -> i64 = consume([Row { text: "one".clone() }, Row { text: "two".clone() }], 0)
fn later_element(stop: bool) -> i64 {
  return consume([Row { text: "one".clone() }, Row { text: if stop { return 7 } else { "two".clone() } }], 0)
}
fn later_argument(stop: bool) -> i64 {
  return consume([Row { text: "one".clone() }, Row { text: "two".clone() }], if stop { return 8 } else { 1 })
}
fn text(stop: bool) -> Result<string, i32> {
  if stop { return Err(9) }
  return Ok("two".clone())
}
fn propagation(stop: bool) -> Result<i64, i32> {
  value := consume([Row { text: "one".clone() }, Row { text: text(stop).map_err(fn error: i32 { error })? }], 0)
  return Ok(value)
}
fn fallbacks(stop: bool) -> i64 {
  return consume([Row { text: "one".clone() }, Row { text: text(stop) else "replacement".clone() }], 0)
}
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn selected(choice: i64) -> i64 {
  return consume([Row { text: "one".clone() }, Row { text: match choice { 1 => "two".clone(), _ => loop { break "two".clone() } } }], 0)
}
fn loop_exit(stop: bool) -> i64 {
  return loop {
    result := consume([Row { text: "one".clone() }, Row { text: if stop { break 11 } else { "two".clone() } }], 0)
    break result
  }
}
fn moved_source() -> i64 {
  text := "one".clone()
  return consume([Row { text: text }, Row { text: "two".clone() }], 0)
}
fn main() -> i32 {
  allocated_before := unsafe { align_rt_alloc_count() }
  freed_before := unsafe { align_rt_free_count() }
  mut iteration := 0
  loop {
    if iteration == 32 { break }
    if ordinary() != 6 { return 1 }
    if later_element(true) != 7 || later_element(false) != 6 { return 2 }
    if later_argument(true) != 8 || later_argument(false) != 7 { return 3 }
    if (match propagation(true) { Ok(_) => 0, Err(error) => error }) != 9 { return 4 }
    if (propagation(false) else 0) != 6 { return 5 }
    if fallbacks(true) != 14 || fallbacks(false) != 6 { return 6 }
    if selected(1) != 6 || selected(0) != 6 { return 8 }
    if loop_exit(true) != 11 || loop_exit(false) != 6 { return 9 }
    if moved_source() != 6 { return 10 }
    iteration = iteration + 1
  }
  allocated := unsafe { align_rt_alloc_count() } - allocated_before
  freed := unsafe { align_rt_free_count() } - freed_before
  if allocated != 800 || allocated != freed { print(allocated); print(freed); return 7 }
  return 0
}
"#;

#[test]
fn existing_admission_and_view_lifetimes_stay_checked() {
    for (label, source, expected) in [
        (
            "bare-return",
            "fn bad() -> [i64; 2] = [1, 2]\nfn main() {}",
            "bare array literal",
        ),
        (
            "bare-branch",
            "fn main() { values := if true { [1, 2] } else { [3, 4] } }",
            "bare array literal",
        ),
        (
            "replacement",
            "fn main() { mut values: [i64; 2] := [1, 2]; values = [3, 4] }",
            "whole-array reassignment",
        ),
        (
            "temporary-borrow",
            "fn take(borrow values: [i64; 2]) {}\nfn main() { take([1, 2]) }",
            "borrow",
        ),
        (
            "indirect",
            "fn take(values: [i64; 2]) {}\nfn main() { f := take; f([1, 2]) }",
            "function value",
        ),
        (
            "view-escape",
            "fn first(values: [str; 1]) -> str = values[0]\nfn bad() -> str { text := \"owned\".clone(); view: str := text; return first([view]) }\nfn main() {}",
            "cannot return a view that borrows local storage",
        ),
    ] {
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, label, source);
        let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
        assert!(
            checked.diags.has_errors() && diagnostics.contains(expected),
            "{label}: {diagnostics}"
        );
    }
}

#[test]
fn cached_generic_literals_preserve_outputs_after_edit_and_restore() {
    owned_fixture::run(
        "cached_generic_literals_preserve_outputs_after_edit_and_restore",
        cached_generic_literals_preserve_outputs_after_edit_and_restore_in,
    );
}

fn cached_generic_literals_preserve_outputs_after_edit_and_restore_in(stage: &Path) {
    let entry = stage.join("main.align");
    let helper = stage.join("helper.align");
    let main = "import helper\nfn main() { print(helper.total(true, [4, 5])); print(helper.total(7, [6, 7])) }\n";
    std::fs::write(&entry, main).unwrap();
    let context = align_driver::CacheContext::at(stage.join("cache"));
    let mut snapshots = Vec::new();
    for (round, bias) in [0, 0, 1, 0].into_iter().enumerate() {
        std::fs::write(&helper, format!("module helper\npub fn sum(values: [i64; 2]) -> i64 = values[0] + values[1]\npub fn total<T>(marker: T, values: [i64; 2]) -> i64 = sum([values[0], values[1]]) + {bias}\n")).unwrap();
        let mut sources = SourceMap::new();
        let mut built = align_driver::build_package(
            &mut sources,
            entry.to_str().unwrap(),
            main,
            &context,
            align_driver::UnitReuse::Allowed,
        );
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &built.diags)
        );
        for unit in ["helper", "main"] {
            let item = built.units.iter().find(|item| item.unit == unit).unwrap();
            assert_eq!(
                item.frontend.as_ref().unwrap().hit,
                matches!(round, 1 | 3),
                "{round}/{unit}"
            );
        }
        let mut snapshot = Vec::new();
        let mut objects = Vec::new();
        let mut libraries = Vec::new();
        for index in 0..built.units.len() {
            let program = built.materialize(index).unwrap();
            snapshot.push(align_mir::print::program_to_string(&program));
            if backend_available() {
                let object = stage.join(format!("{round}-{index}.o"));
                emit_object_file(
                    &program,
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
        }
        match round {
            1 | 3 => assert_eq!(snapshot, snapshots[0]),
            2 => assert_ne!(snapshot, snapshots[0]),
            _ => {}
        }
        snapshots.push(snapshot);
        if backend_available() {
            let executable = stage.join(format!("run-{round}"));
            let object_refs: Vec<_> = objects.iter().map(|path| path.as_path()).collect();
            link_objects(
                &align_driver::CDriver::default(),
                &object_refs,
                &executable,
                &libraries,
                Profile::Release,
            )
            .unwrap();
            let output = std::process::Command::new(executable).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                if bias == 0 { "9\n13\n" } else { "10\n14\n" }
            );
        }
    }
}
