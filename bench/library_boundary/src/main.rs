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
            effect: Effect::Pure,
            generic_body: None,
        });
    }
    let mut summary = InterfaceSummary {
        unit: "bench.surface".to_string(),
        fns: functions,
        structs: Vec::new(),
        enums: Vec::new(),
        consts: Vec::new(),
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
            effect: Effect::Pure,
            generic_body: None,
        })
        .collect();
    InterfaceSummary {
        unit: "bench.provenance".to_string(),
        fns: functions,
        structs: structures,
        enums: Vec::new(),
        consts: Vec::new(),
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

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("interface") => run_interface(),
        Some("provenance") => run_provenance(),
        Some(other) => {
            eprintln!("unknown library-boundary benchmark group `{other}`");
            std::process::exit(2);
        }
        None => {
            eprintln!("usage: run.sh interface|provenance");
            std::process::exit(2);
        }
    }
}
