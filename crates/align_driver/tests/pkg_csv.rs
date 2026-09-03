//! `pkg.csv` owner: canonical generic formation, checked direct-fill lowering, runtime behavior,
//! and the transitive non-Send destination-region gate.

mod common;
use common::*;

fn csv_source() -> &'static str { fixture("apps/csv/pkg/csv.align") }
fn descriptor_source() -> &'static str { fixture("apps/csv/pkg/csv/internal/descriptor.align") }

fn files(main: &str) -> [(&str, &str); 3] {
    [
        ("pkg/csv.align", csv_source()),
        ("pkg/csv/internal/descriptor.align", descriptor_source()),
        ("main.align", main),
    ]
}

const DECODE: &str = r#"module main
import pkg.csv

Row { score: i64, active: bool, symbol: str, mark: char }

fn main() -> i32 {
  arena out {
    options := pkg.csv.DecodeOptions {
      header: pkg.csv.Header.Present,
      line_ending: pkg.csv.LineEnding.Lf,
      max_rows: 2,
    }
    decoded: Result<soa<Row>, pkg.csv.Error> := pkg.csv.decode(
      "ignored,symbol,active,score,mark\nskip,A,true,-7,é\nnot-a-number,\"say \"\"hi\"\"\",false,42,\"\"\"\"",
      out,
      options,
    )
    rows := decoded else { return 90 }
    print(rows.score.sum())
    print(rows.active.count())
    print(rows.symbol[0])
    print(rows.symbol[1])
    print(rows.mark[0])
    print(rows.mark[1])
  }
  return 0
}
"#;

#[test]
fn canonical_decode_runs_whole_and_per_unit() {
    let files = files(DECODE);
    let checked = diff_check_multi("pkg-csv-check", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
    if !backend_available() { return; }
    for output in [
        build_and_run_multi("pkg-csv-whole", &files, "main.align"),
        build_per_unit_multi("pkg-csv-units", &files, "main.align").link_and_run(),
    ] {
        assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "35\n2\nA\nsay \"hi\"\né\n\"\n");
    }
}

#[test]
fn checked_operation_keeps_explicit_status_cfg_and_exact_a123_call() {
    let files = files(DECODE);
    let mir = whole_mir_multi("pkg-csv-mir", &files, "main.align");
    assert!(mir.contains("csv_decode"), "{mir}");
    assert!(mir.contains("runtime process_abort"), "{mir}");
    if !backend_available() { return; }
    let ir = emit_llvm_multi("pkg-csv-llvm", &files, "main.align");
    assert!(ir.contains("@align_rt_csv_decode_soa_v1("), "{ir}");
    assert!(ir.contains("i64 4, ptr"), "the descriptor count must be four: {ir}");
    assert!(ir.contains("csv_fields"), "{ir}");
    assert!(
        !ir.contains("call i32 @align_rt_json_decode_soa_v1"),
        "CSV must not route through JSON: {ir}",
    );
}

