//! Mach-O linking regression net (the Codex-audit portability fix: ELF-only linker flags and
//! ELF-only size tooling used to break every macOS build). Every test early-returns unless the
//! build target is Mach-O, so this file is inert on the Linux CI baseline — the ELF behavior is
//! pinned by `capability_linking.rs` / `build_profiles.rs`, which run on both formats.

mod common;
use common::*;

/// Whether this build target is Mach-O — the gate for every test in this file.
fn macho() -> bool {
    align_driver::target_object_format() == Ok(ObjectFormat::MachO)
}

#[test]
fn hello_links_libsystem_and_no_gated_library() {
    if !macho() || !backend_available() || !cc_available() {
        return;
    }
    let Some(readobj) = llvm_readobj() else { return };
    // The ld64 flag set (`-dead_strip`/`-dead_strip_dylibs`, no `-lpthread/-ldl/-lm`) must produce
    // a runnable image whose only dylib is libSystem: the runtime's dead compress/crypto/tls code
    // is dead-stripped and no gated library is requested, exactly like the ELF case.
    let exe = build_exe("macho-hello", "fn main() {\n  print(\"hello\")\n}\n");
    let out = std::process::Command::new(&exe.exe).output().expect("run hello");
    assert_eq!(out.status.code(), Some(0), "hello runs");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
    let libs = needed_libs(&readobj, &exe.exe);
    assert!(
        libs.iter().any(|l| is_lib(l, "System")),
        "libSystem must be an LC_LOAD_DYLIB, got {libs:?}"
    );
    for gated in ["z", "zstd", "crypto", "ssl"] {
        assert!(
            !libs.iter().any(|l| is_lib(l, gated)),
            "hello must not link lib{gated}, got {libs:?}"
        );
    }
}

#[test]
fn tiny_profile_strips_and_stays_runnable() {
    if !macho() || !backend_available() || !cc_available() {
        return;
    }
    let Some(nm) = align_driver::llvm_tool("llvm-nm") else { return };
    // Mach-O strip is the post-link external `strip` (ld64 has no `--strip-all`), and Apple's
    // `strip` must re-sign the binary ad hoc — so the stripped image must BOTH run (the signature
    // stays valid) AND carry no symbols beyond the Mach-O header symbol.
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "macho-tiny", "fn main() {\n  print(\"tiny\")\n}\n");
    assert!(!checked.diags.has_errors(), "unexpected errors");
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let obj = dir.join(format!("align-macho-tiny-{pid}.o"));
    let exe = dir.join(format!("align-macho-tiny-{pid}"));
    emit_object_file(&mir, &obj, BuildTarget::Baseline, Profile::Tiny, &[]).expect("codegen");
    link_executable(&obj, &exe, &mir.link_libs, Profile::Tiny).expect("link + strip");
    let out = std::process::Command::new(&exe).output().expect("run stripped binary");
    let _ = std::fs::remove_file(&obj);
    let nm_out = std::process::Command::new(&nm)
        .arg("--defined-only")
        .arg(&exe)
        .output()
        .expect("run llvm-nm");
    let _ = std::fs::remove_file(&exe);
    assert_eq!(out.status.code(), Some(0), "the stripped binary must still run (valid signature)");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "tiny\n");
    let nm_text = String::from_utf8_lossy(&nm_out.stdout).into_owned();
    let leftover = nm_text.lines().filter(|l| !l.trim().is_empty() && !l.contains("__mh_")).count();
    assert_eq!(
        leftover, 0,
        "tiny must strip every symbol (only the Mach-O header symbol may remain):\n{nm_text}"
    );
}
