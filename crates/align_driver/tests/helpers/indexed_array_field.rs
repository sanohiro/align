//! Native/whole/per-unit harness shared by the indexed field observation owners.
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;
pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
pub fn run_programs(
    stage: &Path,
    programs: &[align_mir::Program],
    label: &str,
    rt_lto: bool,
) -> TestResult<std::process::Output> {
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
            rt_lto,
        )?;
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
    )?;
    let output = std::process::Command::new(binary).output()?;
    Ok(output)
}
pub fn programs(stage: &Path, source: &str, per_unit: bool) -> TestResult<Vec<align_mir::Program>> {
    let path = stage.join("main.align");
    std::fs::write(&path, source)?;
    let entry = path.to_str().ok_or("non-UTF8 fixture path")?;
    let mut sources = SourceMap::new();
    if per_unit {
        let built = align_driver::build_per_unit(&mut sources, entry, source);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &built.diags)
        );
        Ok(built.units.into_iter().map(|unit| unit.mir).collect())
    } else {
        let checked = align_driver::check(&mut sources, entry, source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        Ok(vec![align_mir::lower_program_checked(
            &checked.hir,
            false,
            Some(&sources),
        )
        .map_err(|error| format!("{error:?}"))?])
    }
}

pub fn execute(stage: &Path, source: &str, name: &str) -> TestResult {
    for per_unit in [false, true] {
        let built = programs(stage, source, per_unit)
            .map_err(|error| format!("{name}/{per_unit}: {error}"))?;
        let output = run_programs(stage, &built, &format!("{name}-{per_unit}"), false)
            .map_err(|error| format!("{name}/{per_unit}: {error}"))?;
        assert!(
            output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
            "{name}/{per_unit}: {output:?}"
        );
    }
    Ok(())
}