#[test]
fn public_errors_map_exactly_and_zero_rows_succeed() {
    let main = r#"module main
import pkg.csv
Row { value: i8 }
fn code(result: Result<soa<Row>, pkg.csv.Error>) -> i32 = match result {
  Ok(rows) => rows.len() as i32,
  Err(error) => match error {
    Invalid => 10,
    LimitExceeded => 20,
  },
}
fn main() -> i32 {
  arena out {
    absent := pkg.csv.DecodeOptions { header: pkg.csv.Header.Absent, line_ending: pkg.csv.LineEnding.Lf, max_rows: 1 }
    zero := pkg.csv.DecodeOptions { header: pkg.csv.Header.Absent, line_ending: pkg.csv.LineEnding.Lf, max_rows: 0 }
    if code(pkg.csv.decode("+1", out, absent)) != 10 { return 1 }
    if code(pkg.csv.decode("1\n2", out, absent)) != 20 { return 2 }
    if code(pkg.csv.decode("", out, zero)) != 0 { return 3 }
  }
  return 0
}
"#;
    let files = files(main);
    let checked = diff_check_multi("pkg-csv-errors-check", &files, "main.align");
    assert!(!checked.whole_errors && !checked.per_unit_errors, "{}\n{}", checked.whole_diags, checked.per_unit_diags);
    if backend_available() {
        let output = build_and_run_multi("pkg-csv-errors-run", &files, "main.align");
        assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn generic_forwarding_and_explicit_row_layouts_keep_the_soa_plain_domain() {
    let main = r#"module main
import pkg.csv
layout(C) CRow { small: i8, wide: i64 }
align(16) ARow { value: f64 }
fn forward<R: SoaPlain>(input: str, out: region, options: pkg.csv.DecodeOptions) -> Result<soa<R>, pkg.csv.Error> =
  pkg.csv.decode(input, out, options)
fn main() -> i32 {
  arena out {
    options := pkg.csv.DecodeOptions { header: pkg.csv.Header.Absent, line_ending: pkg.csv.LineEnding.Lf, max_rows: 1 }
    c: Result<soa<CRow>, pkg.csv.Error> := forward("2,40", out, options)
    a: Result<soa<ARow>, pkg.csv.Error> := forward("0.5", out, options)
    c_rows := c else { return 1 }
    a_rows := a else { return 2 }
    return c_rows.small[0] as i32 + c_rows.wide[0] as i32 + a_rows.value[0] as i32
  }
}

"#;
    let accepted_files = files(main);
    let checked = diff_check_multi("pkg-csv-generic-layouts", &accepted_files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
    if backend_available() {
        let output = build_and_run_multi("pkg-csv-generic-layouts-run", &accepted_files, "main.align");
        assert_eq!(output.status.code(), Some(42), "{}", String::from_utf8_lossy(&output.stderr));
    }

    let invalid = r#"module main
import pkg.csv
Owned { value: string }
fn main() -> i32 {
  arena out {
    options := pkg.csv.DecodeOptions { header: pkg.csv.Header.Absent, line_ending: pkg.csv.LineEnding.Lf, max_rows: 1 }
    rows: Result<soa<Owned>, pkg.csv.Error> := pkg.csv.decode("x", out, options)
    return 0
  }
}
"#;
    let rendered = check_multi_diagnostics("pkg-csv-non-soa-plain", &files(invalid), "main.align");
    assert!(
        rendered.contains("soa<T> requires a non-empty struct of primitive-scalar or `str` fields"),
        "{rendered}",
    );
}

#[test]
fn absent_headers_do_not_add_a_csv_schema_width_cap() {
    let field_count = 1025usize;
    let declaration = (0..field_count)
        .map(|index| format!("f{index}: i8"))
        .collect::<Vec<_>>()
        .join(", ");
    let row = (0..field_count).map(|_| "1").collect::<Vec<_>>().join(",");
    let main = format!(
        "module main\nimport pkg.csv\nWide {{ {declaration} }}\n\nfn main() -> i32 {{\n  arena out {{\n    options := pkg.csv.DecodeOptions {{ header: pkg.csv.Header.Absent, line_ending: pkg.csv.LineEnding.Lf, max_rows: 1 }}\n    decoded: Result<soa<Wide>, pkg.csv.Error> := pkg.csv.decode(\"{row}\", out, options)\n    rows := decoded else {{ return 2 }}\n    return rows.f1024[0] as i32\n  }}\n}}\n"
    );
    let files = files(&main);
    let checked = diff_check_multi("pkg-csv-wide-absent", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
    if backend_available() {
        let output = build_and_run_multi("pkg-csv-wide-absent-run", &files, "main.align");
        assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn owned_input_retention_is_precise_for_primitive_and_string_rows() {
    let primitive = r#"module main
import pkg.csv
Row { value: i64 }
fn main() -> i32 {
  arena out {
    mut source := "1".clone()
    options := pkg.csv.DecodeOptions { header: pkg.csv.Header.Absent, line_ending: pkg.csv.LineEnding.Lf, max_rows: 1 }
    decoded: Result<soa<Row>, pkg.csv.Error> := pkg.csv.decode(source, out, options)
    source = "2".clone()
    rows := decoded else { return 2 }
    return rows.value[0] as i32
  }
}
"#;
    let primitive_files = files(primitive);
    let checked = diff_check_multi("pkg-csv-primitive-input-release", &primitive_files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
    if backend_available() {
        let output = build_and_run_multi("pkg-csv-primitive-input-release-run", &primitive_files, "main.align");
        assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    }

    let strings = primitive.replace("Row { value: i64 }", "Row { value: str }");
    let rendered = check_multi_diagnostics("pkg-csv-string-input-retained", &files(&strings), "main.align");
    assert!(
        rendered.contains("borrow") || rendered.contains("invalidated") || rendered.contains("does not live long enough"),
        "{rendered}",
    );
}

#[test]
fn destination_region_is_rejected_through_nested_function_environments() {
    let spawn = r#"module main
import pkg.csv
Row { value: i64 }
fn invoke(f: fn() -> i32) -> i32 = f()
fn main() -> i32 {
  arena out {
    options := pkg.csv.DecodeOptions { header: pkg.csv.Header.Absent, line_ending: pkg.csv.LineEnding.Lf, max_rows: 1 }
    inner := fn {
      rows: Result<soa<Row>, pkg.csv.Error> := pkg.csv.decode("1", out, options)
      0
    }
    call := fn { invoke(inner) }
    task_group {
      task := spawn(fn { invoke(call) })
      wait()
      print(task.get())
    }
  }
  return 0
}
"#;
    let par_map = r#"module main
import pkg.csv
Row { value: i64 }
fn decode_one(out: region, options: pkg.csv.DecodeOptions) -> i64 {
  decoded: Result<soa<Row>, pkg.csv.Error> := pkg.csv.decode("1", out, options)
  return 0
}
fn main() -> i32 {
  arena out {
    options := pkg.csv.DecodeOptions { header: pkg.csv.Header.Absent, line_ending: pkg.csv.LineEnding.Lf, max_rows: 1 }
    print([1, 2].par_map(fn value: i64 {
      decode_one(out, options)
      value
    }).sum())
  }
  return 0
}
"#;
    for (name, main) in [("spawn", spawn), ("par-map", par_map)] {
        let rendered = check_multi_diagnostics(&format!("pkg-csv-worker-region-{name}"), &files(main), "main.align");
        assert!(
            rendered.contains("parallel worker cannot invoke a function that reaches a region capability"),
            "{name}: {rendered}",
        );
    }
}

#[test]
fn canonical_source_and_private_empty_module_are_sealed() {
    for (name, root, internal) in [
        ("root-body", csv_source().replace("descriptor.decode", "descriptor.other"), descriptor_source().to_owned()),
        ("root-bound", csv_source().replace("R: SoaPlain", "R"), descriptor_source().to_owned()),
        ("root-tag", csv_source().replace("  Present\n  Absent", "  Absent\n  Present"), descriptor_source().to_owned()),
        ("internal-item", csv_source().to_owned(), "module pkg.csv.internal.descriptor\nfn decode() -> i32 = 0\n".to_owned()),
    ] {
        let main = "module main\nimport pkg.csv\nfn main() -> i32 = 0\n";
        let files = [("pkg/csv.align", root.as_str()), ("pkg/csv/internal/descriptor.align", internal.as_str()), ("main.align", main)];
        assert!(check_multi_errs(&format!("pkg-csv-sealed-{name}"), &files, "main.align"), "{name}");
    }
}
