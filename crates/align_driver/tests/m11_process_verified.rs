//! Verified process authority uses ordinary owners and errors.
mod common;
use common::*;

#[test]
fn memory_seal_positional_read_and_consume() {
    let source = r#"import std.fs
import std.process
fn finish(value: fs.memory_writer) -> Result<fs.sealed_file,Error> = value.seal()
pub fn main() -> Result<(), Error> {
    mut writer := fs.memory_file(fs.memory_kind.Data, 4)?
    writer.write("abc")?
    print(match writer.write("de") { Err(error) => match error { Invalid => true, _ => false }, Ok(_) => false })
    file := finish(writer)?
    print(file.len())
    zero: u8 := 0
    mut bytes := [zero,zero,zero,zero]
    print(file.read_at(1,bytes)?)
    print(bytes[0])
    print(bytes[1])
    print(bytes[2])
    print(match process.executable(file) { Err(error) => match error { Invalid => true, _ => false }, Ok(image) => false })
    Ok(())
}
"#;
    let files = [("main.align", source)];
    let checked = diff_check_multi("verified-memory", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() && cfg!(target_os = "linux") {
        for output in [
            build_and_run("verified-memory", source),
            build_per_unit_multi("verified-memory-unit", &files, "main.align").link_and_run(),
        ] {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "true\n3\n2\n98\n99\n0\ntrue\n"
            );
        }
    }
}

