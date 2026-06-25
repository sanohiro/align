//! Inline lambdas (`fn x { … }`) as pipeline-stage arguments (`draft.md` §11). Slice ①: a
//! non-capturing lambda in `map` / `where` is lifted to a synthetic top-level function, so it
//! flows through the existing fused-loop lowering — optimized identically to a named function.


mod common;
use common::*;

#[test]
fn map_lambda() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3].map(fn x { x * 2 }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-map", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
}

#[test]
fn where_lambda() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3, 4, 5].where(fn x { x > 2 }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-where", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
}

#[test]
fn map_and_where_lambdas_fuse() {
    if !backend_available() {
        return;
    }
    // map *10, keep >25 → 30+40+50 = 120. Both lambdas lift and fuse into one loop.
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3, 4, 5].map(fn x { x * 10 }).where(fn x { x > 25 }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-fuse", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "120\n");
}

#[test]
fn lambda_lifts_to_a_called_function() {
    // The lambda becomes a synthetic top-level function the fused loop calls (so LLVM inlines it
    // exactly like a named stage function).
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3].map(fn x { x * 2 }).sum())\n  return Ok(())\n}\n";
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("$lambda"), "lambda should be lifted to a synthetic function:\n{text}");
    assert!(text.contains("call main$lambda0"), "the fused loop should call the lifted lambda:\n{text}");
}

#[test]
fn lambda_can_call_a_named_function() {
    if !backend_available() {
        return;
    }
    // A lambda body may reference top-level functions (resolved via signatures), just not
    // enclosing locals (capture is a follow-up slice).
    let src = "fn inc(x: i64) -> i64 = x + 1\nfn main() -> Result<(), Error> {\n  print([1, 2, 3].map(fn x { inc(x) * 2 }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-callnamed", src);
    assert_eq!(out.status.code(), Some(0));
    // (2+4+6)*... → (1+1)*2 + (2+1)*2 + (3+1)*2 = 4+6+8 = 18
    assert_eq!(String::from_utf8_lossy(&out.stdout), "18\n");
}

#[test]
fn reduce_lambda() {
    if !backend_available() {
        return;
    }
    // `reduce(init, f)` with a two-parameter lambda: 1+2+3+4 = 10.
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3, 4].reduce(0, fn acc, x { acc + x }))\n  return Ok(())\n}\n";
    let out = build_and_run("lam-reduce", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n");
}

#[test]
fn par_map_lambda_pure() {
    if !backend_available() {
        return;
    }
    // A Pure lambda runs in parallel: (1+100)+(2+100)+(3+100) = 306.
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3].par_map(fn x { x + 100 }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-parmap", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "306\n");
}

#[test]
fn par_map_impure_lambda_rejected() {
    // The Pure requirement applies to a lambda too (a lifted impure lambda is rejected).
    let src = "fn show(x: i64) -> i64 {\n  print(x)\n  return x\n}\nfn main() -> Result<(), Error> {\n  ys := [1, 2].par_map(fn x { show(x) })\n  print(ys.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("lam-parmap-impure", src));
}

#[test]
fn any_all_lambda() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  if [2, 4, 6].all(fn x { x % 2 == 0 }) { print(1) }\n  if [1, 2, 3].any(fn x { x > 2 }) { print(2) }\n  return Ok(())\n}\n";
    let out = build_and_run("lam-anyall", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n");
}

#[test]
fn scan_lambda() {
    if !backend_available() {
        return;
    }
    // Prefix sums [1,3,6,10]; last = 10.
    let src = "fn main() -> Result<(), Error> {\n  arena {\n    ps := [1, 2, 3, 4].scan(0, fn acc, x { acc + x })\n    print(ps[3])\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("lam-scan", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n");
}

#[test]
fn partition_lambda() {
    if !backend_available() {
        return;
    }
    // Evens [2,4] sum 6, odds [1,3,5] sum 9.
    let src = "fn main() -> Result<(), Error> {\n  (ev, od) := [1, 2, 3, 4, 5].partition(fn x { x % 2 == 0 })\n  print(ev.sum())\n  print(od.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-partition", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n9\n");
}

#[test]
fn bare_lambda_rejected() {
    // A lambda value outside a stage argument is not yet a first-class value.
    let src = "fn main() -> Result<(), Error> {\n  f := fn x { x * 2 }\n  return Ok(())\n}\n";
    assert!(check_errs("lam-bare", src));
}

