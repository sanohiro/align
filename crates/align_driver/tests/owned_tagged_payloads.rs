//! L1a: the canonical recursive Drop plan and `Option<string>` struct fields.
//!
//! Runtime cases intentionally exercise ownership transfers and replacement. A stale alias
//! double-frees and aborts; a missing transfer corrupts the observed payload. Allocation/free
//! parity is measured by `bench/owned_tagged_payload`.

mod common;
use common::*;

const DECL: &str = "Item { detail: Option<string>, n: i64 }\n";

fn mir_text(src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "owned-tagged-payload.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    align_mir::print::program_to_string(&lower_to_mir(&checked.hir))
}

#[test]
fn option_string_field_constructs_none_and_some() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Item { detail: Option<string>, n: i64 }\n",
        "fn main() -> i32 {\n",
        "  a := Item { detail: None, n: 3 }\n",
        "  b := Item { detail: Some(\"hello\".clone()), n: 4 }\n",
        "  s := b.detail else { return 90 }\n",
        "  return (a.n + b.n + s.len()) as i32\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("owned-option-field-basic", src).status.code(),
        Some(12)
    );
}

#[test]
fn whole_struct_return_and_pass_transfer_one_owner() {
    if !backend_available() {
        return;
    }
    let src = format!(
        "{DECL}\
         fn make(some: bool) -> Item {{\n\
           if some {{ return Item {{ detail: Some(\"abc\".clone()), n: 5 }} }}\n\
           return Item {{ detail: None, n: 7 }}\n\
         }}\n\
         fn take(v: Item) -> i64 {{\n\
           s := v.detail else {{ return v.n }}\n\
           return v.n + s.len()\n\
         }}\n\
         fn main() -> i32 {{ return (take(make(true)) + take(make(false))) as i32 }}\n"
    );
    assert_eq!(
        build_and_run("owned-option-field-transfer", &src)
            .status
            .code(),
        Some(15)
    );
}

#[test]
fn whole_struct_replacement_covers_all_tag_transitions() {
    if !backend_available() {
        return;
    }
    let src = format!(
        "{DECL}\
         fn main() -> i32 {{\n\
           mut v := Item {{ detail: Some(\"old\".clone()), n: 1 }}\n\
           v = Item {{ detail: Some(\"newer\".clone()), n: 2 }}\n\
           v = Item {{ detail: None, n: 3 }}\n\
           v = Item {{ detail: Some(\"final\".clone()), n: 4 }}\n\
           s := v.detail else {{ return 90 }}\n\
           return (v.n + s.len()) as i32\n\
         }}\n"
    );
    assert_eq!(
        build_and_run("owned-option-field-reassign", &src)
            .status
            .code(),
        Some(9)
    );
}

#[test]
fn field_replacement_covers_all_tag_transitions() {
    if !backend_available() {
        return;
    }
    let src = format!(
        "{DECL}\
         fn main() -> i32 {{\n\
           mut v := Item {{ detail: Some(\"old\".clone()), n: 7 }}\n\
           v.detail = Some(\"newer\".clone())\n\
           v.detail = None\n\
           v.detail = Some(\"final\".clone())\n\
           s := v.detail else {{ return 90 }}\n\
           return (v.n + s.len()) as i32\n\
         }}\n"
    );
    let mir = mir_text(&src);
    assert_eq!(
        mir.matches("drop_value").count(),
        3,
        "each old tagged field must be dropped before replacement:\n{mir}"
    );
    let ir = emit_llvm(&src);
    assert!(
        ir.matches("dropoptissome").count() >= 4,
        "replacement and final struct Drop must all test the tag:\n{ir}"
    );
    assert_eq!(
        build_and_run("owned-option-field-replace", &src)
            .status
            .code(),
        Some(12)
    );
}

