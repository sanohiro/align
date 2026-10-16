//! Plan206: one synchronous CLI parser for array and slice argv carriers.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::{Path, PathBuf};

fn compile(stage: &Path, programs: &[align_mir::Program], label: &str) -> PathBuf {
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
    binary
}

const MODEL: &str = r#"module model
import std.cli
pub fn identity<T>(value: T) -> T = value
pub fn count(args: slice<str>) -> Result<i64, Error> {
  c := cli.command("count")
  c.flag_i64("count", 7)
  p := c.parse(identity(args))?
  return Ok(p.get_i64("count"))
}
"#;

// AM-R10's direct subcommand path, with imported helpers and parser observations.
const MAIN: &str = r#"module main
import model
import std.cli
fn early(args: slice<str>) -> Result<i64, Error> {
  c := cli.command("early")
  _ := c.parse({ return Ok(9); args })?
  return Err(Error.Invalid)
}
fn failure() -> Result<slice<str>, Error> = Err(Error.Invalid)
fn propagate() -> Result<(), Error> {
  c := cli.command("propagate")
  _ := c.parse(failure()?)?
  return Ok(())
}
pub fn main(args: array<str>) -> Result<(), Error> {
  if args.len() < 2 { return Err(Error.Invalid) }
  command := cli.command("media video")
  command.flag_i64("segments", 3)
  command.flag_str("text", "default")
  command.flag_bool("verbose")
  parsed := match command.parse(args[1..]) {
    Ok(value) => value,
    Err(error) => { print(match error { Invalid => 1, _ => 2 }); return Ok(()) }
  }
  print(parsed.get_i64("segments"))
  print(parsed.get_str("text"))
  print(parsed.occurrences("segments"))
  print(parsed.occurrences("verbose"))
  // Prefix and suffix would fail if the runtime read outside the selected span.
  fixed := ["bad-prefix", "subcommand", "--count=11", "bad-suffix"]
  outer := fixed[1..]
  inner := outer[..2]
  if model.count(inner)? != 11 { return Err(Error.Invalid) }
  if model.count(fixed[2..2])? != 7 { return Err(Error.Invalid) }
  if model.count(fixed[1..2])? != 7 { return Err(Error.Invalid) }
  mut turn := 0
  loop {
    if turn == 2 { break }
    selected := if turn == 0 { inner } else { fixed[1..3] }
    chosen := match turn { 0 => selected, _ => inner }
    if model.count(chosen)? != 11 { return Err(Error.Invalid) }
    turn = turn + 1
  }
  if early(inner)? != 9 { return Err(Error.Invalid) }
  match propagate() {
    Err(error) => match error { Invalid => (), _ => { return Err(Error.Invalid) } },
    Ok(_) => { return Err(Error.Invalid) }
  }
  // Parsed owns text, independently of both the argv headers and their text owner.
  mut c := cli.command("inner")
  c.flag_str("text", "")
  mut owned := "independent\u{0}text".clone()
  view: str := owned
  mut words := ["inner", "--text", view].to_array()
  independent := c.parse(words[..])?
  words = ["replacement"].to_array()
  owned = "replacement".clone()
  c = cli.command("replacement")
  if independent.get_str("text") != "independent\u{0}text" { return Err(Error.Invalid) }
  arena_parsed := arena scratch {
    arena_command := cli.command("arena")
    arena_command.flag_str("text", "")
    mut arena_words: array_builder<str> := array_builder(scratch)
    arena_words.push("arena")
    arena_words.push("--text=arena-owned-input")
    argv := arena_words.build()
    arena_command.parse(argv[..])?
  }
  if arena_parsed.get_str("text") != "arena-owned-input" { return Err(Error.Invalid) }
  return Ok(())
}
"#;

