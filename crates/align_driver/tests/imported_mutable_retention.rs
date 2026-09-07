//! Requests 43/49: exact mutable-retention facts cross validated module interfaces.
mod common;
use common::*;

#[test]
fn independent_outputs_and_cloned_local_operands_cross_modules() {
    let helper = r#"module helper
pub Cols { count: i64, ids: array<i64> }
pub Status { text: string }
fn values(n: i64) -> array<i64> {
  mut b: array_builder<i64> := array_builder()
  b.push(n)
  return b.build()
}
fn inner(borrow mut tokens: Cols) { tokens = Cols { count: 1, ids: values(1) } }
pub fn fill(borrow mut tokens: Cols, borrow mut schedule: Cols) {
  inner(tokens)
  schedule = Cols { count: 1, ids: values(3) }
}
pub fn set(borrow mut status: Status, text: str) { status = Status { text: text.clone() } }
"#;
    let main = r#"module main
import helper
fn forward(borrow mut status: helper.Status) {
  n := 42
  local := template "value={n}"
  helper.set(status, local)
}
fn empty() -> array<i64> {
  mut b: array_builder<i64> := array_builder()
  return b.build()
}
fn main() -> i32 {
  mut tokens := helper.Cols { count: 0, ids: empty() }
  mut schedule := helper.Cols { count: 0, ids: empty() }
  helper.fill(tokens, schedule)
  if schedule.ids[0] != 3 { return 1 }
  if tokens.ids[0] != 1 { return 2 }
  mut status := helper.Status { text: "".clone() }
  forward(status)
  if status.text != "value=42" { return 3 }
  return 0
}
"#;
    let files = [("main.align", main), ("helper.align", helper)];
    let checked = diff_check_multi("imported-independent-outputs", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        for output in [
            build_and_run_multi("independent-outputs-whole", &files, "main.align"),
            build_per_unit_multi("independent-outputs-units", &files, "main.align").link_and_run(),
        ] {
            assert!(
                output.status.success(),
                "status {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn imported_retention_preserves_all_returning_control_paths() {
    for (name, body) in [
        ("direct", "dst = value"),
        ("if", "if flag { dst = value }"),
        ("early", "if flag { return }; dst = value"),
        (
            "match",
            "_ := match Some(flag) { Some(_) => { dst = value } None => {} }",
        ),
        ("loop", "loop { dst = value; break }"),
        ("forward", "inner(dst, value)"),
    ] {
        let helper = format!(
            "module helper\nfn inner(borrow mut dst: str, value: str) {{ dst = value }}\npub fn install(borrow mut dst: str, value: str, flag: bool) {{ {body} }}\n"
        );
        let main = r#"module main
import helper
fn main() -> i32 {
  mut destination := "old"
  arena {
    n := 42
    short := template "short={n}"
    helper.install(destination, short, true)
  }
  if destination == "old" { return 1 }
  return 0
}
"#;
        let checked = diff_check_multi(
            &format!("imported-retention-{name}"),
            &[("main.align", main), ("helper.align", &helper)],
            "main.align",
        );
        for diagnostics in [&checked.whole_diags, &checked.per_unit_diags] {
            assert!(
                diagnostics.contains("shorter-lived") || diagnostics.contains("longer-lived"),
                "{name}: {diagnostics}"
            );
        }
    }
}

#[test]
fn imported_replacement_preserves_possible_heap_backing() {
    for (name, body, valid) in [
        ("unchanged", "", true),
        ("heap", "dst = [4, 5].to_array()", false),
        (
            "early",
            "if keep { return }; dst = [4, 5].to_array()",
            false,
        ),
        ("branch", "if keep { dst = [4, 5].to_array() }", false),
        (
            "region",
            "mut b: array_builder<i64> := array_builder(out); b.push(7); dst = b.build()",
            true,
        ),
    ] {
        let helper = format!(
            "module helper\npub fn replace(borrow mut dst: array<i64>, keep: bool, out: region) {{ {body} }}\n"
        );
        let main = r#"module main
import helper
fn make(out: region) -> slice<i64> {
  mut builder: array_builder<i64> := array_builder(out)
  builder.push(1)
  mut dst := builder.build()
  helper.replace(dst, false, out)
  return dst[..]
}
fn main() -> i32 = 0
"#;
        // An unchanged exclusive call still ends old views. Read its current owner for the
        // positive control rather than promising a newly returned view across that boundary.
        let main = if name == "unchanged" {
            main.replace("-> slice<i64>", "-> i64")
                .replace("return dst[..]", "return dst[0]")
        } else {
            main.to_string()
        };
        let checked = diff_check_multi(
            &format!("imported-replacement-{name}"),
            &[("main.align", &main), ("helper.align", &helper)],
            "main.align",
        );
        assert_eq!(
            checked.whole_errors, !valid,
            "{name} whole: {}",
            checked.whole_diags
        );
        assert_eq!(
            checked.per_unit_errors, !valid,
            "{name} imported: {}",
            checked.per_unit_diags
        );
        if !valid {
            for diagnostics in [&checked.whole_diags, &checked.per_unit_diags] {
                assert!(diagnostics.contains("local"), "{name}: {diagnostics}");
            }
        }
    }
}

#[test]
fn imported_generic_and_recursive_forwarding_use_body_inference() {
    let helper = r#"module helper
pub fn put<T>(borrow mut dst: T, value: T) { dst = value }
fn right(borrow mut dst: str, value: str, n: i64) {
  if n > 0 { left(dst, value, n - 1) } else { dst = value }
}
pub fn left(borrow mut dst: str, value: str, n: i64) {
  if n > 0 { right(dst, value, n - 1) } else { dst = value }
}
"#;
    let main = r#"module main
import helper
fn main() -> i32 {
  mut n := 1
  helper.put(n, 3)
  mut text := "old"
  helper.left(text, "new", 4)
  if n == 3 && text == "new" { return 0 }
  return 1
}
"#;
    let files = [("main.align", main), ("helper.align", helper)];
    let checked = diff_check_multi(
        "imported-retention-generics-recursion",
        &files,
        "main.align",
    );
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole: {}\nimported: {}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let output = build_per_unit_multi(
            "imported-retention-generics-recursion-run",
            &files,
            "main.align",
        )
        .link_and_run();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn indirect_mutable_calls_keep_the_unavailable_summary_fallback() {
    let helper = "module helper\npub fn ignore(borrow mut dst: str, ignored: str) {}\n";
    let main = r#"module main
import helper
fn apply(borrow mut destination: str) {
  call := helper.ignore
  arena {
    n := 42
    short := template "short={n}"
    call(destination, short)
  }
}
fn main() -> i32 = 0
"#;
    let checked = diff_check_multi(
        "indirect-retention-fallback",
        &[("main.align", main), ("helper.align", helper)],
        "main.align",
    );
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "whole: {}\nimported: {}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    for diagnostics in [&checked.whole_diags, &checked.per_unit_diags] {
        assert!(diagnostics.contains("shorter-lived"), "{diagnostics}");
    }
}
