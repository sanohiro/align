//! Plan181: finite flags and occurrence counts through source, native ABI and replay.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::{Path, PathBuf};

const MODEL: &str = "module model\npub fn identity<T>(value:T)->T = value\n";
const MAIN: &str = r#"module main
import model
import std.cli
import std.io
fn independent(args:array<str>)->Result<f64,Error> {
  c:=cli.command("inner")
  c.flag_f64("scale",0.5)
  p:=c.parse(args)?
  return Ok(p.get_f64("scale"))
}
fn stop_before_registration()->i64 {
  c:=cli.command("stop")
  c.flag_f64("never",{return 7; 0.0})
  return 8
}
fn stop_before_getter(args:array<str>)->Result<i64,Error> {
  c:=cli.command("stop")
  p:=c.parse(args)?
  p.get_f64({return Ok(9); "unregistered"})
  return Ok(10)
}
fn invalid_name()->Result<str,Error> = Err(Error.Invalid)
fn fail_before_count(args:array<str>)->Result<i64,Error> {
  c:=cli.command("stop")
  p:=c.parse(args)?
  return Ok(p.occurrences(invalid_name()?))
}
pub fn main(args:array<str>)->Result<(),Error> {
  if stop_before_registration()!=7 {return Err(Error.Invalid)}
  if stop_before_getter(["stop"].to_array())?!=9 {return Err(Error.Invalid)}
  match fail_before_count(["stop"].to_array()) {
    Err(error)=>match error {Invalid=>(), _=>{return Err(Error.Invalid)}},
    Ok(_)=>{return Err(Error.Invalid)}
  }
  mut c:=cli.command("test")
  c.flag_bool("verbose")
  c.flag_str("text","")
  c.flag_i64("count",3)
  c.flag_f64(model.identity("scale"),{c.flag_bool("nested");model.identity(1.0)})
  p:=match c.parse(args) {
    Ok(parsed)=>parsed,
    Err(error)=>{
      print(match error {Invalid=>1, _=>2})
      usage:=c.usage()
      io.stdout.write(usage)?
      return Ok(())
    }
  }
  // Parsed storage survives command replacement; the old owner is dropped exactly once.
  c=cli.command("replacement")
  print(p.get_f64(model.identity("scale")).to_bits())
  print(p.occurrences(model.identity("scale")))
  print(p.occurrences("verbose"))
  print(p.occurrences("text"))
  print(p.occurrences("count"))
  print(p.get_i64("count"))
  io.stdout.write(p.get_str("text"))?
  io.stdout.write("\n")?
  own:=["inner","--scale=0.25"].to_array()
  scalar:=independent(own)?
  if scalar!=0.25 {return Err(Error.Invalid)}
  // Scalar receiver loans end within an enclosing expression; text views still retain p.
  local_args:=["local"].to_array()
  local_command:=cli.command("local")
  local_command.flag_f64("s",0.25)
  mut local_parsed:=local_command.parse(local_args)?
  sum:=local_parsed.get_f64("s")+{local_parsed=local_command.parse(local_args)?;0.75}
  total:=local_parsed.occurrences("s")+{local_parsed=local_command.parse(local_args)?;2}
  if sum!=1.0 || total!=2 {return Err(Error.Invalid)}
  return Ok(())
}
"#;

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
fn run_cases(binary: &Path) {
    for (args, expected) in [
        (vec![], "4607182418800017408\n0\n0\n0\n0\n3\n\n"),
        (
            vec![
                "--scale=0.5",
                "--text",
                "--verbose",
                "--scale",
                "1.5",
                "--verbose",
                "--verbose",
                "--count=4",
                "--count",
                "9",
            ],
            "4609434218613702656\n2\n2\n1\n2\n9\n--verbose\n",
        ),
        (
            vec!["--text", "first", "--text=", "--scale=-0"],
            "9223372036854775808\n1\n0\n2\n0\n3\n\n",
        ),
    ] {
        let output = std::process::Command::new(binary)
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert_eq!(output.stdout, expected.as_bytes(), "{args:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
    }
    for bad in ["NaN", "1e9999", ".5", "1 "] {
        let output = std::process::Command::new(binary)
            .args(["--text=partial", "--scale", bad])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "1\nusage: test [flags]\n  --verbose  (bool)\n  --text  (str, default: )\n  --count  (i64, default: 3)\n  --nested  (bool)\n  --scale  (f64, default: 1)\n"
        );
        assert!(output.stderr.is_empty(), "{output:?}");
    }
}
#[test]
fn cli_admission_executes_and_replays() {
    owned_fixture::run("cli_admission_executes_and_replays", |stage| {
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
fn cli_new_getter_programmer_errors_abort() {
    use std::os::unix::process::ExitStatusExt;
    owned_fixture::run("cli_new_getter_programmer_errors_abort", |stage| {
        for (index, call, expected) in [
            (0, "p.get_f64(\"missing\")", "no such flag"),
            (1, "p.get_f64(\"int\")", "flag is not an f64"),
            (2, "p.occurrences(\"missing\")", "no such flag"),
        ] {
            let source = format!(
                "import std.cli\npub fn main(args:array<str>)->Result<(),Error>{{c:=cli.command(\"test\");c.flag_i64(\"int\",1);p:=c.parse(args)?;_:= {call};return Ok(())}}"
            );
            let entry = stage.join(format!("abort-{index}.align"));
            let mut sources = SourceMap::new();
            let checked = align_driver::check(&mut sources, entry.to_str().unwrap(), &source);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &checked.diags)
            );
            let program =
                align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
            let output =
                std::process::Command::new(compile(stage, &[program], &format!("abort-{index}")))
                    .output()
                    .unwrap();
            assert_eq!(output.status.signal(), Some(libc::SIGABRT), "{output:?}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(expected),
                "{output:?}"
            );
        }
    });
}
#[test]
fn cli_exact_types_and_bound_receivers() {
    for (name, body) in [
        ("integer", "c.flag_f64(\"scale\",1)"),
        ("f32", "value:f32:=1.0;c.flag_f64(\"scale\",value)"),
        ("bool", "c.flag_f64(\"scale\",true)"),
        ("arity", "c.flag_f64(\"scale\")"),
        ("bad-name", "c.flag_f64(1,1.0)"),
        ("temporary", "cli.command(\"x\").flag_f64(\"scale\",1.0)"),
        ("getter-name", "p:=c.parse(args)?;p.get_f64(1)"),
        ("count-arity", "p:=c.parse(args)?;p.occurrences()"),
        ("getter-temporary", "c.parse(args)?.get_f64(\"scale\")"),
        ("count-temporary", "c.parse(args)?.occurrences(\"scale\")"),
    ] {
        let source = format!(
            "import std.cli\npub fn main(args:array<str>)->Result<(),Error>{{c:=cli.command(\"test\");{body};return Ok(())}}"
        );
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, &format!("{name}.align"), &source);
        assert!(
            checked.diags.has_errors(),
            "{name} admitted invalid CLI input"
        );
    }
}

