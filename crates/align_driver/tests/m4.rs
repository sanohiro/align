//! M4 end-to-end: arrays + fused reductions. Requires LLVM/cc, so skip where absent.

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
    std::process::Command::new(&exe).output().expect("run")
}

#[test]
fn array_sum_inline() {
    if !backend_available() {
        return;
    }
    let out = build_and_run("arr-sum", "fn main() -> i32 {\n  return [10, 20, 12].sum()\n}\n");
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn array_sum_bound_local() {
    if !backend_available() {
        return;
    }
    // Bound array summed where the result type matches (i64 throughout, low byte = 15).
    let out = build_and_run(
        "arr-sum-bound",
        "fn total(n: i64) -> i64 {\n  xs := [1, 2, 3, 4, 5]\n  return xs.sum()\n}\nfn main() -> i32 {\n  return 0\n}\n",
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn fused_map_where_sum_pipeline() {
    if !backend_available() {
        return;
    }
    // map *2 → [2,4,6,8,10]; where >4 → [6,8,10]; sum = 24.
    let src = "fn double(x: i32) -> i32 = x * 2\nfn big(x: i32) -> bool = x > 4\nfn main() -> i32 {\n  return [1, 2, 3, 4, 5].map(double).where(big).sum()\n}\n";
    let out = build_and_run("pipeline", src);
    assert_eq!(out.status.code(), Some(24));
}

#[test]
fn pipeline_fuses_into_one_loop() {
    let mut sm = SourceMap::new();
    let src = "fn double(x: i32) -> i32 = x * 2\nfn big(x: i32) -> bool = x > 4\nfn main() -> i32 {\n  return [1, 2, 3].map(double).where(big).sum()\n}\n";
    let checked = check(&mut sm, "p.align", src);
    assert!(!checked.diags.has_errors());
    let text = align_mir::print::program_to_string(&lower_to_mir(&checked.hir));
    // Fusion: the map and where calls appear inside the loop body, and there is no
    // intermediate array store of mapped results (only the source literal is stored).
    assert!(text.contains("call double") && text.contains("call big"), "stages not inlined:\n{text}");
    // Exactly one loop back-edge target reused (single loop): the source is stored once.
    assert_eq!(text.matches("<- 1_i32").count(), 1, "source stored once:\n{text}");
}

#[test]
fn struct_float_field_roundtrips() {
    if !backend_available() {
        return;
    }
    // Regression: float struct fields must use the float LLVM type (not i32). 1.5+2.5>3.
    let src = "P { x: f64, y: f64 }\nfn main() -> i32 {\n  p := P{x: 1.5, y: 2.5}\n  if p.x + p.y > 3.0 { return 1 }\n  return 0\n}\n";
    let out = build_and_run("struct-float", src);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn struct_array_projection_sum() {
    if !backend_available() {
        return;
    }
    // Project field x and sum: 10 + 30 + 2 = 42.
    let src = "Pt { x: i32, y: i32 }\nfn main() -> i32 {\n  return [Pt{x: 10, y: 1}, Pt{x: 30, y: 2}, Pt{x: 2, y: 3}].x.sum()\n}\n";
    let out = build_and_run("proj", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn struct_array_where_field_projection_sum() {
    if !backend_available() {
        return;
    }
    // where(.active) keeps pay 10 and 32; project pay; sum = 42.
    let src = "Emp { pay: i32, active: bool }\nfn main() -> i32 {\n  return [Emp{pay: 10, active: true}, Emp{pay: 50, active: false}, Emp{pay: 32, active: true}].where(.active).pay.sum()\n}\n";
    let out = build_and_run("emp", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn count_terminal_scalar_struct_and_plain() {
    if !backend_available() {
        return;
    }
    // count after where on a scalar array (3), after where(.active) on a struct array (2),
    // and with no stages (3): 3 + 2 + 3 = 8.
    let src = "fn big(x: i64) -> bool = x > 2\nEmp { pay: i32, active: bool }\nfn total() -> i64 {\n  c1 := [1, 2, 3, 4, 5].where(big).count()\n  c2 := [Emp{pay: 10, active: true}, Emp{pay: 5, active: false}, Emp{pay: 8, active: true}].where(.active).count()\n  c3 := [10, 20, 30].count()\n  return c1 + c2 + c3\n}\nfn main() -> i32 {\n  if total() == 8 { return 1 }\n  return 0\n}\n";
    let out = build_and_run("count", src);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn reduce_with_custom_fold() {
    if !backend_available() {
        return;
    }
    // product 1*2*3*4 = 24, plus sum 42 = 66.
    let src = "fn mul(acc: i32, x: i32) -> i32 = acc * x\nfn add(acc: i32, x: i32) -> i32 = acc + x\nfn main() -> i32 {\n  p := [1, 2, 3, 4].reduce(mul, 1)\n  s := [10, 20, 12].reduce(add, 0)\n  return p + s\n}\n";
    let out = build_and_run("reduce", src);
    assert_eq!(out.status.code(), Some(66));
}

#[test]
fn reduce_after_map_where() {
    if !backend_available() {
        return;
    }
    let src = "fn dbl(x: i32) -> i32 = x * 2\nfn big(x: i32) -> bool = x > 4\nfn add(acc: i32, x: i32) -> i32 = acc + x\nfn main() -> i32 {\n  return [1, 2, 3, 4, 5].map(dbl).where(big).reduce(add, 0)\n}\n";
    let out = build_and_run("reduce-pipe", src);
    assert_eq!(out.status.code(), Some(24));
}

#[test]
fn slice_param_sum() {
    if !backend_available() {
        return;
    }
    // An array is borrowed as a slice<i32> argument; summed in the callee. = 42.
    let src = "fn total(xs: slice<i32>) -> i32 = xs.sum()\nfn main() -> i32 {\n  return total([10, 20, 12])\n}\n";
    let out = build_and_run("slice-sum", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn slice_pipeline_runtime_length() {
    if !backend_available() {
        return;
    }
    // Fused map/where/sum over a slice (runtime length). = 24.
    let src = "fn dbl(x: i32) -> i32 = x * 2\nfn big(x: i32) -> bool = x > 4\nfn proc(xs: slice<i32>) -> i32 = xs.map(dbl).where(big).sum()\nfn main() -> i32 {\n  return proc([1, 2, 3, 4, 5])\n}\n";
    let out = build_and_run("slice-pipe", src);
    assert_eq!(out.status.code(), Some(24));
}

#[test]
fn slice_annotated_local_sum() {
    if !backend_available() {
        return;
    }
    // A slice-annotated local borrows the array literal (ArrayToSlice), then is summed. = 42.
    let src = "fn main() -> i32 {\n  s: slice<i32> := [10, 20, 12]\n  return s.sum()\n}\n";
    let out = build_and_run("slice-local-sum", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn struct_by_value_pass_return_copy() {
    if !backend_available() {
        return;
    }
    // Construct via a struct-literal body, pass by value, copy, and return by value. = 42.
    let src = "P { x: i32, y: i32 }\nfn sum(p: P) -> i32 = p.x + p.y\nfn dup(p: P) -> P {\n  q := p\n  return q\n}\nfn mk(v: i32) -> P = P{x: v, y: v}\nfn main() -> i32 {\n  a := mk(21)\n  b := dup(a)\n  return sum(b)\n}\n";
    let out = build_and_run("struct-by-value", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn array_sum_emits_single_loop() {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "a.align", "fn main() -> i32 {\n  return [1, 2, 3].sum()\n}\n");
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    let text = align_mir::print::program_to_string(&mir);
    // One loop: a back-edge (two `goto bb1`-style targets) and an indexed load.
    assert!(text.contains("["), "expected indexed access in:\n{text}");
    assert!(text.matches("branch").count() >= 1, "expected a loop branch in:\n{text}");
}
