//! align-LLM runway A2 — binary decode/encode on `bytes`/`buffer`. Bounds-checked, endian-explicit
//! scalar reads `bytes.<scalar>_<le|be>(off)` and the matching growable-`buffer` writes
//! `buf.put_<scalar>_<le|be>(v)` / `buf.append(data)`. GGUF-class header parsing and
//! `alignpack`/`alignidx` emission are the consumers. Completion: encode→hex round trip across
//! every width + both endians (incl. floats), decode back with correct signedness, out-of-range
//! reads abort (same policy as `slice[i]`), and the negative diagnostics (immutable receiver, wrong
//! receiver/value type). (`docs/open-questions.md` Open → "align-LLM runway" A2; `draft.md` §18.2.)

mod common;
use common::*;

fn explicit_export_core_body<'a>(ir: &'a str, name: &str) -> &'a str {
    let hex = name
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let symbol = format!("align_fn${}${hex}", name.len());
    let header = ir
        .find(&format!("@\"{symbol}\"("))
        .unwrap_or_else(|| panic!("missing specialized core {symbol}:\n{ir}"));
    let body = ir[header..]
        .find("{\n")
        .map(|offset| header + offset + 2)
        .unwrap_or_else(|| panic!("missing specialized core body {symbol}:\n{ir}"));
    let end = ir[body..]
        .find("\n}")
        .map(|offset| body + offset)
        .unwrap_or_else(|| panic!("unterminated specialized core {symbol}:\n{ir}"));
    &ir[body..end]
}

#[test]
fn constructor_termination_stops_later_operands_and_allocation() {
    if !backend_available() { return; }
    for body in [
        "b := buffer.filled({ return 7 }, side()); return b.len()",
        "b := buffer.filled(3, { return 7 }); return b.len()",
        "mut b: array_builder<i64> := array_builder({ return 7 }); xs := b.build(); return xs.len()",
        "arena out { mut b: array_builder<i64> := array_builder(out, { return 7 }); xs := b.build(); return xs.len() }",
    ] {
        let source = format!("fn side() -> u8 {{ print(99); return 1 }}\nfn early() -> i64 {{ {body} }}\nfn main() {{ print(early()) }}\n");
        for per_unit in [false, true] {
            let out = if per_unit {
                build_per_unit_multi("constructor-termination-unit", &[("main.align", source.as_str())], "main.align").link_and_run()
            } else { build_and_run("constructor-termination", &source) };
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
        }
    }
}

#[test]
fn filled_buffer_initialization_growth_and_operand_order() {
    if !backend_available() { return; }
    let source = "fn size() -> i64 { print(1); return 7 }\nfn byte() -> u8 { print(2); return 165 }\nfn main() -> i32 { mut empty := buffer.filled(0, 255); if empty.len() != 0 { return 1 }; mut b := buffer.filled(size(), byte()); if b.len() != 7 { return 2 }; mut i := 0; loop { if i >= 7 { break }; if b.bytes()[i] != 165 { return 4 }; i = i + 1 }; b.put_u8(33); if b.len() != 8 { return 5 }; if b.bytes()[7] != 33 { return 6 }; return 0 }\n";
    for per_unit in [false, true] {
        let out = if per_unit {
            build_per_unit_multi("filled-buffer-unit", &[("main.align", source)], "main.align").link_and_run()
        } else { build_and_run("filled-buffer", source) };
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n");
    }
    for count in ["-1", "9223372036854775807"] {
        let out = build_and_run("filled-buffer-invalid", &format!("fn main() {{ b := buffer.filled({count}, 0); print(b.len()) }}"));
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn float_inspection_exact_bits_and_classification() {
    if !backend_available() { return; }
    let mut library = String::from("module inspection\n");
    let mut calls = String::new();
    for width in [32, 64] {
        library.push_str(&format!(
            "pub fn bits{width}(value: f{width}) -> u{width} = value.to_bits()\n\
             pub fn finite{width}(value: f{width}) -> bool = value.is_finite()\n\
             pub fn nan{width}(value: f{width}) -> bool = value.is_nan()\n\
             pub fn infinite{width}(value: f{width}) -> bool = value.is_infinite()\n"
        ));
        let patterns: &[u64] = if width == 32 {
            &[0, 0x80000000, 1, 0x007fffff, 0x00800000, 0x3fc00000, 0x7f7fffff,
              0x7f800000, 0xff800000, 0x7f800001, 0x7fc01234, 0xffc01234]
        } else {
            &[0, 0x8000000000000000, 1, 0x000fffffffffffff, 0x0010000000000000,
              0x3ff8000000000000, 0x7fefffffffffffff, 0x7ff0000000000000,
              0xfff0000000000000, 0x7ff0000000000001, 0x7ff8123456789abc,
              0xfff8123456789abc]
        };
        for (index, &bits) in patterns.iter().enumerate() {
            let (finite, nan, infinite) = if width == 32 {
                let value = f32::from_bits(u32::try_from(bits).expect("f32 pattern"));
                (value.is_finite(), value.is_nan(), value.is_infinite())
            } else {
                let value = f64::from_bits(bits);
                (value.is_finite(), value.is_nan(), value.is_infinite())
            };
            calls.push_str(&format!(
                "  mut b{width}_{index} := buffer(8)\n\
                 b{width}_{index}.put_u{width}_le({bits})\n\
                 v{width}_{index} := b{width}_{index}.bytes().f{width}_le(0)\n\
                 if inspection.bits{width}(v{width}_{index}) != {bits} {{ return 1 }}\n\
                 if inspection.finite{width}(v{width}_{index}) != {finite} {{ return 2 }}\n\
                 if inspection.nan{width}(v{width}_{index}) != {nan} {{ return 3 }}\n\
                 if inspection.infinite{width}(v{width}_{index}) != {infinite} {{ return 4 }}\n"
            ));
        }
    }
    let caller = format!("import inspection\nfn main() -> i32 {{\n{calls} return 0\n}}\n");
    let files = [("inspection.align", library.as_str()), ("main.align", caller.as_str())];
    let whole = build_and_run_multi("float-inspection-whole", &files, "main.align");
    assert_eq!(whole.status.code(), Some(0), "{}", String::from_utf8_lossy(&whole.stderr));
    let units = build_per_unit_multi("float-inspection-units", &files, "main.align");
    let out = units.link_and_run();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let src = library.trim_start_matches("module inspection\n").replace("pub fn", "fn");
    let exports = ["bits32", "bits64", "finite32", "finite64", "nan32", "nan64", "infinite32", "infinite64"];
    for (optimized, ir) in [(false, emit_llvm_with_exports(&src, &exports)), (true, emit_llvm_optimized(&src, &exports))] {
        assert!(ir.contains("bitcast float") && ir.contains("bitcast double"), "{ir}");
        // LLVM may recognize bit classification as register-only fabs/comparison intrinsics.
        for export in exports {
            for line in explicit_export_core_body(&ir, export)
                .lines()
                .filter(|line| line.contains("call "))
            {
                assert!(optimized && line.contains("@llvm."), "{line}");
            }
        }
        if optimized {
            assert!(!ir.contains("alloca "), "{ir}");
        }
    }
}

#[test]
fn float_inspection_inference_and_receiver_matrix() {
    let valid = [
        "fn f() -> u64 = (1.5).to_bits()",
        "fn f() -> u32 = (1.5).to_bits()",
        "fn f() -> u32 { x := 1.5; bits := x.to_bits(); y: f32 := x; return bits }",
        "fn f() -> u64 { x := 1.5; a := x.to_bits(); b := x.to_bits(); return a | b }",
        "R { x: f32 }\nfn f(r: R) -> u32 = (r.x).to_bits()",
        "fn value() -> f32 = 1.5\nfn f() -> bool = value().is_finite()",
        "fn f() -> bool = (1.5).is_nan()",
        "fn f() -> u32 { x := 1.5; bits := x.to_bits(); values := [1, 2].map(fn v { v }).to_array(); y: f32 := x; return bits }",
        "fn f() -> u64 { x := 1.5; bits := x.to_bits(); values := [1, 2].map(fn v { bits }).to_array(); return values[0] }",
        "fn f() -> u32 { x: f32 := 1.5; values := [1, 2].map(fn v { x.to_bits() }).to_array(); return values[0] }",
        "fn f() -> u64 { x := 1.5; values := [x.to_bits()].map(fn v { v }).to_array(); return values[0] }",
    ];
    let invalid = [
        "fn f(x: f64) -> u32 = x.to_bits()",
        "fn f(x: f32) -> i32 = x.to_bits()",
        "fn f(x: i32) -> bool = x.is_finite()",
        "fn f(x: bool) -> bool = x.is_nan()",
        "fn f(x: f32) -> u32 = x.to_bits(1)",
        "fn f(x: vec4<f32>) -> bool = x.is_finite()",
    ];
    for (sources, errors) in [(valid.as_slice(), false), (invalid.as_slice(), true)] {
        for src in sources {
            for per_unit in [false, true] {
                let mut sm = SourceMap::new();
                let diags = if per_unit {
                    check_per_unit(&mut sm, "float-method.align", src).diags
                } else {
                    check(&mut sm, "float-method.align", src).diags
                };
                assert_eq!(diags.has_errors(), errors, "{src}: {}", align_driver::format_diagnostics(&sm, &diags));
            }
        }
    }
}

/// Encoding every width and both byte orders into a growable buffer, then hex-encoding its bytes,
/// produces the exact byte layout — LE reverses, BE keeps source order, `u8` has no endian tag.
#[test]
fn encode_all_widths_and_endians_to_hex() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  mut b := buffer(0)
  b.put_u8(0x41)
  b.put_u16_le(0x1234)
  b.put_u16_be(0x1234)
  b.put_u32_le(0xDEADBEEF)
  b.put_u32_be(0xDEADBEEF)
  b.put_u64_le(0x0102030405060708)
  print(encoding.hex_encode(b.bytes()))
  return Ok(())
}
";
    let out = build_and_run("a2-encode-hex", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "4134121234efbeaddedeadbeef0807060504030201\n"
    );
}

