//! `core.codec` owner tests: canonical bytes, zero-copy access, ownership, checked-HIR, and the
//! alignment-independent LLVM lowering contract.

mod common;
use common::*;

#[test]
fn codec_encoder_and_views_round_trip_all_four_kinds() {
    if !backend_available() {
        return;
    }
    let source = r#"import core.codec
pub fn main() -> Result<(), Error> {
  ids := [-1, 2]
  scores := [1.5, -0.0]
  flags := [true, false]
  names := ["x", ""]
  mut encoder := codec.encoder(2)?
  encoder.put_i64("i", ids)?
  encoder.put_f64("f", scores)?
  encoder.put_bool("b", flags)?
  encoder.put_str("s", names)?
  finished := encoder.finish()
  batch := codec.open(finished.bytes())?
  print(batch.rows())
  print(batch.columns())
  print(batch.name(0) else "missing")
  kind := batch.kind(1) else { return Err(Error.Invalid) }
  print(match kind { F64 => true, _ => false })
  print(batch.find("b") else -1)
  i64s := batch.i64s(0) else { return Err(Error.Invalid) }
  f64s := batch.f64s(1) else { return Err(Error.Invalid) }
  bools := batch.bools(2) else { return Err(Error.Invalid) }
  strs := batch.strs(3) else { return Err(Error.Invalid) }
  print(i64s.len())
  print(i64s.at(0) else 0)
  print(f64s.at(0) else 0.0)
  print(bools.at(0) else false)
  print(strs.at(0) else "missing")
  print(strs.at(1) else "missing")
  return Ok(())
}
"#;
    let output = build_and_run("core-codec-round-trip", source);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "2\n4\ni\ntrue\n2\n2\n-1\n1.5\ntrue\nx\n\n",
    );
}

#[test]
fn codec_surface_rejects_missing_import_wrong_types_unbound_receivers_and_shared_mutation() {
    assert!(check_errs(
        "core-codec-import",
        "fn main() { _ := codec.encoder(0) }\n",
    ));
    assert!(check_errs(
        "core-codec-open-type",
        "import core.codec\nfn main() { _ := codec.open(7) }\n",
    ));
    assert!(check_errs(
        "core-codec-open-str",
        "import core.codec\nfn main() { _ := codec.open(\"ALNCOL01\") }\n",
    ));
    for (name, method) in [("at", "at(0)"), ("finish", "finish()")] {
        let diagnostics = check_diagnostics(
            &format!("core-codec-batch-cross-method-{name}"),
            &format!(
                "import core.codec\nfn bad(batch: codec.batch) {{ _ := batch.{method} }}\nfn main() -> i32 = 0\n"
            ),
        );
        assert!(diagnostics.contains("is not a method on codec.batch"), "{diagnostics}");
    }
    assert!(check_errs(
        "core-codec-put-type",
        "import core.codec\nfn main() -> Result<(), Error> { encoder := codec.encoder(1)?; encoder.put_i64(\"x\", [true])?; return Ok(()) }\n",
    ));
    assert!(check_errs(
        "core-codec-unbound-put",
        "import core.codec\nfn main() -> Result<(), Error> { codec.encoder(1)?.put_i64(\"x\", [1])?; return Ok(()) }\n",
    ));
    let argument_diagnostics = check_diagnostics(
        "core-codec-unbound-put-argument-first",
        "import core.codec\nfn main() -> Result<(), Error> { codec.encoder(1)?.put_i64(\"x\", [true])?; return Ok(()) }\n",
    );
    assert!(argument_diagnostics.contains("i64"), "{argument_diagnostics}");
    assert!(!argument_diagnostics.contains("bind the codec encoder"), "{argument_diagnostics}");
    assert!(check_errs(
        "core-codec-shared-put",
        "import core.codec\nfn bad(borrow encoder: codec.encoder) -> Result<(), Error> { encoder.put_i64(\"x\", [1])?; return Ok(()) }\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "core-codec-shared-finish",
        "import core.codec\nfn bad(borrow encoder: codec.encoder) -> buffer = encoder.finish()\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "core-codec-borrow-mut-finish",
        "import core.codec\nfn bad(borrow mut encoder: codec.encoder) -> buffer = encoder.finish()\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "core-codec-use-after-finish",
        "import core.codec\nfn main() -> Result<(), Error> { encoder := codec.encoder(0)?; finished := encoder.finish(); second := encoder.finish(); _ := finished.len(); _ := second.len(); return Ok(()) }\n",
    ));
}

