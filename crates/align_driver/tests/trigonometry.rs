//! Scalar/vector trigonometry, using independent finite sample goldens.
#![cfg(unix)]
#[path = "helpers/trig_cases.rs"]
mod cases;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{ArtifactStage, BuildTarget, Profile};
use align_span::SourceMap;
use std::{fs, path::Path, process::Command};

fn execute(stage: &Path, programs: &[align_mir::Program], probe: bool, name: &str) {
    let llc = align_driver::llvm_tool("llc").unwrap();
    let mut objects = if probe {
        vec![stage.join("probe.o")]
    } else {
        Vec::new()
    };
    for (i, program) in programs.iter().enumerate() {
        let input = stage.join(format!("{name}-{i}.ll"));
        let object = input.with_extension("o");
        fs::write(
            &input,
            align_driver::emit_llvm_ir(
                program,
                BuildTarget::Baseline,
                Profile::Release,
                false,
                &[],
                false,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            Command::new(&llc)
                .args(["-filetype=obj", "-relocation-model=pic"])
                .arg(&input)
                .arg("-o")
                .arg(&object)
                .status()
                .unwrap()
                .success()
        );
        objects.push(object);
    }
    let executable = stage.join(name);
    align_driver::link_objects(
        &align_driver::CDriver::default(),
        &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        &executable,
        &[],
        Profile::Release,
    )
    .unwrap();
    assert!(
        Command::new(executable).status().unwrap().success(),
        "{name}"
    );
}

#[test]
fn trigonometry_independent_ieee_samples_whole_and_per_unit() {
    owned_fixture::run(
        "trigonometry_independent_ieee_samples_whole_and_per_unit",
        |stage| {
            let (c, source, count) = cases::sources();
            assert_eq!(cases::SAMPLES.len(), 293);
            assert_eq!(count, 1541);
            fs::write(stage.join("probe.c"), c).unwrap();
            assert!(
                Command::new("cc")
                    .args(["-O2", "-c"])
                    .arg(stage.join("probe.c"))
                    .arg("-o")
                    .arg(stage.join("probe.o"))
                    .status()
                    .unwrap()
                    .success()
            );
            fs::write(stage.join("maths.align"), format!("module maths\n{source}")).unwrap();
            let entry = stage.join("main.align");
            let source = "import maths\nfn main()->i32=maths.exercise()\n";
            fs::write(&entry, source).unwrap();
            for per_unit in [false, true] {
                let mut sm = SourceMap::new();
                let programs = if per_unit {
                    let built =
                        align_driver::build_per_unit(&mut sm, entry.to_str().unwrap(), source);
                    assert!(
                        !built.diags.has_errors(),
                        "{}",
                        align_driver::format_diagnostics(&sm, &built.diags)
                    );
                    built
                        .units
                        .into_iter()
                        .map(|unit| unit.mir)
                        .collect::<Vec<_>>()
                } else {
                    let checked = align_driver::check(&mut sm, entry.to_str().unwrap(), source);
                    assert!(
                        !checked.diags.has_errors(),
                        "{}",
                        align_driver::format_diagnostics(&sm, &checked.diags)
                    );
                    vec![align_driver::lower_to_mir(&checked.hir)]
                };
                execute(stage, &programs, true, &format!("numeric-{per_unit}"));
            }
        },
    );
}

#[test]
fn trigonometry_imports_generics_and_cache() {
    owned_fixture::run("trigonometry_imports_generics_and_cache", |stage| {
        let cache = align_driver::CacheContext::at(stage.join("cache"));
        let entry = stage.join("main.align");
        for (round, operation) in ["sin()", "sin()", "atan2(x)", "sin()"]
            .into_iter()
            .enumerate()
        {
            let expected = cases::SAMPLES
                .iter()
                .find(|row| {
                    row.bits == 64
                        && row.input == 1.0f64.to_bits()
                        && row.operation == operation.split('(').next().unwrap()
                        && (row.operation != "atan2" || row.other == 1.0f64.to_bits())
                })
                .and_then(|row| row.expected)
                .unwrap();
            fs::write(stage.join("maths.align"), format!("module maths\npub fn keep<T>(x:T)->T=x\npub fn compute(x:f64)->f64=keep(x.{operation})\n")).unwrap();
            let source = format!(
                "import maths\nfn main()->i32 {{ value:=maths.compute(1.0)\n bits:=value.to_bits()\n expected:u64:={expected}\n distance:=if bits>expected {{ bits-expected }} else {{ expected-bits }}\n return if distance<=4 {{0}} else {{1}}\n}}\n"
            );
            fs::write(&entry, &source).unwrap();
            let mut sm = SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sm,
                entry.to_str().unwrap(),
                &source,
                &cache,
                align_driver::UnitReuse::Allowed,
            );
            assert!(
                !built.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &built.diags)
            );
            assert_eq!(built.units.len(), 2);
            for unit in &built.units {
                assert_eq!(
                    unit.frontend.as_ref().unwrap().hit,
                    round == 1 || round == 3,
                    "{round}/{}",
                    unit.unit
                );
            }
            let programs = (0..built.units.len())
                .map(|i| built.materialize(i).unwrap().clone())
                .collect::<Vec<_>>();
            execute(stage, &programs, false, &format!("cache-{round}"));
        }
    });
}

