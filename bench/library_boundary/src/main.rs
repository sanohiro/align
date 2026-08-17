use std::hint::black_box;
use std::time::{Duration, Instant};

use align_interface::{
    Effect, Hash128, IFnSig, IParam, IStructDef, IType, ITypeParam, InterfaceSummary, ParamMode,
    ReturnBorrowSummary, ReturnRegionSummary, deserialize, encode_interface_surface, serialize,
    validate_for_import,
};

fn named(path: &str) -> IType {
    IType::Named {
        path: path.to_string(),
        args: Vec::new(),
    }
}

fn interface_fixture() -> InterfaceSummary {
    let mut functions = Vec::with_capacity(512);
    for index in 0..512 {
        let first_mode = if index % 8 == 0 {
            ParamMode::Out
        } else {
            ParamMode::ByValue
        };
        functions.push(IFnSig {
            name: format!("operation_{index:04}"),
            type_params: Vec::new(),
            params: vec![
                IParam {
                    mode: first_mode,
                    ty: IType::Named {
                        path: "slice".to_string(),
                        args: vec![named("i64")],
                    },
                },
                IParam {
                    mode: ParamMode::ByValue,
                    ty: named("i64"),
                },
                IParam {
                    mode: ParamMode::ByValue,
                    ty: named("str"),
                },
            ],
            ret: named("i64"),
            return_borrow: ReturnBorrowSummary::None,
            return_region: ReturnRegionSummary::None,
            return_cleanup: align_sema::hir::ReturnCleanupAbi::None,
            effect: Effect::Pure,
            parallel_transfer_params: Vec::new(),
            generic_body: None,
            resource_hook_body: false,
        });
    }
    let mut summary = InterfaceSummary {
        unit: "bench.surface".to_string(),
        fns: functions,
        structs: Vec::new(),
        owned_json_descriptors: Vec::new(),
        enums: Vec::new(),
        consts: Vec::new(),
        resources: Vec::new(),
        capabilities: Vec::new(),
        interface_hash: Hash128 { lo: 0, hi: 0 },
        impl_hash: Hash128 { lo: 0, hi: 0 },
    };
    summary.interface_hash = Hash128::of(&encode_interface_surface(&summary));
    summary
}

fn run_interface() {
    let summary = interface_fixture();
    let artifact = serialize(&summary);
    assert_eq!(
        deserialize(&artifact).expect("fixture must decode"),
        summary
    );

    let minimum = Duration::from_millis(750);
    let mut iterations = 0_u64;
    let start = Instant::now();
    while start.elapsed() < minimum {
        let decoded = deserialize(black_box(&artifact)).expect("benchmark decode");
        black_box(decoded);
        iterations += 1;
    }
    let elapsed = start.elapsed();
    let bytes = artifact.len() as f64 * iterations as f64;
    let mib_per_second = bytes / (1024.0 * 1024.0) / elapsed.as_secs_f64();

    println!("interface-size\t{}\tbytes", artifact.len());
    println!("decode-throughput\t{mib_per_second:.2}\tMiB/s");
}

fn provenance_fixture() -> String {
    let mut source = String::new();
    for index in (1..256).rev() {
        source.push_str(&format!(
            "pub fn select_{index:04}(first: str, second: str) -> str = \
             select_{previous:04}(first, second)\n",
            previous = index - 1,
        ));
    }
    source.push_str("pub fn select_0000(first: str, second: str) -> str = second\n");
    source.push_str("fn main() -> i32 = 0\n");
    source
}

fn mir_continuation_fixture() -> String {
    let mut source = String::new();
    for index in 0..256 {
        source.push_str(&format!(
            "fn branch_{index:04}(x: i32) -> i32 {{\n\
             \u{20}\u{20}first := if x == {index} {{ x + 1 }} else {{ x - 1 }}\n\
             \u{20}\u{20}second := if first > 0 {{ first * 2 }} else {{ 0 - first }}\n\
             \u{20}\u{20}return if second == x {{ first }} else {{ second }}\n\
             }}\n"
        ));
    }
    source.push_str("fn main() -> i32 = branch_0255(1)\n");
    source
}

fn mir_global_type_fixture() -> String {
    let mut source = String::new();
    for index in 0..512 {
        source.push_str(&format!("Record_{index:04} {{ value: i64 }}\n"));
    }
    source.push_str("fn main() -> i32 = 0\n");
    source
}

