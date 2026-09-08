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

#[test]
fn retained_local_views_reject_across_helper_boundaries() {
    let sources = [
        ("buffer", "data.bytes()"),
        ("alias", "{ alias := data.bytes(); alias }"),
        ("subslice", "data.bytes()[..]"),
        ("if", "if flag { data.bytes() } else { seed }"),
        ("if-reversed", "if flag { seed } else { data.bytes() }"),
        ("match", "match Some(flag) { Some(_) => data.bytes(), None => seed }"),
        ("loop", "loop { break data.bytes() }"),
        ("nested-loop", "loop { break loop { break data.bytes() } }"),
        ("loop-branch", "loop { if flag { break seed }; break data.bytes() }"),
        ("loop-record", "{ record := loop { break helper.Holder { view: data.bytes() } }; record.view }"),
        ("optional", "Some(data.bytes()) else seed"),
        ("result", "{ result: Result<slice<u8>, Error> := Ok(data.bytes()); result else seed }"),
        ("try", "{ result: Result<slice<u8>, Error> := Ok(data.bytes()); result? }"),
        ("map-error", "{ result: Result<slice<u8>, Error> := Ok(data.bytes()); result.map_err(identity_error)? }"),
        ("generic", "helper.identity(data.bytes())"),
        ("string", "text.bytes()"),
        ("record", "{ holder := helper.Holder { view: data.bytes() }; holder.view }"),
    ];
    for mode in ["", "borrow "] {
        for (name, source) in sources {
            let helper = format!(r#"module helper
pub Holder {{ view: slice<u8> }}
pub fn retain(borrow mut owner: Holder, {mode}bytes: slice<u8>) {{ owner.view = bytes }}
pub fn identity<T>(value: T) -> T = value
"#);
            let main = format!(r#"import helper
fn identity_error(error: Error) -> Error = error
fn caller(borrow mut owner: helper.Holder, seed: slice<u8>, flag: bool) -> Result<(), Error> {{
  mut data := buffer(1)
  data.put_u8(65)
  text := "local".clone()
  bytes := {source}
  helper.retain(owner, bytes)
  return Ok(())
}}
fn main() {{}}
"#);
            let checked = diff_check_multi(
                &format!("retained-view-{mode}-{name}"),
                &[("helper.align", &helper), ("main.align", &main)],
                "main.align",
            );
            for diagnostics in [&checked.whole_diags, &checked.per_unit_diags] {
                let rejected = diagnostics.contains("shorter-lived")
                    || name.contains("loop") && diagnostics.contains("cannot `break`");
                assert!(rejected, "{mode}{name}: {diagnostics}");
            }
            let longer_lived = main.replace("data.bytes()", "seed").replace("text.bytes()", "seed");
            let checked = diff_check_multi(
                &format!("retained-view-control-{mode}-{name}"),
                &[("helper.align", &helper), ("main.align", &longer_lived)],
                "main.align",
            );
            assert!(!checked.whole_errors && !checked.per_unit_errors,
                "{mode}{name} caller-backed control: {}\n{}", checked.whole_diags, checked.per_unit_diags);
        }
    }
}

#[test]
fn retained_views_preserve_destination_and_argument_completion_lifetimes() {
    let helper = r#"module helper
pub fn put<T>(borrow mut dst: T, value: T) { dst = value }
pub fn install(borrow mut dst: slice<u8>, value: slice<u8>, ignored: i64) { dst = value }
pub fn buffer_view(borrow mut dst: slice<u8>, borrow data: buffer) { dst = data.bytes() }
"#;
    for (name, body, valid) in [
        ("local-destination", "mut data := buffer(1); mut dst := seed; helper.put(dst, data.bytes())", true),
        ("caller-destination", "mut data := buffer(1); helper.put(destination, data.bytes())", false),
        ("borrowed-buffer", "mut data := buffer(1); helper.buffer_view(destination, data)", false),
        ("borrowed-buffer-local", "mut data := buffer(1); mut dst := seed; helper.buffer_view(dst, data)", true),
        ("arena", "arena { n := 42; text := template \"value={n}\"; helper.put(destination, text.bytes()) }", false),
        ("completion", "mut data := buffer(1); mut bytes := data.bytes(); helper.install(destination, bytes, { bytes = seed; 0 })", false),
        ("known-join", "values := [65 as u8].to_array(); bytes := if flag { values[..] } else { seed }; helper.put(destination, bytes)", false),
        ("known-join-reversed", "values := [65 as u8].to_array(); bytes := if flag { seed } else { values[..] }; helper.put(destination, bytes)", false),
        ("arena-join", "arena out { mut builder: array_builder<u8> := array_builder(out); builder.push(65 as u8); values := builder.build(); bytes := if flag { values[..] } else { seed }; helper.put(destination, bytes) }", false),
        ("arena-join-reversed", "arena out { mut builder: array_builder<u8> := array_builder(out); builder.push(65 as u8); values := builder.build(); bytes := if flag { seed } else { values[..] }; helper.put(destination, bytes) }", false),
        ("arena-destination", "arena out { mut builder: array_builder<u8> := array_builder(out); builder.push(65 as u8); values := builder.build(); bytes := if flag { values[..] } else { seed }; mut dst: slice<u8> := []; helper.put(dst, bytes) }", true),
    ] {
        let main = format!("import helper\nfn caller(borrow mut destination: slice<u8>, seed: slice<u8>, flag: bool) {{ {body} }}\nfn main() {{}}\n");
        let checked = diff_check_multi(
            &format!("retained-view-lifetime-{name}"),
            &[("helper.align", helper), ("main.align", &main)],
            "main.align",
        );
        assert_eq!(checked.whole_errors, !valid, "{name}: {}", checked.whole_diags);
        assert_eq!(checked.per_unit_errors, !valid, "{name}: {}", checked.per_unit_diags);
        if !valid {
            for diagnostics in [&checked.whole_diags, &checked.per_unit_diags] {
                assert!(diagnostics.contains("shorter-lived"), "{name}: {diagnostics}");
            }
        }
    }
}

#[test]
fn retained_loop_views_ignore_nonreturning_break_edges() {
    let helper = "module helper\npub Holder { view: slice<u8> }\npub fn retain(borrow mut owner: Holder, bytes: slice<u8>) { owner.view = bytes }\n";
    for (name, source) in [
        ("return", "loop { if flag { break seed }; return; break data.bytes() }"),
        ("if", "loop { if flag { break seed } else { return }; break data.bytes() }"),
        ("match", "loop { match Some(flag) { Some(_) => { break seed }, None => { return } }; break data.bytes() }"),
        ("operand", "loop { if flag { break seed }; break { return; data.bytes() } }"),
        ("nested", "loop { if flag { break loop { break seed } }; loop {}; break data.bytes() }"),
        ("record", "{ value := loop { break helper.Holder { view: seed } }; value.view }"),
        ("reevaluation", "loop { mut i := 0; loop { _ := loop { break seed }; i = i + 1; if i == 2 { break } }; break seed }"),
    ] {
        let main = format!(r#"import helper
fn caller(borrow mut owner: helper.Holder, seed: slice<u8>, flag: bool) {{
  mut data := buffer(1)
  bytes := {source}
  helper.retain(owner, bytes)
}}
fn main() {{}}
"#);
        let checked = diff_check_multi(
            &format!("retained-loop-reachability-{name}"),
            &[("helper.align", helper), ("main.align", &main)],
            "main.align",
        );
        assert!(!checked.whole_errors && !checked.per_unit_errors,
            "{name}: {}\n{}", checked.whole_diags, checked.per_unit_diags);
    }
}
