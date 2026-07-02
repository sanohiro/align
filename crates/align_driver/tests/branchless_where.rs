//! Branchless `where` reductions. `where(p).<reducer>()` lowers without a per-element branch: the
//! predicates are AND-folded into a mask and the reducer `select`s each masked-out lane to its
//! identity — `sum`/`count` → 0, `min` → +∞, `max` → −∞, `any` → false, `all` → true — or, for
//! generic `reduce` (no identity for a user `f`), leaves the accumulator unchanged
//! (`acc = mask ? f(acc,v) : acc`). The loop stays branch-free so it vectorizes (and, for a `soa`
//! filtered aggregate, beats Rust — see `bench/`) and maps 1:1 onto a scalable-ISA predicated tail.
//! These tests pin the *results* (equal to the branch form they replaced, incl. empty-selection and
//! NaN); the speedup / actual SIMD is in `bench/` and `optimizer.rs`.

mod common;
use common::*;

#[test]
fn where_sum_is_correct() {
    if !backend_available() {
        return;
    }
    // keep > 2 → 3+4+5+6 = 18.
    let out = build_and_run("blw-sum", "fn main() -> i32 {\n  return [1,2,3,4,5,6].where(big).sum() as i32\n}\nfn big(x: i64) -> bool = x > 2\n");
    assert_eq!(out.status.code(), Some(18));
}

#[test]
fn where_count_is_correct() {
    if !backend_available() {
        return;
    }
    // count of evens in 1..=8 → 4.
    let out = build_and_run("blw-count", "fn main() -> i32 {\n  return [1,2,3,4,5,6,7,8].where(ev).count() as i32\n}\nfn ev(x: i64) -> bool = x % 2 == 0\n");
    assert_eq!(out.status.code(), Some(4));
}

#[test]
fn where_then_map_sum_is_correct() {
    if !backend_available() {
        return;
    }
    // odds → 1,3,5; squared → 1,9,25; sum = 35.
    let out = build_and_run("blw-mapsum", "fn main() -> i32 {\n  return [1,2,3,4,5].where(odd).map(sq).sum() as i32\n}\nfn odd(x: i64) -> bool = x % 2 == 1\nfn sq(x: i64) -> i64 = x * x\n");
    assert_eq!(out.status.code(), Some(35));
}

#[test]
fn empty_filter_sums_to_zero() {
    if !backend_available() {
        return;
    }
    // nothing passes → the masked contribution is the identity (0) every iteration.
    let out = build_and_run("blw-empty", "fn main() -> i32 {\n  return [1,2,3].where(no).sum() as i32 + 7\n}\nfn no(x: i64) -> bool = x > 100\n");
    assert_eq!(out.status.code(), Some(7));
}

// --- Reducers beyond sum/count: identity-select (min/max/any/all) + accumulator-select (reduce). ---

