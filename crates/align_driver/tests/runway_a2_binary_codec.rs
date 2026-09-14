//! align-LLM runway A2 — binary decode/encode on `bytes`/`buffer`. Bounds-checked, endian-explicit
//! scalar reads `bytes.<scalar>_<le|be>(off)` and the matching growable-`buffer` writes
//! `buf.put_<scalar>_<le|be>(v)` / `buf.append(data)`. GGUF-class header parsing and
//! `alignpack`/`alignidx` emission are the consumers. Completion: encode→hex round trip across
//! every width + both endians (incl. floats), decode back with correct signedness, out-of-range
//! reads abort (same policy as `slice[i]`), and the negative diagnostics (immutable receiver, wrong
//! receiver/value type). (`docs/open-questions.md` Open → "align-LLM runway" A2; `draft.md` §18.2.)

mod common;
use common::*;

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
    assert!(optimized.contains("bitcast float"), "{optimized}");
    assert!(!optimized.contains("call "), "{optimized}");
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
        ("escape", "mut b := buffer(4); b.put_u32_le(x); return consume(b.bytes())"),
    ] {
        let ir = emit_llvm_with_exports(&format!("fn consume(xs: slice<u8>) -> u32 = xs.u32_le(0)\nfn f(x: u32) -> u32 {{ {body} }}\n"), &["f"]);
        assert!(ir.contains("call ptr @align_rt_buffer_new"), "{label}: {ir}");
    }
}
