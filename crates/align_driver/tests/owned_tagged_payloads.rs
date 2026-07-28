//! L1a/L1b: the canonical recursive Drop plan and direct Move tagged payloads.
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
fn option_move_struct_field_constructs_extracts_and_drops() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Inner { name: string }\n",
        "Outer { value: Option<Inner> }\n",
        "fn main() -> i32 {\n",
        "  outer := Outer { value: Some(Inner { name: \"owned\".clone() }) }\n",
        "  inner := outer.value else { return 90 }\n",
        "  return inner.name.len() as i32\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("option-move-struct", src).status.code(),
        Some(5)
    );
}

#[test]
fn option_struct_recursively_drops_a_move_sum_field() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Inner { content: Content }\n",
        "Content { Empty, Data(array<i64>) }\n",
        "Outer { value: Option<Inner> }\n",
        "fn main() -> i32 {\n",
        "  outer := Outer { value: Some(Inner { content: Content.Data([1, 2, 3].to_array()) }) }\n",
        "  inner := outer.value else { return 90 }\n",
        "  return match inner.content { Empty => 91, Data(values) => values.len() as i32 }\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("option-enum-move-struct", src)
            .status
            .code(),
        Some(3)
    );
}

#[test]
fn retained_result_with_recursive_move_payload_is_supported() {
    for (name, src) in [
        (
            "fs-read-dir-retained-result.align",
            concat!(
                "import std.fs\n",
                "fn main() -> i32 {\n",
                "  result := fs.read_dir(\".\")\n",
                "  return 0\n",
                "}\n",
            ),
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
        ),
        (
            "option-string-array.align",
            concat!(
                "fn main() -> i32 {\n",
                "  value: Option<array<string>> := None\n",
                "  return 0\n",
                "}\n",
            ),
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
        ),
    ] {
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, name, src);
        assert!(
            !checked.diags.has_errors(),
            "retaining a recursive Move tagged payload must be supported in {name}:\n{}",
            align_driver::format_diagnostics(&sm, &checked.diags)
        );
    }
}

#[test]
fn recursive_move_tagged_payloads_are_supported_in_signatures() {
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
        assert!(
            !checked.diags.has_errors(),
            "recursive tagged cleanup must be accepted at the declared type boundary:\n{}",
            align_driver::format_diagnostics(&sm, &checked.diags)
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
fn try_cannot_propagate_an_arena_owned_recursive_move_error() {
    let src = concat!(
        "OwnedError { Values(array<i64>) }\n",
        "fn relay() -> Result<i64, OwnedError> {\n",
        "  arena {\n",
        "    value: i64 := Err(OwnedError.Values([1, 2].to_array()))?\n",
        "    return Ok(value)\n",
        "  }\n",
        "}\n",
        "fn main() -> i32 = 0\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "arena-recursive-error.align", src);
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        checked.diags.has_errors(),
        "an arena-owned recursive error cannot survive the implicit return edge of `?`"
    );
    assert!(
        rendered.contains("cannot propagate an arena-owned error through `?`"),
        "diagnostic must identify the recursive implicit escape:\n{rendered}"
    );
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
fn try_propagates_a_recursive_move_error_payload() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Part { kind: str }\n",
        "Content { Text(str), Parts(array<Part>) }\n",
        "fn base() -> Result<i64, Error> = Err(error(1))\n",
        "fn own_error(e: Error) -> Content = Content.Parts([Part { kind: \"owned\" }].to_array())\n",
        "fn relay() -> Result<(), Content> {\n",
        "  value := base().map_err(own_error)?\n",
        "  return Ok(())\n",
        "}\n",
        "fn main() -> i32 = match relay() {\n",
        "  Ok(value) => 90,\n",
        "  Err(content) => match content {\n",
        "    Text(value) => 91,\n",
        "    Parts(parts) => parts.len() as i32,\n",
        "  },\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("try-move-error", src).status.code(),
        Some(1)
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
fn native_db_error_payloads_close_direct_tagged_edges() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "NativeError { code: Option<string>, message: string }\n",
        "DbError { Native(NativeError), Decode(string) }\n",
        "Output { text: Option<string> }\n",
        "fn run(mode: i64) -> Result<Output, DbError> {\n",
        "  if mode == 0 { return Ok(Output { text: None }) }\n",
        "  if mode == 1 { return Ok(Output { text: Some(\"ok\".clone()) }) }\n",
        "  if mode == 2 {\n",
        "    return Err(DbError.Native(NativeError { code: Some(\"7\".clone()), message: \"native\".clone() }))\n",
        "  }\n",
        "  return Err(DbError.Decode(\"decode\".clone()))\n",
        "}\n",
        "fn score(result: Result<Output, DbError>) -> i64 = match result {\n",
        "  Ok(output) => match output.text { None => 1, Some(value) => 2 },\n",
        "  Err(err) => match err { Native(value) => 3, Decode(message) => 4 },\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  return (score(run(0)) + score(run(1)) + score(run(2)) + score(run(3))) as i32\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("native-db-error-shape", src).status.code(),
        Some(10)
    );
}

#[test]
fn recursive_move_results_replace_and_join_with_one_live_owner() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "DbError { Decode(string), Other }\n",
        "fn run(mode: i64) -> Result<i64, DbError> {\n",
        "  if mode == 0 { return Ok(5) }\n",
        "  if mode == 1 { return Err(DbError.Decode(\"old\".clone())) }\n",
        "  return Err(DbError.Decode(\"joined\".clone()))\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  mut first := run(1)\n",
        "  first = if true { run(2) } else { run(0) }\n",
        "  mut second := run(1)\n",
        "  second = loop {\n",
        "    if true { break run(2) }\n",
        "    break run(0)\n",
        "  }\n",
        "  a := match first { Ok(value) => value, Err(err) => match err { Decode(message) => message.len(), Other => 90 } }\n",
        "  b := match second { Ok(value) => value, Err(err) => match err { Decode(message) => message.len(), Other => 91 } }\n",
        "  return (a + b) as i32\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("recursive-move-result-joins", src)
            .status
            .code(),
        Some(12)
    );
}

