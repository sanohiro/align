//! `partition(p)` — split a pipeline's surviving elements into two owned arrays `(array<T>,
//! array<T>)` (predicate true, then false) in one fused loop. Built on owned-tuple support.

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
fn partition_even_odd() {
    if !backend_available() {
        return;
    }
    // [1..5] split by is_even → evens (2,4) sum 6 / len 2, odds (1,3,5) sum 9 / len 3.
    let src = "fn is_even(x: i64) -> bool = x % 2 == 0\nfn main() -> Result<(), Error> {\n  (evens, odds) := [1, 2, 3, 4, 5].partition(is_even)\n  print(evens.sum())\n  print(odds.sum())\n  print(evens.len())\n  print(odds.len())\n  return Ok(())\n}\n";
    let out = build_and_run("part-eo", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n9\n2\n3\n");
}

#[test]
fn partition_after_map() {
    if !backend_available() {
        return;
    }
    // map *2 → [2,4,6,8,10], then partition by >5: big (6,8,10) sum 24 / small (2,4) sum 6.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn big(x: i64) -> bool = x > 5\nfn main() -> Result<(), Error> {\n  (b, s) := [1, 2, 3, 4, 5].map(dbl).partition(big)\n  print(b.sum())\n  print(s.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("part-map", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "24\n6\n");
}

#[test]
fn partition_inside_arena_no_double_free() {
    if !backend_available() {
        return;
    }
    // Regression: inside an arena the two buffers are arena-allocated (bulk-freed), so the
    // destructured locals must inherit the arena region and NOT also be dropped — else a
    // double-free. positives (3,4,5) sum 12 / negatives (-1,-2) sum -3.
    let src = "fn pos(x: i64) -> bool = x > 0\nfn main() -> Result<(), Error> {\n  arena {\n    (p, n) := [3, -1, 4, -2, 5].partition(pos)\n    print(p.sum())\n    print(n.sum())\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("part-arena", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n-3\n");
}

#[test]
fn partition_all_one_side() {
    if !backend_available() {
        return;
    }
    // A predicate true for every element: one array gets all, the other is empty (len 0, sum 0).
    let src = "fn yes(x: i64) -> bool = true\nfn main() -> Result<(), Error> {\n  (a, b) := [4, 5, 6].partition(yes)\n  print(a.sum())\n  print(b.len())\n  return Ok(())\n}\n";
    let out = build_and_run("part-all", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "15\n0\n");
}

// --- diagnostics ---

#[test]
fn partition_wrong_arg_count_errors() {
    assert!(check_errs("part-arity", "fn main() -> i32 {\n  (a, b) := [1, 2].partition()\n  return 0\n}\n"));
}

#[test]
fn partition_over_struct_element_errors() {
    let src = "Emp { pay: i32, active: bool }\nfn keep(p: i32) -> bool = p > 0\nfn main() -> i32 {\n  (a, b) := [Emp{pay: 1, active: true}].partition(keep)\n  return 0\n}\n";
    assert!(check_errs("part-struct", src));
}
