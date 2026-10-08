//! `pkg.csv` owner: canonical generic formation, checked direct-fill lowering, runtime behavior,
//! the transitive non-Send destination-region gate, and the bounded summary application.

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
Wrap<T> { value: T }
fn keep<T>(value: Wrap<T>) -> Wrap<T> = value
fn forward<R: SoaPlain>(input: str, out: region, options: pkg.csv.DecodeOptions) -> Result<soa<R>, pkg.csv.Error> =
  pkg.csv.decode(input, out, options)
fn main() -> i32 {
  arena out {
    options := pkg.csv.DecodeOptions { header: pkg.csv.Header.Absent, line_ending: pkg.csv.LineEnding.Lf, max_rows: 1 }
    c: Result<soa<CRow>, pkg.csv.Error> := forward("2,40", out, options)
    a: Result<soa<ARow>, pkg.csv.Error> := forward("0.5", out, options)
    w: Result<soa<Wrap<i64>>, pkg.csv.Error> := forward("0", out, options)
    c_rows := c else { return 1 }
    a_rows := a else { return 2 }
    w_rows := w else { return 3 }
    _ := keep(Wrap { value: 0 })
    return c_rows.small[0] as i32 + c_rows.wide[0] as i32 + a_rows.value[0] as i32 + w_rows.value[0] as i32
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

// File-backed streams avoid pipe-pressure deadlocks. The child guard is armed immediately after
// spawn, and one deadline includes execution, interrupted probes and kill/reap on failure.
struct CsvExampleChild {
    child: Option<std::process::Child>,
    deadline: std::time::Instant,
}
impl CsvExampleChild {
    fn spawn(command: &mut std::process::Command) -> Self {
        use std::os::unix::process::CommandExt;
        let child = command
            .process_group(0)
            .spawn()
            .expect("spawn CSV example owner");
        Self {
            child: Some(child),
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
        }
    }
    fn wait(mut self) -> std::process::ExitStatus {
        let work_deadline = self.deadline - std::time::Duration::from_secs(5);
        loop {
            assert!(
                std::time::Instant::now() < work_deadline,
                "CSV example work deadline"
            );
            match self.child.as_mut().expect("live owned child").try_wait() {
                Ok(Some(status)) => {
                    self.child.take();
                    return status;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(1)),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => panic!("poll CSV example: {error}"),
            }
        }
    }
}
impl Drop for CsvExampleChild {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        if let Ok(pid) = i32::try_from(child.id()) {
            loop {
                // SAFETY: the live owned application child leads its fresh process group.
                if unsafe { libc::kill(-pid, libc::SIGKILL) } == 0 {
                    break;
                }
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted
                    || std::time::Instant::now() >= self.deadline
                {
                    break;
                }
            }
        }
        loop {
            match child.kill() {
                Err(error)
                    if error.kind() == std::io::ErrorKind::Interrupted
                        && std::time::Instant::now() < self.deadline => {}
                _ => break,
            }
        }
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => {
                    eprintln!("CSV example cleanup probe failed: {error}");
                    break;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(1)),
            }
            if std::time::Instant::now() >= self.deadline {
                eprintln!("CSV example kill/reap deadline exceeded");
                break;
            }
        }
    }
}

fn run_summary(
    exe: &std::path::Path,
    root: &std::path::Path,
    bytes: &[u8],
    args: &[&str],
    readonly_stdout: bool,
) -> std::process::Output {
    let input = root.join("input.csv");
    std::fs::write(&input, bytes).unwrap();
    let output = root.join("stdout");
    std::fs::write(&output, b"").unwrap();
    let stdout = if readonly_stdout {
        std::fs::File::open(&input).unwrap()
    } else {
        std::fs::File::create(&output).unwrap()
    };
    let mut command = std::process::Command::new(exe);
    command.args(args).current_dir(root)
        .stdin(std::process::Stdio::null()).stdout(stdout)
        .stderr(std::fs::File::create(root.join("stderr")).unwrap());
    let status = CsvExampleChild::spawn(&mut command).wait();
    assert_eq!(std::fs::read(input).unwrap(), bytes, "input remains unchanged");
    std::process::Output {
        status,
        stdout: std::fs::read(output).unwrap(),
        stderr: std::fs::read(root.join("stderr")).unwrap(),
    }
}

