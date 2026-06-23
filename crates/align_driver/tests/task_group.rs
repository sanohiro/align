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
fn get_before_wait_rejected() {
    // ④c: a task's result is ready only after `wait()` joins — `get()` before it is rejected.
    assert!(check_errs(
        "tg-get-before-wait",
        "fn main() -> Result<(), Error> {\n  task_group {\n    a := spawn(fn { 1 })\n    print(a.get())\n    wait()\n  }\n  return Ok(())\n}\n"
    ));
}

#[test]
fn wait_in_both_branches_dominates() {
    if !backend_available() {
        return;
    }
    // `wait()` on every path before the `get()` → accepted (dominance).
    let src = "fn main() -> Result<(), Error> {\n  c := true\n  task_group {\n    a := spawn(fn { 5 })\n    if c { wait() } else { wait() }\n    print(a.get())\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("tg-dom-ok", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}

#[test]
fn wait_in_one_branch_rejected() {
    // `wait()` on only one path does not dominate the `get()` → rejected (sound, not just linear).
    assert!(check_errs(
        "tg-dom-bad",
        "fn main() -> Result<(), Error> {\n  c := true\n  task_group {\n    a := spawn(fn { 5 })\n    if c { wait() }\n    print(a.get())\n  }\n  return Ok(())\n}\n"
    ));
}

#[test]
fn lambda_wait_does_not_leak_to_enclosing() {
    // A `wait()` inside a lambda body must not set the enclosing task_group's wait-state at compile
    // time (the lambda is a separate function body); the enclosing `get()` is still rejected.
    assert!(check_errs(
        "tg-lambda-leak",
        "fn main() -> Result<(), Error> {\n  task_group {\n    a := spawn(fn { 1 })\n    f := fn { wait() }\n    print(a.get())\n    wait()\n  }\n  return Ok(())\n}\n"
    ));
}

#[test]
fn else_unwrap_conditional_spawn_rejected() {
    // A `spawn` in a conditional `else`-unwrap fallback clears the wait-state by dominance, so a
    // later `get()` of the conditionally-respawned task is rejected (no bypass).
    assert!(check_errs(
        "tg-else-cond-spawn",
        "fn main() -> Result<(), Error> {\n  task_group {\n    mut t := spawn(fn { 1 })\n    wait()\n    opt := None\n    val := opt else {\n      t = spawn(fn { 2 })\n      0\n    }\n    print(t.get())\n  }\n  return Ok(())\n}\n"
    ));
}

#[test]
fn fallible_tasks_all_ok() {
    if !backend_available() {
        return;
    }
    // Tasks returning Result<R, Error>; all Ok → wait()? continues, get() yields the Ok values.
    let src = "fn try_n(n: i64) -> Result<i64, Error> {\n  if n < 0 { return Err(error(7)) }\n  return Ok(n * 2)\n}\nfn main() -> Result<(), Error> {\n  task_group {\n    a := spawn(fn { try_n(5) })\n    b := spawn(fn { try_n(10) })\n    wait()?\n    print(a.get() + b.get())\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("tg-fallible-ok", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "30\n");
}

#[test]
fn fallible_task_err_propagates() {
    if !backend_available() {
        return;
    }
    // One task fails → wait()? propagates its Err out of main → exit code 7, nothing printed.
    let src = "fn try_n(n: i64) -> Result<i64, Error> {\n  if n < 0 { return Err(error(7)) }\n  return Ok(n * 2)\n}\nfn main() -> Result<(), Error> {\n  task_group {\n    a := spawn(fn { try_n(5) })\n    b := spawn(fn { try_n(-1) })\n    wait()?\n    print(a.get() + b.get())\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("tg-fallible-err", src);
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn fallible_bare_wait_then_get_rejected() {
    // A fallible group: a bare `wait()` (Err ignored) does not make `get()` safe — needs `wait()?`.
    assert!(check_errs(
        "tg-fallible-barewait",
        "fn ok() -> Result<i64, Error> = Ok(5)\nfn main() -> Result<(), Error> {\n  task_group {\n    a := spawn(fn { ok() })\n    wait()\n    print(a.get())\n  }\n  return Ok(())\n}\n"
    ));
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