#[test]
fn multiple_move_payloads_construct_bind_and_drop() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "PairError { Both(string, string), One(string), Empty }\n",
        "fn main() -> i32 {\n",
        "  pair := PairError.Both(\"left\".clone(), \"right\".clone())\n",
        "  return match pair {\n",
        "    Both(left, right) => (left.len() + right.len()) as i32\n",
        "    One(value) => value.len() as i32\n",
        "    Empty => 90\n",
        "  }\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("multiple-move-payload-bind", src)
            .status
            .code(),
        Some(9)
    );
}

#[test]
fn multiple_move_payload_construction_consumes_every_source() {
    for source in ["left", "right"] {
        let src = format!(
            "PairError {{ Both(string, string), Empty }}\n\
             fn main() -> i32 {{\n\
               left := \"left\".clone()\n\
               right := \"right\".clone()\n\
               error := PairError.Both(left, right)\n\
               return {source}.len() as i32\n\
             }}\n"
        );
        let mut sm = SourceMap::new();
        let checked = check(
            &mut sm,
            &format!("multiple-move-payload-consume-{source}.align"),
            &src,
        );
        let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
        assert!(checked.diags.has_errors());
        assert!(
            rendered.contains(&format!("use of moved value '{source}'")),
            "each payload source must be consumed:\n{rendered}"
        );
    }
}

#[test]
fn multiple_move_payload_construction_keeps_earlier_owner_on_early_exit() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "PairError { Both(string, string), Empty }\n",
        "fn fail() -> Result<string, string> = Err(\"later\".clone())\n",
        "fn build() -> Result<PairError, string> {\n",
        "  pair := PairError.Both(\"first\".clone(), fail()?)\n",
        "  return Ok(pair)\n",
        "}\n",
        "fn build_return() -> Result<PairError, string> {\n",
        "  pair := PairError.Both(\"first\".clone(), { return Err(\"later\".clone()) })\n",
        "  return Ok(pair)\n",
        "}\n",
        "fn score(result: Result<PairError, string>) -> i32 = match result {\n",
        "  Ok(pair) => 90\n",
        "  Err(message) => message.len() as i32\n",
        "}\n",
        "fn main() -> i32 = score(build()) + score(build_return())\n",
    );
    assert_eq!(
        build_and_run("multiple-move-payload-early-exit", src)
            .status
            .code(),
        Some(10)
    );
}

