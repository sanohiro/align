//! HTML text admission and ordinary string ownership across imported builds.
mod common;
use common::*;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct ChildOwner {
    child: Option<std::process::Child>,
    deadline: Instant,
}
impl ChildOwner {
    fn tick(&self) {
        assert!(
            Instant::now() + Duration::from_secs(5) < self.deadline,
            "compiled HTML child exceeded work deadline"
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
                Err(error) => panic!("poll compiled HTML child: {error}"),
            }
        }
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // This generated program only accesses its HTML and never launches descendants.
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
                    eprintln!("compiled HTML child did not reap before deadline");
                    return;
                }
            }
        }
    }
}

fn build(
    stage: &align_driver::ArtifactStage,
    source: &str,
    per_unit: bool,
    rt_lto: bool,
    name: &str,
) -> PathBuf {
    let entry = stage.path().join("main.align");
    std::fs::write(&entry, source).unwrap();
    let mut sm = SourceMap::new();
    let programs = if per_unit {
        let walk = build_per_unit(&mut sm, entry.to_str().unwrap(), source);
        assert!(
            !walk.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &walk.diags)
        );
        walk.units
            .into_iter()
            .map(|unit| (unit.mir, unit.is_entry))
            .collect::<Vec<_>>()
    } else {
        let checked = check(&mut sm, entry.to_str().unwrap(), source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &checked.diags)
        );
        vec![(lower_to_mir(&checked.hir), true)]
    };
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, (mir, is_entry)) in programs.iter().enumerate() {
        let object = stage.path().join(format!("{name}-{index}.o"));
        emit_object_file(
            mir,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            rt_lto,
        )
        .expect("HTML codegen");
        objects.push(object);
        for library in &mir.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
        if *is_entry {
            let ir = emit_llvm_ir(
                mir,
                BuildTarget::Baseline,
                Profile::Release,
                false,
                &[],
                rt_lto,
            )
            .expect("HTML raw IR");
            assert!(
                ir.contains("@align_rt_html_escape(ptr readonly captures(none), i64)"),
                "{ir}"
            );
        }
    }
    let executable = stage.path().join(name);
    let refs = objects.iter().map(|p| p.as_path()).collect::<Vec<_>>();
    link_objects(
        &align_driver::CDriver::default(),
        &refs,
        &executable,
        &libraries,
        Profile::Release,
    )
    .expect("HTML link");
    executable
}

fn run(stage: &align_driver::ArtifactStage, executable: &Path, id: usize) -> std::process::Output {
    let stdout = stage.path().join(format!("stdout-{id}"));
    let stderr = stage.path().join(format!("stderr-{id}"));
    let mut command = std::process::Command::new(executable);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::fs::File::create_new(&stdout).unwrap())
        .stderr(std::fs::File::create_new(&stderr).unwrap());
    let mut child = ChildOwner {
        child: Some(command.spawn().expect("spawn HTML executable")),
        deadline: Instant::now() + Duration::from_secs(20),
    };
    let status = child.wait();
    let read = |path: &Path| {
        assert!(
            std::fs::metadata(path).unwrap().len() <= 16_384,
            "unbounded HTML output"
        );
        std::fs::read(path).unwrap()
    };
    std::process::Output {
        status,
        stdout: read(&stdout),
        stderr: read(&stderr),
    }
}

#[test]
fn html_text_source_admission_is_text_only() {
    let stage = align_driver::ArtifactStage::temp("html-text-source").unwrap();
    let entry = stage.path().join("main.align");
    let prelude = "import std.encoding\nfn f(raw: slice<u8>, owned: string) -> string = ";
    for expression in [
        "encoding.html_escape(raw)",
        "encoding.html_escape(12)",
        "encoding.html_escape(true)",
        "encoding.html_escape(missing)",
        "encoding.html_escape()",
        "encoding.html_escape(owned, owned)",
        "encoding.html_escape(owned.no_such_method())",
    ] {
        let source = format!("{prelude}{expression}\nfn main() {{}}\n");
        std::fs::write(&entry, &source).unwrap();
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, entry.to_str().unwrap(), &source);
        assert!(checked.diags.has_errors(), "accepted {expression}");
        let messages = align_driver::format_diagnostics(&sm, &checked.diags);
        assert!(!messages.contains("internal compiler error"), "{messages}");
    }
}

#[test]
fn html_text_whole_and_per_unit_preserve_owned_text() {
    assert!(backend_available(), "HTML owner requires LLVM");
    let stage = align_driver::ArtifactStage::temp("html-text-execution").unwrap();
    std::fs::write(
        stage.path().join("helper.align"),
        r#"module helper
import std.encoding
pub fn identity<T>(value: T) -> T = value
pub fn escape_owned(value: string) -> string {
  escaped := encoding.html_escape(value)
  print(value)
  return identity(escaped)
}
pub fn from_bytes(raw: slice<u8>) -> Result<string, Error> {
  text := raw.as_str()?
  return Ok(encoding.html_escape(text))
}
"#,
    )
    .unwrap();
    let source = r#"import std.encoding
import helper
fn keep(error: Error) -> Error = error
fn fallback(raw: slice<u8>) -> string {
  text := raw.as_str() else { return "fallback".clone() }
  return encoding.html_escape(text)
}
pub fn main() -> Result<(), Error> {
  print(encoding.html_escape(""))
  print(helper.escape_owned("<&日本".clone()))
  print(encoding.html_escape(helper.identity("temporary &".clone())))
  mut replaced := encoding.html_escape("old")
  replaced = encoding.html_escape("new >")
  print(replaced)
  bytes := encoding.hex_decode("3c2622")?
  text := bytes.bytes().as_str()?
  print(encoding.html_escape(text))
  print(helper.from_bytes(bytes.bytes()).map_err(keep)?)
  bad := encoding.hex_decode("ff")?
  match helper.from_bytes(bad.bytes()) {
    Ok(_) => print("unexpected"),
    Err(_) => print("invalid"),
  }
  print(fallback(bad.bytes()))
  selected := if text == "" { "" } else { text }
  length := loop { break encoding.html_escape(selected).len() }
  print(length)
  print(encoding.html_escape("a\0b\r\n'"))
  return Ok(())
}
"#;
    let expected = "\n<&日本\n&lt;&amp;日本\ntemporary &amp;\nnew &gt;\n&lt;&amp;&quot;\n&lt;&amp;&quot;\ninvalid\nfallback\n15\na\0b\r\n&#39;\n";
    for per_unit in [false, true] {
        for rt_lto in [false, true] {
            let name = format!("html-{per_unit}-{rt_lto}");
            let executable = build(&stage, source, per_unit, rt_lto, &name);
            let result = run(
                &stage,
                &executable,
                usize::from(per_unit) * 2 + usize::from(rt_lto),
            );
            assert!(result.status.success(), "{:?}", result.stderr);
            assert_eq!(result.stdout, expected.as_bytes());
            assert!(result.stderr.is_empty(), "{:?}", result.stderr);
        }
    }
}