#[test]
fn trigonometry_rejects_invalid_receivers_arity_and_mixed_arguments() {
    let stage = ArtifactStage::temp("trig-domain").unwrap();
    let mut rows = Vec::new();
    for method in ["sin", "cos", "atan2"] {
        for ty in [
            "i32",
            "bool",
            "str",
            "vec4<i32>",
            "array<f64>",
            "mask4<f32>",
        ] {
            rows.push((
                format!("fn bad(value:{ty}) {{value.{method}()}}\n"),
                "needs a float",
            ));
        }
    }
    for method in ["sin", "cos"] {
        rows.push((
            format!("fn bad(value:f32) {{value.{method}(1.0)}}\n"),
            "takes 0 argument",
        ));
        rows.push((
            format!("fn bad(value:vec16<f64>) {{value.{method}(value)}}\n"),
            "takes 0 argument",
        ));
    }
    rows.extend([
        ("fn bad(y:f64) {y.atan2()}\n".to_owned(), "takes 1 argument"),
        (
            "fn bad(y:f64) {y.atan2(y,y)}\n".to_owned(),
            "takes 1 argument",
        ),
    ]);
    for (left, right) in [
        ("f32", "f64"),
        ("f64", "i64"),
        ("vec2<f32>", "vec4<f32>"),
        ("vec4<f32>", "vec4<f64>"),
        ("vec4<f32>", "f32"),
        ("f32", "vec4<f32>"),
    ] {
        rows.push((
            format!("fn bad(y:{left},x:{right}) {{y.atan2(x)}}\n"),
            "mismatch",
        ));
    }
    for (source, reason) in rows {
        let source = format!("{source}fn main() {{}}\n");
        let entry = stage.path().join("bad.align");
        fs::write(&entry, &source).unwrap();
        for per_unit in [false, true] {
            let mut sm = SourceMap::new();
            let diags = if per_unit {
                align_driver::build_per_unit(&mut sm, entry.to_str().unwrap(), &source).diags
            } else {
                align_driver::check(&mut sm, entry.to_str().unwrap(), &source).diags
            };
            let text = align_driver::format_diagnostics(&sm, &diags);
            assert!(
                diags.has_errors() && text.contains(reason),
                "{source}/{per_unit}: {text}"
            );
            if reason == "needs a float" {
                assert!(!text.contains("takes 1 argument"), "{text}");
            }
        }
    }
}

#[test]
fn trigonometry_copy_control_paths_and_pure_parallel_closures() {
    owned_fixture::run(
        "trigonometry_copy_control_paths_and_pure_parallel_closures",
        |stage| {
            let entry = stage.join("main.align");
            for operation in ["sin()", "cos()", "atan2(1.0)"] {
                let source = r#"
Cell { value: f64 }
fn maybe(x:f64, yes:bool)->Option<f64> = if yes {Some(x)} else {None}
fn attempt(x:f64, yes:bool)->Result<f64,f64> = if yes {Ok(x)} else {Err(x)}
fn mapped(x:f64, yes:bool)->Result<f64,f64> {
    return attempt(x,yes).map_err(fn e:f64 {e.OP})
}
fn control(x:f64, yes:bool)->Result<f64,f64> {
    cell:=Cell {value:x}
    values:=[cell.value]
    a:=if yes {values[0].OP} else {x.OP}
    b:=match maybe(x,yes) {Some(v)=>v.OP,None=>x.OP}
    c:=maybe(x,yes) else x
    d:=attempt(x,yes)?
    e:=loop {break d.OP}
    return Ok(a+b+c.OP+e)
}
fn main()->i32 {
    x:f64:=0.25
    expected:=x.OP
    success:=control(x,true) else 0.0
    if success!=expected+expected+expected+expected {return 1}
    match control(x,false) {Ok(_)=>{return 2},Err(e)=>{if e!=x {return 3}}}
    match mapped(x,false) {Ok(_)=>{return 4},Err(e)=>{if e!=expected {return 5}}}
    match mapped(x,true) {Ok(v)=>{if v!=x {return 6}},Err(_)=>{return 7}}
    values:array<f64>:=[x,x,x,x].to_array()
    results:=values.par_map(fn v:f64 {v.OP})
    if results.len()!=4 || results[0]!=expected || results[3]!=expected {return 8}
    return 0
}
"#
                .replace("OP", operation);
                fs::write(&entry, &source).unwrap();
                for per_unit in [false, true] {
                    let mut sm = SourceMap::new();
                    let programs = if per_unit {
                        let built =
                            align_driver::build_per_unit(&mut sm, entry.to_str().unwrap(), &source);
                        assert!(
                            !built.diags.has_errors(),
                            "{}",
                            align_driver::format_diagnostics(&sm, &built.diags)
                        );
                        built
                            .units
                            .into_iter()
                            .map(|unit| unit.mir)
                            .collect::<Vec<_>>()
                    } else {
                        let checked =
                            align_driver::check(&mut sm, entry.to_str().unwrap(), &source);
                        assert!(
                            !checked.diags.has_errors(),
                            "{}",
                            align_driver::format_diagnostics(&sm, &checked.diags)
                        );
                        vec![align_driver::lower_to_mir(&checked.hir)]
                    };
                    execute(
                        stage,
                        &programs,
                        false,
                        &format!("control-{operation}-{per_unit}"),
                    );
                }
            }
        },
    );
}