#[test]
fn multiple_move_payloads_require_one_allocation_mode() {
    if backend_available() {
        let uniform = concat!(
            "PairError { Both(array<i64>, array<i64>), Empty }\n",
            "fn main() -> i32 {\n",
            "  return arena {\n",
            "    pair := PairError.Both([1, 2].to_array(), [3, 4, 5].to_array())\n",
            "    match pair { Both(left, right) => (left.len() + right.len()) as i32, Empty => 90 }\n",
            "  }\n",
            "}\n",
        );
        assert_eq!(
            build_and_run("multiple-move-payload-arena", uniform)
                .status
                .code(),
            Some(5)
        );
    }

    let mixed = concat!(
        "PairError { Both(array<i64>, array<i64>), Empty }\n",
        "fn heap_value() -> array<i64> = [1, 2].to_array()\n",
        "fn main() -> i32 {\n",
        "  return arena {\n",
        "    pair := PairError.Both(heap_value(), [3, 4, 5].to_array())\n",
        "    match pair { Both(left, right) => (left.len() + right.len()) as i32, Empty => 90 }\n",
        "  }\n",
        "}\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "multiple-move-payload-mixed.align", mixed);
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(checked.diags.has_errors());
    assert!(
        rendered.contains("cannot mix free-standing and arena-owned"),
        "mixed ownership diagnostic must be deterministic:\n{rendered}"
    );

    let path_dependent = concat!(
        "PairError { Both(array<i64>, array<i64>), Empty }\n",
        "fn heap_value() -> array<i64> = [1, 2].to_array()\n",
        "fn run(use_arena: bool) -> i32 {\n",
        "  return arena {\n",
        "    mut selected := heap_value()\n",
        "    if use_arena { selected = [3, 4].to_array() }\n",
        "    pair := PairError.Both(selected, [5].to_array())\n",
        "    match pair { Both(left, right) => (left.len() + right.len()) as i32, Empty => 90 }\n",
        "  }\n",
        "}\n",
        "fn main() -> i32 = run(false)\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(
        &mut sm,
        "multiple-move-payload-path-dependent.align",
        path_dependent,
    );
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(checked.diags.has_errors());
    assert!(
        rendered.contains("cannot mix free-standing and arena-owned"),
        "path-dependent ownership must fail closed:\n{rendered}"
    );
}

#[test]
fn multiple_move_payloads_drop_through_wildcard_and_or_patterns() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "PairError { Both(string, string), One(string), Empty }\n",
        "fn main() -> i32 {\n",
        "  wildcard := match PairError.Both(\"left\".clone(), \"right\".clone()) { _ => 3 }\n",
        "  grouped := match PairError.Both(\"a\".clone(), \"b\".clone()) {\n",
        "    Both | One => 4\n",
        "    Empty => 90\n",
        "  }\n",
        "  return wildcard + grouped\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("multiple-move-payload-discard", src)
            .status
            .code(),
        Some(7)
    );
}

#[test]
fn generic_sum_allows_multiple_move_payloads_after_substitution() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Pair<T> { Both(T, T), Empty }\n",
        "fn main() -> i32 {\n",
        "  pair := Pair.Both(\"left\".clone(), \"right\".clone())\n",
        "  return match pair { Both(left, right) => (left.len() + right.len()) as i32, Empty => 90 }\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("generic-multiple-move-payload", src)
            .status
            .code(),
        Some(9)
    );
}

#[test]
fn else_drops_a_recursive_move_error_before_fallback() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "NativeError { code: Option<string>, message: string }\n",
        "DbError { Native(NativeError), Decode(string) }\n",
        "fn fail() -> Result<i64, DbError> = Err(DbError.Native(NativeError {\n",
        "  code: Some(\"7\".clone()),\n",
        "  message: \"native\".clone(),\n",
        "}))\n",
        "fn main() -> i32 {\n",
        "  value := fail() else { return 7 }\n",
        "  return value as i32\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("else-drop-recursive-error", src)
            .status
            .code(),
        Some(7)
    );
}

