//! Request146: binary setters preserve caller-backed byte views across repeated calls.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

fn fixture() -> (String, String) {
    let mut cases = vec![
        ("set_u8".to_string(), "0x12", vec![0x12]),
        ("set_i8".to_string(), "-1", vec![0xff]),
    ];
    for (scalar, value, bytes) in [
        ("u16", "0x1234", 0x1234u16.to_le_bytes().to_vec()),
        ("i16", "-2", (-2i16).to_le_bytes().to_vec()),
        ("u32", "0x12345678", 0x12345678u32.to_le_bytes().to_vec()),
        ("i32", "-2", (-2i32).to_le_bytes().to_vec()),
        (
            "u64",
            "0x0102030405060708",
            0x0102030405060708u64.to_le_bytes().to_vec(),
        ),
        ("i64", "-2", (-2i64).to_le_bytes().to_vec()),
        ("f32", "1.0", 1.0f32.to_le_bytes().to_vec()),
        ("f64", "1.0", 1.0f64.to_le_bytes().to_vec()),
    ] {
        for endian in ["le", "be"] {
            let mut expected = bytes.clone();
            if endian == "be" {
                expected.reverse();
            }
            cases.push((format!("set_{scalar}_{endian}"), value, expected));
        }
    }
    let mut model = String::from("module model\n");
    let mut main = String::from("module main\nimport model\n");
    for (index, (method, value, expected)) in cases.iter().enumerate() {
        let width = expected.len();
        model.push_str(&format!(
            r#"
pub fn write_{index}(borrow mut bytes:slice<u8>) {{ bytes.{method}(0,{value}) }}
pub fn repeat_{index}(borrow mut bytes:slice<u8>,count:i64)->i64 {{
  mut at:=0
  loop {{
    if at>=count {{break}}
    mut active:=bytes[0..{width}]
    write_{index}(active)
    at=at+1
  }}
  return at
}}
"#
        ));
        let check = expected
            .iter()
            .enumerate()
            .map(|(i, value)| format!("bytes[{i}]!={value}"))
            .collect::<Vec<_>>()
            .join(" || ");
        let zero = expected
            .iter()
            .enumerate()
            .map(|(i, _)| format!("bytes[{i}]!=0"))
            .collect::<Vec<_>>()
            .join(" || ");
        main.push_str(&format!(
            r#"
fn case_{index}()->i32 {{
  mut storage:=buffer.filled({width},0)
  mut bytes:=storage.bytes()
  if model.repeat_{index}(bytes,0)!=0 || {zero} {{return 1}}
  if model.repeat_{index}(bytes,1)!=1 || {check} {{return 2}}
  if model.repeat_{index}(bytes,3)!=3 || {check} {{return 3}}
  mut nothing:=buffer.filled(0,0)
  mut empty:=nothing.bytes()
  if model.repeat_{index}(empty,0)!=0 {{return 4}}
  return 0
}}
"#
        ));
    }
    model.push_str(r#"
pub fn forward<T>(borrow mut bytes:slice<u8>, marker:T) {write_0(bytes)}
pub fn fail(flag:bool)->Result<(),Error> {if flag {return Err(Error.Invalid)};return Ok(())}
pub fn keep(error:Error)->Error = error
pub fn after(borrow mut bytes:slice<u8>,flag:bool)->Result<(),Error> {write_0(bytes);fail(flag).map_err(keep)?;return Ok(())}
pub fn control(borrow mut bytes:slice<u8>)->Result<(),Error> {
  mut active:=bytes[0..1]
  forward(active,1)
  if bytes[0]!=0x12 {return Err(Error.Invalid)}
  match after(active,true) {Ok(_)=>{return Err(Error.Invalid)},Err(_)=>{}}
  if bytes[0]!=0x12 {return Err(Error.Invalid)}
  after(active,false)?
  return Ok(())
}
"#);
    main.push_str("fn main()->i32 {\n");
    for index in 0..cases.len() {
        main.push_str(&format!("if case_{index}()!=0 {{return {}}}\n", index + 1));
    }
    main.push_str("mut owner:=buffer.filled(1,0)\nmut bytes:=owner.bytes()\nmodel.control(bytes) else {return 40}\nreturn 0\n}\n");
    (model, main)
}

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
fn binary_byte_write_loops_and_replay() {
    owned_fixture::run("binary_byte_write_loops_and_replay", |stage| {
        let (model, main) = fixture();
        std::fs::write(stage.join("model.align"), model).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, &main).unwrap();
        let entry = entry.to_str().unwrap();
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, entry, &main);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[program], "whole");
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry, &main);
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
        let cache = align_driver::CacheContext::at(stage.join("cache"));
        for round in 0..2 {
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sources,
                entry,
                &main,
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
                built
                    .units
                    .iter()
                    .all(|unit| unit.frontend.as_ref().unwrap().hit == (round == 1))
            );
            let programs = (0..built.units.len())
                .map(|i| built.materialize(i).unwrap().clone())
                .collect::<Vec<_>>();
            if round == 1 {
                execute(stage, &programs, "cache-hit");
            }
        }
    });
}

