//! Top-level named constants — `NAME := expr` / `NAME: Type := expr` (`draft.md` §3). A constant is
//! keyword-less and immutable (`mut` is rejected), **evaluated at compile time** to a scalar / string,
//! aggregate slice, or the exact raw-null sentinel and substituted at every use. Constants are
//! per-module namespaced like functions/types: `pub` exports one, and an importer names it qualified
//! (`mod.NAME`). A constant initializer may be a literal, the exact `raw.null()` sentinel, a
//! unary/binary expression, or a reference to another constant in the same module.

mod common;
use common::*;

#[test]
fn a_bare_integer_constant_defaults_to_i64() {
    if !backend_available() {
        return;
    }
    // A constant has a definition-fixed type (unlike a local, it does not infer from its use site —
    // it must be stable across modules), so an unannotated integer defaults to i64.
    let src = "ANSWER := 42\nfn main() -> i32 { return ANSWER as i32 }\n";
    let out = build_and_run("const-bare", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn an_annotated_constant_takes_its_type() {
    if !backend_available() {
        return;
    }
    // The annotation fixes the type; the i32-returning `main` accepts it with no coercion.
    let src = "LIMIT: i32 := 40\nfn main() -> i32 { return LIMIT }\n";
    let out = build_and_run("const-annotated", src);
    assert_eq!(out.status.code(), Some(40));
}

#[test]
fn a_constant_expression_is_folded() {
    if !backend_available() {
        return;
    }
    // Arithmetic + a reference to another constant, all folded at compile time.
    let src = concat!(
        "WIDTH: i32 := 6\n",
        "HEIGHT: i32 := 7\n",
        "AREA: i32 := WIDTH * HEIGHT\n",
        "fn main() -> i32 { return AREA }\n",
    );
    let out = build_and_run("const-expr", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn constants_resolve_regardless_of_declaration_order() {
    if !backend_available() {
        return;
    }
    // `A` references `B` declared *below* it — evaluation is order-independent (memoized).
    let src = concat!(
        "A: i32 := B + 2\n",
        "B: i32 := 40\n",
        "fn main() -> i32 { return A }\n",
    );
    let out = build_and_run("const-order", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn a_bool_constant_drives_a_branch() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "ENABLED := true\n",
        "fn main() -> i32 {\n",
        "  if ENABLED { return 7 }\n",
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("const-bool", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn a_negative_constant_folds() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "OFFSET: i32 := 0 - 50\n",
        "fn main() -> i32 { return OFFSET + 92 }\n",
    );
    let out = build_and_run("const-neg", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn a_pub_constant_is_used_qualified_across_modules() {
    if !backend_available() {
        return;
    }
    let cfg = "module cfg\npub BASE: i32 := 30\npub STEP: i32 := 6\n";
    let main = concat!(
        "import cfg\n",
        "fn main() -> i32 { return cfg.BASE + cfg.STEP * 2 }\n",
    );
    let out = build_and_run_multi("const-xmod", &[("cfg.align", cfg), ("main.align", main)], "main.align");
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn a_private_constant_is_not_exportable() {
    let cfg = "module cfg\nSECRET: i32 := 99\n";
    let main = "import cfg\nfn main() -> i32 { return cfg.SECRET }\n";
    assert!(check_multi_errs("const-priv", &[("cfg.align", cfg), ("main.align", main)], "main.align"));
}

#[test]
fn the_same_constant_name_in_two_modules_does_not_collide() {
    if !backend_available() {
        return;
    }
    // Each module has its own `MAX`; the entry's bare `MAX` and the import's `cfg.MAX` are distinct.
    let cfg = "module cfg\npub MAX: i32 := 2\n";
    let main = concat!(
        "import cfg\n",
        "MAX: i32 := 40\n",
        "fn main() -> i32 { return MAX + cfg.MAX }\n",
    );
    let out = build_and_run_multi("const-namespace", &[("cfg.align", cfg), ("main.align", main)], "main.align");
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn raw_null_constants_are_exact_locally_and_across_units() {
    if !backend_available() {
        return;
    }
    let cfg = concat!(
        "module cfg\n",
        "pub NULL: raw := raw.null()\n",
        "pub INFERRED := raw.null()\n",
        "pub ALIAS: raw := NULL\n",
    );
    let main = concat!(
        "import cfg\n",
        "LOCAL: raw := raw.null()\n",
        "fn is_null(p: raw) -> bool { unsafe { return p.is_null() } }\n",
        "fn main() -> i32 {\n",
        "  if is_null(LOCAL) && is_null(cfg.NULL) && is_null(cfg.INFERRED) && is_null(cfg.ALIAS) { return 42 }\n",
        "  return 1\n",
        "}\n",
    );
    let files = [("cfg.align", cfg), ("main.align", main)];
    let whole = build_and_run_multi("const-raw-null-whole", &files, "main.align");
    assert_eq!(whole.status.code(), Some(42));
    let per_unit = build_per_unit_multi("const-raw-null-unit", &files, "main.align").link_and_run();
    assert_eq!(per_unit.status.code(), Some(42));
}

#[test]
fn raw_null_constant_substitution_is_a_direct_llvm_null() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "NULL: raw := raw.null()\n",
        "pub fn probe() -> raw = NULL\n",
        "fn main() -> i32 { unsafe { if probe().is_null() { return 0 }; return 1 } }\n",
    );
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "raw-null-ir", src);
    assert!(!checked.diags.has_errors());
    assert_eq!(
        checked.hir.fns.iter().map(|function| function.name.as_str()).collect::<Vec<_>>(),
        ["probe", "main"]
    );
    let mir = lower_to_mir(&checked.hir);
    assert_eq!(mir.fns.len(), 2, "both raw-null consumer functions must reach MIR");
    let mut malformed = checked.hir.clone();
    let probe = malformed.fns.iter_mut().find(|function| function.name == "probe").expect("probe HIR");
    let value = probe.body.value.as_deref_mut().expect("probe expression body");
    value.ty = align_sema::Ty::Int(align_sema::IntTy { bits: 64, signed: true });
    assert!(
        align_driver::try_lower_to_mir(&malformed).is_err(),
        "checked HIR must reject a RawNull leaf forged with a non-raw result type"
    );
    for ir in [emit_llvm_with_exports(src, &["probe"]), emit_llvm_optimized(src, &["probe"])] {
        assert!(ir.contains("ret ptr null"), "raw-null constant must lower to an LLVM null pointer:\n{ir}");
        assert!(!ir.contains("call ptr @align_rt_alloc"), "raw-null constant must not allocate:\n{ir}");
        assert!(!ir.contains("raw_null"), "raw-null constant must not call a constructor:\n{ir}");
        assert!(!ir.contains("@llvm.global_ctors"), "raw-null constant must not create a module initializer:\n{ir}");
    }
}

#[test]
fn raw_null_constant_keeps_the_initializer_grammar_closed() {
    let rejected = [
        ("wrong annotation", "BAD: i64 := raw.null()\n"),
        ("argument", "BAD: raw := raw.null(1)\n"),
        ("allocator", "BAD: raw := raw.alloc(8)\n"),
        ("offset", "BAD: raw := raw.offset(raw.null(), 1)\n"),
        ("unsafe block", "BAD: raw := unsafe { raw.null() }\n"),
        ("user call", "fn make() -> raw { unsafe { return raw.null() } }\nBAD: raw := make()\n"),
        ("ordinary expression", "fn main() -> raw = raw.null()\n"),
    ];
    for (name, declaration) in rejected {
        let src = format!("{declaration}fn main() -> i32 = 0\n");
        assert!(check_errs(&format!("const-raw-null-{name}"), &src), "{name} must reject");
    }
}

#[test]
fn raw_null_is_not_an_aggregate_constant_element() {
    let rejected = [
        "TABLE := [raw.null()]\n",
        "NULL: raw := raw.null()\nTABLE := [NULL]\n",
        "NULL := raw.null()\nTABLE := [NULL, NULL]\n",
        "TABLE: slice<raw> := [raw.null()]\n",
        "NULL := raw.null()\nTABLE := [NULL, 1]\n",
    ];
    for (index, declaration) in rejected.into_iter().enumerate() {
        let src = format!("{declaration}fn main() -> i32 = 0\n");
        assert!(check_errs(&format!("const-raw-null-array-{index}"), &src));
    }
}

#[test]
fn a_top_level_mut_is_rejected() {
    assert!(check_errs("const-mut", "mut X := 1\nfn main() -> i32 { return 0 }\n"));
}

#[test]
fn a_type_mismatch_against_the_annotation_is_an_error() {
    // 3.0 is a float; the i32 annotation rejects it (no implicit coercion).
    assert!(check_errs("const-mismatch", "X: i32 := 3.0\nfn main() -> i32 { return 0 }\n"));
}

#[test]
fn using_a_constant_at_the_wrong_type_is_an_error() {
    // `K` is i64 (unconstrained default); returning it where i32 is expected is a type error.
    assert!(check_errs(
        "const-wrong-use",
        "K := 5\nfn main() -> i32 { return K }\n",
    ));
}

#[test]
fn division_by_zero_in_a_constant_is_an_error() {
    assert!(check_errs("const-div0", "X := 1 / 0\nfn main() -> i32 { return X }\n"));
}

#[test]
fn a_cyclic_constant_is_an_error() {
    assert!(check_errs(
        "const-cycle",
        "A := B\nB := A\nfn main() -> i32 { return 0 }\n",
    ));
}

#[test]
fn a_reference_to_a_failed_constant_does_not_panic() {
    // `A` fails to fold (division by zero); `B := -A` references it. Folding `B` must not panic
    // (the failed `A` resolves to an error sentinel) and must report only the root cause.
    assert!(check_errs(
        "const-fail-ref",
        "A := 1 / 0\nB := -A\nfn main() -> i32 { return B as i32 }\n",
    ));
}

#[test]
fn a_function_and_a_constant_may_not_share_a_name() {
    assert!(check_errs(
        "const-fn-clash",
        "DUP := 1\nfn DUP() -> i32 { return 0 }\nfn main() -> i32 { return 0 }\n",
    ));
}
