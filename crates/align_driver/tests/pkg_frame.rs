//! `pkg.frame` owner tests: canonical package admission, stable bounded joins, checked lowering,
//! and whole/per-unit interface parity.

mod common;
use common::*;

fn frame_source() -> &'static str {
    fixture("apps/frame/pkg/frame.align")
}

const I64_MAIN: &str = r#"module main
import core.codec
import pkg.frame

fn main() -> Result<(), Error> {
  left_values := [2, 1, 2, 3]
  mut left_encoder := codec.encoder(left_values.len())?
  left_encoder.put_i64("key", left_values)?
  left_buffer := left_encoder.finish()
  left_batch := codec.open(left_buffer.bytes())?
  left := left_batch.i64s(0) else { return Err(Error.Invalid) }

  right_values := [2, 2, 3]
  mut right_encoder := codec.encoder(right_values.len())?
  right_encoder.put_i64("key", right_values)?
  right_buffer := right_encoder.finish()
  right_batch := codec.open(right_buffer.bytes())?
  right := right_batch.i64s(0) else { return Err(Error.Invalid) }

  joined := pkg.frame.inner_join_i64(left, right, 5)
  match joined {
    Ok(rows) => {
      print(rows.len())
      mut index := 0
      loop {
        if index >= rows.len() { break }
        print(rows[index].left)
        print(rows[index].right)
        index = index + 1
      }
    }
    Err(_) => { return Err(Error.Invalid) }
  }
  return Ok(())
}
"#;

fn frame_files(main: &str) -> [(&str, &str); 2] {
    [("pkg/frame.align", frame_source()), ("main.align", main)]
}

#[test]
fn frame_i64_join_is_stable_and_whole_per_unit_equivalent() {
    let files = frame_files(I64_MAIN);
    let differential = diff_check_multi("pkg-frame-i64-interface", &files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    if !backend_available() {
        return;
    }
    for (name, output) in [
        (
            "whole",
            build_and_run_multi("pkg-frame-i64-whole", &files, "main.align"),
        ),
        (
            "per-unit",
            build_per_unit_multi("pkg-frame-i64-per-unit", &files, "main.align").link_and_run(),
        ),
    ] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "5\n0\n0\n0\n1\n2\n0\n2\n1\n3\n2\n",
            "{name}",
        );
    }
}

#[test]
fn frame_string_join_preserves_byte_equality_through_an_indirect_wrapper_call() {
    let main = r#"module main
import core.codec
import pkg.frame

fn main() -> Result<(), Error> {
  left_values := ["a\0b", "é", "\n", "prefix-x", "a\0b"]
  mut left_encoder := codec.encoder(left_values.len())?
  left_encoder.put_str("key", left_values)?
  left_buffer := left_encoder.finish()
  left_batch := codec.open(left_buffer.bytes())?
  left := left_batch.strs(0) else { return Err(Error.Invalid) }

  right_values := ["a\0b", "\n", "prefix", "é"]
  mut right_encoder := codec.encoder(right_values.len())?
  right_encoder.put_str("key", right_values)?
  right_buffer := right_encoder.finish()
  right_batch := codec.open(right_buffer.bytes())?
  right := right_batch.strs(0) else { return Err(Error.Invalid) }

  join := pkg.frame.inner_join_str
  result := join(left, right, 4)
  return match result {
    Ok(rows) => {
      if rows.len() != 4 { return Err(Error.Invalid) }
      print(rows[0].left)
      print(rows[0].right)
      print(rows[1].left)
      print(rows[1].right)
      print(rows[2].left)
      print(rows[2].right)
      print(rows[3].left)
      print(rows[3].right)
      Ok(())
    }
    Err(_) => Err(Error.Invalid)
  }
}
"#;
    let files = frame_files(main);
    let differential = diff_check_multi("pkg-frame-str-indirect", &files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    if !backend_available() {
        return;
    }
    let output = build_and_run_multi("pkg-frame-str-indirect-run", &files, "main.align");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "0\n0\n1\n3\n2\n1\n4\n0\n"
    );
}

