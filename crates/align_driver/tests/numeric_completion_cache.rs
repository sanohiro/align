//! Plan225: imported numeric methods preserve whole/per-unit and cached behavior.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn execute(stage: &Path, programs: &[align_mir::Program], label: &str, rt_lto: bool) -> TestResult {
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
    assert!(
        output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
    Ok(())
}
fn programs(stage: &Path, source: &str, per_unit: bool) -> TestResult<Vec<align_mir::Program>> {
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
        Ok(vec![
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources))
                .map_err(|error| format!("{error:?}"))?,
        ])
    }
}

const MODEL: &str = r#"module numeric
pub fn identity<T>(value: T) -> T = value
pub fn bits(value: vec2<i32>, count: i32) -> vec2<i32> = ~(identity(value) << identity(count))
pub fn even(value: vec2<f64>) -> vec2<f64> = identity(value).round_even()
pub fn controls(value: vec2<i32>, count: i32, choose: bool) -> vec2<i32> {
    mut current := if choose { value & count } else { count | value }
    tag:i32 := if choose { 1 } else { 0 }
    current = match tag { 1 => current ^ value, _ => current >> count }
    next := loop { break current << count }
    return next
}
pub fn optional(value: vec2<i32>, count: i32, choose: bool) -> vec2<i32> {
    selected: Option<i32> := if choose { Some(count) } else { None }
    scalar := selected else 7
    return scalar | value
}
pub fn fallible(value: vec2<i32>, count: i32) -> Result<i32, Error> {
    result := value ^ count
    return Ok(result[0])
}
pub fn propagate(value: vec2<i32>, count: i32) -> Result<i32, Error> {
    selected := fallible(value, count).map_err(fn error: Error { error })?
    result := selected >> value
    return Ok(result[0])
}
"#;
const MAIN: &str = r#"module main
import numeric
fn main() -> Result<(), Error> {
    value: vec2<i32> := [5, 9]
    shifted := numeric.bits(value, 33)
    if shifted[0] != -11 || shifted[1] != -19 { return Err(Error.Invalid) }
    input: vec2<f64> := [2.5, -0.5]
    even := numeric.even(input)
    if even[0] != 2.0 || even[1].to_bits() != 9223372036854775808 { return Err(Error.Invalid) }
    yes := numeric.controls(value, 1, true)
    no := numeric.controls(value, 1, false)
    if yes[0] != 8 || yes[1] != 16 || no[0] != 4 || no[1] != 8 { return Err(Error.Invalid) }
    some := numeric.optional(value, 3, true)
    none := numeric.optional(value, 3, false)
    if some[0] != 7 || some[1] != 11 || none[0] != 7 || none[1] != 15 { return Err(Error.Invalid) }
    result := numeric.propagate(value, 1)?
    if result != 0 { return Err(Error.Invalid) }
    return Ok(())
}
"#;

