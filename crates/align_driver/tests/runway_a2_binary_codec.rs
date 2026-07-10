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
