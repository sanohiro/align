//! Environment decoding, nested owned results, and whole/per-unit interface parity.
mod common;
use common::*;
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::time::{Duration, Instant};

struct ChildOwner {
    child: Option<std::process::Child>,
    deadline: Instant,
}
impl ChildOwner {
    fn tick(&self) {
        assert!(
            Instant::now() + Duration::from_secs(5) < self.deadline,
            "compiled environment child exceeded work deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    fn wait(&mut self) -> std::process::ExitStatus {
        loop {
            self.tick();
            match self.child.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    self.child.take();
                    return status;
                }
                Ok(None) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("poll compiled environment child: {error}"),
            }
        }
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // This generated program only accesses its environment and never launches descendants.
        loop {
            match child.kill() {
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => break,
            }
        }
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => {
                    eprintln!("compiled environment child did not reap before deadline");
                    return;
                }
            }
        }
    }
}

#[test]
fn environment_results_preserve_text_errors_and_owned_control_flow() {
    if !backend_available() {
        return;
    }
    let helper = r#"module helper
import std.env
pub fn read(name: str) -> Result<Option<string>, Error> = env.get(name)
pub fn generic<T>(marker: T, name: str) -> Result<Option<string>, Error> = env.get(name)
pub fn checked(name: str) -> Result<string, Error> {
  optional := env.get(name)?
  return Ok(optional else { "fallback".clone() })
}
"#;
    let main = r#"import std.env
import helper
fn keep(error: Error) -> Error = error
fn invalid(name: str) -> bool {
  return match env.get(name) {
    Err(error) => match error { Invalid => true, _ => false },
    Ok(value) => false,
  }
}
fn operand(stop: bool) -> Result<Option<string>, Error> {
  return env.get({
    if stop { return Err(Error.Invalid) }
    "ALIGN_ENV_TEXT_GOOD"
  })
}
fn run() -> Result<(), Error> {
  name := "ALIGN_ENV_TEXT_GOOD".clone()
  owned := arena { helper.generic(true, name)? }
  first := owned else { return Err(Error.Code(10)) }
  if first != "original日本語" || name != "ALIGN_ENV_TEXT_GOOD" { return Err(Error.Code(11)) }
  callback := helper.read
  mut replacement := callback(name)?
  replacement = helper.read(name).map_err(keep)?
  replaced := replacement else { return Err(Error.Code(12)) }
  if replaced != first { return Err(Error.Code(13)) }
  joined := if true { helper.read(name) } else { helper.read("ALIGN_ENV_TEXT_MISSING") }
  selected := match joined { Ok(value) => value, Err(error) => { return Err(error) } }
  loop_value := loop { break selected }
  retained := loop_value else { return Err(Error.Code(14)) }
  if retained != first { return Err(Error.Code(15)) }
  env.set(name, "changed")?
  if first != "original日本語" || retained != first { return Err(Error.Code(16)) }
  if helper.checked(name)? != "changed" { return Err(Error.Code(17)) }
  if helper.checked("ALIGN_ENV_TEXT_MISSING")? != "fallback" { return Err(Error.Code(18)) }
  if helper.checked("ALIGN_ENV_TEXT_EMPTY")? != "" { return Err(Error.Code(19)) }
  if !invalid("ALIGN_ENV_TEXT_BAD") || !invalid("") || !invalid("a=b") || !invalid("a\0b") {
    return Err(Error.Code(20))
  }
  propagated := match helper.checked("ALIGN_ENV_TEXT_BAD").map_err(keep) {
    Err(error) => match error { Invalid => true, _ => false },
    Ok(value) => false,
  }
  if !propagated { return Err(Error.Code(21)) }
  recovered := helper.read("ALIGN_ENV_TEXT_BAD") else { None }
  absent := match recovered { None => true, Some(value) => false }
  if !absent { return Err(Error.Code(22)) }
  early := match operand(true) { Err(error) => true, Ok(value) => false }
  if !early { return Err(Error.Code(23)) }
  actual := operand(false)? else { return Err(Error.Code(24)) }
  if actual != "changed" { return Err(Error.Code(25)) }
  discarded := helper.read(name)
  return Ok(())
}
pub fn main() -> i32 {
  return match run() {
    Ok(value) => 42,
    Err(error) => match error { Code(code) => code as i32, _ => 2 },
  }
}
"#;
    let stage = align_driver::ArtifactStage::temp("environment-text")
        .expect("exclusive environment fixture");
    std::fs::write(stage.path().join("helper.align"), helper).unwrap();
    let entry = stage.path().join("main.align");
    std::fs::write(&entry, main).unwrap();
    for per_unit in [false, true] {
        let mut sm = SourceMap::new();
        let programs = if per_unit {
            let walk = build_per_unit(&mut sm, entry.to_str().unwrap(), main);
            assert!(
                !walk.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &walk.diags)
            );
            let encoded = align_interface::serialize(
                &walk
                    .units
                    .iter()
                    .find(|unit| unit.unit == "helper")
                    .unwrap()
                    .summary,
            );
            let decoded =
                align_interface::deserialize(&encoded).expect("decode environment interface");
            let named = |path: &str, args| align_interface::IType::Named {
                path: path.to_owned(),
                args,
            };
            let expected = named(
                "Result",
                vec![
                    named("Option", vec![named("string", Vec::new())]),
                    named("Error", Vec::new()),
                ],
            );
            for name in ["read", "generic"] {
                let function = decoded
                    .fns
                    .iter()
                    .find(|function| function.name == name)
                    .unwrap();
                assert_eq!(
                    function.ret, expected,
                    "{name}: serialized nested owned result"
                );
            }
            assert_eq!(align_interface::serialize(&decoded), encoded);
            walk.units
                .into_iter()
                .map(|unit| unit.mir)
                .collect::<Vec<_>>()
        } else {
            let checked = check(&mut sm, entry.to_str().unwrap(), main);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &checked.diags)
            );
            vec![lower_to_mir(&checked.hir)]
        };
        let mut objects = Vec::new();
        let mut libraries = Vec::new();
        for (index, mir) in programs.iter().enumerate() {
            let object = stage.path().join(format!("{per_unit}-{index}.o"));
            emit_object_file(
                mir,
                &object,
                BuildTarget::Baseline,
                Profile::Release,
                &[],
                false,
            )
            .expect("environment codegen");
            objects.push(object);
            for library in &mir.link_libs {
                if !libraries.contains(library) {
                    libraries.push(library.clone());
                }
            }
        }
        let binary = stage.path().join(format!("environment-{per_unit}"));
        let references: Vec<_> = objects.iter().map(|p| p.as_path()).collect();
        link_objects(
            &align_driver::CDriver::default(),
            &references,
            &binary,
            &libraries,
            Profile::Release,
        )
        .expect("environment link");
        let mut child = ChildOwner {
            child: Some(
                std::process::Command::new(&binary)
                    .env("ALIGN_ENV_TEXT_GOOD", "original日本語")
                    .env("ALIGN_ENV_TEXT_EMPTY", "")
                    .env("ALIGN_ENV_TEXT_BAD", OsStr::from_bytes(b"\xff\xc0"))
                    .env_remove("ALIGN_ENV_TEXT_MISSING")
                    .stdin(std::process::Stdio::null())
                    .spawn()
                    .expect("spawn compiled environment owner"),
            ),
            deadline: Instant::now() + Duration::from_secs(20),
        };
        assert_eq!(child.wait().code(), Some(42), "{}", binary.display());
    }
}

#[test]
fn environment_get_requires_result_shape_import_and_string_operand() {
    for (prefix, body) in [
        (
            "import std.env\n",
            "fn old() -> Option<string> = env.get(\"x\")\n",
        ),
        ("import std.env\n", "fn bad() { env.get(1) }\n"),
        ("import std.env\n", "fn bad() { env.get() }\n"),
        ("import std.env\n", "fn bad() { env.get(\"x\", \"y\") }\n"),
        ("", "fn bad() { env.get(\"x\") }\n"),
        (
            "import std.env\n",
            "fn query(n: i64) -> i64 { observed := env.get(\"x\"); return n }\npub fn main() -> i64 = [1,2].par_map(query).sum()\n",
        ),
    ] {
        let diagnostics =
            check_diagnostics("environment-invalid-source", &format!("{prefix}{body}"));
        assert!(!diagnostics.is_empty(), "unexpected admission: {body}");
        if body.contains("par_map") {
            assert!(diagnostics.to_lowercase().contains("pure"), "{diagnostics}");
        }
    }
}