#[test]
fn retained_executable_command_and_descriptor_authority() {
    let helper = r#"module helper
import std.fs
import std.process
pub fn make() -> Result<command, Error> {
    input := fs.open("/bin/sh")?
    mut writer := fs.memory_file(fs.memory_kind.Executable, 16000000)?
    mut chunk := buffer(4096)
    loop {
        count := input.read(chunk)?
        if count == 0 { break }
        writer.write(chunk.bytes())?
    }
    sealed := writer.seal()?
    image := process.executable(sealed)?
    print(image.len() == sealed.len())
    mut command := process.command_image(image, ["sh", "-c", "read -r value <&3; test -e /dev/fd/7; printf %s $value"])?
    mut data := fs.memory_file(fs.memory_kind.Data, 8)?
    data.write("sealed\n")?
    file := data.seal()?
    command.inherit_file(file, 3)?
    print(match command.inherit_file(file, 3) { Err(error) => match error { Invalid => true, _ => false }, Ok(_) => false })
    namespace := process.user_namespace("/proc/self/ns/user")?
    command.inherit_namespace(namespace, 7)?
    Ok(command)
}
"#;
    let main = r#"import helper
pub fn main() -> Result<(), Error> {
    command := helper.make()?
    first := command.run()?
    second := command.run()?
    print(first.stdout())
    print(second.stdout())
    Ok(())
}
"#;
    let files = [("helper.align", helper), ("main.align", main)];
    let checked = diff_check_multi("verified-command", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() && cfg!(target_os = "linux") {
        for output in [
            build_and_run_multi("verified-command", &files, "main.align"),
            build_per_unit_multi("verified-command-unit", &files, "main.align").link_and_run(),
        ] {
            assert!(
                output.status.success(),
                "stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "true\ntrue\nsealed\nsealed\n"
            );
        }
    }
}

#[test]
fn borrowed_namespace_carriers_preserve_descriptor_authority() {
    let helper = r#"module helper
import std.fs
import std.process
Namespace { value: Option<process.user_namespace> }
Commands { first: command, second: command }
fn descriptors() -> Result<i64, Error> {
    entries := fs.read_dir("/proc/self/fd")?
    return Ok(entries.len())
}
fn invalid(value: Result<(), Error>) -> bool = match value {
    Err(error) => match error { Invalid => true, _ => false },
    Ok(_) => false,
}
fn attach(borrow mut target: command, borrow source: Option<process.user_namespace>, slot: i64, expected: i64) -> Result<(), Error> {
    return match source {
        None => if slot == 0 { Ok(()) } else { Err(Error.Invalid) },
        Some(value) => {
            if descriptors()? != expected { return Err(Error.Invalid) }
            target.inherit_namespace(value, slot)
        },
    }
}
fn attach_record(borrow mut target: command, borrow source: Namespace, slot: i64, expected: i64) -> Result<(), Error> {
    return match source.value {
        None => if slot == 0 { Ok(()) } else { Err(Error.Invalid) },
        Some(value) => {
            if descriptors()? != expected { return Err(Error.Invalid) }
            target.inherit_namespace(value, slot)
        },
    }
}
pub fn absent() -> Result<(), Error> {
    mut target := process.command("/bin/sh", ["sh", "-c", "exit 0"])
    source: Option<process.user_namespace> := None
    attach(target, source, 0, 0)?
    if !invalid(attach(target, source, 7, 0)) { return Err(Error.Invalid) }
    record := Namespace { value: source }
    attach_record(target, record, 0, 0)?
    if !invalid(attach_record(target, record, 7, 0)) { return Err(Error.Invalid) }
    return Ok(())
}
fn prepare() -> Result<Commands, Error> {
    mut first := process.command("/bin/sh", ["sh", "-c", "test -e /dev/fd/7 && printf ready"])
    mut second := process.command("/bin/sh", ["sh", "-c", "test -e /dev/fd/7 && printf ready"])
    first.timeout_ns(5_000_000_000)
    second.timeout_ns(5_000_000_000)
    before := descriptors()?
    source: Option<process.user_namespace> := Some(process.user_namespace("/proc/self/ns/user")?)
    if descriptors()? != before + 1 { return Err(Error.Invalid) }
    if !invalid(attach(first, source, 2, before + 1)) { return Err(Error.Invalid) }
    if descriptors()? != before + 1 { return Err(Error.Invalid) }
    attach(first, source, 7, before + 1)?
    if descriptors()? != before + 2 { return Err(Error.Invalid) }
    if !invalid(attach(first, source, 7, before + 2)) { return Err(Error.Invalid) }
    if descriptors()? != before + 2 { return Err(Error.Invalid) }
    record := Namespace { value: source }
    if !invalid(attach_record(second, record, 1024, before + 2)) { return Err(Error.Invalid) }
    if descriptors()? != before + 2 { return Err(Error.Invalid) }
    attach_record(second, record, 7, before + 2)?
    if descriptors()? != before + 3 { return Err(Error.Invalid) }
    if !invalid(attach_record(second, record, 7, before + 3)) { return Err(Error.Invalid) }
    if descriptors()? != before + 3 { return Err(Error.Invalid) }
    return Ok(Commands { first: first, second: second })
}
fn run_commands() -> Result<(), Error> {
    before := descriptors()?
    commands := prepare()?
    // The shared source has expired; only the two explicit command duplicates remain.
    if descriptors()? != before + 2 { return Err(Error.Invalid) }
    first_command := commands.first
    second_command := commands.second
    first := first_command.run()?
    second := second_command.run()?
    again_first := first_command.run()?
    again_second := second_command.run()?
    if first.stdout() != "ready" || second.stdout() != "ready" ||
        again_first.stdout() != "ready" || again_second.stdout() != "ready" {
        return Err(Error.Invalid)
    }
    return Ok(())
}
pub fn present() -> Result<(), Error> {
    before := descriptors()?
    absent()?
    if descriptors()? != before { return Err(Error.Invalid) }
    run_commands()?
    if descriptors()? != before { return Err(Error.Invalid) }
    return Ok(())
}
"#;
    let main = if cfg!(target_os = "linux") {
        "import helper\nfn main() -> Result<(), Error> { helper.present()?\nprint(true)\nOk(()) }\n"
    } else {
        "import helper\nfn main() -> Result<(), Error> { helper.absent()?\nprint(true)\nOk(()) }\n"
    };
    let files = [("helper.align", helper), ("main.align", main)];
    let checked = diff_check_multi("verified-borrowed-namespace", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        for output in [
            build_and_run_multi("verified-borrowed-namespace", &files, "main.align"),
            build_per_unit_multi("verified-borrowed-namespace-unit", &files, "main.align")
                .link_and_run(),
        ] {
            assert!(
                output.status.success(),
                "stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n");
        }
    }
}

#[test]
fn borrowed_namespace_payloads_cannot_move_or_escape() {
    for (name, parameter, selected) in [
        ("direct", "Option<process.user_namespace>", "source"),
        ("record", "Namespace", "source.value"),
    ] {
        for (action, body) in [
            (
                "move",
                format!(
                    "fn take(value: process.user_namespace) {{}}\nfn bad(borrow source: {parameter}) {{ match {selected} {{ None => {{}}, Some(value) => take(value), }} }}"
                ),
            ),
            (
                "escape",
                format!(
                    "fn bad(borrow source: {parameter}) -> Result<process.user_namespace, Error> {{ return match {selected} {{ None => Err(Error.Invalid), Some(value) => Ok(value), }} }}"
                ),
            ),
        ] {
            let source = format!(
                "import std.process\nNamespace {{ value: Option<process.user_namespace> }}\n{body}\nfn main() {{}}\n"
            );
            let checked = diff_check_multi(
                &format!("verified-borrowed-namespace-{name}-{action}"),
                &[("main.align", &source)],
                "main.align",
            );
            assert!(
                checked.whole_errors && checked.per_unit_errors,
                "{name}/{action} accepted: {}\n{}",
                checked.whole_diags,
                checked.per_unit_diags
            );
        }
    }
}

#[test]
fn formation_and_carriers() {
    for ty in [
        "fs.memory_writer",
        "fs.sealed_file",
        "process.image",
        "process.user_namespace",
        "command",
    ] {
        let helper = format!(
            r#"module helper
import std.fs
import std.process
pub Record {{ value: {ty} }}
pub Choice {{ Empty, Value(Record) }}
pub fn identity<T>(value: T) -> T = value
pub fn carry(value: {ty}, choose: bool) -> Result<Option<Choice>, Error> {{
    record := Record {{ value: identity(value) }}
    selected := if choose {{ Choice.Value(record) }} else {{ Choice.Empty }}
    Ok(Some(selected))
}}
pub fn discard(value: Result<Option<Choice>, Error>) {{}}
"#
        );
        let main = "import helper\nfn main() {}\n";
        let checked = diff_check_multi(
            &format!("verified-carrier-{ty}"),
            &[("helper.align", &helper), ("main.align", main)],
            "main.align",
        );
        assert!(
            !checked.whole_errors && !checked.per_unit_errors,
            "{ty}: {}\n{}",
            checked.whole_diags,
            checked.per_unit_diags
        );
        for (name, body) in [
            ("array", format!("fn bad(value: array<{ty}>) {{}}")),
            ("slice", format!("fn bad(value: slice<{ty}>) {{}}")),
            ("box", format!("fn bad(value: box<{ty}>) {{}}")),
            ("tuple", format!("fn bad(value: ({ty}, i64)) {{}}")),
            ("out", format!("fn bad(out value: {ty}) {{}}")),
            (
                "copy",
                format!(
                    "fn take(value: {ty}) {{}}\nfn bad(value: {ty}) {{ take(value)\ntake(value) }}"
                ),
            ),
        ] {
            let source = format!("import std.fs\nimport std.process\n{body}\nfn main() {{}}\n");
            let checked = diff_check_multi(
                &format!("verified-negative-{ty}-{name}"),
                &[("main.align", &source)],
                "main.align",
            );
            assert!(
                checked.whole_errors && checked.per_unit_errors,
                "{ty}/{name} accepted"
            );
        }
    }
}

#[test]
fn borrowed_arguments_remain_live_until_action() {
    for (name, body) in [
        (
            "image",
            "fn consume(value: process.image) -> str = \"gone\"\nfn bad(value: process.image) { process.command_image(value, [consume(value)]) }",
        ),
        (
            "namespace",
            "fn consume(value: process.user_namespace) -> i64 = 3\nfn bad(borrow mut command: command, value: process.user_namespace) { command.inherit_namespace(value, consume(value)) }",
        ),
        (
            "file",
            "fn consume(value: fs.sealed_file) -> i64 = 3\nfn bad(borrow mut command: command, value: fs.sealed_file) { command.inherit_file(value, consume(value)) }",
        ),
        (
            "seal-borrow",
            "fn bad(borrow value: fs.memory_writer) { value.seal() }",
        ),
        (
            "write-shared",
            "fn bad(borrow value: fs.memory_writer) { value.write(\"x\") }",
        ),
    ] {
        let source = format!("import std.fs\nimport std.process\n{body}\nfn main() {{}}\n");
        let checked = diff_check_multi(
            &format!("verified-action-{name}"),
            &[("main.align", &source)],
            "main.align",
        );
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{name} accepted: {}\n{}",
            checked.whole_diags,
            checked.per_unit_diags
        );
    }
}

