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
    let output = std::process::Command::new(binary).current_dir(stage).output()?;
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
import std.fs
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
pub fn preset<T>(tag:T, data:slice<u8>)->Result<T,Error> {
  z:=compress.zlib_compress(data,6,data)?
  r:=compress.deflate_raw_compress(data,6,data)?
  s:=compress.zstd_compress(data,3,data)?
  a:=compress.zlib_decompress(z.bytes(),data.len(),data)?
  b:=compress.deflate_raw_decompress(r.bytes(),data.len(),data)?
  c:=compress.zstd_decompress(s.bytes(),data.len(),data)?
  if a.len()!=data.len() || b.bytes()[1]!=data[1] || c.bytes()[0]!=data[0] { return Err(Error.Invalid) }
  return Ok(tag)
}
pub fn early(flag:bool)->Result<i64,Error> {
  b:=compress.zlib_decompress("",-1,{ if flag { return Ok(19) }; "seed" })?
  return Ok(b.len())
}
"#;
fn stream_model() -> String {
    let mut source = MODEL.to_owned();
    for codec in ["gzip", "zlib", "deflate_raw", "zstd"] {
        let (dict, limit) = if codec == "gzip" { ("", "") } else { (", data", ", data.len(), data") };
        source.push_str(&format!(r#"
pub fn {codec}_stream<T>(tag:T, data:slice<u8>)->Result<T,Error> {{
  input:=fs.open("stream-input")?
  packed:=fs.create("stream-packed")?
  size:=compress.{codec}_compress_to(input,packed,3{dict})?
  packed.flush()?
  mut scratch:=buffer.try_new(16)?
  eof:=input.read(scratch)?
  if size<=0 || eof!=0 {{ return Err(Error.Invalid) }}
  encoded:=fs.open("stream-packed")?
  result:=fs.create("stream-decoded")?
  bytes:=compress.{codec}_decompress_to(encoded,result{limit})?
  result.flush()?
  decoded:=fs.open("stream-decoded")?
  read:=decoded.read(scratch)?
  if read!=4 || bytes!=4 || scratch.bytes()[1]!=255 {{ return Err(Error.Invalid) }}
  return Ok(tag)
}}
"#));
    }
    source
}
const MAIN: &str = r#"import codecs
import std.fs
import std.compress
import std.encoding
fn keep(error:Error)->Error=error
fn main()->Result<(),Error> {
  input:=encoding.hex_decode("00ff8041")?
  file:=fs.create("stream-input")?
  file.write(input.bytes())?
  file.flush()?
  a_tag:=codecs.gzip_stream(true,input.bytes())?
  b_tag:=codecs.zlib_stream(2,input.bytes())?
  c_tag:=codecs.deflate_raw_stream(3,input.bytes())?
  d_tag:=codecs.zstd_stream(4,input.bytes())?
  if !a_tag || b_tag+c_tag+d_tag!=9 { return Err(Error.Invalid) }
  tag:=codecs.preset(7,input.bytes())?
  early:=codecs.early(true)?
  if tag!=7 || early!=19 { return Err(Error.Invalid) }
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
fn limits_whole_per_unit_and_cached_lifecycle() {
    owned_fixture::run("limits_whole_per_unit_and_cached_lifecycle", |stage| {
        let result = (|| -> TestResult {
            let mut model = String::from("module codecs\nimport std.compress\n");
            let mut main = String::from(
                "import codecs\nimport std.compress\nfn keep(error:Error)->Error=error\nfn mark(borrow mut trace:i64, digit:i64)->i64 { trace=trace*10+digit; return 0 }\n",
            );
            let mut calls = String::new();
            for codec in ["gzip", "zlib", "deflate_raw", "zstd"] {
                model.push_str(&format!(
                    r#"
pub fn {codec}<T>(tag:T, limit:i64)->Result<buffer,Error> {{
  packed:=compress.{codec}_compress("left",6)?
  return compress.{codec}_decompress(packed.bytes(),limit)
}}
"#
                ));
                main.push_str(&format!(r#"
fn {codec}_exit(flag:bool)->Result<i64,Error> {{
  packed:=compress.{codec}_compress("left",6)?
  result:=compress.{codec}_decompress(packed.bytes(),if flag {{ return Ok(17) }} else {{ 4 }})?
  return Ok(result.len())
}}
fn {codec}_check()->Result<(),Error> {{
  mut result:=arena {{ codecs.{codec}(false,4)? }}
  if result.bytes().as_str()?!="left" {{ return Err(Error.Invalid) }}
  result=codecs.{codec}(1,5).map_err(keep)?
  if result.len()!=4 {{ return Err(Error.Invalid) }}
  match codecs.{codec}(false,3) {{
    Ok(_) => {{ return Err(Error.Invalid) }},
    Err(error) => match error {{ Invalid => {{}}, _ => {{ return Err(error) }} }},
  }}
  packed:=compress.{codec}_compress("left",6)?
  mut trace:=0
  ordered:=compress.{codec}_decompress(packed.bytes()[mark(trace,1)..],4+mark(trace,2))?
  if trace!=12 || ordered.len()!=4 {{ return Err(Error.Invalid) }}
  captured:=4
  limit:=fn {{ captured }}
  indirect:=compress.{codec}_decompress(packed.bytes(),limit())?
  if indirect.len()!=4 {{ return Err(Error.Invalid) }}
  default:=compress.{codec}_decompress(packed.bytes())?
  explicit:=compress.{codec}_decompress(packed.bytes(),1073741824)?
  if default.bytes().as_str()?!=explicit.bytes().as_str()? {{ return Err(Error.Invalid) }}
  empty:=compress.{codec}_compress("",6)?
  zero:=compress.{codec}_decompress(empty.bytes(),0)?
  if zero.len()!=0 {{ return Err(Error.Invalid) }}
  mut index:=0
  loop {{
    if index==3 {{ break }}
    result=compress.{codec}_decompress(packed.bytes(),match index {{ 0=>4, _=>5 }}) else {{ return Err(Error.Invalid) }}
    index=index+1
  }}
  if result.len()!=4 || {codec}_exit(true)?!=17 || {codec}_exit(false)?!=4 {{ return Err(Error.Invalid) }}
  return Ok(())
}}
"#));
                calls.push_str(&format!("{codec}_check()?\n"));
            }
            main.push_str(&format!(
                "fn main()->Result<(),Error> {{\n{calls}return Ok(())\n}}\n"
            ));
            std::fs::write(stage.join("codecs.align"), model)?;
            for per_unit in [false, true] {
                execute(
                    stage,
                    &programs(stage, &main, per_unit)?,
                    if per_unit {
                        "limits-units"
                    } else {
                        "limits-whole"
                    },
                    false,
                )?;
            }
            let entry = stage.join("main.align");
            let cache = align_driver::CacheContext::at(stage.join("limits-cache"));
            for (round, hit) in [(0, false), (1, true)] {
                let mut sources = SourceMap::new();
                let mut built = align_driver::build_package(
                    &mut sources,
                    entry.to_str().ok_or("entry path")?,
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
                assert!(built.units.iter().all(|unit| {
                    unit.frontend
                        .as_ref()
                        .is_some_and(|frontend| frontend.hit == hit)
                }));
                let lowered = (0..built.units.len())
                    .map(|index| built.materialize(index).cloned())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| format!("{error:?}"))?;
                execute(stage, &lowered, &format!("limits-cache-{round}"), false)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}

// ELF symbol wrapping observes standard handles and provider contexts on real exit paths.
#[cfg(target_os = "linux")]
#[test]
fn stream_standard_handle_cleanup() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    owned_fixture::run("stream_standard_handle_cleanup", |stage| {
        let result = (|| -> TestResult {
            let source = common::fixture("crates/align_driver/tests/fixtures/stream_codec_lifecycle.align");
            let c = stage.join("observe.c");
            let observer = stage.join("observe.o");
            std::fs::write(&c, common::fixture("crates/align_driver/tests/fixtures/stream_codec_lifecycle.c"))?;
            assert!(Command::new("cc").arg("-c").arg(c).arg("-o").arg(&observer).status()?.success());
            let wrapper = stage.join("link-observer");
            let symbols = ["align_rt_io_reader_stdin", "align_rt_io_reader_free", "align_rt_io_writer_std",
                "align_rt_io_writer_free", "deflateInit2_", "inflateInit2_", "deflateEnd", "inflateEnd",
                "deflateSetDictionary", "ZSTD_createCCtx", "ZSTD_freeCCtx", "ZSTD_createDStream",
                "ZSTD_freeDStream", "ZSTD_CCtx_loadDictionary"];
            let flags = symbols.iter().map(|name| format!("-Wl,--wrap={name}")).collect::<Vec<_>>().join(" ");
            std::fs::write(&wrapper, format!("#!/bin/sh\nexec cc {flags} \"$@\"\n"))?;
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))?;
            let program = programs(stage, source, false)?.remove(0);
            let object = stage.join("exercise.o");
            align_driver::emit_object_file(&program, &object, BuildTarget::Baseline, Profile::Release, &[], false)?;
            let output = stage.join("exercise");
            align_driver::link_objects(&align_driver::CDriver::explicit(wrapper)?, &[&object, &observer], &output, &program.link_libs, Profile::Release)?;
            let run = Command::new(output).current_dir(stage).output()?;
            assert!(run.status.success(), "{run:?}");
            assert!(run.stdout.starts_with(&[0x1f, 0x8b]), "{run:?}");
            Ok(())
        })();
        if let Err(error) = result { panic!("{error}") }
    });
}

#[test]
fn stream_numeric_preconditions_abort_before_io() {
    use std::os::unix::process::ExitStatusExt;
    owned_fixture::run("stream_numeric_preconditions_abort_before_io", |_| {
        for (method, number, diagnostic) in [
            ("gzip_compress_to", 10, "level out of range"),
            ("deflate_raw_compress_to", -1, "level out of range"),
            ("zstd_compress_to", 23, "level out of range"),
            ("zlib_decompress_to", -1, "max_bytes must be nonnegative"),
        ] {
            let source = format!("import std.compress\nimport std.io\nfn main()->Result<(),Error> {{ n:=compress.{method}(io.stdin,io.stdout,{number})?; return Ok(()) }}\n");
            let output = common::build_and_run("stream-numeric", &source);
            assert_eq!(output.status.signal(), Some(libc::SIGABRT));
            assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic), "{output:?}");
            assert!(output.stdout.is_empty());
        }
    });
}

#[test]
fn stream_source_argument_domain_and_handle_liveness() {
    owned_fixture::run("stream_source_argument_domain_and_handle_liveness", |stage| {
        for (call, accepted) in [
            ("compress.gzip_compress_to(r,w,3)", true),
            ("compress.gzip_compress_to(io.stdin,io.stdout,3)", true),
            ("compress.zstd_decompress_to(r,io.stderr,17,\"dictionary\")", true),
            ("compress.zlib_compress_to(r,w,take_r(other))", true),
            ("compress.zlib_compress_to(r,w,take_r(r))", false),
            ("compress.zlib_compress_to(r,w,take_w(w))", false),
            ("compress.zstd_decompress_to(r,w,4,{ take_w(w); \"dictionary\" })", false),
            ("compress.zlib_compress_to(r,w,retire(conn))", false),
            ("compress.gzip_compress_to(w,r,3)", false),
            ("compress.gzip_compress_to(r,w)", false),
            ("compress.gzip_decompress_to(r,w,3,\"seed\")", false),
            ("compress.zstd_decompress_to(r,w,true)", false),
            ("compress.zlib_decompress_to(r,w,3,4)", false),
            ("compress.zlib_compress_to(fs.open(\"x\")?,w,4)", false),
            ("compress.zlib_compress_to(r,io.stdout.buffered(),4)", false),
        ] {
            let source = format!("import std.compress\nimport std.io\nimport std.fs\nimport std.net\nfn take_r(p:reader)->i64=3\nfn take_w(p:writer)->i64=3\nfn retire(c:tcp_conn)->i64=3\nfn f(other:reader)->Result<i64,Error> {{ conn:=tcp.connect(\"localhost\",80)?; r:=conn.reader(); w:=conn.writer(); return {call} }}\nfn main(){{}}\n");
            let path = stage.join("main.align");
            std::fs::write(&path, &source).unwrap();
            let entry = path.to_str().unwrap();
            for partition in [false, true] {
                let mut sources = SourceMap::new();
                let diag = if partition {
                    align_driver::build_per_unit(&mut sources, entry, &source).diags
                } else {
                    align_driver::check(&mut sources, entry, &source).diags
                };
                assert_eq!(!diag.has_errors(), accepted, "{call}: {}", align_driver::format_diagnostics(&sources, &diag));
            }
        }
    });
}

#[test]
fn limits_source_domain_and_input_liveness() {
    owned_fixture::run("limits_source_domain_and_input_liveness", |stage| {
        for codec in ["gzip", "zlib", "deflate_raw", "zstd"] {
            for limit in [
                "true",
                "1.0",
                "\"x\"",
                "1 as i32",
                "1 as u64",
                "missing_name",
            ] {
                let source = format!(
                    "import std.compress\nfn main()->Result<(),Error> {{ b:=compress.{codec}_decompress(\"\",{limit})?; return Ok(()) }}\n"
                );
                let mut sources = SourceMap::new();
                let checked = align_driver::check(&mut sources, "limit-domain", &source);
                assert!(checked.diags.has_errors(), "{source}");
            }
            for mutation in ["input=buffer(0)", "consumed:=input", "input.append(\"x\")"] {
                let mut calls = vec![format!("compress.{codec}_decompress(input.bytes(),{{ {mutation}; 4 }})")];
                if codec != "gzip" {
                    for action in ["compress", "decompress"] {
                        calls.push(format!("compress.{codec}_{action}(input.bytes(),4,{{ {mutation}; \"seed\" }})"));
                    }
                }
                for call in calls {
                    let source = format!("import std.compress\nfn main()->Result<(),Error> {{ mut input:=buffer(16); b:={call}?; return Ok(()) }}\n");
                    let path = stage.join("main.align");
                    std::fs::write(&path, &source).unwrap_or_else(|error| panic!("{error}"));
                    for per_unit in [false, true] {
                        let mut sources = SourceMap::new();
                        let entry = path.to_str().unwrap_or("invalid fixture path");
                        let diags = if per_unit {
                            align_driver::build_per_unit(&mut sources, entry, &source).diags
                        } else {
                            align_driver::check(&mut sources, entry, &source).diags
                        };
                        assert!(diags.has_errors(), "input invalidation accepted: {source}");
                    }
                }
            }
        }
    });
}

#[test]
fn preset_dictionary_expression_cannot_grow_an_earlier_views_owner() {
    owned_fixture::run("preset_alias_liveness", |stage| {
        for (argument, accepted) in [("other", true), ("input", false)] {
            let source = format!(r#"import std.compress
fn grow(borrow mut owner:buffer)->str {{ owner.append("longer"); return "dictionary" }}
fn main()->Result<(),Error> {{
  mut input:=buffer(16)
  mut other:=buffer(16)
  input.append("source")
  saved:=input.bytes()
  packed:=compress.zlib_compress(saved,6,grow({argument}))?
  return Ok(())
}}
"#);
            let path = stage.join("main.align");
            std::fs::write(&path, &source).unwrap();
            let entry = path.to_str().unwrap();
            for per_unit in [false, true] {
                let mut sources = SourceMap::new();
                let diags = if per_unit {
                    align_driver::build_per_unit(&mut sources, entry, &source).diags
                } else {
                    align_driver::check(&mut sources, entry, &source).diags
                };
                let report = align_driver::format_diagnostics(&sources, &diags);
                assert_eq!(!diags.has_errors(), accepted, "{source}\n{report}");
                if !accepted { assert!(report.contains("invalidated"), "{report}"); }
            }
        }
    });
}

#[test]
fn limits_abort_on_invalid_domain_before_decoding() {
    owned_fixture::run("limits_abort_on_invalid_domain_before_decoding", |stage| {
        use std::os::unix::process::ExitStatusExt;
        for codec in ["gzip", "zlib", "deflate_raw", "zstd"] {
            let source = format!(
                r#"import std.compress
fn main(args:array<str>)->Result<(),Error> {{
  limit:=args[1].parse_i64()?
  decoded:=compress.{codec}_decompress("invalid stream",limit)?
  return Ok(())
}}
"#
            );
            let result = (|| -> TestResult {
                let compiled = programs(stage, &source, false)?;
                let object = stage.join("abort.o");
                align_driver::emit_object_file(
                    &compiled[0],
                    &object,
                    BuildTarget::Baseline,
                    Profile::Release,
                    &[],
                    false,
                )?;
                let binary = stage.join("limit-abort");
                align_driver::link_objects(
                    &align_driver::CDriver::default(),
                    &[object.as_path()],
                    &binary,
                    &compiled[0].link_libs,
                    Profile::Release,
                )?;
                for limit in ["-1", "1073741825"] {
                    let output = std::process::Command::new(&binary).arg(limit).output()?;
                    assert_eq!(
                        output.status.signal(),
                        Some(libc::SIGABRT),
                        "{codec}/{limit}: {output:?}"
                    );
                    assert!(
                        String::from_utf8_lossy(&output.stderr).contains("max_bytes out of range"),
                        "{output:?}"
                    );
                }
                Ok(())
            })();
            if let Err(error) = result {
                panic!("{error}");
            }
        }
    });
}
#[test]
fn framing_whole_per_unit_and_cached_lifecycle() {
    owned_fixture::run("framing_whole_per_unit_and_cached_lifecycle", |stage| {
        let result = (|| -> TestResult {
            std::fs::write(stage.join("codecs.align"), stream_model())?;
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
            (format!("compress.{codec}_decompress(\"x\",6,0)"), false),
            (format!("compress.{codec}_compress(\"x\",6,\"seed\")"), codec != "gzip"),
            (format!("compress.{codec}_decompress(\"x\",6,\"seed\")"), codec != "gzip"),
            (format!("compress.{codec}_compress(\"x\",6,true)"), false),
            (format!("compress.{codec}_decompress(\"x\",\"seed\")"), false),
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
