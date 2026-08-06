//! F-B region materialization: named arena capabilities and their first explicit consumers.

mod common;
use common::*;

fn code(out: &std::process::Output) -> Option<i32> {
    out.status.code()
}

#[test]
fn named_arena_passes_exact_region_capability_to_an_ordinary_function() {
    if !backend_available() {
        return;
    }
    let src = "fn use_region(out: region, value: i64) -> i64 = value\nfn main() -> i32 {\n  arena out {\n    return use_region(out, 37) as i32\n  }\n}\n";
    let out = build_and_run("fb-region-param", src);
    assert_eq!(code(&out), Some(37), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn named_region_binding_does_not_escape_its_arena_scope() {
    let src = "fn use_region(out: region) -> i64 = 1\nfn main() -> i32 {\n  arena out {\n    print(use_region(out))\n  }\n  return use_region(out) as i32\n}\n";
    let diags = check_diagnostics("fb-region-scope", src);
    assert!(diags.contains("unknown name 'out'"), "expected scope diagnostic, got:\n{diags}");
}

#[test]
fn region_capability_cannot_cross_a_spawn_boundary() {
    let src = "fn main() -> Result<(), Error> {\n  arena out {\n    task_group {\n      task := spawn(fn { \"task\".clone_in(out).len() })\n      wait()\n      print(task.get())\n    }\n  }\n  return Ok(())\n}\n";
    let diags = check_diagnostics("fb-region-spawn", src);
    assert!(
        diags.contains("cannot capture a region capability"),
        "expected non-Send region diagnostic, got:\n{diags}"
    );
}

#[test]
fn named_and_anonymous_arenas_keep_the_same_return_and_question_cleanup() {
    if !backend_available() {
        return;
    }
    let named = emit_llvm(
        "fn ready() -> Result<i64, Error> = Ok(5)\nfn early() -> i64 { arena out { return 7 } }\nfn fallible() -> Result<i64, Error> { arena out { value := ready()?\n  return Ok(value) } }\nfn main() -> i32 = early() as i32\n",
    );
    let anonymous = emit_llvm(
        "fn ready() -> Result<i64, Error> = Ok(5)\nfn early() -> i64 { arena { return 7 } }\nfn fallible() -> Result<i64, Error> { arena { value := ready()?\n  return Ok(value) } }\nfn main() -> i32 = early() as i32\n",
    );
    for symbol in ["align_rt_arena_begin", "align_rt_arena_end"] {
        let needle = format!("call {}", if symbol.ends_with("begin") { "ptr" } else { "void" });
        let named_calls = named
            .lines()
            .filter(|line| line.contains(&needle) && line.contains(&format!("@{symbol}(")))
            .count();
        let anonymous_calls = anonymous
            .lines()
            .filter(|line| line.contains(&needle) && line.contains(&format!("@{symbol}(")))
            .count();
        assert_eq!(named_calls, anonymous_calls, "named arena changed {symbol} cleanup shape");
        assert!(named_calls >= 2, "return and ? paths did not both lower {symbol}");
    }
}

#[test]
fn region_capability_cannot_be_returned() {
    let src = "fn leak(out: region) -> region = out\nfn main() -> i32 = 0\n";
    let diags = check_diagnostics("fb-region-return", src);
    assert!(
        diags.contains("region capability cannot be returned"),
        "expected return restriction, got:\n{diags}"
    );
}

#[test]
fn clone_in_uses_the_region_passed_through_an_ordinary_function() {
    if !backend_available() {
        return;
    }
    let src = include_str!("fixtures/fb_region_clone.align");
    let out = build_and_run("fb-clone-in", src);
    assert_eq!(code(&out), Some(11), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "region-copy\n");
}

#[test]
fn clone_in_llvm_uses_the_explicit_region_and_guards_the_native_length() {
    if !backend_available() {
        return;
    }
    let llvm = emit_llvm(
        "fn copy_text(out: region, value: str) -> str = value.clone_in(out)\nfn main() -> i32 { arena out { return copy_text(out, \"copy\").len() as i32 } }\n",
    );
    assert!(llvm.contains("call ptr @align_rt_arena_alloc("), "clone_in skipped the explicit arena:\n{llvm}");
    assert!(llvm.contains("icmp slt i64"), "clone_in did not reject a negative native length:\n{llvm}");
    assert!(llvm.contains("call void @align_rt_alloc_size_fail("), "clone_in omitted the allocation-size failure edge:\n{llvm}");
    assert!(!llvm.contains("call { ptr, i64 } @align_rt_str_clone("), "clone_in selected heap clone:\n{llvm}");
    assert!(!llvm.contains("call ptr @align_rt_alloc("), "clone_in called the heap allocator:\n{llvm}");
}

#[test]
fn explicit_region_operands_survive_hir_to_mir() {
    let src = "fn copy_text(out: region, value: str) -> str = value.clone_in(out)\nfn main() -> i32 {\n  arena out {\n    copied := copy_text(out, \"copy\")\n    mut values: array_builder<str> := array_builder(out)\n    values.push(copied)\n    return values.build().len() as i32\n  }\n}\n";
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "fb-region-mir", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected diagnostics:\n{}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    let mir = align_mir::print::program_to_string(&lower_to_mir(&checked.hir));
    assert!(mir.contains("clone_in("), "clone_in lost its MIR operation:\n{mir}");
    assert!(
        mir.contains("array_builder_new(elem=Str, region=%"),
        "region builder lost its exact handle operand:\n{mir}"
    );
    assert!(
        !mir.contains("array_builder_new(elem=Str, region=heap)"),
        "region builder changed allocation mode:\n{mir}"
    );
}

#[test]
fn clone_in_result_cannot_escape_the_selected_arena() {
    let src = "fn copy_text(out: region, value: str) -> str = value.clone_in(out)\nfn leak() -> str {\n  arena out {\n    return copy_text(out, \"nope\")\n  }\n}\nfn main() -> i32 = 0\n";
    let diags = check_diagnostics("fb-clone-in-escape", src);
    assert!(diags.contains("arena"), "expected arena escape diagnostic, got:\n{diags}");
}

#[test]
fn clone_in_copies_bytes_and_preserves_the_selected_region_identity() {
    let bytes = "fn copy_bytes(out: region, value: slice<u8>) -> slice<u8> = value.clone_in(out)\nfn main() -> i32 {\n  arena out {\n    copied := copy_bytes(out, \"abc\".bytes())\n    return (copied[0] + copied.len()) as i32\n  }\n}\n";
    let diags = check_diagnostics("fb-clone-in-bytes", bytes);
    assert!(diags.is_empty(), "unexpected byte clone diagnostics:\n{diags}");

    let wrong_region = "fn copy_text(out: region, value: str) -> str = value.clone_in(out)\nfn main() -> i32 {\n  arena outer {\n    mut values: array_builder<str> := array_builder(outer)\n    arena inner {\n      temporary := copy_text(inner, \"temporary\")\n      values.push(temporary)\n    }\n    return 0\n  }\n}\n";
    let diags = check_diagnostics("fb-clone-in-wrong-region", wrong_region);
    assert!(
        diags.contains("clone_in(outer)") || diags.contains("outlive"),
        "expected exact-region builder-store diagnostic, got:\n{diags}"
    );

    let outer_clone = "fn copy_text(out: region, value: str) -> str = value.clone_in(out)\nfn main() -> i32 {\n  arena outer {\n    mut values: array_builder<str> := array_builder(outer)\n    arena inner {\n      retained := copy_text(outer, \"retained\")\n      values.push(retained)\n    }\n    return values.build().len() as i32\n  }\n}\n";
    let diags = check_diagnostics("fb-clone-in-outer-region", outer_clone);
    assert!(diags.is_empty(), "outer-region clone should remain valid:\n{diags}");
}

#[test]
fn clone_in_recursively_copies_region_plain_struct_views() {
    let src = "Record { id: i64, name: str, alias: Option<str>, data: slice<u8> }\nfn copy_record(out: region, value: Record) -> Record = value.clone_in(out)\nfn main() -> i32 {\n  arena out {\n    copied := copy_record(out, Record { id: 20, name: \"plain\", alias: Some(\"view\"), data: \"bc\".bytes() })\n    alias := copied.alias else \"\"\n    return (copied.id + copied.name.len() + alias.len() + copied.data[0]) as i32\n  }\n}\n";
    let diags = check_diagnostics("fb-clone-in-plain-struct", src);
    assert!(diags.is_empty(), "unexpected plain-struct clone diagnostics:\n{diags}");
    if !backend_available() {
        return;
    }
    let llvm = emit_llvm(src);
    assert_eq!(
        llvm.matches("call ptr @align_rt_arena_alloc(").count(),
        3,
        "each view-bearing field must receive one explicit region allocation:\n{llvm}",
    );
    let out = build_and_run("fb-clone-in-plain-struct", src);
    assert_eq!(code(&out), Some(127), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn clone_in_rejects_structs_with_independently_owned_fields() {
    let src = "Owned { name: string }\nfn copy_owned(out: region, value: Owned) -> Owned = value.clone_in(out)\nfn main() -> i32 = 0\n";
    let diags = check_diagnostics("fb-clone-in-owned-struct", src);
    assert!(
        diags.contains("field 'name' owns independent heap storage"),
        "expected recursive clone_in ownership diagnostic, got:\n{diags}",
    );
}

#[test]
fn captured_region_identity_survives_indirect_closure_calls() {
    let valid = "fn main() -> i32 {\n  arena out {\n    copy := fn value: str { value.clone_in(out) }\n    copied := copy(\"captured\")\n    return copied.len() as i32\n  }\n}\n";
    let diags = check_diagnostics("fb-region-captured-closure", valid);
    assert!(diags.is_empty(), "captured region should remain callable:\n{diags}");

    let invalid = "fn main() -> i32 {\n  arena outer {\n    mut values: array_builder<str> := array_builder(outer)\n    arena inner {\n      copy := fn value: str { value.clone_in(inner) }\n      temporary := copy(\"captured\")\n      values.push(temporary)\n    }\n    return 0\n  }\n}\n";
    let diags = check_diagnostics("fb-region-captured-closure-wrong-region", invalid);
    assert!(
        diags.contains("clone_in(outer)") || diags.contains("shorter-lived"),
        "expected captured-region identity rejection, got:\n{diags}"
    );
}

#[test]
fn region_builder_grows_across_chunks_and_compacts_to_one_array() {
    if !backend_available() {
        return;
    }
    let src = include_str!("fixtures/fb_region_builder.align");
    let out = build_and_run("fb-region-builder", src);
    // sum(0..17) + len = 136 + 17.
    assert_eq!(code(&out), Some(153), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn heap_builder_rejects_region_only_view_elements() {
    let src = "fn main() -> i32 {\n  mut values: array_builder<str> := array_builder()\n  return 0\n}\n";
    let diags = check_diagnostics("fb-region-builder-heap-view", src);
    assert!(diags.contains("use `array_builder(out)`"), "expected allocation-mode diagnostic, got:\n{diags}");
}

#[test]
fn region_builder_rejects_independently_owned_fields() {
    let src = "Owned { name: string }\nfn main() -> i32 {\n  arena out {\n    mut values: array_builder<Owned> := array_builder(out)\n    return 0\n  }\n}\n";
    let diags = check_diagnostics("fb-region-builder-owned", src);
    assert!(diags.contains("field 'name' owns independent heap storage"), "expected RegionPlain field diagnostic, got:\n{diags}");
}

#[test]
fn region_plain_bytes_excludes_signed_byte_slices() {
    let src = "fn main() -> i32 {\n  arena out {\n    mut values: array_builder<slice<i8>> := array_builder(out)\n    return 0\n  }\n}\n";
    let diags = check_diagnostics("fb-region-builder-signed-bytes", src);
    assert!(
        diags.contains("cannot use region storage") || diags.contains("unsupported type"),
        "expected RegionPlain byte-view diagnostic, got:\n{diags}"
    );
}

#[test]
fn region_builder_materializes_option_and_plain_struct_elements() {
    if !backend_available() {
        return;
    }
    let src = include_str!("fixtures/fb_region_builder_aggregate.align");
    let out = build_and_run("fb-region-builder-aggregate", src);
    assert_eq!(code(&out), Some(47), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn region_builder_appends_a_fixed_array_without_heap_storage() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  arena out {\n    source := [4, 8, 15, 16, 23, 42]\n    mut values: array_builder<i64> := array_builder(out)\n    values.append(source[..])\n    built := values.build()\n    return (built.sum() + built.len()) as i32\n  }\n}\n";
    let out = build_and_run("fb-region-builder-fixed-array", src);
    assert_eq!(code(&out), Some(114), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn borrow_mut_builder_helper_cannot_consume_the_callers_builder() {
    let src = "fn finish(borrow mut values: array_builder<i64>) -> array<i64> = values.build()\nfn main() -> i32 = 0\n";
    let diags = check_diagnostics("fb-region-builder-borrow-build", src);
    assert!(
        diags.contains("cannot move") || diags.contains("borrow"),
        "expected borrowed-builder consumption diagnostic, got:\n{diags}"
    );
}

#[test]
fn region_builder_cannot_be_returned_from_its_helper() {
    let src = "fn leak(out: region) -> array_builder<i64> {\n  mut values: array_builder<i64> := array_builder(out)\n  return values\n}\nfn main() -> i32 = 0\n";
    let diags = check_diagnostics("fb-region-builder-return", src);
    assert!(
        diags.contains("cannot return a region-backed array_builder"),
        "expected region-builder return diagnostic, got:\n{diags}"
    );
}

#[test]
fn region_builder_crosses_calls_only_as_borrow_mut() {
    let shared = "fn inspect(borrow values: array_builder<i64>) {}\nfn main() -> i32 {\n  arena out {\n    mut values: array_builder<i64> := array_builder(out)\n    inspect(values)\n    return 0\n  }\n}\n";
    let diags = check_diagnostics("fb-region-builder-shared-call", shared);
    assert!(
        diags.contains("may be passed only as `borrow mut`"),
        "expected shared-region-builder rejection, got:\n{diags}"
    );

    let by_value = shared.replace("borrow values", "values");
    let diags = check_diagnostics("fb-region-builder-value-call", &by_value);
    assert!(
        diags.contains("arena-owned value cannot be moved into a function call"),
        "expected by-value region-builder rejection, got:\n{diags}"
    );

    let heap = "fn inspect(borrow values: array_builder<i64>) {}\nfn main() -> i32 {\n  mut values: array_builder<i64> := array_builder()\n  inspect(values)\n  return 0\n}\n";
    let diags = check_diagnostics("fb-heap-builder-shared-call", heap);
    assert!(diags.is_empty(), "heap builder borrow should remain legal:\n{diags}");

    let uncertain = "fn inspect(borrow values: array_builder<i64>) {}\nfn relay(borrow mut values: array_builder<i64>) { inspect(values) }\nfn main() -> i32 = 0\n";
    let diags = check_diagnostics("fb-incoming-builder-shared-call", uncertain);
    assert!(
        diags.contains("may be passed only as `borrow mut`"),
        "a possibly region-backed incoming builder must fail closed, got:\n{diags}"
    );

    let heap_only = "fn inspect(borrow values: array_builder<string>) {}\nfn relay(borrow mut values: array_builder<string>) { inspect(values) }\nfn main() -> i32 = 0\n";
    let diags = check_diagnostics("fb-incoming-heap-builder-shared-call", heap_only);
    assert!(
        diags.is_empty(),
        "a heap-only incoming builder should remain borrowable:\n{diags}"
    );
}

#[test]
fn borrow_mut_builder_helper_preserves_new_view_roots_in_the_caller() {
    let valid = "fn push_view(borrow mut values: array_builder<str>, value: str) { values.push(value) }\nfn main() -> i32 {\n  arena out {\n    mut values: array_builder<str> := array_builder(out)\n    push_view(values, \"static\")\n    return values.build().len() as i32\n  }\n}\n";
    let diags = check_diagnostics("fb-region-builder-helper-view", valid);
    assert!(diags.is_empty(), "view helper should type-check:\n{diags}");

    let invalid = "fn copy_text(out: region, value: str) -> str = value.clone_in(out)\nfn push_view(borrow mut values: array_builder<str>, value: str) { values.push(value) }\nfn main() -> i32 {\n  arena outer {\n    mut values: array_builder<str> := array_builder(outer)\n    arena inner {\n      temporary := copy_text(inner, \"short\")\n      push_view(values, temporary)\n    }\n    return values.build().len() as i32\n  }\n}\n";
    let diags = check_diagnostics("fb-region-builder-helper-wrong-region", invalid);
    assert!(
        diags.contains("invalidated") || diags.contains("dropped") || diags.contains("outlive"),
        "expected cross-call retained-view diagnostic, got:\n{diags}"
    );

    let invalidated_owner = "fn push_view(borrow mut values: array_builder<str>, value: str) { values.push(value) }\nfn main() -> i32 {\n  arena out {\n    mut owner := \"first\".clone()\n    view: str := owner\n    mut values: array_builder<str> := array_builder(out)\n    push_view(values, view)\n    owner = \"second\".clone()\n    return values.build().len() as i32\n  }\n}\n";
    let diags = check_diagnostics(
        "fb-region-builder-helper-owner-invalidation",
        invalidated_owner,
    );
    assert!(
        diags.contains("invalidated") || diags.contains("borrow") || diags.contains("dependent"),
        "expected retained owner-generation diagnostic, got:\n{diags}"
    );
}

#[test]
fn borrowed_region_builders_reject_callee_local_views() {
    let incoming = "fn retain_local(borrow mut values: array_builder<str>) {\n  arena inner {\n    temporary := \"short\".clone_in(inner)\n    values.push(temporary)\n  }\n}\nfn main() -> i32 {\n  arena outer {\n    mut values: array_builder<str> := array_builder(outer)\n    retain_local(values)\n    return 0\n  }\n}\n";
    let diags = check_diagnostics("fb-region-builder-callee-local", incoming);
    assert!(
        diags.contains("shorter-lived view") || diags.contains("outlive"),
        "expected callee-local retained-view diagnostic, got:\n{diags}"
    );

    let constructed = "fn materialize(out: region) -> i64 {\n  mut values: array_builder<str> := array_builder(out)\n  arena inner {\n    temporary := \"short\".clone_in(inner)\n    values.push(temporary)\n  }\n  return values.build().len()\n}\nfn main() -> i32 {\n  arena out { return materialize(out) as i32 }\n}\n";
    let diags = check_diagnostics("fb-region-builder-param-local", constructed);
    assert!(
        diags.contains("shorter-lived view") || diags.contains("outlive"),
        "expected parameter-region builder diagnostic, got:\n{diags}"
    );
}

#[test]
fn helper_region_parameter_is_checked_against_the_concrete_builder_region() {
    let helper = "fn retain_copy(out: region, borrow mut values: array_builder<str>) {\n  values.push(\"copy\".clone_in(out))\n}\n";
    let valid = format!(
        "{helper}fn main() -> i32 {{\n  arena outer {{\n    mut values: array_builder<str> := array_builder(outer)\n    retain_copy(outer, values)\n    return values.build().len() as i32\n  }}\n}}\n"
    );
    let diags = check_diagnostics("fb-region-builder-param-same", &valid);
    assert!(diags.is_empty(), "same caller region should remain valid:\n{diags}");

    let invalid = format!(
        "{helper}fn main() -> i32 {{\n  arena outer {{\n    mut values: array_builder<str> := array_builder(outer)\n    arena inner {{ retain_copy(inner, values) }}\n    return 0\n  }}\n}}\n"
    );
    let diags = check_diagnostics("fb-region-builder-param-different", &invalid);
    assert!(
        diags.contains("shorter-lived view") || diags.contains("outlive"),
        "expected concrete call-site region diagnostic, got:\n{diags}"
    );
}

#[test]
fn region_builder_llvm_uses_only_explicit_arena_growth_and_generic_copy() {
    if !backend_available() {
        return;
    }
    let src = include_str!("fixtures/fb_region_builder.align");
    let llvm = emit_llvm(src);
    assert!(llvm.contains("call ptr @align_rt_array_builder_new_in("), "missing region constructor:\n{llvm}");
    assert!(llvm.contains("call { ptr, i64 } @align_rt_array_builder_build("), "missing compacting build:\n{llvm}");
    assert!(!llvm.contains("call ptr @align_rt_array_builder_new(i64"), "region form selected heap constructor:\n{llvm}");
    assert!(!llvm.contains("call ptr @align_rt_array_builder_init_stack("), "region form selected heap stack header:\n{llvm}");
    assert!(!llvm.contains("call ptr @align_rt_alloc("), "region form called the heap allocator:\n{llvm}");
    assert!(!llvm.contains("call ptr @align_rt_realloc("), "region form called the heap reallocator:\n{llvm}");
}

#[test]
fn region_plain_aggregate_push_uses_target_layout_copy() {
    if !backend_available() {
        return;
    }
    let llvm = emit_llvm(include_str!("fixtures/fb_region_builder_aggregate.align"));
    assert!(
        llvm.contains("call void @align_rt_array_builder_push_bytes("),
        "aggregate push did not use the exact-layout copy seam:\n{llvm}"
    );
    assert!(!llvm.contains("call ptr @align_rt_alloc("), "aggregate region builder used heap allocation:\n{llvm}");
    assert!(!llvm.contains("call ptr @align_rt_realloc("), "aggregate region builder used heap reallocation:\n{llvm}");
}

#[test]
fn region_parameters_and_builder_helpers_match_whole_and_per_unit_compilation() {
    let lib = "module regionlib\npub fn copy_text(out: region, value: str) -> str = value.clone_in(out)\npub fn push_value(borrow mut values: array_builder<i64>, value: i64) { values.push(value) }\npub fn build_value(out: region, value: i64) -> array<i64> {\n  mut values: array_builder<i64> := array_builder(out)\n  push_value(values, value)\n  return values.build()\n}\n";
    let main = "import regionlib\nfn main() -> i32 {\n  arena out {\n    copied := regionlib.copy_text(out, \"unit\")\n    built := regionlib.build_value(out, 38)\n    return (built[0] + copied.len()) as i32\n  }\n}\n";
    let files = [("regionlib.align", lib), ("main.align", main)];
    let checked = assert_same_verdict("fb-region-per-unit-check", &files, "main.align");
    assert!(!checked.diags.has_errors(), "unexpected per-unit diagnostics");
    if !backend_available() {
        return;
    }
    let out = build_per_unit_multi("fb-region-per-unit-run", &files, "main.align").link_and_run();
    assert_eq!(code(&out), Some(42), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn imported_view_helper_keeps_the_same_wrong_region_rejection() {
    let lib = "module regionviews\npub fn push_view(borrow mut values: array_builder<str>, value: str) { values.push(value) }\n";
    let main = "import regionviews\nfn copy_text(out: region, value: str) -> str = value.clone_in(out)\nfn main() -> i32 {\n  arena outer {\n    mut values: array_builder<str> := array_builder(outer)\n    arena inner {\n      temporary := copy_text(inner, \"short\")\n      regionviews.push_view(values, temporary)\n    }\n    return values.build().len() as i32\n  }\n}\n";
    let checked = assert_same_verdict(
        "fb-region-imported-helper-reject",
        &[("regionviews.align", lib), ("main.align", main)],
        "main.align",
    );
    assert!(checked.diags.has_errors(), "imported helper lost wrong-region rejection");
}

#[test]
fn builder_operands_remain_visible_to_effect_inference() {
    let src = "fn noisy() -> i64 { print(1)\n  return 1 }\nfn materialize(value: i64) -> i64 {\n  mut values: array_builder<i64> := array_builder()\n  values.push(noisy())\n  return values.build()[0] + value\n}\nfn main() -> i32 {\n  out := [1, 2].par_map(materialize)\n  return out[0] as i32\n}\n";
    let diags = check_diagnostics("fb-region-builder-effect", src);
    assert!(
        diags.contains("Pure") || diags.contains("Impure"),
        "expected parallel-effect rejection, got:\n{diags}"
    );
}

#[test]
fn invalid_clone_in_region_stops_before_typed_hir_construction() {
    let src = "fn main() -> i32 {\n  copied := \"x\".clone_in(missing)\n  return 0\n}\n";
    let diags = check_diagnostics("fb-clone-in-invalid-region", src);
    assert!(
        diags.contains("unknown name 'missing'"),
        "expected region operand diagnostic, got:\n{diags}"
    );
}
