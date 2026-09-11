//! R65 native process observations, live capture, and ownership boundaries.
mod common;
use common::*;

#[test]
fn typed_wait_and_cached_result() {
    let source = r#"import std.process
fn describe(value: process.wait_result) -> i64 {
    match value.termination {
        Exited(code) => code
        Signaled(signal) => -signal
    }
}
pub fn main() -> Result<(), Error> {
    child := process.spawn("/bin/sh", ["sh", "-c", "exit 143"])?
    first := child.wait()?
    second := child.wait()?
    print(describe(first))
    print(describe(second))
    print(match first.max_rss_bytes { Some(count) => count >= 0, None => true })
    Ok(())
}
"#;
    let checked = diff_check_multi("process-live-wait", &[("main.align", source)], "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let output = build_and_run("process-live-wait", source);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "143\n143\ntrue\n");
    }
}

#[test]
fn live_read_and_synchronous_status() {
    let source = r#"import std.process
pub fn main() -> Result<(), Error> {
    command := process.command("/bin/sh", ["sh", "-c", "printf abcd"])
    mut child := command.start()?
    zero: u8 := 0
    mut bytes := [zero, zero, zero, zero]
    child.poll(process.readiness { stdout: true, stderr: false, status: false }, 1000000000)?
    print(child.read_stdout(bytes)? else -1)
    print(bytes[0])
    print(child.id() > 0)
    result := command.run()?
    status := result.status()
    print(match status.termination { Exited(code) => code, Signaled(signal) => -signal })
    print(result.stdout())
    Ok(())
}
"#;
    let checked = diff_check_multi("process-live-read", &[("main.align", source)], "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let output = build_and_run("process-live-read", source);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "4\n97\ntrue\n0\nabcd\n"
        );
    }
}

#[test]
fn live_out_buffers_and_borrow_modes() {
    let valid = r#"import std.process
fn read(borrow mut child: child, out destination: slice<u8>) -> Result<Option<i64>, Error> {
    child.read_stdout(destination)
}
pub fn main() -> Result<(), Error> {
    command := process.command("/bin/sh", ["sh", "-c", "printf x"])
    mut child := command.start()?
    zero: u8 := 0
    mut bytes := [zero]
    child.poll(process.readiness { stdout: true, stderr: false, status: false }, 1000000000)?
    print(read(child, bytes)? else -1)
    Ok(())
}
"#;
    let checked = diff_check_multi("process-out-helper", &[("main.align", valid)], "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let files = &[("main.align", valid)];
        let linked = build_per_unit_multi("process-out-helper", files, "main.align").link_and_run();
        assert!(
            linked.status.success(),
            "{}",
            String::from_utf8_lossy(&linked.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&linked.stdout), "1\n");
    }
    for (name, body) in [
        (
            "literal",
            "mut bytes := \"constant\".bytes()\n child.read_stdout(bytes)?",
        ),
        (
            "string",
            "mut text := \"mutable string\".clone()\n mut bytes := text.bytes()\n child.read_stdout(bytes)?",
        ),
        (
            "shared",
            "fn bad(borrow mut child: child, input: slice<u8>) -> Result<Option<i64>, Error> {\n mut bytes := input\n child.read_stdout(bytes)\n}",
        ),
        (
            "shared-child",
            "fn bad(borrow child: child, out bytes: slice<u8>) -> Result<Option<i64>, Error> { child.read_stdout(bytes) }",
        ),
        ("old-code", "out := command.run()?\n print(out.code())"),
    ] {
        let source = if body.starts_with("fn ") {
            format!("import std.process\n{body}\npub fn main() -> i32 = 0\n")
        } else {
            format!(
                "import std.process\npub fn main() -> Result<(), Error> {{\ncommand := process.command(\"/bin/sh\", [\"sh\",\"-c\",\"exit 0\"])\nchild := command.start()?\n{body}\nOk(())\n}}\n"
            )
        };
        let checked = diff_check_multi(name, &[("main.align", source.as_str())], "main.align");
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{name} admitted readonly/obsolete operation"
        );
    }
}

