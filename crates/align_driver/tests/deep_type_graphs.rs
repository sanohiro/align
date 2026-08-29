//! am-d owner: finite nominal type DAGs have no ambient depth limit.

mod common;
use common::*;

fn deep_types(module: Option<&str>, depth: usize) -> String {
    let mut source = String::new();
    if let Some(module) = module {
        source.push_str(&format!("module {module}\n"));
    }
    for index in 0..depth {
        let visibility = if module.is_some() { "pub " } else { "" };
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

struct DeepArtifacts {
    object: std::path::PathBuf,
    executable: std::path::PathBuf,
}

impl Drop for DeepArtifacts {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.object);
        let _ = std::fs::remove_file(&self.executable);
    }
}

fn emit_llvm_raw_optimized_and_run(
    name: &str,
    source: &str,
) -> (String, String, std::process::Output) {
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, name, source);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let raw =
        emit_llvm_ir(&mir, BuildTarget::Baseline, false, &[], false).expect("emit raw llvm ir");
    let optimized = emit_llvm_ir(&mir, BuildTarget::Baseline, true, &[], false)
        .expect("emit optimized llvm ir");

    let directory = std::env::temp_dir();
    let pid = std::process::id();
    let object = directory.join(format!("align-test-{pid}-{name}.o"));
    let executable = directory.join(format!(
        "align-test-{pid}-{name}{}",
        std::env::consts::EXE_SUFFIX
    ));
    let _artifacts = DeepArtifacts {
        object: object.clone(),
        executable: executable.clone(),
    };
    emit_object_file(
        &mir,
        &object,
        BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
    )
    .expect("emit object file");
    link_executable(&object, &executable, &mir.link_libs, Profile::Release)
        .expect("link executable");
    let output = std::process::Command::new(&executable)
        .output()
        .expect("run executable");
    (raw, optimized, output)
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

            let (raw, optimized, output) =
                emit_llvm_raw_optimized_and_run("deep-type-graph", &source);
            assert!(raw.contains("define i32 @main()"));
            assert!(optimized.contains("@main()"));
            assert!(optimized.contains("ret i32 7"));
            assert_eq!(output.status.code(), Some(7));
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