fn mir_nominal_link_fixture() -> String {
    let mut source = String::new();
    for index in 0..256 {
        source.push_str(&format!("Record_{index:04} {{ value: i64 }}\n"));
        source.push_str(&format!("Choice_{index:04} {{ Empty, Value(i64) }}\n"));
    }
    source.push_str("extern \"C\" link(\"m\") {\n  fn sqrt(x: f64) -> f64\n}\n");
    source.push_str("fn main() -> i32 = 0\n");
    source
}

fn canonical_type_graph_fixture() -> String {
    let mut source = String::new();
    for index in 0..128 {
        source.push_str(&format!("Canonical_{index:04} {{ value: i64 }}\n"));
        source.push_str(&format!(
            "fn canonical_{index:04}(value: Canonical_{index:04}) -> Canonical_{index:04} = value\n"
        ));
    }
    source.push_str("fn main() -> i32 = 0\n");
    source
}

fn callable_namespace_fixture() -> String {
    let mut source = String::new();
    for index in 0..256 {
        source.push_str(&format!(
            "fn target_{index:04}(value: i64) -> i64 = value + {index}\n"
        ));
        source.push_str(&format!(
            "fn caller_{index:04}(value: i64) -> i64 = target_{index:04}(value)\n"
        ));
    }
    source.push_str("fn main() -> i32 = caller_0255(1) as i32\n");
    source
}

fn import_validation_fixture() -> InterfaceSummary {
    let parameter = ITypeParam {
        name: "T".to_string(),
        bound: None,
    };
    let mut structures = Vec::with_capacity(128);
    for index in 0..128 {
        let (field_type, field_source) = if index == 127 {
            (named("T"), "T".to_string())
        } else {
            (
                IType::Named {
                    path: format!("Layer_{:04}", index + 1),
                    args: vec![named("T")],
                },
                format!("Layer_{:04}<T>", index + 1),
            )
        };
        structures.push(IStructDef {
            name: format!("Layer_{index:04}"),
            type_params: vec![parameter.clone()],
            fields: vec![("value".to_string(), field_type)],
            align: None,
            c_repr: false,
            generic_body: Some(format!(
                "Layer_{index:04}<T> {{ value: {field_source} }}"
            )),
        });
    }
    let root = IType::Named {
        path: "Layer_0000".to_string(),
        args: vec![named("str")],
    };
    let functions = (0..256)
        .map(|index| IFnSig {
            name: format!("borrow_{index:04}"),
            type_params: Vec::new(),
            params: vec![IParam {
                mode: ParamMode::ByValue,
                ty: root.clone(),
            }],
            ret: root.clone(),
            return_borrow: ReturnBorrowSummary::Roots {
                params: vec![0],
                captures: Vec::new(),
            },
            return_region: ReturnRegionSummary::Roots {
                params: vec![0],
                captures: Vec::new(),
            },
            return_cleanup: align_sema::hir::ReturnCleanupAbi::None,
            effect: Effect::Pure,
            parallel_transfer_params: Vec::new(),
            generic_body: None,
            resource_hook_body: false,
        })
        .collect();
    InterfaceSummary {
        unit: "bench.provenance".to_string(),
        fns: functions,
        structs: structures,
        owned_json_descriptors: Vec::new(),
        enums: Vec::new(),
        consts: Vec::new(),
        resources: Vec::new(),
        capabilities: Vec::new(),
        interface_hash: Hash128 { lo: 0, hi: 0 },
        impl_hash: Hash128 { lo: 0, hi: 0 },
    }
}

