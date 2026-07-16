// Link the two Align-compiled kernel objects (the `after` = post-change build and the `before` =
// main-worktree build, whose exports were sed-renamed with a `_before` suffix) plus the Align
// runtime cdylib into this harness. `run.sh` produces the objects and points these env vars at them
// and at the runtime build dir. The runtime is built with `--features alloc-count`, so the harness
// can resolve `align_rt_alloc_count` / `align_rt_free_count` for the short-size scratch matrix.
use std::env;

fn main() {
    let after = env::var("ALIGN_KERNEL_AFTER").expect("set ALIGN_KERNEL_AFTER — run via ./run.sh");
    let before = env::var("ALIGN_KERNEL_BEFORE").expect("set ALIGN_KERNEL_BEFORE — run via ./run.sh");
    let rt_dir = env::var("ALIGN_RUNTIME_DIR").expect("set ALIGN_RUNTIME_DIR — run via ./run.sh");

    // Both kernel objects (static), then the runtime resolved dynamically from the cdylib.
    println!("cargo:rustc-link-arg={after}");
    println!("cargo:rustc-link-arg={before}");
    println!("cargo:rustc-link-search=native={rt_dir}");
    println!("cargo:rustc-link-lib=dylib=align_runtime");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{rt_dir}");

    println!("cargo:rerun-if-env-changed=ALIGN_KERNEL_AFTER");
    println!("cargo:rerun-if-env-changed=ALIGN_KERNEL_BEFORE");
    println!("cargo:rerun-if-env-changed=ALIGN_RUNTIME_DIR");
    println!("cargo:rerun-if-changed={after}");
    println!("cargo:rerun-if-changed={before}");
    println!("cargo:rerun-if-changed=kernel.align");
}