#[test]
fn retained_file_output_and_group_observation() {
    struct OwnedDirectory(std::path::PathBuf);
    impl Drop for OwnedDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let template = std::env::temp_dir().join("align-process-file-XXXXXX");
    let mut bytes = template.as_os_str().as_bytes().to_vec();
    bytes.push(0);
    // mkdtemp atomically creates a private directory; only that acquired path is removed.
    assert!(!unsafe { libc::mkdtemp(bytes.as_mut_ptr().cast()) }.is_null());
    bytes.pop();
    let directory = OwnedDirectory(std::ffi::OsString::from_vec(bytes).into());
    let file = directory.0.join("captured");
    let source = format!(
        r#"import std.process
import std.fs
Outputs {{ handle: writer }}
fn bind(borrow mut command: command, borrow outputs: Outputs) -> Result<(), Error> {{
    command.stdout_to(outputs.handle)?
    command.stderr_to(outputs.handle)?
    Ok(())
}}
fn configure(borrow mut command: command, path: str) -> Result<(), Error> {{
    outputs := Outputs {{ handle: fs.create(path)? }}
    bind(command, outputs)?
    Ok(())
}}
pub fn main() -> Result<(), Error> {{
    mut command := process.command("/bin/sh", ["sh", "-c", "printf out; printf err >&2"])
    configure(command,"{}")?
    first := command.start()?
    first.wait()?
    second := command.start()?
    second.wait()?
    text := fs.read_file("{}")?
    print(text)
    match command.run() {{ Ok(_) => print(false), Err(error) => print(match error {{ Invalid => true, _ => false }}) }}
    table := process.table(100000)?
    print(table.len() > 0)
    session := process.command("/bin/sh", ["sh", "-c", "sleep 10"])
    session.new_session(true)
    child := session.start()?
    members := child.group_members(100000)?
    print(members.len() > 0)
    child.kill_group(process.signal_number(process.signal.Kill))?
    child.poll(process.readiness {{ stdout: false, stderr: false, status: true }}, 1000000000)?
    status := child.wait()?
    print(match status.termination {{ Exited(_) => false, Signaled(value) => value == process.signal_number(process.signal.Kill) }})
    Ok(())
}}
"#,
        file.display(),
        file.display()
    );
    let checked = diff_check_multi(
        "process-file",
        &[("main.align", source.as_str())],
        "main.align",
    );
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let output = build_and_run("process-file", &source);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "outerrouterr\ntrue\ntrue\ntrue\ntrue\n"
        );
    }
}

#[test]
fn imported_process_copy_signatures_reconstruct_capabilities() {
    let helper = r#"module process_helpers
import std.process
pub fn termination() -> process.termination = process.termination.Exited(7)
pub fn status() -> process.wait_result = process.wait_result { termination: termination(), max_rss_bytes: None }
pub fn ready() -> process.readiness = process.readiness { stdout: true, stderr: false, status: true }
pub fn signal() -> process.signal = process.signal.Terminate
pub fn selection() -> process.signal_set = process.signal_set { hangup: false, interrupt: true, quit: false, terminate: true }
pub fn snapshot() -> process.snapshot = process.snapshot { pid: 1, parent_pid: 0, rss_bytes: None, cpu_ns: None, threads: None }
pub fn number(value: process.signal) -> i64 = process.signal_number(value)
"#;
    let main = r#"import process_helpers
pub fn main() {
    status := process_helpers.status()
    print(match status.termination { Exited(value) => value, Signaled(value) => -value })
    ready := process_helpers.ready()
    print(ready.stdout)
    selection := process_helpers.selection()
    print(selection.interrupt)
    snapshot := process_helpers.snapshot()
    print(snapshot.pid)
    print(process_helpers.number(process_helpers.signal()) > 0)
}
"#;
    let files = [("process_helpers.align", helper), ("main.align", main)];
    let checked = diff_check_multi("process-imported-copy", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let output =
            build_per_unit_multi("process-imported-copy", &files, "main.align").link_and_run();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "7\ntrue\ntrue\n1\ntrue\n"
        );
    }
}

#[test]
fn user_nominals_keep_precedence_over_process_owner_spellings() {
    for name in ["command", "run_output"] {
        let helper = format!(
            r#"module helper
pub {name}<T> {{ value: T }}
pub fn make(value: str) -> {name}<str> = {name} {{ value: value }}
pub fn view(borrow value: {name}<str>) -> str = value.value
"#
        );
        let main = r#"import helper
fn main() {
    record := helper.make("retained")
    print(helper.view(record))
}
"#;
        let files = [("helper.align", helper.as_str()), ("main.align", main)];
        let checked = diff_check_multi(&format!("process-nominal-{name}"), &files, "main.align");
        assert!(
            !checked.whole_errors && !checked.per_unit_errors,
            "{}\n{}",
            checked.whole_diags,
            checked.per_unit_diags
        );
        if backend_available() {
            let output =
                build_per_unit_multi(&format!("process-nominal-run-{name}"), &files, "main.align")
                    .link_and_run();
            assert!(output.status.success());
            assert_eq!(String::from_utf8_lossy(&output.stdout), "retained\n");
        }
    }
}

