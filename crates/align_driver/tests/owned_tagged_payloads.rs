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
fn arena_owned_field_replacement_does_not_drop_the_old_leaf_individually() {
    let src = format!(
        "{DECL}\
         fn main() -> i32 {{\n\
           mut v := Item {{ detail: Some(\"old\".clone()), n: 7 }}\n\
           v.detail = Some(\"new\".clone())\n\
           return v.n as i32\n\
         }}\n"
    );
    let mut sm = SourceMap::new();
    let mut checked = check(&mut sm, "arena-owned-field-replace.align", &src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );

    // The public language has no arena-backed `string` producer yet: `.clone()` is deliberately
    // free-standing. Override the analysis facts on this already-checked HIR to exercise the MIR
    // provenance path that future arena-backed owned leaves use. Both the aggregate and replacement
    // are marked arena-owned, so the old field must be left for ArenaEnd rather than DropValue.
    let main = checked
        .hir
        .fns
        .iter_mut()
        .find(|f| f.name == "main")
        .expect("main function");
    main.drop_individual_locals.clear();
    for individual in main.drop_individual_exprs.values_mut() {
        *individual = false;
    }
    let mir = align_mir::print::program_to_string(&lower_to_mir(&checked.hir));
    assert!(
        !mir.contains("drop_value"),
        "arena-owned field replacement must not individually free its old leaf:\n{mir}"
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
fn field_self_assignment_preserves_existing_borrows() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "User { name: string }\n",
        "fn main() -> i32 {\n",
        "  mut user := User { name: \"hello\".clone() }\n",
        "  view: str := user.name\n",
        "  user.name = user.name\n",
        "  return view.len() as i32\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("owned-field-self-assign-borrow", src)
            .status
            .code(),
        Some(5)
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
fn retained_result_with_recursive_move_payload_is_an_l1b_diagnostic() {
    for (name, src, expected) in [
        (
            "fs-read-dir-retained-result.align",
            concat!(
                "import std.fs\n",
                "fn main() -> i32 {\n",
                "  result := fs.read_dir(\".\")\n",
                "  return 0\n",
                "}\n",
            ),
            "retained Move Option/Result payloads are implemented in L1b",
        ),
        (
            "dns-resolve-retained-result.align",
            concat!(
                "import std.net\n",
                "fn main() -> i32 {\n",
                "  result := dns.resolve(\"localhost\")\n",
                "  return 0\n",
                "}\n",
            ),
            "retained Move Option/Result payloads are implemented in L1b",
        ),
        (
            "option-string-array.align",
            concat!(
                "fn main() -> i32 {\n",
                "  value: Option<array<string>> := None\n",
                "  return 0\n",
                "}\n",
            ),
            "recursive/deep Move tagged payloads are implemented in L1b",
        ),
        (
            "user-result-move-enum.align",
            concat!(
                "Part { kind: str }\n",
                "Content { Text(str), Parts(array<Part>) }\n",
                "fn main() -> i32 {\n",
                "  result: Result<Content, Error> := Ok(Content.Parts([Part { kind: \"owned\" }].to_array()))\n",
                "  return 0\n",
                "}\n",
            ),
            "nullable/fallible owned union is a later slice",
        ),
        (
            "json-result-move-enum.align",
            concat!(
                "import core.json\n",
                "Part { kind: str }\n",
                "Content { Text(str), Parts(array<Part>) }\n",
                "fn main() -> i32 {\n",
                "  result: Result<Content, Error> := json.decode(\"[]\")\n",
                "  return 0\n",
                "}\n",
            ),
            "recursive/deep Move tagged payloads are implemented in L1b",
        ),
        (
            "json-result-move-struct-array.align",
            concat!(
                "import core.json\n",
                "Part { kind: str }\n",
                "Content { Text(str), Parts(array<Part>) }\n",
                "Message { content: Content }\n",
                "fn main() -> i32 {\n",
                "  result: Result<array<Message>, Error> := json.decode(\"[]\")\n",
                "  return 0\n",
                "}\n",
            ),
            "recursive/deep Move tagged payloads are implemented in L1b",
        ),
        (
            "http-result-response-array.align",
            concat!(
                "import std.http\n",
                "fn main() -> i32 {\n",
                "  urls := [\"http://x/\"]\n",
                "  cl := http.client()\n",
                "  result := cl.get_many(urls, 1)\n",
                "  return 0\n",
                "}\n",
            ),
            "retained Move Option/Result payloads are implemented in L1b",
        ),
    ] {
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, name, src);
        let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
        assert!(
            checked.diags.has_errors(),
            "retaining Result<MoveEnum, Error> must remain rejected"
        );
        assert!(
            rendered.contains(expected),
            "diagnostic must report the unsupported cleanup shape:\n{rendered}"
        );
    }
}

#[test]
fn recursive_move_tagged_payloads_are_rejected_in_signatures() {
    for (name, src) in [
        (
            "result-move-enum-param.align",
            concat!(
                "Part { kind: str }\n",
                "Content { Text(str), Parts(array<Part>) }\n",
                "fn discard(value: Result<Content, Error>) -> i32 = 0\n",
                "fn main() -> i32 = 0\n",
            ),
        ),
        (
            "option-move-enum-param.align",
            concat!(
                "Part { kind: str }\n",
                "Content { Text(str), Parts(array<Part>) }\n",
                "fn discard(value: Option<Content>) -> i32 = 0\n",
                "fn main() -> i32 = 0\n",
            ),
        ),
        (
            "result-move-struct-array-param.align",
            concat!(
                "Part { kind: str }\n",
                "Content { Text(str), Parts(array<Part>) }\n",
                "Message { content: Content }\n",
                "fn discard(value: Result<array<Message>, Error>) -> i32 = 0\n",
                "fn main() -> i32 = 0\n",
            ),
        ),
    ] {
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, name, src);
        let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
        assert!(
            checked.diags.has_errors(),
            "unsupported tagged cleanup must be rejected at the declared type boundary"
        );
        assert!(
            rendered.contains("recursive/deep Move tagged payloads are implemented in L1b"),
            "diagnostic must identify the owning slice:\n{rendered}"
        );
    }
}

