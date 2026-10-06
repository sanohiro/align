//! Sparse sampling across imported whole/per-unit builds.
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
            "compiled sampling child exceeded work deadline"
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
                Err(error) => panic!("poll compiled sampling child: {error}"),
            }
        }
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // This generated program only samples its in-memory array and never launches descendants.
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
                    eprintln!("compiled sampling child did not reap before deadline");
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
        .expect("sampling codegen");
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
    .expect("sampling link");
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
        child: Some(command.spawn().expect("spawn sampling executable")),
        deadline: Instant::now() + Duration::from_secs(20),
    };
    let status = child.wait();
    let read = |path: &Path| {
        assert!(
            std::fs::metadata(path).unwrap().len() <= 16_384,
            "unbounded sampling output"
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
fn sparse_sample_imported_execution_preserves_output_and_rng() {
    assert!(backend_available(), "sampling owner requires LLVM");
    let stage = align_driver::ArtifactStage::temp("sparse-sample").unwrap();
    let helper = r#"module helper
import std.rand
pub fn draw(xs: slice<i64>) -> array<i64> {
  mut r := rand.seed_with(123)
  selected := r.sample(xs, 8)
  print(r.next())
  return selected
}
"#;
    let main = r#"import helper
fn main() -> i32 {
  mut values: array_builder<i64> := array_builder(4096)
  mut index := 0
  loop {
    if index == 4096 { break }
    values.push(index)
    index = index + 1
  }
  xs := values.build()
  selected := helper.draw(xs)
  print(selected.len())
  mut position := 0
  loop {
    if position == selected.len() { break }
    print(selected[position])
    position = position + 1
  }
  print(xs[0])
  print(xs[4095])
  return 0
}
"#;
    std::fs::write(stage.path().join("helper.align"), helper).unwrap();
    for per_unit in [false, true] {
        let name = format!("sample-{per_unit}");
        let executable = build(&stage, main, per_unit, &name);
        let output = run(&stage, &executable, usize::from(per_unit));
        assert!(output.status.success(), "{:?}", output.stderr);
        assert_eq!(
            output.stdout,
            b"-868699986926070454\n8\n2645\n3433\n2727\n2149\n233\n2464\n3139\n3748\n0\n4095\n"
        );
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    }
}
