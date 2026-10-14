//! Plan191: canonical package, owned result, calls and failure boundaries.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn package_source(relative: &str) -> TestResult<String> {
    Ok(std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/unicode/pkg")
            .join(relative),
    )?)
}
const MODEL: &str = r#"module model
import pkg.unicode
pub fn identity<T>(value:T)->T = value
pub fn normalize(value:str)->Result<string,Error> = pkg.unicode.nfc(value)
pub fn size(value:str)->i64 { text:=pkg.unicode.nfc(value) else {return -1}; return text.len() }
"#;
const VALUES: &str = r#"module main
import pkg.unicode
import model
fn fresh()->Result<string,Error> {
  return arena out { input:="é".clone_in(out); pkg.unicode.nfc(input) }
}
fn stop()->Result<string,Error> {
  text:=pkg.unicode.nfc("temporary")?
  return Err(Error.Invalid)
}
fn early()->Result<string,Error> = pkg.unicode.nfc({return Ok("early".clone()); ""})
fn main()->Result<(),Error> {
  if pkg.unicode.version()!="16.0.0" {return Err(Error.Code(1))}
  source:="がくせい".clone()
  result:=model.identity(model.normalize(source))?
  if result!="がくせい" || source!="がくせい" {return Err(Error.Code(2))}
  callback:=pkg.unicode.nfc
  direct:=callback("Å")?
  if direct!="Å" {return Err(Error.Code(3))}
  independent:=fresh()?
  if independent!="é" {return Err(Error.Code(4))}
  temporary:=pkg.unicode.nfc("é".clone())?
  if temporary!="é" {return Err(Error.Code(12))}
  hangul:=pkg.unicode.nfc("각")?
  if hangul!="각" {return Err(Error.Code(5))}
  empty:=pkg.unicode.nfc("")?
  if empty!="" {return Err(Error.Code(6))}
  values:=["é","Å"]
  if values.par_map(model.size).sum()!=4 {return Err(Error.Code(7))}
  mut repeated:Result<string,Error>:=pkg.unicode.nfc("first")
  repeated=pkg.unicode.nfc("second")
  second:=repeated?
  if second!="second" {return Err(Error.Code(8))}
  match stop().map_err(fn e:Error {e}) {Ok(_)=>{return Err(Error.Code(9))},Err(_)=>{}}
  early_value:=early()?
  if early_value!="early" {return Err(Error.Code(10))}
  mut n:=0
  loop {if n==3 {break}; text:=if n==1 {pkg.unicode.nfc("Å")} else {pkg.unicode.nfc("Å")}; value:=text?; if value!="Å" {return Err(Error.Code(11))}; n=n+1}
  return Ok(())
}
"#;

fn write_package(stage: &Path, package: &str, descriptor: &str) -> TestResult {
    std::fs::create_dir_all(stage.join("pkg/unicode/internal"))?;
    std::fs::write(stage.join("pkg/unicode.align"), package)?;
    std::fs::write(
        stage.join("pkg/unicode/internal/descriptor.align"),
        descriptor,
    )?;
    std::fs::write(stage.join("model.align"), MODEL)?;
    Ok(())
}
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
fn values(stage: &Path) -> TestResult {
    write_package(
        stage,
        &package_source("unicode.align")?,
        &package_source("unicode/internal/descriptor.align")?,
    )?;
    for per_unit in [false, true] {
        let programs = programs(stage, VALUES, per_unit)?;
        execute(stage, &programs, &format!("values-{per_unit}"), per_unit)?;
    }
    let cache = align_driver::CacheContext::at(stage.join("cache"));
    let path = stage.join("main.align");
    let entry = path.to_str().ok_or("non-UTF8 fixture path")?;
    for round in 0..2 {
        let mut sources = SourceMap::new();
        let mut built = align_driver::build_package(
            &mut sources,
            entry,
            VALUES,
            &cache,
            align_driver::UnitReuse::Allowed,
        );
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &built.diags)
        );
        assert_eq!(built.units.len(), 4);
        assert!(built.units.iter().all(|unit| {
            unit.frontend
                .as_ref()
                .is_some_and(|state| state.hit == (round == 1))
        }));
        let mut programs = Vec::new();
        for index in 0..built.units.len() {
            programs.push(
                built
                    .materialize(index)
                    .map_err(|error| format!("{error:?}"))?
                    .clone(),
            );
        }
        if round == 1 {
            execute(stage, &programs, "warm-cache", false)?;
        }
    }
    Ok(())
}
#[test]
fn unicode_values_and_transport() {
    owned_fixture::run("unicode_values_and_transport", |stage| {
        if let Err(error) = values(stage) {
            panic!("{error}");
        }
    });
}

