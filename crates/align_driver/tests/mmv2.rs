//! Memory Model v2 end-to-end tests. Slice 3: owned `array<T>` via `.to_array()`
//! (arena-bump-allocated), consumed by `.sum()` / `.len()` (reusing the slice path).

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

#[test]
fn to_array_map_where_then_sum() {
    if !backend_available() {
        return;
    }
    // map *2 → [2,4,6,8,10]; where >4 → [6,8,10]; to_array materializes; sum = 24.
    let src = "fn double(x: i32) -> i32 = x * 2\nfn big(x: i32) -> bool = x > 4\nfn main() -> i32 {\n  arena {\n    return [1, 2, 3, 4, 5].map(double).where(big).to_array().sum()\n  }\n}\n";
    let out = build_and_run("to-array-sum", src);
    assert_eq!(out.status.code(), Some(24));
}

#[test]
fn to_array_bound_then_len_and_sum() {
    if !backend_available() {
        return;
    }
    // Bind the owned array, then read it twice (borrow, not move): `.len()` (== 3) gates
    // `.sum()` (== 12). The leading `map(id)` pins the element type to i32 (a `where`-first
    // inline literal would otherwise default to i64 — a separate inference limitation).
    // where >2 over [1..5] keeps [3,4,5]; len 3, sum 12.
    let src = "fn id(x: i32) -> i32 = x\nfn big(x: i32) -> bool = x > 2\nfn main() -> i32 {\n  arena {\n    ys := [1, 2, 3, 4, 5].map(id).where(big).to_array()\n    if ys.len() == 3 {\n      return ys.sum()\n    }\n    return 0\n  }\n}\n";
    let out = build_and_run("to-array-len", src);
    assert_eq!(out.status.code(), Some(12));
}

#[test]
fn free_standing_to_array_sum() {
    if !backend_available() {
        return;
    }
    // No arena: `.to_array()` heap-allocates a free-standing owned array (dropped at exit).
    // [1,2,3].map(*2) = [2,4,6]; sum = 12.
    let src = "fn double(x: i32) -> i32 = x * 2\nfn main() -> i32 {\n  return [1, 2, 3].map(double).to_array().sum()\n}\n";
    let out = build_and_run("free-to-array", src);
    assert_eq!(out.status.code(), Some(12));
}

#[test]
fn return_owned_array_across_functions() {
    if !backend_available() {
        return;
    }
    // `make` returns a free-standing owned array (ownership moves to the caller, so `make`
    // does not drop it); `main` binds it and drops it at exit after summing. sum = 12.
    let src = "fn double(x: i32) -> i32 = x * 2\nfn make() -> array<i32> = [1, 2, 3].map(double).to_array()\nfn main() -> i32 {\n  ys := make()\n  return ys.sum()\n}\n";
    let out = build_and_run("return-owned", src);
    assert_eq!(out.status.code(), Some(12));
}

#[test]
fn return_owned_array_via_trailing_block_expr() {
    if !backend_available() {
        return;
    }
    // `make` binds the owned array to a local and returns it as the trailing block expression
    // (not the `= expr` form). The local is moved out to the caller, so `make` must NOT drop it
    // (else double-free / use-after-free); `main` owns and drops it. sum = 12.
    let src = "fn double(x: i32) -> i32 = x * 2\nfn make() -> array<i32> {\n  ys := [1, 2, 3].map(double).to_array()\n  ys\n}\nfn main() -> i32 {\n  zs := make()\n  return zs.sum()\n}\n";
    let out = build_and_run("return-trailing", src);
    assert_eq!(out.status.code(), Some(12));
}

#[test]
fn conditional_move_is_freed_on_both_paths() {
    if !backend_available() {
        return;
    }
    // `ys` is moved into `zs` only on the `c` branch. null-on-move means: on the moved path
    // `ys`'s slot is nulled (its exit Drop is a no-op) and `zs` is freed once; on the not-moved
    // path `ys` is still freed at exit. Neither path double-frees nor leaks. With `c = true`
    // the sum flows through `zs` (== 12); the program must run cleanly to exit.
    let src = "fn double(x: i32) -> i32 = x * 2\nfn run(c: bool) -> i32 {\n  ys := [1, 2, 3].map(double).to_array()\n  mut total := 0\n  if c {\n    zs := ys\n    total = zs.sum()\n  }\n  return total\n}\nfn main() -> i32 {\n  return run(true)\n}\n";
    let out = build_and_run("cond-move", src);
    assert_eq!(out.status.code(), Some(12));
}

#[test]
fn call_returned_owned_array_consumed_in_place() {
    if !backend_available() {
        return;
    }
    // `make()` returns a fresh owned array; consuming it in place (`make().sum()`) without
    // binding it must free the returned buffer after the fold (no leak, no double-free).
    // [1,2,3].map(*2) = [2,4,6]; sum = 12.
    let src = "fn double(x: i32) -> i32 = x * 2\nfn make() -> array<i32> = [1, 2, 3].map(double).to_array()\nfn main() -> i32 {\n  return make().sum()\n}\n";
    let out = build_and_run("call-temp-sum", src);
    assert_eq!(out.status.code(), Some(12));
}

