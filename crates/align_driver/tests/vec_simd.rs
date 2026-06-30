//! M6 SIMD slice 1 — the explicit fixed-width vector type `vecN<T>` (`vec2`/`vec4`/`vec8`/`vec16`
//! of a numeric scalar). Constructed from an array literal under a `vecN<T>` annotation; supports
//! elementwise `+`/`-`/`*`/`/` (lowered to LLVM vector arithmetic) and constant-lane read `v[i]`
//! (extractelement). `mask`/comparisons/`select`/`dot`/broadcast are later slices.

mod common;
use common::*;

#[test]
fn int_vector_add_mul_and_lane() {
    if !backend_available() {
        return;
    }
    // c = a + b = [11,22,33,44]; d = c * b = [110,440,990,1760]; d[2] = 990; 990 % 256 = 222.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 2, 3, 4]\n",
        "  b: vec4<i32> := [10, 20, 30, 40]\n",
        "  c := a + b\n",
        "  d := c * b\n",
        "  return d[2]\n",
        "}\n",
    );
    let out = build_and_run("vec-int", src);
    assert_eq!(out.status.code(), Some(222));
}

#[test]
fn float_vector_arithmetic_and_lane() {
    if !backend_available() {
        return;
    }
    // a = [1.5, 2.5]; (a + a) = [3.0, 5.0]; lane 1 = 5.0; as i32 = 5.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec2<f32> := [1.5, 2.5]\n",
        "  s := a + a\n",
        "  return s[1] as i32\n",
        "}\n",
    );
    let out = build_and_run("vec-float", src);
    assert_eq!(out.status.code(), Some(5));
}

#[test]
fn int_vector_division_and_wider_width() {
    if !backend_available() {
        return;
    }
    // vec8<i32>: q = a / b lane-wise; a[5]=60, b[5]=6 → q[5]=10.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec8<i32> := [10, 20, 30, 40, 50, 60, 70, 80]\n",
        "  b: vec8<i32> := [1, 2, 3, 4, 5, 6, 7, 8]\n",
        "  q := a / b\n",
        "  return q[5]\n",
        "}\n",
    );
    let out = build_and_run("vec-div", src);
    assert_eq!(out.status.code(), Some(10));
}

// The M6 completion condition — the generated IR really uses vector types (`<4 x i32>`, `add <4 x
// i32>`, insertelement/extractelement) — is verified out-of-band via `alignc emit-llvm` (as the
// optimizer suite notes for auto-vectorization). The per-lane run tests above prove the vector
// arithmetic is correct lane-by-lane (each lane holds a distinct value, and a specific lane is read).

#[test]
fn wrong_length_literal_is_rejected() {
    // A `vec4` annotation needs exactly 4 elements.
    let src = "fn main() -> i32 {\n  a: vec4<i32> := [1, 2, 3]\n  return a[0]\n}\n";
    assert!(check_errs("vec-badlen", src));
}

#[test]
fn non_constant_lane_is_rejected() {
    // A lane index must be a constant literal.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 2, 3, 4]\n",
        "  mut i := 0\n",
        "  i = 2\n",
        "  return a[i]\n",
        "}\n",
    );
    assert!(check_errs("vec-dynlane", src));
}

#[test]
fn out_of_range_lane_is_rejected() {
    let src = "fn main() -> i32 {\n  a: vec4<i32> := [1, 2, 3, 4]\n  return a[4]\n}\n";
    assert!(check_errs("vec-oob", src));
}

#[test]
fn non_numeric_element_is_rejected() {
    let src = "fn main() -> i32 {\n  a: vec4<bool> := [true, false, true, false]\n  return 0\n}\n";
    assert!(check_errs("vec-bool", src));
}

#[test]
fn invalid_width_is_rejected() {
    // Only 2/4/8/16 are valid widths; `vec3` is an unknown type.
    let src = "fn main() -> i32 {\n  a: vec3<i32> := [1, 2, 3]\n  return 0\n}\n";
    assert!(check_errs("vec-width3", src));
}

