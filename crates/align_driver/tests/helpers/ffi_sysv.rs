//! Runs inside the existing C-layout owner's bounded group and private artifact stage.
use std::{fs, path::Path};
pub(super) fn run(stage: &Path) {
    let (c, source, calls) = super::ffi_sysv_cases::sources();
    super::compile_c(stage, &c);
    fs::write(
        stage.join("factory.align"),
        format!("module factory\n{source}"),
    )
    .unwrap();
    for per_unit in [false, true] {
        super::build_and_run(
            stage,
            "import factory\nfn main() -> i32 = factory.exercise()\n",
            per_unit,
            "sysv-values",
        );
    }
    println!("SysV native record calls: {calls}, whole/per-unit");
    super::cache_alignment(stage);
}
