use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/http_tls_policy.c");
    println!("cargo:rerun-if-env-changed=OPENSSL_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=OPENSSL_DIR");
    let mut tls = cc::Build::new();
    if let Some(include) = std::env::var_os("OPENSSL_INCLUDE_DIR").map(PathBuf::from)
        .or_else(|| std::env::var_os("OPENSSL_DIR").map(|p| PathBuf::from(p).join("include"))) {
        tls.include(include);
    } else if let Ok(output) = Command::new("pkg-config").args(["--variable=includedir", "openssl"]).output()
        && output.status.success() {
        tls.include(std::str::from_utf8(&output.stdout).expect("OpenSSL include directory").trim());
    } else if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let prefix = Command::new("brew").args(["--prefix", "openssl@3"]).output().expect("find OpenSSL 3 headers");
        assert!(prefix.status.success(), "install openssl@3 or set OPENSSL_INCLUDE_DIR");
        tls.include(PathBuf::from(std::str::from_utf8(&prefix.stdout).expect("OpenSSL prefix").trim()).join("include"));
    }
    println!("cargo:rerun-if-changed=src/http_tls_sessions.c");
    tls.file("src/http_tls_policy.c").file("src/http_tls_sessions.c").std("c11").compile("align_http_tls_policy");
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