#[test]
fn fixed_array_option_string_field_replacement_is_rejected() {
    let cases = [
        (
            "owned-option-fixed-array-field-inline.align",
            concat!(
                "Item { detail: Option<string>, n: i64 }\n",
                "fn main() -> i32 {\n",
                "  mut items := [Item { detail: Some(\"old\".clone()), n: 1 }]\n",
                "  items[0].detail = Some(\"new\".clone())\n",
                "  return 0\n",
                "}\n",
            ),
        ),
        (
            "owned-option-fixed-array-field-bound.align",
            concat!(
                "Item { detail: Option<string>, n: i64 }\n",
                "fn main() -> i32 {\n",
                "  mut items := [Item { detail: Some(\"old\".clone()), n: 1 }]\n",
                "  value := Some(\"new\".clone())\n",
                "  items[0].detail = value\n",
                "  return 0\n",
                "}\n",
            ),
        ),
    ];
    for (name, src) in cases {
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, name, src);
        let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
        assert!(
            checked.diags.has_errors(),
            "tagged fixed-array field replacement must remain rejected: {name}"
        );
        assert!(
            rendered.contains(
                "element-field assignment of Option<string> into a fixed struct array is not supported yet"
            ),
            "diagnostic must name the unsupported ownership shape in {name}:\n{rendered}"
        );
    }
}

