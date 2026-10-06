//! Complete-input decompression across imported whole/per-unit builds.
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
            "compiled compression child exceeded work deadline"
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
                Err(error) => panic!("poll compiled compression child: {error}"),
            }
        }
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // This generated program only decodes its embedded bytes and never launches descendants.
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
                    eprintln!("compiled compression child did not reap before deadline");
                    return;
                }
            }
        }
    }
}

fn build(stage: &align_driver::ArtifactStage, source: &str, per_unit: bool, name: &str) -> PathBuf {
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
    for (index, (mir, _is_entry)) in programs.iter().enumerate() {
        let object = stage.path().join(format!("{name}-{index}.o"));
        emit_object_file(
            mir,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
        )
        .expect("compression codegen");
        objects.push(object);
        for library in &mir.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
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
    .expect("compression link");
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
        child: Some(command.spawn().expect("spawn compression executable")),
        deadline: Instant::now() + Duration::from_secs(20),
    };
    let status = child.wait();
    let read = |path: &Path| {
        assert!(
            std::fs::metadata(path).unwrap().len() <= 16_384,
            "unbounded compression output"
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
fn compression_frames_whole_and_per_unit_publish_only_complete_results() {
    assert!(backend_available(), "compression owner requires LLVM");
    let stage = align_driver::ArtifactStage::temp("compression-frames").unwrap();
    std::fs::write(
        stage.path().join("helper.align"),
        r#"module helper
import std.compress
pub fn identity<T>(value: T) -> T = value
pub fn gzip(raw: slice<u8>) -> Result<buffer, Error> {
  decoded := compress.gzip_decompress(raw)?
  return Ok(identity(decoded))
}
pub fn zstd(raw: slice<u8>) -> Result<buffer, Error> {
  decoded := compress.zstd_decompress(raw)?
  return Ok(identity(decoded))
}
"#,
    )
    .unwrap();
    // Independent golden members encode "left" and "right". An appended zero is invalid.
    let source = r#"import std.compress
import std.encoding
import helper
fn keep(error: Error) -> Error = error
pub fn main() -> Result<(), Error> {
  gz := encoding.hex_decode("1f8b08000000000002ffcb494d2b010068e7677a040000001f8b08000000000002ff2bca4ccf2801001475cab405000000")?
  zs := encoding.hex_decode("28b52ffd04582100006c656674c89be64828b52ffd04582900007269676874923eb3be")?
  mut decoded := helper.gzip(gz.bytes()).map_err(keep)?
  print(decoded.bytes().as_str()?)
  decoded = helper.zstd(zs.bytes())?
  print(decoded.bytes().as_str()?)
  gz_bad := encoding.hex_decode("1f8b08000000000002ffcb494d2b010068e7677a0400000000")?
  zs_bad := encoding.hex_decode("28b52ffd04582100006c656674c89be64800")?
  match helper.gzip(gz_bad.bytes()) {
    Ok(_) => print("unexpected gzip"),
    Err(error) => match error { Invalid => print("invalid gzip"), _ => print("wrong error") },
  }
  match helper.zstd(zs_bad.bytes()) {
    Ok(_) => print("unexpected zstd"),
    Err(error) => match error { Invalid => print("invalid zstd"), _ => print("wrong error") },
  }
  empty := encoding.hex_decode("5f2a4d1803000000aabbcc")?
  zero := compress.zstd_decompress(empty.bytes())?
  print(zero.len())
  return Ok(())
}
"#;
    for per_unit in [false, true] {
        let name = format!("compression-{per_unit}");
        let executable = build(&stage, source, per_unit, &name);
        let result = run(&stage, &executable, usize::from(per_unit));
        assert!(result.status.success(), "{:?}", result.stderr);
        assert_eq!(
            result.stdout,
            b"leftright\nleftright\ninvalid gzip\ninvalid zstd\n0\n"
        );
        assert!(result.stderr.is_empty(), "{:?}", result.stderr);
    }
}
