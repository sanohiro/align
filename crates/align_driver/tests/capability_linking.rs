//! M13 Slice 2 — capability-based linking. Two layers of coverage:
//!
//!  1. **Unit (no backend):** the `link_libs` a program's MIR carries. `alignc` links only these
//!     gated libraries (`libz`/`libzstd`/`libcrypto`/`libssl`) plus the always-linked Rust-std
//!     support libs (`pthread`/`dl`/`m`) — so this pins exactly which programs pull which library.
//!
//!  2. **Integration (backend + `cc` + `llvm-readobj`):** the dynamic-dependency list (`DT_NEEDED`
//!     on ELF, `LC_LOAD_DYLIB` on Mach-O) of the produced binary. The completion condition —
//!     `fn main() -> i32 = 0` links NONE of z/zstd/crypto/ssl, and a `hello` that pulls the runtime
//!     core links none of them either (the runtime's dead compress/crypto/tls code is
//!     garbage-collected) — is asserted against the real image, while a program that DOES use a
//!     gated feature keeps its library.
//!
//! Fail-closed: because the driver links only the collected libraries, any external-library builtin
//! that is added but not classified in `align_mir::rvalue_capability` has its library dropped, so its
//! `build_and_run` test (here and in `m11_compress`/`m11_crypto`/`m11_http`) fails to link.

mod common;
use common::*;

/// The gated libraries the program's MIR requests, as `-l<name>` bare names. Everything the driver
/// links unconditionally (`pthread`/`dl`/`m`) is excluded — this is only the capability-gated set.
fn gated_link_libs(name: &str, src: &str) -> Vec<String> {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(!checked.diags.has_errors(), "unexpected errors in {name}");
    let mir = lower_to_mir(&checked.hir);
    let mut libs: Vec<String> = mir
        .link_libs
        .iter()
        .filter(|l| matches!(l.as_str(), "z" | "zstd" | "crypto" | "ssl"))
        .cloned()
        .collect();
    libs.sort();
    libs
}

#[test]
fn pure_program_requests_no_gated_library() {
    // A program that touches no compress/crypto/http feature must not request any gated library —
    // not even after pulling the runtime core (`print`).
    assert_eq!(gated_link_libs("cap-empty", "fn main() -> i32 = 0\n"), Vec::<String>::new());
    assert_eq!(gated_link_libs("cap-hello", "fn main() {\n  print(\"hi\")\n}\n"), Vec::<String>::new());
    // par_map / task_group use threads (pthread, linked unconditionally), never a gated library.
    assert_eq!(
        gated_link_libs("cap-par", "fn main() {\n  print([1,2,3].par_map(fn x { x + 1 }).sum())\n}\n"),
        Vec::<String>::new()
    );
}

#[test]
fn gzip_requests_only_libz() {
    let src = "import std.compress\nfn main() -> Result<(), Error> {\n  c := compress.gzip_compress(\"aaaaaaaa\", 6)?\n  print(c.len() as i64)\n  return Ok(())\n}\n";
    assert_eq!(gated_link_libs("cap-gzip", src), vec!["z".to_string()]);
}

#[test]
fn zstd_requests_only_libzstd() {
    let src = "import std.compress\nfn main() -> Result<(), Error> {\n  c := compress.zstd_compress(\"aaaaaaaa\", 6)?\n  print(c.len() as i64)\n  return Ok(())\n}\n";
    assert_eq!(gated_link_libs("cap-zstd", src), vec!["zstd".to_string()]);
}

#[test]
fn crypto_requests_crypto_and_the_compress_libs() {
    // The single-member runtime co-locates crypto with compress, so `--gc-sections` cannot drop the
    // compress libraries once crypto is used (see `align_mir::Capability`). The mapping is a superset.
    let src = "import std.crypto\nfn main() -> Result<(), Error> {\n  h := crypto.sha256(\"hi\")\n  print(h.len() as i64)\n  return Ok(())\n}\n";
    assert_eq!(gated_link_libs("cap-crypto", src), vec!["crypto".to_string(), "z".to_string(), "zstd".to_string()]);
}

#[test]
fn http_client_requests_the_full_tls_set() {
    // Any HTTP client use may hit the TLS path (the scheme is a runtime decision) → ssl + crypto,
    // transitively the compress libraries.
    let src = "import std.http\nfn main() -> Result<(), Error> {\n  client := http.client()\n  r := client.get(\"https://example.com\")?\n  print(r.status())\n  return Ok(())\n}\n";
    assert_eq!(
        gated_link_libs("cap-https", src),
        vec!["crypto".to_string(), "ssl".to_string(), "z".to_string(), "zstd".to_string()]
    );
}

#[test]
fn ct_equal_and_random_touch_no_library() {
    // `crypto.ct_equal` is a constant-time byte compare and `crypto.random` is OS getrandom — neither
    // links libcrypto. This guards against over-linking the whole `std.crypto` surface.
    let src = "import std.crypto\nfn main() -> Result<(), Error> {\n  b := buffer(16)\n  crypto.random(b)\n  print(b.len() as i64)\n  return Ok(())\n}\n";
    assert_eq!(gated_link_libs("cap-rand", src), Vec::<String>::new());
}