fn summary_ok(output: &std::process::Output, rows: i64, active: i64, sum: i64) {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(output.stdout, format!("rows={rows}\nactive={active}\nscore-sum={sum}\n").as_bytes());
}

fn summary_error(output: &std::process::Output, code: i32) {
    assert_eq!(output.status.code(), Some(code.clamp(1, 255)), "{output:?}");
    assert!(output.stdout.is_empty(), "no summary on refusal: {output:?}");
    assert_eq!(output.stderr, format!("error: code {code}\n").as_bytes(), "{output:?}");
}

#[test]
fn bounded_csv_summary_example_admission_and_output() {
    assert!(backend_available(), "CSV application owner requires LLVM");
    let source = fixture("apps/csv/main.align");
    let project = files(source);
    let whole = build_exe_multi("csv-summary-whole", &project, "main.align");
    let units = build_per_unit_multi("csv-summary-units", &project, "main.align");
    let objects = units.emit_objects(false);
    let refs: Vec<_> = objects.iter().map(|p| p.as_path()).collect();
    let unit_exe = units.dir.join(format!("summary{}", std::env::consts::EXE_SUFFIX));
    link_objects(&align_driver::CDriver::default(), &refs, &unit_exe,
        &units.link_libs_union(), Profile::Release).unwrap();
    let stage = align_driver::ArtifactStage::temp("csv-summary-inputs").unwrap();
    let root = stage.path();
    let fifo = std::ffi::CString::new(root.join("fifo").as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: the owned fixture path is NUL-terminated and names no existing entry.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    std::os::unix::fs::symlink(root.join("input.csv"), root.join("link.csv")).unwrap();
    let ordinary = b"active,score\ntrue,7\nfalse,100\ntrue,-2\n";
    for exe in [&whole.exe, &unit_exe] {
        let default = ["--file", "input.csv"];
        summary_ok(&run_summary(exe, root, ordinary, &default, false), 3, 2, 5);
        summary_ok(&run_summary(exe, root, ordinary, &["--file", "link.csv"], false), 3, 2, 5);
        summary_ok(&run_summary(exe, root, b"active,score", &default, false), 0, 0, 0);
        summary_ok(&run_summary(exe, root, b"active,score\n", &["--file", "input.csv", "--max-rows", "0"], false), 0, 0, 0);
        summary_ok(&run_summary(exe, root,
            "\u{feff}note,score,active\n\"say \"\"hi\"\"\n世界\",2147483647,true\nx,2147483647,true".as_bytes(),
            &default, false), 2, 2, 4294967294);
        summary_ok(&run_summary(exe, root, b"score,active\n-2147483648,true\n-2147483648,true",
            &default, false), 2, 2, -4294967296);
        summary_ok(&run_summary(exe, root, b"active,score\r\ntrue,9\r\n",
            &["--file", "input.csv", "--crlf"], false), 1, 1, 9);
        summary_ok(&run_summary(exe, root, b"active,score\nfalse,9\n",
            &["--file", "input.csv", "--max-rows", "1000000"], false), 1, 0, 0);

        let byte_count = ordinary.len().to_string();
        summary_ok(&run_summary(exe, root, ordinary,
            &["--file", "input.csv", "--max-input-bytes", &byte_count, "--max-rows", "3"], false), 3, 2, 5);
        let too_short = (ordinary.len() - 1).to_string();
        summary_error(&run_summary(exe, root, ordinary,
            &["--file", "input.csv", "--max-input-bytes", &too_short], false), -1);
        for row_cap in ["0", "2"] {
            summary_error(&run_summary(exe, root, ordinary,
                &["--file", "input.csv", "--max-rows", row_cap], false), -1);
        }
        summary_error(&run_summary(exe, root, b"x",
            &["--file", "input.csv", "--max-input-bytes", "0"], false), -1);
        summary_error(&run_summary(exe, root, b"",
            &["--file", "input.csv", "--max-input-bytes", "0"], false), 2);

        // UTF-8, quoted records and EOF independently straddle the fixed read window.
        for size in [65535, 65536, 65537, 131091] {
            let mut input = b"active,score,note\ntrue,1,\"".to_vec();
            input.resize(size - 1, b'x');
            input.push(b'"');
            let maximum = size.to_string();
            summary_ok(&run_summary(exe, root, &input,
                &["--file", "input.csv", "--max-input-bytes", &maximum], false), 1, 1, 1);
        }
        let mut split = b"active,score,note\ntrue,1,\"".to_vec();
        split.resize(65535, b'x');
        split.extend_from_slice("é\n\"\"quoted\"\"\"".as_bytes());
        summary_ok(&run_summary(exe, root, &split, &default, false), 1, 1, 1);

        for bad in [
            b"".as_slice(), b"active\ntrue", b"active,active,score\ntrue,false,1",
            b"active,score\ntrue,1,extra", b"active,score\ntrue", b"active,score\nTRUE,1",
            b"active,score\ntrue,2147483648", b"active,score\ntrue,-2147483649",
            b"active,score\ntrue,1\ntrue,no", b"active,score,note\ntrue,1,\"unclosed",
            b"active,score,note\ntrue,1,\"closed\"x", b"active,score\r\ntrue,1\r\n",
            b"active,score,note\ntrue,1,\xff", b"active,score,note\ntrue,1,\xc3",
        ] {
            summary_error(&run_summary(exe, root, bad, &default, false), 2);
        }
        summary_error(&run_summary(exe, root, ordinary,
            &["--file", "input.csv", "--crlf"], false), 2);

        for args in [
            vec![], vec!["--file", ""], vec!["--unknown"], vec!["--file"],
            vec!["--file", "missing", "--max-input-bytes", "-1"],
            vec!["--file", "missing", "--max-input-bytes", "67108865"],
            vec!["--file", "missing", "--max-rows", "-1"],
            vec!["--file", "missing", "--max-rows", "1000001"],
            vec!["--file", "fifo", "--max-rows", "nan"],
            vec!["--file", "fifo"], vec!["--file", "."],
        ] {
            summary_error(&run_summary(exe, root, ordinary, &args, false), 2);
        }
        summary_error(&run_summary(exe, root, ordinary, &["--file", "missing"], false), 1);
        summary_error(&run_summary(exe, root, ordinary,
            &["--file", "missing", "--max-input-bytes", "67108864"], false), 1);
        let help = run_summary(exe, root, ordinary, &["--help"], false);
        assert_eq!(help.status.code(), Some(0), "{help:?}");
        assert!(help.stderr.is_empty(), "{help:?}");
        let usage = String::from_utf8(help.stdout).unwrap();
        for flag in ["file", "max-input-bytes", "max-rows", "crlf", "help"] {
            assert!(usage.contains(flag), "{usage}");
        }
        summary_error(&run_summary(exe, root, ordinary, &default, true), libc::EBADF);
    }
}

#[test]
fn bounded_csv_summary_example_propagates_buffer_refusal() {
    assert!(backend_available(), "CSV application owner requires LLVM");
    let source = fixture("apps/csv/main.align");
    let stage = align_driver::ArtifactStage::temp("csv-summary-refused").unwrap();
    for call in ["buffer.try_new(maximum)", "buffer.try_new(65536)"] {
        assert_eq!(source.matches(call).count(), 1);
        let refused = format!("{}\nfn refused_buffer() -> Result<buffer, Error> = Err(Error.Code(12))\n",
            source.replace(call, "refused_buffer()"));
        let exe = build_exe_multi("csv-summary-refused", &files(&refused), "main.align");
        for bytes in [b"".as_slice(), b"active,score\ntrue,1"] {
            summary_error(&run_summary(&exe.exe, stage.path(), bytes,
                &["--file", "input.csv"], false), 12);
        }
    }
}