fn lifecycle(stage: &Path) -> TestResult {
    write_package(
        stage,
        &package_source("unicode.align")?,
        &package_source("unicode/internal/descriptor.align")?,
    )?;
    let source = r#"module main
import pkg.unicode
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
fn fail()->Result<string,Error> { value:=pkg.unicode.nfc("one")?; return Err(Error.Invalid) }
fn run()->Result<(),Error> {
  mut result:Result<string,Error>:=pkg.unicode.nfc("two")
  result=pkg.unicode.nfc("three")
  text:=result?
  if text!="three" {return Err(Error.Invalid)}
  failed:=fail() else "fallback".clone()
  if failed!="fallback" {return Err(Error.Invalid)}
  return Ok(())
}
fn main()->i32 {
  unsafe {align_rt_requested_live_reset()}
  before:=unsafe {align_rt_alloc_count()}
  freed:=unsafe {align_rt_free_count()}
  value:=run() else {return 1}
  if unsafe {align_rt_alloc_count()}-before!=7 {return 2}
  if unsafe {align_rt_free_count()}-freed!=7 {return 3}
  if unsafe {align_rt_requested_live_bytes()}!=0 {return 4}
  return 0
}
"#;
    for per_unit in [false, true] {
        execute(
            stage,
            &programs(stage, source, per_unit)?,
            &format!("lifecycle-{per_unit}"),
            false,
        )?;
    }
    Ok(())
}
#[test]
fn unicode_lifecycle() {
    owned_fixture::run("unicode_lifecycle", |stage| {
        if let Err(error) = lifecycle(stage) {
            panic!("{error}");
        }
    });
}