#[test]
fn frame_public_wrapper_reaches_the_bridge_through_fields_and_joined_function_values() {
    let main = r#"module main
import core.codec
import pkg.frame

Joiner {
  call: fn(codec.i64_column, codec.i64_column, i64) -> Result<array<pkg.frame.RowPair>, pkg.frame.JoinError>
}

fn forward(
  left: codec.i64_column,
  right: codec.i64_column,
  max_pairs: i64,
) -> Result<array<pkg.frame.RowPair>, pkg.frame.JoinError> =
  pkg.frame.inner_join_i64(left, right, max_pairs)

fn count(result: Result<array<pkg.frame.RowPair>, pkg.frame.JoinError>) -> i64 = match result {
  Ok(rows) => rows.len()
  Err(_) => -1
}

fn main() -> Result<(), Error> {
  mut encoder := codec.encoder(1)?
  encoder.put_i64("key", [7])?
  buffer := encoder.finish()
  batch := codec.open(buffer.bytes())?
  column := batch.i64s(0) else { return Err(Error.Invalid) }

  direct := Joiner { call: pkg.frame.inner_join_i64 }
  local := Joiner { call: forward }
  mut selected := pkg.frame.inner_join_i64
  if column.len() == 1 { selected = forward } else { selected = pkg.frame.inner_join_i64 }
  print(count(direct.call(column, column, 1)))
  print(count(local.call(column, column, 1)))
  print(count(selected(column, column, 1)))
  return Ok(())
}
"#;
    let files = frame_files(main);
    let differential = diff_check_multi("pkg-frame-target-shapes", &files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    if !backend_available() {
        return;
    }
    for (name, output) in [
        (
            "whole",
            build_and_run_multi("pkg-frame-target-shapes-whole", &files, "main.align"),
        ),
        (
            "per-unit",
            build_per_unit_multi("pkg-frame-target-shapes-per-unit", &files, "main.align")
                .link_and_run(),
        ),
    ] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "1\n1\n1\n",
            "{name}"
        );
    }
}

