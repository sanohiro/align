//! L2b-a2-am-e: exact source-entry and C-entry ABI.

mod common;
use common::*;

use std::process::Command;

const UNIT: &str = "fn main() {}\n";
const I32: &str = "fn main() -> i32 = 17\n";
const RESULT_OK: &str = "fn main() -> Result<(), Error> { return Ok(()) }\n";
const RESULT_ERR: &str = "fn main() -> Result<(), Error> { return Err(error(23)) }\n";

fn whole_mir(name: &str, source: &str) -> align_driver::MirProgram {
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, name, source);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags)
    );
    lower_to_mir(&checked.hir)
}

fn definition_line<'a>(ir: &'a str, symbol: &str) -> &'a str {
    let bare = format!("@{symbol}(");
    let quoted = format!("@\"{symbol}\"(");
    ir.lines()
        .find(|line| {
            line.starts_with("define ") && (line.contains(&bare) || line.contains(&quoted))
        })
        .unwrap_or_else(|| panic!("missing definition for @{symbol}:\n{ir}"))
}

fn align_symbol(value: &str) -> String {
    let hex = value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("align_fn${}${hex}", value.len())
}

#[test]
fn whole_and_per_unit_entries_preserve_exit_behavior() {
    if !backend_available() {
        return;
    }
    for (name, source, expected) in [
        ("unit", UNIT, 0),
        ("i32", I32, 17),
        ("result-ok", RESULT_OK, 0),
        ("result-err", RESULT_ERR, 23),
    ] {
        let whole = build_and_run(&format!("main-abi-whole-{name}"), source);
        assert_eq!(whole.status.code(), Some(expected), "whole {name}");

        let per = build_per_unit_multi(
            &format!("main-abi-per-{name}"),
            &[("main.align", source)],
            "main.align",
        )
        .link_and_run();
        assert_eq!(per.status.code(), Some(expected), "per-unit {name}");
    }
}

#[test]
fn argv_result_entry_is_exact_in_both_build_paths() {
    if !backend_available() {
        return;
    }
    let source = "fn main(args: array<str>) -> Result<(), Error> {\n  print(args.len())\n  return Ok(())\n}\n";
    let whole = build_and_run_args("main-abi-argv-whole", source, &["one", "two"]);
    assert_eq!(whole.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&whole.stdout), "3\n");

    let built = build_per_unit_multi("main-abi-argv-per", &[("main.align", source)], "main.align");
    let objects = built.emit_objects(false);
    let object_refs = objects
        .iter()
        .map(|path| path.as_path())
        .collect::<Vec<_>>();
    let exe = built
        .dir
        .join(format!("argv{}", std::env::consts::EXE_SUFFIX));
    link_objects(
        &align_driver::CDriver::default(),
        &object_refs,
        &exe,
        &built.link_libs_union(),
        Profile::Release,
    )
    .expect("link argv entry");
    let per = Command::new(&exe)
        .args(["one", "two"])
        .output()
        .expect("run argv entry");
    assert_eq!(per.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&per.stdout), "3\n");
}

#[test]
fn non_entry_generic_main_is_an_ordinary_module_function() {
    if !backend_available() {
        return;
    }
    let library = "module lib\npub fn main<T>(value: T) -> T = value\n";
    let entry = "module main\nimport lib\nfn main() -> i32 {\n  if lib.main(true) { return 0 }\n  return 1\n}\n";
    let files = &[("lib.align", library), ("main.align", entry)];
    let whole = build_and_run_multi("main-abi-module-whole", files, "main.align");
    let per = build_per_unit_multi("main-abi-module-per", files, "main.align").link_and_run();
    assert_eq!(whole.status.code(), Some(0));
    assert_eq!(per.status.code(), Some(0));
}

