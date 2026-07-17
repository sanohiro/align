// Link the Align-compiled donation-ON and donation-OFF kernel objects plus the Align runtime cdylib
// (built with `--features alloc-count`, so `align_rt_alloc_count` / `align_rt_free_count` resolve)
// into this harness. `run.sh` produces both objects (the OFF one has its entry symbols renamed
// `*_off`) and points these env vars at them and at the runtime build dir.
use std::env;

fn main() {
    let kernel_on = env::var("ALIGN_KERNEL_ON").expect("set ALIGN_KERNEL_ON — run via ./run.sh");
    let kernel_off = env::var("ALIGN_KERNEL_OFF").expect("set ALIGN_KERNEL_OFF — run via ./run.sh");
    let rt_dir = env::var("ALIGN_RUNTIME_DIR").expect("set ALIGN_RUNTIME_DIR — run via ./run.sh");

    println!("cargo:rustc-link-arg={kernel_on}");
    println!("cargo:rustc-link-arg={kernel_off}");
    println!("cargo:rustc-link-search=native={rt_dir}");
    println!("cargo:rustc-link-lib=dylib=align_runtime");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{rt_dir}");

    println!("cargo:rerun-if-env-changed=ALIGN_KERNEL_ON");
    println!("cargo:rerun-if-env-changed=ALIGN_KERNEL_OFF");
    println!("cargo:rerun-if-env-changed=ALIGN_RUNTIME_DIR");
    println!("cargo:rerun-if-changed={kernel_on}");
    println!("cargo:rerun-if-changed={kernel_off}");
    println!("cargo:rerun-if-changed=kernel.align");
}
