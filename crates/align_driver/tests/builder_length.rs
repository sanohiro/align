//! Scalar progress observations preserve unfinished builder ownership.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{backend_available, build_per_unit, check, emit_object_file, link_objects,
    lower_to_mir, BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

fn check_diagnostics(label: &str, source: &str) -> String {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, label, source);
    align_driver::format_diagnostics(&sources, &checked.diags)
}
fn check_errs(label: &str, source: &str) -> bool {
    let mut sources = SourceMap::new();
    check(&mut sources, label, source).diags.has_errors()
}

// Called only inside owned_fixture::run, including every transitive native wait.
fn execute(stage: &Path, files: &[(&str, &str)], per_unit: bool) -> Option<std::process::Output> {
    for (name, source) in files { std::fs::write(stage.join(name), source).unwrap(); }
    let entry = stage.join("main.align");
    let source = std::fs::read_to_string(&entry).unwrap();
    let mut sources = SourceMap::new();
    let programs = if per_unit {
        let walk = build_per_unit(&mut sources, entry.to_str().unwrap(), &source);
        assert!(!walk.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &walk.diags));
        assert_eq!(walk.units.len(), files.len());
        walk.units.into_iter().map(|unit| unit.mir).collect::<Vec<_>>()
    } else {
        let checked = check(&mut sources, entry.to_str().unwrap(), &source);
        assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &checked.diags));
        vec![lower_to_mir(&checked.hir)]
    };
    if !backend_available() { return None; }
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{per_unit}-{index}.o"));
        emit_object_file(program, &object, BuildTarget::Baseline, Profile::Release, &[], false).unwrap();
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) { libraries.push(library.clone()); }
        }
    }
    let executable = stage.join(format!("program-{per_unit}"));
    link_objects(&align_driver::CDriver::default(),
        &objects.iter().map(|path| path.as_path()).collect::<Vec<_>>(),
        &executable, &libraries, Profile::Release).unwrap();
    Some(std::process::Command::new(executable).output().unwrap())
}

fn assert_checked(label: &str, source: &str) {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, label, source);
    assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &checked.diags));
    let effects = align_sema::fn_effects(&checked.hir, &std::collections::HashMap::new());
    for function in checked.hir.fns.iter().filter(|function| function.name == "query") {
        assert_eq!(effects.get(&function.name), Some(&align_sema::FnEffect::Pure));
        assert_eq!(function.return_borrow, align_sema::hir::ReturnBorrowSummary::None);
    }
    assert!(align_mir::lower_program_checked(&checked.hir, false, None).is_ok());
}

#[test]
fn formation_and_ownership() {
    for (ty, new, finish) in [
        ("builder", "builder()", "to_string"),
        ("array_builder<i64>", "array_builder()", "build"),
    ] {
        for mode in ["borrow", "borrow mut", ""] {
            assert_checked("builder-length-mode", &format!(
                "fn query({mode} value: {ty}) -> i64 = value.len()\nfn main() {{}}\n"));
        }
        for (body, expected) in [
            (format!("value: {ty} := {new}; print(value.len(1))"), "takes no arguments"),
            ("print(create().len())".to_owned(), "bind the builder"),
            ("print(create().len(1))".to_owned(), "takes no arguments"),
            (format!("value: {ty} := {new}; print((if true {{ value }} else {{ create() }}).len())"), "bind the builder"),
            (format!("value: {ty} := {new}; finished := value.{finish}(); print(value.len())"), "moved"),
            (format!("value: {ty} := {new}; wrong: bool := value.len()"), "i64 vs bool"),
        ] {
            let program = format!("fn create() -> {ty} = {new}\nfn main() {{ {body} }}\n");
            let diagnostics = check_diagnostics("builder-length-reject", &program);
            assert!(diagnostics.contains(expected), "{program}\n{diagnostics}");
            if body == "print(create().len(1))" {
                assert!(!diagnostics.contains("bind the builder"), "arity precedes place: {diagnostics}");
            }
        }
        let borrowed = format!("fn bad(borrow value: {ty}) {{ before := value.len(); result := value.{finish}() }}\nfn main() {{}}\n");
        assert!(check_errs("builder-length-borrowed-finish", &borrowed));
    }
    for source in [
        "fn bad(borrow value: builder) { before := value.len(); value.write(\"x\") }\nfn main() {}",
        "fn bad(borrow value: array_builder<i64>) { before := value.len(); value.push(1) }\nfn main() {}",
    ] {
        assert!(check_errs("builder-length-borrow-exclusion", source), "{source}");
    }
}

