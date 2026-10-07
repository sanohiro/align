//! Owned builder contents transfer without borrowing the consumed local slot.
#![cfg(unix)]

use align_driver::{
    ArtifactStage, BuildTarget, Profile, build_per_unit, check, emit_llvm_ir, link_objects,
    lower_to_mir,
};
use align_span::SourceMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::os::unix::{
    net::{UnixListener, UnixStream},
    process::CommandExt,
};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

struct ChildGroup {
    child: Child,
    deadline: Instant,
}
impl ChildGroup {
    fn spawn(command: &mut Command) -> Self {
        Self {
            child: command
                .process_group(0)
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
            deadline: Instant::now() + Duration::from_secs(60),
        }
    }
    fn tick(&self) {
        assert!(
            Instant::now() + Duration::from_secs(5) < self.deadline,
            "builder owner exceeded work deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    fn wait(&mut self) -> ExitStatus {
        loop {
            self.tick();
            match self.child.try_wait() {
                // Keep ownership of the group even after the leader has exited and been reaped.
                Ok(Some(status)) => return status,
                Ok(None) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("poll builder owner: {error}"),
            }
        }
    }
}
impl Drop for ChildGroup {
    fn drop(&mut self) {
        let pid = i32::try_from(self.child.id()).expect("native child pid");
        loop {
            // SAFETY: this invocation owns the fresh process group, including surviving helpers.
            if unsafe { libc::kill(-pid, libc::SIGKILL) } == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != ErrorKind::Interrupted || Instant::now() >= self.deadline {
                break;
            }
        }
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => {
                    eprintln!("builder owner failed to reap before deadline");
                    break;
                }
            }
        }
    }
}

fn helper(stage: &Path, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "builder_transfer_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("ALIGN_BUILDER_TRANSFER_STAGE", stage)
        .env("ALIGN_BUILDER_TRANSFER_ROLE", role)
        .stdout(File::create(stage.join(format!("{role}.stdout"))).unwrap())
        .stderr(File::create(stage.join(format!("{role}.stderr"))).unwrap());
    command
}

#[test]
fn builder_transfer_whole_and_per_unit() {
    let stage = ArtifactStage::temp("builder-transfer").unwrap();
    let status = ChildGroup::spawn(&mut helper(stage.path(), "compile")).wait();
    assert!(
        status.success(),
        "{}\n{}",
        fs::read_to_string(stage.path().join("compile.stdout")).unwrap(),
        fs::read_to_string(stage.path().join("compile.stderr")).unwrap()
    );
}

#[test]
#[ignore = "runs only in a parent-owned process group and artifact stage"]
fn builder_transfer_helper() {
    let stage = std::env::var_os("ALIGN_BUILDER_TRANSFER_STAGE").expect("parent-owned stage");
    let stage = Path::new(&stage);
    match std::env::var("ALIGN_BUILDER_TRANSFER_ROLE")
        .unwrap()
        .as_str()
    {
        "compile" => compile_and_run(stage),
        "leader" => {
            // Deliberately leave a live descendant after the group leader exits. Its ownership
            // remains with the parent test's ChildGroup, not with this intentionally short helper.
            helper(stage, "descendant").spawn().unwrap();
        }
        "descendant" => {
            let _stream = UnixStream::connect(stage.join("socket")).unwrap();
            loop {
                std::thread::park();
            }
        }
        _ => panic!("unknown helper role"),
    }
}