#[test]
fn scalar_boundary_values_survive_whole_and_per_unit_calls() {
    if !backend_available() {
        return;
    }
    let library = concat!(
        "module scalar\n",
        "pub fn bool_id(value: bool) -> bool = value\n",
        "pub fn u8_id(value: u8) -> u8 = value\n",
        "pub fn u16_id(value: u16) -> u16 = value\n",
        "pub fn u32_id(value: u32) -> u32 = value\n",
        "pub fn i8_id(value: i8) -> i8 = value\n",
        "pub fn i16_id(value: i16) -> i16 = value\n",
        "pub fn i32_id(value: i32) -> i32 = value\n",
        "pub fn char_id(value: char) -> char = value\n",
    );
    let entry = concat!(
        "module main\n",
        "import scalar\n",
        "fn indirect(value: bool) -> bool { callback := scalar.bool_id\n return callback(value) }\n",
        "fn spawned() -> bool { return task_group { task := spawn(fn { true })\n wait()\n task.get() } }\n",
        "fn main() -> i32 {\n",
        "  if !scalar.bool_id(true) { return 1 }\n",
        "  if scalar.bool_id(false) { return 2 }\n",
        "  if !indirect(true) { return 3 }\n",
        "  if !spawned() { return 4 }\n",
        "  if scalar.u8_id(255) != 255 { return 5 }\n",
        "  if scalar.u16_id(65535) != 65535 { return 6 }\n",
        "  if scalar.u32_id(4294967295) != 4294967295 { return 7 }\n",
        "  if scalar.i8_id(-128) != -128 { return 8 }\n",
        "  if scalar.i16_id(-32768) != -32768 { return 9 }\n",
        "  if scalar.i32_id(-2147483648) != -2147483648 { return 10 }\n",
        "  if scalar.char_id('\\0') != '\\0' { return 11 }\n",
        "  if scalar.char_id('\\u{10ffff}') != '\\u{10ffff}' { return 12 }\n",
        "  return 0\n",
        "}\n",
    );
    let files = &[("scalar.align", library), ("main.align", entry)];
    let whole = build_and_run_multi("scalar-abi-whole", files, "main.align");
    let per = build_per_unit_multi("scalar-abi-per", files, "main.align").link_and_run();
    assert_eq!(whole.status.code(), Some(0), "whole-program scalar boundary");
    assert_eq!(per.status.code(), Some(0), "per-unit scalar boundary");
}

#[test]
fn raw_and_optimized_whole_and_per_unit_c_signatures_are_exact() {
    if !backend_available() {
        return;
    }
    for (name, source, argv, wrapped) in [
        ("unit", UNIT, false, true),
        ("i32", I32, false, false),
        ("result", RESULT_OK, false, true),
        (
            "argv-result",
            "fn main(args: array<str>) -> Result<(), Error> { return Ok(()) }\n",
            true,
            true,
        ),
    ] {
        let whole = whole_mir(&format!("main-abi-ir-whole-{name}"), source);
        let per = build_per_unit_multi(
            &format!("main-abi-ir-per-{name}"),
            &[("main.align", source)],
            "main.align",
        );
        let per_entry = per
            .walk
            .units
            .iter()
            .find(|unit| unit.is_entry)
            .expect("per-unit build has an entry artifact");
        for (path, mir) in [("whole", &whole), ("per-unit", &per_entry.mir)] {
            for optimized in [false, true] {
                let ir = emit_llvm_ir(mir, BuildTarget::Baseline, align_driver::Profile::Release, optimized, &[], false)
                    .unwrap_or_else(|error| panic!("{path} {name} LLVM: {error}"));
                let c_main = definition_line(&ir, "main");
                let (prefix, remainder) = c_main.split_once("@main(").expect("main signature");
                assert_eq!(
                    prefix.split_whitespace().last(),
                    Some("i32"),
                    "{path} {name} optimized={optimized}: {c_main}"
                );
                let parameters = remainder.split_once(')').expect("closed main signature").0;
                if argv {
                    let parameter_types = parameters
                        .split(',')
                        .map(|parameter| parameter.split_whitespace().next())
                        .collect::<Vec<_>>();
                    assert_eq!(
                        parameter_types,
                        [Some("i32"), Some("ptr")],
                        "{path} {name} optimized={optimized}: {c_main}"
                    );
                } else {
                    assert_eq!(
                        parameters, "",
                        "{path} {name} optimized={optimized}: {c_main}"
                    );
                }
                assert!(
                    !c_main.starts_with("define internal "),
                    "the C entry stays externally linked: {c_main}"
                );

                if !optimized {
                    if wrapped {
                        let body = align_symbol("main");
                        assert!(
                            definition_line(&ir, &body).starts_with("define internal "),
                            "{path} {name} Align body must be internal"
                        );
                    } else {
                        assert!(
                            !ir.contains(&align_symbol("main")),
                            "{path} exact i32 is the direct C entry"
                        );
                    }
                }
            }
        }
    }
}
