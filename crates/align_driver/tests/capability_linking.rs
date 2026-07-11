//! M13 Slice 2 — capability-based linking. Two layers of coverage:
//!
//!  1. **Unit (no backend):** the `link_libs` a program's MIR carries. `alignc` links only these
//!     gated libraries (`libz`/`libzstd`/`libcrypto`/`libssl`) plus the always-linked Rust-std
//!     support libs (`pthread`/`dl`/`m`) — so this pins exactly which programs pull which library.
//!
//!  2. **Integration (backend + `cc` + `readelf`):** the `DT_NEEDED` list of the produced binary.
//!     The completion condition — `fn main() -> i32 = 0` links NONE of z/zstd/crypto/ssl, and a
//!     `hello` that pulls the runtime core links none of them either (the runtime's dead
//!     compress/crypto/tls code is garbage-collected) — is asserted against the real image, while a
//!     program that DOES use a gated feature keeps its library.
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
// Integration: the produced binary's DT_NEEDED. Gated behind the backend, `cc`, and `readelf`.

/// Whether `readelf` is available for the DT_NEEDED assertions.
fn readelf_available() -> bool {
    std::process::Command::new("readelf")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The `DT_NEEDED` shared-library base names of the ELF at `path` (via `readelf -d`), lowercased.
/// Returns an empty vec if `readelf` fails — callers gate on [`readelf_available`] first.
fn dt_needed(path: &std::path::Path) -> Vec<String> {
    let out = std::process::Command::new("readelf").arg("-d").arg(path).output().expect("run readelf");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut libs = Vec::new();
    for line in text.lines() {
        if !line.contains("(NEEDED)") {
            continue;
        }
        // The library name is inside the last pair of square brackets: `Shared library: [libc.so.6]`.
        if let Some(start) = line.rfind('[')
            && let Some(end) = line[start..].find(']')
        {
            libs.push(line[start + 1..start + end].to_string());
        }
    }
    libs
}

/// True iff any `DT_NEEDED` entry names one of the four gated libraries.
fn needs_any_gated(libs: &[String]) -> bool {
    libs.iter().any(|l| {
        l.starts_with("libz.") || l.starts_with("libzstd.") || l.starts_with("libcrypto.") || l.starts_with("libssl.")
    })
}

#[test]
fn empty_binary_links_no_gated_library() {
    if !backend_available() || !cc_available() || !readelf_available() {
        return;
    }
    // The headline completion condition: `fn main() -> i32 = 0` depends on none of z/zstd/crypto/ssl.
    let exe = build_exe("cap-elf-empty", "fn main() -> i32 = 0\n");
    let libs = dt_needed(&exe.exe);
    assert!(!needs_any_gated(&libs), "empty binary must not link any gated library, got {libs:?}");
}

#[test]
fn hello_binary_links_no_gated_library() {
    if !backend_available() || !cc_available() || !readelf_available() {
        return;
    }
    // The substantive case: a program that pulls the runtime core (`print` → the single runtime
    // member that also references z/zstd/crypto/ssl) still links none of them — `--gc-sections`
    // removes the dead compress/crypto/tls code and its external references.
    let exe = build_exe("cap-elf-hello", "fn main() {\n  print(\"hello\")\n}\n");
    let libs = dt_needed(&exe.exe);
    assert!(!needs_any_gated(&libs), "hello binary must not link any gated library, got {libs:?}");
}

#[test]
fn gzip_binary_links_libz_and_not_the_others() {
    if !backend_available() || !cc_available() || !readelf_available() {
        return;
    }
    let src = "import std.compress\nfn main() -> Result<(), Error> {\n  c := compress.gzip_compress(\"aaaaaaaaaaaaaaaa\", 6)?\n  print(c.len() as i64)\n  return Ok(())\n}\n";
    let exe = build_exe("cap-elf-gzip", src);
    let libs = dt_needed(&exe.exe);
    assert!(libs.iter().any(|l| l.starts_with("libz.")), "gzip binary must link libz, got {libs:?}");
    assert!(
        !libs.iter().any(|l| l.starts_with("libssl.") || l.starts_with("libcrypto.")),
        "gzip binary must not link ssl/crypto, got {libs:?}"
    );
}