fn run_cases(binary: &Path) {
    for (args, expected) in [
        (vec!["video"], "3\ndefault\n0\n0\n"),
        (
            vec![
                "video",
                "--segments=4",
                "--segments",
                "8",
                "--text",
                "--verbose",
                "--verbose",
            ],
            "8\n--verbose\n2\n1\n",
        ),
        (vec!["video", "--text="], "3\n\n0\n0\n"),
        (vec!["video", "--segments"], "1\n"),
        (vec!["video", "--segments=bad"], "1\n"),
        (vec!["video", "--unknown"], "1\n"),
        (vec!["video", "positional"], "1\n"),
    ] {
        let output = std::process::Command::new(binary)
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert_eq!(output.stdout, expected.as_bytes(), "{args:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
    }
}

#[test]
fn cli_slices_execute_and_replay() {
    owned_fixture::run("cli_slices_execute_and_replay", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, MAIN).unwrap();
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, entry.to_str().unwrap(), MAIN);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        run_cases(&compile(stage, &[program], "whole"));
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), MAIN);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &built.diags)
        );
        run_cases(&compile(
            stage,
            &built
                .units
                .into_iter()
                .map(|unit| unit.mir)
                .collect::<Vec<_>>(),
            "units",
        ));
        let cache = align_driver::CacheContext::at(stage.join("cache"));
        for round in 0..2 {
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sources,
                entry.to_str().unwrap(),
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
                built
                    .units
                    .iter()
                    .all(|unit| unit.frontend.as_ref().unwrap().hit == (round == 1))
            );
            let programs = (0..built.units.len())
                .map(|index| built.materialize(index).unwrap().clone())
                .collect::<Vec<_>>();
            if round == 1 {
                run_cases(&compile(stage, &programs, "cache-hit"));
            }
        }
    });
}

#[test]
fn cli_slices_reject_invalid_types_and_lifetimes() {
    owned_fixture::run("cli_slices_reject_invalid_types_and_lifetimes", |stage| {
        for (name, body, expected) in [
            (
                "fixed",
                "_:=c.parse([\"program\"])?",
                "takes `array<str>` or `slice<str>`",
            ),
            (
                "integer-slice",
                "data:=[1,2];_:=c.parse(data[..])?",
                "got slice<i64>",
            ),
            (
                "integer-array",
                "data:=[1,2].to_array();_:=c.parse(data)?",
                "got array<i64>",
            ),
            (
                "owned-text",
                "data:=[\"program\".clone()];_:=c.parse(data[..])?",
                "got slice<string>",
            ),
            ("arity", "_:=c.parse()?", "takes 1 argument"),
            (
                "temporary",
                "_:=cli.command(\"other\").parse(args[..])?",
                "bind the cli command",
            ),
            (
                "receiver",
                "_:=c.parse({c=cli.command(\"other\");args[..]})?",
                "borrow",
            ),
            (
                "argv",
                "mut data:=[\"program\"].to_array();view:=data[..];data=[\"new\"].to_array();_:=c.parse(view)?",
                "invalid",
            ),
            (
                "text",
                "mut text:=\"old\".clone();view:str:=text;data:=[\"p\",\"--text\",view];text=\"new\".clone();_:=c.parse(data[..])?",
                "invalid",
            ),
            (
                "parsed",
                "mut p:=c.parse(args[..])?;view:=p.get_str(\"text\");p=c.parse(args[..])?;print(view)",
                "invalid",
            ),
        ] {
            let source = format!(
                "import std.cli\npub fn main(args:array<str>)->Result<(),Error>{{mut c:=cli.command(\"test\");c.flag_str(\"text\",\"\");{body};return Ok(())}}"
            );
            let entry = stage.join(format!("{name}.align"));
            std::fs::write(&entry, &source).unwrap();
            let mut sources = SourceMap::new();
            let checked = align_driver::check(&mut sources, entry.to_str().unwrap(), &source);
            let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
            assert!(
                checked.diags.has_errors() && diagnostics.contains(expected),
                "{name}: {diagnostics}"
            );
            let mut sources = SourceMap::new();
            let built =
                align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), &source);
            let diagnostics = align_driver::format_diagnostics(&sources, &built.diags);
            assert!(
                built.diags.has_errors() && diagnostics.contains(expected),
                "per-unit {name}: {diagnostics}"
            );
        }
    });
}
