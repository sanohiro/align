//! Explicit export roots — `emit-obj`/`emit-llvm --export <name>` (M13 Codex-audit item 1,
//! `docs/impl/07-roadmap.md`). M13 Slice 1 (`link_hygiene.rs`) made every Align program function
//! `internal`, which broke the bench harnesses' link (they call `pub fn` kernels compiled with
//! `emit-obj`, no `main`). `--export <name>` restores a linkable C-ABI surface one name at a time:
//! the named function (matched by source-level `Function::name`, independent of `pub` visibility)
//! keeps `external` linkage instead of the default `internal`; everything else is unaffected.
//!
//! See `link_hygiene.rs` for the full default linkage-map table this mechanism is an exception to.

mod common;
use common::*;

/// The text LLVM prints between `define ` and the `@` of the definition named `sym` (the linkage
/// words). Duplicated from `link_hygiene.rs` — each integration-test file is its own crate, so
/// private helpers cannot be shared without a `common`-module addition this narrow check does not
/// warrant. Panics if there is no such definition.
fn define_prefix<'a>(ir: &'a str, sym: &str) -> &'a str {
    let bare = format!("@{sym}(");
    let quoted = format!("@\"{sym}\"(");
    for line in ir.lines() {
        let l = line.trim_start();
        if !l.starts_with("define ") {
            continue;
        }
        if l.contains(&bare) || l.contains(&quoted) {
            let after = &l["define ".len()..];
            let at = after.find('@').expect("a define line always names a symbol");
            return &after[..at];
        }
    }
    panic!("no `define` for @{sym} found in IR:\n{ir}");
}

fn encoded(sym: &str) -> String {
    let hex = sym
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("align_fn${}${hex}", sym.len())
}

fn assert_internal(ir: &str, sym: &str) {
    let pfx = define_prefix(ir, sym);
    assert!(pfx.contains("internal"), "@{sym} should have `internal` linkage, got `define {pfx}@{sym}(...`");
}

/// External linkage prints as *no* linkage word — LLVM omits it.
fn assert_external(ir: &str, sym: &str) {
    let pfx = define_prefix(ir, sym);
    assert!(
        !pfx.contains("internal") && !pfx.contains("private"),
        "@{sym} must be external (an export root), got `define {pfx}@{sym}(...`"
    );
}

/// A no-`main` library: `k1` calls `helper` and is exported; `k2` and `helper` are not.
const LIB: &str = concat!(
    "pub fn k1(x: i64) -> i64 = helper(x) + 1\n",
    "pub fn k2(x: i64) -> i64 = x * 2\n",
    "fn helper(x: i64) -> i64 = x + 10\n",
);

#[test]
fn exported_fn_is_external() {
    if !backend_available() {
        return;
    }
    let ir = emit_llvm_with_exports(LIB, &["k1"]);

    // The named export root is external; everything else keeps the whole-program default
    // (`internal`) — `--export` is additive, not a switch that turns off internalization.
    assert_external(&ir, "k1");
    assert_internal(&ir, &encoded("k2"));
    assert_internal(&ir, &encoded("helper"));

    // `helper` must still be defined and actually called from `k1` — the export-roots change must
    // not perturb which functions are lowered or how they call each other, only their linkage.
    assert!(
        ir.contains(&format!("call i64 @\"{}\"(", encoded("helper"))),
        "k1 must still call helper:\n{ir}"
    );
}

#[test]
fn unexported_still_internal() {
    if !backend_available() {
        return;
    }
    // Same library, no `--export` at all: every program function stays internal (pins the M13
    // Slice 1 default — `pub` alone never exports; `--export` is the only opt-in).
    let ir = emit_llvm_with_exports(LIB, &[]);
    assert_internal(&ir, &encoded("k1"));
    assert_internal(&ir, &encoded("k2"));
    assert_internal(&ir, &encoded("helper"));
}

/// A `Result`-returning `main`: lowers to TWO LLVM definitions — the body under its encoded Align
/// symbol, and a separately generated C `main` wrapper (always external so `crt0` finds it).
const RESULT_MAIN: &str = concat!(
    "fn helper(x: i64) -> i64 = x + 1\n",
    "fn main() -> Result<(), Error> {\n",
    "  print(helper(41))\n",
    "  return Ok(())\n",
    "}\n",
);

