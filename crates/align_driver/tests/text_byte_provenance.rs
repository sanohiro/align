//! Text-byte element provenance is certified through view retypes.
mod common;
use common::*;

fn fixture(files: &[(&str, &str)]) -> align_driver::ArtifactStage {
    let stage = align_driver::ArtifactStage::temp("text-byte-provenance")
        .expect("exclusive text-byte fixture");
    for (name, source) in files {
        let path = stage.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    stage
}

fn run(stage: &align_driver::ArtifactStage, per_unit: bool) -> std::process::Output {
    use std::io::ErrorKind;
    use std::time::{Duration, Instant};
    let entry = stage.path().join("main.align");
    let source = std::fs::read_to_string(&entry).unwrap();
    let mut sm = SourceMap::new();
    let programs = if per_unit {
        let built = build_per_unit(&mut sm, entry.to_str().unwrap(), &source);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &built.diags)
        );
        for unit in &built.units {
            let bytes = align_interface::serialize(&unit.summary);
            let replayed = align_interface::deserialize(&bytes).unwrap();
            assert_eq!(bytes, align_interface::serialize(&replayed));
        }
        built
            .units
            .into_iter()
            .map(|unit| unit.mir)
            .collect::<Vec<_>>()
    } else {
        let checked = check(&mut sm, entry.to_str().unwrap(), &source);
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
        .expect("text-byte codegen");
        objects.push(object);
        for library in &mir.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
    }
    let executable = stage.path().join(format!("run-{per_unit}"));
    let refs = objects
        .iter()
        .map(|path| path.as_path())
        .collect::<Vec<_>>();
    link_objects(
        &align_driver::CDriver::default(),
        &refs,
        &executable,
        &libraries,
        Profile::Release,
    )
    .expect("text-byte link");

    struct ChildOwner {
        child: Option<std::process::Child>,
        deadline: Instant,
    }
    impl Drop for ChildOwner {
        fn drop(&mut self) {
            let Some(child) = self.child.as_mut() else {
                return;
            };
            // This fixture only computes and prints; it cannot launch descendants.
            loop {
                match child.kill() {
                    Err(error)
                        if error.kind() == ErrorKind::Interrupted
                            && Instant::now() < self.deadline => {}
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
                        if error.kind() == ErrorKind::Interrupted
                            && Instant::now() < self.deadline => {}
                    _ => {
                        eprintln!("text-byte child did not reap before deadline");
                        return;
                    }
                }
            }
        }
    }
    let stdout = stage.path().join(format!("stdout-{per_unit}"));
    let stderr = stage.path().join(format!("stderr-{per_unit}"));
    let mut command = std::process::Command::new(executable);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::fs::File::create(&stdout).unwrap())
        .stderr(std::fs::File::create(&stderr).unwrap());
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut owner = ChildOwner {
        child: Some(command.spawn().expect("spawn text-byte owner")),
        deadline,
    };
    let status = loop {
        assert!(
            Instant::now() + Duration::from_secs(5) < owner.deadline,
            "text-byte child exceeded work deadline"
        );
        match owner.child.as_mut().unwrap().try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(1)),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => panic!("poll text-byte child: {error}"),
        }
    };
    owner.child.take();
    std::process::Output {
        status,
        stdout: std::fs::read(stdout).unwrap(),
        stderr: std::fs::read(stderr).unwrap(),
    }
}

#[test]
fn text_byte_bounds_keep_caller_provenance_whole_and_per_unit() {
    assert!(backend_available(), "text-byte owner requires LLVM");
    let helper = r#"module helper
fn width(first: i64) -> i64 {
  if first < 128 { return 1 }
  if first < 224 { return 2 }
  if first < 240 { return 3 }
  return 4
}
fn ascii(value: u8) -> bool = value == 32
fn space(value: str) -> bool = value == " " || value == "\u{85}"
pub fn trim_view(value: str, unicode: bool) -> str {
  bytes := value.bytes()
  mut begin := 0
  mut end := value.len()
  loop {
    if begin >= end { break }
    next := begin + width(bytes[begin] as i64)
    if !(if unicode { space(value[begin..next]) } else {
      next == begin + 1 && ascii(bytes[begin])
    }) { break }
    begin = next
  }
  loop {
    if end <= begin { break }
    mut previous := end - 1
    loop {
      if previous <= begin || bytes[previous] < 128 || bytes[previous] >= 192 { break }
      previous = previous - 1
    }
    if !(if unicode { space(value[previous..end]) } else {
      previous == end - 1 && ascii(bytes[previous])
    }) { break }
    end = previous
  }
  return value[begin..end]
}
pub fn generic<T>(marker: T, text: str) -> str = trim_view(text, true)
pub fn owned(borrow text: string) -> str = trim_view(text, true)
"#;
    let main = r#"module main
import helper
fn local_view(text: str) -> str {
  bytes := text.bytes()
  // The first byte chooses a small byte offset without discarding its provenance.
  offset := if bytes[0] == 32 { bytes[0] as i64 - 31 } else { 0 }
  return text[offset..text.len()]
}
fn main() -> i32 {
  if helper.trim_view("", true) != "" { return 1 }
  if helper.trim_view("   ", true) != "" { return 2 }
  if helper.trim_view("  hello  ", true) != "hello" { return 3 }
  if helper.trim_view("\u{85}日本語\u{85}", true) != "日本語" { return 4 }
  if helper.trim_view(" \u{85}日本語\u{85} ", false) != "\u{85}日本語\u{85}" { return 5 }
  if helper.trim_view("日本語", true) != "日本語" { return 6 }
  if helper.generic(7, " \u{85}a\u{85} ") != "a" { return 7 }
  owned := " \u{85}owned日本語\u{85} ".clone()
  if helper.owned(owned) != "owned日本語" { return 8 }
  if local_view(" abc") != "abc" || local_view("日本語") != "日本語" { return 9 }
  independent := helper.trim_view(owned, true).clone()
  if independent != "owned日本語" { return 10 }
  mut copied := helper.trim_view(owned, true).bytes().to_array()
  copied[0] = 65
  if copied[0] != 65 || helper.trim_view(owned, true) != independent { return 11 }
  print(independent)
  return 42
}
"#;
    let stage = fixture(&[("helper.align", helper), ("main.align", main)]);
    for per_unit in [false, true] {
        let output = run(&stage, per_unit);
        assert_eq!(
            output.status.code(),
            Some(42),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "owned日本語\n");
    }
}

#[test]
fn text_byte_views_keep_readonly_and_lifetime_restrictions() {
    for (name, source, needle) in [
        (
            "escape",
            "fn bad() -> str { owned := \" x\".clone(); bytes := owned.bytes(); return owned[bytes[0] as i64 - 31..owned.len()] }",
            "cannot return a view that borrows local storage",
        ),
        (
            "write",
            "fn bad(text: str) { mut bytes := text.bytes(); bytes[0] = 65 }",
            "read-only",
        ),
    ] {
        let stage = fixture(&[("main.align", source)]);
        let entry = stage.path().join("main.align");
        for per_unit in [false, true] {
            let mut sm = SourceMap::new();
            let diags = if per_unit {
                check_per_unit(&mut sm, entry.to_str().unwrap(), source).diags
            } else {
                check(&mut sm, entry.to_str().unwrap(), source).diags
            };
            let rendered = align_driver::format_diagnostics(&sm, &diags);
            assert!(
                diags.has_errors() && rendered.contains(needle),
                "{name}, per_unit={per_unit}: {rendered}"
            );
        }
    }
}