#[test]
fn owned_control_flow_and_temporary_cleanup() {
    let source = r#"import std.fs
import std.process
Record { file: fs.sealed_file }
Choice { Empty, Value(Record) }
fn identity<T>(value:T) -> T = value
fn make() -> Result<fs.sealed_file,Error> {
    mut writer := fs.memory_file(fs.memory_kind.Executable, 16000000)?
    input := fs.open("/bin/true")?
    mut chunk := buffer(4096)
    loop {
        count := input.read(chunk)?
        if count == 0 { break }
        writer.write(chunk.bytes())?
    }
    writer.seal()
}
fn keep(error:Error) -> Error = error
fn early(file:fs.sealed_file) -> Result<(),Error> {
    zero:Result<(),Error> := Err(Error.Invalid)
    zero.map_err(keep)?
    print(file.len())
    Ok(())
}
fn iteration(branch:bool) -> Result<(),Error> {
    mut unfinished := fs.memory_file(fs.memory_kind.Data, 1)?
    unfinished = fs.memory_file(fs.memory_kind.Data, 0)?
    namespace := process.user_namespace("/proc/self/ns/user")?
    record := Record { file: identity(make()?) }
    selected := if branch { Choice.Value(record) } else { Choice.Empty }
    match selected {
        Value(value) => { process.executable(value.file)? },
        Empty => { fresh := make()?
process.executable(fresh)? }
    }
    optional:Option<fs.sealed_file> := Some(make()?)
    held := optional else { return Ok(()) }
    loop { early(held) else { break }
        break }
    source := make()?
    image := process.executable(source)?
    command := process.command_image(image,["true"])?
    command.run()?
    Ok(())
}
pub fn main() -> Result<(),Error> {
    before := fs.read_dir("/proc/self/fd")?
    mut index := 0
    loop {
        if index == 16 { break }
        iteration(index % 2 == 0)?
        index = index + 1
    }
    after := fs.read_dir("/proc/self/fd")?
    print(before.len() == after.len())
    Ok(())
}
"#;
    let checked = diff_check_multi(
        "verified-control-flow",
        &[("main.align", source)],
        "main.align",
    );
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() && cfg!(target_os = "linux") {
        for output in [
            build_and_run("verified-control-flow", source),
            build_per_unit_multi(
                "verified-control-flow-unit",
                &[("main.align", source)],
                "main.align",
            )
            .link_and_run(),
        ] {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n");
        }
    }
    if backend_available() && cfg!(target_os = "linux") {
        for per_unit in [false, true] {
            let project = Proj::new(
                "verified-drop-negative",
                &[("main.align", source)],
                "main.align",
            );
            let entry = project.dir.join("main.align");
            let mut map = SourceMap::new();
            let mut programs = if per_unit {
                let walk = build_per_unit(&mut map, &entry.display().to_string(), source);
                assert!(!walk.diags.has_errors());
                walk.units
                    .into_iter()
                    .map(|unit| unit.mir)
                    .collect::<Vec<_>>()
            } else {
                let checked = check(&mut map, &entry.display().to_string(), source);
                assert!(!checked.diags.has_errors());
                vec![lower_to_mir(&checked.hir)]
            };
            let mut removed = 0;
            for program in &mut programs {
                for function in &mut program.fns {
                    if !function.name.as_str().ends_with("early") {
                        continue;
                    }
                    for block in &mut function.blocks {
                        block.stmts.retain(|statement| {
                            let omit=matches!(statement,align_mir::Stmt::Drop(slot) if function.slots[*slot as usize]==align_sema::Ty::FsSealedFile);
                            if omit { removed+=1; }
                            !omit
                        });
                        block.stmt_lines.clear();
                    }
                }
            }
            assert!(removed > 0, "negative control removed no sealed Drop");
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
                .expect("emit negative");
                objects.push(object);
                for library in &program.link_libs {
                    if !libraries.contains(library) {
                        libraries.push(library.clone());
                    }
                }
            }
            let executable = project.dir.join("negative");
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
            .expect("link negative");
            let output = std::process::Command::new(executable)
                .output()
                .expect("run negative");
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "false\n",
                "omitted Drop must be observable"
            );
        }
    }
}