#[test]
fn lambda_captures_local_in_map() {
    if !backend_available() {
        return;
    }
    // A lambda captures an enclosing local by value (passed as a synthetic parameter): (1+2+3)*3 = 18.
    let src = "fn main() -> Result<(), Error> {\n  factor := 3\n  print([1, 2, 3].map(fn x { x * factor }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-capture-map", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "18\n");
}

#[test]
fn lambda_captures_in_where() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  threshold := 2\n  print([1, 2, 3, 4].where(fn x { x > threshold }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-capture-where", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn lambda_captures_function_parameter() {
    if !backend_available() {
        return;
    }
    // Capturing a function parameter — what named functions fundamentally cannot do.
    let src = "fn scale(xs: slice<i64>, k: i64) -> i64 = xs.map(fn x { x * k }).sum()\nfn main() -> Result<(), Error> {\n  a := [1, 2, 3, 4]\n  print(scale(a, 10))\n  return Ok(())\n}\n";
    let out = build_and_run("lam-capture-param", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "100\n");
}

#[test]
fn lambda_captures_multiple() {
    if !backend_available() {
        return;
    }
    // Two captures: (1*2+5)+(2*2+5)+(3*2+5) = 7+9+11 = 27.
    let src = "fn main() -> Result<(), Error> {\n  a := 2\n  b := 5\n  print([1, 2, 3].map(fn x { x * a + b }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-capture-multi", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "27\n");
}

#[test]
fn lambda_captures_lower_to_extra_call_args() {
    // The capture becomes a trailing parameter of the lifted function, passed at the call.
    let src = "fn main() -> Result<(), Error> {\n  factor := 3\n  print([1, 2, 3].map(fn x { x * factor }).sum())\n  return Ok(())\n}\n";
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("fn main$lambda0(_0: i64, _1: i64)"), "lambda should take the element + the capture:\n{text}");
}

#[test]
fn lambda_captures_in_reduce() {
    if !backend_available() {
        return;
    }
    // Capture in a two-parameter fold: 1*2 + 2*2 + 3*2 = 12.
    let src = "fn main() -> Result<(), Error> {\n  k := 2\n  print([1, 2, 3].reduce(0, fn acc, x { acc + x * k }))\n  return Ok(())\n}\n";
    let out = build_and_run("lam-capture-reduce", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
}

#[test]
fn lambda_captures_in_partition_and_any_all() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  t := 2\n  (big, small) := [1, 2, 3, 4].partition(fn x { x > t })\n  print(big.sum())\n  print(small.sum())\n  if [3, 4, 5].all(fn x { x > t }) { print(1) }\n  return Ok(())\n}\n";
    let out = build_and_run("lam-capture-part", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n3\n1\n");
}

#[test]
fn lambda_captures_in_par_map_falls_back_to_sequential() {
    if !backend_available() {
        return;
    }
    // A capturing par_map lambda is correct via the sequential path (the parallel thunk takes no
    // capture context): (1+100)+(2+100)+(3+100) = 306.
    let src = "fn main() -> Result<(), Error> {\n  b := 100\n  print([1, 2, 3].par_map(fn x { x + b }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-capture-parmap", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "306\n");
    // With a capture, par_map does NOT take the parallel runtime path.
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(!text.contains("par_map["), "a capturing par_map must fall back to sequential:\n{text}");
}

#[test]
fn lambda_capture_does_not_false_positive_move_or_escape() {
    if !backend_available() {
        return;
    }
    // The flow analyses now walk stage captures; a valid copy-value capture (used after the
    // pipeline, and a fixed-array capture) must not be wrongly flagged as moved/escaping.
    let src = "fn main() -> Result<(), Error> {\n  factor := 4\n  a := [10, 20, 30]\n  s := [1, 2].map(fn x { x * factor + a[0] }).sum()\n  print(s)\n  print(factor)\n  return Ok(())\n}\n";
    let out = build_and_run("lam-capture-noflag", src);
    assert_eq!(out.status.code(), Some(0));
    // (1*4+10) + (2*4+10) = 14 + 18 = 32, then factor=4.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "32\n4\n");
}

#[test]
fn lambda_capture_owned_value_rejected_for_now() {
    // Slice ③ captures copy values; capturing an owned (Move) value is deferred.
    let src = "fn main() -> Result<(), Error> {\n  ys := [10, 20].to_array()\n  print([1, 2, 3].map(fn x { x + ys.sum() }).sum())\n  return Ok(())\n}\n";
    assert!(check_errs("lam-capture-owned", src));
}

#[test]
fn lambda_wrong_arity_rejected() {
    // A `map` lambda takes exactly one parameter.
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3].map(fn x, y { x + y }).sum())\n  return Ok(())\n}\n";
    assert!(check_errs("lam-arity", src));
}
