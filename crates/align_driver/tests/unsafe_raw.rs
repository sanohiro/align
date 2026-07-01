//! `unsafe {}` blocks + `raw.*` operations (draft.md §6.5 / §15). The first slice: `unsafe {}` as a
//! marker block expression, `raw.alloc(size)` → a `raw` byte pointer, `raw.free(p)`. `raw.*` ops are
//! confined to an `unsafe` block; a function containing one is inferred impure.

mod common;
use common::*;

fn ok(src: &str) -> bool {
    let mut sm = SourceMap::new();
    !check(&mut sm, "unsafe", src).diags.has_errors()
}

#[test]
fn unsafe_alloc_free_runs() {
    if !backend_available() {
        return;
    }
    // The draft §6.5 example: allocate + free inside `unsafe {}`. Exits 7 → the alloc/free ran and
    // (crucially) `p` was NOT auto-dropped after the explicit `raw.free` (which would double-free).
    let out = build_and_run(
        "unsafe-alloc-free",
        "fn main() -> i32 {\n  unsafe {\n    p := raw.alloc(64)\n    raw.free(p)\n  }\n  return 7\n}\n",
    );
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn raw_op_outside_unsafe_is_rejected() {
    // `raw.alloc` / `raw.free` are only valid inside an `unsafe {}` block.
    assert!(!ok("fn main() -> i32 {\n  p := raw.alloc(64)\n  raw.free(p)\n  return 0\n}\n"));
}

#[test]
fn raw_is_copy_and_not_auto_dropped() {
    // A `raw` pointer is Copy — binding it again does not move it (a double-free is the programmer's
    // responsibility, which is the whole point of `unsafe`). This must type-check.
    assert!(ok("fn main() -> i32 {\n  unsafe {\n    p := raw.alloc(64)\n    q := p\n    raw.free(p)\n    raw.free(q)\n  }\n  return 0\n}\n"));
}

#[test]
fn raw_is_a_nameable_parameter_type() {
    if !backend_available() {
        return;
    }
    // `raw` is a nameable type (a function parameter); holding a `raw` is safe, only `raw.*` ops need
    // `unsafe`. Exit 1 confirms the pointer threads through the call and frees cleanly.
    let out = build_and_run(
        "unsafe-raw-param",
        "fn use_it(p: raw) -> i32 {\n  unsafe { raw.free(p) }\n  return 1\n}\nfn main() -> i32 {\n  unsafe {\n    p := raw.alloc(32)\n    return use_it(p)\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn unsafe_block_is_a_transparent_marker_for_regions() {
    // `unsafe {}` opens no region of its own — an arena value returned through an `unsafe {}` block is
    // still caught by the escape check (the block's value inherits the arena value's region, not the
    // `Static` fallback). This guards against a use-after-free hole the marker block could open.
    assert!(!ok(concat!(
        "fn build() -> i32 {\n  mut out := 0\n  unsafe {\n    arena {\n      p := heap.new(5)\n      out = p.get()\n    }\n  }\n  return out\n}\n",
        "fn escape() -> box<i64> {\n  unsafe {\n    arena {\n      return heap.new(5)\n    }\n  }\n}\n",
    )));
    // The plain in-arena use (no escape) is fine.
    assert!(ok("fn build() -> i32 {\n  mut out := 0\n  unsafe {\n    arena {\n      p := heap.new(5)\n      out = p.get()\n    }\n  }\n  return out\n}\n"));
}

#[test]
fn raw_alloc_size_must_be_an_integer() {
    assert!(!ok("fn main() -> i32 {\n  unsafe {\n    p := raw.alloc(true)\n    raw.free(p)\n  }\n  return 0\n}\n"));
}