#[test]
fn map_err_transfers_a_recursive_move_error() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "DbError { Decode(string), Other }\n",
        "fn fail() -> Result<i64, string> = Err(\"decode\".clone())\n",
        "fn wrap(message: string) -> DbError = DbError.Decode(message)\n",
        "fn main() -> i32 = match fail().map_err(wrap) {\n",
        "  Ok(value) => 90,\n",
        "  Err(err) => match err { Decode(message) => message.len() as i32, Other => 91 },\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("map-err-recursive-error", src)
            .status
            .code(),
        Some(6)
    );
}

#[test]
fn recursive_inline_sum_layouts_are_rejected() {
    for (name, src) in [
        (
            "direct-recursive-sum.align",
            "Cycle { Again(Cycle), End }\nfn main() -> i32 = 0\n",
        ),
        (
            "mutual-recursive-sum.align",
            "Left { RightValue(Right) }\nRight { LeftValue(Left) }\nfn main() -> i32 = 0\n",
        ),
        (
            "optional-recursive-sum-field.align",
            "Node { next: Option<Link> }\nLink { NodeValue(Node), End }\nfn main() -> i32 = 0\n",
        ),
        (
            "nested-tagged-recursive-struct-field.align",
            "Node { next: Option<Result<Node, bool>> }\nfn main() -> i32 = 0\n",
        ),
        (
            "nested-tagged-recursive-sum-field.align",
            "Node { next: Option<Result<Link, bool>> }\nLink { NodeValue(Node), End }\nfn main() -> i32 = 0\n",
        ),
    ] {
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, name, src);
        let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
        assert!(
            checked.diags.has_errors(),
            "inline recursive tagged layout must be rejected: {name}"
        );
        assert!(
            rendered.contains("recursive"),
            "diagnostic must identify the recursive layout in {name}:\n{rendered}"
        );
    }
}

#[test]
fn nested_tagged_payload_executes_exact_pkg_db_shape() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Output { text: string, note: Option<string> }\n",
        "NativeError { code: Option<string>, message: string }\n",
        "DbError { Native(NativeError), Decode(string) }\n",
        "fn run(mode: i32) -> Result<Option<Output>, DbError> {\n",
        "  if mode == 0 { return Ok(None) }\n",
        "  if mode == 1 {\n",
        "    return Ok(Some(Output { text: \"row\".clone(), note: Some(\"note\".clone()) }))\n",
        "  }\n",
        "  if mode == 2 { return Err(DbError.Decode(\"decode\".clone())) }\n",
        "  return Err(DbError.Native(NativeError {\n",
        "    code: Some(\"7\".clone()),\n",
        "    message: \"native\".clone(),\n",
        "  }))\n",
        "}\n",
        "fn score(result: Result<Option<Output>, DbError>) -> i32 = match result {\n",
        "  Ok(value) => match value {\n",
        "    Some(output) => output.text.len() as i32 + match output.note {\n",
        "      Some(note) => note.len() as i32,\n",
        "      None => 0,\n",
        "    },\n",
        "    None => 2,\n",
        "  },\n",
        "  Err(error) => match error {\n",
        "    Native(value) => value.message.len() as i32 + match value.code {\n",
        "      Some(code) => code.len() as i32,\n",
        "      None => 0,\n",
        "    },\n",
        "    Decode(message) => message.len() as i32,\n",
        "  },\n",
        "}\n",
        "fn main() -> i32 = score(run(0)) + score(run(1)) + score(run(2)) + score(run(3))\n",
    );
    let ir = emit_llvm(src);
    assert!(
        ir.contains("%align.tagged."),
        "nested Option/Result payloads must use identified tagged LLVM types:\n{ir}"
    );
    assert!(
        ir.contains("%align.tagged.0 = type { i8, %Output }")
            && ir.contains("dropoptissome"),
        "nested layout and recursive Option Drop must preserve their tags:\n{ir}"
    );
    assert_eq!(
        build_and_run("nested-tagged-payload", src).status.code(),
        Some(22)
    );
}