// -------------------------------------------------------------------------------------------------
// Integration: the produced binary's dynamic dependencies. Gated behind the backend, `cc`, and
// `llvm-readobj` (the version-matched LLVM tool the driver's `llvm_tool` locates — works on both
// ELF and Mach-O, unlike GNU `readelf`).

/// True iff any dynamic-dependency entry names one of the four gated libraries.
fn needs_any_gated(libs: &[String]) -> bool {
    libs.iter().any(|l| ["z", "zstd", "crypto", "ssl"].iter().any(|g| is_lib(l, g)))
}

/// Whether the system linker can resolve every `-l<lib>` in `libs` (a trivial-C link probe). The
/// tests that really link a gated library skip where it is not installed (e.g. no libzstd on a
/// stock macOS); on the Linux CI baseline the probe always succeeds, so coverage there is
/// unchanged.
fn can_link(libs: &[&str]) -> bool {
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let tag = libs.join("-");
    let src = dir.join(format!("align-canlink-{pid}-{tag}.c"));
    let exe = dir.join(format!("align-canlink-{pid}-{tag}"));
    std::fs::write(&src, "int main(void) { return 0; }\n").expect("write probe source");
    let mut cmd = std::process::Command::new("cc");
    cmd.arg(&src).arg("-o").arg(&exe);
    for lib in libs {
        cmd.arg(format!("-l{lib}"));
    }
    let ok = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&exe);
    ok
}

#[test]
fn empty_binary_links_no_gated_library() {
    if !backend_available() || !cc_available() {
        return;
    }
    let Some(readobj) = llvm_readobj() else { return };
    // The headline completion condition: `fn main() -> i32 = 0` depends on none of z/zstd/crypto/ssl.
    let exe = build_exe("cap-bin-empty", "fn main() -> i32 = 0\n");
    let libs = needed_libs(&readobj, &exe.exe);
    assert!(!needs_any_gated(&libs), "empty binary must not link any gated library, got {libs:?}");
}

#[test]
fn hello_binary_links_no_gated_library() {
    if !backend_available() || !cc_available() {
        return;
    }
    let Some(readobj) = llvm_readobj() else { return };
    // The substantive case: a program that pulls the runtime core (`print`, which never touches
    // compress/crypto/tls) requests none of the gated libraries in the first place — capability
    // gating (empty `link_libs`) is what drives the empty dependency list here. Dead-code removal
    // (`--gc-sections` / `-dead_strip`) is a separate, independent mechanism: it drops the
    // runtime's dead compress/crypto/tls code (and its external references) so that the link
    // succeeds even though those libraries aren't passed.
    let exe = build_exe("cap-bin-hello", "fn main() {\n  print(\"hello\")\n}\n");
    let libs = needed_libs(&readobj, &exe.exe);
    assert!(!needs_any_gated(&libs), "hello binary must not link any gated library, got {libs:?}");
}

#[test]
fn gzip_binary_links_libz_and_not_the_others() {
    if !backend_available() || !cc_available() || !can_link(&["z"]) {
        return;
    }
    let Some(readobj) = llvm_readobj() else { return };
    let src = "import std.compress\nfn main() -> Result<(), Error> {\n  c := compress.gzip_compress(\"aaaaaaaaaaaaaaaa\", 6)?\n  print(c.len() as i64)\n  return Ok(())\n}\n";
    let exe = build_exe("cap-bin-gzip", src);
    let libs = needed_libs(&readobj, &exe.exe);
    assert!(libs.iter().any(|l| is_lib(l, "z")), "gzip binary must link libz, got {libs:?}");
    assert!(!libs.iter().any(|l| is_lib(l, "zstd")), "gzip binary must not link libzstd, got {libs:?}");
    assert!(
        !libs.iter().any(|l| is_lib(l, "ssl") || is_lib(l, "crypto")),
        "gzip binary must not link ssl/crypto, got {libs:?}"
    );
}

#[test]
fn crypto_binary_links_the_superset_and_not_ssl() {
    if !backend_available() || !cc_available() || !can_link(&["crypto", "z", "zstd"]) {
        return;
    }
    let Some(readobj) = llvm_readobj() else { return };
    // Mirrors `crypto_requests_crypto_and_the_compress_libs` at the binary level: the single-member
    // runtime co-locates crypto with compress, so dead-code removal cannot drop the compress
    // libraries once crypto is used — the dependencies must show libcrypto + libz + libzstd, but
    // not libssl (which only the HTTPS client path pulls in).
    let src = "import std.crypto\nfn main() -> Result<(), Error> {\n  h := crypto.sha256(\"hi\")\n  print(h.len() as i64)\n  return Ok(())\n}\n";
    let exe = build_exe("cap-bin-crypto", src);
    let libs = needed_libs(&readobj, &exe.exe);
    assert!(libs.iter().any(|l| is_lib(l, "crypto")), "crypto binary must link libcrypto, got {libs:?}");
    assert!(libs.iter().any(|l| is_lib(l, "z")), "crypto binary must link libz, got {libs:?}");
    assert!(libs.iter().any(|l| is_lib(l, "zstd")), "crypto binary must link libzstd, got {libs:?}");
    assert!(!libs.iter().any(|l| is_lib(l, "ssl")), "crypto binary must not link libssl, got {libs:?}");
}
