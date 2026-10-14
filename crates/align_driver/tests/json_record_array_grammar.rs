//! Plan187: malformed indexed JSON arrays recover through the ordinary error model.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

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
        &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        &binary,
        &libraries,
        Profile::Release,
    )
    .unwrap();
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(
        output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
}

#[test]
fn json_record_array_grammar_source_matrix() {
    owned_fixture::run("json_record_array_grammar_source_matrix", |stage| {
        let cases: &[(&str, Option<&[i64]>)] = &[
            ("[]", Some(&[])),
            (r#"[{"id":1},{"id":-2}]"#, Some(&[1, -2])),
            (r#"[{"id":1,"skip":1e100000}]"#, Some(&[1])),
            (r#"[{"id":1},,{"id":2}]"#, None),
            (r#"[{"id":1},]"#, None),
            (r#"[{"id":1} {"id":2}]"#, None),
            (r#"[42]"#, None),
            (r#"[{"id":1,"skip":bogus}]"#, None),
            (r#"[{"id":1,"skip":0},{"id":2,"skip":bogus}]"#, None),
            (r#"[{"id":1.5}]"#, None),
            (r#"[{"id":1e2}]"#, None),
            (r#"[{"id":1},{"id":2.5}]"#, None),
            (r#"[{"id":1},{"id":2e3}]"#, None),
            (r#"[{"id":1,"skip":[true,]}]"#, None),
            (r#"[{"id":1,"id":2}]"#, None),
        ];
        let mut source =
            String::from("module main\nimport core.json\nRow { id:i64 }\nfn main()->i32 {\n");
        for mode in ["array", "soa"] {
            for (case, (input, expected)) in cases.iter().enumerate() {
                source.push_str(&format!("arena {{ value:Result<{mode}<Row>,Error>:=json.decode({input:?})\nmatch value {{\n"));
                if let Some(values) = expected {
                    source.push_str(&format!(
                        "Ok(rows)=>{{if rows.len()!={} {{return 1}}\n",
                        values.len()
                    ));
                    for (index, value) in values.iter().enumerate() {
                        let access = if mode == "array" {
                            format!("rows[{index}].id")
                        } else {
                            format!("rows.id[{index}]")
                        };
                        source.push_str(&format!("if {access}!={value} {{return 2}}\n"));
                    }
                    source.push_str("},Err(error)=>{return 3}\n");
                } else {
                    source.push_str(&format!("Ok(rows)=>{{print(\"{mode} case {case} accepted\");return 4}},Err(error)=>match error {{Code(code)=>{{if code!=1 {{return 5}}}},_=>{{return 6}}}}\n"));
                }
                source.push_str("} }\n");
            }
        }
        source.push_str("return 0\n}\n");
        let entry = stage.join("main.align");
        std::fs::write(&entry, &source).unwrap();
        let entry = entry.to_str().unwrap();
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, entry, &source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[program], "whole");
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry, &source);
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
    });
}