#[test]
fn text_progress_and_nonconsuming_helpers() {
    owned_fixture::run("text_progress_and_nonconsuming_helpers", text_progress_and_nonconsuming_helpers_in);
}

fn text_progress_and_nonconsuming_helpers_in(stage: &Path) {
    let source = r#"fn text_count(borrow output: builder) -> i64 = output.len()
fn element_count(borrow values: array_builder<i64>) -> i64 = values.len()
fn main() -> i32 {
  mut output := builder(1)
  output.write("あ")
  print(text_count(output))
  output.write_int(42)
  print(text_count(output))
  output.write_int(output.len())
  print(text_count(output))
  print(output.to_string())
  mut values: array_builder<i64> := array_builder(1)
  values.push(7)
  print(element_count(values))
  values.push(9)
  print(element_count(values))
  values.push(values.len())
  print(element_count(values))
  print(values.build().sum())
  return 0
}
"#;
    assert_checked("builder-length-progress", source);
    if let Some(output) = execute(stage, &[("main.align", source)], false) {
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "3\n5\n6\nあ425\n1\n2\n3\n18\n");
    }
}

#[test]
fn all_element_families_and_allocation_modes() {
    owned_fixture::run("all_element_families_and_allocation_modes", all_element_families_and_allocation_modes_in);
}

fn all_element_families_and_allocation_modes_in(stage: &Path) {
    let source = r#"Row { value: i64 }
Owned { value: string }
Zero { value: [i64; 0] }
fn main() -> i32 {
  mut scalar: array_builder<i64> := array_builder(2)
  scalar.push(7)
  print(scalar.len())
  print(scalar.build().sum())
  mut records: array_builder<Row> := array_builder()
  records.push(Row { value: 8 })
  print(records.len())
  print(records.build()[0].value)
  mut strings: array_builder<string> := array_builder()
  strings.push("text".clone())
  print(strings.len())
  print(strings.build()[0])
  mut owned: array_builder<Owned> := array_builder()
  owned.push(Owned { value: "owned".clone() })
  print(owned.len())
  print(owned.build().len())
  arena out {
    mut views: array_builder<str> := array_builder(out, 1)
    views.push("view")
    print(views.len())
    print(views.build()[0])
    vector: vec4<i32> := [1, 2, 3, 4]
    other: vec4<i32> := [0, 3, 2, 5]
    mut vectors: array_builder<vec4<i32>> := array_builder(out)
    vectors.push(vector)
    print(vectors.len())
    print(vectors.build()[0][2])
    mut masks: array_builder<mask4<i32>> := array_builder(out)
    masks.push(vector > other)
    print(masks.len())
    print(select(masks.build()[0], vector, other)[1])
    mut fixed: array_builder<[i64; 2]> := array_builder(out)
    fixed_input: [i64; 2] := [4, 5]
    fixed.push(fixed_input)
    print(fixed.len())
    fixed_built := fixed.build()
    fixed_value := fixed_built[0]
    print(fixed_value[1])
    mut fixed_records: array_builder<[Row; 1]> := array_builder(out)
    record_input: [Row; 1] := [Row { value: 6 }]
    fixed_records.push(record_input)
    print(fixed_records.len())
    records_built := fixed_records.build()
    record_value := records_built[0]
    print(record_value[0].value)
    mut empty: array_builder<Zero> := array_builder(out)
    zero: [i64; 0] := []
    empty_input := Zero { value: zero }
    empty.push(empty_input)
    empty.push(empty_input)
    print(empty.len())
    print(empty.build().len())
  }
  return 0
}
"#;
    assert_checked("builder-length-families", source);
    if let Some(output) = execute(stage, &[("main.align", source)], false) {
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n7\n1\n8\n1\ntext\n1\n1\n1\nview\n1\n3\n1\n3\n1\n5\n1\n6\n2\n2\n");
    }
}

#[test]
fn observations_cross_control_and_units() {
    owned_fixture::run("observations_cross_control_and_units", observations_cross_control_and_units_in);
}

