//! Tests for binary slice operations: `set_*`, `fill`, `fill_*`, and `copy_from` (Plan 65 Rows W, M, P).

mod common;
use common::*;

#[test]
fn bytes_set_scalars_and_endianness() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut b := buffer.filled(16, 0)
  mut sl := b.bytes()

  // Single-byte set
  sl.set_u8(0, 0x12)
  sl.set_i8(1, -1)
  if sl[0] != 0x12 { return 1 }
  if sl[1] != 0xff { return 2 }

  // 16-bit
  sl.set_u16_le(2, 0x1234)
  if sl[2] != 0x34 || sl[3] != 0x12 { return 3 }
  sl.set_u16_be(2, 0x1234)
  if sl[2] != 0x12 || sl[3] != 0x34 { return 4 }

  sl.set_i16_le(4, -2) // 0xfffe
  if sl[4] != 0xfe || sl[5] != 0xff { return 5 }
  sl.set_i16_be(4, -2)
  if sl[4] != 0xff || sl[5] != 0xfe { return 6 }

  // 32-bit
  sl.set_u32_le(6, 0x12345678)
  if sl[6] != 0x78 || sl[7] != 0x56 || sl[8] != 0x34 || sl[9] != 0x12 { return 7 }
  sl.set_u32_be(6, 0x12345678)
  if sl[6] != 0x12 || sl[7] != 0x34 || sl[8] != 0x56 || sl[9] != 0x78 { return 8 }

  // 64-bit
  sl.set_i64_le(0, 0x0102030405060708)
  if sl[0] != 0x08 || sl[1] != 0x07 || sl[2] != 0x06 || sl[3] != 0x05 ||
     sl[4] != 0x04 || sl[5] != 0x03 || sl[6] != 0x02 || sl[7] != 0x01 { return 9 }
  sl.set_i64_be(0, 0x0102030405060708)
  if sl[0] != 0x01 || sl[1] != 0x02 || sl[2] != 0x03 || sl[3] != 0x04 ||
     sl[4] != 0x05 || sl[5] != 0x06 || sl[6] != 0x07 || sl[7] != 0x08 { return 10 }

  return 0
}
";
    assert_eq!(build_and_run("bytes-set-scalars", src).status.code(), Some(0));
}

#[test]
fn bytes_set_floats() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut b := buffer.filled(8, 0)
  mut sl := b.bytes()

  // f32: 1.0f32 is 0x3f800000
  sl.set_f32_be(0, 1.0)
  if sl[0] != 0x3f || sl[1] != 0x80 || sl[2] != 0x00 || sl[3] != 0x00 { return 1 }
  sl.set_f32_le(0, 1.0)
  if sl[0] != 0x00 || sl[1] != 0x00 || sl[2] != 0x80 || sl[3] != 0x3f { return 2 }

  // f64: 1.0f64 is 0x3ff0000000000000
  sl.set_f64_be(0, 1.0)
  if sl[0] != 0x3f || sl[1] != 0xf0 || sl[2] != 0x00 || sl[3] != 0x00 ||
     sl[4] != 0x00 || sl[5] != 0x00 || sl[6] != 0x00 || sl[7] != 0x00 { return 3 }
  sl.set_f64_le(0, 1.0)
  if sl[0] != 0x00 || sl[1] != 0x00 || sl[2] != 0x00 || sl[3] != 0x00 ||
     sl[4] != 0x00 || sl[5] != 0x00 || sl[6] != 0xf0 || sl[7] != 0x3f { return 4 }

  return 0
}
";
    assert_eq!(build_and_run("bytes-set-floats", src).status.code(), Some(0));
}

#[test]
fn bytes_fill_single_and_multi_byte() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut b := buffer.filled(8, 0)
  mut sl := b.bytes()

  // fill single byte
  sl.fill(0xab)
  if sl[0] != 0xab || sl[7] != 0xab { return 1 }

  // fill multi-byte with identical bytes (0)
  sl.fill_u32_le(0)
  if sl[0] != 0 || sl[3] != 0 || sl[4] != 0 || sl[7] != 0 { return 2 }

  // fill multi-byte with non-identical pattern
  sl.fill_u32_be(0x12345678)
  if sl[0] != 0x12 || sl[1] != 0x34 || sl[2] != 0x56 || sl[3] != 0x78 { return 3 }
  if sl[4] != 0x12 || sl[5] != 0x34 || sl[6] != 0x56 || sl[7] != 0x78 { return 4 }

  // 0-length fill is a no-op
  mut empty_buf := buffer.filled(0, 0)
  mut empty := empty_buf.bytes()
  empty.fill(0xff)
  empty.fill_u32_be(0x12345678)

  return 0
}
";
    assert_eq!(build_and_run("bytes-fill-multi", src).status.code(), Some(0));
}

#[test]
fn bytes_copy_from() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut b1 := buffer.filled(4, 0)
  mut b2 := buffer.filled(4, 0)

  mut s1 := b1.bytes()
  s1[0] = 1
  s1[1] = 2
  s1[2] = 3
  s1[3] = 4

  mut dst := b2.bytes()
  src_sl := b1.bytes()

  dst.copy_from(src_sl)

  if dst[0] != 1 || dst[1] != 2 || dst[2] != 3 || dst[3] != 4 { return 1 }

  // 0-length copy is a no-op
  mut e1 := buffer.filled(0, 0)
  mut e2 := buffer.filled(0, 0)
  mut d0 := e1.bytes()
  s0 := e2.bytes()
  d0.copy_from(s0)

  return 0
}
";
    assert_eq!(build_and_run("bytes-copy-from", src).status.code(), Some(0));
}

