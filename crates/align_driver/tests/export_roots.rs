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
    assert_internal(&ir, "k2");
    assert_internal(&ir, "helper");

    // `helper` must still be defined and actually called from `k1` — the export-roots change must
    // not perturb which functions are lowered or how they call each other, only their linkage.
    assert!(
        ir.contains("call i64 @helper(") || ir.contains("call i64 @\"helper\"("),
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
    assert_internal(&ir, "k1");
    assert_internal(&ir, "k2");
    assert_internal(&ir, "helper");
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
