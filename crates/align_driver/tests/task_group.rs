//! `task_group` structured concurrency (slice ④a — walking skeleton). `spawn(fn { … })` returns
//! a `Task<R>`; `wait()` joins; `t.get()` reads the result. ④a runs tasks eagerly/sequentially
//! (correct results; real threads arrive in ④b). `spawn`/`wait` are valid only inside the scope.

use align_driver::{backend_available, check, emit_object_file, link_executable, lower_to_mir};
use align_span::SourceMap;

fn build_and_run(name: &str, src: &str) -> std::process::Output {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let obj = dir.join(format!("align-test-{name}.o"));
    let exe = dir.join(format!("align-test-{name}"));
    emit_object_file(&mir, &obj).expect("codegen");
    link_executable(&obj, &exe).expect("link");
    let out = std::process::Command::new(&exe).output().expect("run");
    let _ = std::fs::remove_file(&obj);
    let _ = std::fs::remove_file(&exe);
    out
}

fn check_errs(name: &str, src: &str) -> bool {
    let mut sm = SourceMap::new();
    check(&mut sm, name, src).diags.has_errors()
}

#[test]
fn spawn_wait_get() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  task_group {\n    a := spawn(fn { 21 + 21 })\n    wait()\n    print(a.get())\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("tg-basic", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn multiple_capturing_tasks() {
    if !backend_available() {
        return;
    }
    // Each spawned closure captures `k` by value; results combined after the join.
    let src = "fn main() -> Result<(), Error> {\n  k: i64 := 100\n  task_group {\n    a := spawn(fn { k + 5 })\n    b := spawn(fn { k * 2 })\n    wait()\n    print(a.get() + b.get())\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("tg-multi", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "305\n");
}

#[test]
fn unit_returning_side_effect_task() {
    if !backend_available() {
        return;
    }
    // A fire-and-forget side-effect task returns `()` (a primitive scalar — box-able).
    let src = "fn main() -> Result<(), Error> {\n  x: i64 := 7\n  task_group {\n    a := spawn(fn { print(x) })\n    wait()\n    a.get()\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("tg-unit", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn tasks_run_deferred_at_wait() {
    if !backend_available() {
        return;
    }
    // ④b-1b: a spawned task runs at `wait()`, not at `spawn` — so the side effect prints after
    // the statements between `spawn` and `wait` (matching the eventual "complete by wait" model).
    let src = "fn main() -> Result<(), Error> {\n  task_group {\n    a := spawn(fn { print(1) })\n    print(2)\n    wait()\n    a.get()\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("tg-deferred", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n1\n");
}

#[test]
fn many_parallel_tasks() {
    if !backend_available() {
        return;
    }
    // ④b-2: each task runs on its own worker thread (joined at `wait`); results are read after the
    // join, so the sum is deterministic regardless of thread interleaving.
    let src = "fn main() -> Result<(), Error> {\n  k: i64 := 10\n  task_group {\n    a := spawn(fn { k + 1 })\n    b := spawn(fn { k + 2 })\n    c := spawn(fn { k + 3 })\n    d := spawn(fn { k + 4 })\n    wait()\n    print(a.get() + b.get() + c.get() + d.get())\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("tg-parallel", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "50\n");
}

#[test]
fn early_return_joins_tasks() {
    if !backend_available() {
        return;
    }
    // An early `return` out of a `task_group` still joins its tasks (structured concurrency):
    // the spawned side effect runs during the exit cleanup.
    let src = "fn main() -> Result<(), Error> {\n  task_group {\n    spawn(fn { print(9) })\n    return Ok(())\n  }\n}\n";
    let out = build_and_run("tg-early-return", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "9\n");
}

#[test]
fn owned_payload_task_rejected() {
    // ④b-1a: a task result is boxed in the region, so it must be a primitive scalar for now;
    // an owned result (`string`) is rejected (the region drop/borrow handling is a later slice).
    assert!(check_errs(
        "tg-owned",
        "fn main() -> Result<(), Error> {\n  task_group {\n    s := spawn(fn { \"hi\".clone() })\n    wait()\n    return Ok(())\n  }\n}\n"
    ));
}

#[test]
fn task_cannot_escape_scope() {
    // A `Task` handle is a box in the task_group region — it cannot escape as the block's value
    // (it would outlive the region). (Reading the scalar result with `.get()` is fine.)
    assert!(check_errs(
        "tg-escape",
        "fn main() -> Result<(), Error> {\n  t := task_group {\n    a := spawn(fn { 42 })\n    wait()\n    a\n  }\n  return Ok(())\n}\n"
    ));
}

#[test]
fn spawn_outside_task_group_rejected() {
    assert!(check_errs(
        "tg-outside",
        "fn main() -> Result<(), Error> {\n  a := spawn(fn { 1 })\n  return Ok(())\n}\n"
    ));
}

#[test]
fn wait_outside_task_group_rejected() {
    assert!(check_errs(
        "tg-wait-outside",
        "fn main() -> Result<(), Error> {\n  wait()\n  return Ok(())\n}\n"
    ));
}