#[test]
fn nested_pkg_db_shape_preserves_transfer_and_replacement_rules() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Output { text: string, note: Option<string> }\n",
        "DbError { Decode(string) }\n",
        "fn run(mode: i32) -> Result<Option<Output>, DbError> {\n",
        "  if mode == 0 { return Ok(None) }\n",
        "  if mode == 1 {\n",
        "    return Ok(Some(Output { text: \"row\".clone(), note: Some(\"note\".clone()) }))\n",
        "  }\n",
        "  return Err(DbError.Decode(\"decode\".clone()))\n",
        "}\n",
        "fn score(result: Result<Option<Output>, DbError>) -> i32 = match result {\n",
        "  Ok(value) => match value {\n",
        "    Some(output) => output.text.len() as i32 + match output.note {\n",
        "      Some(note) => note.len() as i32,\n",
        "      None => 0,\n",
        "    },\n",
        "    None => 2,\n",
        "  },\n",
        "  Err(error) => match error { Decode(message) => message.len() as i32 },\n",
        "}\n",
        "fn relay(mode: i32) -> Result<Option<Output>, DbError> {\n",
        "  value := run(mode)?\n",
        "  return Ok(value)\n",
        "}\n",
        "fn discard_error(mode: i32) -> i32 {\n",
        "  value := run(mode) else { return 3 }\n",
        "  return score(Ok(value))\n",
        "}\n",
        "fn source_error() -> Result<Option<Output>, string> = Err(\"decode\".clone())\n",
        "fn wrap_error(message: string) -> DbError = DbError.Decode(message)\n",
        "fn replace() -> i32 {\n",
        "  mut result: Result<Option<Output>, DbError> := run(1)\n",
        "  result = run(0)\n",
        "  result = run(2)\n",
        "  result = run(1)\n",
        "  return score(result)\n",
        "}\n",
        "fn main() -> i32 = score(relay(1)) + score(relay(2))\n",
        "  + discard_error(2) + discard_error(0)\n",
        "  + score(source_error().map_err(wrap_error)) + replace()\n",
    );
    let mir = mir_text(src);
    assert!(
        mir.matches("    drop _0\n").count() >= 3 && mir.contains("    drop _6\n"),
        "nested replacement and discarded error paths must retain recursive cleanup:\n{mir}"
    );
    assert_eq!(
        build_and_run("nested-pkg-db-transfer-and-replacement", src)
            .status
            .code(),
        Some(31)
    );
}

#[test]
fn nested_tagged_struct_fields_and_sum_payloads_use_recursive_ownership() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Holder { value: Result<Option<string>, string> }\n",
        "Nested { value: Option<Result<string, string>> }\n",
        "Wrapped { Value(Option<Result<string, string>>), Empty }\n",
        "fn main() -> i32 {\n",
        "  holder := Holder { value: Ok(Some(\"a\".clone())) }\n",
        "  nested := Nested { value: Some(Err(\"bb\".clone())) }\n",
        "  wrapped := Wrapped.Value(Some(Ok(\"ccc\".clone())))\n",
        "  a := match holder.value {\n",
        "    Ok(value) => match value { Some(text) => text.len(), None => 90 },\n",
        "    Err(message) => 91,\n",
        "  }\n",
        "  b := match nested.value {\n",
        "    Some(value) => match value { Ok(text) => 92, Err(message) => message.len() },\n",
        "    None => 93,\n",
        "  }\n",
        "  c := match wrapped {\n",
        "    Value(value) => match value {\n",
        "      Some(result) => match result { Ok(text) => text.len(), Err(message) => 94 },\n",
        "      None => 95,\n",
        "    },\n",
        "    Empty => 96,\n",
        "  }\n",
        "  return (a + b + c) as i32\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("nested-tagged-fields-and-sum-payloads", src)
            .status
            .code(),
        Some(6)
    );
}

#[test]
fn ignored_inner_tagged_binding_still_runs_recursive_drop() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "fn main() -> i32 {\n",
        "  value: Option<Result<string, string>> := Some(Ok(\"owned\".clone()))\n",
        "  return match value { Some(inner) => 7, None => 90 }\n",
        "}\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "nested-tagged-ignored-inner.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let program = lower_to_mir(&checked.hir);
    let main = program
        .fns
        .iter()
        .find(|function| function.name == "main")
        .expect("main MIR");
    let inner_slot = main
        .slots
        .iter()
        .position(|ty| matches!(ty, align_sema::Ty::Result(..)))
        .expect("inner Result binding") as u32;
    assert!(
        main.blocks.iter().flat_map(|block| &block.stmts).any(
            |stmt| matches!(stmt, align_mir::Stmt::Drop(slot) if *slot == inner_slot)
        ),
        "the ignored inner Result binding must retain path-local cleanup:\n{}",
        align_mir::print::function_to_string(main)
    );
    let ir = emit_llvm(src);
    assert!(
        ir.contains("drop.result.ok"),
        "the ignored inner Result binding must recursively drop its active owned arm:\n{ir}"
    );
    assert_eq!(
        build_and_run("nested-tagged-ignored-inner", src)
            .status
            .code(),
        Some(7)
    );
}