/// A full encode→read-back round trip within one buffer: put scalars of several widths/endians,
/// then read them at their offsets through `b.bytes()`. Values (incl. a float) survive exactly.
#[test]
fn round_trip_put_then_read() {
    if !backend_available() {
        return;
    }
    let prog = "\
pub fn main() -> Result<(), Error> {
  mut b := buffer(0)
  b.put_u32_le(305419896)
  b.put_u32_be(305419896)
  b.put_f64_le(3.5)
  b.put_f32_be(1.5)
  bv := b.bytes()
  print(bv.u32_le(0))
  print(bv.u32_be(4))
  print(bv.f64_le(8))
  print(bv.f32_be(16))
  return Ok(())
}
";
    let out = build_and_run("a2-round-trip", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "305419896\n305419896\n3.5\n1.5\n"
    );
}

#[test]
fn checked_typed_byte_view_round_trip_and_failures() {
    if !backend_available() {
        return;
    }
    let program = r#"
fn read_word(raw: slice<u8>) -> i64 {
  words: slice<u32> := raw.view_le() else { return -1 }
  back := words.as_bytes()
  if back.len() != raw.len() { return -2 }
  return words[0] as i64
}

fn main() -> i32 {
  mut aligned := buffer(0)
  aligned.put_u32_le(305419896)
  if read_word(aligned.bytes()) != 305419896 { return 1 }

  mut short := buffer(0)
  short.put_u16_le(7)
  if read_word(short.bytes()) != -1 { return 2 }

  mut shifted := buffer(0)
  shifted.put_u8(0)
  shifted.put_u32_le(9)
  if read_word(shifted.bytes()[1..5]) != -1 { return 3 }
  return 0
}
"#;
    for per_unit in [false, true] {
        let output = if per_unit {
            build_per_unit_multi("checked-byte-view-unit", &[("main.align", program)], "main.align")
                .link_and_run()
        } else {
            build_and_run("checked-byte-view", program)
        };
        assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    }

    let library = r#"module views
pub fn read_word(raw: slice<u8>) -> i64 {
  words: slice<u32> := raw.view_le() else { return -1 }
  return words[0] as i64
}
"#;
    let caller = r#"import views
fn main() -> i32 {
  mut bytes := buffer(0)
  bytes.put_u32_le(305419896)
  if views.read_word(bytes.bytes()) != 305419896 { return 1 }
  return 0
}
"#;
    let output = build_per_unit_multi(
        "checked-byte-view-interface",
        &[("views.align", library), ("main.align", caller)],
        "main.align",
    )
    .link_and_run();
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn checked_typed_byte_views_reach_vector_loads_and_stores() {
    if !backend_available() {
        return;
    }
    for (label, elem, lanes, llvm_elem, increment) in [
        ("f32x2", "f32", 2, "float", "1.0"),
        ("f32x4", "f32", 4, "float", "1.0"),
        ("f32x8", "f32", 8, "float", "1.0"),
        ("f32x16", "f32", 16, "float", "1.0"),
        ("i32x4", "i32", 4, "i32", "1"),
    ] {
        let source = format!(
            "fn transform(out raw: slice<u8>) -> {elem} {{\n  mut values: slice<{elem}> := raw.view_le() else {{ return 0 as {elem} }}\n  lanes: vec{lanes}<{elem}> := values.load(0)\n  values.store(0, lanes + {increment})\n  return lanes[0]\n}}\n"
        );
        let ir = emit_llvm_with_exports(&source, &["transform"]);
        assert!(ir.contains(&format!("load <{lanes} x {llvm_elem}>")), "{label}: {ir}");
        assert!(ir.contains(&format!("store <{lanes} x {llvm_elem}>")), "{label}: {ir}");
        assert!(!ir.contains("llvm.memcpy"), "{label}: conversion copied payload: {ir}");
    }
}

#[test]
fn checked_typed_byte_views_are_descriptor_only_in_llvm() {
    if !backend_available() {
        return;
    }
    let source = "fn view(raw: slice<u8>) -> Option<slice<u64>> = raw.view_le()\n\
                  fn inverse(values: slice<u64>) -> slice<u8> = values.as_bytes()\n";
    let ir = emit_llvm_with_exports(source, &["view", "inverse"]);
    let view = explicit_export_core_body(&ir, "view");
    assert!(view.contains("ptrtoint ptr"), "{view}");
    assert!(view.contains("urem i64"), "{view}");
    assert!(view.contains("udiv i64"), "{view}");
    assert!(!view.contains("@align_rt_"), "{view}");
    assert!(!view.contains("memcpy"), "{view}");

    let inverse = explicit_export_core_body(&ir, "inverse");
    assert!(inverse.contains("@llvm.umul.with.overflow.i64"), "{inverse}");
    assert!(!inverse.contains("@align_rt_"), "{inverse}");
    assert!(!inverse.contains("memcpy"), "{inverse}");
}

#[test]
fn checked_typed_byte_view_runtime_predicates_cover_the_closed_domain() {
    if !backend_available() {
        return;
    }
    let domain = [
        ("u16", 2), ("u32", 4), ("u64", 8),
        ("i16", 2), ("i32", 4), ("i64", 8),
        ("f32", 4), ("f64", 8),
    ];
    let mut source = String::new();
    for (elem, _) in domain {
        source.push_str(&format!(
            "fn view_{elem}(raw: slice<u8>) -> i64 {{ values: slice<{elem}> := raw.view_le() else {{ return -1 }}; return values.len() }}\n"
        ));
    }
    source.push_str("fn main() -> i32 {\n");
    for (index, (elem, width)) in domain.into_iter().enumerate() {
        let failure = index * 4 + 1;
        source.push_str(&format!(
            "  mut exact_{index} := buffer.filled({width}, 0)\n\
             if view_{elem}(exact_{index}.bytes()) != 1 {{ return {failure} }}\n\
             if view_{elem}(exact_{index}.bytes()[0..0]) != 0 {{ return {} }}\n\
             mut uneven_{index} := buffer.filled({}, 0)\n\
             if view_{elem}(uneven_{index}.bytes()) != -1 {{ return {} }}\n\
             if view_{elem}(uneven_{index}.bytes()[1..1]) != -1 {{ return {} }}\n",
            failure + 1,
            width + 1,
            failure + 2,
            failure + 3,
        ));
    }
    source.push_str("  return 0\n}\n");
    let output = build_and_run("checked-byte-view-domain", &source);
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn checked_typed_byte_view_inference_and_domain_diagnostics() {
    let valid = [
        "fn f(raw: slice<u8>) -> i64 { xs: slice<u16> := raw.view_le() else { return 0 }; return xs.len() }",
        "fn f(xs: slice<f64>) -> slice<u8> = xs.as_bytes()",
        "fn f(raw: slice<u8>) -> Option<slice<i32>> = raw.view_le()",
        "fn f(raw: slice<u8>) -> i64 { xs: Option<slice<i64>> := raw.view_le(); return 0 }",
        "fn use(xs: slice<u16>) -> i64 = xs.len()\nfn f(raw: slice<u8>) -> i64 = use(raw.view_le() else { return 0 })",
        "fn identity(raw: slice<u8>) -> slice<u8> = raw\nfn f(raw: slice<u8>) -> Option<slice<u32>> = identity(raw).view_le()",
        "Packet { raw: slice<u8> }\nfn f(p: Packet) -> Option<slice<u32>> = p.raw.view_le()",
        "fn f(a: slice<u8>, b: slice<u8>, first: bool) -> Option<slice<u32>> = (if first { a } else { b }).view_le()",
        "fn main() -> i32 { mut b := buffer(0); b.put_u32_le(1); mut xs: slice<u32> := b.bytes().view_le() else { return 1 }; xs[0] = 9; return b.bytes().u32_le(0) as i32 }",
    ];
    let invalid = [
        "fn f(raw: slice<u8>) { x := raw.view_le() }",
        "fn f(raw: slice<u8>) -> Option<slice<u8>> = raw.view_le()",
        "fn f(raw: slice<u8>) -> Option<slice<bool>> = raw.view_le()",
        "fn f(xs: slice<u8>) -> slice<u8> = xs.as_bytes()",
        "fn f(xs: slice<u32>) -> slice<u8> = xs.as_bytes(1)",
        "fn f(xs: i64) -> slice<u8> = xs.as_bytes()",
        "fn f() { mut xs: slice<u32> := \"abcd\".bytes().view_le() else { return }; xs[0] = 1 }",
        "fn f() { xs: slice<u32> := \"abcd\".bytes().view_le() else { return }; mut raw := xs.as_bytes(); raw.set_u8(0, 1) }",
        "fn f() -> slice<u8> { mut b := buffer(0); b.put_u32_le(1); xs: slice<u32> := b.bytes().view_le() else { return []; }; return xs.as_bytes() }",
    ];
    for (sources, errors) in [(valid.as_slice(), false), (invalid.as_slice(), true)] {
        for source in sources {
            for per_unit in [false, true] {
                let mut source_map = SourceMap::new();
                let diagnostics = if per_unit {
                    check_per_unit(&mut source_map, "checked-byte-view.align", source).diags
                } else {
                    check(&mut source_map, "checked-byte-view.align", source).diags
                };
                assert_eq!(
                    diagnostics.has_errors(),
                    errors,
                    "{source}: {}",
                    align_driver::format_diagnostics(&source_map, &diagnostics)
                );
            }
        }
    }
}
/// Reading is signedness-aware: the byte `0xff` reads as `255` through `u8` but `-1` through `i8`;
/// a little-endian `u16` and a big-endian `u16` of the same two bytes differ. Decode source is a
/// `buffer` from `hex_decode` (the encoding-domain producer), viewed as `bytes`.
#[test]
fn decode_signedness_and_endianness() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  dec := encoding.hex_decode(\"ff0102\")?
  bv := dec.bytes()
  print(bv.u8(0))
  print(bv.i8(0))
  print(bv.u16_le(1))
  print(bv.u16_be(1))
  return Ok(())
}
";
    let out = build_and_run("a2-decode-signed", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // 0xff -> 255 / -1; bytes[1..3] = 01 02 -> LE 0x0201=513, BE 0x0102=258.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "255\n-1\n513\n258\n");
}

/// `buf.append(data)` copies a raw `bytes`/`str` blob onto the buffer after existing content,
/// growing it — the raw-blob complement to the typed `put_*` writers.
#[test]
fn append_raw_blob_after_puts() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  mut b := buffer(0)
  b.put_u16_be(0x4869)
  b.append(\"!!\")
  print(encoding.hex_encode(b.bytes()))
  return Ok(())
}
";
    let out = build_and_run("a2-append", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "48692121\n");
}

/// Appending a buffer's own bytes is a copy, even when doubling the payload forces the backing Vec
/// to reallocate. The source view must be snapshotted before that growth so it cannot dangle.
#[test]
fn append_own_bytes_survives_reallocation() {
    if !backend_available() {
        return;
    }
    const PAYLOAD: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let prog = "\
pub fn main() -> Result<(), Error> {
  mut b := buffer(0)
  b.append(\"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\")
  own := b.bytes()
  b.append(own)
  print(b.bytes().as_str()?)
  return Ok(())
}
";
    let out = build_and_run("a2-append-self", prog);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{PAYLOAD}{PAYLOAD}\n")
    );
}

