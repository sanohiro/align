// Link the Align-compiled kernel object (`eq_count` / `sum_sq_pos`) + the Align runtime into this
// harness. `run.sh` produces the object via `alignc emit-obj` (once WITHOUT and once WITH `--rt-lto`)
// and points these env vars at it and at the runtime's build dir. The kernel calls `align_rt_str_*`
// (resolved from the cdylib for the non-guarded `str_cmp` path; the guarded four are inlined into the
// object under `--rt-lto`). The runtime comes from `libalign_runtime.so` (dynamic, over the C-ABI, so
// its bundled std does not collide with this harness's std as a staticlib would).
use std::env;

fn main() {
    let kobj = env::var("ALIGN_KERNEL_OBJ").expect("set ALIGN_KERNEL_OBJ — run via ./run.sh");
    let rt_dir = env::var("ALIGN_RUNTIME_DIR").expect("set ALIGN_RUNTIME_DIR — run via ./run.sh");
    println!("cargo:rustc-link-arg={kobj}");
    println!("cargo:rustc-link-search=native={rt_dir}");
    println!("cargo:rustc-link-lib=dylib=align_runtime");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{rt_dir}");
    println!("cargo:rerun-if-env-changed=ALIGN_KERNEL_OBJ");
    println!("cargo:rerun-if-env-changed=ALIGN_RUNTIME_DIR");
    // The kernel object is a raw link-arg, so cargo doesn't track it — relink when its content
    // changes even if the path (env var) stays the same across the OFF/ON passes.
    println!("cargo:rerun-if-changed={kobj}");
    println!("cargo:rerun-if-changed=kernel.align");
}
