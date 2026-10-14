//! Plan180: task publication during caller work and shared-backing access across interfaces.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
pub fn observe(text: str) -> i64 = text.len()
pub fn read(values: slice<i64>) -> i64 = values[0]
pub fn change(values: slice<i64>) { mut alias := values; alias[0] = 9 }
pub fn identity<T>(value: T) -> T = value
"#;

const SOURCE: &str = r#"module main
import model
extern "C" fn task_signal(round: i64)
extern "C" fn task_observe(round: i64) -> i32
fn main() -> i32 {
  owned := "captured".clone()
  view: str := owned
  task_group {
    first := spawn(fn { unsafe { task_signal(1) }; model.observe(model.identity(view)) })
    if unsafe { task_observe(1) } != 0 { return 1 }
    wait()
    if first.get() != 8 { return 2 }
    second := spawn(fn { unsafe { task_signal(2) }; model.observe(view) })
    if unsafe { task_observe(2) } != 0 { return 3 }
    wait()
    if second.get() != 8 || first.get() != 8 { return 4 }
  }
  return 0
}
"#;

const NATIVE: &str = r#"
#include <pthread.h>
#include <stdint.h>
#include <time.h>
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t changed = PTHREAD_COND_INITIALIZER;
static int64_t seen = 0;
void task_signal(int64_t round) {
    pthread_mutex_lock(&lock);
    seen = round;
    pthread_cond_signal(&changed);
    pthread_mutex_unlock(&lock);
}
int32_t task_observe(int64_t round) {
    struct timespec deadline;
    clock_gettime(CLOCK_REALTIME, &deadline);
    deadline.tv_sec += 5;
    pthread_mutex_lock(&lock);
    while (seen < round) {
        if (pthread_cond_timedwait(&changed, &lock, &deadline) != 0) {
            pthread_mutex_unlock(&lock);
            return 1;
        }
    }
    int32_t result = seen != round;
    pthread_mutex_unlock(&lock);
    return result;
}
"#;

fn execute(stage: &Path, programs: &[align_mir::Program], native: &Path, label: &str) {
    let mut objects = vec![native.to_path_buf()];
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{index}.o"));
        align_driver::emit_object_file(program, &object, BuildTarget::Baseline, Profile::Release, &[], false).unwrap();
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) { libraries.push(library.clone()); }
        }
    }
    let binary = stage.join(label);
    align_driver::link_objects(&align_driver::CDriver::default(),
        &objects.iter().map(|path| path.as_path()).collect::<Vec<_>>(),
        &binary, &libraries, Profile::Release).unwrap();
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(output.status.success(), "{label}: {output:?}");
}

#[test]
fn tasks_progress_before_wait_across_compilation_modes() {
    owned_fixture::run("tasks_progress_before_wait_across_compilation_modes", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, SOURCE).unwrap();
        let native_source = stage.join("signal.c");
        let native = stage.join("signal.o");
        std::fs::write(&native_source, NATIVE).unwrap();
        let output = std::process::Command::new(align_driver::CDriver::default().program())
            .args(["-c", "-pthread"]).arg(&native_source).arg("-o").arg(&native).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, entry.to_str().unwrap(), SOURCE);
        assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &checked.diags));
        let whole = align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[whole], &native, "whole");
        let mut sources = SourceMap::new();
        let units = align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), SOURCE);
        assert!(!units.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &units.diags));
        execute(stage, &units.units.into_iter().map(|unit| unit.mir).collect::<Vec<_>>(), &native, "units");
    });
}

#[test]
fn imported_access_facts_reject_shared_writes() {
    owned_fixture::run("imported_access_facts_reject_shared_writes", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        for (index, (body, valid)) in [
            ("spawn(fn { model.read(view) }); model.change(view); wait()", false),
            ("spawn(fn { model.change(view) }); wait()", false),
            ("spawn(fn { model.read(view) }); wait(); model.change(view)", true),
            ("spawn(fn { model.read(view) }); model.read(view); wait()", true),
            ("spawn(fn { model.read(view) }); model.change(model.identity(view)); wait()", false),
            ("spawn(fn { model.read(view) }); [1].map(fn x { model.identity(Holder { values: view }) }).map(fn item { model.change(item.values); 1 }).sum(); wait()", false),
            ("spawn(fn { model.read(view) }); wait(); [1].map(fn x { model.identity(Holder { values: view }) }).map(fn item { model.change(item.values); 1 }).sum()", true),
            ("spawn(fn { local := view; [1].map(fn x { model.identity(Holder { values: local }) }).map(fn item { model.change(item.values); 1 }).sum() }); wait()", false),
            ("spawn(fn { local := view; [1].map(fn x { model.identity(Holder { values: local }) }).map(fn item { model.read(item.values) }).sum() }); wait()", true),
            ("spawn(fn { model.read(view) }); { rows := [1].map(fn x { model.identity(Holder { values: view }) }).to_array(); rows.map(fn item { model.change(item.values); 1 }).sum() }; wait()", false),
            ("spawn(fn { model.read(view) }); wait(); { rows := [1].map(fn x { model.identity(Holder { values: view }) }).to_array(); rows.map(fn item { model.change(item.values); 1 }).sum() }", true),
            ("spawn(fn { local := view; { rows := [1].map(fn x { model.identity(Holder { values: local }) }).to_array(); rows.map(fn item { model.change(item.values); 1 }).sum() } }); wait()", false),
            ("spawn(fn { local := view; { rows := [1].map(fn x { model.identity(Holder { values: local }) }).to_array(); rows.map(fn item { model.read(item.values) }).sum() } }); wait()", true),
        ].into_iter().enumerate() {
            let source = format!("module main\nimport model\nHolder {{ values: slice<i64> }}\nfn main() {{ mut values := [1,2]; view := values[..]; task_group {{ {body}; () }} }}");
            let entry = stage.join(format!("input-{index}.align"));
            std::fs::write(&entry, &source).unwrap();
            let mut sources = SourceMap::new();
            let checked = align_driver::check(&mut sources, entry.to_str().unwrap(), &source);
            let messages = align_driver::format_diagnostics(&sources, &checked.diags);
            assert_eq!(!checked.diags.has_errors(), valid, "whole {index}: {messages}");
            if !valid { assert!(messages.contains("spawned task"), "whole {index}: {messages}"); }
            let mut sources = SourceMap::new();
            let built = align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), &source);
            let messages = align_driver::format_diagnostics(&sources, &built.diags);
            assert_eq!(!built.diags.has_errors(), valid, "{index}: {messages}");
            if !valid { assert!(messages.contains("spawned task"), "{index}: {messages}"); }
        }
    });
}
