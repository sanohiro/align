use std::hint::black_box;
use std::time::{Duration, Instant};

use align_interface::{
    Effect, Hash128, IFnSig, IParam, IType, InterfaceSummary, ParamMode, ReturnBorrowSummary,
    ReturnRegionSummary, deserialize, encode_interface_surface, serialize,
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

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("interface") => run_interface(),
        Some(other) => {
            eprintln!("unknown library-boundary benchmark group `{other}`");
            std::process::exit(2);
        }
        None => {
            eprintln!("usage: run.sh interface");
            std::process::exit(2);
        }
    }
}
