//! The "unnecessary heap" lint (`draft.md` §16, M8 lint batch 2). A `box<T>` payload is a scalar in
//! M3, so `.get()` is a plain copy-out; a box that is only ever read back with `.get()` and never
//! escapes serves no purpose (a stack value suffices). The lint emits a **warning** for it; the
//! program still type-checks and runs. It has two disjoint slices:
//!
//!   * **narrow** (inline `heap.new(x).get()`): a `.get()` whose receiver is the allocating `heap.new`
//!     *itself*, detected purely locally in `finalize_expr`.
//!   * **broad** (`p := heap.new(x); … p.get()`): a box bound to a local that is only ever a `.get()`
//!     receiver, detected by a whole-function box-use scan (`UnnecessaryHeapScan`) that classifies
//!     every occurrence of every box local. Any other occurrence — a move, a `.clone()`, a store, a
//!     return, a call argument, a capture, a reassignment target — suppresses it (sound / conservative).

mod common;
use common::*;

/// The formatted diagnostics for checking `src` (warnings included).
fn diags(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    align_driver::format_diagnostics(&sm, &checked.diags)
}

/// Whether checking `src` emits an "unnecessary heap allocation" diagnostic.
fn warns_heap(name: &str, src: &str) -> bool {
    diags(name, src).contains("unnecessary heap allocation")
}

// --- positive: `heap.new(x).get()` inline warns ------------------------------------------------

#[test]
fn inline_heap_new_get_warns() {
    // Allocate a box and immediately read it back — the allocation is pointless. (`x: i32` fixes
    // the box payload width so the example is itself a well-typed program.)
    let src = concat!(
        "fn main() -> i32 {\n",
        "  x: i32 := 7\n",
        "  arena {\n",
        "    v: i32 := heap.new(x).get()\n",
        "    v\n",
        "  }\n",
        "}\n",
    );
    let d = diags("inline-get", src);
    assert!(d.contains("unnecessary heap allocation"), "expected a warning, got:\n{d}");
    assert!(d.contains("a stack value suffices"), "message should suggest the fix:\n{d}");
}

#[test]
fn inline_heap_new_get_in_expression_warns() {
    // The same smell inside a larger expression still fires (the receiver of `.get()` is the
    // `heap.new` itself).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  x: i32 := 7\n",
        "  arena {\n",
        "    v: i32 := heap.new(x).get() + 1\n",
        "    v\n",
        "  }\n",
        "}\n",
    );
    assert!(warns_heap("inline-get-expr", src));
}

// --- positive (broad): a box local only ever `.get()`-ed warns ---------------------------------

#[test]
fn box_bound_to_local_then_read_warns() {
    // `p := heap.new(x); p.get()` — the box is bound to a local and only ever read back with
    // `.get()`, never escaping. The whole-function scan flags it (the narrow inline lint would not,
    // since the `.get()` receiver is the local `p`, not the `heap.new`).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    p: box<i32> := heap.new(7)\n",
        "    p.get()\n",
        "  }\n",
        "}\n",
    );
    let d = diags("local-get", src);
    assert!(d.contains("unnecessary heap allocation"), "expected a warning, got:\n{d}");
    assert!(d.contains("a stack value suffices"), "message should suggest the fix:\n{d}");
}

#[test]
fn box_read_back_multiple_times_still_warns() {
    // Multiple `.get()`s are all read-backs — no other use — so it still fires (one warning).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    p: box<i32> := heap.new(7)\n",
        "    p.get() + p.get()\n",
        "  }\n",
        "}\n",
    );
    let d = diags("multi-get", src);
    assert_eq!(
        d.matches("unnecessary heap allocation").count(),
        1,
        "exactly one warning per box local, got:\n{d}",
    );
}

