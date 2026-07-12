//! Guarded and branchless `where` reductions. A suffix of field operations plus a builtin
//! `sum`/`count`/`min`/`max` uses a mask and identity-select so it stays vectorizable. A general
//! callable after `where` is control-flow guarded because Pure does not imply total/non-trapping.
//! These tests pin results, trapping behavior, and sequential effect order; the positive SIMD shape
//! is pinned in `vectorize_shapes.rs`.

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
fn rejected_element_does_not_run_trapping_map() {
    if !backend_available() {
        return;
    }
    let divide = "fn nonzero(x: i64) -> bool = x != 0\nfn reciprocal(x: i64) -> i64 = 10 / x\nfn main() -> i32 {\n  return [0, 2].where(nonzero).map(reciprocal).sum() as i32\n}\n";
    assert_eq!(build_and_run("where-guard-div", divide).status.code(), Some(5));

    let index = "fn valid(x: i64) -> bool = x < 2\nfn lookup(x: i64) -> i64 {\n  values := [10, 20]\n  return values[x]\n}\nfn main() -> i32 {\n  return [0, 2].where(valid).map(lookup).sum() as i32\n}\n";
    assert_eq!(build_and_run("where-guard-index", index).status.code(), Some(10));
}

#[test]
fn rejected_element_does_not_run_later_predicate_or_callable_reducer() {
    if !backend_available() {
        return;
    }
    let later_where = "fn nonzero(x: i64) -> bool = x != 0\nfn reciprocal_positive(x: i64) -> bool = 10 / x > 0\nfn main() -> i32 {\n  return [0, 2].where(nonzero).where(reciprocal_positive).sum() as i32\n}\n";
    assert_eq!(build_and_run("where-guard-predicate", later_where).status.code(), Some(2));

    let reduce = "fn nonzero(x: i64) -> bool = x != 0\nfn add_reciprocal(acc: i64, x: i64) -> i64 = acc + 10 / x\nfn main() -> i32 {\n  return [0, 2].where(nonzero).reduce(0, add_reciprocal) as i32\n}\n";
    assert_eq!(build_and_run("where-guard-reduce", reduce).status.code(), Some(5));

    let any_all = "fn nonzero(x: i64) -> bool = x != 0\nfn reciprocal_positive(x: i64) -> bool = 10 / x > 0\nfn main() -> i32 {\n  a := [0, 2].where(nonzero).any(reciprocal_positive)\n  b := [0, 2].where(nonzero).all(reciprocal_positive)\n  return if a && b { 1 } else { 0 }\n}\n";
    assert_eq!(build_and_run("where-guard-any-all", any_all).status.code(), Some(1));
}

#[test]
fn impure_sequential_stage_keeps_guarded_source_order() {
    if !backend_available() {
        return;
    }
    let after = "fn nonzero(x: i64) -> bool = x != 0\nfn noisy(x: i64) -> i64 {\n  print(x)\n  return x\n}\nfn main() -> Result<(), Error> {\n  total := [0, 2].where(nonzero).map(noisy).sum()\n  print(total)\n  return Ok(())\n}\n";
    let out = build_and_run("where-guard-impure-after", after);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n2\n");

    let before = "fn nonzero(x: i64) -> bool = x != 0\nfn noisy(x: i64) -> i64 {\n  print(x)\n  return x\n}\nfn main() -> Result<(), Error> {\n  total := [0, 2].map(noisy).where(nonzero).sum()\n  print(total)\n  return Ok(())\n}\n";
    let out = build_and_run("where-guard-impure-before", before);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n2\n2\n");
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
    // The seed-valued endpoints must survive the identity-select — the boundary case: an element
    // equal to the reducer's seed. `min` seeds with i64::MAX, so a surviving i64::MIN must still win
    // (it must beat that seed); `max` seeds with i64::MIN, so a surviving i64::MAX must still win.
    // The array carries *both* extremes (and a 0 the `where` drops), so each reducer must pick the
    // correct endpoint out of the two. i64::MIN as i32 = 0 (low 32 bits), i64::MAX as i32 = -1.
    let prelude = "fn nz(x: i64) -> bool = x != 0\n";
    let arr = "[0, -9223372036854775808, 9223372036854775807]";
    let min = build_and_run(
        "blw-min-ext",
        &format!("fn main() -> i32 {{\n  return {arr}.where(nz).min() as i32\n}}\n{prelude}"),
    );
    assert_eq!(min.status.code(), Some(0)); // i64::MIN → (0i32) as u8
    let max = build_and_run(
        "blw-max-ext",
        &format!("fn main() -> i32 {{\n  return {arr}.where(nz).max() as i32\n}}\n{prelude}"),
    );
    assert_eq!(max.status.code(), Some(255)); // i64::MAX → (-1i32) as u8
}
