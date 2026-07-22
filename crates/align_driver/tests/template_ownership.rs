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

/// An owned `string` hole **borrows**: the MIR must read the local's `{ptr,len}` for the piece and
/// leave the local's ownership alone, so exactly one `drop` frees it — at scope end, after the
/// template and after every later use. (Before the fix this never reached MIR's ownership question
/// at all: the hole became an `IntHole` and codegen panicked.)
#[test]
fn an_owned_string_hole_borrows_and_is_dropped_once() {
    let src = r#"
import std.encoding
fn main() -> Result<(), Error> {
  h := encoding.hex_encode("ab".bytes())
  print(template "v={h}")
  print(h.len())
  return Ok(())
}
"#;
    let mir = mir_text(src);
    assert!(mir.contains("str("), "the hole must render through the str piece:\n{mir}");
    // One template + one owned `string` local, so exactly two owners are dropped: the hidden
    // template owner and `h` itself. A moved-out hole would have nulled `h` (`_0 <- null`) or
    // dropped it twice.
    assert_eq!(
        mir.lines().filter(|l| l.trim_start().starts_with("drop _")).count(),
        2,
        "the interpolated string must be freed exactly once, and only by its own scope:\n{mir}"
    );
    assert!(
        !mir.contains("null"),
        "interpolation must not move (null) the owned string:\n{mir}"
    );
    if backend_available() {
        let out = build_and_run("owned-string-hole", src);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "v=6162\n4\n");
    }
}

/// A fresh owned `string` interpolated inside a `loop` gets MIR's hidden owner, freed on **every**
/// loop edge — the temporary neither leaks per iteration nor outlives the pass that made it.
#[test]
fn an_owned_string_hole_temporary_is_freed_each_iteration() {
    let src = r#"
import std.encoding
fn main() -> Result<(), Error> {
  mut i := 0
  mut n := 0
  loop {
    line := template "v={encoding.hex_encode(\"ab\".bytes())}"
    n = n + line.len()
    i = i + 1
    if i >= 3 { break }
  }
  print(n)
  return Ok(())
}
"#;
    let mir = mir_text(src);
    assert!(mir.contains("encode_Hex"), "the temporary producer must be lowered:\n{mir}");
    // Break edge + back edge: the hidden owner of the temporary is dropped on both, alongside the
    // template's own owner.
    assert!(
        mir.lines().filter(|l| l.trim_start().starts_with("drop _")).count() >= 3,
        "a per-iteration temporary hole needs its own hidden-owner drops:\n{mir}"
    );
    if backend_available() {
        let ir = optimized_llvm(src);
        assert!(
            ir.contains("call void @align_rt_free("),
            "the interpolated temporary must be freed:\n{ir}"
        );
        let out = build_and_run("owned-string-hole-loop", src);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "18\n");
    }
}

/// Every *other* aggregate / Move type in a hole is rejected with a real diagnostic at the sema
/// boundary — never a codegen panic. `print`'s own path is checked by the same classification, so
/// it is swept here too.
#[test]
fn an_unprintable_hole_is_a_diagnostic_not_a_codegen_panic() {
    let cases: &[(&str, &str, &str)] = &[
        ("array", "fn main() -> i32 {\n  a := [1, 2, 3].to_array()\n  print(template \"{a}\")\n  return 0\n}\n", "array<i64>"),
        ("slice", "fn main() -> i32 {\n  a := [1, 2, 3]\n  s := a[0..2]\n  print(template \"{s}\")\n  return 0\n}\n", "slice<i64>"),
        ("buffer", "fn main() -> i32 {\n  b := buffer(4)\n  print(template \"{b}\")\n  return 0\n}\n", "buffer"),
        ("builder", "fn main() -> i32 {\n  b := builder()\n  print(template \"{b}\")\n  return 0\n}\n", "builder"),
        ("array_builder", "fn main() -> i32 {\n  mut b: array_builder<i64> := array_builder()\n  print(template \"{b}\")\n  return 0\n}\n", "array_builder<i64>"),
        ("reader", "import std.io\nfn main() -> Result<(), Error> {\n  r := io.stdin\n  print(template \"{r}\")\n  return Ok(())\n}\n", "reader"),
        ("struct", "P { x: i64 }\nfn main() -> i32 {\n  p := P { x: 1 }\n  print(template \"{p}\")\n  return 0\n}\n", "struct"),
        ("tuple", "fn main() -> i32 {\n  t := (1, 2)\n  print(template \"{t}\")\n  return 0\n}\n", "tuple"),
        ("option", "fn main() -> i32 {\n  o := Some(3)\n  print(template \"{o}\")\n  return 0\n}\n", "Option<i64>"),
        ("enum", "C { R, G }\nfn main() -> i32 {\n  c := C.R\n  print(template \"{c}\")\n  return 0\n}\n", "enum"),
        ("unit", "fn u() {}\nfn main() -> i32 {\n  print(template \"{u()}\")\n  return 0\n}\n", "()"),
        ("fn-value", "fn g(x: i64) -> i64 = x\nfn main() -> i32 {\n  f := g\n  print(template \"{f}\")\n  return 0\n}\n", "fn"),
    ];
    for (name, src, want) in cases {
        let diagnostics = check_diagnostics(&format!("template-hole-{name}"), src);
        assert!(
            diagnostics.contains("a template hole must be an int, float, str, bool, or char"),
            "a `{name}` hole must be rejected in sema:\n{diagnostics}"
        );
        assert!(
            diagnostics.contains(want),
            "the `{name}` diagnostic must name the offending type:\n{diagnostics}"
        );
    }
    // `print` classifies its argument with the same `print_kind`, so its unsupported set matches.
    for (name, src) in [
        ("array", "fn main() -> i32 {\n  a := [1, 2, 3].to_array()\n  print(a)\n  return 0\n}\n"),
        ("struct", "P { x: i64 }\nfn main() -> i32 {\n  p := P { x: 1 }\n  print(p)\n  return 0\n}\n"),
        ("buffer", "fn main() -> i32 {\n  b := buffer(4)\n  print(b)\n  return 0\n}\n"),
        ("unit", "fn u() {}\nfn main() -> i32 {\n  print(u())\n  return 0\n}\n"),
    ] {
        let diagnostics = check_diagnostics(&format!("print-arg-{name}"), src);
        assert!(
            diagnostics.contains("'print' expects an int, float, str, bool, or char"),
            "`print({name})` must be rejected in sema:\n{diagnostics}"
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
