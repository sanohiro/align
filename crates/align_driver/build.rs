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

use std::path::PathBuf;
use std::process::Command;

fn main() {
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