#[test]
fn immediate_try_may_consume_a_recursive_move_result() {
    let src = concat!(
        "import core.json\n",
        "Part { kind: str }\n",
        "Content { Text(str), Parts(array<Part>) }\n",
        "fn decode() -> Result<(), Error> {\n",
        "  content: Content := json.decode(\"[]\")?\n",
        "  return Ok(())\n",
        "}\n",
        "fn main() -> i32 = 0\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "immediate-try-move-enum.align", src);
    assert!(
        !checked.diags.has_errors(),
        "an immediate `?` owns the decoded payload without retaining its tagged Result:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
}

#[test]
fn try_cannot_propagate_an_arena_owned_shallow_move_error() {
    for (name, body) in [
        (
            "direct-arena-error.align",
            "    value: i64 := Err([1, 2].to_array())?\n",
        ),
        (
            "bound-arena-error.align",
            concat!(
                "    result: Result<i64, array<i64>> := Err([1, 2].to_array())\n",
                "    value := result?\n",
            ),
        ),
    ] {
        let src = format!(
            "fn relay() -> Result<i64, array<i64>> {{\n  arena {{\n{body}    return Ok(value)\n  }}\n}}\nfn main() -> i32 = 0\n"
        );
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, name, &src);
        let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
        assert!(
            checked.diags.has_errors(),
            "an arena-owned error cannot survive the implicit return edge of `?`: {name}"
        );
        assert!(
            rendered.contains("cannot propagate an arena-owned error through `?`"),
            "diagnostic must identify the implicit escape:\n{rendered}"
        );
    }
}

#[test]
fn try_cannot_propagate_a_borrowed_error() {
    for (name, body) in [
        (
            "direct-borrowed-error.align",
            "  value: i64 := Err(view)?\n",
        ),
        (
            "bound-borrowed-error.align",
            concat!(
                "  result: Result<i64, str> := Err(view)\n",
                "  value := result?\n",
            ),
        ),
    ] {
        let src = format!(
            "fn relay() -> Result<i64, str> {{\n  storage := \"boom\".clone()\n  view: str := storage\n{body}  return Ok(value)\n}}\nfn main() -> i32 = 0\n"
        );
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, name, &src);
        let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
        assert!(
            checked.diags.has_errors(),
            "a borrowed error cannot survive the implicit return edge of `?`: {name}"
        );
        assert!(
            rendered.contains("cannot return a view that borrows local storage"),
            "diagnostic must identify the dangling implicit return:\n{rendered}"
        );
    }
}

#[test]
fn try_may_propagate_a_free_standing_error_from_inside_an_arena() {
    let src = concat!(
        "fn relay() -> Result<i64, string> {\n",
        "  arena {\n",
        "    value: i64 := Err(\"boom\".clone())?\n",
        "    return Ok(value)\n",
        "  }\n",
        "}\n",
        "fn main() -> i32 = 0\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "free-standing-arena-error.align", src);
    assert!(
        !checked.diags.has_errors(),
        "`string.clone()` is free-standing even when called inside an arena:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
}

#[test]
fn try_cannot_propagate_a_recursive_move_error_payload() {
    let src = concat!(
        "Part { kind: str }\n",
        "Content { Text(str), Parts(array<Part>) }\n",
        "fn base() -> Result<i64, Error> = Err(error(1))\n",
        "fn own_error(e: Error) -> Content = Content.Parts([Part { kind: \"owned\" }].to_array())\n",
        "fn relay() -> Result<(), Content> {\n",
        "  value := base().map_err(own_error)?\n",
        "  return Ok(())\n",
        "}\n",
        "fn main() -> i32 = 0\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "try-move-error.align", src);
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        checked.diags.has_errors(),
        "`?` propagates rather than consumes its error payload"
    );
    assert!(
        rendered.contains("propagating a recursive Move error payload through `?`"),
        "diagnostic must identify the unsupported propagation path:\n{rendered}"
    );
}

#[test]
fn try_transfers_a_bound_shallow_move_error_payload() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "fn fail() -> Result<i64, string> = Err(\"boom\".clone())\n",
        "fn relay() -> Result<i64, string> {\n",
        "  result := fail()\n",
        "  value := result?\n",
        "  return Ok(value)\n",
        "}\n",
        "fn main() -> i32 = match relay() {\n",
        "  Ok(value) => value as i32,\n",
        "  Err(message) => message.len() as i32,\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("try-bound-move-error", src).status.code(),
        Some(4)
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
