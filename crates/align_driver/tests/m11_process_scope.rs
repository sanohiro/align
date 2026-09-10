//! Exclusive child scopes retain kernel-authenticated process authority.
mod common;
use common::*;

#[test]
fn lifecycle_and_whole_per_unit_transport() {
    let helper = r#"module helper
import std.process
pub fn stop(borrow member:process.member) -> Result<(),Error> {
    if member.finished()? { return Ok(()) }
    member.kill(process.signal_number(process.signal.Kill))
}
pub fn pass_entry(entry:process.member_info) -> process.member_info = entry
pub fn pass_reaped(event:process.reaped) -> process.reaped = event
pub fn stop_entry(borrow entry:process.member_info) -> Result<(),Error> = stop(entry.handle)
pub fn cleanup(borrow mut scope:process.child_scope) -> Result<(),Error> {
    loop {
        entries := scope.children(4096)?
        mut index := 0
        loop {
            if index == entries.len() { break }
            stop_entry(entries[index])?
            index = index + 1
        }
        scope.reap(4096)?
        if scope.release()? { break }
    }
    Ok(())
}
pub fn make(borrow command:command) -> Result<process.child_scope,Error> = command.start_scope()
pub fn identity<T>(value:T) -> T = value
pub fn discard(value:process.child_scope) {}
"#;
    let main = r#"import std.process
import helper
pub fn main() -> Result<(),Error> {
    command := process.command("/bin/sh",["sh","-c","exec sleep 30"])
    mut scope := helper.identity(helper.make(command)?)
    print(scope.id() > 0)
    print(scope.owner_id() > 0)
    print(match helper.make(command) {Err(error) => true,Ok(other) => false})
    print(scope.release()? == false)
    helper.cleanup(scope)?
    print(scope.release()?)
    print(match scope.status()? {Some(result) => true,None => false})
    mut next := helper.make(command)?
    helper.discard(scope)
    helper.cleanup(next)?
    print(next.release()?)
    Ok(())
}
"#;
    let files = [("helper.align", helper), ("main.align", main)];
    let checked = diff_check_multi("scope-lifecycle", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() && cfg!(target_os = "linux") {
        for output in [
            build_and_run_multi("scope-lifecycle", &files, "main.align"),
            build_per_unit_multi("scope-lifecycle-unit", &files, "main.align").link_and_run(),
        ] {
            assert!(
                output.status.success(),
                "stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n"
            );
        }
    }
}

#[test]
fn formation_and_carriers() {
    for ty in ["process.child_scope", "process.member"] {
        let helper = format!(
            r#"module helper
import std.process
pub Record {{ value:{ty} }}
pub Choice {{ Empty,Value(Record) }}
pub fn identity<T>(value:T) -> T = value
pub fn carry(value:{ty},branch:bool) -> Result<Option<Choice>,Error> {{
    record := Record {{value:identity(value)}}
    selected := if branch {{Choice.Value(record)}} else {{Choice.Empty}}
    Ok(Some(selected))
}}
"#
        );
        let checked = diff_check_multi(
            &format!("scope-carrier-{ty}"),
            &[
                ("helper.align", &helper),
                ("main.align", "import helper\nfn main() {}\n"),
            ],
            "main.align",
        );
        assert!(
            !checked.whole_errors && !checked.per_unit_errors,
            "{}\n{}",
            checked.whole_diags,
            checked.per_unit_diags
        );
        for (name, body) in [
            ("array", format!("fn bad(value:array<{ty}>) {{}}")),
            ("tuple", format!("fn bad(value:({ty},i64)) {{}}")),
            ("box", format!("fn bad(value:box<{ty}>) {{}}")),
            ("out", format!("fn bad(out value:{ty}) {{}}")),
            (
                "copy",
                format!(
                    "fn take(value:{ty}) {{}}\nfn bad(value:{ty}) {{take(value)\ntake(value)}}"
                ),
            ),
        ] {
            let source = format!("import std.process\n{body}\nfn main() {{}}\n");
            let checked = diff_check_multi(
                &format!("scope-negative-{ty}-{name}"),
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
fn zombie_leader_is_not_process_completion() {
    if !backend_available() || !cfg!(target_os = "linux") {
        return;
    }
    let source = r#"import std.process
extern "C" fn scope_test_threaded_exit() -> i64
fn finished(borrow member:process.member) -> Result<bool,Error> = member.finished()
fn entry_finished(borrow entry:process.member_info) -> Result<bool,Error> = finished(entry.handle)
fn kill(borrow member:process.member) -> Result<(),Error> = member.kill(process.signal_number(process.signal.Kill))
fn kill_entry(borrow entry:process.member_info) -> Result<(),Error> = kill(entry.handle)
pub fn main(args:array<str>) -> Result<(),Error> {
    if args.len() > 1 { unsafe {scope_test_threaded_exit()}
return Ok(()) }
    command := process.command(args[0],[args[0],"thread-child"])
    mut scope := command.start_scope()?
    ready := scope.poll(process.readiness {stdout:true,stderr:false,status:false},5000000000)?
    print(ready.stdout)
    zero:u8 := 0
    mut bytes := [zero]
    print(match scope.read_stdout(bytes)? {Some(count) => count == 1,None => false})
    print(match scope.status()? {None => true,Some(value) => false})
    entries := scope.children(4096)?
    print(entries.len() == 1)
    print(entry_finished(entries[0])? == false)
    kill_entry(entries[0])?
    loop {scope.reap(4096)?
if scope.release()? {break}}
    print(entry_finished(entries[0])?)
    Ok(())
}
"#;
    let fixture = r#"
#define _GNU_SOURCE
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/prctl.h>
#include <unistd.h>
#include <stdlib.h>
static void *worker(void *unused) {
    (void)unused;
    char path[128], text[1024];
    snprintf(path,sizeof(path),"/proc/self/task/%d/stat",getpid());
    for(;;) {
        FILE *file=fopen(path,"r");
        if(!file) abort();
        char *line=fgets(text,sizeof(text),file);
        fclose(file);
        char *end=line?strrchr(text,')'):NULL;
        if(end && end[1]==' ' && end[2]=='Z') break;
        sched_yield();
    }
    if(write(1,"R",1)!=1) abort();
    for(;;) pause();
    return NULL;
}
int64_t scope_test_threaded_exit(void) {
    if(prctl(PR_GET_NO_NEW_PRIVS,0,0,0,0)!=1) abort();
    pthread_t thread;
    if(pthread_create(&thread,NULL,worker,NULL)!=0) abort();
    syscall(SYS_exit,0);
    abort();
}
"#;
    for output in [
        build_and_run_with_c("scope-zombie-leader", source, fixture),
        build_and_run_multi_with_c(
            "scope-zombie-leader-unit",
            &[("main.align", source)],
            "main.align",
            fixture,
        ),
    ] {
        assert!(
            output.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "true\ntrue\ntrue\ntrue\ntrue\ntrue\n"
        );
    }
}

#[test]
fn owned_control_flow() {
    let helper = r#"module helper
import std.process
pub Record {scope:process.child_scope}
pub Choice {Empty,Held(Record)}
pub fn make() -> Result<process.child_scope,Error> {
    command := process.command("/bin/sh",["sh","-c","exec sleep 30"])
    command.start_scope()
}
pub fn early(scope:process.child_scope) -> Result<(),Error> {
    failed:Result<(),Error> := Err(Error.Invalid)
    failed?
    print(scope.id())
    Ok(())
}
pub fn drop_choice(value:Choice) {}
pub fn exercise(branch:bool) -> Result<(),Error> {
    record := Record {scope:make()?}
    selected := if branch {Choice.Held(record)} else {Choice.Empty}
    drop_choice(selected)
    // The branch that did not transfer record retains it until function exit.
    Ok(())
}
"#;
    let main = r#"import helper
import std.process
import std.fs
fn keep(error:Error) -> Error = error
pub fn main() -> Result<(),Error> {
    before := fs.read_dir("/proc/self/fd")?
    mut index := 0
    loop {
        if index == 8 {break}
        helper.exercise(index % 2 == 0)?
        helper.early(helper.make()?).map_err(keep) else {}
        mut pending:Option<process.child_scope> := Some(helper.make()?)
        pending = None
        pending = Some(helper.make()?)
        match pending {Some(scope) => {helper.early(scope) else {}},None => {}}
        index = index + 1
    }
    after := fs.read_dir("/proc/self/fd")?
    print(before.len() == after.len())
    Ok(())
}
"#;
    let files = [("helper.align", helper), ("main.align", main)];
    let checked = diff_check_multi("scope-control-flow", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() && cfg!(target_os = "linux") {
        for output in [
            build_and_run_multi("scope-control-flow", &files, "main.align"),
            build_per_unit_multi("scope-control-flow-unit", &files, "main.align").link_and_run(),
        ] {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n");
        }
    }
}

#[test]
fn clone_parent_is_owned_and_reaped() {
    if !backend_available() || !cfg!(target_os = "linux") {
        return;
    }
    let source = r#"import std.process
extern "C" fn scope_test_clone_parent() -> i64
fn stop(borrow member:process.member) -> Result<(),Error> = member.kill(process.signal_number(process.signal.Kill))
fn stop_entry(borrow entry:process.member_info) -> Result<(),Error> = stop(entry.handle)
pub fn main(args:array<str>) -> Result<(),Error> {
    if args.len() > 1 {unsafe {scope_test_clone_parent()}
return Ok(())}
    command := process.command(args[0],[args[0],"clone-child"])
    mut scope := command.start_scope()?
    loop {if match scope.status()? {Some(value) => true,None => false} {break}}
    scope.reap(1)?
    print(scope.release()? == false)
    entries := scope.children(4096)?
    print(entries.len() == 1)
    stop_entry(entries[0])?
    mut count := 0
    loop {
        rows := scope.reap(4096)?
        count = count + rows.len()
        if scope.release()? {break}
    }
    print(count == 1)
    Ok(())
}
"#;
    let fixture = r#"
#define _GNU_SOURCE
#include <stdint.h>
#include <sched.h>
#include <sys/syscall.h>
#include <unistd.h>
#include <stdlib.h>
int64_t scope_test_clone_parent(void) {
    long pid=syscall(SYS_clone,CLONE_PARENT,0,0,0,0);
    if(pid<0) abort();
    if(pid==0) {for(;;) pause();}
    return 0;
}
"#;
    for output in [
        build_and_run_with_c("scope-clone-parent", source, fixture),
        build_and_run_multi_with_c(
            "scope-clone-parent-unit",
            &[("main.align", source)],
            "main.align",
            fixture,
        ),
    ] {
        assert!(
            output.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "true\ntrue\ntrue\n"
        );
    }
}