#[test]
fn generic_nested_tagged_results_reintern_each_concrete_shape() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Output { text: string, note: Option<string> }\n",
        "fn wrap<T>(value: T) -> Result<Option<T>, string> = Ok(Some(value))\n",
        "fn main() -> i32 {\n",
        "  number: Result<Option<i64>, string> := wrap(5)\n",
        "  output: Result<Option<Output>, string> := wrap(Output {\n",
        "    text: \"row\".clone(),\n",
        "    note: Some(\"note\".clone()),\n",
        "  })\n",
        "  a := match number { Ok(value) => value else 90, Err(message) => 91 }\n",
        "  b := match output {\n",
        "    Ok(value) => match value {\n",
        "      Some(row) => row.text.len() + match row.note { Some(note) => note.len(), None => 0 },\n",
        "      None => 92,\n",
        "    },\n",
        "    Err(message) => 93,\n",
        "  }\n",
        "  return (a + b) as i32\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("generic-nested-tagged-results", src)
            .status
            .code(),
        Some(12)
    );
}

#[test]
fn nested_tagged_mismatch_diagnostics_render_structural_types() {
    let src = concat!(
        "fn take(value: Result<Option<i64>, bool>) -> i32 = 0\n",
        "fn main() -> i32 {\n",
        "  value: Result<Option<bool>, bool> := Ok(Some(true))\n",
        "  return take(value)\n",
        "}\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "nested-tagged-mismatch.align", src);
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(checked.diags.has_errors(), "mismatched nested types must fail");
    assert!(
        rendered.contains("Result<Option<i64>, bool>")
            && rendered.contains("Result<Option<bool>, bool>"),
        "diagnostics must render complete structural nested types:\n{rendered}"
    );
    assert!(
        !rendered.contains("nested tagged type") && !rendered.contains("tagged#"),
        "diagnostics must not expose compiler-internal tagged identities:\n{rendered}"
    );
}

#[test]
fn generic_struct_and_sum_substitute_nested_tagged_payloads() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Holder<T> { value: Option<Result<T, string>> }\n",
        "Envelope<T> { Value(Option<Result<T, string>>), Empty }\n",
        "fn main() -> i32 {\n",
        "  holder_value: Option<Result<i64, string>> := Some(Ok(7))\n",
        "  envelope_value: Option<Result<i64, string>> := Some(Ok(5))\n",
        "  holder := Holder { value: holder_value }\n",
        "  envelope := Envelope.Value(envelope_value)\n",
        "  a := match holder.value {\n",
        "    Some(result) => match result { Ok(value) => value, Err(message) => 90 },\n",
        "    None => 91,\n",
        "  }\n",
        "  b := match envelope {\n",
        "    Value(value) => match value {\n",
        "      Some(result) => match result { Ok(number) => number, Err(message) => 92 },\n",
        "      None => 93,\n",
        "    },\n",
        "    Empty => 94,\n",
        "  }\n",
        "  return (a + b) as i32\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("generic-nested-tagged-aggregates", src)
            .status
            .code(),
        Some(12)
    );
}

