//! Branchless `where` reductions. `where(p).sum()` / `.count()` lower without a per-element branch:
//! the predicates are AND-folded into a mask and the contribution is `select`ed to the identity
//! (`acc += mask ? value : 0`), so the loop vectorizes (and, for a `soa` filtered aggregate, beats
//! Rust — see `bench/`). These tests pin the *results*; the speedup is in `bench/`.

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
