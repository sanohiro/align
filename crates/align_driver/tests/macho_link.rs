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
    emit_object_file(&mir, &obj, BuildTarget::Baseline, Profile::Tiny, &[], false).expect("codegen");
    link_executable(&align_driver::CDriver::default(), &obj, &exe, &mir.link_libs, Profile::Tiny).expect("link + strip");
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

// ---- Deployment target: the objects and the image are stamped from ONE resolved value -----------

/// The `minos` field of a Mach-O file's `LC_BUILD_VERSION`, as `"<major>.<minor>"`.
///
/// Parsed from the bytes rather than read out of `otool`, so the check is an exact byte-level proof
/// with no external tool and no output-format drift. 64-bit little-endian Mach-O only, which is
/// every target this file runs on (`LC_BUILD_VERSION` = `0x32`; `minos` packs `X<<16 | Y<<8 | Z`).
fn build_version_minos(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let u32_at = |at: usize| -> Option<u32> {
        let slice: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        Some(u32::from_le_bytes(slice))
    };
    // 64-bit Mach-O magic (`MH_MAGIC_64`), then `ncmds` and the 32-byte header.
    if u32_at(0)? != 0xfeed_facf {
        return None;
    }
    let ncmds = u32_at(16)?;
    let mut at = 32usize;
    for _ in 0..ncmds {
        let cmd = u32_at(at)?;
        let cmdsize = u32_at(at + 4)? as usize;
        if cmdsize < 8 {
            return None;
        }
        if cmd == 0x32 {
            let packed = u32_at(at + 12)?;
            return Some(format!("{}.{}", packed >> 16, (packed >> 8) & 0xff));
        }
        at = at.checked_add(cmdsize)?;
    }
    None
}

/// The deployment target is a single-sourced part of the target identity: the triple carries it, the
/// object is stamped with it, and the linked image agrees — because the link states the same value
/// instead of inheriting whatever the `cc` driver guesses (issue 1087).
///
/// Before this contract the triple was `arm64-apple-darwin<kernel>`, which LLVM ran through its
/// Darwin → macOS renumbering table, so objects claimed a macOS one major too high while the image
/// (stamped from the `cc` driver's own injected `-platform_version`) claimed the right one.
#[test]
fn objects_and_the_linked_image_carry_the_resolved_deployment_target() {
    if !macho() || !backend_available() || !cc_available() {
        return;
    }
    let triple = align_codegen_llvm::default_triple().expect("a resolved host triple");
    assert!(
        !triple.contains("darwin"),
        "no triple the compiler constructs may spell the kernel version: {triple}"
    );
    let version = triple
        .rsplit_once("macosx")
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| panic!("a macOS triple must name its deployment target: {triple}"));

    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "macho-deploy", "fn main() {\n  print(\"deploy\")\n}\n");
    assert!(!checked.diags.has_errors(), "unexpected errors");
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let obj = dir.join(format!("align-macho-deploy-{pid}.o"));
    let exe = dir.join(format!("align-macho-deploy-{pid}"));
    emit_object_file(&mir, &obj, BuildTarget::Baseline, Profile::Release, &[], false).expect("codegen");
    link_executable(&align_driver::CDriver::default(), &obj, &exe, &mir.link_libs, Profile::Release)
        .expect("link");

    let object_minos = build_version_minos(&obj);
    let image_minos = build_version_minos(&exe);
    let ran = std::process::Command::new(&exe).output().expect("run");
    let _ = std::fs::remove_file(&obj);
    let _ = std::fs::remove_file(&exe);

    assert_eq!(
        object_minos.as_deref(),
        Some(version.as_str()),
        "the object must be stamped with the resolved deployment target, not a renumbered kernel major"
    );
    assert_eq!(
        image_minos.as_deref(),
        Some(version.as_str()),
        "the image must agree with the objects it was linked from"
    );
    assert_eq!(ran.status.code(), Some(0), "the stamped binary must still run");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "deploy\n");
}