#[test]
fn nested_tagged_table_changes_codegen_identity() {
    let src = concat!(
        "fn nested() -> Result<Option<i64>, bool> = Ok(Some(1))\n",
        "fn main() -> i32 = 0\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "nested-tagged-codegen-hash.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let program = lower_to_mir(&checked.hir);
    let mut changed = program.clone();
    changed.tagged_types[0] =
        align_sema::hir::TaggedType::Option(align_sema::Scalar::Bool);
    assert_ne!(
        align_interface::codegen_impl_hash(&program),
        align_interface::codegen_impl_hash(&changed),
        "the complete canonical tagged table must participate in object-cache identity"
    );
}

#[test]
fn nested_tagged_values_fail_closed_at_unsupported_boundaries() {
    let cases = [
        (
            "nested-tagged-ffi.align",
            concat!(
                "extern \"C\" fn foreign(value: Option<Result<i64, bool>>) -> i64\n",
                "fn main() -> i32 = 0\n",
            ),
            "FFI",
        ),
        (
            "nested-tagged-box.align",
            "fn take(value: box<Option<Result<i64, bool>>>) -> i32 = 0\nfn main() -> i32 = 0\n",
            "box payload",
        ),
        (
            "nested-tagged-array.align",
            concat!(
                "fn make() -> Option<Result<i64, bool>> = Some(Ok(1))\n",
                "fn main() -> i32 {\n",
                "  values := [make()]\n",
                "  return 0\n",
                "}\n",
            ),
            "array",
        ),
        (
            "nested-tagged-print.align",
            concat!(
                "fn make() -> Option<Result<i64, bool>> = Some(Ok(1))\n",
                "fn main() -> i32 {\n",
                "  print(make())\n",
                "  return 0\n",
                "}\n",
            ),
            "print",
        ),
        (
            "nested-tagged-hash.align",
            concat!(
                "fn make() -> Option<Result<i64, bool>> = Some(Ok(1))\n",
                "fn main() -> i32 {\n",
                "  value := hash64(make())\n",
                "  return 0\n",
                "}\n",
            ),
            "hash64",
        ),
        (
            "nested-tagged-json.align",
            concat!(
                "import core.json\n",
                "Holder { value: Option<Result<i64, bool>> }\n",
                "fn main() -> i32 {\n",
                "  holder := Holder { value: Some(Ok(1)) }\n",
                "  encoded := json.encode(holder)\n",
                "  return encoded.len() as i32\n",
                "}\n",
            ),
            "json",
        ),
    ];
    for (name, src, expected) in cases {
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, name, src);
        let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
        assert!(
            checked.diags.has_errors(),
            "{name} must reject a nested tagged value at this unsupported boundary"
        );
        assert!(
            rendered.to_ascii_lowercase().contains(&expected.to_ascii_lowercase()),
            "{name} diagnostic must name the rejected boundary:\n{rendered}"
        );
    }
}

#[test]
fn nested_tagged_public_surface_matches_whole_and_per_unit_builds() {
    if !backend_available() {
        return;
    }
    let producer = concat!(
        "module query\n",
        "pub Output { text: string, note: Option<string> }\n",
        "pub DbError { Decode(string) }\n",
        "pub fn run(mode: i32) -> Result<Option<Output>, DbError> {\n",
        "  if mode == 0 { return Ok(None) }\n",
        "  if mode == 1 {\n",
        "    return Ok(Some(Output { text: \"row\".clone(), note: Some(\"note\".clone()) }))\n",
        "  }\n",
        "  return Err(DbError.Decode(\"decode\".clone()))\n",
        "}\n",
    );
    let consumer = concat!(
        "module main\n",
        "import query\n",
        "// This private shape exists only in the consumer unit and perturbs its local tagged table.\n",
        "fn unrelated() -> Option<Result<i64, bool>> = Some(Ok(1))\n",
        "fn score(result: Result<Option<query.Output>, query.DbError>) -> i32 = match result {\n",
        "  Ok(value) => match value {\n",
        "    Some(output) => output.text.len() as i32 + match output.note {\n",
        "      Some(note) => note.len() as i32,\n",
        "      None => 0,\n",
        "    },\n",
        "    None => 2,\n",
        "  },\n",
        "  Err(error) => match error { Decode(message) => message.len() as i32 },\n",
        "}\n",
        "fn main() -> i32 = score(query.run(0)) + score(query.run(1)) + score(query.run(2)) + match unrelated() {\n",
        "  Some(value) => match value { Ok(n) => n as i32, Err(flag) => 90 },\n",
        "  None => 91,\n",
        "}\n",
    );
    let files = [("query.align", producer), ("main.align", consumer)];
    assert_eq!(
        build_and_run_multi("nested-tagged-whole", &files, "main.align")
            .status
            .code(),
        Some(16)
    );
    assert_eq!(
        build_per_unit_multi("nested-tagged-per-unit", &files, "main.align")
            .link_and_run()
            .status
            .code(),
        Some(16)
    );
}

