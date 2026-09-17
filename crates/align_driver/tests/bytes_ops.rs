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

// ---------------------------------------------------------------------------------------------
// `buffer.append_filled(length, value)` (#1073 part 1, Plan 65 Row B2) — the append member of the
// bulk-write family whose constructor is `buffer.filled` and whose in-place member is
// `slice<u8>.fill`. Same `(i64, u8)` grammar, same terminal policy for an invalid count, one
// growth rather than one opaque runtime call per byte.
// ---------------------------------------------------------------------------------------------

#[test]
fn append_filled_matches_the_filled_constructor_byte_for_byte() {
    if !backend_available() {
        return;
    }
    // Family consistency: `buffer.filled(n, v).bytes()` and `buffer(0)` + `append_filled(n, v)`
    // produce the same published length and the same bytes, for n in {0, 1, 7, 4096, 65536} and
    // v in {0x00, 0xff}. The helper compares length, first byte, last byte and a checksum.
    let src = "\
fn same(n: i64, v: u8) -> bool {
  c := buffer.filled(n, v)
  mut g := buffer(0)
  g.append_filled(n, v)
  cb := c.bytes()
  gb := g.bytes()
  if cb.len() != gb.len() { return false }
  if gb.len() != n { return false }
  mut i := 0
  loop {
    if i >= n { break }
    if cb[i] != gb[i] { return false }
    i = i + 1
  }
  return true
}

fn main() -> i32 {
  if !same(0, 0x00 as u8) { return 1 }
  if !same(0, 0xff as u8) { return 2 }
  if !same(1, 0x00 as u8) { return 3 }
  if !same(1, 0xff as u8) { return 4 }
  if !same(7, 0x00 as u8) { return 5 }
  if !same(7, 0xff as u8) { return 6 }
  if !same(4096, 0x00 as u8) { return 7 }
  if !same(4096, 0xff as u8) { return 8 }
  if !same(65536, 0x00 as u8) { return 9 }
  if !same(65536, 0xff as u8) { return 10 }
  return 0
}
";
    let out = build_and_run("bo-append-filled-family", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn append_filled_extends_an_existing_window_and_composes_with_the_other_writers() {
    if !backend_available() {
        return;
    }
    // It *extends*: the already-published bytes survive, and ordinary `put_*` / `append` keep
    // working on either side of it. Zero length is a no-op that publishes nothing.
    let src = "\
fn main() -> i32 {
  mut b := buffer(0)
  b.put_u8(0x11 as u8)
  b.append_filled(3, 0xaa as u8)
  b.append_filled(0, 0xbb as u8)
  b.put_u16_be(0x2233 as u16)
  b.append_filled(2, 0xcc as u8)
  v := b.bytes()
  if v.len() != 8 { return 1 }
  if v[0] != 0x11 { return 2 }
  if v[1] != 0xaa { return 3 }
  if v[3] != 0xaa { return 4 }
  if v[4] != 0x22 { return 5 }
  if v[5] != 0x33 { return 6 }
  if v[6] != 0xcc { return 7 }
  if v[7] != 0xcc { return 8 }
  return 0
}
";
    let out = build_and_run("bo-append-filled-extend", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn a_bulk_zero_fill_emits_one_runtime_call_and_no_per_byte_put() {
    if !backend_available() {
        return;
    }
    // The point of the operation: a 64 KiB fill is one `align_rt_buffer_append_filled` call, and
    // the per-byte `align_rt_buffer_put` loop that was the only spelling before is gone.
    let src = "\
pub fn fill_zero(borrow mut b: buffer, n: i64) {
  b.append_filled(n, 0 as u8)
}
fn main() -> i32 {
  mut b := buffer(0)
  fill_zero(b, 65536)
  return b.bytes().len() as i32 - 65536
}
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "bo-append-filled-ir", src);
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let ir = emit_llvm_ir(&mir, BuildTarget::Baseline, Profile::Release, false, &[], false)
        .expect("emit llvm ir");
    assert_eq!(
        ir.matches("call void @align_rt_buffer_append_filled").count(),
        1,
        "want exactly one bulk append call:\n{ir}"
    );
    assert!(
        !ir.contains("call void @align_rt_buffer_put"),
        "a bulk append must not emit any per-byte put:\n{ir}"
    );
    let out = build_and_run("bo-append-filled-64k", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn a_negative_append_filled_length_aborts_before_any_write() {
    if !backend_available() {
        return;
    }
    // Same terminal policy as the `buffer.filled` constructor: an invalid count aborts rather than
    // silently publishing nothing.
    let src = "\
fn main() -> i32 {
  mut b := buffer(0)
  b.append_filled(1, 0x7f as u8)
  b.append_filled(0 - 1, 0x00 as u8)
  return 0
}
";
    let out = build_and_run("bo-append-filled-negative", src);
    assert_ne!(out.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("allocation size overflow"),
        "want the shared allocation-size failure: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn append_filled_rejects_a_wrong_receiver_arity_or_argument_type() {
    // A `buffer` method: a non-buffer receiver, an immutable buffer, the wrong argument count and
    // the wrong argument types are all static errors.
    assert!(check_errs(
        "bo-af-recv",
        "fn main() -> i32 {\n  mut s := \"x\"\n  s.append_filled(1, 0 as u8)\n  return 0\n}\n",
    ));
    assert!(check_errs(
        "bo-af-immutable",
        "fn main() -> i32 {\n  b := buffer(0)\n  b.append_filled(1, 0 as u8)\n  return 0\n}\n",
    ));
    assert!(check_errs(
        "bo-af-arity",
        "fn main() -> i32 {\n  mut b := buffer(0)\n  b.append_filled(1)\n  return 0\n}\n",
    ));
    assert!(check_errs(
        "bo-af-value-ty",
        "fn main() -> i32 {\n  mut b := buffer(0)\n  b.append_filled(1, 300)\n  return 0\n}\n",
    ));
    let rendered = check_diagnostics(
        "bo-af-len-ty",
        "fn main() -> i32 {\n  mut b := buffer(0)\n  b.append_filled(\"n\", 0 as u8)\n  return 0\n}\n",
    );
    assert!(
        rendered.contains("append_filled"),
        "want an `append_filled` diagnostic:\n{rendered}"
    );
}