#[test]
fn builder_transfer_group_cleanup_survives_leader_exit_and_unwind() {
    // A short root is needed for the native sockaddr_un path limit on macOS.
    let stage = ArtifactStage::in_dir(Path::new("/tmp"), "builder-group").unwrap();
    let listener = UnixListener::bind(stage.path().join("socket")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut owner = ChildGroup::spawn(&mut helper(stage.path(), "leader"));
    let pid = i32::try_from(owner.child.id()).unwrap();
    let (mut stream, _) = loop {
        owner.tick();
        match listener.accept() {
            Ok(stream) => break stream,
            Err(error)
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
            Err(error) => panic!("accept descendant witness: {error}"),
        }
    };
    assert!(owner.wait().success());
    drop(owner);
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert_eq!(
        stream.read(&mut [0]).unwrap(),
        0,
        "descendant must close its socket after group retirement"
    );
    let mut status = 0;
    // SAFETY: waitpid observes only our own direct child; it must already have been reaped.
    assert_eq!(
        unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );

    let mut child_pid = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let owner = ChildGroup::spawn(Command::new("/bin/sleep").arg("60"));
        child_pid = Some(i32::try_from(owner.child.id()).unwrap());
        panic!("injected failure immediately after acquisition");
    }));
    assert!(result.is_err());
    // SAFETY: same direct-child reap observation after unwind cleanup.
    assert_eq!(
        unsafe { libc::waitpid(child_pid.unwrap(), &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

fn compile_and_run(stage: &Path) {
    let mut factory = String::from(
        "module factory\npub CopyRow { value: i64 }\npub MoveRow { value: string }\npub fn keep<T>(value: T) -> T = value\n",
    );
    let mut source = String::from("import factory\n");
    for (name, ty, value, read) in [
        ("scalar", "i64", "7", "rows[0]"),
        ("text", "string", "\"seven\".clone()", "rows[0].len()"),
        (
            "copy_row",
            "factory.CopyRow",
            "factory.CopyRow { value: 7 }",
            "rows[0].value",
        ),
        (
            "move_row",
            "factory.MoveRow",
            "factory.MoveRow { value: \"seven\".clone() }",
            "rows[0].value.len()",
        ),
    ] {
        factory.push_str(&format!("pub fn {name}() -> array_builder<{ty}> {{ mut b: array_builder<{ty}> := array_builder(); b.push({value}); return b }}\n").replace("factory.", ""));
        source.push_str(&format!(
            r#"
fn touch_{name}(borrow mut rows: array<{ty}>) -> i64 = rows.len()
fn {name}() {{
 first := factory.{name}()
 mut rows := first.build()
 mut index := 0
 loop {{
   if index == 2 {{ break }}
   if index == 0 {{
     mut next: array_builder<{ty}> := array_builder()
     next.push({value})
     selected := factory.keep(next)
     rows = selected.build()
   }}
   print(touch_{name}(rows))
   print({read})
   index = index + 1
 }}
 print({read})
}}
"#
        ));
    }
    source.push_str("fn main() { scalar(); text(); copy_row(); move_row() }\n");
    fs::write(stage.join("factory.align"), factory).unwrap();
    let entry = stage.join("main.align");
    fs::write(&entry, &source).unwrap();
    for per_unit in [false, true] {
        let mut sm = SourceMap::new();
        let programs = if per_unit {
            let walk = build_per_unit(&mut sm, entry.to_str().unwrap(), &source);
            assert!(
                !walk.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &walk.diags)
            );
            assert_eq!(walk.units.len(), 2);
            for unit in &walk.units {
                let bytes = align_interface::serialize(&unit.summary);
                assert_eq!(
                    bytes,
                    align_interface::serialize(&align_interface::deserialize(&bytes).unwrap())
                );
            }
            walk.units
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
        let llc = align_driver::llvm_tool("llc").expect("matched LLVM llc");
        let mut objects = Vec::new();
        let mut libraries = Vec::new();
        for (index, mir) in programs.iter().enumerate() {
            let input = stage.join(format!("{per_unit}-{index}.ll"));
            let object = stage.join(format!("{per_unit}-{index}.o"));
            fs::write(
                &input,
                emit_llvm_ir(
                    mir,
                    BuildTarget::Baseline,
                    Profile::Release,
                    false,
                    &[],
                    false,
                )
                .unwrap(),
            )
            .unwrap();
            assert!(
                Command::new(&llc)
                    .args(["-filetype=obj", "-relocation-model=pic"])
                    .arg(input)
                    .arg("-o")
                    .arg(&object)
                    .status()
                    .unwrap()
                    .success()
            );
            objects.push(object);
            for library in &mir.link_libs {
                if !libraries.contains(library) {
                    libraries.push(library.clone());
                }
            }
        }
        let executable = stage.join(format!("program-{per_unit}"));
        link_objects(
            &align_driver::CDriver::default(),
            &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
            &executable,
            &libraries,
            Profile::Release,
        )
        .unwrap();
        let output = stage.join(format!("output-{per_unit}"));
        assert!(
            Command::new(executable)
                .stdout(File::create(&output).unwrap())
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            fs::read_to_string(output).unwrap(),
            "1\n7\n1\n7\n7\n1\n5\n1\n5\n5\n1\n7\n1\n7\n7\n1\n5\n1\n5\n5\n"
        );
    }

    for bad in [
        "mut b: array_builder<i64> := array_builder(); b.push({ b = array_builder(); 1 })",
        "mut b: array_builder<i64> := array_builder(); b.push({ rows := b.build(); 1 })",
        "values := [1]; mut b: array_builder<i64> := array_builder(); b.append({ b = array_builder(); values[..] })",
        "values := [1]; mut b: array_builder<i64> := array_builder(); b.append({ rows := b.build(); values[..] })",
    ] {
        let source = format!("import factory\nfn main() {{ {bad} }}\n");
        fs::write(&entry, &source).unwrap();
        for per_unit in [false, true] {
            let mut sm = SourceMap::new();
            let diags = if per_unit {
                build_per_unit(&mut sm, entry.to_str().unwrap(), &source).diags
            } else {
                check(&mut sm, entry.to_str().unwrap(), &source).diags
            };
            let rendered = align_driver::format_diagnostics(&sm, &diags);
            assert!(
                diags.has_errors() && rendered.contains("invalidated"),
                "per_unit={per_unit}: {bad}: {rendered}"
            );
        }
    }
}
