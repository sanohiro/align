// Link the Align-compiled kernel object (`pmap`) + the Align runtime into this harness. `run.sh`
// produces the object via `alignc emit-obj` and points these env vars at it and at the runtime's
// build dir. The kernel calls `align_rt_par_map` / pool workers; from `libalign_runtime.so` (cdylib).
// cdylib — linked dynamically over the C-ABI so its bundled std doesn't collide with this harness's
// std, which a staticlib would).
use std::env;

fn main() {
    let rt_dir = env::var("ALIGN_RUNTIME_DIR").expect("set ALIGN_RUNTIME_DIR — run via ./run.sh");
    println!("cargo:rerun-if-env-changed=ALIGN_KERNEL_BASELINE");
    println!("cargo:rerun-if-env-changed=ALIGN_KERNEL_CANDIDATE");
    match (
        env::var("ALIGN_KERNEL_BASELINE"),
        env::var("ALIGN_KERNEL_CANDIDATE"),
    ) {
        (Ok(baseline), Ok(candidate)) => {
            let standard = env::var("ALIGN_KERNEL_OBJ")
                .expect("source-consumer mode also requires the standard kernel object");
            println!("cargo:rustc-link-arg={baseline}");
            println!("cargo:rustc-link-arg={candidate}");
            println!("cargo:rustc-link-arg={standard}");
            println!("cargo:rerun-if-env-changed=ALIGN_KERNEL_OBJ");
            println!("cargo:rerun-if-changed={baseline}");
            println!("cargo:rerun-if-changed={candidate}");
            println!("cargo:rerun-if-changed={standard}");
            println!("cargo:rerun-if-changed=source_consumer.align");
        }
        (Err(_), Err(_)) => {
            let kobj = env::var("ALIGN_KERNEL_OBJ")
                .expect("set ALIGN_KERNEL_OBJ or both source-consumer objects — run via ./run.sh");
            // The kernel object (static), then the runtime resolved dynamically from the cdylib.
            println!("cargo:rustc-link-arg={kobj}");
            println!("cargo:rerun-if-env-changed=ALIGN_KERNEL_OBJ");
            println!("cargo:rerun-if-changed={kobj}");
            println!("cargo:rerun-if-changed=kernel.align");
        }
        _ => panic!("set both ALIGN_KERNEL_BASELINE and ALIGN_KERNEL_CANDIDATE, or neither"),
    }
    println!("cargo:rustc-link-search=native={rt_dir}");
    println!("cargo:rustc-link-lib=dylib=align_runtime");
    // Find the .so at run time without installing it.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{rt_dir}");
    println!("cargo:rerun-if-env-changed=ALIGN_RUNTIME_DIR");
}