const STATUS_HELPER: &str = r#"module helper
import std.process
pub Snapshot { label: string, code: i64, stdout: string, stderr: string }
pub fn exit_code(status: process.wait_result) -> i64 {
    match status.termination { Exited(code) => code, Signaled(signal) => 128 + signal }
}
pub fn identity<T>(value: T) -> T = value
fn keep_error(error: Error) -> Error = error
fn gate(fail: bool) -> Result<(), Error> {
    if fail { return Err(Error.Invalid) }
    Ok(())
}
pub fn capture(path: str, direct: bool, fail: bool) -> Snapshot {
    command := process.command(path, ["sh", "-c", "printf captured; printf errors >&2; exit 7"])
    return match command.run() {
        Ok(output) => {
            status := output.status()
            code := if direct {
                match status.termination { Exited(value) => value, Signaled(value) => 128 + value }
            } else { exit_code(status) }
            text := output.stdout().clone()
            error_text := output.stderr().clone()
            return match gate(fail) {
                Ok(_) => Snapshot { label: "success".clone(), code: code, stdout: text, stderr: error_text },
                Err(_) => Snapshot { label: "rejected".clone(), code: -2, stdout: "fallback".clone(), stderr: "error".clone() },
            }
        },
        Err(_) => Snapshot { label: "launch".clone(), code: -1, stdout: "failed".clone(), stderr: "error".clone() },
    }
}
pub fn bytes() -> Result<Snapshot, Error> {
    command := process.command("/bin/sh", ["sh", "-c", "exit 8"])
    output := command.run_bytes()?
    status := output.status()
    code := exit_code(status)
    Ok(Snapshot { label: "bytes".clone(), code: code, stdout: "literal".clone(), stderr: "error".clone() })
}
pub fn waited(fail: bool) -> Result<Snapshot, Error> {
    child := process.spawn("/bin/sh", ["sh", "-c", "exit 9"])?
    status := child.wait().map_err(keep_error)?
    code := match status.termination { Exited(value) => value, Signaled(value) => 128 + value }
    rss := status.max_rss_bytes else 0
    if rss < 0 { return Err(Error.Invalid) }
    text := "waited".clone()
    gate(fail)?
    Ok(Snapshot { label: "wait".clone(), code: code, stdout: text, stderr: "error".clone() })
}
"#;

const STATUS_MAIN: &str = r#"import helper
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn exercise() -> Result<(), Error> {
    first := helper.capture("/bin/sh", true, false)
    if first.code != 7 || first.stdout != "captured" || first.stderr != "errors" { return Err(Error.Invalid) }
    mut current := helper.identity(helper.capture("/bin/sh", false, false))
    mut index := 0
    loop {
        if index == 2 { break }
        current = helper.identity(helper.capture("/bin/sh", false, false))
        index = index + 1
    }
    moved_text := current.stdout
    if moved_text != "captured" { return Err(Error.Invalid) }
    rejected := helper.capture("/bin/sh", false, true)
    if rejected.code != -2 || rejected.stdout != "fallback" { return Err(Error.Invalid) }
    failed := helper.capture("/dev/null/align-status-command", false, false)
    if failed.code != -1 { return Err(Error.Invalid) }
    bytes := helper.bytes()?
    if bytes.code != 8 || bytes.stdout != "literal" { return Err(Error.Invalid) }
    waited := helper.waited(false)?
    if waited.code != 9 || waited.stdout != "waited" { return Err(Error.Invalid) }
    match helper.waited(true) {
        Ok(unexpected) => { return Err(Error.Invalid) },
        Err(_) => {},
    }
    Ok(())
}
fn main() -> i32 {
    before_alloc := unsafe { align_rt_alloc_count() }
    before_free := unsafe { align_rt_free_count() }
    exercise() else { return 2 }
    allocated := unsafe { align_rt_alloc_count() } - before_alloc
    freed := unsafe { align_rt_free_count() } - before_free
    print(allocated)
    print(freed)
    if allocated < 20 { return 3 }
    if allocated != freed { return 1 }
    0
}
"#;