#[test]
fn call_returned_owned_array_as_collect_source() {
    if !backend_available() {
        return;
    }
    // A fresh owned array (from `make()`) used as a `.to_array()` source: the collect loop
    // copies it into a new buffer and frees the source temporary at its exit. [2,4,6].map(+1)
    // = [3,5,7]; sum = 15.
    let src = "fn double(x: i32) -> i32 = x * 2\nfn inc(x: i32) -> i32 = x + 1\nfn make() -> array<i32> = [1, 2, 3].map(double).to_array()\nfn main() -> i32 {\n  return make().map(inc).to_array().sum()\n}\n";
    let out = build_and_run("call-temp-collect", src);
    assert_eq!(out.status.code(), Some(15));
}

#[test]
fn scan_prefix_sum_in_arena() {
    if !backend_available() {
        return;
    }
    // scan(0, add) over [1,2,3,4] → running sums [1,3,6,10]; sum of those = 20.
    let src = "fn add(acc: i32, x: i32) -> i32 = acc + x\nfn id(x: i32) -> i32 = x\nfn main() -> i32 {\n  arena {\n    prefix := [1, 2, 3, 4].map(id).scan(0, add)\n    return prefix.sum()\n  }\n}\n";
    let out = build_and_run("scan-arena", src);
    assert_eq!(out.status.code(), Some(20));
}

#[test]
fn scan_after_where_free_standing() {
    if !backend_available() {
        return;
    }
    // where >1 over [1,2,3,4] keeps [2,3,4]; scan(0, add) → [2,5,9]; max = 9. No arena, so the
    // scan buffer is free-standing and freed as an unbound temporary after `.max()` consumes it.
    let src = "fn add(acc: i32, x: i32) -> i32 = acc + x\nfn id(x: i32) -> i32 = x\nfn big(x: i32) -> bool = x > 1\nfn main() -> i32 {\n  return [1, 2, 3, 4].map(id).where(big).scan(0, add).max()\n}\n";
    let out = build_and_run("scan-where", src);
    assert_eq!(out.status.code(), Some(9));
}

#[test]
fn sort_orders_ascending_in_arena() {
    if !backend_available() {
        return;
    }
    // sort([3,1,2]) → [1,2,3]; an order-sensitive base-4 Horner fold (acc*4 + x) = 27 proves
    // the elements are actually ordered (min/max/sum would not). Within a byte exit range.
    let src = "fn id(x: i32) -> i32 = x\nfn horner(acc: i32, x: i32) -> i32 = acc * 4 + x\nfn main() -> i32 {\n  arena {\n    return [3, 1, 2].map(id).sort().reduce(0, horner)\n  }\n}\n";
    let out = build_and_run("sort-arena", src);
    assert_eq!(out.status.code(), Some(27));
}

#[test]
fn sort_reverse_input_free_standing() {
    if !backend_available() {
        return;
    }
    // Worst case for insertion sort: [4,3,2,1] → [1,2,3,4]; base-5 Horner = 194. No arena, so
    // the sorted buffer is a free-standing temporary freed after `reduce` consumes it.
    let src = "fn id(x: i32) -> i32 = x\nfn h(acc: i32, x: i32) -> i32 = acc * 5 + x\nfn main() -> i32 {\n  return [4, 3, 2, 1].map(id).sort().reduce(0, h)\n}\n";
    let out = build_and_run("sort-rev", src);
    assert_eq!(out.status.code(), Some(194));
}

#[test]
fn sort_after_where() {
    if !backend_available() {
        return;
    }
    // where >2 over [5,3,1,4] keeps [5,3,4]; sort → [3,4,5]; base-8 Horner = 229.
    let src = "fn id(x: i32) -> i32 = x\nfn big(x: i32) -> bool = x > 2\nfn h8(acc: i32, x: i32) -> i32 = acc * 8 + x\nfn main() -> i32 {\n  return [5, 3, 1, 4].map(id).where(big).sort().reduce(0, h8)\n}\n";
    let out = build_and_run("sort-where", src);
    assert_eq!(out.status.code(), Some(229));
}

#[test]
fn to_array_map_only_keeps_all() {
    if !backend_available() {
        return;
    }
    // map-only (no filter): every element survives, so length == source length.
    // [1,2,3].map(+10) = [11,12,13]; sum = 36.
    let src = "fn inc(x: i32) -> i32 = x + 10\nfn main() -> i32 {\n  arena {\n    return [1, 2, 3].map(inc).to_array().sum()\n  }\n}\n";
    let out = build_and_run("to-array-map", src);
    assert_eq!(out.status.code(), Some(36));
}