#[test]
fn current_image_binding() {
    if !backend_available() || !cfg!(target_os = "linux") {
        return;
    }
    let source = r#"import std.process
extern "C" fn unlink_owned_main() -> i64
pub fn main() -> Result<(),Error> {
    before := process.current_image()?
    print(unsafe { unlink_owned_main() } == 0)
    after := process.current_image()?
    mut first := buffer(4)
    mut second := buffer(4)
    print(before.read(first)? == 4)
    print(after.read(second)? == 4)
    print(first.bytes()[0] == second.bytes()[0])
    print(second.bytes()[0])
    Ok(())
}
"#;
    // Only the executable created and owned by this fixture is unlinked. Its
    // still-open main-image object remains readable through the kernel binding.
    let fixture = r#"
#define _POSIX_C_SOURCE 200809L
#include <unistd.h>
#include <stdint.h>
int64_t unlink_owned_main(void) {
    char path[4096];
    ssize_t n=readlink("/proc/self/exe",path,sizeof(path)-1);
    if(n<=0 || n>=(ssize_t)sizeof(path)-1) return -1;
    path[n]=0;
    return unlink(path);
}
"#;
    for output in [
        build_and_run_with_c("verified-unlinked-main", source, fixture),
        build_and_run_multi_with_c(
            "verified-unlinked-main-unit",
            &[("main.align", source)],
            "main.align",
            fixture,
        ),
    ] {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "true\ntrue\ntrue\ntrue\n127\n"
        );
    }
}

#[test]
fn unsupported_platform_uses_the_shared_error_code() {
    let source = r#"import std.fs
import std.process
fn show(error:Error) { match error { Code(value) => print(value), _ => print(-1) } }
fn main() {
    match fs.memory_file(fs.memory_kind.Data,0) { Err(error) => show(error), Ok(value) => print(-1) }
    match process.user_namespace("/valid") { Err(error) => show(error), Ok(value) => print(-1) }
    match process.current_image() { Err(error) => show(error), Ok(value) => print(-1) }
}
"#;
    let checked = diff_check_multi(
        "verified-unsupported-check",
        &[("main.align", source)],
        "main.align",
    );
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if !backend_available() || !cfg!(target_os = "macos") {
        return;
    }
    for output in [
        build_and_run("verified-unsupported", source),
        build_per_unit_multi(
            "verified-unsupported-unit",
            &[("main.align", source)],
            "main.align",
        )
        .link_and_run(),
    ] {
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("{0}\n{0}\n{0}\n", libc::ENOTSUP)
        );
    }
}
