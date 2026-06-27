// Link the Align-compiled kernel object (`agg`) + the Align runtime into this harness. `run.sh`
// produces the object via `alignc emit-obj` and points these env vars at it and at the runtime's
// build dir. The kernel calls `align_rt_json_decode_struct_array` / arena / alloc; those come from
// `libalign_runtime.so` (a cdylib — linked dynamically over the C-ABI so its bundled std doesn't
// collide with this harness's std, which a staticlib would).
use std::env;

fn main() {
    let kobj = env::var("ALIGN_KERNEL_OBJ").expect("set ALIGN_KERNEL_OBJ — run via ./run.sh");
    let rt_dir = env::var("ALIGN_RUNTIME_DIR").expect("set ALIGN_RUNTIME_DIR — run via ./run.sh");
    // The kernel object (static), then the runtime resolved dynamically from the cdylib.
    println!("cargo:rustc-link-arg={kobj}");
    println!("cargo:rustc-link-search=native={rt_dir}");
    println!("cargo:rustc-link-lib=dylib=align_runtime");
    // Find the .so at run time without installing it.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{rt_dir}");
    println!("cargo:rerun-if-env-changed=ALIGN_KERNEL_OBJ");
    println!("cargo:rerun-if-env-changed=ALIGN_RUNTIME_DIR");
    println!("cargo:rerun-if-changed=kernel.align");
}
