//! FFI views: passing a `str`/`slice` to an `extern "C"` function as its **data pointer** (draft.md
//! §15). An Align view is `{ptr, len}`; across the C boundary it lowers to just the pointer
//! (C `char*`/`void*`), and the length is passed separately by the caller (`s.len()`) when the C
//! function needs it — matching the C `(ptr, len)` idiom without hiding an argument. A view is *not*
//! a valid return type (a bare C pointer carries no length), so returns stay scalar-only.

mod common;
use common::*;

fn ok(src: &str) -> bool {
    let mut sm = SourceMap::new();
    !check(&mut sm, "ffi_views", src).diags.has_errors()
}

#[test]
fn str_arg_via_memcmp() {
    if !backend_available() {
        return;
    }
    // Two `str` args lower to two `char*`; memcmp over 5 bytes: "hello"=="hello" → 0, "hello"!="world"
    // → nonzero. Exit 7 confirms both pointers reached C correctly.
    let out = build_and_run(
        "ffi-str-memcmp",
        "extern \"C\" fn memcmp(a: str, b: str, n: i64) -> i32\n\nfn main() -> i32 {\n  unsafe {\n    x := \"hello\"\n    y := \"hello\"\n    eq := memcmp(x, y, 5)\n    z := \"world\"\n    ne := memcmp(x, z, 5)\n    if ne != 0 {\n      return eq + 7\n    }\n    return 99\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn str_len_as_length_argument() {
    if !backend_available() {
        return;
    }
    // The length travels separately, via `s.len()` — the C idiom. memcmp of equal strings → 0.
    let out = build_and_run(
        "ffi-str-len",
        "extern \"C\" fn memcmp(a: str, b: str, n: i64) -> i32\n\nfn main() -> i32 {\n  unsafe {\n    x := \"abcdef\"\n    y := \"abcdef\"\n    return memcmp(x, y, x.len())\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn slice_arg_from_array_via_memcmp() {
    if !backend_available() {
        return;
    }
    // An `array` coerces to a `slice` at the call; the slice is passed as its data pointer. memcmp of
    // a buffer against itself (24 bytes) → 0, +5 → 5.
    let out = build_and_run(
        "ffi-slice-memcmp",
        "extern \"C\" fn memcmp(a: slice<i64>, b: slice<i64>, n: i64) -> i32\n\nfn main() -> i32 {\n  arr := [10, 20, 30]\n  unsafe {\n    return memcmp(arr, arr, 24) + 5\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(5));
}

#[test]
fn str_param_is_accepted() {
    assert!(ok("extern \"C\" fn f(s: str, n: i64) -> i32\nfn main() -> i32 {\n  unsafe {\n    return f(\"x\", 1)\n  }\n}\n"));
}

#[test]
fn slice_param_is_accepted() {
    assert!(ok("extern \"C\" fn f(s: slice<u8>, n: i64) -> i32\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn slice_of_non_scalar_element_is_rejected() {
    // A `slice<str>` element (a `{ptr,len}` view) has no settled C representation, so handing C a
    // pointer to that buffer would misinterpret it — rejected. Only int/float element slices qualify.
    assert!(!ok("extern \"C\" fn f(s: slice<str>, n: i64) -> i32\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn str_return_is_rejected() {
    // A bare C pointer carries no length, so a view is not a valid return type.
    assert!(!ok("extern \"C\" fn f(x: i32) -> str\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn slice_return_is_rejected() {
    assert!(!ok("extern \"C\" fn f(x: i32) -> slice<u8>\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn str_arg_call_still_requires_unsafe() {
    assert!(!ok("extern \"C\" fn memcmp(a: str, b: str, n: i64) -> i32\nfn main() -> i32 {\n  return memcmp(\"a\", \"a\", 1)\n}\n"));
}