fn run_provenance() {
    let source = provenance_fixture();
    let mut source_map = align_span::SourceMap::new();
    let (summaries, diagnostics) =
        align_driver::build_interface_summaries(&mut source_map, "provenance.align", &source);
    assert!(!diagnostics.has_errors(), "provenance fixture must check");
    let summary = summaries.first().expect("one benchmark summary");
    let last = summary
        .fns
        .iter()
        .find(|function| function.name == "select_0255")
        .expect("last chained signature");
    assert_eq!(
        last.return_borrow,
        ReturnBorrowSummary::Roots {
            params: vec![1],
            captures: vec![],
        }
    );
    let artifact_bytes: usize = summaries
        .iter()
        .map(|summary| serialize(summary).len())
        .sum();

    let minimum = Duration::from_millis(750);
    let mut iterations = 0_u64;
    let start = Instant::now();
    while start.elapsed() < minimum {
        let mut source_map = align_span::SourceMap::new();
        let checked = align_driver::check(&mut source_map, "provenance.align", black_box(&source));
        assert!(!checked.diags.has_errors());
        black_box(checked.hir);
        iterations += 1;
    }
    let elapsed = start.elapsed();
    let milliseconds = elapsed.as_secs_f64() * 1_000.0 / iterations as f64;
    println!("summary-inference\t{milliseconds:.3}\tms/check\t{artifact_bytes}\tbytes");

    let import_summary = import_validation_fixture();
    validate_for_import(&import_summary).expect("import-validation fixture");
    let mut iterations = 0_u64;
    let start = Instant::now();
    while start.elapsed() < minimum {
        validate_for_import(black_box(&import_summary)).expect("benchmark semantic import");
        iterations += 1;
    }
    let elapsed = start.elapsed();
    let milliseconds = elapsed.as_secs_f64() * 1_000.0 / iterations as f64;
    println!("import-validation\t{milliseconds:.3}\tms/import");

    let global_type_source = mir_global_type_fixture();
    let mut source_map = align_span::SourceMap::new();
    let checked = align_driver::check(
        &mut source_map,
        "mir-global-type.align",
        &global_type_source,
    );
    assert!(
        !checked.diags.has_errors(),
        "MIR global-type fixture must check"
    );
    let mir = align_driver::lower_to_mir(&checked.hir);
    assert_eq!(
        mir.structs
            .iter()
            .filter(|definition| definition.source_name.starts_with("Record_"))
            .count(),
        512,
        "fixture must retain every concrete nominal root"
    );

    let mut iterations = 0_u64;
    let start = Instant::now();
    while start.elapsed() < minimum {
        let mir = align_driver::lower_to_mir(black_box(&checked.hir));
        black_box(mir);
        iterations += 1;
    }
    let elapsed = start.elapsed();
    let milliseconds = elapsed.as_secs_f64() * 1_000.0 / iterations as f64;
    println!(
        "mir-global-type-validation\t{milliseconds:.3}\tms/lower\t{}\ttypes",
        checked.hir.structs.len()
    );

    let nominal_source = mir_nominal_link_fixture();
    let mut source_map = align_span::SourceMap::new();
    let checked = align_driver::check(&mut source_map, "mir-nominal-link.align", &nominal_source);
    assert!(
        !checked.diags.has_errors(),
        "MIR nominal/link fixture must check"
    );
    let mut nominal_hir = checked.hir.clone();
    let original_structs = nominal_hir.structs.len();
    for index in 0..original_structs {
        let mut duplicate = nominal_hir.structs[index].clone();
        duplicate.name = format!("bench_origin_struct_{index}");
        nominal_hir.structs.push(duplicate);
    }
    let original_enums = nominal_hir.enums.len();
    for index in 0..original_enums {
        let mut duplicate = nominal_hir.enums[index].clone();
        duplicate.name = format!("bench_origin_enum_{index}");
        nominal_hir.enums.push(duplicate);
    }
    let nominal_definitions = nominal_hir.structs.len() + nominal_hir.enums.len();
    let mir = align_driver::lower_to_mir(&nominal_hir);
    assert_eq!(
        mir.structs.len() + mir.enums.len(),
        nominal_definitions,
        "fixture must retain all nominal source-shape twins"
    );

    let mut iterations = 0_u64;
    let start = Instant::now();
    while start.elapsed() < minimum {
        let mir = align_driver::lower_to_mir(black_box(&nominal_hir));
        black_box(mir);
        iterations += 1;
    }
    let elapsed = start.elapsed();
    let milliseconds = elapsed.as_secs_f64() * 1_000.0 / iterations as f64;
    println!(
        "mir-nominal-link-validation\t{milliseconds:.3}\tms/lower\t{nominal_definitions}\tdefinitions"
    );

    let mut iterations = 0_u64;
    let start = Instant::now();
    while start.elapsed() < minimum {
        let mir = align_driver::lower_to_mir(black_box(&nominal_hir));
        black_box(mir);
        iterations += 1;
    }
    let elapsed = start.elapsed();
    let milliseconds = elapsed.as_secs_f64() * 1_000.0 / iterations as f64;
    println!(
        "canonical-source-shape-comparison\t{milliseconds:.3}\tms/lower\t{nominal_definitions}\tdefinitions"
    );

    let canonical_source = canonical_type_graph_fixture();
    let mut source_map = align_span::SourceMap::new();
    let canonical_checked = align_driver::check(
        &mut source_map,
        "canonical-type-graph.align",
        &canonical_source,
    );
    assert!(
        !canonical_checked.diags.has_errors(),
        "canonical type-graph fixture must check"
    );
    let canonical_mir = align_driver::lower_to_mir(&canonical_checked.hir);
    let canonical_roots = canonical_mir.fns.len();
    for function in &canonical_mir.fns {
        align_mir::CanonicalTy::from_program(function.ret, &canonical_mir)
            .expect("canonical fixture root");
    }
    let mut iterations = 0_u64;
    let start = Instant::now();
    while start.elapsed() < minimum {
        for function in &canonical_mir.fns {
            let canonical = align_mir::CanonicalTy::from_program(
                black_box(function.ret),
                black_box(&canonical_mir),
            )
            .expect("benchmark canonical root");
            black_box(canonical);
        }
        iterations += 1;
    }
    let elapsed = start.elapsed();
    let milliseconds = elapsed.as_secs_f64() * 1_000.0 / iterations as f64;
    println!(
        "canonical-type-graph\t{milliseconds:.3}\tms/all-roots\t{canonical_roots}\troots"
    );

    let callable_source = callable_namespace_fixture();
    let mut source_map = align_span::SourceMap::new();
    let callable_checked = align_driver::check(
        &mut source_map,
        "mir-callable-namespace.align",
        &callable_source,
    );
    assert!(
        !callable_checked.diags.has_errors(),
        "callable namespace fixture must check"
    );
    let callable_mir = align_driver::lower_to_mir(&callable_checked.hir);
    let callable_count = callable_mir.fns.len();
    assert!(callable_count >= 512, "fixture must retain every callable declaration");
    let mut iterations = 0_u64;
    let start = Instant::now();
    while start.elapsed() < minimum {
        let mir = align_driver::lower_to_mir(black_box(&callable_checked.hir));
        black_box(mir);
        iterations += 1;
    }
    let elapsed = start.elapsed();
    let milliseconds = elapsed.as_secs_f64() * 1_000.0 / iterations as f64;
    println!(
        "mir-callable-namespace-validation\t{milliseconds:.3}\tms/lower\t{callable_count}\tcallables"
    );

    let header_source = provenance_fixture();
    let mut source_map = align_span::SourceMap::new();
    let checked = align_driver::check(
        &mut source_map,
        "mir-header.align",
        &header_source,
    );
    assert!(
        !checked.diags.has_errors(),
        "MIR header fixture must check"
    );
    let valid = align_driver::lower_to_mir(&checked.hir);
    assert!(!valid.fns.is_empty(), "header fixture must lower");
    let mut malformed = checked.hir.clone();
    malformed.fns[0].name.push('\0');
    let invalid = align_driver::lower_to_mir(&malformed);
    assert!(invalid.fns.is_empty(), "malformed header must fail closed");

    let mut iterations = 0_u64;
    let start = Instant::now();
    while start.elapsed() < minimum {
        let valid = align_driver::lower_to_mir(black_box(&checked.hir));
        let invalid = align_driver::lower_to_mir(black_box(&malformed));
        black_box((valid, invalid));
        iterations += 1;
    }
    let elapsed = start.elapsed();
    let milliseconds = elapsed.as_secs_f64() * 1_000.0 / iterations as f64;
    println!(
        "mir-header-validation\t{milliseconds:.3}\tms/valid+malformed-lower\t{}\tfunctions",
        checked.hir.fns.len()
    );

    let continuation_source = mir_continuation_fixture();
    let mut source_map = align_span::SourceMap::new();
    let checked = align_driver::check(
        &mut source_map,
        "mir-continuation.align",
        &continuation_source,
    );
    assert!(
        !checked.diags.has_errors(),
        "MIR continuation fixture must check"
    );
    let mir = align_driver::lower_to_mir(&checked.hir);
    let block_count: usize = mir.fns.iter().map(|function| function.blocks.len()).sum();
    assert!(
        block_count >= 2_000,
        "fixture must retain a high-CFG lowering workload"
    );

    let mut iterations = 0_u64;
    let start = Instant::now();
    while start.elapsed() < minimum {
        let mir = align_driver::lower_to_mir(black_box(&checked.hir));
        black_box(mir);
        iterations += 1;
    }
    let elapsed = start.elapsed();
    let milliseconds = elapsed.as_secs_f64() * 1_000.0 / iterations as f64;
    println!("mir-continuation-lowering\t{milliseconds:.3}\tms/lower\t{block_count}\tblocks");
}

