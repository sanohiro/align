//! Cancellation-sensitive libm math, using independent finite sample goldens.
#![cfg(unix)]
#[path = "helpers/math_libm_cases.rs"]
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
fn cancellation_math_independent_ieee_samples_whole_and_per_unit() {
    owned_fixture::run(
        "cancellation_math_independent_ieee_samples_whole_and_per_unit",
        |stage| {
            let (c, source, count) = cases::sources();
            assert_eq!(cases::SAMPLES.len(), 106);
            assert_eq!(count, 582);
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
fn cancellation_math_generic_cache_replays_operation_changes() {
    owned_fixture::run(
        "cancellation_math_generic_cache_replays_operation_changes",
        |stage| {
            let cache = align_driver::CacheContext::at(stage.join("cache"));
            let entry = stage.join("main.align");
            for (round, operation) in ["expm1", "expm1", "log1p", "expm1"].into_iter().enumerate() {
                let expected = cases::SAMPLES
                    .iter()
                    .find(|row| {
                        row.bits == 64
                            && row.input == 1.0f64.to_bits()
                            && row.operation == operation
                    })
                    .and_then(|row| row.expected)
                    .unwrap();
                fs::write(stage.join("maths.align"), format!("module maths\npub fn keep<T>(x:T)->T=x\npub fn compute(x:f64)->f64=keep(x.{operation}())\n")).unwrap();
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
        },
    );
}

#[test]
fn cancellation_math_rejects_invalid_receivers_arity_and_aliases() {
    let stage = ArtifactStage::temp("math-domain").unwrap();
    for operation in ["expm1", "log1p"] {
        for (ty, args, reason) in [
            ("i32", "1", "needs a float"),
            ("bool", "", "needs a float"),
            ("str", "", "needs a float"),
            ("vec4<i32>", "", "needs a float vector"),
            ("array<f64>", "", "needs a float"),
            ("mask4<f32>", "", "needs a float"),
            ("f32", "1.0", "takes 0 argument"),
            ("vec16<f64>", "value", "takes 0 argument"),
        ] {
            let source =
                format!("fn bad(value:{ty}) {{ value.{operation}({args}) }}\nfn main() {{}}\n");
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
                    "{operation}/{ty}/{per_unit}: {text}"
                );
                if ty == "i32" {
                    assert!(!text.contains("takes 0 argument"), "{text}");
                }
            }
        }
    }
    let mut sm = SourceMap::new();
    let checked = align_driver::check(
        &mut sm,
        "alias",
        "fn bad(x:f64)->f64=x.logp1()\nfn main() {}\n",
    );
    assert!(checked.diags.has_errors());
    assert!(align_driver::format_diagnostics(&sm, &checked.diags).contains("unknown method"));
}

#[test]
fn cancellation_math_reserves_native_exports_but_accepts_matching_externs() {
    let stage = ArtifactStage::temp("math-native-identities").unwrap();
    let entry = stage.path().join("main.align");
    for (symbol, bits) in [("expm1f", 32), ("expm1", 64), ("log1pf", 32), ("log1p", 64)] {
        for (declaration, export, rejected) in [
            (
                format!("pub fn {symbol}(x:f{bits})->f{bits}=x"),
                false,
                false,
            ),
            (format!("pub fn {symbol}(x:f{bits})->f{bits}=x"), true, true),
            (format!("pub fn {symbol}()->i64=7"), true, true),
            (
                format!("extern \"C\" fn {symbol}(x:f{bits})->f{bits}"),
                false,
                false,
            ),
            (
                format!("extern \"C\" fn {symbol}(x:i64)->f{bits}"),
                false,
                true,
            ),
        ] {
            // Deliberately no local MathOp: native identity ownership crosses units.
            let source = format!("{declaration}\nfn main()->i32=0\n");
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
                    let checked = align_driver::check(&mut sm, entry.to_str().unwrap(), &source);
                    assert!(
                        !checked.diags.has_errors(),
                        "{}",
                        align_driver::format_diagnostics(&sm, &checked.diags)
                    );
                    vec![align_driver::lower_to_mir(&checked.hir)]
                };
                assert_eq!(programs.len(), 1);
                let exports = if export {
                    vec![symbol.to_owned()]
                } else {
                    Vec::new()
                };
                for program in programs {
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
                            .expect("fixed math native identity must reject")
                            .to_string();
                        assert!(
                            error.contains(if export {
                                "callable external identity collision"
                            } else {
                                "native extern ABI mismatch"
                            }),
                            "{error}"
                        );
                    } else {
                        assert!(result.is_ok(), "{symbol}/{per_unit}: {result:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn cancellation_math_copy_control_paths_and_pure_parallel_closures() {
    owned_fixture::run("cancellation_math_copy_control_paths_and_pure_parallel_closures", |stage| {
        let entry=stage.join("main.align");
        for operation in ["expm1","log1p"] {
            let source=r#"
Cell { value: f64 }
fn maybe(x:f64, yes:bool)->Option<f64> = if yes {Some(x)} else {None}
fn attempt(x:f64, yes:bool)->Result<f64,f64> = if yes {Ok(x)} else {Err(x)}
fn mapped(x:f64, yes:bool)->Result<f64,f64> {
    return attempt(x,yes).map_err(fn e:f64 {e.OP()})
}
fn control(x:f64, yes:bool)->Result<f64,f64> {
    cell:=Cell {value:x}
    values:=[cell.value]
    a:=if yes {values[0].OP()} else {x.OP()}
    b:=match maybe(x,yes) {Some(v)=>v.OP(),None=>x.OP()}
    c:=maybe(x,yes) else x
    d:=attempt(x,yes)?
    e:=loop {break d.OP()}
    return Ok(a+b+c.OP()+e)
}
fn main()->i32 {
    x:f64:=0.25
    expected:=x.OP()
    success:=control(x,true) else 0.0
    if success!=expected+expected+expected+expected {return 1}
    match control(x,false) {Ok(_)=>{return 2},Err(e)=>{if e!=x {return 3}}}
    match mapped(x,false) {Ok(_)=>{return 4},Err(e)=>{if e!=expected {return 5}}}
    match mapped(x,true) {Ok(v)=>{if v!=x {return 6}},Err(_)=>{return 7}}
    values:array<f64>:=[x,x,x,x].to_array()
    results:=values.par_map(fn v:f64 {v.OP()})
    if results.len()!=4 || results[0]!=expected || results[3]!=expected {return 8}
    return 0
}
"#.replace("OP",operation);
            fs::write(&entry,&source).unwrap();
            for per_unit in [false,true] {
                let mut sm=SourceMap::new();
                let programs=if per_unit {
                    let built=align_driver::build_per_unit(&mut sm,entry.to_str().unwrap(),&source);
                    assert!(!built.diags.has_errors(),"{}",align_driver::format_diagnostics(&sm,&built.diags));
                    built.units.into_iter().map(|unit|unit.mir).collect::<Vec<_>>()
                } else {
                    let checked=align_driver::check(&mut sm,entry.to_str().unwrap(),&source);
                    assert!(!checked.diags.has_errors(),"{}",align_driver::format_diagnostics(&sm,&checked.diags));
                    vec![align_driver::lower_to_mir(&checked.hir)]
                };
                execute(stage,&programs,false,&format!("control-{operation}-{per_unit}"));
            }
        }
    });
}
