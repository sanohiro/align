//! The `align(N)` over-alignment attribute on a struct declaration (M6). `align(N) Name { … }`
//! over-aligns the type's storage to `N` bytes (a power of two) — for GPU / DMA / page-aligned
//! zero-copy interop. The alignment is honored at the one `type_align` codegen seam (the slot
//! alloca / struct-array element); it only ever *over*-aligns (max with the natural alignment).

mod common;
use common::*;

#[test]
fn an_aligned_struct_compiles_and_runs() {
    if !backend_available() {
        return;
    }
    // The over-alignment doesn't change the value's semantics — it just over-aligns its storage.
    let src = concat!(
        "align(64) Aligned {\n",
        "  x: i32,\n",
        "  y: i32,\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  p := Aligned { x: 3, y: 4 }\n",
        "  return p.x + p.y\n",
        "}\n",
    );
    let out = build_and_run("align-ok", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn a_page_aligned_struct_works() {
    if !backend_available() {
        return;
    }
    // A larger, page-sized alignment (4096) — the DMA / page-aligned interop case.
    let src = concat!(
        "align(4096) Page {\n",
        "  a: i64,\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  p := Page { a: 41 }\n",
        "  return (p.a + 1) as i32\n",
        "}\n",
    );
    let out = build_and_run("align-page", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn a_non_power_of_two_alignment_is_rejected() {
    let src = "align(24) Bad {\n  x: i32,\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("align-pow2", src));
}

#[test]
fn an_alignment_on_a_sum_type_is_rejected() {
    let src = "align(16) Color {\n  Red,\n  Green,\n  Blue,\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("align-enum", src));
}

#[test]
fn an_aligned_struct_embedded_in_a_field_or_array_is_rejected() {
    // The over-alignment is honored only for a standalone value (size-padding for embedding is
    // deferred), so embedding it must be a clean error rather than a silently-dropped alignment.
    let nested = "align(16) Inner {\n  x: i32,\n}\nOuter {\n  i: Inner,\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("align-nested", nested));
    let array = "align(16) S {\n  x: i32,\n}\nfn f(a: array<S>) -> i32 = 0\nfn main() -> i32 = 0\n";
    assert!(check_errs("align-array", array));
    let lit = "align(16) S {\n  x: i32,\n}\nfn main() -> i32 {\n  xs := [S { x: 1 }, S { x: 2 }]\n  return 0\n}\n";
    assert!(check_errs("align-arraylit", lit));
}

#[test]
fn a_too_large_alignment_is_rejected() {
    let src = "align(4294967296) Huge {\n  x: i32,\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("align-huge", src));
}