fn observations_cross_control_and_units_in(stage: &Path) {
    let helper = r#"module lengths
pub fn text(borrow output: builder) -> i64 = output.len()
pub fn elements<T: RegionPlain>(borrow values: array_builder<T>) -> i64 = values.len()
pub fn detached() -> i64 { output := builder(); output.write("gone"); return output.len() }
pub fn replace(borrow mut output: builder) -> i64 {
  before := output.len()
  output = builder(32)
  output.write("xy")
  return before
}
pub fn flow(borrow output: builder) -> Result<i64, i32> {
  selected := if output.len() > 0 { output.len() } else { 0 }
  optional: Option<i64> := Some(output.len())
  count := optional else output.len()
  result: Result<i64, i32> := Ok(output.len())
  last := result.map_err(fn error: i32 { error })?
  return Ok(selected + count + last)
}
pub fn early(borrow output: builder, stop: bool) -> i64 {
  if stop { return output.len() }
  return output.len() + 1
}
"#;
    let source = r#"import lengths
fn main() -> i32 {
  print(lengths.detached())
  mut output := builder(1)
  output.write("a")
  first := output.len()
  output.write_int(42)
  second := output.len()
  output.write("b")
  print(first); print(second)
  print(lengths.replace(output))
  print(lengths.text(output))
  print(match lengths.flow(output) { Ok(value) => value, Err(_) => -1 })
  print(lengths.early(output, true))
  print(lengths.early(output, false))
  print(output.to_string())
  mut values: array_builder<i64> := array_builder(1)
  mut index := 0
  loop {
    values.push(index)
    print(lengths.elements(values))
    index = index + 1
    if index == 3 { break }
  }
  old := values.len()
  values = array_builder(16)
  print(old); print(values.len())
  suffix := [8, 9]
  values.append(suffix[..])
  print(lengths.elements(values))
  print(values.build().sum())
  return 0
}
"#;
    let files = [("lengths.align", helper), ("main.align", source)];
    for per_unit in [false, true] {
        let Some(output) = execute(stage, &files, per_unit) else { continue; };
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "4\n1\n3\n4\n2\n6\n2\n3\nxy\n1\n2\n3\n3\n0\n2\n17\n");
    }
}

#[test]
fn cached_generic_queries_preserve_outputs_after_edit_and_restore() {
    owned_fixture::run("cached_generic_queries_preserve_outputs_after_edit_and_restore", cached_generic_queries_preserve_outputs_after_edit_and_restore_in);
}

fn cached_generic_queries_preserve_outputs_after_edit_and_restore_in(stage: &Path) {
    let entry = stage.join("main.align");
    let helper = stage.join("helper.align");
    let main = "import helper\nfn main() { b := builder(); b.write(\"あ\"); mut a: array_builder<i64> := array_builder(); a.push(7); print(helper.text(b)); print(helper.count(a)) }\n";
    std::fs::write(&entry, main).unwrap();
    let context = align_driver::CacheContext::at(stage.join("cache"));
    let mut snapshots = Vec::new();
    for (round, bias) in [0, 0, 1, 0].into_iter().enumerate() {
        std::fs::write(&helper, format!("module helper\npub fn text(borrow b: builder) -> i64 = b.len() + {bias}\npub fn count<T: RegionPlain>(borrow a: array_builder<T>) -> i64 = a.len() + {bias}\n")).unwrap();
        let mut sources = SourceMap::new();
        let mut built = align_driver::build_package(&mut sources, entry.to_str().unwrap(), main, &context, align_driver::UnitReuse::Allowed);
        assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &built.diags));
        for unit in ["helper", "main"] {
            let item = built.units.iter().find(|item| item.unit == unit).unwrap();
            assert_eq!(item.frontend.as_ref().unwrap().hit, matches!(round, 1 | 3), "{round}/{unit}");
        }
        let mut snapshot = Vec::new();
        let mut objects = Vec::new();
        let mut libraries = Vec::new();
        for index in 0..built.units.len() {
            let program = built.materialize(index).unwrap();
            snapshot.push(align_mir::print::program_to_string(&program));
            if backend_available() {
                let object = stage.join(format!("{round}-{index}.o"));
                emit_object_file(&program, &object, BuildTarget::Baseline, Profile::Release, &[], false).unwrap();
                objects.push(object);
                for library in &program.link_libs {
                    if !libraries.contains(library) { libraries.push(library.clone()); }
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
            link_objects(&align_driver::CDriver::default(), &object_refs, &executable, &libraries, Profile::Release).unwrap();
            let output = std::process::Command::new(executable).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout), if bias == 0 { "3\n1\n" } else { "4\n2\n" });
        }
    }
}
