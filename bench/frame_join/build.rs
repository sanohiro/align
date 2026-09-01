use std::env;

fn main() {
    let runtime = env::var("ALIGN_RUNTIME_DIR").expect("run through bench/frame_join/run.sh");
    println!("cargo:rustc-link-search=native={runtime}");
    println!("cargo:rustc-link-lib=dylib=align_runtime");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{runtime}");
    println!("cargo:rerun-if-env-changed=ALIGN_RUNTIME_DIR");
}
