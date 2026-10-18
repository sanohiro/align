//! Plan226: imported framing methods, source admission, ownership and cache parity.
#![cfg(unix)]
mod common;
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

const MODEL: &str = r#"module codecs
import std.compress
pub fn identity<T>(value:T)->T=value
pub fn zlib(data:slice<u8>)->Result<buffer,Error> {
  packed:=compress.zlib_compress(data,6)?
  decoded:=compress.zlib_decompress(packed.bytes())?
  return Ok(identity(decoded))
}
pub fn raw(data:slice<u8>)->Result<buffer,Error> {
  packed:=compress.deflate_raw_compress(data,6)?
  decoded:=compress.deflate_raw_decompress(packed.bytes())?
  return Ok(identity(decoded))
}
"#;
const MAIN: &str = r#"import codecs
import std.compress
import std.encoding
fn keep(error:Error)->Error=error
fn main()->Result<(),Error> {
  input:=encoding.hex_decode("00ff8041")?
  mut decoded:=codecs.zlib(input.bytes()).map_err(keep)?
  if decoded.len()!=4 || decoded.bytes()[1]!=255 { return Err(Error.Invalid) }
  decoded=codecs.raw(input.bytes())?
  if decoded.len()!=4 || decoded.bytes()[2]!=128 || input.len()!=4 { return Err(Error.Invalid) }
  detached:=arena { codecs.zlib(input.bytes())? }
  if detached.bytes()[3]!=65 { return Err(Error.Invalid) }
  zlib:=encoding.hex_decode("789ccb494d2b0100042301ac")?
  raw:=encoding.hex_decode("cb494d2b0100")?
  a:=compress.zlib_decompress(zlib.bytes())?
  b:=compress.deflate_raw_decompress(raw.bytes())?
  if a.bytes().as_str()?!="left" || b.bytes().as_str()?!="left" { return Err(Error.Invalid) }
  bad_zlib:=encoding.hex_decode("789ccb494d2b0100042301ac00")?
  bad_raw:=encoding.hex_decode("cb494d2b01000300")?
  match compress.zlib_decompress(bad_zlib.bytes()).map_err(keep) {
    Ok(_) => { return Err(Error.Invalid) },
    Err(error) => match error { Invalid => {}, _ => { return Err(error) } },
  }
  match compress.deflate_raw_decompress(bad_raw.bytes()) {
    Ok(_) => { return Err(Error.Invalid) },
    Err(error) => match error { Invalid => {}, _ => { return Err(error) } },
  }
  return Ok(())
}
"#;
#[test]
fn framing_whole_per_unit_and_cached_lifecycle() {
    owned_fixture::run("framing_whole_per_unit_and_cached_lifecycle", |stage| {
        let result = (|| -> TestResult {
            std::fs::write(stage.join("codecs.align"), MODEL)?;
            for per_unit in [false, true] {
                execute(
                    stage,
                    &programs(stage, MAIN, per_unit)?,
                    if per_unit { "units" } else { "whole" },
                    false,
                )?;
            }
            let entry = stage.join("main.align");
            let cache = align_driver::CacheContext::at(stage.join("cache"));
            let mut first = Vec::new();
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
                assert!(built.units.iter().all(|unit| {
                    unit.frontend
                        .as_ref()
                        .is_some_and(|frontend| frontend.hit == hit)
                }));
                let lowered = (0..built.units.len())
                    .map(|index| built.materialize(index).cloned())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| format!("{error:?}"))?;
                let keys = lowered
                    .iter()
                    .map(align_interface::codegen_impl_hash)
                    .collect::<Vec<_>>();
                if hit {
                    assert_eq!(keys, first);
                } else {
                    first = keys;
                }
                execute(stage, &lowered, &format!("cache-{round}"), false)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}

#[test]
fn framing_source_domain_and_effects() {
    for codec in ["zlib", "deflate_raw"] {
        for (call, expected) in [
            (format!("compress.{codec}_compress(\"x\",6)"), true),
            (format!("compress.{codec}_decompress(\"x\")"), true),
            (format!("compress.{codec}_compress(\"x\".clone(),6)"), true),
            (format!("compress.{codec}_compress(\"x\")"), false),
            (format!("compress.{codec}_compress(\"x\",1.0)"), false),
            (format!("compress.{codec}_compress(\"x\",1 as i32)"), false),
            (format!("compress.{codec}_compress(true,6)"), false),
            (format!("compress.{codec}_decompress(1)"), false),
            (format!("compress.{codec}_decompress(\"x\",6)"), false),
        ] {
            let source = format!(
                "import std.compress\nfn main()->Result<(),Error> {{ result:={call}?; return Ok(()) }}\n"
            );
            let mut sources = SourceMap::new();
            let checked = align_driver::check(&mut sources, "framing-domain", &source);
            assert_eq!(
                !checked.diags.has_errors(),
                expected,
                "{source}\n{}",
                align_driver::format_diagnostics(&sources, &checked.diags)
            );
        }
        let source = format!(
            "fn main()->Result<(),Error> {{ result:=compress.{codec}_decompress(\"x\")?; return Ok(()) }}\n"
        );
        assert!(common::check_errs("framing-import", &source));
        let source = format!(
            "import std.compress\nfn f(x:i64)->i64 {{ b:=compress.{codec}_compress(\"x\",6) else {{ return x }}; return b.len() }}\nfn main() {{ arena {{ result:=[1,2][0..2].par_map(f).to_array(); print(result.len()) }} }}\n"
        );
        let diagnostics = common::check_diagnostics("framing-effect", &source);
        assert!(
            diagnostics.contains("pure") || diagnostics.contains("Pure"),
            "{diagnostics}"
        );
    }
}

#[test]
fn framing_levels_abort_before_native_work() {
    owned_fixture::run("framing_levels_abort_before_native_work", |_| {
        use std::os::unix::process::ExitStatusExt;
        for codec in ["gzip", "zlib", "deflate_raw"] {
            for level in ["-1", "10", "9223372036854775807", "-9223372036854775808"] {
                let source = format!(
                    "import std.compress\nfn main()->Result<(),Error> {{ b:=compress.{codec}_compress(\"\",{level})?; return Ok(()) }}\n"
                );
                let output = common::build_and_run("framing-level", &source);
                assert_eq!(
                    output.status.signal(),
                    Some(libc::SIGABRT),
                    "{codec}/{level}: {output:?}"
                );
                assert!(
                    String::from_utf8_lossy(&output.stderr)
                        .contains(&format!("compress.{codec}_compress: level out of range")),
                    "{output:?}"
                );
            }
        }
    });
}
