//! `sort_by_key(f)` — materialize the surviving scalar elements and sort ascending by `f(element)`
//! (`draft.md` §8). The key function may be named or a lambda (which may capture). Shares the MIR
//! stable merge sort with `sort()`, precomputing each key once (decorate) and comparing keys instead
//! of elements. The larger correctness/stability net lives in `sort_merge.rs`.


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

/// An owned `string` key is orderable (`Ord` compares it through the same `str` comparator), but
/// the fused sort buffers one key per element and has no per-key drop. Sema stopped at
/// orderability, so a `string` key passed `check` and then failed HIR validation at the MIR
/// boundary as an internal error instead of being diagnosed.
///
/// This owns the wording for both key-function spellings; `align_mir`'s
/// `move_copy_positions_are_refused_by_the_producer_not_the_boundary` owns the property that the
/// build stops here (a `check`-only assertion about the boundary would be vacuously true, since
/// `check` never runs it).
#[test]
fn sort_by_key_move_key_rejected() {
    for (label, src) in [
        (
            "named-fn",
            "fn key(x: i64) -> string = \"k\".clone()\nfn main() -> Result<(), Error> {\n  s := [3, 1, 2].sort_by_key(key)\n  print(s[0])\n  return Ok(())\n}\n",
        ),
        (
            "lambda",
            "fn main() -> Result<(), Error> {\n  s := [3, 1, 2].sort_by_key(fn x { \"k\".clone() })\n  print(s[0])\n  return Ok(())\n}\n",
        ),
    ] {
        let diagnostics = check_diagnostics(&format!("sbk-move-key-{label}"), src);
        assert!(
            diagnostics.contains("'sort_by_key' cannot buffer a Move key"),
            "an owned `string` key must be diagnosed ({label}):\n{diagnostics}",
        );
        // The key function's return type is the user's to choose, so this row — unlike the element
        // rows — can name a workaround that actually exists.
        assert!(
            diagnostics.contains("return a Copy key (int/float/char) or a borrowed `str`"),
            "the diagnostic must point at the real workaround ({label}):\n{diagnostics}",
        );
    }
}