fn return_fixture(kind: &str, calls: u64) -> String {
    let (producer, consume) = match kind {
        "copy-return-control" => (
            "fn produce(value: i64) -> i64 = value\n",
            "fn consume(value: i64) -> i64 = value\n",
        ),
        "move-return-none" => (
            "fn produce(_: i64) -> Option<string> = None\n",
            "fn consume(value: Option<string>) -> i64 = match value { Some(text) => text.len() None => 0 }\n",
        ),
        "move-return-some" => (
            "fn produce(_: i64) -> Option<string> = Some(\"owned\".clone())\n",
            "fn consume(value: Option<string>) -> i64 = match value { Some(text) => text.len() None => 0 }\n",
        ),
        "move-return-err" => (
            "fn produce(_: i64) -> Result<string, string> = Err(\"owned\".clone())\n",
            "fn consume(value: Result<string, string>) -> i64 = match value { Ok(text) => text.len() Err(text) => text.len() }\n",
        ),
        _ => unreachable!("closed benchmark row"),
    };
    format!(
        "{producer}{consume}\
         fn main() -> i32 {{\n\
         \u{20}\u{20}mut index: i64 := 0\n\
         \u{20}\u{20}mut total: i64 := 0\n\
         \u{20}\u{20}loop {{\n\
         \u{20}\u{20}\u{20}\u{20}if index == {calls} {{ break }}\n\
         \u{20}\u{20}\u{20}\u{20}total = total + consume(produce(index))\n\
         \u{20}\u{20}\u{20}\u{20}index = index + 1\n\
         \u{20}\u{20}}}\n\
         \u{20}\u{20}if total < 0 {{ return 1 }}\n\
         \u{20}\u{20}return 0\n\
         }}\n"
    )
}