#[test]
fn codec_types_cross_whole_program_and_per_unit_interfaces() {
    let files = &[
        (
            "codec_api.align",
            r#"module codec_api
import core.codec
pub fn open(input: slice<u8>) -> Result<codec.batch, Error> = codec.open(input)
pub fn names(batch: codec.batch) -> Option<codec.str_column> = batch.strs(3)
pub fn start(rows: i64) -> Result<codec.encoder, Error> = codec.encoder(rows)
pub fn add_ids(encoder: codec.encoder, values: slice<i64>) -> Result<codec.encoder, Error> {
  encoder.put_i64("i", values)?
  return Ok(encoder)
}
pub fn finish(encoder: codec.encoder) -> buffer = encoder.finish()
pub fn kind(batch: codec.batch) -> Option<codec.kind> = batch.kind(0)
"#,
        ),
        (
            "main.align",
            r#"import core.codec
import codec_api
pub fn main() -> Result<(), Error> {
  values := [7, 9]
  encoder := codec_api.start(2)?
  encoder2 := codec_api.add_ids(encoder, values)?
  finished := codec_api.finish(encoder2)
  batch := codec_api.open(finished.bytes())?
  print(batch.rows())
  ids := batch.i64s(0) else { return Err(Error.Invalid) }
  print(ids.at(1) else -1)
  return Ok(())
}
"#,
        ),
    ];
    let differential = diff_check_multi("core-codec-interface", files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    if backend_available() {
        let output =
            build_per_unit_multi("core-codec-interface-run", files, "main.align").link_and_run();
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "2\n9\n");
    }
}

#[test]
fn codec_views_keep_the_input_generation_live_and_stay_out_of_invalid_carriers() {
    let carriers = r#"import core.codec
Holder { batch: codec.batch, names: Option<codec.str_column> }
Choice { Batch(codec.batch), Empty }
Generic<T> { Value(T), Empty }
fn hold(batch: codec.batch) -> Holder = Holder { batch: batch, names: batch.strs(0) }
fn rows(holder: Holder) -> i64 = holder.batch.rows()
fn choose(batch: codec.batch) -> Choice = Choice.Batch(batch)
fn inspect(choice: Choice) -> i64 = match choice { Batch(batch) => batch.rows(), Empty => 0 }
fn generic_view(value: Generic<codec.batch>) -> i64 = match value { Value(batch) => batch.rows(), Empty => 0 }
fn generic_owner(value: Generic<codec.encoder>) -> i32 = 0
fn main() -> i32 = 0
"#;
    assert!(
        !check_errs("core-codec-view-carriers", carriers),
        "{}",
        check_diagnostics("core-codec-view-carriers", carriers),
    );
    assert!(check_errs(
        "core-codec-owner-mutation",
        "import core.codec\nfn main() -> Result<(), Error> { mut encoder := codec.encoder(0)?; finished := encoder.finish(); batch := codec.open(finished.bytes())?; finished.append(\"x\"); _ := batch.rows(); return Ok(()) }\n",
    ));
    assert!(check_errs(
        "core-codec-local-owner-escape",
        "import core.codec\nfn bad() -> Result<codec.batch, Error> { encoder := codec.encoder(0)?; finished := encoder.finish(); return codec.open(finished.bytes()) }\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "core-codec-column-array",
        "import core.codec\nfn bad(values: array<codec.i64_column>) {}\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "core-codec-encoder-array",
        "import core.codec\nfn bad(values: array<codec.encoder>) {}\nfn main() -> i32 = 0\n",
    ));
}