#[test]
fn a_mask_can_be_annotated_and_threaded_through_a_function() {
    if !backend_available() {
        return;
    }
    // A written `maskN<T>` type: a `let` annotation and a function parameter. The mask threads
    // through `blend`. select(a > b, a, b) = elementwise max [4, 5, 6, 8]; sum = 23.
    let src = concat!(
        "fn blend(m: mask4<i32>, a: vec4<i32>, b: vec4<i32>) -> vec4<i32> = select(m, a, b)\n",
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 5, 3, 8]\n",
        "  b: vec4<i32> := [4, 2, 6, 7]\n",
        "  m: mask4<i32> := a > b\n",
        "  hi := blend(m, a, b)\n",
        "  return hi[0] + hi[1] + hi[2] + hi[3]\n",
        "}\n",
    );
    let out = build_and_run("vec-mask-annot", src);
    assert_eq!(out.status.code(), Some(23));
}

#[test]
fn a_mask_with_a_mismatched_element_or_width_is_rejected() {
    // The annotation element must match the compared vectors' element.
    let elem = "fn main() -> i32 {\n  a: vec4<i32> := [1, 2, 3, 4]\n  m: mask4<f32> := a > a\n  return 0\n}\n";
    assert!(check_errs("vec-mask-elem", elem));
    // Only 2/4/8/16 are valid mask widths.
    let width = "fn main() -> i32 {\n  a: vec4<i32> := [1, 2, 3, 4]\n  m: mask3<i32> := a > a\n  return 0\n}\n";
    assert!(check_errs("vec-mask-width", width));
}

#[test]
fn comparison_and_select_compute_elementwise_max() {
    if !backend_available() {
        return;
    }
    // m = a > b = [F, T, F, T]; select(m, a, b) = elementwise max = [4, 5, 6, 8]; sum = 23.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 5, 3, 8]\n",
        "  b: vec4<i32> := [4, 2, 6, 7]\n",
        "  m := a > b\n",
        "  c := select(m, a, b)\n",
        "  return c[0] + c[1] + c[2] + c[3]\n",
        "}\n",
    );
    let out = build_and_run("vec-mask-max", src);
    assert_eq!(out.status.code(), Some(23));
}

#[test]
fn float_comparison_select_is_elementwise() {
    if !backend_available() {
        return;
    }
    // m = a < b = [T, F]; select(m, a, b) picks the smaller lane → [1.0, 2.0]; lane 0 = 1.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec2<f32> := [1.0, 9.0]\n",
        "  b: vec2<f32> := [4.0, 2.0]\n",
        "  m := a < b\n",
        "  c := select(m, a, b)\n",
        "  return c[1] as i32\n",
        "}\n",
    );
    // lane 1: a[1]=9 < b[1]=2 is false → b[1]=2.
    let out = build_and_run("vec-mask-fmin", src);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn lane_assignment_writes_one_lane() {
    if !backend_available() {
        return;
    }
    // `v[i] = x` writes a single lane: v = [10, 99, 30, 1]; sum = 140.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  mut v: vec4<i32> := [10, 20, 30, 40]\n",
        "  v[1] = 99\n",
        "  v[3] = 1\n",
        "  return v[0] + v[1] + v[2] + v[3]\n",
        "}\n",
    );
    let out = build_and_run("vec-laneset", src);
    assert_eq!(out.status.code(), Some(140));
}

#[test]
fn lane_assignment_to_an_immutable_vector_is_rejected() {
    let src = "fn main() -> i32 {\n  v: vec4<i32> := [1, 2, 3, 4]\n  v[0] = 9\n  return v[0]\n}\n";
    assert!(check_errs("vec-laneset-immut", src));
}

#[test]
fn lane_assignment_with_a_non_constant_or_oob_lane_is_rejected() {
    let dyn_lane = "fn main() -> i32 {\n  mut v: vec4<i32> := [1, 2, 3, 4]\n  mut i := 0\n  v[i] = 9\n  return v[0]\n}\n";
    assert!(check_errs("vec-laneset-dyn", dyn_lane));
    let oob = "fn main() -> i32 {\n  mut v: vec4<i32> := [1, 2, 3, 4]\n  v[4] = 9\n  return v[0]\n}\n";
    assert!(check_errs("vec-laneset-oob", oob));
}

