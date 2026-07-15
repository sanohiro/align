//! Arena-free template ownership. Dynamic `template` / `json.encode` results are `str` views over
//! path-local synthetic string owners; static-only templates fold to pooled string literals.

mod common;
use common::*;

fn mir_text(src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "template-ownership.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    align_mir::print::program_to_string(&lower_to_mir(&checked.hir))
}

fn optimized_llvm(src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "template-ownership-ir.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    emit_llvm_ir(
        &lower_to_mir(&checked.hir),
        BuildTarget::Baseline,
        true,
        &[],
        false,
    )
    .expect("optimized LLVM IR")
}

#[test]
fn static_only_template_is_a_pooled_literal() {
    let src = "fn text() -> str = template \"hello\"\nfn main() -> i32 = text().len() as i32\n";
    let mir = mir_text(src);
    assert!(mir.contains("\"hello\""), "literal missing:\n{mir}");
    assert!(
        !mir.contains("template("),
        "static template must fold before MIR:\n{mir}"
    );
    if backend_available() {
        let ir = optimized_llvm(src);
        assert!(
            !ir.contains("@align_rt_builder_new"),
            "static template must not allocate:\n{ir}"
        );
        assert_eq!(build_and_run("static-template", src).status.code(), Some(5));
    }
}

#[test]
fn arena_free_template_uses_an_owned_synthetic_lifetime() {
    let src = r#"
fn main() -> i32 {
  mut i := 0
  mut total := 0
  loop {
    text := template "item={i}"
    total = total + text.len() as i32
    i = i + 1
    if i >= 20000 { break }
  }
  return total % 251
}
"#;
    let mir = mir_text(src);
    assert!(
        mir.contains("template["),
        "dynamic template missing:\n{mir}"
    );
    assert!(
        mir.contains("drop _"),
        "synthetic string owner must be dropped:\n{mir}"
    );
    if backend_available() {
        let ir = optimized_llvm(src);
        assert!(
            ir.contains("call void @align_rt_free("),
            "owned template bytes must be freed:\n{ir}"
        );
        assert!(
            !ir.contains("@align_rt_builder_finish"),
            "arena-free finish must not leak a str:\n{ir}"
        );
        assert_eq!(
            build_and_run("owned-template-loop", src).status.code(),
            Some(138)
        );
    }
}

#[test]
fn arena_free_json_encode_uses_the_same_owned_path() {
    let src = r#"
import core.json
Row { id: i64, ok: bool }
fn main() -> i32 {
  row := Row { id: 7, ok: true }
  text := json.encode(row)
  print(text)
  return text.len() as i32
}
"#;
    if backend_available() {
        let ir = optimized_llvm(src);
        assert!(
            ir.contains("call void @align_rt_free("),
            "encoded bytes must have an owner:\n{ir}"
        );
        assert!(
            !ir.contains("@align_rt_builder_finish"),
            "arena-free encode must not use the leaking finish:\n{ir}"
        );
        let out = build_and_run("owned-json-encode", src);
        assert_eq!(out.status.code(), Some(18));
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "{\"id\":7,\"ok\":true}\n"
        );
    }
}

#[test]
fn arena_free_dynamic_template_view_cannot_escape() {
    let diagnostics = check_diagnostics(
        "template-frame-escape",
        "fn bad(n: i64) -> str = template \"n={n}\"\nfn main() -> i32 = 0\n",
    );
    assert!(
        diagnostics.contains("cannot return") || diagnostics.contains("cannot escape"),
        "an owned template view must not outlive its hidden owner:\n{diagnostics}"
    );
}

#[test]
fn arena_free_template_can_be_consumed_inside_a_pipeline_lambda() {
    let src = r#"
fn main() -> i32 {
  total := [1, 20, 300].reduce(0, fn acc, x {
    text := template "n={x}"
    acc + text.len()
  })
  return total as i32
}
"#;
    let mir = mir_text(src);
    assert!(mir.contains("template["), "lambda template missing:\n{mir}");
    assert!(
        mir.contains("drop _"),
        "lambda-local owner must be dropped:\n{mir}"
    );
    if backend_available() {
        let ir = optimized_llvm(src);
        assert!(
            ir.contains("call void @align_rt_free("),
            "lambda template bytes must be freed:\n{ir}"
        );
        assert!(
            !ir.contains("@align_rt_builder_finish"),
            "lambda template must not leak:\n{ir}"
        );
        assert_eq!(
            build_and_run("owned-template-lambda", src).status.code(),
            Some(12)
        );
    }
}

#[test]
fn question_in_a_template_hole_cleans_up_before_the_builder_exists() {
    let src = r#"
fn value(ok: bool) -> Result<i64, Error> {
  if ok { return Ok(7) }
  return Err(Error.Invalid)
}
fn render(ok: bool) -> Result<i64, Error> {
  text := template "v={value(ok)?}"
  return Ok(text.len())
}
fn main() -> Result<(), Error> {
  render(false)?
  return Ok(())
}
"#;
    let mir = mir_text(src);
    assert!(mir.contains("template["), "success edge must still build the template:\n{mir}");
    assert!(mir.contains("drop _"), "both result paths must include owner cleanup:\n{mir}");
    if backend_available() {
        assert_eq!(build_and_run("owned-template-question", src).status.code(), Some(2));
    }
}