#[test]
fn codec_llvm_uses_keyed_runtime_rows_and_alignment_one_inline_loads() {
    if !backend_available() {
        return;
    }
    let mut sources = SourceMap::new();
    let checked = check(
        &mut sources,
        "core-codec-llvm",
        r#"import core.codec
pub fn inspect(input: slice<u8>, index: i64) -> Result<i64, Error> {
  batch := codec.open(input)?
  ids := batch.i64s(0) else { return Err(Error.Invalid) }
  return Ok(ids.at(index) else -1)
}
pub fn encode(values: slice<i64>) -> Result<buffer, Error> {
  encoder := codec.encoder(values.len())?
  encoder.put_i64("i", values)?
  return Ok(encoder.finish())
}
fn main() -> i32 = 0
"#,
    );
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags),
    );
    let mir = align_driver::try_lower_to_mir(&checked.hir).expect("checked codec HIR");
    let llvm = emit_llvm_ir(&mir, BuildTarget::Baseline, false, &[], false).expect("LLVM IR");
    for call in [
        "call i32 @align_rt_codec_open_v1(ptr",
        "call i32 @align_rt_codec_encoder_new_v1(i64",
        "call i32 @align_rt_codec_encoder_put_i64_v1(ptr",
        "call ptr @align_rt_codec_encoder_finish_v1(ptr",
        "call void @align_rt_codec_encoder_free_v1(ptr",
    ] {
        assert!(llvm.contains(call), "missing `{call}`:\n{llvm}");
    }
    assert!(
        llvm.matches("align 1").count() >= 5,
        "header, descriptor, and element loads must be explicitly unaligned:\n{llvm}",
    );
    assert!(!llvm.contains("align_rt_codec_batch_"));
    assert!(!llvm.contains("align_rt_codec_column_"));
}

#[test]
fn codec_all_four_golden_decodes_at_every_base_alignment() {
    if !backend_available() {
        return;
    }
    let source = r#"import core.codec
import std.encoding
fn verify(input: slice<u8>) -> Result<i64, Error> {
  batch := codec.open(input)?
  if batch.rows() != 2 || batch.columns() != 4 { return Err(Error.Invalid) }
  if (batch.name(-1) else "none") != "none" { return Err(Error.Invalid) }
  if (batch.name(4) else "none") != "none" { return Err(Error.Invalid) }
  if (batch.find("B") else -1) != -1 { return Err(Error.Invalid) }
  kind := batch.kind(3) else { return Err(Error.Invalid) }
  if !match kind { Str => true, _ => false } { return Err(Error.Invalid) }
  ids := batch.i64s(0) else { return Err(Error.Invalid) }
  scores := batch.f64s(1) else { return Err(Error.Invalid) }
  flags := batch.bools(2) else { return Err(Error.Invalid) }
  names := batch.strs(3) else { return Err(Error.Invalid) }
  if match batch.f64s(0) { Some(_) => true, None => false } { return Err(Error.Invalid) }
  if (ids.at(-1) else 17) != 17 || (ids.at(2) else 19) != 19 { return Err(Error.Invalid) }
  if (ids.at(0) else 0) != -1 || (ids.at(1) else 0) != 2 { return Err(Error.Invalid) }
  if (scores.at(0) else 0.0) != 1.5 || (scores.at(1) else 1.0) != -0.0 { return Err(Error.Invalid) }
  if !(flags.at(0) else false) || (flags.at(1) else true) { return Err(Error.Invalid) }
  if (names.at(0) else "none") != "x" || (names.at(1) else "none") != "" { return Err(Error.Invalid) }
  return Ok(ids.at(1) else -1)
}
pub fn main() -> Result<(), Error> {
  raw := encoding.hex_decode("414c4e434f4c3031280100000000000002000000000000000400000000000000e0000000000000000100000000000000e800000000000000100000000000000000000000000000000000000000000000e1000000000000000100000001000000f800000000000000100000000000000000000000000000000000000000000000e20000000000000001000000020000000801000000000000010000000000000000000000000000000000000000000000e300000000000000010000000300000010010000000000000c00000000000000200100000000000001000000000000006966627300000000ffffffffffffffff0200000000000000000000000000f83f00000000000000800100000000000000000000000100000001000000000000007800000000000000")?
  mut residue := 0
  loop {
    mut shifted := buffer(0)
    mut padding := 0
    loop {
      if padding >= residue { break }
      shifted.put_u8(0)
      padding = padding + 1
    }
    shifted.append(raw.bytes())
    bytes := shifted.bytes()
    print(verify(bytes[residue..])?)
    residue = residue + 1
    if residue == 8 { break }
  }
  return Ok(())
}
"#;
    let output = build_and_run("core-codec-alignment", source);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "2\n2\n2\n2\n2\n2\n2\n2\n");
}
