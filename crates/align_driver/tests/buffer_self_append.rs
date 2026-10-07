//! Existing source self-append admission through whole-program and per-unit consumers.
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
fn self_append_survives_growth_moves_and_returns() {
    owned_fixture::run("self_append_survives_growth_moves_and_returns", exercise);
}

fn exercise(stage: &Path) {
    let helper = r#"module helper
pub fn make(capacity: i64) -> buffer {
  mut value := buffer(capacity, 64)
  value.append("abcdefgh".bytes())
  value.append(value.bytes())
  return value
}
pub fn extend(borrow mut value: buffer) {
  value.append(value.bytes()[2..5])
}
"#;
    let source = r#"import helper
fn main() -> i32 {
  mut small := helper.make(8)
  helper.extend(small)
  print(small.len())
  print(small.bytes().as_str() else "invalid")
  mut large := helper.make(64)
  helper.extend(large)
  print(large.len())
  print(large.bytes().as_str() else "invalid")
  mut moved := small
  moved.append(moved.bytes()[18..19])
  print(moved.len())
  print(moved.bytes().as_str() else "invalid")
  return 0
}
"#;
    std::fs::write(stage.join("helper.align"), helper).unwrap();
    let entry = stage.join("main.align");
    std::fs::write(&entry, source).unwrap();
    // The shared parent guard bounds backend discovery, compilation, linking and execution.
    assert!(
        backend_available(),
        "self-append owner requires the native backend"
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
        let output = std::process::Command::new(executable).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "19\nabcdefghabcdefghcde\n19\nabcdefghabcdefghcde\n20\nabcdefghabcdefghcdee\n"
        );
    }
}