#[test]
fn vector_load_and_store_roundtrip() {
    if !backend_available() {
        return;
    }
    // Load 4 lanes from `src`, double them, store back into the `out` slice. xs = [10,20,30,40];
    // doubled = [20,40,60,80]; ys[0] + ys[3] = 20 + 80 = 100.
    let src = concat!(
        "fn scale(src: slice<i64>, out dst: slice<i64>) {\n",
        "  v: vec4<i64> := src.load(0)\n",
        "  dst.store(0, v * 2)\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  xs := [10, 20, 30, 40]\n",
        "  mut ys := [0, 0, 0, 0]\n",
        "  scale(xs, ys)\n",
        "  return (ys[0] + ys[3]) as i32\n",
        "}\n",
    );
    let out = build_and_run("vec-loadstore", src);
    assert_eq!(out.status.code(), Some(100));
}

#[test]
fn vector_load_at_an_offset() {
    if !backend_available() {
        return;
    }
    // Load starting at index 2: [30, 40, 50, 60].sum() = 180.
    let src = concat!(
        "fn tail(src: slice<i64>) -> i64 {\n",
        "  v: vec4<i64> := src.load(2)\n",
        "  return v.sum()\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  xs := [10, 20, 30, 40, 50, 60]\n",
        "  return tail(xs) as i32\n",
        "}\n",
    );
    let out = build_and_run("vec-loadoff", src);
    assert_eq!(out.status.code(), Some(180));
}

#[test]
fn load_without_a_vector_annotation_is_rejected() {
    let src = "fn f(s: slice<i64>) -> i64 {\n  v := s.load(0)\n  return v.sum()\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("vec-load-noann", src));
}

#[test]
fn store_into_an_immutable_slice_is_rejected() {
    // A plain (non-`out`, non-`mut`) slice cannot be stored into.
    let src = concat!(
        "fn f(s: slice<i64>, v: vec4<i64>) {\n",
        "  s.store(0, v)\n",
        "}\n",
        "fn main() -> i32 = 0\n",
    );
    assert!(check_errs("vec-store-immut", src));
}

#[test]
fn load_store_element_type_mismatch_is_rejected() {
    // The slice element type must match the vector element type.
    let src = "fn f(s: slice<i32>) -> i64 {\n  v: vec4<i64> := s.load(0)\n  return v.sum()\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("vec-load-elem", src));
}

#[test]
fn vector_sum_reduction() {
    if !backend_available() {
        return;
    }
    // v.sum() = 10+20+30+40 = 100; and a non-local float receiver: (a+b).sum() = (2.5+3.5) = 6.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  v: vec4<i32> := [10, 20, 30, 40]\n",
        "  a: vec2<f32> := [1.5, 2.5]\n",
        "  b: vec2<f32> := [1.0, 1.0]\n",
        "  return v.sum() + ((a + b).sum() as i32)\n",
        "}\n",
    );
    let out = build_and_run("vec-sum", src);
    assert_eq!(out.status.code(), Some(106));
}

#[test]
fn an_invalid_reduction_receiver_reports_one_error() {
    // The speculative vec-vs-array receiver check rolls back its diagnostics when the receiver is
    // not a vector, so an undefined receiver yields exactly one error (not a duplicate from the
    // array path re-checking it).
    for m in ["sum", "min", "max"] {
        let src = format!("fn main() -> i32 {{\n  return undefinedthing.{m}() as i32\n}}\n");
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, "vec-dup", &src);
        let n = align_driver::format_diagnostics(&sm, &checked.diags).matches("error:").count();
        assert_eq!(n, 1, "`.{m}()` on an undefined receiver should report exactly one error, got {n}");
    }
}

#[test]
fn array_pipeline_sum_still_works() {
    if !backend_available() {
        return;
    }
    // The fused array pipeline `xs.map(f).sum()` (a separate path) is unaffected: 2*(1+2+3+4) = 20.
    let src = concat!(
        "fn dbl(x: i64) -> i64 = x * 2\n",
        "fn main() -> i32 {\n",
        "  a := [1, 2, 3, 4]\n",
        "  return a.map(dbl).sum() as i32\n",
        "}\n",
    );
    let out = build_and_run("arr-map-sum", src);
    assert_eq!(out.status.code(), Some(20));
}

