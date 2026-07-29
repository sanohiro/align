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

fn import_validation_fixture() -> InterfaceSummary {
    let parameter = ITypeParam {
        name: "T".to_string(),
        bound: None,
    };
    let mut structures = Vec::with_capacity(128);
    for index in 0..128 {
        let field_type = if index == 127 {
            named("T")
        } else {
            IType::Named {
                path: format!("Layer_{:04}", index + 1),
                args: vec![named("T")],
            }
        };
        structures.push(IStructDef {
            name: format!("Layer_{index:04}"),
            type_params: vec![parameter.clone()],
            fields: vec![("value".to_string(), field_type)],
            align: None,
            c_repr: false,
            generic_body: Some(format!(
                "pub Layer_{index:04}<T> {{ value: T }}"
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
