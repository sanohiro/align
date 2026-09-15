//! Total UTF-8 boundary inspection and existing slicing failure semantics.
mod common;
use common::*;

#[test]
fn text_boundary_matches_unicode_oracle_in_both_compilation_modes() {
    if !backend_available() {
        return;
    }
    let library = "module boundaries\npub fn query(text: str, index: i64) -> bool = text.is_char_boundary(index)\npub fn generic<T>(value: T, text: str, index: i64) -> bool = text.is_char_boundary(index)\n";
    let mut body = String::new();
    for (ordinal, text) in ["", "ascii", "Aéあ😀Ā", "\0x"].iter().enumerate() {
        let literal = text.replace('\0', "\\0");
        body.push_str(&format!("s{ordinal} := \"{literal}\".clone()\n"));
        for offset in (-1..=i64::try_from(text.len()).unwrap() + 1).chain([i64::MIN, i64::MAX]) {
            let expected = usize::try_from(offset)
                .ok()
                .is_some_and(|index| text.is_char_boundary(index));
            body.push_str(&format!("if boundaries.query(s{ordinal}, {offset}) != {expected} {{ return 1 }}\nif boundaries.generic(7, s{ordinal}, {offset}) != {expected} {{ return 2 }}\n"));
        }
    }
    body.push_str("if !\"あ\".clone().is_char_boundary(3) { return 3 }\nreturn 0\n");
    let main = format!("import boundaries\nfn main() -> i32 {{\n{body}}}\n");
    let files = [("boundaries.align", library), ("main.align", main.as_str())];
    for out in [
        build_and_run_multi("text-boundary-whole", &files, "main.align"),
        build_per_unit_multi("text-boundary-units", &files, "main.align").link_and_run(),
    ] {
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn text_boundary_receiver_and_index_order_and_termination() {
    if !backend_available() {
        return;
    }
    let source = "fn text() -> string { print(1); return \"あ\".clone() }\nfn index() -> i64 { print(2); return 1 }\nfn early() -> i64 { x := text().is_char_boundary({ return 7 }); return 9 }\nfn main() { print(text().is_char_boundary(index())); print(early()) }\n";
    for per_unit in [false, true] {
        let out = if per_unit {
            build_per_unit_multi(
                "text-boundary-order-unit",
                &[("main.align", source)],
                "main.align",
            )
            .link_and_run()
        } else {
            build_and_run("text-boundary-order", source)
        };
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\nfalse\n1\n7\n");
    }
}

#[test]
fn text_boundary_exact_types_and_owned_field_borrow() {
    let valid = "R { text: string }\nfn inspect(borrow r: R, i: i64) -> bool = r.text.is_char_boundary(i)\nfn choose(text: str, flag: bool) -> bool = text.is_char_boundary(if flag { 0 } else { text.len() })\n";
    let invalid = [
        "fn f(text: str, index: u64) -> bool = text.is_char_boundary(index)",
        "fn f(text: str, index: i32) -> bool = text.is_char_boundary(index)",
        "fn f(text: str) -> bool = text.is_char_boundary()",
        "fn f(text: str) -> bool = text.is_char_boundary(0, 1)",
        "fn f(text: str) -> bool = text.is_char_boundary(false)",
        "fn f(text: str) -> i64 = text.is_char_boundary(0)",
        "fn f(text: slice<u8>) -> bool = text.is_char_boundary(0)",
    ];
    for (source, error) in
        std::iter::once((valid, false)).chain(invalid.into_iter().map(|s| (s, true)))
    {
        for per_unit in [false, true] {
            let mut sm = SourceMap::new();
            let diags = if per_unit {
                check_per_unit(&mut sm, "boundary.align", source).diags
            } else {
                check(&mut sm, "boundary.align", source).diags
            };
            assert_eq!(
                diags.has_errors(),
                error,
                "{source}: {}",
                align_driver::format_diagnostics(&sm, &diags)
            );
        }
    }
    if backend_available() {
        let source = format!(
            "{valid}\nfn main() -> i32 {{ r := R {{ text: \"あ\".clone() }}; if !inspect(r, 3) {{ return 1 }}; if inspect(r, 1) {{ return 2 }}; return 0 }}\n"
        );
        let out = build_and_run("text-boundary-field", &source);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn text_boundary_lowering_is_guarded_and_slicing_still_traps() {
    if !backend_available() {
        return;
    }
    let edge = "fn probe(text: str) -> bool = text.is_char_boundary(text.len())\n";
    let ir = emit_llvm_optimized(edge, &["probe"]);
    assert!(!ir.contains("load i8"), "{ir}");
    assert!(!ir.contains("@align_rt_"), "{ir}");
    let query = "fn probe(text: str, index: i64) -> bool = text.is_char_boundary(index)\n";
    let ir = emit_llvm_optimized(query, &["probe"]);
    assert!(ir.contains("load i8"), "{ir}");
    assert!(ir.contains("br i1"), "{ir}");
    assert!(!ir.contains("@align_rt_"), "{ir}");
    let slice = "fn main() { text := \"あ\"; print(text[0..1]) }\n";
    let out = build_and_run("text-boundary-existing-trap", slice);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
}

#[test]
fn text_boundary_checks_live_observations_before_scalar_cutoff() {
    for (body, invalid) in [
        ("print(text.is_char_boundary({ alias[0] = 255; -1 }))", true),
        (
            "answer := text.is_char_boundary(0); alias[0] = 255; print(answer)",
            false,
        ),
        ("alias[0] = 255; print(text.is_char_boundary(0))", true),
    ] {
        let source = format!(
            "fn main() {{ owner := [(65 as u8), (66 as u8)].to_array(); mut alias: slice<u8> := owner; text := alias.as_str() else {{ return }}; {body} }}\n"
        );
        for per_unit in [false, true] {
            let mut sm = SourceMap::new();
            let diags = if per_unit {
                check_per_unit(&mut sm, "boundary-observation.align", &source).diags
            } else {
                check(&mut sm, "boundary-observation.align", &source).diags
            };
            assert_eq!(
                diags.has_errors(),
                invalid,
                "{source}: {}",
                align_driver::format_diagnostics(&sm, &diags)
            );
        }
    }
}

#[test]
fn text_boundary_temporary_owner_is_freed_on_success_and_early_return() {
    if !backend_available() {
        return;
    }
    let source = r#"
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn early() -> i32 { answer := "あ".clone().is_char_boundary({ return 7 }); return 9 }
fn main() -> i32 {
  before_alloc := unsafe { align_rt_alloc_count() }
  before_free := unsafe { align_rt_free_count() }
  if "あ".clone().is_char_boundary(1) { return 1 }
  if early() != 7 { return 2 }
  if unsafe { align_rt_alloc_count() } - before_alloc != 2 { return 3 }
  if unsafe { align_rt_free_count() } - before_free != 2 { return 4 }
  return 0
}
"#;
    for per_unit in [false, true] {
        let out = if per_unit {
            build_per_unit_multi(
                "boundary-cleanup-unit",
                &[("main.align", source)],
                "main.align",
            )
            .link_and_run()
        } else {
            build_and_run("boundary-cleanup", source)
        };
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn text_boundary_control_results_and_callable_effects() {
    if !backend_available() { return; }
    let source = r#"
fn keep(error: i64) -> i64 = error
fn query(text: str, index: i64) -> bool = text.is_char_boundary(index)
fn result_index(text: str, input: Result<i64, i64>) -> Result<bool, i64> = Ok(text.is_char_boundary(input.map_err(keep)?))
fn main() -> i32 {
  text := "あ"
  some: Option<i64> := Some(3)
  none: Option<i64> := None
  if !text.is_char_boundary(match some { Some(i) => i None => 1 }) { return 1 }
  if !text.is_char_boundary(none else { 0 }) { return 2 }
  if !text.is_char_boundary(loop { break 3 }) { return 3 }
  mut index: i64 := 0
  loop { if index == 4 { break }; expected := index == 0 || index == 3; if text.is_char_boundary(index) != expected { return 4 }; index = index + 1 }
  good: Result<i64, i64> := Ok(3)
  bad: Result<i64, i64> := Err(9)
  match result_index(text, good) { Ok(b) => { if !b { return 5 } } Err(e) => { return 6 } }
  match result_index(text, bad) { Ok(b) => { return 7 } Err(e) => { if e != 9 { return 8 } } }
  callable := query
  if !callable(text, 3) { return 9 }
  values := [0, 1, 2, 3].map(fn i { text.is_char_boundary(i) }).to_array()
  if !values[0] || values[1] || values[2] || !values[3] { return 10 }
  return 0
}
"#;
    for per_unit in [false, true] {
        let out = if per_unit {
            build_per_unit_multi("boundary-control-unit", &[("main.align", source)], "main.align").link_and_run()
        } else { build_and_run("boundary-control", source) };
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    }
}