#[test]
fn vector_min_and_max_reductions() {
    if !backend_available() {
        return;
    }
    // a.max() = 40, a.min() = 10, difference = 30.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [30, 10, 40, 20]\n",
        "  return a.max() - a.min()\n",
        "}\n",
    );
    let out = build_and_run("vec-minmax", src);
    assert_eq!(out.status.code(), Some(30));
}

#[test]
fn float_and_unsigned_min_max() {
    if !backend_available() {
        return;
    }
    // float max = 9.0 → 9; and an unsigned vector min = 7 → exercises the umin intrinsic. 9 + 7 = 16.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  f: vec4<f32> := [3.5, 1.5, 9.0, 2.0]\n",
        "  u: vec4<u32> := [42, 7, 100, 13]\n",
        "  return (f.max() as i32) + (u.min() as i32)\n",
        "}\n",
    );
    let out = build_and_run("vec-minmax-fu", src);
    assert_eq!(out.status.code(), Some(16));
}

#[test]
fn min_max_on_a_non_local_vector_receiver() {
    if !backend_available() {
        return;
    }
    // The reduction works on any vector-valued receiver, not just a local: an arithmetic expression
    // and a function return. (a + b).max() = [41,32,23,14].max() = 41; mk().min() = 2; sum = 43.
    let src = concat!(
        "fn mk() -> vec4<i32> = [5, 8, 2, 9]\n",
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 2, 3, 4]\n",
        "  b: vec4<i32> := [40, 30, 20, 10]\n",
        "  return (a + b).max() + mk().min()\n",
        "}\n",
    );
    let out = build_and_run("vec-minmax-recv", src);
    assert_eq!(out.status.code(), Some(43));
}

#[test]
fn array_pipeline_min_still_routes_to_the_array_reduction() {
    if !backend_available() {
        return;
    }
    // A `.where(p)` pipeline receiver must still reach the array reduction (not be mis-checked as a
    // vector). Keep values > 2 → [5, 9]; min = 5.
    let src = concat!(
        "fn busy(x: i64) -> bool = x > 2\n",
        "fn main() -> i32 {\n",
        "  a := [1, 5, 2, 9]\n",
        "  return a.where(busy).min() as i32\n",
        "}\n",
    );
    let out = build_and_run("arr-where-min", src);
    assert_eq!(out.status.code(), Some(5));
}

#[test]
fn array_min_max_still_works() {
    if !backend_available() {
        return;
    }
    // The array reduction `arr.min()` (a separate path) is unaffected: min([5,2,8,1]) = 1.
    let src = "fn main() -> i32 {\n  a := [5, 2, 8, 1]\n  return a.min() as i32\n}\n";
    let out = build_and_run("arr-min", src);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn dot_is_the_vector_dot_product() {
    if !backend_available() {
        return;
    }
    // dot([1,2,3,4], [10,20,30,40]) = 10+40+90+160 = 300; the process exit code is 300 % 256 = 44.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 2, 3, 4]\n",
        "  b: vec4<i32> := [10, 20, 30, 40]\n",
        "  return dot(a, b)\n",
        "}\n",
    );
    let out = build_and_run("vec-dot", src);
    assert_eq!(out.status.code(), Some(44));
}

#[test]
fn float_dot_product() {
    if !backend_available() {
        return;
    }
    // dot([1.5, 2.0], [4.0, 3.0]) = 6.0 + 6.0 = 12.0; as i32 = 12.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec2<f32> := [1.5, 2.0]\n",
        "  b: vec2<f32> := [4.0, 3.0]\n",
        "  return dot(a, b) as i32\n",
        "}\n",
    );
    let out = build_and_run("vec-dotf", src);
    assert_eq!(out.status.code(), Some(12));
}

