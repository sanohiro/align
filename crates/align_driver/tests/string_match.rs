//! Exact string-literal patterns in `match` (#1085).

mod common;
use common::*;

#[test]
fn str_and_string_match_exact_decoded_bytes() {
    if !backend_available() {
        return;
    }
    let src = r#"
fn classify(s: str) -> i32 = match s {
  "" => 1,
  "é" => 2,
  "a\0b" | "other" => 3,
  _ => 9,
}

fn classify_owned(s: string) -> i32 {
  result := match s {
    "owned" => 4,
    _ => 8,
  }
  return result + s.len() as i32
}

fn main() -> i32 {
  if classify("") != 1 { return 11 }
  if classify("é") != 2 { return 12 }
  if classify("a\0b") != 3 { return 13 }
  if classify("miss") != 9 { return 14 }
  owned := "owned".clone()
  if classify_owned(owned) != 9 { return 15 }
  return 0
}
"#;
    let out = build_and_run("string-match-exact", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn scrutinee_runs_once_and_temporary_survives_dispatch() {
    if !backend_available() {
        return;
    }
    let src = r#"
fn observed() -> str {
  print("scrutinee")
  return "hit"
}

fn main() -> i32 {
  first := match observed() {
    "hit" => 20,
    _ => 1,
  }
  second := match "temporary".clone() {
    "temporary" => 22,
    _ => 2,
  }
  print(first + second)
  return 0
}
"#;
    let out = build_and_run("string-match-once", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "scrutinee\n42\n");
}

#[test]
fn temporary_owner_is_dropped_after_selection_before_the_arm() {
    let source = r#"
fn choose() -> i32 = match "temporary".clone() {
  "temporary" => { return 7 },
  _ => 0,
}
fn main() -> i32 = 0
"#;
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "string-match-cleanup", source);
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let rendered = align_mir::print::program_to_string(&mir);
    let function = rendered
        .split("fn main()")
        .next()
        .expect("choose MIR");
    assert_eq!(function.matches("str_clone(").count(), 1, "{function}");
    assert_eq!(function.matches("drop _").count(), 2, "each selected edge owns one static Drop site:\n{function}");
    let dispatch = function.find("str_match ").expect("string dispatch");
    let first_drop = function.find("drop _").expect("temporary drop");
    let selected_return = function.rfind("return 7_i32").expect("selected early return");
    assert!(dispatch < first_drop && first_drop < selected_return, "cleanup must follow selection and precede the arm return:\n{function}");
}

#[test]
fn diagnostics_are_domain_specific_and_non_cascading() {
    let missing = check_diagnostics(
        "string-match-missing-wildcard",
        "fn f(s: str) -> i32 = match s { \"x\" => 1 }\nfn main() -> i32 = 0\n",
    );
    assert_eq!(missing.matches("non-exhaustive `match` on 'str': missing `_` wildcard").count(), 1, "{missing}");

    let duplicate = check_diagnostics(
        "string-match-duplicate",
        "fn f(s: str) -> i32 = match s { \"A\" | \"\\u{41}\" => 1, _ => 0 }\nfn main() -> i32 = 0\n",
    );
    assert_eq!(duplicate.matches("duplicate string pattern in `match`").count(), 1, "{duplicate}");

    let string_range = check_diagnostics(
        "string-match-inclusive-range",
        "fn f(s: str) -> i32 = match s { \"a\"..=\"z\" => 1, _ => 0 }\nfn main() -> i32 = 0\n",
    );
    assert_eq!(string_range.matches("string range patterns are not supported").count(), 1, "{string_range}");

    let half_open = check_diagnostics(
        "string-match-half-open-range",
        "fn f(s: str) -> i32 = match s { \"a\"..\"z\" => 1, _ => 0 }\nfn main() -> i32 = 0\n",
    );
    assert_eq!(half_open.matches("half-open range patterns (`..`) are not supported").count(), 1, "{half_open}");
    assert!(!half_open.contains("string range patterns are not supported"), "{half_open}");

    let mixed = check_diagnostics(
        "string-match-mixed-domain",
        "fn text(s: str) -> i32 = match s { 1 => 1, _ => 0 }\nfn number(n: i32) -> i32 = match n { \"x\" => 1, _ => 0 }\nfn main() -> i32 = 0\n",
    );
    assert_eq!(mixed.matches("found integer literal").count(), 1, "{mixed}");
    assert_eq!(mixed.matches("found string literal").count(), 1, "{mixed}");
}

#[test]
fn four_cases_use_length_and_byte_dispatch_without_string_copies() {
    let src = r#"
pub fn classify(s: str) -> i32 = match s {
  "same-a" => 1,
  "same-b" => 2,
  "same-c" => 3,
  "other!" => 4,
  _ => 0,
}
fn main() -> i32 = 0
"#;
    let ir = emit_llvm_with_exports(src, &["classify"]);
    assert!(ir.matches("switch i64").count() >= 1, "missing length switch:\n{ir}");
    assert!(ir.matches("switch i8").count() >= 1, "missing byte switch:\n{ir}");
    assert!(!ir.contains("call ptr @align_rt_str_clone"), "dispatch cloned its scrutinee:\n{ir}");
    assert!(!ir.contains("call ptr @align_rt_alloc("), "dispatch allocated:\n{ir}");
}

fn dispatch_corpus_source() -> String {
    let mut source = String::new();
    for count in [4usize, 8, 29] {
        source.push_str(&format!("fn classify_{count}(s: str) -> i32 = match s {{\n"));
        for index in 0..count {
            source.push_str(&format!("  \"shared-{index:02}\" => {},\n", index + 1));
        }
        source.push_str("  _ => 0,\n}\n");
    }
    source.push_str("fn main() -> i32 {\n");
    for count in [4usize, 8, 29] {
        for index in 0..count {
            source.push_str(&format!(
                "  if classify_{count}(\"shared-{index:02}\") != {} {{ return 1 }}\n",
                index + 1
            ));
        }
        source.push_str(&format!("  if classify_{count}(\"missing\") != 0 {{ return 2 }}\n"));
    }
    source.push_str("  return 0\n}\n");
    source
}

#[test]
fn four_eight_and_twenty_nine_case_trees_hit_every_target_and_miss() {
    if !backend_available() {
        return;
    }
    let source = dispatch_corpus_source();
    let out = build_and_run("string-match-dispatch-corpus", &source);
    assert_eq!(out.status.code(), Some(0));
    let ir = emit_llvm(&source);
    assert!(ir.matches("switch i64").count() >= 3, "each large match needs a length switch:\n{ir}");
    assert!(ir.matches("switch i8").count() >= 3, "each equal-length group needs byte dispatch:\n{ir}");
}

#[test]
fn imported_generic_string_match_agrees_whole_program_and_per_unit() {
    let files = &[
        (
            "classify.align",
            "module classify\npub fn text<T>(s: str, ignored: T) -> i32 = match s { \"yes\" => 7, _ => 3 }\n",
        ),
        (
            "main.align",
            "import classify\nfn main() -> i32 = classify.text(\"yes\", true) - 7\n",
        ),
    ];
    let checked = diff_check_multi("string-match-imported-generic", files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let whole = build_and_run_multi("string-match-imported-generic-whole", files, "main.align");
        let per_unit = build_per_unit_multi("string-match-imported-generic-unit", files, "main.align");
        assert_eq!(whole.status.code(), Some(0));
        assert_eq!(per_unit.link_and_run().status.code(), Some(0));
    }
}
