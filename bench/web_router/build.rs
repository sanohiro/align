// Link the Align-compiled bench objects + the Align runtime into this harness. `run.sh` compiles the
// pkg.web sources (copied out of `apps/web/`) plus the bench kernel with `alignc emit-obj`, which
// writes ONE object per module, and points these env vars at them and at the runtime's build dir.
// The kernel calls `align_rt_*` (string ops, the array builder); those come from
// `libalign_runtime.so` — a cdylib linked over the C-ABI so its bundled std does not collide with
// this harness's std, which a staticlib would.
use std::env;

fn main() {
    let objs = env::var("ALIGN_KERNEL_OBJS").expect("set ALIGN_KERNEL_OBJS — run via ./run.sh");
    let rt_dir = env::var("ALIGN_RUNTIME_DIR").expect("set ALIGN_RUNTIME_DIR — run via ./run.sh");
    for obj in objs.split(':').filter(|s| !s.is_empty()) {
        println!("cargo:rustc-link-arg={obj}");
        // Raw link-args are not tracked by cargo — relink when an object's content changes even
        // though its path (the env var) stayed the same.
        println!("cargo:rerun-if-changed={obj}");
    }
    println!("cargo:rustc-link-search=native={rt_dir}");
    println!("cargo:rustc-link-lib=dylib=align_runtime");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{rt_dir}");
    println!("cargo:rerun-if-env-changed=ALIGN_KERNEL_OBJS");
    println!("cargo:rerun-if-env-changed=ALIGN_RUNTIME_DIR");
}