#[test]
fn two_get_only_boxes_warn_once_each() {
    // Two independent get-only boxes → one warning per box (root cause = the allocation).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    p: box<i32> := heap.new(7)\n",
        "    q: box<i32> := heap.new(35)\n",
        "    p.get() + q.get()\n",
        "  }\n",
        "}\n",
    );
    let d = diags("two-boxes", src);
    assert_eq!(
        d.matches("unnecessary heap allocation").count(),
        2,
        "one warning per get-only box, got:\n{d}",
    );
}

#[test]
fn get_only_box_in_nested_block_warns() {
    // The scan is whole-function, so a get-only box in a nested block still fires.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    r: i32 := {\n",
        "      p: box<i32> := heap.new(9)\n",
        "      p.get()\n",
        "    }\n",
        "    r\n",
        "  }\n",
        "}\n",
    );
    assert!(warns_heap("nested", src));
}

// --- negative (broad): any use beyond `.get()` suppresses the warning ---------------------------

#[test]
fn moved_box_does_not_warn() {
    // Binding the box to a new name *moves* it (a non-`.get()` occurrence of `p`), so the heap
    // identity is genuinely used — no warning. (`q` is bound to a move, not a `heap.new`, so it is
    // not itself a scanned box local.)
    let src = concat!(
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    p: box<i32> := heap.new(7)\n",
        "    q: box<i32> := p\n",
        "    q.get()\n",
        "  }\n",
        "}\n",
    );
    assert!(!warns_heap("moved", src));
}

#[test]
fn cloned_box_does_not_warn() {
    // A `.clone()` receiver is not a `.get()`, so `p` has an "other" occurrence and nothing fires —
    // and `q` is bound to a `.clone()`, not a `heap.new`, so it is not a scanned box local either.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    p: box<i32> := heap.new(7)\n",
        "    q: box<i32> := p.clone()\n",
        "    p.get() + q.get()\n",
        "  }\n",
        "}\n",
    );
    assert!(!warns_heap("cloned", src));
}

#[test]
fn box_never_read_does_not_warn() {
    // A box that is allocated but never `.get()`-ed is a different smell (a dead allocation), not
    // this lint — the scan requires at least one read-back, so it stays silent here.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    p: box<i32> := heap.new(7)\n",
        "    0\n",
        "  }\n",
        "}\n",
    );
    assert!(!warns_heap("never-read", src));
}

// --- the lint is a warning, not a hard error --------------------------------------------------

#[test]
fn the_lint_is_not_a_hard_error() {
    let src = concat!(
        "fn main() -> i32 {\n",
        "  x: i32 := 7\n",
        "  arena {\n",
        "    v: i32 := heap.new(x).get()\n",
        "    v\n",
        "  }\n",
        "}\n",
    );
    assert!(!check_errs("heap-not-error", src));
}

#[test]
fn the_broad_lint_is_not_a_hard_error() {
    // The broad (box-local) form is also a warning only — it must warn but not error.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    p: box<i32> := heap.new(7)\n",
        "    p.get()\n",
        "  }\n",
        "}\n",
    );
    assert!(warns_heap("broad-not-error", src));
    assert!(!check_errs("broad-not-error", src));
}

#[test]
fn a_broad_unnecessary_heap_program_still_compiles_and_runs() {
    if !backend_available() {
        return;
    }
    // The box-local warning is emitted, yet the program builds and runs (exit 7).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    p: box<i32> := heap.new(7)\n",
        "    p.get()\n",
        "  }\n",
        "}\n",
    );
    let out = build_and_run("broad-heap-run", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn an_unnecessary_heap_program_still_compiles_and_runs() {
    if !backend_available() {
        return;
    }
    // The warning is emitted, the program builds and runs (exit 7).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  x: i32 := 7\n",
        "  arena {\n",
        "    v: i32 := heap.new(x).get()\n",
        "    v\n",
        "  }\n",
        "}\n",
    );
    let out = build_and_run("heap-run", src);
    assert_eq!(out.status.code(), Some(7));
}