#[test]
fn cli_eager_operands_cannot_replace_or_move_receivers() {
    owned_fixture::run(
        "cli_eager_operands_cannot_replace_or_move_receivers",
        |stage| {
            let name_change = "{c=cli.command(\"other\");\"flag\"}";
            let value_change = "{c=cli.command(\"other\");1.0}";
            let mut statements = vec![
                format!("c.flag_bool({name_change})"),
                format!("c.flag_str({name_change},\"default\")"),
                format!("c.flag_i64({name_change},1)"),
                format!("c.flag_f64({name_change},1.0)"),
                format!("c.flag_f64(\"flag\",{value_change})"),
                "c.flag_str(\"flag\",{c=cli.command(\"other\");\"value\"})".into(),
                "c.flag_i64(\"flag\",{c=cli.command(\"other\");1})".into(),
                "c.flag_f64(\"flag\",{moved:=c;1.0})".into(),
                "c.parse({c=cli.command(\"other\");args})".into(),
                "c.flag_f64(\"flag\",if args.len()>0 {c=cli.command(\"other\");1.0} else {2.0})"
                    .into(),
            ];
            for method in ["get_bool", "get_str", "get_i64", "get_f64", "occurrences"] {
                statements.push(format!(
                    "mut p:=c.parse(args)?;p.{method}({{p=c.parse(args)?;\"flag\"}})"
                ));
                statements.push(format!(
                    "mut p:=c.parse(args)?;p.{method}({{moved:=p;\"flag\"}})"
                ));
            }
            statements.push(
                "mut p:=c.parse(args)?;_:=p.get_str(\"flag\")=={p=c.parse(args)?;\"\"}".into(),
            );
            statements.push(
                "c.flag_f64(\"flag\",{c.flag_bool({c=cli.command(\"other\");\"nested\"});1.0})"
                    .into(),
            );
            statements
                .push("c.flag_f64(\"flag\",loop {c=cli.command(\"other\");break 1.0})".into());
            for (index, statement) in statements.iter().enumerate() {
                let source = format!(
                    "import std.cli\npub fn main(args:array<str>)->Result<(),Error>{{mut c:=cli.command(\"test\");{statement};return Ok(())}}"
                );
                let entry = stage.join(format!("receiver-{index}.align"));
                std::fs::write(&entry, &source).unwrap();
                let mut sources = SourceMap::new();
                let checked = align_driver::check(&mut sources, entry.to_str().unwrap(), &source);
                assert!(
                    checked
                        .diags
                        .iter()
                        .any(|d| d.message.contains("invalidated before")),
                    "receiver invalidation not diagnosed: {statement}: {}",
                    align_driver::format_diagnostics(&sources, &checked.diags)
                );
                let mut sources = SourceMap::new();
                let checked =
                    align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), &source);
                assert!(
                    checked
                        .diags
                        .iter()
                        .any(|d| d.message.contains("invalidated before")),
                    "per-unit receiver invalidation not diagnosed: {statement}: {}",
                    align_driver::format_diagnostics(&sources, &checked.diags)
                );
            }
        },
    );
}