#[test]
fn export_main_is_a_harmless_noop_for_result_main() {
    if !backend_available() {
        return;
    }
    // `--export main` names the SOURCE function `main` — but for a `Result`-returning `main`,
    // `Function::name == "main"` while the LLVM body uses the encoded program identity. The C
    // wrapper is already external unconditionally, so `--export main` must remain a genuine no-op:
    // the encoded body stays internal either way.
    let ir = emit_llvm_with_exports(RESULT_MAIN, &["main"]);
    assert_external(&ir, "main");
    assert_internal(&ir, &encoded("main"));
    assert_internal(&ir, &encoded("helper"));
}

#[test]
fn unknown_export_rejected() {
    // The fail-closed seam `align_driver::unknown_exports` (the driver's `--export` validation):
    // a name that matches no `Function::name` in the lowered MIR must come back, so the CLI can
    // reject it with a listed diagnostic instead of silently compiling a wrong object.
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "ir", LIB);
    assert!(!checked.diags.has_errors(), "unexpected errors:\n{}", align_driver::format_diagnostics(&sm, &checked.diags));
    let mir = lower_to_mir(&checked.hir);

    let one = ["nope".to_string()];
    assert_eq!(align_driver::unknown_exports(&mir, &one), vec!["nope"]);

    // A mix of a real name and an unknown one: only the unknown one is reported.
    let mixed = ["k1".to_string(), "nope".to_string()];
    assert_eq!(align_driver::unknown_exports(&mir, &mixed), vec!["nope"]);

    // Every name known: nothing reported.
    let all_known = ["k1".to_string(), "k2".to_string(), "helper".to_string()];
    assert!(align_driver::unknown_exports(&mir, &all_known).is_empty());
}

#[test]
fn c_harness_calls_conservative_borrow_mut_exports() {
    if !backend_available() || !cc_available() {
        return;
    }
    let source = "\
pub fn inspect(prefix: i64, borrow mut value: string, suffix: i64) -> i64 = prefix + value.len() + suffix
pub fn replace(tag: i64, borrow mut value: string) { if tag == 7 { value = \"abc\".clone() } }
";
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "drop-state-export", source);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let directory = (0_u32..)
        .find_map(|attempt| {
            let candidate = std::env::temp_dir().join(format!(
                "align-drop-state-export-{}-{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&candidate) {
                Ok(()) => Some(candidate),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => panic!("create private C-harness directory: {error}"),
            }
        })
        .expect("exhausted C-harness directory names");
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let align_object = directory.join("align.o");
    let c_source = directory.join("harness.c");
    let c_object = directory.join("harness.o");
    let executable = directory.join(format!("harness{}", std::env::consts::EXE_SUFFIX));
    emit_object_file(
        &mir,
        &align_object,
        BuildTarget::Baseline,
        Profile::Release,
        &["inspect".to_owned(), "replace".to_owned()],
        false,
    )
    .expect("emit exported Align object");
    std::fs::write(
        &c_source,
        r#"#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
typedef struct { const unsigned char *ptr; int64_t len; } AlignString;
typedef struct { AlignString *value; bool *cleanup; } AlignBorrowMutString;
extern int64_t inspect(int64_t prefix, AlignBorrowMutString value, int64_t suffix);
extern void replace(int64_t tag, AlignBorrowMutString value);
extern void align_rt_free(void *ptr);
int main(void) {
    AlignString value = { NULL, 0 };
    bool cleanup = false;
    AlignBorrowMutString borrow = { &value, &cleanup };
    if (inspect(10, borrow, 32) != 42 || cleanup) return 2;
    replace(7, borrow);
    if (!cleanup || value.ptr == NULL || value.len != 3) return 3;
    if (inspect(10, borrow, 29) != 42 || !cleanup) return 4;
    align_rt_free((void *)value.ptr);
    return 42;
}
"#,
    )
    .expect("write C harness");
    let compiled = std::process::Command::new("cc")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-c", "-O0"])
        .arg(&c_source)
        .arg("-o")
        .arg(&c_object)
        .output()
        .expect("launch C compiler");
    assert!(
        compiled.status.success(),
        "C harness compilation failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    link_objects(
        &align_driver::CDriver::default(),
        &[align_object.as_path(), c_object.as_path()],
        &executable,
        &mir.link_libs,
        Profile::Release,
    )
    .expect("link C harness and Align exports");
    let output = std::process::Command::new(&executable).output().expect("run C harness");
    assert_eq!(output.status.code(), Some(42));
}
