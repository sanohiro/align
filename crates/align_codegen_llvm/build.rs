//! Build script for align_codegen_llvm.
//!
//! Compiles the ThinLTO C++ shim (`cpp/thinlto_shim.cpp`) UNCONDITIONALLY: the
//! `--thin-lto` driver path needs the three summary-based entry points in every
//! `alignc` binary (llvm-sys 221 cannot emit module summaries nor drive
//! `FunctionImporter` on its own). The shim links against the SAME libLLVM-22 the
//! workspace already links via llvm-sys (prefer-dynamic), so there is a single
//! LLVM in the process. Requires `llvm-config-22` on PATH and the LLVM 22 C++
//! headers (`llvm-22-dev`) — already workspace prerequisites.
//!
//! The shim also carries the production instrument-PGO entry
//! (`align_pgo_run_pipeline`) — no `PGOOptions` C surface exists, so `--pgo-*`
//! needs it in every `alignc` too. It links against libLLVM only.
//!
//! The `thinlto-spike` feature adds ONLY the legacy `ThinLTOCodeGenerator` C API
//! (`libLTO.so`) that the ThinLTO S0 spike tests use for their collapse/
//! minimal-mechanism probes; the production shim entries need only libLLVM.

use std::process::Command;

fn main() {
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
        "align_codegen_llvm requires LLVM 22 (llvm-config reports {version}); \
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
    // The C++ runtime name differs per platform: libc++ on macOS, libstdc++ on
    // Linux (the M1 host builds this workspace too).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
    // Bake an rpath so the binary finds libLLVM-22 at runtime (the LLVM libdir is
    // not necessarily on the default loader path). The prefer-dynamic llvm-sys
    // already needs it; this keeps the shim's own dep resolvable too.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{libdir}");

    // The legacy ThinLTOCodeGenerator C API (llvm-sys `lto` bindings, used ONLY
    // by the S0 spike's collapse + minimal-mechanism tests) lives in libLTO.so,
    // NOT libLLVM. Link it only when that feature is enabled.
    if std::env::var_os("CARGO_FEATURE_THINLTO_SPIKE").is_some() {
        println!("cargo:rustc-link-lib=dylib=LTO");
    }
}