/// An out-of-range read aborts (the offset+width exceeds the view length) — the same fail-closed
/// policy as `slice[i]`, so a parser must check `.len()` first. The process exits non-zero.
#[test]
fn read_past_end_aborts() {
    if !backend_available() {
        return;
    }
    let prog = "\
pub fn main() -> Result<(), Error> {
  mut b := buffer(0)
  b.put_u8(1)
  bv := b.bytes()
  print(bv.u32_le(0))
  return Ok(())
}
";
    let out = build_and_run("a2-oob-read", prog);
    assert_ne!(out.status.code(), Some(0), "an out-of-range binary read must abort");
}

/// A negative offset aborts too (the range check's `start < 0` arm).
#[test]
fn negative_offset_aborts() {
    if !backend_available() {
        return;
    }
    let prog = "\
pub fn main() -> Result<(), Error> {
  mut b := buffer(0)
  b.put_u32_le(7)
  bv := b.bytes()
  print(bv.u8(-1))
  return Ok(())
}
";
    let out = build_and_run("a2-neg-offset", prog);
    assert_ne!(out.status.code(), Some(0), "a negative binary-read offset must abort");
}

// --- negative diagnostics ----------------------------------------------------------------------

/// An immutable `buffer` cannot be grown — `put_*` requires a `mut` binding.
#[test]
fn put_on_immutable_buffer_is_an_error() {
    let prog = "\
pub fn main() -> Result<(), Error> {
  b := buffer(0)
  b.put_u8(1)
  return Ok(())
}
";
    let errs = check_diagnostics("a2-immutable-put", prog);
    assert!(errs.contains("immutable buffer") || errs.contains("mut"), "diagnostics: {errs}");
}

/// A binary read is only valid on a `bytes` (`slice<u8>`) view — not on a scalar.
#[test]
fn read_on_non_bytes_is_an_error() {
    let prog = "\
pub fn main() -> Result<(), Error> {
  x := 5
  print(x.u32_le(0))
  return Ok(())
}
";
    let errs = check_diagnostics("a2-read-nonbytes", prog);
    assert!(errs.contains("bytes") || errs.contains("slice<u8>"), "diagnostics: {errs}");
}

/// The value handed to `put_*` must match the writer's scalar type exactly (no silent coercion).
#[test]
fn put_value_type_mismatch_is_an_error() {
    let prog = "\
pub fn main() -> Result<(), Error> {
  mut b := buffer(0)
  b.put_u32_le(1.5)
  return Ok(())
}
";
    let errs = check_diagnostics("a2-put-mismatch", prog);
    assert!(errs.contains("expects a u32") || errs.contains("u32"), "diagnostics: {errs}");
}

