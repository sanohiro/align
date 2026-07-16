//! Build script for align_codegen_llvm.
//!
//! By default this is a NO-OP: ordinary `cargo build` / `cargo test` do not
//! compile any C++ and require no LLVM C++ headers beyond what llvm-sys already
//! needs. The ThinLTO S0 spike shim (`cpp/thinlto_shim.cpp`) is compiled ONLY
//! when the `thinlto-spike` feature is enabled, and it links against the SAME
//! libLLVM-22 the workspace already links via llvm-sys (prefer-dynamic).

use std::process::Command;

fn main() {
    // Feature gate: nothing to do for normal builds.
    if std::env::var_os("CARGO_FEATURE_THINLTO_SPIKE").is_none() {
        return;
    }

    println!("cargo:rerun-if-changed=cpp/thinlto_shim.cpp");
    println!("cargo:rerun-if-changed=build.rs");

    let llvm_config =
        std::env::var("LLVM_CONFIG").unwrap_or_else(|_| "llvm-config-22".to_string());

    let cfg = |arg: &str| -> String {
        let out = Command::new(&llvm_config)
            .arg(arg)
            .output()
            .unwrap_or_else(|e| panic!("failed to run {llvm_config} {arg}: {e}"));
        assert!(
            out.status.success(),
            "{llvm_config} {arg} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };

    // Assert the LLVM major version matches the workspace (22) LOUDLY: a shim
    // compiled against different LLVM C++ headers than the linked libLLVM would
    // be silent UB.
    let version = cfg("--version");
    let major = version
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or_else(|| panic!("cannot parse LLVM version {version:?}"));
    assert_eq!(
        major, 22,
        "thinlto-spike requires LLVM 22 (llvm-config reports {version}); \
         set LLVM_CONFIG to an llvm-config-22"
    );

    let includedir = cfg("--includedir");
    let libdir = cfg("--libdir");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("cpp/thinlto_shim.cpp")
        .include(&includedir)
        .flag("-std=c++17")
        .flag_if_supported("-fno-exceptions")
        // LLVM headers use its own RTTI; distro libLLVM-22 is built with
        // RTTI enabled, so we do NOT force -fno-rtti (matches llvm-config
        // --cxxflags, which omits it).
        .define("_GNU_SOURCE", None)
        .define("__STDC_CONSTANT_MACROS", None)
        .define("__STDC_FORMAT_MACROS", None)
        .define("__STDC_LIMIT_MACROS", None)
        .warnings(false);
    build.compile("align_thinlto_shim");

    // Link against the same dynamic libLLVM the workspace already uses, plus
    // the C++ runtime for the shim's own std:: usage.
    println!("cargo:rustc-link-search=native={libdir}");
    println!("cargo:rustc-link-lib=dylib=LLVM-22");
    println!("cargo:rustc-link-lib=dylib=stdc++");
    // The legacy ThinLTOCodeGenerator C API (llvm-sys `lto` bindings, used by
    // the spike's collapse + minimal-mechanism tests) lives in libLTO.so, NOT
    // libLLVM. The 3-entry shim itself needs only libLLVM; this is a spike-test
    // dependency.
    println!("cargo:rustc-link-lib=dylib=LTO");
    // Bake an rpath so the spike test binary finds libLTO/libLLVM at runtime
    // (the LLVM libdir is not necessarily on the default loader path).
    println!("cargo:rustc-link-arg=-Wl,-rpath,{libdir}");
}