#[test]
fn process_status_owned_composition() {
    // Link the instrumented runtime archive used by the generated native probes.
    let _: extern "C" fn() -> i64 = align_runtime::align_rt_alloc_count;
    let _: extern "C" fn() -> i64 = align_runtime::align_rt_free_count;
    let files = [("helper.align", STATUS_HELPER), ("main.align", STATUS_MAIN)];
    let checked = diff_check_multi("status-owned", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if !backend_available() {
        return;
    }
    for per_unit in [false, true] {
        for omit_drop in [false, true] {
            let project = status_project(&files);
            let entry = project.dir.join("main.align");
            let mut map = SourceMap::new();
            let mut programs = if per_unit {
                let walk = build_per_unit(&mut map, &entry.display().to_string(), STATUS_MAIN);
                assert!(
                    !walk.diags.has_errors(),
                    "{}",
                    align_driver::format_diagnostics(&map, &walk.diags)
                );
                walk.units
                    .into_iter()
                    .map(|unit| unit.mir)
                    .collect::<Vec<_>>()
            } else {
                let checked = check(&mut map, &entry.display().to_string(), STATUS_MAIN);
                assert!(
                    !checked.diags.has_errors(),
                    "{}",
                    align_driver::format_diagnostics(&map, &checked.diags)
                );
                vec![lower_to_mir(&checked.hir)]
            };
            if omit_drop {
                let mut removed = 0;
                for program in &mut programs {
                    for function in &mut program.fns {
                        if function.name.as_str() != "exercise" {
                            continue;
                        }
                        for block in &mut function.blocks {
                            block.stmts.retain(|statement| {
                                let remove = matches!(statement, align_mir::Stmt::Drop(slot)
                                    if matches!(function.slots[*slot as usize], align_sema::Ty::Struct(id)
                                        if program.structs[id as usize].fields.iter().any(|field| field.name == "stdout")));
                                if remove { removed += 1; }
                                !remove
                            });
                            block.stmt_lines.clear();
                        }
                    }
                }
                assert!(
                    removed > 0,
                    "negative control removed no returned record Drop"
                );
            }
            let mut objects = Vec::new();
            let mut libraries = Vec::new();
            for (index, program) in programs.iter().enumerate() {
                let object = project.dir.join(format!("unit{index}.o"));
                emit_object_file(
                    program,
                    &object,
                    BuildTarget::Baseline,
                    Profile::Release,
                    &[],
                    false,
                )
                .expect("emit status probe");
                objects.push(object);
                for library in &program.link_libs {
                    if !libraries.contains(library) {
                        libraries.push(library.clone());
                    }
                }
            }
            let executable = project
                .dir
                .join(format!("probe{}", std::env::consts::EXE_SUFFIX));
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
            .expect("link status probe");
            let (status, stdout, stderr) = run_status_probe(&executable);
            assert_eq!(
                status.code(),
                Some(i32::from(omit_drop)),
                "unit={per_unit}, omit={omit_drop}: {stdout}\n{stderr}"
            );
        }
    }
}

fn status_project(files: &[(&str, &str)]) -> Proj {
    for attempt in 0..100 {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "align-status-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                let project = Proj {
                    dir,
                    entry: "main.align".to_owned(),
                };
                for (name, source) in files {
                    project.write(name, source);
                }
                return project;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create status project: {error}"),
        }
    }
    panic!("could not exclusively create status project");
}

struct StatusChild(Option<std::process::Child>);
impl Drop for StatusChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            #[cfg(unix)]
            if let Ok(pid) = i32::try_from(child.id()) {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
            let _ = child.kill();
            loop {
                match child.wait() {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    _ => break,
                }
            }
        }
    }
}

fn run_status_probe(executable: &std::path::Path) -> (std::process::ExitStatus, String, String) {
    let directory = executable.parent().expect("probe directory");
    let stdout = directory.join("stdout");
    let stderr = directory.join("stderr");
    let mut command = std::process::Command::new(executable);
    command.stdout(std::fs::File::create(&stdout).expect("stdout log"));
    command.stderr(std::fs::File::create(&stderr).expect("stderr log"));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = StatusChild(Some(command.spawn().expect("spawn status probe")));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "status probe timed out: {}",
            std::fs::read_to_string(&stderr).unwrap()
        );
        match child.0.as_mut().expect("live probe").try_wait() {
            Ok(Some(status)) => {
                child.0.take();
                return (
                    status,
                    std::fs::read_to_string(stdout).unwrap(),
                    std::fs::read_to_string(stderr).unwrap(),
                );
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => panic!("wait for status probe: {error}"),
        }
    }
}

#[test]
fn process_status_borrowed_escape_rejected() {
    for (operation, view) in [
        ("run", "output.stdout()"),
        ("run_bytes", "output.stdout().as_str()?"),
    ] {
        let source = format!(
            "import std.process\nView {{ code: i64, text: str }}\n\
             fn escape() -> Result<View, Error> {{\n\
             command := process.command(\"/bin/sh\", [\"sh\", \"-c\", \"printf captured\"])\n\
             output := command.{operation}()?\nstatus := output.status()\n\
             code := match status.termination {{ Exited(code) => code, Signaled(signal) => signal }}\n\
             Ok(View {{ code: code, text: {view} }})\n}}\n"
        );
        let checked = diff_check_multi("status-escape", &[("main.align", &source)], "main.align");
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{operation}: borrowed output escaped"
        );
        for diagnostics in [&checked.whole_diags, &checked.per_unit_diags] {
            assert!(
                diagnostics.contains("borrow") || diagnostics.contains("outlive"),
                "{diagnostics}"
            );
        }
    }
}
