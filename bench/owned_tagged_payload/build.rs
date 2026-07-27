use std::env;

fn main() {
    let kernel = env::var("ALIGN_KERNEL").expect("set ALIGN_KERNEL — run via ./run.sh");
    let rt_dir = env::var("ALIGN_RUNTIME_DIR").expect("set ALIGN_RUNTIME_DIR — run via ./run.sh");
    println!("cargo:rustc-link-arg={kernel}");
    println!("cargo:rustc-link-search=native={rt_dir}");
    println!("cargo:rustc-link-lib=dylib=align_runtime");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{rt_dir}");
    println!("cargo:rerun-if-env-changed=ALIGN_KERNEL");
    println!("cargo:rerun-if-env-changed=ALIGN_RUNTIME_DIR");
    println!("cargo:rerun-if-changed={kernel}");
    println!("cargo:rerun-if-changed=kernel.align");
}
