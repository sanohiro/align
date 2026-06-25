//! `sort_by_key(f)` — materialize the surviving scalar elements and sort ascending by `f(element)`
//! (`draft.md` §8). The key function may be named or a lambda (which may capture). Reuses the MIR
//! insertion sort, comparing keys instead of elements.


mod common;
use common::*;

#[test]
fn sort_by_key_descending_via_named_fn() {
    if !backend_available() {
        return;
    }
    // Sorting by `-x` yields descending order: [5,4,3,2,1,1].
    let src = "fn neg(x: i64) -> i64 = -x\nfn main() -> Result<(), Error> {\n  arena {\n    s := [3, 1, 4, 1, 5, 2].sort_by_key(neg)\n    print(s[0])\n    print(s[5])\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("sbk-desc", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n1\n");
}

#[test]
fn sort_by_key_lambda_by_last_digit() {
    if !backend_available() {
        return;
    }
    // Sort by last digit: 10(0), 21(1), 32(2), 3(3) → first 10, last 3.
    let src = "fn main() -> Result<(), Error> {\n  arena {\n    s := [10, 21, 32, 3].sort_by_key(fn x { x % 10 })\n    print(s[0])\n    print(s[3])\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("sbk-mod", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n3\n");
}

#[test]
fn sort_by_key_capturing_lambda() {
    if !backend_available() {
        return;
    }
    // The key lambda captures `mult` (positive), so `x * mult` orders the same as `x` ascending.
    let src = "fn main() -> Result<(), Error> {\n  mult := 3\n  arena {\n    s := [1, 5, 3, 2, 4].sort_by_key(fn x { x * mult })\n    print(s[0])\n    print(s[4])\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("sbk-capture", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n5\n");
}

#[test]
fn sort_by_key_outside_arena_frees_temp() {
    if !backend_available() {
        return;
    }
    // `sort_by_key(...).sum()` outside an arena: the sorted array is a heap temporary consumed by
    // `sum` — it must be freed, not leaked or double-freed. 3+1+2 = 6.
    let src = "fn neg(x: i64) -> i64 = -x\nfn main() -> Result<(), Error> {\n  print([3, 1, 2].sort_by_key(neg).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("sbk-sum", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n");
}

#[test]
fn sort_by_key_non_orderable_key_rejected() {
    // A bool key is not orderable.
    let src = "fn main() -> Result<(), Error> {\n  arena {\n    s := [1, 2, 3].sort_by_key(fn x { x > 1 })\n    print(s[0])\n  }\n  return Ok(())\n}\n";
    assert!(check_errs("sbk-bad-key", src));
}