#[test]
fn bounded_byte_object_numeric_matrix() {
    if !backend_available() { return; }
    let mut src = String::new();
    let mut calls = String::new();
    let mut expected = String::new();
    for (index, (ty, val, want)) in [
        ("u16", "4660", "13330"), ("i16", "-2", "-257"),
        ("u32", "305419896", "2018915346"), ("i32", "-2", "-16777217"),
        ("u64", "72623859790382856", "578437695752307201"),
        ("i64", "-2", "-72057594037927937"),
    ].into_iter().enumerate() {
        for endian in ["le", "be"] {
            let opposite = if endian == "le" { "be" } else { "le" };
            let name = format!("word_{index}_{endian}");
            // The prefix forces unaligned, mixed-width storage. The opposite-endian read is
            // checked against an independent byte-reversal oracle, not a same-codec round trip.
            src.push_str(&format!("fn {name}(x: {ty}) -> {ty} {{\n mut b := buffer(0)\n b.put_u8(7)\n b.put_{ty}_{endian}(x)\n return b.bytes().{ty}_{opposite}(1)\n}}\n"));
            calls.push_str(&format!(" print({name}({val}))\n"));
            expected.push_str(&format!("{want}\n"));
        }
    }
    for (ty, input) in [("i8", "-2"), ("u8", "254")] {
        src.push_str(&format!("fn byte_{ty}(x: {ty}) -> {ty} {{ mut b := buffer(1); b.put_{ty}(x); return b.bytes().{ty}(0) }}\n"));
        calls.push_str(&format!(" print(byte_{ty}({input}))\n"));
        expected.push_str(&format!("{input}\n"));
    }
    for bits in [32, 64] {
        src.push_str(&format!("fn float_{bits}(x: u{bits}) -> u{bits} {{\n mut a := buffer(0)\n a.put_u{bits}_be(x)\n number := a.bytes().f{bits}_be(0)\n mut b := buffer(0)\n b.put_f{bits}_le(number)\n return b.bytes().u{bits}_le(0)\n}}\n"));
        let patterns: &[u64] = if bits == 32 { &[0, 0x80000000, 0x7f800000, 0xff800000, 0x7fc01234, 0x7f801234] }
            else { &[0, 0x8000000000000000, 0x7ff0000000000000, 0xfff0000000000000, 0x7ff8000000001234, 0x7ff0000000001234] };
        for pattern in patterns {
            calls.push_str(&format!(" print(float_{bits}({pattern}))\n"));
            let printed = i64::from_ne_bytes(pattern.to_ne_bytes());
            expected.push_str(&format!("{printed}\n"));
        }
    }
    let ir = emit_llvm(&src);
    assert!(!ir.contains("call ptr @align_rt_buffer_new"), "{ir}");
    assert!(!ir.contains("call void @align_rt_buffer_free"), "{ir}");
    src.push_str(&format!("fn main() -> Result<(), Error> {{\n{calls} return Ok(())\n}}\n"));
    let out = build_and_run("bounded-byte-numeric", &src);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn bounded_byte_object_control_matrix() {
    if !backend_available() { return; }
    let functions = r#"
fn bits(x: f32) -> u32 {
  mut b := buffer(4)
  b.put_f32_le(x)
  return b.bytes().u32_le(0)
}
fn branch(flag: bool) -> u32 {
  mut b := buffer(0)
  if flag { b.put_u32_le(7) } else { b.put_u32_be(9) }
  return b.bytes().u32_le(0)
}
fn repeated(n: i64) -> u64 {
  mut i := 0
  mut total: u64 := 0
  loop {
    if i >= n { break }
    mut b := buffer(8)
    b.put_u64_le(i as u64)
    total = total + b.bytes().u64_le(0)
    i = i + 1
  }
  return total
}
fn literal() -> u16 {
  mut b := buffer(0)
  b.append("AB")
  return b.bytes().u16_be(0)
}
"#;
    let ir = emit_llvm_with_exports(functions, &["bits", "branch", "repeated", "literal"]);
    assert!(!ir.contains("call ptr @align_rt_buffer_new"), "{ir}");
    assert!(!ir.contains("call void @align_rt_buffer_free"), "{ir}");
    let optimized = emit_llvm_optimized("fn bits(x: f32) -> u32 { mut b := buffer(4); b.put_f32_le(x); return b.bytes().u32_le(0) }\n", &["bits"]);
    let bits = explicit_export_core_body(&optimized, "bits");
    assert!(bits.contains("bitcast float"), "{optimized}");
    assert!(!bits.contains("call "), "{optimized}");
    let main = "fn main() -> Result<(), Error> { print(bits(1.5)); print(branch(true)); print(branch(false)); print(repeated(5)); print(literal()); return Ok(()) }\n";
    let out = build_and_run("bounded-byte-control", &format!("{functions}{main}"));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1069547520\n7\n150994944\n10\n16706\n");
    // Interface transport imports the scalar helper, never a trusted storage-selection record.
    let library = format!("module codec\n{}", functions.replace("fn ", "pub fn "));
    let caller = main.replace("bits(", "codec.bits(").replace("branch(", "codec.branch(")
        .replace("repeated(", "codec.repeated(").replace("literal(", "codec.literal(");
    let caller = format!("import codec\n{caller}");
    let unit = build_per_unit_multi("bounded-byte-units", &[("codec.align", &library), ("main.align", &caller)], "main.align");
    let objects = unit.emit_objects_with(Profile::Dev, false);
    let refs: Vec<_> = objects.iter().map(|path| path.as_path()).collect();
    let exe = unit.dir.join("codec-dev");
    link_objects(&align_driver::CDriver::default(), &refs, &exe, &unit.link_libs_union(), Profile::Dev).expect("link dev");
    let output = unit.dir.join("codec-dev.stdout");
    let stdout = std::fs::File::create(&output).expect("create dev stdout");
    struct Running(std::process::Child);
    impl Drop for Running {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }
    let mut child = Running(std::process::Command::new(&exe).stdout(stdout).spawn().expect("spawn dev"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        assert!(std::time::Instant::now() < deadline, "dev child exceeded deadline");
        match child.0.try_wait() {
            Ok(Some(status)) => { assert!(status.success()); break; }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => panic!("poll dev child: {error}"),
        }
    }
    assert_eq!(std::fs::read_to_string(output).expect("read dev stdout"), "1069547520\n7\n150994944\n10\n16706\n");
    let out = unit.link_and_run();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1069547520\n7\n150994944\n10\n16706\n");

    for (offset, success) in [(0, true), (-1, false), (1, false), (i64::MAX, false)] {
        let src = format!("fn read(off: i64) -> u32 {{ mut b := buffer(4); b.put_u32_le(7); return b.bytes().u32_le(off) }}\nfn main() -> i32 {{ print(read({offset})); return 0 }}\n");
        let out = build_and_run(&format!("bounded-read-{offset}"), &src);
        assert_eq!(out.status.success(), success);
        if !success { assert!(String::from_utf8_lossy(&out.stderr).contains("out of bounds")); }
    }
    for (label, body) in [
        ("large", "mut b := buffer(65); b.put_u32_le(x); return b.bytes().u32_le(0)"),
        ("dynamic", "mut b := buffer(x as i64); b.put_u32_le(x); return b.bytes().u32_le(0)"),
        ("mixed", "mut b := buffer(0); if x > 0 { b.put_u8(1) }; b.put_u32_le(x); return b.bytes().u32_le(0)"),
        ("escape", "mut b := buffer(4); b.put_u32_le(x); return consume(b.bytes()).u32_le(0)"),
    ] {
        let ir = emit_llvm_with_exports(&format!("fn consume(xs: slice<u8>) -> slice<u8> = xs\nfn f(x: u32) -> u32 {{ {body} }}\n"), &["f"]);
        assert!(ir.contains("call ptr @align_rt_buffer_new"), "{label}: {ir}");
    }
}

#[test]
fn bounded_byte_object_rejects_forged_put_widths() {
    use align_mir::{Operand, Rvalue, Stmt};
    use align_sema::{FloatTy, IntTy, Ty};
    if !backend_available() { return; }
    for (actual, claimed) in [("u64", "u8"), ("u8", "u64"), ("u64", "f64"), ("f64", "u64"), ("f64", "f32"), ("f32", "f64")] {
        let suffix = if actual == "u8" { "" } else { "_le" };
        let source = format!("fn f(x: {actual}) -> i64 {{ mut b := buffer(0); b.put_{actual}{suffix}(x); return b.len() }}\n");
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, "forged-byte-width", &source);
        assert!(!checked.diags.has_errors());
        let original = lower_to_mir(&checked.hir);
        assert_eq!(align_mir::byte_storage::plan(&original.fns[0], &Default::default()).slots().count(), 1);
        assert!(emit_llvm_ir(&original, BuildTarget::Baseline, align_driver::Profile::Release, false, &[], false).is_ok());
        let scalar = match claimed {
            "u8" => Ty::Int(IntTy { bits: 8, signed: false }),
            "u64" => Ty::Int(IntTy { bits: 64, signed: false }),
            "f32" => Ty::Float(FloatTy { bits: 32 }),
            "f64" => Ty::Float(FloatTy { bits: 64 }),
            _ => unreachable!(),
        };
        let mut mismatched = original.clone();
        let function = &mut mismatched.fns[0];
        let mut input_id = None;
        for block in &mut function.blocks {
            for statement in &mut block.stmts {
                if let Stmt::Let(_, Rvalue::BufferPut { scalar: width, value: Operand::Value(value), .. }) = statement {
                    *width = scalar;
                    input_id = Some(*value);
                }
            }
        }
        assert_eq!(align_mir::byte_storage::plan(function, &Default::default()).slots().count(), 0, "{actual}/{claimed}");
        // A matching declared type cannot authorize a wider store: Load still emits actual.
        function.value_tys[input_id.expect("put input") as usize] = scalar;
        assert_eq!(align_mir::byte_storage::plan(function, &Default::default()).slots().count(), 1, "forged table reaches backend check");
        let error = emit_llvm_ir(&mismatched, BuildTarget::Baseline, align_driver::Profile::Release, false, &[], false).expect_err("actual LLVM width/class must be checked");
        assert!(error.contains("byte storage put operand does not match scalar width"), "{actual}/{claimed}: {error}");
    }

    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "byte-result-types", "fn f(x: u64) -> u64 { mut b := buffer(0); b.put_u64_le(x); b.append(\"ab\"); n := b.len(); return b.bytes().u64_le(n - 10) }\n");
    assert!(!checked.diags.has_errors());
    let program = lower_to_mir(&checked.hir);
    let function = &program.fns[0];
    assert_eq!(align_mir::byte_storage::plan(function, &Default::default()).slots().count(), 1);
    let operations: Vec<_> = function.blocks.iter().flat_map(|block| &block.stmts).filter_map(|stmt| match stmt {
        Stmt::Let(id, Rvalue::BufferNew { .. } | Rvalue::BufferPut { .. } | Rvalue::BufferAppend { .. } | Rvalue::BufferBytes(_) | Rvalue::BufferLen(_)) => Some(*id),
        _ => None,
    }).collect();
    assert_eq!(operations.len(), 5);
    for id in operations {
        let mut bad = function.clone();
        bad.value_tys[id as usize] = Ty::Bool;
        assert_eq!(align_mir::byte_storage::plan(&bad, &Default::default()).slots().count(), 0, "result {id}");
    }
}