#[test]
fn numeric_methods_import_generic_whole_per_unit_and_cache() {
    owned_fixture::run(
        "numeric_methods_import_generic_whole_per_unit_and_cache",
        |stage| {
            let result = (|| -> TestResult {
                std::fs::write(stage.join("numeric.align"), MODEL)?;
                for per_unit in [false, true] {
                    let built = programs(stage, MAIN, per_unit)?;
                    execute(
                        stage,
                        &built,
                        if per_unit { "units" } else { "whole" },
                        false,
                    )?;
                }
                let entry = stage.join("main.align");
                let cache = align_driver::CacheContext::at(stage.join("cache"));
                let mut original_keys = Vec::new();
                for (round, hit) in [(0, false), (1, true)] {
                    let mut sources = SourceMap::new();
                    let mut built = align_driver::build_package(
                        &mut sources,
                        entry.to_str().ok_or("entry path")?,
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
                        built.units.iter().all(|unit| unit
                            .frontend
                            .as_ref()
                            .is_some_and(|frontend| frontend.hit == hit)),
                        "round {round}: hit={hit}"
                    );
                    let lowered = (0..built.units.len())
                        .map(|index| built.materialize(index).cloned())
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| format!("{error:?}"))?;
                    let keys = lowered
                        .iter()
                        .map(align_interface::codegen_impl_hash)
                        .collect::<Vec<_>>();
                    if round == 0 {
                        original_keys = keys;
                    } else {
                        assert_eq!(
                            keys, original_keys,
                            "cache must preserve canonical numeric MIR"
                        );
                    }
                    execute(stage, &lowered, &format!("cache-{round}"), false)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                panic!("{error}");
            }
        },
    );
}

#[test]
fn numeric_operand_context_checks_each_source_once() {
    owned_fixture::run("numeric_operand_context_checks_each_source_once", |stage| {
        let result = (|| -> TestResult {
            let mut source =
                String::from("fn identity<T>(value:T)->T=value\nfn apply(f:fn()->i32)->i32=f()\n");
            let mut main = String::from("fn main()->i32 {\n v:vec2<i32>:=[4,5]\n");
            for (index, (scalar, vector)) in [
                ("x", "v"),
                ("identity(x)", "identity(v)"),
                ("{ x }", "{ v }"),
                ("if flag { x } else { 3 }", "if flag { v } else { [4,5] }"),
                ("match tag { 0=>x, _=>3 }", "match tag { 0=>v, _=>[4,5] }"),
                ("loop { break x }", "loop { break [4,5] }"),
                ("identity(3)", "identity([4,5])"),
                ("apply(fn { x })", "v"),
            ]
            .into_iter()
            .enumerate()
            {
                for (order, left, right) in [(0, scalar, vector), (1, vector, scalar)] {
                    let name = format!("case_{index}_{order}");
                    source.push_str(&format!("fn {name}(x:i32,v:vec2<i32>,flag:bool,tag:i32)->vec2<i32> = ({left}) | ({right})\n"));
                    main.push_str(&format!(" a{index}_{order}:={name}(3,v,true,0)\n b{index}_{order}:={name}(3,v,false,1)\n if a{index}_{order}[0]!=7 || a{index}_{order}[1]!=7 || b{index}_{order}[0]!=7 || b{index}_{order}[1]!=7 {{ return 1 }}\n"));
                }
            }
            main.push_str(" return 0\n}\n");
            source.push_str(&main);
            let path = stage.join("main.align");
            std::fs::write(&path, &source)?;
            let mut sources = SourceMap::new();
            let checked = align_driver::check(&mut sources, path.to_str().ok_or("path")?, &source);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &checked.diags)
            );
            assert_eq!(
                checked
                    .hir
                    .fns
                    .iter()
                    .filter(|function| function.name.contains("$lambda"))
                    .count(),
                2,
                "each of the two source lambdas must be lifted exactly once"
            );
            for per_unit in [false, true] {
                execute(
                    stage,
                    &programs(stage, &source, per_unit)?,
                    if per_unit {
                        "contexts-units"
                    } else {
                        "contexts-whole"
                    },
                    false,
                )?;
            }
            for expression in [
                "if flag { x } else { v }",
                "if flag { v } else { x }",
                "match tag { 0=>x, _=>v }",
                "match tag { 0=>v, _=>x }",
                "loop { if flag { break x }; break v }",
                "loop { if flag { break v }; break x }",
                "same(x,v)",
                "same(v,x)",
                "identity(missing)",
                "missing",
            ] {
                for (left, right) in [(expression, "v"), ("v", expression)] {
                    let invalid = format!(
                        "fn identity<T>(value:T)->T=value\nfn same<T>(a:T,b:T)->T=a\nfn bad(x:i32,v:vec2<i32>,flag:bool,tag:i32)->vec2<i32> = ({left}) | ({right})\nfn main() {{}}\n"
                    );
                    std::fs::write(&path, &invalid)?;
                    let mut sources = SourceMap::new();
                    let checked =
                        align_driver::check(&mut sources, path.to_str().ok_or("path")?, &invalid);
                    let text = align_driver::format_diagnostics(&sources, &checked.diags);
                    assert!(checked.diags.has_errors(), "accepted {left} | {right}");
                    if expression.contains("missing") {
                        assert_eq!(
                            text.matches("undefined name: 'missing'").count(),
                            1,
                            "{text}"
                        );
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}