#[test]
fn recursive_move_payloads_match_whole_program_and_per_unit_builds() {
    if !backend_available() {
        return;
    }
    let types = concat!(
        "module types\n",
        "pub NativeError { code: Option<string>, message: string }\n",
        "pub DbError { Native(NativeError), Decode(string) }\n",
        "pub PairError { Both(string, string), Empty }\n",
        "pub fn fail(native: bool) -> Result<i64, DbError> {\n",
        "  if native {\n",
        "    return Err(DbError.Native(NativeError { code: Some(\"7\".clone()), message: \"native\".clone() }))\n",
        "  }\n",
        "  return Err(DbError.Decode(\"decode\".clone()))\n",
        "}\n",
        "pub fn pair_error() -> PairError = PairError.Both(\"left\".clone(), \"right\".clone())\n",
    );
    let main = concat!(
        "module main\n",
        "import types\n",
        "fn score(result: Result<i64, types.DbError>) -> i64 = match result {\n",
        "  Ok(value) => 90,\n",
        "  Err(err) => match err { Native(value) => 3, Decode(message) => 4 },\n",
        "}\n",
        "fn pair_score(error: types.PairError) -> i64 = match error {\n",
        "  Both(left, right) => left.len() + right.len(),\n",
        "  Empty => 90,\n",
        "}\n",
        "fn main() -> i32 = (score(types.fail(true)) + score(types.fail(false)) + pair_score(types.pair_error())) as i32\n",
    );
    let files = [("types.align", types), ("main.align", main)];
    let whole = build_and_run_multi("owned-tagged-whole", &files, "main.align");
    assert_eq!(whole.status.code(), Some(16));
    let per_unit = build_per_unit_multi("owned-tagged-per-unit", &files, "main.align");
    assert_eq!(per_unit.link_and_run().status.code(), Some(16));
}

#[test]
fn generic_sum_recomputes_recursive_move_plan_after_substitution() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Inner { name: string }\n",
        "Maybe<T> { Value(T), Empty }\n",
        "fn main() -> i32 {\n",
        "  value := Maybe.Value(Inner { name: \"owned\".clone() })\n",
        "  return match value { Value(inner) => inner.name.len() as i32, Empty => 90 }\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("generic-recursive-move-plan", src)
            .status
            .code(),
        Some(5)
    );
}

#[test]
fn direct_move_payload_construction_consumes_its_source() {
    let src = concat!(
        "NativeError { code: Option<string>, message: string }\n",
        "DbError { Native(NativeError), Decode(string) }\n",
        "fn main() -> i32 {\n",
        "  native := NativeError { code: Some(\"7\".clone()), message: \"native\".clone() }\n",
        "  err := DbError.Native(native)\n",
        "  return native.message.len() as i32\n",
        "}\n",
    );
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "direct-move-payload-source.align", src);
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(checked.diags.has_errors(), "sum construction must consume a Move struct source");
    assert!(
        rendered.contains("use of moved value 'native'"),
        "diagnostic must name the consumed payload source:\n{rendered}"
    );
}

#[test]
fn recursive_tagged_drop_reaches_an_opaque_move_handle_leaf() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "import std.http\n",
        "Payload { Response(response_builder), Empty }\n",
        "fn main() -> i32 {\n",
        "  value := Some(Payload.Response(http.response(204)))\n",
        "  return 0\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("recursive-tagged-handle-leaf", src)
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn move_sum_payload_may_contain_another_move_sum() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Outer { InnerValue(Inner), Other }\n",
        "Inner { Data(string), Empty }\n",
        "fn main() -> i32 {\n",
        "  outer := Outer.InnerValue(Inner.Data(\"owned\".clone()))\n",
        "  return match outer {\n",
        "    InnerValue(inner) => match inner { Data(value) => value.len() as i32, Empty => 90 }\n",
        "    Other => 91\n",
        "  }\n",
        "}\n",
    );
    assert_eq!(
        build_and_run("nested-move-sum-payload", src)
            .status
            .code(),
        Some(5)
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
