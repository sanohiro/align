//! `par_map(f)` — apply a Pure function to each element, materializing an owned `array<R>`
//! (`draft.md` §11). The Pure requirement is enforced by effect/purity inference. The first cut
//! runs sequentially (real thread-parallel execution is a runtime follow-up).

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
fn par_map_pure_function() {
    if !backend_available() {
        return;
    }
    // par_map a pure function over an array, then sum: 2 + 4 + 6 = 12.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  doubled := [1, 2, 3].par_map(dbl)\n  print(doubled.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-pure", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
}

#[test]
fn par_map_after_where() {
    if !backend_available() {
        return;
    }
    // Stages before par_map compose: keep >2, then *10 → [30, 40, 50]; sum = 120.
    let src = "fn big(x: i64) -> bool = x > 2\nfn dec(x: i64) -> i64 = x * 10\nfn main() -> Result<(), Error> {\n  out := [1, 2, 3, 4, 5].where(big).par_map(dec)\n  print(out.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-where", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "120\n");
}

#[test]
fn par_map_over_struct_field() {
    if !backend_available() {
        return;
    }
    // par_map a struct-consuming pure function (multi-field) → array<i32>; sum = (10+5)+(20+7)=42.
    let src = "Emp { base: i32, bonus: i32 }\nfn net(e: Emp) -> i32 = e.base + e.bonus\nfn main() -> Result<(), Error> {\n  ns := [Emp{base: 10, bonus: 5}, Emp{base: 20, bonus: 7}].par_map(net)\n  print(ns.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-struct", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn par_map_chained_into_reduction_frees_intermediate() {
    if !backend_available() {
        return;
    }
    // `arr.par_map(f).sum()` — the par_map result is a fresh owned array consumed by `sum`; it
    // must be freed (`drop_value`), not leaked. 2 + 4 + 6 = 12.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  print([1, 2, 3].par_map(dbl).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-chain", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
    // The consumed intermediate buffer is freed (no leak).
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("drop_value"), "the par_map intermediate must be freed:\n{text}");
}

// --- purity (Pure requirement) ---

#[test]
fn par_map_impure_function_rejected() {
    // A function that prints has a side effect — rejected by the Pure requirement.
    let src = "fn noisy(x: i64) -> i64 {\n  print(x)\n  return x\n}\nfn main() -> Result<(), Error> {\n  ys := [1, 2].par_map(noisy)\n  print(ys.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("pm-impure", src));
}

#[test]
fn par_map_transitively_impure_rejected() {
    // Purity is transitive: `mid` calls `leaf` which prints, so `mid` is impure too.
    let src = "fn leaf(x: i64) -> i64 {\n  print(x)\n  return x\n}\nfn mid(x: i64) -> i64 = leaf(x) + 1\nfn main() -> Result<(), Error> {\n  ys := [1, 2].par_map(mid)\n  print(ys.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("pm-trans", src));
}

#[test]
fn par_map_calling_pure_helper_ok() {
    if !backend_available() {
        return;
    }
    // A pure function that calls another pure function is still Pure — accepted.
    let src = "fn inc(x: i64) -> i64 = x + 1\nfn step(x: i64) -> i64 = inc(x) * 2\nfn main() -> Result<(), Error> {\n  ys := [1, 2, 3].par_map(step)\n  print(ys.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-purehelper", src);
    assert_eq!(out.status.code(), Some(0));
    // (1+1)*2 + (2+1)*2 + (3+1)*2 = 4 + 6 + 8 = 18
    assert_eq!(String::from_utf8_lossy(&out.stdout), "18\n");
}