#[test]
fn dot_on_mismatched_or_non_vectors_is_rejected() {
    // Different widths.
    let m = "fn main() -> i32 {\n  a: vec4<i32> := [1, 2, 3, 4]\n  b: vec2<i32> := [1, 2]\n  return dot(a, b)\n}\n";
    assert!(check_errs("vec-dot-width", m));
    // A non-vector operand.
    let s = "fn main() -> i32 {\n  a: vec4<i32> := [1, 2, 3, 4]\n  return dot(a, 5)\n}\n";
    assert!(check_errs("vec-dot-scalar", s));
}

#[test]
fn array_dot_still_works() {
    if !backend_available() {
        return;
    }
    // The array-pipeline `xs.dot(ys)` (a separate method terminal) is unaffected by the vector
    // free-function `dot(a, b)`. dot([1,2,3],[4,5,6]) = 4+10+18 = 32 (i64 array elements → cast).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a := [1, 2, 3]\n",
        "  b := [4, 5, 6]\n",
        "  return a.dot(b) as i32\n",
        "}\n",
    );
    let out = build_and_run("arr-dot", src);
    assert_eq!(out.status.code(), Some(32));
}

#[test]
fn scalar_broadcasts_in_vector_arithmetic() {
    if !backend_available() {
        return;
    }
    // A scalar on the right of a vector op broadcasts across the lanes:
    // c = a + 5 = [15, 25, 35, 45]; d = c * 2 = [30, 50, 70, 90]; d[1] + d[3] = 50 + 90 = 140.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [10, 20, 30, 40]\n",
        "  c := a + 5\n",
        "  d := c * 2\n",
        "  return d[1] + d[3]\n",
        "}\n",
    );
    let out = build_and_run("vec-bcast", src);
    assert_eq!(out.status.code(), Some(140));
}

#[test]
fn sum_where_is_a_masked_horizontal_sum() {
    if !backend_available() {
        return;
    }
    // The draft §9 example: sum the lanes above 80. scores = [70,90,60,85]; >80 → 90 + 85 = 175.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  scores: vec4<i32> := [70, 90, 60, 85]\n",
        "  m := scores > 80\n",
        "  return scores.sum_where(m)\n",
        "}\n",
    );
    let out = build_and_run("vec-sumwhere", src);
    assert_eq!(out.status.code(), Some(175));
}

#[test]
fn broadcasting_a_mismatched_scalar_type_is_rejected() {
    // A float scalar cannot broadcast into an int vector.
    let src = "fn main() -> i32 {\n  a: vec4<i32> := [1, 2, 3, 4]\n  c := a + 2.0\n  return c[0]\n}\n";
    assert!(check_errs("vec-bcast-mismatch", src));
}

#[test]
fn scalar_on_the_left_of_a_vector_broadcasts() {
    if !backend_available() {
        return;
    }
    // A scalar on the LEFT broadcasts too, with operand order preserved for non-commutative ops:
    // s = 10 + a = [11,12,13,14]; d = 20 - a = [19,18,17,16]; m = 2 < a = [F,F,T,T];
    // select(m, s, d) = [19, 18, 13, 14]; sum = 64.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 2, 3, 4]\n",
        "  s := 10 + a\n",
        "  d := 20 - a\n",
        "  m := 2 < a\n",
        "  picked := select(m, s, d)\n",
        "  return picked[0] + picked[1] + picked[2] + picked[3]\n",
        "}\n",
    );
    let out = build_and_run("vec-scalar-left", src);
    assert_eq!(out.status.code(), Some(64));
}

#[test]
fn a_mismatched_scalar_type_broadcasting_into_a_vector_is_rejected() {
    // A float scalar cannot broadcast into an int vector — in either operand order.
    let left = "fn main() -> i32 {\n  a: vec4<i32> := [1, 2, 3, 4]\n  c := 2.0 + a\n  return c[0]\n}\n";
    assert!(check_errs("vec-bcast-left-mismatch", left));
    let right = "fn main() -> i32 {\n  a: vec4<i32> := [1, 2, 3, 4]\n  c := a + 2.0\n  return c[0]\n}\n";
    assert!(check_errs("vec-bcast-right-mismatch", right));
}

