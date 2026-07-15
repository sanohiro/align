//! Compile the fast-path string primitives to a standalone LLVM-bitcode artifact for `--rt-lto`
//! (M14 Slice 2, `docs/impl/07-roadmap.md`).
//!
//! `crates/align_runtime/src/str_prims.rs` is ONE source of truth compiled twice: (a) into
//! `libalign_runtime.a` via the normal `cargo build` of `align_runtime`, and (b) here, standalone,
//! to `$OUT_DIR/str_prims.bc`. `main.rs` bakes the `.bc` into the `alignc` binary with
//! `include_bytes!`, so there is no on-disk artifact to keep fresh and no LLVM-major skew: the same
//! `cargo build` regenerates it, and the same `rustc` (matching inkwell's LLVM major) builds both
//! sides. The driver parses it from an in-memory buffer and links it into the program module.
//!
//! The set is dependency-free (slice ops only — no `align_hash`/`memchr`), so it compiles as a
//! self-contained crate root. `-O -Ccodegen-units=1` gives the inliner a clean single-body module;
//! `-Cpanic=abort` drops the `rust_eh_personality` reference these never-unwinding functions would
//! otherwise carry, leaving `bcmp` as the only undefined symbol (the symbol-set gate).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The shared seed for the runtime-source content digest (M15 S3b). The digest is baked here and
/// recomputed at link time by `align_driver::runtime_src_digest`; the two MUST stay algorithm- and
/// seed-identical (a `#[test]` asserts the recomputed digest equals the baked value). Keep this
/// constant in sync with `RUNTIME_SRC_DIGEST_SEED` in `src/lib.rs`.
const RUNTIME_SRC_DIGEST_SEED: u64 = 0x616C_6967_6E5F_7274; // "align_rt"

/// A deterministic, mtime-independent content digest of every `*.rs` file under `dir` (recursive):
/// relative paths sorted, then for each `(rel_path, len, bytes)` folded into one buffer and hashed.
/// `None` if the tree is absent/unreadable. Mirrors `align_driver::runtime_src_digest`.
fn runtime_src_digest(dir: &Path) -> Option<String> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).ok()?;
        for entry in entries.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let path = entry.path();
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() && path.extension().is_some_and(|x| x == "rs") {
                let rel = path.strip_prefix(dir).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                files.push((rel, path));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut buf: Vec<u8> = Vec::new();
    for (rel, path) in &files {
        let bytes = std::fs::read(path).ok()?;
        buf.extend_from_slice(rel.as_bytes());
        buf.push(0);
        buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        buf.extend_from_slice(&bytes);
    }
    Some(format!("{:016x}", align_hash::wyhash(&buf, RUNTIME_SRC_DIGEST_SEED)))
}

fn main() {
    // M15 S3b: bake a content digest of the whole runtime source tree so `align_driver` can detect a
    // stale `libalign_runtime.a` by CONTENT (not mtime) at link time — killing the false-stale
    // papercut where a touch/checkout bumps source mtimes without changing content.
    let runtime_src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../align_runtime/src");
    let digest = runtime_src_digest(&runtime_src).unwrap_or_default();
    println!("cargo:rustc-env=ALIGN_RUNTIME_SRC_DIGEST={digest}");
    // Re-run (re-bake the digest) whenever any runtime source file changes.
    println!("cargo:rerun-if-changed={}", runtime_src.display());

    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../align_runtime/src/str_prims.rs");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR set by cargo")).join("str_prims.bc");
    // rustc that cargo is driving this build with — the same LLVM major as inkwell, by construction.
    let rustc = std::env::var("RUSTC").expect("RUSTC set by cargo");
    // The actual compilation target (may differ from the host when cross-compiling `alignc`) — the
    // baked bitcode must match the triple/datalayout of the final binary, not the machine building it.
    let target = std::env::var("TARGET").expect("TARGET set by cargo");

    // Rebuild the bitcode whenever the shared source changes (never drifts from the staticlib copy).
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed=build.rs");

    let status = Command::new(&rustc)
        .args([
            "--emit=llvm-bc",
            "--crate-type",
            "lib",
            "--edition",
            "2024",
            "-O",
            "-Ccodegen-units=1",
            "-Cpanic=abort",
            "--target",
        ])
        .arg(&target)
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .status()
        .unwrap_or_else(|e| panic!("failed to invoke rustc ({rustc}) for str_prims bitcode: {e}"));
    assert!(
        status.success(),
        "rustc failed to emit str_prims bitcode (status {status}); \
         the --rt-lto artifact cannot be baked"
    );
    assert!(
        out.exists(),
        "rustc reported success but {} was not written",
        out.display()
    );
}
