//! Native argv admission before body entry, across whole/per-unit and runtime-LTO builds.
mod common;
use common::*;
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::os::unix::{ffi::OsStrExt, process::CommandExt};
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
            "compiled argument child exceeded work deadline"
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
                Err(error) => panic!("poll compiled argument child: {error}"),
            }
        }
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // This generated program only accesses its argument and never launches descendants.
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
                    eprintln!("compiled argument child did not reap before deadline");
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
        .expect("argument codegen");
        objects.push(object);
        for library in &mir.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
        if source.contains("args: array<str>") && *is_entry {
            let ir = emit_llvm_ir(
                mir,
                BuildTarget::Baseline,
                Profile::Release,
                false,
                &[],
                rt_lto,
            )
            .expect("argument raw IR");
            let wrapper = ir
                .split("define i32 @main(")
                .nth(1)
                .expect("native wrapper");
            let wrapper = wrapper.split("\n}").next().unwrap();
            let admitted = wrapper
                .split("args.admitted:")
                .nth(1)
                .expect("admission block");
            assert!(admitted.contains("load { ptr, i64 }"), "{wrapper}");
            assert!(admitted.contains("call "), "{wrapper}");
            let rejected = wrapper
                .split("args.rejected:")
                .nth(1)
                .unwrap()
                .split("args.admitted:")
                .next()
                .unwrap();
            assert!(rejected.contains("@align_rt_report_error"), "{wrapper}");
            assert!(rejected.contains("ret i32"), "{wrapper}");
            assert!(
                !rejected.contains("load { ptr, i64 }") && !rejected.contains("@\"align_fn$"),
                "{wrapper}"
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
    .expect("argument link");
    executable
}

fn run(
    stage: &align_driver::ArtifactStage,
    executable: &Path,
    argv: &[Vec<u8>],
    id: usize,
) -> std::process::Output {
    let stdout = stage.path().join(format!("stdout-{id}"));
    let stderr = stage.path().join(format!("stderr-{id}"));
    let mut command = std::process::Command::new(executable);
    command.arg0(OsStr::from_bytes(&argv[0]));
    command.args(argv[1..].iter().map(|arg| OsStr::from_bytes(arg)));
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::fs::File::create_new(&stdout).unwrap())
        .stderr(std::fs::File::create_new(&stderr).unwrap());
    let mut child = ChildOwner {
        child: Some(command.spawn().expect("spawn argument executable")),
        deadline: Instant::now() + Duration::from_secs(20),
    };
    let status = child.wait();
    let read = |path: &Path| {
        assert!(
            std::fs::metadata(path).unwrap().len() <= 16_384,
            "unbounded argument output"
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
fn argv_is_admitted_before_owned_whole_and_per_unit_entry() {
    if !backend_available() {
        return;
    }
    let stage = align_driver::ArtifactStage::temp("argv-text").expect("exclusive argv fixture");
    std::fs::write(
        stage.path().join("helper.align"),
        r#"module helper
pub fn identity<T>(value: T) -> T = value
pub fn pass(value: array<str>) -> array<str> = value
pub fn consume(args: array<str>) -> Result<(), Error> {
  print("body")
  owned := pass(identity(args))
  count := loop { break owned.len() }
  if count != 3 { return Err(Error.Invalid) }
  first := if owned[0] == "" { "" } else { owned[0] }
  print(first)
  print(owned[1])
  print(owned[2])
  if owned[1] == "error" { return Err(Error.Invalid) }
  return Ok(())
}
"#,
    )
    .unwrap();
    let source = r#"import helper
fn keep(error: Error) -> Error = error
pub fn main(args: array<str>) -> Result<(), Error> {
  helper.consume(args).map_err(keep)?
  return Ok(())
}
"#;
    let mut values = vec![
        Vec::new(),
        b"ordinary".to_vec(),
        "日本語🦀".as_bytes().to_vec(),
        vec![0xff],
        vec![0x80],
        vec![0xc0, 0x80],
        vec![0xed, 0xa0, 0x80],
        vec![0xc3],
        vec![0xf4, 0x90, 0x80, 0x80],
        vec![0xe2, 0x28, 0xa1],
    ];
    let mut late = vec![b'x'; 4096];
    late.push(0xff);
    values.push(late);
    let mut id = 0;
    for per_unit in [false, true] {
        for rt_lto in [false, true] {
            let executable = build(
                &stage,
                source,
                per_unit,
                rt_lto,
                &format!("argv-{per_unit}-{rt_lto}"),
            );
            for value in &values {
                for ordinal in 0..3 {
                    let mut argv = vec![b"program".to_vec(), b"normal".to_vec(), b"tail".to_vec()];
                    argv[ordinal] = value.clone();
                    let result = run(&stage, &executable, &argv, id);
                    id += 1;
                    if std::str::from_utf8(value).is_ok() {
                        let mut expected = b"body\n".to_vec();
                        for argument in &argv {
                            expected.extend(argument);
                            expected.push(b'\n');
                        }
                        assert_eq!(result.status.code(), Some(0));
                        assert_eq!(result.stdout, expected);
                        assert!(result.stderr.is_empty(), "{:?}", result.stderr);
                    } else {
                        assert_eq!(
                            result.status.code(),
                            Some(2),
                            "{per_unit}/{rt_lto}/{ordinal}"
                        );
                        assert!(result.stdout.is_empty(), "body ran before argv admission");
                        assert_eq!(result.stderr, b"error: code 2\n");
                    }
                }
            }
            let result = run(
                &stage,
                &executable,
                &[b"program".to_vec(), b"error".to_vec(), b"tail".to_vec()],
                id,
            );
            id += 1;
            assert_eq!(result.status.code(), Some(2));
            assert_eq!(result.stdout, b"body\nprogram\nerror\ntail\n");
            assert_eq!(result.stderr, b"error: code 2\n");
            let early = run(
                &stage,
                &executable,
                &[b"program".to_vec(), b"short".to_vec()],
                id,
            );
            id += 1;
            assert_eq!(early.status.code(), Some(2));
            assert_eq!(early.stdout, b"body\n");
            assert_eq!(early.stderr, b"error: code 2\n");
        }
    }
}

#[test]
fn unused_argv_is_validated_but_no_argument_entries_do_not_read_it() {
    if !backend_available() {
        return;
    }
    let stage =
        align_driver::ArtifactStage::temp("argv-unused").expect("exclusive unused argv fixture");
    let mut id = 0;
    for per_unit in [false, true] {
        for (name, source, expected) in [
            (
                "argv",
                "fn main(args: array<str>) -> Result<(), Error> { print(\"body\"); return Ok(()) }\n",
                2,
            ),
            ("unit", "fn main() { print(\"body\") }\n", 0),
            (
                "i32",
                "fn main() -> i32 { print(\"body\"); return 17 }\n",
                17,
            ),
            (
                "result",
                "fn main() -> Result<(), Error> { print(\"body\"); return Ok(()) }\n",
                0,
            ),
        ] {
            let executable = build(
                &stage,
                source,
                per_unit,
                false,
                &format!("{name}-{per_unit}"),
            );
            let result = run(&stage, &executable, &[vec![0xff], vec![0xc0, 0x80]], id);
            id += 1;
            assert_eq!(result.status.code(), Some(expected));
            if name == "argv" {
                assert!(result.stdout.is_empty());
                assert_eq!(result.stderr, b"error: code 2\n");
            } else {
                assert_eq!(result.stdout, b"body\n");
                assert!(result.stderr.is_empty());
            }
        }
    }
}