fn compile_call_fixture(
    directory: &std::path::Path,
    row: &str,
    source: &str,
) -> std::path::PathBuf {
    let mut source_map = align_span::SourceMap::new();
    let checked = align_driver::check(&mut source_map, &format!("{row}.align"), source);
    assert!(
        !checked.diags.has_errors(),
        "{row} fixture must check:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags)
    );
    let mir = align_driver::lower_to_mir(&checked.hir);
    let object = directory.join(format!("{row}.o"));
    let executable = directory.join(row);
    align_driver::emit_object_file(
        &mir,
        &object,
        align_driver::BuildTarget::Baseline,
        align_driver::Profile::Release,
        &[],
        false,
    )
    .unwrap_or_else(|error| panic!("{row} object emission failed: {error}"));
    align_driver::link_executable(
        &object,
        &executable,
        &mir.link_libs,
        align_driver::Profile::Release,
    )
    .unwrap_or_else(|error| panic!("{row} link failed: {error}"));
    executable
}

fn run_call_rows(
    group: &str,
    rows: &[&str],
    calls: u64,
    fixture: fn(&str, u64) -> String,
) {
    assert!(align_driver::backend_available(), "{group} benchmark requires the LLVM backend");
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "align-library-boundary-{group}-{}-{unique}", std::process::id()
    ));
    std::fs::create_dir(&directory).expect("create benchmark directory");
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());

    for &row in rows {
        let source = fixture(row, calls);
        let executable = compile_call_fixture(&directory, row, &source);
        let minimum = Duration::from_millis(750);
        let start = Instant::now();
        let mut processes = 0_u64;
        while start.elapsed() < minimum {
            let status = std::process::Command::new(&executable)
                .status()
                .unwrap_or_else(|error| panic!("{row} execution failed: {error}"));
            assert!(status.success(), "{row} returned {status}");
            processes += 1;
        }
        let elapsed = start.elapsed();
        let nanoseconds = elapsed.as_secs_f64() * 1_000_000_000.0
            / (calls * processes) as f64;
        println!("{row}\t{nanoseconds:.3}\tns/call\t{processes}\tprocesses");
    }
}

