//! Top-level **aggregate constants** — `NAME := [1, 2, 3]` / `NAME: slice<T> := [...]` (`draft.md`
//! §3, S1). The array-literal analogue of a scalar constant: it folds at compile time to a
//! `ConstVal::Array` and is substituted at each use as a **static `slice<T>` view** of a per-unit
//! private read-only global (rodata) — ownership is a property of the type, so a top-level array
//! constant is a `slice<T>`, never an owned `array<T>`. Elements are scalars / `str`; a constant
//! index folds to the element literal (no load); a dynamic index / pipeline / `.len()` flows through
//! the existing borrowed-slice paths against the rodata pointer (allocation-free). Struct constants
//! and struct elements are a recorded S1.5 deferral.

mod common;
use common::*;

// --- positive: declaration, inference, annotation, element types ---------------------------------

#[test]
fn a_bare_array_constant_infers_slice_i64() {
    if !backend_available() {
        return;
    }
    // `[10, 20, 30]` with no annotation infers `slice<i64>` (unconstrained-literal defaults).
    let src = "TABLE := [10, 20, 30]\nfn main() -> i32 { return TABLE.sum() as i32 }\n";
    let out = build_and_run("agg-infer", src);
    assert_eq!(out.status.code(), Some(60));
}

#[test]
fn an_annotated_slice_constant_takes_its_element_type() {
    if !backend_available() {
        return;
    }
    // The `slice<i32>` annotation pushes `i32` onto every element; the i32-returning `main` accepts
    // the reduced result with no coercion.
    let src = "TABLE: slice<i32> := [4, 8, 12]\nfn main() -> i32 { return TABLE.sum() }\n";
    let out = build_and_run("agg-annotated", src);
    assert_eq!(out.status.code(), Some(24));
}