fn admission(stage: &Path) -> TestResult {
    let package = package_source("unicode.align")?;
    let descriptor = package_source("unicode/internal/descriptor.align")?;
    let mut cases = vec![
        ("arity", package.clone(), descriptor.clone(), "import pkg.unicode\nfn main(){value:=pkg.unicode.nfc()}".to_owned()),
        ("type", package.clone(), descriptor.clone(), "import pkg.unicode\nfn main(){value:=pkg.unicode.nfc(1)}".to_owned()),
        ("internal", package.clone(), descriptor.clone(), "import pkg.unicode.internal.descriptor\nfn main(){value:=pkg.unicode.internal.descriptor.nfc(\"x\")}".to_owned()),
        ("view-escape", package.clone(), descriptor.clone(), "import pkg.unicode\nfn view()->str {value:=pkg.unicode.nfc(\"x\") else {return \"\"}; return value[0..value.len()]}\nfn main(){}".to_owned()),
        ("move", package.clone(), descriptor.clone(), "import pkg.unicode\nfn main(){value:=pkg.unicode.nfc(\"x\");other:=value;again:=value}".to_owned()),
        ("private-extern", package.clone(), descriptor.clone(), "extern \"C\" fn align_rt_unicode_nfc(out:raw,input:raw,length:i64)->i32\nfn main(){}".to_owned()),
    ];
    cases.push(("region", package.clone(), descriptor.clone(),
        "import pkg.unicode\nfn main()->Result<(),Error>{arena out {task_group {task:=spawn(fn {copied:=\"é\".clone_in(out); normalized:=pkg.unicode.nfc(copied); 0}); wait(); print(task.get())}}; return Ok(())}".to_owned()));
    for (label, package) in [
        ("version", package.replace("16.0.0", "17.0.0")),
        ("return", package.replace("Result<string, Error>", "string")),
        (
            "body",
            package.replace(
                "pkg.unicode.internal.descriptor.nfc(input)",
                "Ok(input.clone())",
            ),
        ),
        ("extra", format!("{package}\npub fn hidden()->i64 = 0\n")),
    ] {
        cases.push((
            label,
            package,
            descriptor.clone(),
            "import pkg.unicode\nfn main(){text:=pkg.unicode.nfc(\"x\")}".to_owned(),
        ));
    }
    cases.push((
        "descriptor",
        package.clone(),
        format!("{descriptor}\npub fn helper()->i64 = 0\n"),
        "import pkg.unicode\nfn main(){}".to_owned(),
    ));
    for (label, package, descriptor, body) in cases {
        write_package(stage, &package, &descriptor)?;
        let source = format!("module main\n{body}\n");
        let path = stage.join("main.align");
        std::fs::write(&path, &source)?;
        let entry = path.to_str().ok_or("non-UTF8 fixture path")?;
        for per_unit in [false, true] {
            if label == "private-extern" {
                let programs = programs(stage, &source, per_unit)?;
                let errors: Vec<_> = programs
                    .iter()
                    .filter_map(|program| {
                        align_driver::emit_llvm_ir(
                            program,
                            BuildTarget::Baseline,
                            Profile::Release,
                            false,
                            &[],
                            false,
                        )
                        .err()
                    })
                    .collect();
                assert!(!errors.is_empty(), "private native extern accepted");
                assert!(
                    errors
                        .iter()
                        .all(|error| error.to_string().contains("native extern ABI mismatch")),
                    "{errors:?}"
                );
                continue;
            }
            let mut sources = SourceMap::new();
            let (failed, diagnostic) = if per_unit {
                let checked = align_driver::build_per_unit(&mut sources, entry, &source);
                (
                    checked.diags.has_errors(),
                    align_driver::format_diagnostics(&sources, &checked.diags),
                )
            } else {
                let checked = align_driver::check(&mut sources, entry, &source);
                (
                    checked.diags.has_errors(),
                    align_driver::format_diagnostics(&sources, &checked.diags),
                )
            };
            assert!(failed, "accepted {label}, per_unit={per_unit}");
            if label == "region" {
                assert!(diagnostic.contains("region capability"), "{diagnostic}");
            }
            assert!(
                !diagnostic.contains("internal compiler error"),
                "{label}: {diagnostic}"
            );
        }
    }
    Ok(())
}
#[test]
fn unicode_package_admission() {
    owned_fixture::run("unicode_package_admission", |stage| {
        if let Err(error) = admission(stage) {
            panic!("{error}");
        }
    });
}

#[test]
fn unicode_documentation_examples() {
    owned_fixture::run("unicode_documentation_examples", |stage| {
        let check = || -> TestResult {
            write_package(
                stage,
                &package_source("unicode.align")?,
                &package_source("unicode/internal/descriptor.align")?,
            )?;
            for relative in ["unicode.md", "ja/unicode.md"] {
                let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../docs/impl/pkg-design")
                    .join(relative);
                let document = std::fs::read_to_string(path)?;
                let mut checked = 0;
                for block in document.split("```align\n").skip(1) {
                    let source = block.split("```").next().ok_or("example fence")?;
                    if source.starts_with("pub fn version()") {
                        continue;
                    }
                    let source = format!("module main\n{source}\nfn main() {{}}\n");
                    programs(stage, &source, false)?;
                    checked += 1;
                }
                assert_eq!(checked, 1, "{relative}");
            }
            Ok(())
        };
        if let Err(error) = check() {
            panic!("{error}");
        }
    });
}