#[test]
fn select_with_a_non_mask_first_arg_is_rejected() {
    // `select`'s first argument must be a mask (a vector comparison result), not a vector.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 2, 3, 4]\n",
        "  b: vec4<i32> := [4, 3, 2, 1]\n",
        "  c := select(a, a, b)\n",
        "  return c[0]\n",
        "}\n",
    );
    assert!(check_errs("vec-sel-nomask", src));
}

#[test]
fn select_width_mismatch_is_rejected() {
    // The mask width must match the vectors' width.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 2, 3, 4]\n",
        "  b: vec4<i32> := [4, 3, 2, 1]\n",
        "  p: vec2<i32> := [1, 2]\n",
        "  q: vec2<i32> := [2, 1]\n",
        "  m := p > q\n",
        "  c := select(m, a, b)\n",
        "  return c[0]\n",
        "}\n",
    );
    assert!(check_errs("vec-sel-width", src));
}

#[test]
fn remainder_on_vectors_is_rejected() {
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 2, 3, 4]\n",
        "  b: vec4<i32> := [1, 2, 3, 4]\n",
        "  c := a % b\n",
        "  return c[0]\n",
        "}\n",
    );
    assert!(check_errs("vec-rem", src));
}

#[test]
fn element_wise_float_vector_math() {
    if !backend_available() {
        return;
    }
    // The unary float math ops apply lane-wise to a float vector, via the LLVM vector intrinsic.
    // sqrt([1,4,9,16]) = [1,2,3,4] → sum 10.
    let sqrt = concat!(
        "fn main() -> i32 {\n",
        "  v: vec4<f32> := [1.0, 4.0, 9.0, 16.0]\n",
        "  w := v.sqrt()\n",
        "  return (w[0] + w[1] + w[2] + w[3]) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("vec-sqrt", sqrt).status.code(), Some(10));
    // abs([-3,5,-2,4]) = [3,5,2,4] → sum 14.
    let abs = concat!(
        "fn main() -> i32 {\n",
        "  v: vec4<f32> := [0.0 - 3.0, 5.0, 0.0 - 2.0, 4.0]\n",
        "  w := v.abs()\n",
        "  return (w[0] + w[1] + w[2] + w[3]) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("vec-abs", abs).status.code(), Some(14));
    // floor([1.7,2.2,3.9,4.5]) = [1,2,3,4] → sum 10.
    let floor = concat!(
        "fn main() -> i32 {\n",
        "  v: vec4<f64> := [1.7, 2.2, 3.9, 4.5]\n",
        "  w := v.floor()\n",
        "  return (w[0] + w[1] + w[2] + w[3]) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("vec-floor", floor).status.code(), Some(10));
}

#[test]
fn vector_math_lowers_to_the_vector_intrinsic() {
    if !backend_available() {
        return;
    }
    // The element-wise op must emit the *vector* intrinsic (`llvm.sqrt.v4f32`), not a per-lane loop.
    let ir = emit_llvm(concat!(
        "fn main() -> i32 {\n",
        "  v: vec4<f32> := [1.0, 4.0, 9.0, 16.0]\n",
        "  w := v.sqrt()\n",
        "  return w[0] as i32\n",
        "}\n",
    ));
    assert!(ir.contains("llvm.sqrt.v4f32"), "expected vector sqrt intrinsic, got:\n{ir}");
}

#[test]
fn integer_vector_math_is_rejected() {
    // The unary float ops are float-only; an integer vector has no `sqrt`.
    assert!(check_errs(
        "vec-int-sqrt",
        "fn main() -> i32 {\n  v: vec4<i32> := [1, 4, 9, 16]\n  w := v.sqrt()\n  return w[0]\n}\n",
    ));
}

#[test]
fn element_wise_binary_vector_min_max() {
    if !backend_available() {
        return;
    }
    // `a.min(b)` / `a.max(b)` with a vector argument are element-wise (one SIMD instruction each),
    // distinct from the no-arg reduction `v.min()`. Works on float and integer vectors.
    let fmin = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<f32> := [1.0, 5.0, 3.0, 8.0]\n",
        "  b: vec4<f32> := [4.0, 2.0, 6.0, 7.0]\n",
        "  lo := a.min(b)\n",
        "  return (lo[0] + lo[1] + lo[2] + lo[3]) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("vec-bin-min", fmin).status.code(), Some(13)); // [1,2,3,7]
    let imax = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 5, 3, 8]\n",
        "  b: vec4<i32> := [4, 2, 6, 7]\n",
        "  hi := a.max(b)\n",
        "  return hi[0] + hi[1] + hi[2] + hi[3]\n",
        "}\n",
    );
    assert_eq!(build_and_run("vec-bin-max", imax).status.code(), Some(23)); // [4,5,6,8]
}

#[test]
fn integer_vector_abs() {
    if !backend_available() {
        return;
    }
    // abs vectorizes for integer vectors too (one `pabsd`-style instruction). |[-3,5,-2,4]| → 14.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  v: vec4<i32> := [0 - 3, 5, 0 - 2, 4]\n",
        "  w := v.abs()\n",
        "  return w[0] + w[1] + w[2] + w[3]\n",
        "}\n",
    );
    assert_eq!(build_and_run("vec-int-abs", src).status.code(), Some(14));
}