#[test]
fn an_element_may_be_a_folded_expression_or_scalar_constant() {
    if !backend_available() {
        return;
    }
    // Element positions fold with the same const-eval as a scalar constant: arithmetic and a
    // reference to another *scalar* constant.
    let src = concat!(
        "W := 6\n",
        "TABLE := [W * 7, 100 - 58, W + W]\n",
        "fn main() -> i32 { return TABLE[1] as i32 }\n",
    );
    let out = build_and_run("agg-folded-elems", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn a_float_array_constant_reduces() {
    if !backend_available() {
        return;
    }
    let src = "TABLE := [1.5, 2.5, 3.0]\nfn main() -> i32 { return TABLE.sum() as i32 }\n";
    let out = build_and_run("agg-float", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn a_bool_array_constant_indexes() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "FLAGS := [false, true, false]\n",
        "fn main() -> i32 {\n",
        "  if FLAGS[1] { return 7 }\n",
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("agg-bool", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn a_char_array_constant_indexes() {
    if !backend_available() {
        return;
    }
    let src = "LETTERS := ['a', 'b', 'c']\nfn main() -> i32 { return LETTERS[2] as i32 }\n";
    let out = build_and_run("agg-char", src);
    assert_eq!(out.status.code(), Some('c' as i32));
}

#[test]
fn a_str_array_constant_looks_up_by_index() {
    if !backend_available() {
        return;
    }
    // A `slice<str>` constant lays out as `[N x {ptr,len}]` in rodata; each element views its own
    // static bytes. A dynamic index yields a `str` view whose `.len()` reads the header.
    let src = concat!(
        "NAMES := [\"a\", \"bb\", \"ccc\"]\n",
        "fn pick(i: i64) -> str = NAMES[i]\n",
        "fn main() -> i32 { return pick(2).len() as i32 }\n",
    );
    let out = build_and_run("agg-str", src);
    assert_eq!(out.status.code(), Some(3));
}

// --- positive: indexing, pipeline, len -----------------------------------------------------------

#[test]
fn a_dynamic_index_reads_the_rodata() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "TABLE := [10, 20, 30, 40]\n",
        "fn at(i: i64) -> i64 = TABLE[i]\n",
        "fn main() -> i32 { return at(3) as i32 }\n",
    );
    let out = build_and_run("agg-dyn-index", src);
    assert_eq!(out.status.code(), Some(40));
}

#[test]
fn a_pipeline_over_the_constant_runs() {
    if !backend_available() {
        return;
    }
    // where + map + reduce over the constant: an allocation-free scan of the rodata view.
    let src = concat!(
        "fn is_even(x: i64) -> bool = x % 2 == 0\n",
        "fn times_ten(x: i64) -> i64 = x * 10\n",
        "TABLE := [1, 2, 3, 4, 5, 6]\n",
        "fn main() -> i32 {\n",
        "  return TABLE.where(is_even).map(times_ten).sum() as i32\n",
        "}\n",
    );
    let out = build_and_run("agg-pipeline", src);
    assert_eq!(out.status.code(), Some(120)); // (2+4+6)*10
}

#[test]
fn len_of_the_constant_folds() {
    if !backend_available() {
        return;
    }
    let src = "TABLE := [3, 1, 4, 1, 5, 9]\nfn main() -> i32 { return TABLE.len() as i32 }\n";
    let out = build_and_run("agg-len", src);
    assert_eq!(out.status.code(), Some(6));
}

// --- IR gates ------------------------------------------------------------------------------------

#[test]
fn a_constant_index_folds_with_no_rodata_or_load() {
    // `TABLE[1]` with a constant index folds to the element literal in sema — the aggregate never
    // reaches codegen, so no rodata global is emitted at all for a pure constant-index program.
    let ir = emit_llvm("TABLE := [10, 20, 30]\nfn main() -> i32 { return TABLE[1] as i32 }\n");
    assert!(!ir.contains("const_arr"), "a constant index must fold away the rodata global:\n{ir}");
    assert!(ir.contains("ret i32 20"), "the folded element 20 must be returned directly:\n{ir}");
}

#[test]
fn the_constant_lowers_to_a_private_rodata_global() {
    // A dynamic use materializes the elements once as a `private unnamed_addr constant` global; the
    // scan reads it in place (no per-element `store`, no allocator call — allocation-free source).
    let ir = emit_llvm("TABLE := [1, 2, 3, 4]\nfn use(i: i64) -> i64 = TABLE[i]\nfn main() -> i32 = 0\n");
    assert!(
        ir.contains("@const_arr = private unnamed_addr constant [4 x i64] [i64 1, i64 2, i64 3, i64 4]"),
        "expected a private rodata global of the four elements:\n{ir}"
    );
}

/// Whether any *instruction* (not a `declare`) calls a runtime symbol matching `needle`.
fn calls_runtime(ir: &str, needle: &str) -> bool {
    ir.lines().any(|l| l.contains("call") && l.contains(needle))
}

#[test]
fn a_pipeline_source_is_allocation_free() {
    // The pipeline reads directly from `@const_arr`; there is no fresh array materialization
    // (no allocator call), unlike an owned `array<T>` producer.
    let ir = emit_llvm("TABLE := [1, 2, 3, 4]\nfn main() -> i32 { return TABLE.sum() as i32 }\n");
    assert!(ir.contains("@const_arr"), "the scan must source the rodata global:\n{ir}");
    assert!(!calls_runtime(&ir, "align_rt_arena_alloc"), "a constant source must not allocate:\n{ir}");
    assert!(!calls_runtime(&ir, "malloc"), "a constant source must not allocate:\n{ir}");
}

#[test]
fn the_constant_emits_no_drop_or_free() {
    // A static `slice<T>` view owns nothing; binding it and dropping scope must emit no free.
    let ir = emit_llvm(concat!(
        "TABLE := [1, 2, 3]\n",
        "fn main() -> i32 {\n",
        "  t := TABLE\n",
        "  return t.sum() as i32\n",
        "}\n",
    ));
    assert!(!calls_runtime(&ir, "align_rt_free"), "a constant slice view must not be freed:\n{ir}");
    assert!(!calls_runtime(&ir, "align_rt_arena_free"), "a constant slice view must not be freed:\n{ir}");
}

// --- cross-module --------------------------------------------------------------------------------

#[test]
fn a_pub_aggregate_constant_is_used_across_modules() {
    if !backend_available() {
        return;
    }
    // A `pub` aggregate constant reaches the importing module's per-unit surface (its `value_src`
    // rematerializes the slice against that unit's own rodata).
    let cfg = "module cfg\npub WEIGHTS := [2, 3, 5]\n";
    let main = concat!(
        "import cfg\n",
        "fn main() -> i32 { return cfg.WEIGHTS.sum() as i32 }\n",
    );
    let out = build_and_run_multi("agg-xmod", &[("cfg.align", cfg), ("main.align", main)], "main.align");
    assert_eq!(out.status.code(), Some(10));
}

// --- ownership / soundness -----------------------------------------------------------------------

#[test]
fn a_constant_slice_may_be_returned_from_a_function() {
    if !backend_available() {
        return;
    }
    // The view is `Static` (rodata-backed), so returning it out of a function is legal — the string
    // literal precedent, applied to arrays.
    let src = concat!(
        "TABLE := [11, 22, 33]\n",
        "fn table() -> slice<i64> = TABLE\n",
        "fn main() -> i32 { return table().sum() as i32 }\n",
    );
    let out = build_and_run("agg-return", src);
    assert_eq!(out.status.code(), Some(66));
}

// --- run-parity ----------------------------------------------------------------------------------

#[test]
fn a_lookup_table_program_prints_hand_computed_values() {
    if !backend_available() {
        return;
    }
    // Index + pipeline + str-table lookup together, against hand-computed results.
    let src = concat!(
        "fn big(x: i64) -> bool = x > 8\n",
        "SQUARES := [0, 1, 4, 9, 16, 25]\n",
        "NAMES := [\"zero\", \"one\", \"two\"]\n",
        "fn sq(i: i64) -> i64 = SQUARES[i]\n",
        "fn name_len(i: i64) -> i64 = NAMES[i].len()\n",
        "fn main() -> i32 {\n",
        "  s := sq(5)\n",                    // 25
        "  n := name_len(2)\n",              // len("two") = 3
        "  p := SQUARES.where(big).sum()\n", // 9 + 16 + 25 = 50
        "  return (s + n + p) as i32\n",     // 25 + 3 + 50 = 78
        "}\n",
    );
    let out = build_and_run("agg-parity", src);
    assert_eq!(out.status.code(), Some(78));
}

// --- negative diagnostics ------------------------------------------------------------------------

#[test]
fn an_array_annotation_is_rejected_with_guidance() {
    let d = check_diagnostics("agg-array-ann", "TABLE: array<i64> := [1, 2, 3]\nfn main() -> i32 = 0\n");
    assert!(
        d.contains("static slice<T> view") && d.contains("write `slice<T>`"),
        "expected the slice<T> guidance, got:\n{d}"
    );
}

#[test]
fn a_function_call_in_an_element_is_rejected() {
    assert!(check_errs(
        "agg-fn-call",
        "fn f() -> i64 = 1\nTABLE := [f(), 2, 3]\nfn main() -> i32 = 0\n",
    ));
}

#[test]
fn an_as_cast_in_an_element_is_rejected() {
    assert!(check_errs(
        "agg-as-cast",
        "TABLE := [1 as i64, 2, 3]\nfn main() -> i32 = 0\n",
    ));
}

#[test]
fn referencing_another_aggregate_constant_is_rejected() {
    let d = check_diagnostics(
        "agg-ref",
        "A := [1, 2, 3]\nB := [A, A]\nfn main() -> i32 = 0\n",
    );
    assert!(d.contains("cannot be referenced from another constant's initializer"), "got:\n{d}");
}

#[test]
fn aliasing_another_aggregate_constant_is_rejected() {
    assert!(check_errs(
        "agg-alias",
        "A := [1, 2, 3]\nB := A\nfn main() -> i32 = 0\n",
    ));
}

#[test]
fn a_nested_array_is_rejected() {
    let d = check_diagnostics(
        "agg-nested",
        "TABLE := [[1, 2], [3, 4]]\nfn main() -> i32 = 0\n",
    );
    assert!(d.contains("nested arrays are not supported"), "got:\n{d}");
}

#[test]
fn a_top_level_mut_array_is_rejected() {
    assert!(check_errs("agg-mut", "mut T := [1, 2, 3]\nfn main() -> i32 = 0\n"));
}

#[test]
fn a_cyclic_aggregate_constant_is_rejected() {
    // `N`'s element references a scalar constant that is defined in terms of the aggregate's length —
    // a self-referential cycle through the element expression.
    assert!(check_errs(
        "agg-cycle",
        "K := TABLE\nTABLE := [K]\nfn main() -> i32 = 0\n",
    ));
}

#[test]
fn an_element_type_mismatch_is_rejected() {
    // A `slice<i64>` annotation with a float element — no implicit coercion.
    assert!(check_errs(
        "agg-elem-mismatch",
        "TABLE: slice<i64> := [1, 2.0, 3]\nfn main() -> i32 = 0\n",
    ));
}

#[test]
fn a_heterogeneous_array_is_rejected() {
    // Elements must share one scalar type; the first fixes it, later ones must match.
    assert!(check_errs(
        "agg-hetero",
        "TABLE := [1, true, 3]\nfn main() -> i32 = 0\n",
    ));
}

#[test]
fn a_struct_element_type_is_rejected() {
    // Struct elements / constants are a recorded S1.5 deferral; a `slice<Struct>` annotation (which
    // would also expose a private element type on a `pub` const) is rejected at the type gate.
    let d = check_diagnostics(
        "agg-struct-elem",
        "P { x: i64 }\nTABLE: slice<P> := []\nfn main() -> i32 = 0\n",
    );
    assert!(d.contains("element type must be a scalar or `str`"), "got:\n{d}");
}

#[test]
fn an_out_of_range_constant_index_is_rejected() {
    assert!(check_errs(
        "agg-oob",
        "TABLE := [1, 2, 3]\nfn main() -> i32 { return TABLE[5] as i32 }\n",
    ));
}

#[test]
fn an_out_of_range_element_under_an_annotation_is_rejected() {
    // A `300` element under a `slice<u8>` annotation would silently truncate in rodata; the same
    // literal-range check as a scalar constant applies to each element at the use site.
    assert!(check_errs(
        "agg-elem-range",
        "TABLE: slice<u8> := [1, 300, 3]\nfn at(i: i64) -> u8 = TABLE[i]\nfn main() -> i32 { return at(0) as i32 }\n",
    ));
}
