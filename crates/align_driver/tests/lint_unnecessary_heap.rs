//! The "unnecessary heap" lint (`draft.md` §16, M8 lint batch 2): `heap.new(x).get()` bump-allocates
//! a box in the arena only to immediately read the scalar straight back out — the allocation serves
//! no purpose (a `box<T>` payload is a scalar in M3, so `.get()` is a plain copy-out). This lint
//! emits a **warning** for it; the program still type-checks and runs. It is detected purely locally
//! — a `.get()` whose receiver is the allocating `heap.new` *itself* — so it reuses no
//! escape-analysis state and never false-positives. A box bound to a local and read later
//! (`p := heap.new(x); p.get()`) is deliberately *not* flagged: that broader case needs a
//! whole-function box-use scan and is deferred (see `open-questions.md` M8 lint candidates).

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

// --- negative: the box-bound-to-a-local case stays silent (deferred) ---------------------------

#[test]
fn box_bound_to_local_then_read_does_not_warn() {
    // `p := heap.new(x); p.get()` — the `.get()` receiver is the local `p`, not the `heap.new`.
    // The narrow local lint deliberately does not fire here (the broader box-use scan is deferred).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    p: box<i32> := heap.new(7)\n",
        "    p.get()\n",
        "  }\n",
        "}\n",
    );
    assert!(!warns_heap("local-get", src));
}

#[test]
fn cloned_box_does_not_warn() {
    // A `.clone()` receiver is not a `.get()`, so nothing fires — and the box is used twice.
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
