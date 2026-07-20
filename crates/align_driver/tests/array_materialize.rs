//! Owned dynamic arrays (`array<T>`) are heap-allocated (`{ptr,len}`, Move, dropped) and can be
//! returned / passed / reassigned — the materialization is `.to_array()` (visible allocation).
//! A *bare* fixed array literal is a stack value and cannot silently become an owned `array<T>`
//! (that would hide the heap allocation — "Nothing hidden", and codegen miscompiled it): it is a
//! clean error pointing at `.to_array()`.

mod common;
use common::*;

#[test]
fn to_array_result_is_returnable_and_dropped() {
    if !backend_available() {
        return;
    }
    // A heap `array<i64>` built with `.to_array()` (no arena → `malloc`) is returned out of `make`,
    // used, and dropped in `main` — no leak / double-free (the runtime aborts on corruption).
    let src = "\
fn make() -> array<i64> {
  return [10, 20, 30].to_array()
}
fn main() -> i32 {
  xs := make()
  return xs[1] as i32
}
";
    assert_eq!(build_and_run("arr-return", src).status.code(), Some(20));
}

#[test]
fn to_array_is_passable_and_reassignable() {
    if !backend_available() {
        return;
    }
    let src = "\
fn mk(a: i64) -> array<i64> = [a, a + 1, a + 2].to_array()
fn sink(xs: array<i64>) -> i64 = xs[0] + xs[2]
fn main() -> i32 {
  mut xs := mk(1)
  xs = mk(10)
  return sink(xs) as i32
}
";
    // xs = mk(10) = [10, 11, 12]; sink = 10 + 12 = 22. Reassign drops the old [1,2,3] (no leak).
    assert_eq!(build_and_run("arr-pass-reassign", src).status.code(), Some(22));
}

#[test]
fn bare_literal_in_owned_array_context_is_rejected() {
    // A bare fixed literal can't become an owned `array<T>` — it must be `.to_array()`'d (visible
    // heap allocation). Rejected in every owned-array context (was silently miscompiled to garbage):
    // a call argument,
    assert!(check_errs(
        "bare-arg",
        "fn sink(xs: array<i64>) -> i64 = xs[0]\nfn main() -> i32 { return sink([4, 5, 6]) as i32 }\n"
    ));
    // a return value,
    assert!(check_errs(
        "bare-ret",
        "fn f() -> array<i64> = [1, 2, 3]\nfn main() -> i32 = 0\n"
    ));
    // and an `array<T>`-annotated binding.
    assert!(check_errs(
        "bare-let",
        "fn main() -> i32 {\n  xs: array<i64> := [1, 2, 3]\n  return xs[0] as i32\n}\n"
    ));
}

#[test]
fn bare_literal_in_value_position_is_rejected() {
    // A fixed array literal has no free-value MIR lowering; it can initialize a local or feed an
    // array pipeline, but it cannot be the materialized value of a block/if/match expression.
    let block = check_diagnostics(
        "bare-array-block-value",
        "fn main() {\n  xs := { [1, 2, 3] }\n  print(xs[0])\n}\n",
    );
    assert!(block.contains("bare array literal cannot be used as a block value"), "expected a block-value diagnostic:\n{block}");

    let if_value = check_diagnostics(
        "bare-array-if-value",
        "fn main() {\n  xs := if true { [1, 2] } else { [3, 4] }\n  print(xs[0])\n}\n",
    );
    assert!(if_value.contains("bare array literal cannot be used as a block value"), "expected an if-branch value diagnostic:\n{if_value}");

    let match_value = check_diagnostics(
        "bare-array-match-value",
        "fn pick(x: Option<i64>) {\n  xs := match x {\n    Some(_) => [1, 2]\n    None => [3, 4]\n  }\n  print(xs[0])\n}\nfn main() {}\n",
    );
    assert!(match_value.contains("bare array literal cannot be used as a `match` arm value"), "expected a match-arm value diagnostic:\n{match_value}");
}

#[test]
fn fixed_array_literal_as_a_local_still_works() {
    if !backend_available() {
        return;
    }
    // The fix is narrow: a fixed array literal bound without an owned-array annotation is unchanged
    // (a stack array), and its pipeline / indexing still work.
    let src = "\
fn main() -> i32 {
  xs := [3, 4, 5]
  return (xs[0] + xs[2]) as i32
}
";
    assert_eq!(build_and_run("arr-local", src).status.code(), Some(8));
}

#[test]
fn struct_array_literal_with_call_elements_is_stored() {
    if !backend_available() {
        return;
    }
    // An array literal of STRUCTS whose elements are calls (not struct literals) — the route-table
    // shape `[web.get(…), web.post(…)]`. The struct-array store used to handle only `StructLit`
    // elements and silently stored NOTHING for any other, leaving the element uninitialised: a
    // zeroed scalar and a garbage `str` pointer. Each element must round-trip.
    let src = concat!(
        "S { a: str, b: i64 }\n",
        "fn mk(x: i64) -> S = S { a: \"made\", b: x }\n",
        "fn main() -> Result<(), Error> {\n",
        "  lit := [S { a: \"lit\", b: 1 }]\n",
        "  l0 := lit[0]\n",
        "  print(l0.a)\n",
        "  calls := [mk(10), mk(20)]\n",
        "  c0 := calls[0]\n",
        "  c1 := calls[1]\n",
        "  print(c0.b)\n",
        "  print(c0.a)\n",
        "  print(c1.b)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("arr-struct-call-elems", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "lit\n10\nmade\n20\n");
}
