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
fn an_aligned_struct_as_a_field_or_dynamic_array_element_is_rejected() {
    // A *fixed* array of an `align(N)` struct is supported (see `a_fixed_array_of_an_aligned_struct_*`
    // below): the stack slot is over-aligned and the struct's LLVM size is padded for a tight stride.
    // Two embeddings stay rejected — they need machinery still deferred, so they must be a clean error
    // rather than a silently-dropped alignment:
    //   - as a *struct field*: honoring it needs the aggregate type's ABI alignment to actually be
    //     `N`, which LLVM can't express for a struct type (it's applied at the alloca, not the type);
    //   - as a *dynamic* `array<S>` element: its heap buffer over-alignment is a separate deferred item.
    let nested = "align(16) Inner {\n  x: i32,\n}\nOuter {\n  i: Inner,\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("align-nested", nested));
    let array = "align(16) S {\n  x: i32,\n}\nfn f(a: array<S>) -> i32 = 0\nfn main() -> i32 = 0\n";
    assert!(check_errs("align-array", array));
}

#[test]
fn a_fixed_array_of_an_aligned_struct_compiles_and_runs() {
    if !backend_available() {
        return;
    }
    // A fixed `[S{…}, …]` array of an `align(64)` struct: every element keeps the over-alignment
    // (padded stride), and the fused pipeline reads/writes it at that stride. Sum of `.x` over the
    // three elements = 1 + 3 + 5 = 9.
    let src = concat!(
        "align(64) P {\n",
        "  x: i64,\n",
        "  y: i64,\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  xs := [P { x: 1, y: 2 }, P { x: 3, y: 4 }, P { x: 5, y: 6 }]\n",
        "  return xs.x.sum() as i32\n",
        "}\n",
    );
    let out = build_and_run("align-arr", src);
    assert_eq!(out.status.code(), Some(9));
}

#[test]
fn a_fixed_array_of_an_aligned_struct_has_a_padded_over_aligned_stride() {
    // The alignment is verified structurally (x86-64 doesn't fault on unaligned access, so a runtime
    // functional test can't observe it): the struct's LLVM type is size-padded up to `align(64)` with
    // an `[K x i8]` tail, and the array's alloca is `align 64`. Given LLVM's `[N x T]` layout contract
    // (elements at `base + i*sizeof(T)`), a 64-byte stride from a 64-aligned base puts *every* element
    // on a 64-byte boundary. The padding tail is `align 1`, so it never inflates the type's own ABI
    // alignment — the over-alignment lives only at the alloca.
    let src = concat!(
        "align(64) P {\n",
        "  x: i64,\n",
        "  y: i64,\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  xs := [P { x: 1, y: 2 }, P { x: 3, y: 4 }, P { x: 5, y: 6 }]\n",
        "  return xs.x.sum() as i32\n",
        "}\n",
    );
    let ir = emit_llvm(src);
    // Natural size 16 (two i64s) padded up to 64 → a 48-byte tail.
    assert!(ir.contains("%P = type { i64, i64, [48 x i8] }"), "want a size-padded struct type:\n{ir}");
    // The array slot is over-aligned to 64 (the base of the over-aligned stride).
    assert!(ir.contains("alloca [3 x %P], align 64"), "want a 64-aligned array alloca:\n{ir}");
}

#[test]
fn a_too_large_alignment_is_rejected() {
    let src = "align(4294967296) Huge {\n  x: i32,\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("align-huge", src));
}
