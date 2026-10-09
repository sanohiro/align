//! Exact JSON failure categories through imported helpers and native lowering.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{
    BuildTarget, Profile, backend_available, build_per_unit, check, emit_object_file, link_objects,
    lower_to_mir,
};
use align_span::SourceMap;
use std::path::Path;

#[test]
fn json_doc_errors_preserve_invalid_across_imports() {
    owned_fixture::run("json_doc_errors_preserve_invalid_across_imports", exercise);
}

fn exercise(stage: &Path) {
    std::fs::write(stage.join("invalid-utf8"), [0xff]).unwrap();
    let helper = r#"module helper
import core.json
pub fn parse(input: str) -> Result<i64, Error> {
  arena {
    doc := json.doc(input)?
    return Ok(doc.len())
  }
}
pub fn label(error: Error) -> i64 = match error {
  NotFound => 1,
  Invalid => 2,
  Denied => 3,
  Timeout => 4,
  Code(_) => 5,
}
"#;
    let source = r#"import helper
import core.json
import std.fs
fn verify(input: str) -> i64 {
  arena {
    match json.doc(input) {
      Ok(_) => { return 0 },
      Err(error) => { return helper.label(error) },
    }
  }
}
fn main() -> i32 {
  bad := ["{", "[", "\"", "tru", "null!", "[1,]", "{\"a\":}"]
  mut index := 0
  loop {
    if index >= bad.len() { break }
    if verify(bad[index]) != 2 { return 11 }
    match helper.parse(bad[index]) {
      Ok(_) => { return 12 },
      Err(error) => { if helper.label(error) != 2 { return 13 } },
    }
    index = index + 1
  }
  arena {
    empty := json.doc("{}") else { return 21 }
    nested := json.doc("{\"a\":[1,{}]}") else { return 22 }
    if empty.len() != 0 || nested.get("a").len() != 2 { return 23 }
    match nested.get("missing").as_i64() {
      Some(_) => { return 24 },
      None => {},
    }
  }
  match fs.read_file("request140-missing-entry") {
    Ok(_) => { return 31 },
    Err(error) => { if helper.label(error) != 1 { return 32 } },
  }
  match fs.read_file("invalid-utf8") {
    Ok(_) => { return 33 },
    Err(error) => { if helper.label(error) != 2 { return 34 } },
  }
  print("Invalid")
  return 0
}
"#;
    std::fs::write(stage.join("helper.align"), helper).unwrap();
    let entry = stage.join("main.align");
    std::fs::write(&entry, source).unwrap();
    // The shared parent guard bounds backend discovery, compilation, linking and execution.
    assert!(
        backend_available(),
        "JSON error owner requires the native backend"
    );
    for per_unit in [false, true] {
        let mut sources = SourceMap::new();
        let programs = if per_unit {
            let walk = build_per_unit(&mut sources, entry.to_str().unwrap(), source);
            assert!(
                !walk.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &walk.diags)
            );
            assert_eq!(walk.units.len(), 2);
            walk.units
                .into_iter()
                .map(|unit| unit.mir)
                .collect::<Vec<_>>()
        } else {
            let checked = check(&mut sources, entry.to_str().unwrap(), source);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &checked.diags)
            );
            vec![lower_to_mir(&checked.hir)]
        };
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
        let output = std::process::Command::new(executable).current_dir(stage).output().unwrap();
        assert!(
            output.status.success(),
            "{:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "Invalid\n"
        );
    }
}
