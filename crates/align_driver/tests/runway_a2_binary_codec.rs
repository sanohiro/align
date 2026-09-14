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
        Stmt::Let(id, Rvalue::BufferNew(_) | Rvalue::BufferPut { .. } | Rvalue::BufferAppend { .. } | Rvalue::BufferBytes(_) | Rvalue::BufferLen(_)) => Some(*id),
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
fn byte_range_malformed_and_invalidated_proofs_fail_closed() {
    use align_ast::BinOp;
    use align_mir::{Const, Operand, Rvalue, Stmt, Term};
    use align_sema::{IntTy, Ty};
    let source = "fn sum(src: slice<u8>, other: slice<u8>) -> u32 { mut result: u32 := 0; mut i := 1; loop { if i >= src.len() / 4 { break }; result = result + src.u32_le(i * 4); i = i + 1 }; return result }\n";
    let mut map = SourceMap::new();
    let checked = check(&mut map, "byte-range-proof", source);
    assert!(!checked.diags.has_errors());
    let mut base = lower_to_mir(&checked.hir).fns.remove(0);
    let integer = Ty::Int(IntTy { bits: 64, signed: true });
    // Start at one to retain the original checked CFG during source lowering,
    // then supply the exact zero-based recurrence to the private MIR pass.
    let mut induction = None;
    for block in &mut base.blocks {
        for statement in &mut block.stmts {
            if let Stmt::Store(slot, Operand::Const(Const::Int(value, ty))) = statement {
                if *ty == integer && *value == 1 { *value = 0; induction = Some(*slot); }
            }
        }
    }
    let induction = induction.expect("induction initialization");
    assert_eq!(reached_byte_guards(&base), 1);
    let mut valid = base.clone();
    align_mir::byte_ranges::simplify(&mut valid);
    assert_eq!(reached_byte_guards(&valid), 0);
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
        align_mir::byte_ranges::simplify(&mut bad);
        assert_eq!(reached_byte_guards(&bad), 1, "mutation {mutation}");
    }
}
