//! am-d owner: finite nominal type DAGs have no ambient depth limit.

mod common;
use common::*;

fn deep_types(module: Option<&str>, depth: usize) -> String {
    let mut source = String::new();
    if let Some(module) = module {
        source.push_str(&format!("module {module}\n"));
    }
    for index in 0..depth {
        let visibility = module.is_some().then_some("pub ").unwrap_or("");
        if index + 1 == depth {
            source.push_str(&format!(
                "{visibility}Deep{index} {{ value: string }}\n"
            ));
        } else {
            source.push_str(&format!(
                "{visibility}Deep{index} {{ next: Deep{} }}\n",
                index + 1
            ));
        }
    }
    source
}

#[test]
fn deep_nominal_graph_reaches_raw_optimized_and_executable() {
    if !backend_available() {
        return;
    }
    std::thread::Builder::new()
        .name("deep-type-driver-owner".to_string())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut source = deep_types(None, 4_096);
            source.push_str("fn main() -> i32 = 7\n");

            let raw = emit_llvm(&source);
            assert!(raw.contains("define i32 @main()"));
            let optimized = emit_llvm_optimized(&source, &[]);
            assert!(optimized.contains("@main()"));
            assert!(optimized.contains("ret i32 7"));
            assert_eq!(
                build_and_run("deep-type-graph", &source).status.code(),
                Some(7)
            );
        })
        .expect("spawn deep type driver owner")
        .join()
        .expect("deep type driver owner");
}

#[test]
fn deep_public_graph_reaches_per_unit_interface_and_link() {
    if !backend_available() {
        return;
    }
    std::thread::Builder::new()
        .name("deep-type-per-unit-owner".to_string())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let library = deep_types(Some("deep"), 4_096);
            let main = "import deep\nfn main() -> i32 = 9\n";
            let files = [
                ("deep.align", library.as_str()),
                ("main.align", main),
            ];
            let built = build_per_unit_multi("deep-type-per-unit", &files, "main.align");
            assert_eq!(built.link_and_run().status.code(), Some(9));
        })
        .expect("spawn deep per-unit type owner")
        .join()
        .expect("deep per-unit type owner");
}