fn checked_programs(stage: &Path, source: &str, per_unit: bool) -> Vec<align_mir::Program> {
    let entry = stage.join("main.align");
    fs::write(&entry, source).unwrap();
    let mut sm = SourceMap::new();
    if per_unit {
        let built = align_driver::build_per_unit(&mut sm, entry.to_str().unwrap(), source);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &built.diags)
        );
        built.units.into_iter().map(|unit| unit.mir).collect()
    } else {
        let checked = align_driver::check(&mut sm, entry.to_str().unwrap(), source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &checked.diags)
        );
        vec![align_driver::lower_to_mir(&checked.hir)]
    }
}

#[test]
fn trigonometry_native_identities_whole_and_per_unit() {
    let stage = ArtifactStage::temp("trig-native-identities").unwrap();
    let mut declarations = Vec::new();
    for (symbol, bits, arity) in [
        ("sinf", 32, 1),
        ("sin", 64, 1),
        ("cosf", 32, 1),
        ("cos", 64, 1),
        ("atan2f", 32, 2),
        ("atan2", 64, 2),
    ] {
        let parameters = (0..arity)
            .map(|i| format!("x{i}:f{bits}"))
            .collect::<Vec<_>>()
            .join(",");
        let definition = format!("pub fn {symbol}({parameters})->f{bits}=x0");
        declarations.push((definition.clone(), Vec::new(), false));
        declarations.push((definition, vec![symbol.to_owned()], true));
        declarations.push((
            format!("extern \"C\" fn {symbol}({parameters})->f{bits}"),
            Vec::new(),
            false,
        ));
        declarations.push((
            format!("extern \"C\" fn {symbol}(x:i64)->f{bits}"),
            Vec::new(),
            true,
        ));
    }
    for symbol in ["sincosf", "sincos", "__sincosf_stret", "__sincos_stret"] {
        declarations.push((
            format!("pub fn {symbol}()->i64=7"),
            vec![symbol.to_owned()],
            true,
        ));
    }
    for (declaration, exports, rejected) in declarations {
        let source = format!("{declaration}\nfn main()->i32=0\n");
        for per_unit in [false, true] {
            for program in checked_programs(stage.path(), &source, per_unit) {
                let result = align_driver::emit_llvm_ir(
                    &program,
                    BuildTarget::Baseline,
                    Profile::Dev,
                    false,
                    &exports,
                    false,
                );
                if rejected {
                    let error = result
                        .err()
                        .expect("reserved identity must reject")
                        .to_string();
                    assert!(
                        error.contains("native extern ABI mismatch")
                            || error.contains("callable external identity collision"),
                        "{error}"
                    );
                } else {
                    assert!(result.is_ok(), "{declaration}/{per_unit}: {result:?}");
                }
            }
        }
    }
}

#[test]
fn trigonometry_evaluates_receiver_then_argument_once() {
    owned_fixture::run(
        "trigonometry_evaluates_receiver_then_argument_once",
        |stage| {
            fs::write(stage.join("probe.c"),concat!(
            "#include <stdint.h>\n#include <stdlib.h>\nstatic int64_t state;\n",
            "double trig_observe(int64_t step){if(++state!=step)abort();return (double)step;}\n",
            "int64_t trig_seen(void){return state;}\nvoid trig_reset(void){state=0;}\n"
        )).unwrap();
            assert!(
                Command::new("cc")
                    .args(["-O2", "-c"])
                    .arg(stage.join("probe.c"))
                    .arg("-o")
                    .arg(stage.join("probe.o"))
                    .status()
                    .unwrap()
                    .success()
            );
            let source = r#"
extern "C" fn trig_observe(step:i64)->f64
extern "C" fn trig_seen()->i64
extern "C" fn trig_reset()
fn left()->vec2<f64> {x:=unsafe {trig_observe(1)};return [x,x]}
fn right()->vec2<f64> {x:=unsafe {trig_observe(2)};return [x,x]}
fn main()->i32 {unsafe {
    value:=trig_observe(1).atan2(trig_observe(2))
    if value<=0.0 || value>=1.0 || trig_seen()!=2 {return 1}
    trig_reset()
    vector:=left().atan2(right())
    if vector[0]!=value || vector[1]!=value || trig_seen()!=2 {return 2}
    return 0
}}
"#;
            for per_unit in [false, true] {
                let programs = checked_programs(stage, source, per_unit);
                execute(stage, &programs, true, &format!("order-{per_unit}"));
            }
        },
    );
}