#[test]
fn bytes_set_out_of_bounds_aborts() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut b := buffer.filled(4, 0)
  mut sl := b.bytes()
  sl.set_i32_le(2, 42) // offset 2 + 4 > 4 => OOB abort
  return 0
}
";
    let res = build_and_run("bytes-set-oob", src);
    assert_ne!(res.status.code(), Some(0), "OOB set must abort");
}

#[test]
fn bytes_fill_indivisible_length_aborts() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut b := buffer.filled(5, 0)
  mut sl := b.bytes()
  sl.fill_u32_le(0) // 5 % 4 != 0 => abort
  return 0
}
";
    let res = build_and_run("bytes-fill-indiv", src);
    assert_ne!(res.status.code(), Some(0), "Indivisible length fill must abort");
}

#[test]
fn bytes_copy_from_length_mismatch_aborts() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut b1 := buffer.filled(3, 0)
  mut b2 := buffer.filled(4, 0)
  mut dst := b2.bytes()
  src_sl := b1.bytes()
  dst.copy_from(src_sl) // 4 != 3 => abort
  return 0
}
";
    let res = build_and_run("bytes-copy-mismatch", src);
    assert_ne!(res.status.code(), Some(0), "Length mismatch copy must abort");
}

#[test]
fn bytes_copy_from_same_backing_rejected() {
    let src = "\
fn probe(borrow mut d: slice<u8>) {
  d.copy_from(d)
}
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "same-backing", src);
    assert!(
        checked.diags.iter().any(|e| e.message.contains("cannot copy from overlapping slice: destination and source have the same backing")),
        "same backing copy must be rejected"
    );
}

#[test]
fn bytes_fill_u8_rejected_with_diagnostic() {
    let src = "\
fn probe(borrow mut d: slice<u8>) {
  d.fill_u8(0)
}
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "fill-u8", src);
    assert!(
        checked.diags.iter().any(|e| e.message.contains(".fill_u8()' is not supported; use '.fill(value)' for single-byte fill")),
        "fill_u8 must be rejected with helpful diagnostic"
    );
}

#[test]
fn bytes_mutation_on_immutable_receiver_rejected() {
    // 1. Shared borrow parameter must not be mutated
    let src1 = "\
fn probe_set(borrow s: slice<u8>) {
  s.set_u8(0, 1)
}
";
    let mut sm = SourceMap::new();
    let checked1 = check(&mut sm, "imm-param-set", src1);
    assert!(
        checked1.diags.iter().any(|e| e.message.contains("cannot mutate bytes of immutable 's'")),
        "set on immutable borrow param must be rejected"
    );

    let src2 = "\
fn probe_fill(borrow s: slice<u8>) {
  s.fill(0)
}
";
    let mut sm = SourceMap::new();
    let checked2 = check(&mut sm, "imm-param-fill", src2);
    assert!(
        checked2.diags.iter().any(|e| e.message.contains("cannot mutate bytes of immutable 's'")),
        "fill on immutable borrow param must be rejected"
    );

    // 2. Immutable local binding must not be mutated
    let src3 = "\
fn main() -> i32 {
  mut b := buffer.filled(4, 0)
  s := b.bytes()
  s.set_u8(0, 1)
  return 0
}
";
    let mut sm = SourceMap::new();
    let checked3 = check(&mut sm, "imm-local-set", src3);
    assert!(
        checked3.diags.iter().any(|e| e.message.contains("cannot mutate bytes of immutable 's'")),
        "set on immutable slice local must be rejected"
    );

    // 3. Mutable slice borrowing immutable buffer must not be mutated
    let src4 = "\
fn main() -> i32 {
  b := buffer.filled(4, 0)
  mut s := b.bytes()
  s.fill(0)
  return 0
}
";
    let mut sm = SourceMap::new();
    let checked4 = check(&mut sm, "imm-buffer-fill", src4);
    assert!(
        checked4.diags.iter().any(|e| e.message.contains("cannot mutate bytes of immutable 'b'")),
        "fill on slice with immutable backing buffer must be rejected"
    );

    // 4. String bytes are immutable
    let src5 = "\
fn main() -> i32 {
  s := \"hello\"
  s.bytes().fill(0)
  return 0
}
";
    let mut sm = SourceMap::new();
    let checked5 = check(&mut sm, "str-bytes-fill", src5);
    assert!(
        checked5.diags.iter().any(|e| e.message.contains("cannot mutate immutable string bytes of 's'")),
        "mutation of string bytes must be rejected"
    );
}

#[test]
fn bytes_copy_from_aliasing_slices_rejected() {
    // 1. Two slice locals from the same backing buffer
    let src1 = "\
fn main() -> i32 {
  mut b := buffer.filled(8, 0)
  mut s1 := b.bytes()
  s2 := b.bytes()
  s1.copy_from(s2)
  return 0
}
";
    let mut sm = SourceMap::new();
    let checked1 = check(&mut sm, "same-buffer-locals", src1);
    assert!(
        checked1.diags.iter().any(|e| e.message.contains("cannot copy from overlapping slice: destination and source have the same backing 'b'")),
        "copy between slices of same buffer must be rejected"
    );

    // 2. Subslice expressions of the same backing slice parameter
    let src2 = "\
fn probe(borrow mut d: slice<u8>) {
  d[0..4].copy_from(d[2..6])
}
";
    let mut sm = SourceMap::new();
    let checked2 = check(&mut sm, "subslice-same-param", src2);
    assert!(
        checked2.diags.iter().any(|e| e.message.contains("cannot copy from overlapping slice: destination and source have the same backing 'd'")),
        "copy between overlapping subslices of same param must be rejected"
    );
}
