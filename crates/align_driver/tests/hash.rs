//! `core.hash` — wyhash, Align's one canonical non-cryptographic hash (`draft.md` §18.1).
//! `hash64(data) -> u64` and `hash128(data) -> (u64, u64)` over a byte view (`str` / owned
//! `string` / `slice<u8>`). Deterministic within a build; the argument is borrowed, not consumed.
//! The byte-faithful port of wyhash itself is unit-tested in `align_runtime`; these are the
//! end-to-end language tests.

mod common;
use common::*;

#[test]
fn hash64_is_deterministic() {
    if !backend_available() {
        return;
    }
    // The same bytes hash to the same value; the result is usable as an ordinary u64.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  if hash64(\"align\") == hash64(\"align\") { return 7 }\n",
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("hash64-determinism", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn distinct_inputs_hash_distinctly() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "fn main() -> i32 {\n",
        "  if hash64(\"a\") != hash64(\"b\") { return 11 }\n",
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("hash64-distinct", src);
    assert_eq!(out.status.code(), Some(11));
}

#[test]
fn hash128_first_lane_equals_hash64() {
    if !backend_available() {
        return;
    }
    // `hash128(x).0 == hash64(x)` — the 128-bit form's low lane is the 64-bit hash.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  pair := hash128(\"align\")\n",
        "  if pair.0 == hash64(\"align\") { return 13 }\n",
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("hash128-low-lane", src);
    assert_eq!(out.status.code(), Some(13));
}

#[test]
fn hash128_lanes_differ() {
    if !backend_available() {
        return;
    }
    // The two lanes are decorrelated (the high lane is a second, reseeded pass).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  pair := hash128(\"align\")\n",
        "  if pair.0 != pair.1 { return 17 }\n",
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("hash128-lanes-differ", src);
    assert_eq!(out.status.code(), Some(17));
}

#[test]
fn hashes_an_owned_string_without_consuming_it() {
    if !backend_available() {
        return;
    }
    // An owned `string` is borrowed by `hash64` (like `print`), so it stays usable afterwards.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  s := \"align\".clone()\n",
        "  h := hash64(s)\n",
        "  if h == hash64(\"align\") { return 19 }\n",
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("hash64-owned-string", src);
    assert_eq!(out.status.code(), Some(19));
}

#[test]
fn hashing_a_non_byte_view_is_rejected() {
    // An int is not a byte view.
    assert!(check_errs(
        "hash-int",
        "fn main() -> i32 {\n  x := hash64(5)\n  return 0\n}\n",
    ));
}

#[test]
fn hash_with_wrong_arity_is_rejected() {
    assert!(check_errs(
        "hash-arity",
        "fn main() -> i32 {\n  x := hash64(\"a\", \"b\")\n  return 0\n}\n",
    ));
}