#[test]
fn binary_vector_min_max_lowers_to_the_vector_intrinsic() {
    if !backend_available() {
        return;
    }
    let ir = emit_llvm(concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<i32> := [1, 5, 3, 8]\n",
        "  b: vec4<i32> := [4, 2, 6, 7]\n",
        "  hi := a.max(b)\n",
        "  return hi[0] as i32\n",
        "}\n",
    ));
    assert!(ir.contains("llvm.smax.v4i32"), "expected vector smax intrinsic, got:\n{ir}");
}

#[test]
fn pow_on_a_vector_is_rejected() {
    // `pow` lowers to a libcall, not a lane-wise instruction, so it stays scalar-only.
    assert!(check_errs(
        "vec-pow",
        "fn main() -> i32 {\n  a: vec4<f32> := [1.0, 2.0, 3.0, 4.0]\n  w := a.pow(a)\n  return w[0] as i32\n}\n",
    ));
}

#[test]
fn fused_multiply_add_scalar_and_vector() {
    if !backend_available() {
        return;
    }
    // `fma(a, b, c)` = a*b + c with one rounding, a free builtin (like `dot`/`select`).
    // Scalar: 2*3 + 1 = 7.
    let scalar = "fn main() -> i32 {\n  return fma(2.0, 3.0, 1.0) as i32\n}\n";
    assert_eq!(build_and_run("fma-scalar", scalar).status.code(), Some(7));
    // Vector: [1,2,3,4]*10 + 1 = [11,21,31,41] → 104.
    let vec = concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<f32> := [1.0, 2.0, 3.0, 4.0]\n",
        "  b: vec4<f32> := [10.0, 10.0, 10.0, 10.0]\n",
        "  c: vec4<f32> := [1.0, 1.0, 1.0, 1.0]\n",
        "  r := fma(a, b, c)\n",
        "  return (r[0] + r[1] + r[2] + r[3]) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("fma-vec", vec).status.code(), Some(104));
}

#[test]
fn fma_lowers_to_the_vector_intrinsic() {
    if !backend_available() {
        return;
    }
    let ir = emit_llvm(concat!(
        "fn main() -> i32 {\n",
        "  a: vec4<f32> := [1.0, 2.0, 3.0, 4.0]\n",
        "  b: vec4<f32> := [10.0, 10.0, 10.0, 10.0]\n",
        "  c: vec4<f32> := [1.0, 1.0, 1.0, 1.0]\n",
        "  r := fma(a, b, c)\n",
        "  return r[0] as i32\n",
        "}\n",
    ));
    assert!(ir.contains("llvm.fma.v4f32"), "expected vector fma intrinsic, got:\n{ir}");
}

#[test]
fn fma_is_float_only_and_takes_three_args() {
    // Integer operands have no fused multiply-add here (float-only).
    assert!(check_errs("fma-int", "fn main() -> i32 {\n  return fma(2, 3, 1)\n}\n"));
    // It needs exactly three operands.
    assert!(check_errs("fma-arity", "fn main() -> i32 {\n  return fma(2.0, 3.0) as i32\n}\n"));
}