#[test]
fn byte_storage_crosses_proved_local_readers_and_unrelated_indexing() {
    if !backend_available() { return; }
    let functions = r#"
fn shared(borrow bytes: slice<u8>) -> u32 = bytes.u32_le(0)
fn copied(bytes: slice<u8>) -> u32 = bytes.u32_le(0)
fn forwarded(borrow bytes: slice<u8>) -> u32 = shared(bytes)
fn leading<T>(unused: T, borrow bytes: slice<u8>) -> u32 = forwarded(bytes)
fn recursive(borrow bytes: slice<u8>, n: i64) -> u32 {
  if n == 0 { return bytes.u32_le(0) }
  return recursive(bytes, n - 1)
}
fn local(x: u32, xs: slice<i64>) -> u32 {
  mut b := buffer(4)
  b.put_u32_le(x)
  bytes := b.bytes()
  return leading(1, bytes) + copied(bytes) + recursive(bytes, 3) + xs[0] as u32
}
"#;
    let ir = emit_llvm_with_exports(functions, &["local"]);
    assert!(!ir.contains("call ptr @align_rt_buffer_new"), "{ir}");
    assert!(!ir.contains("call void @align_rt_buffer_free"), "{ir}");
    let out = build_and_run("local-byte-readers", &format!("{functions}\nfn main() {{ print(local(17, [3])) }}\n"));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "54\n");

    // Returning a Copy view is an escape even though the slice descriptor's
    // argument address is never captured. The backing bytes are the proof root.
    let escaping = r#"
fn identity(bytes: slice<u8>) -> slice<u8> = bytes
fn escaping(x: u32) -> u32 {
  mut b := buffer(4)
  b.put_u32_le(x)
  bytes := identity(b.bytes())
  return bytes.u32_le(0)
}
"#;
    let ir = emit_llvm_with_exports(escaping, &["escaping"]);
    assert!(ir.contains("call ptr @align_rt_buffer_new"), "{ir}");
}

#[test]
fn imported_byte_readers_remain_opaque_to_per_unit_storage_selection() {
    if !backend_available() { return; }
    let library = "module reader\npub fn read(borrow bytes: slice<u8>) -> u32 = bytes.u32_le(0)\n";
    let source = "import reader\nfn use(x: u32) -> u32 { mut b := buffer(4); b.put_u32_le(x); bytes := b.bytes(); return reader.read(bytes) }\nfn main() { print(use(17)) }\n";
    let unit = build_per_unit_multi("opaque-byte-reader", &[("reader.align", library), ("main.align", source)], "main.align");
    let entry = unit.walk.units.iter().find(|unit| unit.is_entry).expect("entry");
    let ir = align_driver::emit_llvm_ir(&entry.mir, BuildTarget::Baseline, Profile::Release, false, &[], false).expect("per-unit byte IR");
    assert!(ir.contains("call ptr @align_rt_buffer_new"), "{ir}");
}

fn reached_byte_guards(function: &align_mir::Function) -> usize {
    use align_mir::{DirectCall, RuntimeKey, Rvalue, Stmt, Term};
    function.blocks.iter().filter(|block| match block.term {
        Term::Branch(_, yes, _) => function.blocks[yes as usize].stmts.iter().any(|stmt|
            matches!(stmt, Stmt::Let(_, Rvalue::Call(DirectCall::Runtime(RuntimeKey::RangeFail), _)))),
        _ => false,
    }).count()
}

#[test]
fn byte_range_recurrence_preserves_tails_and_eliminates_only_proved_guards() {
    if !backend_available() { return; }
    for (scalar, width, reader, put) in [
        ("u8", 1, "u8", "put_u8"),
        ("i8", 1, "i8", "put_i8"),
        ("i16", 2, "i16_le", "put_i16_le"),
        ("i32", 4, "i32_be", "put_i32_be"),
        ("i64", 8, "i64_le", "put_i64_le"),
        ("u16", 2, "u16_be", "put_u16_be"),
        ("u32", 4, "u32_le", "put_u32_le"),
        ("u64", 8, "u64_be", "put_u64_be"),
        ("f32", 4, "f32_le", "put_f32_le"),
        ("f64", 8, "f64_be", "put_f64_be"),
    ] {
        let zero = if scalar.starts_with('f') { "0.0" } else { "0" };
        let one = if scalar.starts_with('f') { "1.0" } else { "1" };
        let two = if scalar.starts_with('f') { "2.0" } else { "2" };
        let kernel = format!("fn sum(src: slice<u8>) -> {scalar} {{ mut result: {scalar} := {zero}; mut i := 0; loop {{ if i >= src.len() / {width} {{ break }}; result = result + src.{reader}(i * {width}); i = i + 1 }}; return result }}\n");
        let mut map = SourceMap::new();
        let checked = check(&mut map, "byte-range-kernel", &kernel);
        assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&map, &checked.diags));
        let mir = lower_to_mir(&checked.hir);
        assert_eq!(reached_byte_guards(&mir.fns[0]), 0, "{scalar}");
        let optimized = emit_llvm_optimized(&kernel, &["sum"]);
        assert!(!optimized.contains("call void @align_rt_range_fail"), "{scalar}: {optimized}");
        // Empty/short input performs no read; a partial final word is excluded by
        // the original floor-divided loop bound, including unaligned views.
        let tail = if width > 1 { "data.put_u8(99);" } else { "" };
        let source = format!("{kernel}\nfn main() {{ mut b := buffer(0); print(sum(b.bytes())); b.put_u8(0); bytes := b.bytes(); print(sum(bytes[1..1])); print(sum(bytes)); mut data := buffer(0); data.put_u8(0); data.{put}({one}); data.{put}({two}); {tail} values := data.bytes(); print(sum(values[1..values.len()])); }}\n");
        let output = build_and_run(&format!("byte-range-{scalar}"), &source);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let numbers: Vec<f64> = String::from_utf8_lossy(&output.stdout).lines().map(|line| line.parse().expect("numeric output")).collect();
        assert_eq!(numbers, [0.0, 0.0, 0.0, 3.0], "{scalar}");
    }
    // An inclusive upper bound reaches an invalid read and must still fail.
    let bad = "fn scan(src: slice<u8>) -> u32 { mut i := 0; mut result: u32 := 0; loop { if i > src.len() / 4 { break }; result = result + src.u32_le(i * 4); i = i + 1 }; return result }\nfn main() { mut b := buffer(0); b.put_u32_le(7); print(scan(b.bytes())) }\n";
    let output = build_and_run("byte-range-inclusive-trap", bad);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("out of bounds"));
}