#[test]
fn fixed_array_option_string_whole_element_replacement_is_supported() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Item { detail: Option<string>, n: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut items := [Item { detail: Some(\"old\".clone()), n: 1 }]\n",
        "  value := Item { detail: Some(\"newer\".clone()), n: 7 }\n",
        "  items[0] = value\n",
        "  return items[0].n as i32\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("owned-option-fixed-array-whole-element", src)
            .status
            .code(),
        Some(7)
    );
}

#[test]
fn fixed_array_option_string_whole_element_replacement_consumes_source() {
    let src = concat!(
        "Item { detail: Option<string>, n: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut items := [Item { detail: Some(\"old\".clone()), n: 1 }]\n",
        "  value := Item { detail: Some(\"newer\".clone()), n: 7 }\n",
        "  items[0] = value\n",
        "  return value.n as i32\n",
        "}\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "owned-option-fixed-array-whole-element-move.align", src);
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        checked.diags.has_errors(),
        "whole-element replacement must consume a Move-struct source"
    );
    assert!(
        rendered.contains("use of moved value 'value'"),
        "diagnostic must identify the consumed source:\n{rendered}"
    );
}

#[test]
fn field_self_assignment_preserves_value_and_ownership() {
    if !backend_available() {
        return;
    }
    let src = format!(
        "{DECL}\
         fn pass(detail: Option<string>) -> Option<string> = detail\n\
         fn main() -> i32 {{\n\
           mut v := Item {{ detail: Some(\"hello\".clone()), n: 7 }}\n\
           v.detail = v.detail\n\
           v.detail = {{ v.detail }}\n\
           v.detail = pass(v.detail)\n\
           s := v.detail else {{ return 90 }}\n\
           return (v.n + s.len()) as i32\n\
         }}\n"
    );
    let mir = mir_text(&src);
    assert_eq!(
        mir.matches("drop_value").count(),
        2,
        "wrapped transfers must path-locally zero then drop the old destination:\n{mir}"
    );
    assert_eq!(
        build_and_run("owned-option-field-self-assign", &src)
            .status
            .code(),
        Some(12)
    );
}

#[test]
fn conditional_field_replacement_drops_old_only_on_live_paths() {
    if !backend_available() {
        return;
    }
    let src = format!(
        "{DECL}\
         fn main() -> i32 {{\n\
           mut a := Item {{ detail: Some(\"hello\".clone()), n: 0 }}\n\
           a.detail = if true {{ a.detail }} else {{ Some(\"new\".clone()) }}\n\
           mut b := Item {{ detail: Some(\"hello\".clone()), n: 0 }}\n\
           b.detail = if false {{ b.detail }} else {{ Some(\"new\".clone()) }}\n\
           x := a.detail else {{ return 90 }}\n\
           y := b.detail else {{ return 91 }}\n\
           return (x.len() + y.len()) as i32\n\
         }}\n"
    );
    let mir = mir_text(&src);
    assert_eq!(
        mir.matches("drop_value").count(),
        2,
        "each joined replacement must drop its path-local old destination:\n{mir}"
    );
    assert_eq!(
        build_and_run("owned-option-field-conditional-replace", &src)
            .status
            .code(),
        Some(8)
    );
}

#[test]
fn match_and_loop_field_replacement_keep_path_local_ownership() {
    if !backend_available() {
        return;
    }
    let src = format!(
        "{DECL}\
         Choice {{ Reuse, Fresh }}\n\
         fn main() -> i32 {{\n\
           mut a := Item {{ detail: Some(\"hello\".clone()), n: 0 }}\n\
           a.detail = match Choice.Reuse {{\n\
             Reuse => a.detail\n\
             Fresh => Some(\"new\".clone())\n\
           }}\n\
           mut b := Item {{ detail: Some(\"hello\".clone()), n: 0 }}\n\
           b.detail = match Choice.Fresh {{\n\
             Reuse => b.detail\n\
             Fresh => Some(\"new\".clone())\n\
           }}\n\
           mut c := Item {{ detail: Some(\"hello\".clone()), n: 0 }}\n\
           c.detail = loop {{\n\
             if true {{ break c.detail }}\n\
             break Some(\"new\".clone())\n\
           }}\n\
           mut d := Item {{ detail: Some(\"hello\".clone()), n: 0 }}\n\
           d.detail = loop {{\n\
             if false {{ break d.detail }}\n\
             break Some(\"new\".clone())\n\
           }}\n\
           w := a.detail else {{ return 90 }}\n\
           x := b.detail else {{ return 91 }}\n\
           y := c.detail else {{ return 92 }}\n\
           z := d.detail else {{ return 93 }}\n\
           return (w.len() + x.len() + y.len() + z.len()) as i32\n\
         }}\n"
    );
    let mir = mir_text(&src);
    assert_eq!(
        mir.matches("drop_value").count(),
        4,
        "match and loop joins must each drop their path-local old destination:\n{mir}"
    );
    assert_eq!(
        build_and_run("owned-option-field-match-loop-replace", &src)
            .status
            .code(),
        Some(16)
    );
}

#[test]
fn if_match_and_loop_edges_keep_one_live_payload() {
    if !backend_available() {
        return;
    }
    let src = format!(
        "{DECL}\
         fn choose(some: bool) -> Item {{\n\
           v := if some {{ Item {{ detail: Some(\"edge\".clone()), n: 2 }} }} else {{ Item {{ detail: None, n: 3 }} }}\n\
           return v\n\
         }}\n\
         fn main() -> i32 {{\n\
           mut v := choose(false)\n\
           mut i := 0\n\
           loop {{\n\
             if i >= 3 {{ break }}\n\
             v = choose(i == 2)\n\
             i = i + 1\n\
           }}\n\
           n := match v.detail {{ Some(s) => s.len(), None => 90 }}\n\
           return (v.n + n) as i32\n\
         }}\n"
    );
    assert_eq!(
        build_and_run("owned-option-field-edges", &src)
            .status
            .code(),
        Some(6)
    );
}

#[test]
fn matching_an_owned_field_marks_that_field_moved() {
    let src = concat!(
        "Item { detail: Option<string>, n: i64 }\n",
        "fn main() -> i32 {\n",
        "  v := Item { detail: Some(\"owned\".clone()), n: 4 }\n",
        "  n := match v.detail { Some(s) => s.len(), None => 0 }\n",
        "  again := v.detail\n",
        "  return (n + v.n) as i32\n",
        "}\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "matched-owned-field-move.align", src);
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        checked.diags.has_errors(),
        "matching Some(string) must consume the field"
    );
    assert!(
        rendered.contains("use of moved field 'detail' of 'v'"),
        "diagnostic must identify the consumed field:\n{rendered}"
    );
}

#[test]
fn matching_a_block_wrapped_owned_field_marks_that_field_moved() {
    let src = concat!(
        "Item { detail: Option<string>, n: i64 }\n",
        "fn main() -> i32 {\n",
        "  v := Item { detail: Some(\"owned\".clone()), n: 4 }\n",
        "  n := match { v.detail } { Some(s) => s.len(), None => 0 }\n",
        "  again := v.detail\n",
        "  return (n + v.n) as i32\n",
        "}\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "matched-block-owned-field-move.align", src);
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        checked.diags.has_errors(),
        "matching a wrapped Some(string) must consume the source field"
    );
    assert!(
        rendered.contains("use of moved field 'detail' of 'v'"),
        "diagnostic must identify the wrapped consumed field:\n{rendered}"
    );
}

#[test]
fn matching_wrapped_owned_sources_tracks_local_and_conditional_moves() {
    let cases = [
        (
            "matched-block-owned-local-move.align",
            concat!(
                "fn main() -> i32 {\n",
                "  detail := Some(\"owned\".clone())\n",
                "  n := match { detail } { Some(s) => s.len(), None => 0 }\n",
                "  again := detail\n",
                "  return n as i32\n",
                "}\n",
            ),
            "use of moved value 'detail'",
        ),
        (
            "matched-conditional-owned-field-move.align",
            concat!(
                "Item { detail: Option<string>, n: i64 }\n",
                "fn no_detail() -> Option<string> = None\n",
                "fn main() -> i32 {\n",
                "  v := Item { detail: Some(\"owned\".clone()), n: 4 }\n",
                "  n := match if v.n > 0 { v.detail } else { no_detail() } { Some(s) => s.len(), None => 0 }\n",
                "  again := v.detail\n",
                "  return n as i32\n",
                "}\n",
            ),
            "use of moved field 'detail' of 'v'",
        ),
        (
            "matched-scrutinee-evaluation-move.align",
            concat!(
                "Item { detail: Option<string>, n: i64 }\n",
                "fn take(s: string) -> i64 = s.len()\n",
                "fn main() -> i32 {\n",
                "  owned := \"side effect\".clone()\n",
                "  v := Item { detail: None, n: 4 }\n",
                "  n := match { used := take(owned); v.detail } { Some(s) => { return 90 }, None => 0 }\n",
                "  again := owned\n",
                "  return (n + v.n) as i32\n",
                "}\n",
            ),
            "use of moved value 'owned'",
        ),
    ];
    for (name, src, expected) in cases {
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, name, src);
        let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
        assert!(
            checked.diags.has_errors(),
            "matching a wrapped owned source must consume it: {name}"
        );
        assert!(
            rendered.contains(expected),
            "diagnostic must identify the wrapped consumed source in {name}:\n{rendered}"
        );
    }
}

#[test]
fn match_binding_uses_post_evaluation_borrow_roots() {
    let src = concat!(
        "fn take(s: string) -> i64 = s.len()\n",
        "fn main() -> i32 {\n",
        "  first := \"first\".clone()\n",
        "  second := \"second\".clone()\n",
        "  mut view: str := first\n",
        "  n := match { view = second; Some(view) } {\n",
        "    Some(s) => { used := take(second); print(s); s.len() }\n",
        "    None => 0\n",
        "  }\n",
        "  return n as i32\n",
        "}\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "match-post-evaluation-borrow.align", src);
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        checked.diags.has_errors(),
        "the arm binding must borrow the source selected while evaluating the scrutinee"
    );
    assert!(
        rendered.contains("use of invalidated borrow 's': its source 'second' was moved"),
        "diagnostic must name the post-evaluation owner:\n{rendered}"
    );
}

#[test]
fn nested_outer_struct_uses_the_same_recursive_plan() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Inner { detail: Option<string> }\n",
        "Outer { inner: Inner, n: i64 }\n",
        "fn main() -> i32 {\n",
        "  o := Outer { inner: Inner { detail: Some(\"nested\".clone()) }, n: 2 }\n",
        "  s := o.inner.detail else { return 90 }\n",
        "  return (o.n + s.len()) as i32\n",
        "}\n",
    );
    // Deep partial moves remain explicitly deferred in L1a.
    assert!(check_errs("owned-option-field-deep-partial", src));

    let supported = concat!(
        "Inner { detail: Option<string> }\n",
        "Outer { inner: Inner, n: i64 }\n",
        "fn size(o: Outer) -> i64 = o.n\n",
        "fn main() -> i32 {\n",
        "  o := Outer { inner: Inner { detail: Some(\"nested\".clone()) }, n: 8 }\n",
        "  return size(o) as i32\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("owned-option-field-nested", supported)
            .status
            .code(),
        Some(8)
    );
}

