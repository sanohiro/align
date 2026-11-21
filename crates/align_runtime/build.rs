use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/crypto_blake3.c");
    println!("cargo:rerun-if-env-changed=BLAKE3_INCLUDE_DIR");
    for name in ["PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR", "PKG_CONFIG_SYSROOT_DIR"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let include = std::env::var_os("BLAKE3_INCLUDE_DIR").map(PathBuf::from).unwrap_or_else(|| {
        let output = Command::new("pkg-config")
            .args(["--variable=includedir", "libblake3"])
            .output()
            .expect("install pkg-config and libblake3, or set BLAKE3_INCLUDE_DIR");
        assert!(output.status.success(), "install libblake3 or set BLAKE3_INCLUDE_DIR (see README.md)");
        PathBuf::from(std::str::from_utf8(&output.stdout).expect("libblake3 include directory").trim())
    });
    println!("cargo:rerun-if-changed={}", include.join("blake3.h").display());
    cc::Build::new().file("src/crypto_blake3.c").include(include).std("c11").compile("align_crypto_blake3");
}