#[test]
fn byte_range_nonzero_and_paired_recurrences() {
    if !backend_available() { return; }
    for initial in [0, 1] {
        for paired in [false, true] {
            for fallible in [false, true] {
                let offset = if paired { "offset" } else { "i * 4" };
                let offset_init = if paired { format!("mut offset := {};", initial * 4) } else { String::new() };
                let offset_step = if paired { "offset = offset + 4;" } else { "" };
                let ret = if fallible { "Result<f32, Error>" } else { "f32" };
                let validate = if fallible { "if !value.is_finite() { return Err(Error.Invalid) };" } else { "" };
                let result = if fallible { "Ok(total)" } else { "total" };
                let kernel = format!("fn scan(src: slice<u8>) -> {ret} {{ mut total: f32 := 0.0; mut i := {initial}; {offset_init} loop {{ if i >= src.len() / 4 {{ break }}; value := src.f32_le({offset}); {validate} total = total + value; i = i + 1; {offset_step} }}; return {result} }}\n");
                let ir = emit_llvm_optimized(&kernel, &["scan"]);
                assert!(!ir.contains("call void @align_rt_range_fail"), "start={initial}, paired={paired}, fallible={fallible}: {ir}");
                let use_result = if fallible { "scan(b.bytes()) else -1.0" } else { "scan(b.bytes())" };
                let source = format!("{kernel} fn main() {{ mut b := buffer(0); print({use_result}); b.put_f32_le(2.0); b.put_f32_le(3.0); b.put_u8(99); print({use_result}); }}");
                let output = build_and_run("paired-byte-range", &source);
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                let values: Vec<f32> = String::from_utf8_lossy(&output.stdout).lines().map(|line| line.parse().expect("float")).collect();
                assert_eq!(values, [0.0, if initial == 0 { 5.0 } else { 3.0 }]);
            }
        }
    }
}

#[test]
fn paired_byte_ranges_keep_unproved_guards() {
    for (initial, offset, step, limit) in [
        ("0", "4", "offset + 4", "src.len() / 4"),
        ("1", "0", "offset + 4", "src.len() / 4"),
        ("0", "-4", "offset + 4", "src.len() / 4"),
        ("0", "0", "offset + 8", "src.len() / 4"),
        ("0", "0", "offset", "src.len() / 4"),
        ("0", "0", "offset + 4", "src.len() / 2"),
        ("seed", "seed * 4", "offset + 4", "src.len() / 4"),
        ("9223372036854775807", "0", "offset + 4", "src.len() / 4"),
    ] {
        let source = format!("fn scan(src: slice<u8>, seed: i64) -> u32 {{ mut i := {initial}; mut offset := {offset}; mut total: u32 := 0; loop {{ if i >= {limit} {{ break }}; total = total + src.u32_le(offset); i = i + 1; offset = {step} }}; return total }}");
        let mut map = SourceMap::new();
        let checked = check(&mut map, "paired-negative", &source);
        assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&map, &checked.diags));
        let mir = lower_to_mir(&checked.hir);
        assert!(reached_byte_guards(&mir.fns[0]) > 0, "{source}");
    }
}

#[test]
fn byte_range_malformed_and_invalidated_proofs_fail_closed() {
    use align_ast::BinOp;
    use align_mir::{Const, Operand, Rvalue, Stmt, Term};
    use align_sema::{IntTy, Ty};
    let source = "fn sum(src: slice<u8>, other: slice<u8>) -> u32 { mut result: u32 := 0; mut i := 9223372036854775807; loop { if i >= src.len() / 4 { break }; result = result + src.u32_le(i * 4); i = i + 1 }; return result }\n";
    let mut map = SourceMap::new();
    let checked = check(&mut map, "byte-range-proof", source);
    assert!(!checked.diags.has_errors());
    let mut base = lower_to_mir(&checked.hir).fns.remove(0);
    let integer = Ty::Int(IntTy { bits: 64, signed: true });
    // Start beyond the scalable domain to retain the original checked CFG during source lowering,
    // then supply the exact zero-based recurrence to the private MIR pass.
    let mut induction = None;
    for block in &mut base.blocks {
        for statement in &mut block.stmts {
            if let Stmt::Store(slot, Operand::Const(Const::Int(value, ty))) = statement {
                if *ty == integer && *value == i128::from(i64::MAX) { *value = 0; induction = Some(*slot); }
            }
        }
    }
    let induction = induction.expect("induction initialization");
    assert_eq!(reached_byte_guards(&base), 1);
    let mut valid = base.clone();
    let proofs = align_mir::byte_ranges::simplify(&mut valid);
    assert_eq!(proofs.len(), 1, "one committed rewrite returns one proof");
    assert_eq!(reached_byte_guards(&valid), 0);
    // A comparison fact belongs to one branch arm, even when destinations
    // coincide or a rejected arm rejoins the admitted arm later.
    for use_lt in [false, true] {
        let mut oriented = base.clone();
        let header = oriented.blocks.iter().position(|block| block.stmts.iter().any(
            |stmt| matches!(stmt, Stmt::Let(_, Rvalue::Bin(BinOp::Ge, _, _)))
        )).expect("admission header");
        if use_lt {
            for stmt in &mut oriented.blocks[header].stmts {
                if let Stmt::Let(_, Rvalue::Bin(op @ BinOp::Ge, _, _)) = stmt { *op = BinOp::Lt; }
            }
            if let Term::Branch(_, yes, no) = &mut oriented.blocks[header].term {
                std::mem::swap(yes, no);
            }
        }
        let mut positive = oriented.clone();
        let proofs = align_mir::byte_ranges::simplify(&mut positive);
        assert_eq!(proofs.len(), 1, "one admitted arm returns one proof");
        assert_eq!(reached_byte_guards(&positive), 0, "admitted arm: lt={use_lt}");
        for rejoin in [false, true] {
            let mut bad = oriented.clone();
            let Term::Branch(_, yes, no) = bad.blocks[header].term.clone() else { panic!("header branch") };
            let (admitted, rejected) = if use_lt { (yes, no) } else { (no, yes) };
            if rejoin {
                bad.blocks[rejected as usize].term = Term::Goto(admitted);
            } else if let Term::Branch(_, yes, no) = &mut bad.blocks[header].term {
                if use_lt { *no = admitted; } else { *yes = admitted; }
            }
            let proofs = align_mir::byte_ranges::simplify(&mut bad);
            assert!(proofs.is_empty(), "a rejected arm returns no proof");
            assert_eq!(reached_byte_guards(&bad), 1, "rejected arm: lt={use_lt}, rejoin={rejoin}");
        }
    }
    for mutation in 0..10 {
        let mut bad = base.clone();
        let mut changed = false;
        for block in &mut bad.blocks {
            for statement in &mut block.stmts {
                match (mutation, statement) {
                    (0, Stmt::Store(slot, Operand::Const(Const::Int(value, _)))) if *slot == induction => { *value = -1; changed = true; }
                    (1, Stmt::Let(_, Rvalue::Bin(BinOp::Add, _, Operand::Const(Const::Int(value, _))))) if *value == 1 => { *value = 2; changed = true; }
                    (2, Stmt::Let(_, Rvalue::Bin(op @ BinOp::Ge, _, _))) => { *op = BinOp::Gt; changed = true; }
                    (3, Stmt::Let(_, Rvalue::Bin(BinOp::Mul, _, Operand::Const(Const::Int(value, _))))) => { *value = 8; changed = true; }
                    (4, Stmt::Let(_, Rvalue::BytesRead { scalar, .. })) => { *scalar = Ty::Int(IntTy { bits: 64, signed: false }); changed = true; }
                    (5, Stmt::Let(id, Rvalue::Load(slot))) if *slot == induction => { bad.value_tys[*id as usize] = Ty::Int(IntTy { bits: 32, signed: true }); changed = true; }
                    (6, Stmt::Let(_, Rvalue::Bin(BinOp::Div, _, Operand::Const(Const::Int(value, _))))) => { *value = 2; changed = true; }
                    _ => {}
                }
            }
        }
        match mutation {
            7 => { bad.param_modes[0] = align_ast::ParamMode::BorrowMut; changed = true; }
            8 => { let statement = bad.blocks[0].stmts[0].clone(); bad.blocks[0].stmts.push(statement); changed = true; }
            9 => {
                let guard = bad.blocks.iter_mut().find(|block| matches!(block.term, Term::Branch(_, yes, _) if base.blocks[yes as usize].stmts.iter().any(|stmt| matches!(stmt, Stmt::Let(_, Rvalue::Call(_, _)))))).expect("range guard");
                let duplicate = guard.stmts.last().expect("guard condition").clone();
                guard.stmts.push(duplicate); changed = true;
            }
            _ => {}
        }
        assert!(changed, "mutation {mutation} must reach the proof");
        let proofs = align_mir::byte_ranges::simplify(&mut bad);
        assert!(proofs.is_empty(), "mutation {mutation} returns no proof");
        assert_eq!(reached_byte_guards(&bad), 1, "mutation {mutation}");
    }
}