#[test]
fn early_try_drops_already_initialized_owned_fields() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Pair { first: string, detail: Option<string> }\n",
        "fn maybe(fail: bool) -> Result<string, Error> {\n",
        "  if fail { return Err(error(17)) }\n",
        "  return Ok(\"ok\".clone())\n",
        "}\n",
        "fn build(fail: bool) -> Result<i64, Error> {\n",
        "  p := Pair { first: \"first\".clone(), detail: Some(maybe(fail)?) }\n",
        "  return Ok(p.first.len())\n",
        "}\n",
        "fn main() -> Result<(), Error> {\n",
        "  print(build(true)?)\n",
        "  return Ok(())\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("owned-option-field-early-try", src)
            .status
            .code(),
        Some(17)
    );
}

#[test]
fn option_move_struct_remains_an_l1b_diagnostic() {
    let src = concat!(
        "Inner { name: string }\n",
        "Outer { value: Option<Inner> }\n",
        "fn main() -> i32 = 0\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "option-move-struct.align", src);
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        checked.diags.has_errors(),
        "Option<MoveStruct> must remain rejected"
    );
    assert!(
        rendered.contains("L1b"),
        "diagnostic must name the owning slice:\n{rendered}"
    );
}

#[test]
fn option_struct_that_is_move_through_enum_remains_an_l1b_diagnostic() {
    let src = concat!(
        "Inner { content: Content }\n",
        "Content { Empty, Data(array<i64>) }\n",
        "Outer { value: Option<Inner> }\n",
        "fn main() -> i32 = 0\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "option-enum-move-struct.align", src);
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        checked.diags.has_errors(),
        "Option<Struct> must not bypass L1b when only a resolved enum field makes the payload Move"
    );
    assert!(
        rendered.contains("Option<MoveStruct> is implemented in L1b"),
        "diagnostic must report the unsupported cleanup shape:\n{rendered}"
    );
}

#[test]
fn llvm_drop_has_a_tag_guard_and_none_constructs_without_allocation() {
    let src = concat!(
        "Item { detail: Option<string> }\n",
        "fn none() -> Item = Item { detail: None }\n",
        "fn main() -> i32 {\n",
        "  x := none()\n",
        "  return 0\n",
        "}\n",
    );
    let ir = emit_llvm(src);
    assert!(
        ir.contains("dropoptissome"),
        "Drop must test the Option tag:\n{ir}"
    );
    let none_start = ir
        .find("define internal")
        .expect("missing function definition");
    let none_body = &ir[none_start..ir.find("\n}").map_or(ir.len(), |end| end + 2)];
    assert!(
        !none_body.contains("@align_rt_alloc") && !none_body.contains("@malloc"),
        "the None constructor must not allocate:\n{none_body}"
    );
}