fn run_move_return() {
    const ROWS: [&str; 4] = [
        "copy-return-control",
        "move-return-none",
        "move-return-some",
        "move-return-err",
    ];
    run_call_rows("move-return", &ROWS, 100_000, return_fixture);
}

fn borrow_fixture(kind: &str, calls: u64) -> String {
    match kind {
        "by-value-call-control" => format!(
            "fn inspect(value: i64) -> i64 = value + 1\n\
             fn main() -> i32 {{ mut index: i64 := 0; mut total: i64 := 0; loop {{ if index == {calls} {{ break }}; total = total + inspect(index); index = index + 1 }}; if total < 0 {{ return 1 }}; return 0 }}\n"
        ),
        "shared-borrow-call" => format!(
            "fn inspect(borrow value: string) -> i64 = value.len()\n\
             fn main() -> i32 {{ value := \"shared\".clone(); mut index: i64 := 0; mut total: i64 := 0; loop {{ if index == {calls} {{ break }}; total = total + inspect(value); index = index + 1 }}; if total < 0 {{ return 1 }}; return 0 }}\n"
        ),
        "copy-aggregate-value-control" | "copy-aggregate-shared-borrow" => {
            let mode = if kind == "copy-aggregate-shared-borrow" {
                "borrow "
            } else {
                ""
            };
            format!(
                "Wide {{ a: i64, b: i64, c: i64, d: i64, e: i64, f: i64, g: i64, h: i64 }}\n\
                 fn inspect({mode}value: Wide) -> i64 = value.a + value.b + value.c + value.d + value.e + value.f + value.g + value.h\n\
                 fn main() -> i32 {{ value := Wide {{ a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8 }}; mut index: i64 := 0; mut total: i64 := 0; loop {{ if index == {calls} {{ break }}; total = total + inspect(value); index = index + 1 }}; if total < 0 {{ return 1 }}; return 0 }}\n"
            )
        }
        "exclusive-copy-control" => format!(
            "fn increment(value: i64) -> i64 = value + 1\n\
             fn main() -> i32 {{ mut index: i64 := 0; mut value: i64 := 0; loop {{ if index == {calls} {{ break }}; value = increment(value); index = index + 1 }}; if value != {calls} {{ return 1 }}; return 0 }}\n"
        ),
        "exclusive-copy-call" => format!(
            "fn increment(borrow mut value: i64) {{ value = value + 1 }}\n\
             fn main() -> i32 {{ mut index: i64 := 0; mut value: i64 := 0; loop {{ if index == {calls} {{ break }}; increment(value); index = index + 1 }}; if value != {calls} {{ return 1 }}; return 0 }}\n"
        ),
        "exclusive-move-replace" => format!(
            "fn replace(borrow mut value: string) {{ value = \"replacement\".clone() }}\n\
             fn main() -> i32 {{ mut index: i64 := 0; mut value := \"initial\".clone(); loop {{ if index == {calls} {{ break }}; replace(value); index = index + 1 }}; if value.len() != 11 {{ return 1 }}; return 0 }}\n"
        ),
        _ => unreachable!("closed borrowed-call benchmark row"),
    }
}

fn run_shared_borrow() {
    const ROWS: [&str; 4] = [
        "by-value-call-control",
        "shared-borrow-call",
        "copy-aggregate-value-control",
        "copy-aggregate-shared-borrow",
    ];
    run_call_rows("shared-borrow", &ROWS, 100_000, borrow_fixture);
}

fn run_exclusive_borrow() {
    const ROWS: [&str; 3] = [
        "exclusive-copy-control",
        "exclusive-copy-call",
        "exclusive-move-replace",
    ];
    run_call_rows("exclusive-borrow", &ROWS, 100_000, borrow_fixture);
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("interface") => run_interface(),
        Some("provenance") => run_provenance(),
        Some("move-return") => run_move_return(),
        Some("shared-borrow") => run_shared_borrow(),
        Some("exclusive-borrow") => run_exclusive_borrow(),
        Some(other) => {
            eprintln!("unknown library-boundary benchmark group `{other}`");
            std::process::exit(2);
        }
        None => {
            eprintln!("usage: run.sh interface|provenance|move-return|shared-borrow|exclusive-borrow");
            std::process::exit(2);
        }
    }
}