fn composed_sampler() -> &'static str { fixture("crates/align_driver/tests/fixtures/composed_sampler.align") }

fn composed_program(source: &str) -> align_mir::Program {
    let mut map = SourceMap::new();
    let checked = check(&mut map, "composed-byte-owner", source);
    assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&map, &checked.diags));
    lower_to_mir(&checked.hir)
}

#[test]
fn composed_byte_sampler_exposes_both_guards_without_changing_cached_mir() {
    let sampler = composed_sampler();
    use align_mir::{DirectCall, Rvalue, Stmt};
    let original = composed_program(sampler);
    let before = format!("{original:?}");
    let defined = original.fns.iter().map(|f| f.name.clone()).collect();
    let prepared = align_mir::byte_prepare::prepare(&original, &defined);
    let select = prepared.fns.iter().find(|f| f.name.as_str() == "select").expect("sampler");
    assert_eq!(reached_byte_guards(select), 0);
    let descriptor_loads = select.blocks.iter().flat_map(|b| &b.stmts).filter(|s|
        matches!(s, Stmt::Let(_, Rvalue::Load(slot)) if *slot == select.params[0])).count();
    assert_eq!(descriptor_loads, 1, "one reached descriptor snapshot");
    assert!(!select.blocks.iter().flat_map(|b| &b.stmts).any(|s|
        matches!(s, Stmt::Let(_, Rvalue::Call(DirectCall::Program(name), _)) if name.as_str() == "finite_f32")));
    assert_eq!(format!("{original:?}"), before);
    let partition = [select.name.clone()].into_iter().collect();
    let function_only = align_mir::byte_prepare::prepare(&original, &partition);
    let select = function_only.fns.iter().find(|f| f.name.as_str() == "select").expect("partition");
    assert!(select.blocks.iter().flat_map(|b| &b.stmts).any(|s|
        matches!(s, Stmt::Let(_, Rvalue::Call(DirectCall::Program(name), _)) if name.as_str() == "finite_f32")));
    assert!(reached_byte_guards(select) > 0, "opaque peer invalidates loop effects");
    assert_eq!(format!("{original:?}"), before, "Whole then Function uses immutable input");
    if backend_available() {
        let optimized = emit_llvm_optimized(sampler, &["select"]);
        assert!(!optimized.contains("call void @align_rt_range_fail"), "{optimized}");
        assert!(optimized.contains("fpext float"), "source f64 policy remains");
        assert!(optimized.contains("call void @align_rt_bounds_fail"), "candidate-array checks remain");
    }
}

#[test]
fn composed_byte_proofs_accept_named_copies_and_ignore_post_loop_effects() {
    for (label, params, bound, read, after, helper) in [
        ("post", "", "src.len() / 4", "src.u32_le(i * 4)", "mut b: array_builder<u32> := array_builder(); b.push(result); a := b.build(); return a[0]", ""),
        ("mut", ", borrow mut other: i64", "src.len() / 4", "src.u32_le(i * 4)", "return result", ""),
        ("named", "", "n", "src.u32_le(offset)", "return result", ""),
        ("leaf", "", "n", "read(src, offset)", "return result", "fn read<T>(borrow s: slice<u8>, offset: T) -> u32 { return s.u32_le(offset as i64) }"),
    ] {
        let helper = helper.replace("<T>", "").replace("offset: T", "offset: i64").replace("offset as i64", "offset");
        let source = format!("{helper}\nfn sum(borrow src: slice<u8>{params}) -> u32 {{ n := src.len() / 4; mut result: u32 := 0; mut i := 0; loop {{ if i >= {bound} {{ break }}; offset := i * 4; result = result + {read}; i = i + 1 }}; {after} }}\n");
        let original = composed_program(&source);
        let defined = original.fns.iter().map(|f| f.name.clone()).collect();
        let prepared = align_mir::byte_prepare::prepare(&original, &defined);
        let function = prepared.fns.iter().find(|f| f.name.as_str() == "sum").expect("sum");
        assert_eq!(reached_byte_guards(function), 0, "{label}");
    }
}

#[test]
fn composed_sampler_matches_checked_execution_and_rng_advancement() {
    let sampler = composed_sampler();
    if !backend_available() { return; }
    // Adding zero is semantically neutral but outside this MIR recurrence's
    // accepted scaled-index grammar. The reference retains its reached guards.
    let reference = sampler[sampler.find("fn finite_f32").expect("reader")..]
        .replace("finite_f32", "reference_finite")
        .replace("pub fn select", "fn reference_select")
        .replace("offset := token * 4", "offset := token * 4 + 0");
    let ref_mir = composed_program(&format!("{}\n{reference}", &sampler[..sampler.find("fn finite_f32").unwrap()]));
    let defined = ref_mir.fns.iter().map(|f| f.name.clone()).collect();
    let prepared = align_mir::byte_prepare::prepare(&ref_mir, &defined);
    assert!(reached_byte_guards(prepared.fns.iter().find(|f| f.name.as_str() == "reference_select").unwrap()) >= 2);
    let harness = r#"
fn compare(bytes: slice<u8>, seed: i64) -> bool {
  mut actual_rng := rand.seed_with(seed)
  mut reference_rng := rand.seed_with(seed)
  actual := actual_select(bytes, actual_rng)
  reference := reference_select(bytes, reference_rng)
  same := match actual {
    Ok(a) => match reference {
      Ok(b) => a.token_id == b.token_id && a.top_k_count == b.top_k_count && a.top_p_count == b.top_p_count && a.min_p_count == b.min_p_count && a.top_token_id == b.top_token_id,
      Err(_) => false,
    },
    Err(_) => match reference { Ok(_) => false, Err(_) => true },
  }
  return same && actual_rng.next() == reference_rng.next()
}
fn main() {
  mut all := true
  mut mode := 0
  loop {
    if mode >= 9 { break }
    mut n := 0
    loop {
      if n > 81 { break }
      mut b := buffer(0)
      b.put_u8(0)
      mut i := 0
      loop {
        if i >= n { break }
        value: f32 := if mode == 0 { i as f32 } else {
          if mode == 1 { (n - i) as f32 } else {
          if mode == 2 { 0.0 } else {
          if mode == 3 { (i % 3) as f32 } else {
          if mode == 4 { ((i * 7919 + 17) % 43) as f32 } else {
          if mode == 5 { -0.0 } else {
          if mode == 6 { 3.4028234e38 } else {
          if mode == 7 { -3.4028234e38 } else { (i % 7 - 3) as f32 }
          } } } } } } }
        b.put_f32_le(value)
        i = i + 1
      }
      bytes := b.bytes()
      all = all && compare(bytes[1..bytes.len()], 991 + n)
      n = n + 1
    }
    mode = mode + 1
  }
  mut bad_at := 0
  loop {
    if bad_at >= 41 { break }
    mut kind := 0
    loop {
      if kind >= 3 { break }
      mut bad := buffer(0)
      mut i := 0
      loop {
        if i >= 41 { break }
        bits: u32 := if i == bad_at { if kind == 0 { 2139095040 } else { if kind == 1 { 4286578688 } else { 2143289344 } } } else { 1065353216 }
        bad.put_u32_le(bits)
        i = i + 1
      }
      all = all && compare(bad.bytes(), 7)
      kind = kind + 1
    }
    bad_at = bad_at + 1
  }
  mut short := buffer(0)
  mut tail := 0
  loop {
    if tail > 7 { break }
    all = all && compare(short.bytes(), 19)
    short.put_u8(0)
    tail = tail + 1
  }
  print(all)
}
"#;
    let actual = sampler.replace("pub fn select", "fn actual_select");
    let source = format!("import std.rand\n{actual}\n{reference}\n{harness}");
    let output = build_and_run("composed-sampler-oracle", &source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n");
}

#[test]
fn composed_byte_effects_and_reaching_definitions_fail_closed() {
    for (label, extra, prefix, middle, bound, offset) in [
        ("mutable-call", "fn touch(borrow mut x: i64) { x = x + 1 }", "", "touch(i);", "src.len() / 4", "i * 4"),
        ("unknown-prefix", "fn observe(borrow x: slice<u8>) -> i64 = x.len()", "unused := observe(src);", "", "src.len() / 4", "i * 4"),
        ("bound-replace", "", "", "n = n + 1;", "n", "i * 4"),
        ("inclusive", "", "", "", "src.len() / 4 + 1", "i * 4"),
        ("wrong-step", "", "", "i = i + 1;", "src.len() / 4", "i * 4"),
        ("overflow-offset", "", "", "", "src.len() / 4", "i * 4 + 9223372036854775807"),
        ("stale-offset", "", "offset_before := i * 4;", "", "src.len() / 4", "offset_before"),
    ] {
        let source = format!("{extra}\nfn sum(borrow src: slice<u8>) -> u32 {{ mut n := src.len() / 4; mut result: u32 := 0; mut i := 0; {prefix} loop {{ if i >= {bound} {{ break }}; {middle} result = result + src.u32_le({offset}); i = i + 1 }}; return result }}\n");
        let original = composed_program(&source);
        let defined = original.fns.iter().map(|f| f.name.clone()).collect();
        let prepared = align_mir::byte_prepare::prepare(&original, &defined);
        let function = prepared.fns.iter().find(|f| f.name.as_str() == "sum").unwrap();
        assert!(reached_byte_guards(function) > 0, "{label}");
    }
}

#[test]
fn composed_leaf_exposure_covers_generics_returns_and_has_bounded_growth() {
    use align_mir::{DirectCall, Rvalue, Stmt};
    let source = "fn read<T>(unused: T, borrow src: slice<u8>, offset: i64) -> u32 { if offset < 0 { return 0 }; return src.u32_le(offset) }\nfn sum(borrow src: slice<u8>) -> u32 { n := src.len() / 4; mut result: u32 := 0; mut i := 0; loop { if i >= n { break }; result = result + read(true, src, i * 4); i = i + 1 }; return result }\n";
    let program = composed_program(source);
    let defined = program.fns.iter().map(|f| f.name.clone()).collect();
    let prepared = align_mir::byte_prepare::prepare(&program, &defined);
    assert_eq!(reached_byte_guards(prepared.fns.iter().find(|f| f.name.as_str() == "sum").unwrap()), 0);
    let mut source = String::from("fn read(borrow src: slice<u8>) -> u32 = src.u32_le(0)\nfn many(borrow src: slice<u8>) -> u32 { mut total: u32 := 0;\n");
    for _ in 0..40 { source.push_str("total = total + read(src);\n"); }
    source.push_str("return total }\n");
    let program = composed_program(&source);
    let defined = program.fns.iter().map(|f| f.name.clone()).collect();
    let prepared = align_mir::byte_prepare::prepare(&program, &defined);
    let many = prepared.fns.iter().find(|f| f.name.as_str() == "many").unwrap();
    assert_eq!(many.blocks.iter().flat_map(|b| &b.stmts).filter(|s|
        matches!(s, Stmt::Let(_, Rvalue::Call(DirectCall::Program(name), _)) if name.as_str() == "read")).count(), 8);
    if backend_available() {
        let out = build_and_run("composed-leaf-cap", &format!("{source}\nfn main() {{ mut b := buffer(4); b.put_u32_le(3); bytes := b.bytes(); print(many(bytes)) }}"));
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "120\n");
    }
}

