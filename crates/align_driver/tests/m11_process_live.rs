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