#[test]
fn frame_result_returns_after_both_codec_inputs_are_dropped() {
    let main = r#"module main
import core.codec
import pkg.frame
import std.process

fn detached() -> Result<array<pkg.frame.RowPair>, pkg.frame.JoinError> {
  mut left_encoder := codec.encoder(2) else { process.abort() }
  left_encoder.put_str("key", ["same", "other"]) else { process.abort() }
  left_buffer := left_encoder.finish()
  left_batch := codec.open(left_buffer.bytes()) else { process.abort() }
  left := left_batch.strs(0) else { process.abort() }

  mut right_encoder := codec.encoder(1) else { process.abort() }
  right_encoder.put_str("key", ["same"]) else { process.abort() }
  right_buffer := right_encoder.finish()
  right_batch := codec.open(right_buffer.bytes()) else { process.abort() }
  right := right_batch.strs(0) else { process.abort() }
  arena {
    return pkg.frame.inner_join_str(left, right, 1)
  }
}

fn main() -> i32 = match detached() {
  Ok(rows) => if rows.len() == 1 && rows[0].left == 0 && rows[0].right == 0 { 0 } else { 1 }
  Err(_) => 2
}
"#;
    let files = frame_files(main);
    let differential = diff_check_multi("pkg-frame-detached-result", &files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    if !backend_available() {
        return;
    }
    for (name, output) in [
        (
            "whole",
            build_and_run_multi("pkg-frame-detached-result-whole", &files, "main.align"),
        ),
        (
            "per-unit",
            build_per_unit_multi("pkg-frame-detached-result-per-unit", &files, "main.align")
                .link_and_run(),
        ),
    ] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
fn frame_limit_errors_are_exact_and_empty_products_allocate_no_rows() {
    let main = r#"module main
import core.codec
import pkg.frame

fn error_code(result: Result<array<pkg.frame.RowPair>, pkg.frame.JoinError>) -> i64 = match result {
  Ok(_) => 0
  Err(error) => match error { InvalidLimit => 1, LimitExceeded => 2 }
}

fn main() -> Result<(), Error> {
  mut encoder := codec.encoder(1)?
  encoder.put_i64("key", [7])?
  buffer := encoder.finish()
  batch := codec.open(buffer.bytes())?
  column := batch.i64s(0) else { return Err(Error.Invalid) }

  print(error_code(pkg.frame.inner_join_i64(column, column, -1)))
  print(error_code(pkg.frame.inner_join_i64(column, column, 0)))
  exact := pkg.frame.inner_join_i64(column, column, 1)
  exact_len := match exact { Ok(rows) => rows.len(), Err(_) => -1 }
  print(exact_len)

  mut empty_encoder := codec.encoder(0)?
  empty_encoder.put_i64("key", [])?
  empty_buffer := empty_encoder.finish()
  empty_batch := codec.open(empty_buffer.bytes())?
  empty := empty_batch.i64s(0) else { return Err(Error.Invalid) }
  empty_len := match pkg.frame.inner_join_i64(column, empty, 0) {
    Ok(rows) => rows.len()
    Err(_) => -1
  }
  print(empty_len)
  return Ok(())
}
"#;
    if !backend_available() {
        return;
    }
    let files = frame_files(main);
    let output = build_and_run_multi("pkg-frame-limits", &files, "main.align");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n2\n1\n0\n");
}

#[test]
fn frame_package_admission_rejects_noncanonical_root_shapes() {
    let main = "module main\nimport pkg.frame\nfn main() -> i32 = 0\n";
    for (name, source) in [
        (
            "error-name",
            frame_source().replacen("InvalidLimit", "WrongLimit", 1),
        ),
        (
            "bridge-name",
            frame_source().replacen("inner_join_i64_bridge", "wrong_bridge", 1),
        ),
        (
            "wrapper-body",
            frame_source().replacen(
                "inner_join_i64_bridge(left, right, max_pairs)",
                "inner_join_i64_bridge(right, left, max_pairs)",
                1,
            ),
        ),
        (
            "bridge-signature",
            frame_source().replacen("  left: codec.i64_column,", "  left: codec.str_column,", 1),
        ),
        (
            "extra-item",
            format!("{}\nfn extra() -> i64 = 0\n", frame_source()),
        ),
    ] {
        let diagnostics = check_multi_diagnostics(
            &format!("pkg-frame-noncanonical-{name}"),
            &[("pkg/frame.align", source.as_str()), ("main.align", main)],
            "main.align",
        );
        assert!(
            diagnostics.contains("must match the canonical package definition"),
            "{name}:\n{diagnostics}",
        );
    }

    let ordinary = r#"module other
pub fn inner_join_i64_bridge(left: i64, right: i64, max_pairs: i64) -> i64 =
  left + right + max_pairs
"#;
    let control = r#"module main
import other
fn main() -> i32 = other.inner_join_i64_bridge(20, 21, 1) as i32
"#;
    if backend_available() {
        let output = build_and_run_multi(
            "pkg-frame-same-name-control",
            &[("other.align", ordinary), ("main.align", control)],
            "main.align",
        );
        assert_eq!(output.status.code(), Some(42));
    }
}

#[test]
fn frame_lowering_uses_the_two_exact_keyed_native_rows() {
    if !backend_available() {
        return;
    }
    let files = frame_files(I64_MAIN);
    let built = build_per_unit_multi("pkg-frame-llvm", &files, "main.align");
    let frame = built.unit("pkg.frame");
    for optimized in [false, true] {
        let llvm = emit_llvm_ir(&frame.mir, BuildTarget::Baseline, optimized, &[], false)
            .expect("pkg.frame LLVM IR");
        assert!(
            llvm.contains("call i32 @align_rt_frame_inner_join_i64_v1(ptr")
                && llvm.contains(
                    "declare i32 @align_rt_frame_inner_join_i64_v1(ptr, i64, ptr, i64, i64, ptr)"
                ),
            "optimized={optimized}\n{llvm}",
        );
        assert!(
            llvm.contains("call i32 @align_rt_frame_inner_join_str_v1(ptr")
                && llvm.contains("declare i32 @align_rt_frame_inner_join_str_v1(ptr, ptr, i64, ptr, ptr, i64, i64, ptr)"),
            "optimized={optimized}\n{llvm}",
        );
    }
}
