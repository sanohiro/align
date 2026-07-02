//! The LLVM middle-end optimization pipeline (`-O2`) is run before object emission, so the lifted
//! lambdas and fused `map`/`where`/`reduce` loops are inlined and vectorized. These tests assert
//! the optimized output is still **correct** — a fused pipeline must compute the same result after
//! the inliner / LICM / vectorizer run on it (a miscompile from latent IR UB would surface here).
//! (Vectorization itself is target-dependent; it is verified out-of-band via `objdump`.)


mod common;
use common::*;

#[test]
fn fused_map_sum_correct_under_o2() {
    if !backend_available() {
        return;
    }
    // `xs.map(dbl).sum()` fuses to one loop that the optimizer inlines + vectorizes;
    // the result must still be 2*(1+..+8) = 72.
    let src = concat!(
        "fn dbl(x: i64) -> i64 = x * 2\n",
        "fn run(xs: slice<i64>) -> i64 = xs.map(dbl).sum()\n",
        "fn main() -> i32 {\n",
        "  a := [1, 2, 3, 4, 5, 6, 7, 8]\n",
        "  return run(a) as i32\n",
        "}\n",
    );
    let out = build_and_run("opt-map-sum", src);
    assert_eq!(out.status.code(), Some(72));
}

#[test]
fn fused_map_where_sum_correct_under_o2() {
    if !backend_available() {
        return;
    }
    // map + where + sum fused into one loop: keep the doubled values that are > 6, then sum.
    // doubled = 2,4,6,8,10,12; kept (>6) = 8,10,12 → 30.
    let src = concat!(
        "fn dbl(x: i64) -> i64 = x * 2\n",
        "fn big(x: i64) -> bool = x > 6\n",
        "fn run(xs: slice<i64>) -> i64 = xs.map(dbl).where(big).sum()\n",
        "fn main() -> i32 {\n",
        "  a := [1, 2, 3, 4, 5, 6]\n",
        "  return run(a) as i32\n",
        "}\n",
    );
    let out = build_and_run("opt-map-where-sum", src);
    assert_eq!(out.status.code(), Some(30));
}

#[test]
fn fused_where_min_correct_under_o2() {
    if !backend_available() {
        return;
    }
    // The branchless min reduction (`select` + `llvm.smin` idiom) must survive -O2 vectorization
    // (over a `slice<i32>` it lowers to `pminsd` over a `pcmpgtd` mask — verified via objdump) and
    // still be correct. keep > 2 of [5,1,8,2,9,3] → {5,8,9,3}; min = 3.
    // The literal is passed straight to the `slice<i32>` param so its elements infer i32 (a
    // `let`-bound array literal would default to i64).
    let src = concat!(
        "fn k(x: i32) -> bool = x > 2\n",
        "fn run(xs: slice<i32>) -> i32 = xs.where(k).min()\n",
        "fn main() -> i32 {\n",
        "  return run([5, 1, 8, 2, 9, 3])\n",
        "}\n",
    );
    assert_eq!(build_and_run("opt-where-min", src).status.code(), Some(3));
}

#[test]
fn fused_where_reduce_correct_under_o2() {
    if !backend_available() {
        return;
    }
    // The accumulator-select reduce (`acc = mask ? f(acc,x) : acc`) must survive -O2 unchanged.
    // keep odd of [1..=6] → {1,3,5}; product seeded 1 = 15.
    let src = concat!(
        "fn odd(x: i64) -> bool = x % 2 == 1\n",
        "fn mul(a: i64, x: i64) -> i64 = a * x\n",
        "fn run(xs: slice<i64>) -> i64 = xs.where(odd).reduce(1, mul)\n",
        "fn main() -> i32 {\n",
        "  a := [1, 2, 3, 4, 5, 6]\n",
        "  return run(a) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("opt-where-reduce", src).status.code(), Some(15));
}