#[test]
fn composed_reader_per_unit_and_thin_cache_bind_private_body_edits() {
    if !backend_available() { return; }
    let dir = std::env::temp_dir().join(format!("align-composed-cache-{}", std::process::id()));
    std::fs::create_dir(&dir).expect("exclusively acquire cache owner directory");
    let project = Proj { dir, entry: "main.align".into() };
    let library = "module scan\nfn read(borrow src: slice<u8>, off: i64) -> u32 = src.u32_le(off) + 0\npub fn sum(borrow src: slice<u8>) -> u32 { mut total: u32 := 0; n := src.len() / 4; mut i := 0; loop { if i >= n { break }; total = total + read(src, i * 4); i = i + 1 }; return total }\n";
    let main = "import scan\nfn main() { mut b := buffer(4); b.put_u32_le(7); bytes := b.bytes(); print(scan.sum(bytes)) }\n";
    project.write("scan.align", library);
    project.write("main.align", main);
    let entry = project.dir.join("main.align");
    let mut map = SourceMap::new();
    let walk = build_per_unit(&mut map, &entry.display().to_string(), main);
    assert!(!walk.diags.has_errors(), "{}", align_driver::format_diagnostics(&map, &walk.diags));
    let scan = walk.units.iter().find(|u| u.unit == "scan").expect("scan unit");
    let defined = scan.mir.fns.iter().map(|f| f.name.clone()).collect();
    let prepared = align_mir::byte_prepare::prepare(&scan.mir, &defined);
    assert!(prepared.fns.iter().any(|f| f.name.as_str().ends_with("sum") && reached_byte_guards(f) == 0));
    let cache = project.cache();
    for (round, source, expected, warm) in [
        ("cold", library.to_string(), "7\n", false),
        ("warm", library.to_string(), "7\n", true),
        ("edit", library.replace("+ 0", "+ 1"), "8\n", false),
        ("revert", library.to_string(), "7\n", true),
    ] {
        project.write("scan.align", &source);
        let built = thin_build(&project, &cache, 2);
        assert_eq!(built.prelink("scan").hit, warm, "{round}: private body cache identity");
        assert_eq!(built.run(&project), expected, "{round}");
    }
}

#[test]
fn composed_leaf_malformed_and_effectful_candidates_remain_calls() {
    use align_mir::{DirectCall, Operand, Rvalue, Stmt, Term};
    let source = "fn read(borrow src: slice<u8>, offset: i64) -> u32 = src.u32_le(offset)\nfn caller(borrow src: slice<u8>) -> u32 = read(src, 0)\n";
    let original = composed_program(source);
    for mutation in 0..7 {
        let mut program = original.clone();
        let leaf = program.fns.iter_mut().find(|f| f.name.as_str() == "read").unwrap();
        match mutation {
            0 => leaf.blocks[0].term = Term::Goto(0),
            1 => leaf.blocks[0].term = Term::Goto(u32::MAX),
            2 => { let duplicate = leaf.blocks[0].stmts[2].clone(); leaf.blocks[0].stmts.push(duplicate); },
            3 => leaf.blocks[0].stmts.push(Stmt::Drop(0)),
            4 => leaf.blocks[0].stmts.push(Stmt::Store(0, Operand::Arg(0))),
            5 => {
                let Stmt::Let(_, rv) = &mut leaf.blocks[0].stmts[2] else { panic!("descriptor load") };
                *rv = Rvalue::Load(u32::MAX);
            }
            6 => {
                let caller = program.fns.iter_mut().find(|f| f.name.as_str() == "caller").unwrap();
                for block in &mut caller.blocks {
                    for stmt in &mut block.stmts {
                        if let Stmt::Let(_, Rvalue::Call(DirectCall::Program(_), args)) = stmt { args.clear(); }
                    }
                }
            }
            _ => unreachable!(),
        }
        let defined = program.fns.iter().map(|f| f.name.clone()).collect();
        let prepared = align_mir::byte_prepare::prepare(&program, &defined);
        let caller = prepared.fns.iter().find(|f| f.name.as_str() == "caller").unwrap();
        assert!(caller.blocks.iter().flat_map(|b| &b.stmts).any(|s|
            matches!(s, Stmt::Let(_, Rvalue::Call(DirectCall::Program(name), _)) if name.as_str() == "read")), "mutation {mutation}");
    }
}

#[test]
fn composed_leaf_arguments_execute_once_and_keep_early_returns() {
    if !backend_available() { return; }
    let source = r#"
fn tick(borrow mut n: i64) -> i64 { n = n + 1; return n }
fn read(borrow src: slice<u8>, first: i64, second: i64) -> u32 {
  if first < 0 { return 99 }
  return src.u32_le(0) + (first * 10 + second) as u32
}
fn main() {
  mut empty := buffer(0)
  none := empty.bytes()
  print(read(none, -1, 0))
  mut b := buffer(4)
  b.put_u32_le(3)
  bytes := b.bytes()
  mut n := 0
  print(read(bytes, tick(n), tick(n)))
  print(n)
}
"#;
    let output = build_and_run("composed-leaf-order", source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "99\n15\n2\n");
}