#[test]
fn binary_byte_write_rejects_invalid_authority_and_old_observations() {
    owned_fixture::run(
        "binary_byte_write_rejects_invalid_authority_and_old_observations",
        |stage| {
            let cases = [
                (
                    "fn bad(borrow bytes:slice<u8>){bytes.set_u8(0,1)}",
                    "immutable",
                ),
                (
                    "fn bad(){mut owner:=buffer.filled(1,0);mut bytes:=owner.bytes();owner=buffer.filled(1,0);bytes.set_u8(0,1)}",
                    "invalidated borrow",
                ),
                (
                    "fn bad()->slice<u8>{mut owner:=buffer.filled(1,0);mut bytes:=owner.bytes();bytes.set_u8(0,1);return bytes}",
                    "cannot return",
                ),
                (
                    "fn bad()->Result<(),Error>{mut owner:=buffer.filled(1,65);mut bytes:=owner.bytes();text:=bytes.as_str()?;bytes.set_u8(0,255);print(text);return Ok(())}",
                    "invalidated borrow",
                ),
                // A conditional write is not a mandatory write, including an error before it.
                (
                    "fn write(borrow mut bytes:slice<u8>,flag:bool){if flag {bytes.set_u8(0,1)}}\nfn bad(borrow mut bytes:slice<u8>,flag:bool){mut active:=bytes[0..1];write(active,flag);print(bytes.len())}",
                    "invalidated borrow",
                ),
                (
                    "fn write(borrow mut bytes:slice<u8>,flag:bool)->Result<(),Error>{if flag {return Err(Error.Invalid)};bytes.set_u8(0,1);return Ok(())}\nfn bad(borrow mut bytes:slice<u8>,flag:bool)->Result<(),Error>{mut active:=bytes[0..1];write(active,flag)?;print(bytes.len());return Ok(())}",
                    "invalidated borrow",
                ),
                // Empty fill/copy may return without writing an element.
                (
                    "fn write(borrow mut bytes:slice<u8>){bytes.fill(1)}\nfn bad(borrow mut bytes:slice<u8>){mut active:=bytes[0..0];write(active);print(bytes.len())}",
                    "invalidated borrow",
                ),
                (
                    "fn write(borrow mut bytes:slice<u8>,source:slice<u8>){bytes.copy_from(source)}\nfn bad(borrow mut bytes:slice<u8>,source:slice<u8>){mut active:=bytes[0..0];write(active,source);print(bytes.len())}",
                    "invalidated borrow",
                ),
            ];
            for (index, (body, expected)) in cases.iter().enumerate() {
                let source = format!("{body}\nfn main() {{}}\n");
                let entry = stage.join(format!("negative-{index}.align"));
                std::fs::write(&entry, &source).unwrap();
                for per_unit in [false, true] {
                    let mut sources = SourceMap::new();
                    let diagnostics = if per_unit {
                        align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), &source)
                            .diags
                    } else {
                        align_driver::check(&mut sources, entry.to_str().unwrap(), &source).diags
                    };
                    let rendered = align_driver::format_diagnostics(&sources, &diagnostics);
                    assert!(
                        diagnostics.has_errors() && rendered.contains(expected),
                        "{index}/{per_unit}: {rendered}"
                    );
                }
            }
        },
    );
}