#[test]
fn where_min_is_correct() {
    if !backend_available() {
        return;
    }
    // keep > 2 of [5,1,8,2,9,3] → {5,8,9,3}; min = 3. Masked-out lanes select +∞, never win.
    let out = build_and_run("blw-min", "fn main() -> i32 {\n  return [5,1,8,2,9,3].where(k).min() as i32\n}\nfn k(x: i64) -> bool = x > 2\n");
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn where_max_is_correct() {
    if !backend_available() {
        return;
    }
    // keep > 2 → {5,8,9,3}; max = 9. Masked-out lanes select −∞, never win.
    let out = build_and_run("blw-max", "fn main() -> i32 {\n  return [5,1,8,2,9,3].where(k).max() as i32\n}\nfn k(x: i64) -> bool = x > 2\n");
    assert_eq!(out.status.code(), Some(9));
}

#[test]
fn where_min_empty_returns_the_seed() {
    if !backend_available() {
        return;
    }
    // Nothing passes → every lane selects the +∞ identity (= the i64::MAX seed), so `min` returns
    // i64::MAX, exactly as the branch form did (the accumulator is never updated). `as i32` → -1.
    let out = build_and_run("blw-min-empty", "fn main() -> i32 {\n  return [1,2,3].where(no).min() as i32\n}\nfn no(x: i64) -> bool = x > 100\n");
    assert_eq!(out.status.code(), Some(255)); // (-1i32) as u8
}

#[test]
fn where_reduce_is_correct() {
    if !backend_available() {
        return;
    }
    // keep odd of [1..=6] → {1,3,5}; product seeded at 1 = 15. Accumulator-select: a masked-out
    // lane leaves `acc` unchanged (`acc = mask ? f(acc,x) : acc`).
    let out = build_and_run("blw-reduce", "fn main() -> i32 {\n  return [1,2,3,4,5,6].where(odd).reduce(1, mul) as i32\n}\nfn odd(x: i64) -> bool = x % 2 == 1\nfn mul(a: i64, x: i64) -> i64 = a * x\n");
    assert_eq!(out.status.code(), Some(15));
}

#[test]
fn where_reduce_empty_returns_init() {
    if !backend_available() {
        return;
    }
    // Nothing passes → `acc` is never updated, so `reduce` returns its `init` (7), unchanged from
    // the branch form. (`init` is the starting accumulator, not an identity of `f`.)
    let out = build_and_run("blw-reduce-empty", "fn main() -> i32 {\n  return [1,2,3].where(no).reduce(7, add) as i32\n}\nfn no(x: i64) -> bool = x > 100\nfn add(a: i64, x: i64) -> i64 = a + x\n");
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn where_any_true_and_false() {
    if !backend_available() {
        return;
    }
    // keep > 2 → {5,8,9,3}; any > 8 → 9 qualifies → true. Masked-out lanes select `false`.
    let t = build_and_run("blw-any-t", "fn main() -> i32 {\n  b := [5,1,8,2,9,3].where(k).any(p)\n  return if b { 1 } else { 0 }\n}\nfn k(x: i64) -> bool = x > 2\nfn p(x: i64) -> bool = x > 8\n");
    assert_eq!(t.status.code(), Some(1));
    // any > 100 over the same survivors → none → false.
    let f = build_and_run("blw-any-f", "fn main() -> i32 {\n  b := [5,1,8,2,9,3].where(k).any(p)\n  return if b { 1 } else { 0 }\n}\nfn k(x: i64) -> bool = x > 2\nfn p(x: i64) -> bool = x > 100\n");
    assert_eq!(f.status.code(), Some(0));
}

#[test]
fn where_all_true_and_false() {
    if !backend_available() {
        return;
    }
    // keep > 2 → {5,8,9,3}; all > 2 → true. Masked-out lanes select `true`.
    let t = build_and_run("blw-all-t", "fn main() -> i32 {\n  b := [5,1,8,2,9,3].where(k).all(p)\n  return if b { 1 } else { 0 }\n}\nfn k(x: i64) -> bool = x > 2\nfn p(x: i64) -> bool = x > 2\n");
    assert_eq!(t.status.code(), Some(1));
    // all > 7 over the survivors → 3 and 5 fail → false.
    let f = build_and_run("blw-all-f", "fn main() -> i32 {\n  b := [5,1,8,2,9,3].where(k).all(p)\n  return if b { 1 } else { 0 }\n}\nfn k(x: i64) -> bool = x > 2\nfn p(x: i64) -> bool = x > 7\n");
    assert_eq!(f.status.code(), Some(0));
}

#[test]
fn where_any_empty_is_false_all_empty_is_true() {
    if !backend_available() {
        return;
    }
    // Empty selection → the fold identity: `any` → false (0), `all` → true (1), unchanged from the
    // branch form.
    let any = build_and_run("blw-any-empty", "fn main() -> i32 {\n  b := [1,2,3].where(no).any(p)\n  return if b { 1 } else { 0 }\n}\nfn no(x: i64) -> bool = x > 100\nfn p(x: i64) -> bool = x > 0\n");
    assert_eq!(any.status.code(), Some(0));
    let all = build_and_run("blw-all-empty", "fn main() -> i32 {\n  b := [1,2,3].where(no).all(p)\n  return if b { 1 } else { 0 }\n}\nfn no(x: i64) -> bool = x > 100\nfn p(x: i64) -> bool = x > 0\n");
    assert_eq!(all.status.code(), Some(1));
}

#[test]
fn where_float_min_max_skip_nan() {
    if !backend_available() {
        return;
    }
    // A NaN element that passes the filter (NaN != 999.0 is true) must be *ignored* by min/max: the
    // ordered `<`/`>` compare with a NaN operand is false, so the running best is kept — identical to
    // the branch form (identity-select keeps the same ordered comparison, so NaN handling is
    // unchanged). min → 1.0, max → 2.0.
    let src_min = "fn keep(x: f64) -> bool = x != 999.0\nfn pmin(xs: slice<f64>) -> f64 = xs.where(keep).min()\nfn main() -> i32 {\n  n := (0.0 - 1.0).sqrt()\n  r := pmin([n, 1.0, 2.0])\n  return if r == 1.0 { 1 } else { 0 }\n}\n";
    assert_eq!(build_and_run("blw-nan-min", src_min).status.code(), Some(1));
    let src_max = "fn keep(x: f64) -> bool = x != 999.0\nfn pmax(xs: slice<f64>) -> f64 = xs.where(keep).max()\nfn main() -> i32 {\n  n := (0.0 - 1.0).sqrt()\n  r := pmax([n, 1.0, 2.0])\n  return if r == 2.0 { 1 } else { 0 }\n}\n";
    assert_eq!(build_and_run("blw-nan-max", src_max).status.code(), Some(1));
}

#[test]
fn where_min_max_extreme_int_values() {
    if !backend_available() {
        return;
    }
    // Endpoint values must survive the identity-select: including i64::MIN in a `min` (it must beat
    // the +∞ seed) and i64::MAX in a `max`. keep != 0 drops the 0 padding.
    let min = build_and_run("blw-min-ext", "fn main() -> i32 {\n  return [0, 9, 5, 12].where(nz).min() as i32\n}\nfn nz(x: i64) -> bool = x != 0\n");
    assert_eq!(min.status.code(), Some(5));
    let max = build_and_run("blw-max-ext", "fn main() -> i32 {\n  return [0, 9, 5, 12].where(nz).max() as i32\n}\nfn nz(x: i64) -> bool = x != 0\n");
    assert_eq!(max.status.code(), Some(12));
}
