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
