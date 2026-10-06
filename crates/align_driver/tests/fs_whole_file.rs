//! Compiled whole/per-unit consumers of the one-descriptor special-file fallback.
mod common;
use common::*;
use std::io::{ErrorKind, Write};
use std::os::unix::{ffi::OsStrExt, fs::OpenOptionsExt};
use std::time::{Duration, Instant};

struct ChildOwner {
    child: Option<std::process::Child>,
    deadline: Instant,
}
impl ChildOwner {
    fn tick(&self) {
        assert!(
            Instant::now() + Duration::from_secs(5) < self.deadline,
            "compiled FIFO reader exceeded work deadline"
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
                Err(error) => panic!("poll compiled reader: {error}"),
            }
        }
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // This generated program only reads three files and never launches descendants.
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
                    eprintln!("compiled reader did not reap before deadline");
                    return;
                }
            }
        }
    }
}

#[test]
fn whole_and_per_unit_fifo_reads_finish_with_one_writer() {
    if !backend_available() {
        return;
    }
    let stage =
        align_driver::ArtifactStage::temp("whole-file-fifo").expect("exclusive FIFO fixture");
    let helper = r#"module helper
import std.fs
pub fn read_owned(path: str) -> Result<string, Error> = fs.read_file(path)
pub fn text_size(path: str) -> Result<i64, Error> = arena {
  value := fs.read_file_view(path)?
  return Ok(value.len())
}
pub fn byte_sum(path: str) -> Result<i64, Error> = arena {
  value := fs.read_bytes_view(path)?
  return Ok((value[0] as i64) + (value[1] as i64) + (value[2] as i64))
}
"#;
    let main = r#"import helper
fn main(args: array<str>) -> Result<(), Error> {
  owned := helper.read_owned(args[1])?
  print(helper.text_size(args[2])?)
  print(helper.byte_sum(args[3])?)
  print(owned)
  return Ok(())
}
"#;
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
            .expect("FIFO codegen");
            objects.push(object);
            for library in &mir.link_libs {
                if !libraries.contains(library) {
                    libraries.push(library.clone());
                }
            }
        }
        let executable = stage.path().join(format!("reader-{per_unit}"));
        let references: Vec<_> = objects.iter().map(|object| object.as_path()).collect();
        link_objects(
            &align_driver::CDriver::default(),
            &references,
            &executable,
            &libraries,
            Profile::Release,
        )
        .expect("FIFO link");
        let paths: Vec<_> = (0..3)
            .map(|mode| stage.path().join(format!("fifo-{per_unit}-{mode}")))
            .collect();
        for path in &paths {
            let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        }
        let stdout = stage.path().join(format!("stdout-{per_unit}"));
        let stderr = stage.path().join(format!("stderr-{per_unit}"));
        let mut command = std::process::Command::new(&executable);
        command
            .args(&paths)
            .stdin(std::process::Stdio::null())
            .stdout(std::fs::File::create(&stdout).unwrap())
            .stderr(std::fs::File::create(&stderr).unwrap());
        let mut child = ChildOwner {
            child: Some(command.spawn().expect("spawn compiled FIFO reader")),
            deadline: Instant::now() + Duration::from_secs(20),
        };
        for (path, payload) in
            paths
                .iter()
                .zip([b"retained\n".as_slice(), b"view", b"\xff\x00\x80"])
        {
            let mut writer = loop {
                child.tick();
                match std::fs::OpenOptions::new()
                    .write(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(path)
                {
                    Ok(file) => break file,
                    Err(error)
                        if error.raw_os_error() == Some(libc::ENXIO)
                            || error.kind() == ErrorKind::Interrupted => {}
                    Err(error) => panic!("open writer: {error}"),
                }
            };
            let mut rest = payload;
            while !rest.is_empty() {
                child.tick();
                match writer.write(rest) {
                    Ok(0) => panic!("zero FIFO write"),
                    Ok(count) => rest = &rest[count..],
                    Err(error)
                        if matches!(
                            error.kind(),
                            ErrorKind::Interrupted | ErrorKind::WouldBlock
                        ) => {}
                    Err(error) => panic!("write FIFO: {error}"),
                }
            }
        }
        let status = child.wait();
        assert!(
            status.success(),
            "{}",
            std::fs::read_to_string(stderr).unwrap()
        );
        assert_eq!(std::fs::read(stdout).unwrap(), b"4\n383\nretained\n\n");
    }
}
