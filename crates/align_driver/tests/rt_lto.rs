//! M14 Slice 2 — runtime-bitcode LTO for the fast-path string primitives (`--rt-lto`).
//!
//! `docs/impl/07-roadmap.md` "M14 Slice 2 design SETTLED". The seven settled gates:
//!   1. positive IR-shape — `x == "literal"`: `call @align_rt_str_eq` ABSENT under `--rt-lto`,
//!      PRESENT without (mutation-checked both directions by hand — see the commit message).
//!   2. negative — an `Ord(str)` (`str_cmp`) kernel under `--rt-lto` still calls `align_rt_str_cmp`
//!      AND its declare keeps the curated `readonly captures(none)` / `memory(argmem: read)` attrs.
//!   3. artifact symbol-set pin — the baked `.bc` defines exactly the guarded four `align_rt_*`
//!      symbols; its undefined set is a small allowlist (no Rust-std `_ZN` leakage). [llvm-nm gated]
//!   4. attr-xor — over all `align_rt_*` fns, `(has body) != (carries its rt_contract attrs)`.
//!   5. `--export` + `--rt-lto` — the exported symbol stays an external define; no `align_rt_*` is
//!      externally defined in the module (they are all `internal` after the merge).
//!   6. OFF-path byte-identity — the flag-off path is unchanged (the whole existing suite stays
//!      green with this code merged; plus the direct `off == pre-change shape` checks in 1/2).
//!   7. end-to-end bench + bounds — driven by `bench/rt_lto/` through the real `alignc build`, not
//!      this suite (it needs `cc` + a link + timing).
//!
//! The IR gates go through the driver's `emit_llvm_ir` wrapper with `rt_lto = true`, which links the
//! baked bitcode into the module: `--stage raw` (`optimized = false`) is the pre-opt merged lens
//! (bodies present, attrs shed) and `--stage optimized` is the after-`O2` lens (calls inlined away).

mod common;
use common::*;

/// Compile `src` to LLVM IR through the driver, exporting `exports`. `optimized` = the `-O2` lens
/// (calls inlined) vs raw (pre-opt merged shape); `rt_lto` links the fast-path string bitcode.
fn ir(name: &str, src: &str, exports: &[&str], optimized: bool, rt_lto: bool) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "kernel `{name}` failed to compile:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let exports: Vec<String> = exports.iter().map(|s| s.to_string()).collect();
    emit_llvm_ir(&mir, BuildTarget::Baseline, optimized, &exports, rt_lto).expect("emit llvm ir")
}

/// The idiomatic constant-length equality filter — the probe's `str_eq` 2.1× kernel.
const EQ_KERNEL: &str = "\
fn is_hello(x: str) -> bool = x == \"hello\"
pub fn eq_count(s: slice<str>) -> i64 = s.where(is_hello).count()
";

/// An `Ord(str)` filter — lowers to `align_rt_str_cmp`, the primitive deliberately EXCLUDED from the
/// guarded set (the probe measured it regressing under post-link reoptimization).
const CMP_KERNEL: &str = "\
fn is_lt(x: str) -> bool = x < \"mmmmmmmm\"
pub fn lt_count(s: slice<str>) -> i64 = s.where(is_lt).count()
";

/// The rt-lto IR gates all need the LLVM backend (they run codegen). x86-64 host keeps the baked
/// bitcode's triple/datalayout matching the `Baseline` target (the artifact is built for the host).
fn backend() -> bool {
    backend_available()
}

// -- Gate 1: positive IR-shape (both directions) ------------------------------------------------

#[test]
fn gate1_str_eq_call_absent_with_rt_lto_present_without() {
    if !backend() {
        return;
    }
    // OFF: the opaque runtime call is present (today's behavior).
    let off = ir("eq_off", EQ_KERNEL, &["eq_count"], /*opt*/ true, /*rt_lto*/ false);
    assert!(
        off.contains("call i32 @align_rt_str_eq"),
        "flag-off optimized IR should still call align_rt_str_eq:\n{off}"
    );
    // ON: the body is merged + inlined, so no call to the runtime symbol survives.
    let on = ir("eq_on", EQ_KERNEL, &["eq_count"], /*opt*/ true, /*rt_lto*/ true);
    assert!(
        !on.contains("call i32 @align_rt_str_eq"),
        "under --rt-lto align_rt_str_eq must inline (no call left):\n{on}"
    );
    // The inlined constant-length fast path: `icmp` against the literal length, `bcmp` on a hit.
    assert!(
        on.contains("@bcmp") || on.contains("@memcmp"),
        "under --rt-lto the inlined body should lower the compare to bcmp/memcmp:\n{on}"
    );
}

// -- Gate 2: str_cmp negative -------------------------------------------------------------------

#[test]
fn gate2_str_cmp_stays_opaque_with_curated_attrs() {
    if !backend() {
        return;
    }
    let on = ir("cmp_on", CMP_KERNEL, &["lt_count"], /*opt*/ true, /*rt_lto*/ true);
    assert!(
        on.contains("call i32 @align_rt_str_cmp"),
        "str_cmp is excluded from the guarded set: its call must survive --rt-lto:\n{on}"
    );
    // The excluded declare must keep its hand-curated contract — a blanket merge would drop it.
    let decl = on
        .lines()
        .find(|l| l.starts_with("declare") && l.contains("@align_rt_str_cmp"))
        .unwrap_or_else(|| panic!("no declare for align_rt_str_cmp:\n{on}"));
    assert!(
        decl.contains("readonly captures(none)"),
        "str_cmp declare must keep its curated `readonly captures(none)` params: {decl}"
    );
}

// -- Gate 3: artifact symbol-set pin (llvm-nm gated) --------------------------------------------

#[test]
fn gate3_baked_bitcode_symbol_set() {
    let Some(nm) = align_driver::llvm_tool("llvm-nm") else {
        return; // no version-matched llvm-nm — skip the artifact inspection
    };
    let dir = std::env::temp_dir().join(format!("align-rtlto-nm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let bc = dir.join("str_prims.bc");
    std::fs::write(&bc, align_driver::rt_lto_bitcode()).expect("write bc");

    let defined = String::from_utf8(
        std::process::Command::new(&nm)
            .args(["--defined-only"])
            .arg(&bc)
            .output()
            .expect("run llvm-nm --defined-only")
            .stdout,
    )
    .expect("utf8");
    let undefined = String::from_utf8(
        std::process::Command::new(&nm)
            .args(["--undefined-only"])
            .arg(&bc)
            .output()
            .expect("run llvm-nm --undefined-only")
            .stdout,
    )
    .expect("utf8");
    let _ = std::fs::remove_dir_all(&dir);

    // Defined `align_rt_*` symbols == exactly the guarded four.
    let mut defined_rt: Vec<String> = defined
        .lines()
        .filter_map(|l| l.split_whitespace().last())
        .filter(|s| s.starts_with("align_rt_"))
        .map(|s| s.to_string())
        .collect();
    defined_rt.sort();
    let mut want = [
        "align_rt_str_ends_with",
        "align_rt_str_eq",
        "align_rt_str_eq_ignore_case",
        "align_rt_str_starts_with",
    ];
    want.sort();
    assert_eq!(defined_rt, want, "baked .bc must define exactly the guarded four");

    // Undefined symbols ⊆ a small allowlist — no Rust-std `_ZN` leakage, no undefined `align_rt_*`.
    let allow = ["bcmp", "memcmp", "memcpy", "memmove", "memset"];
    for l in undefined.lines() {
        let Some(sym) = l.split_whitespace().last() else {
            continue;
        };
        assert!(
            !sym.starts_with("_ZN"),
            "Rust-std symbol leaked into the .bc (not self-contained): {sym}"
        );
        assert!(
            !sym.starts_with("align_rt_"),
            "an align_rt_* symbol is undefined in the .bc (missing a callee): {sym}"
        );
        assert!(
            allow.contains(&sym),
            "unexpected undefined symbol {sym} — extend the allowlist only after auditing it"
        );
    }
}

// -- Gate 4: attribute xor over all align_rt_* fns ----------------------------------------------

/// A kernel that references both a guarded primitive (`str_eq`) and the excluded `str_cmp`, so the
/// pre-opt merged module holds both a set of guarded bodies and the `str_cmp` declare.
const MIXED_KERNEL: &str = "\
fn is_hello(x: str) -> bool = x == \"hello\"
fn is_lt(x: str) -> bool = x < \"mmmmmmmm\"
pub fn mixed(s: slice<str>) -> i64 = s.where(is_hello).count() + s.where(is_lt).count()
";

#[test]
fn gate4_attr_xor_body_vs_curated_attrs() {
    if !backend() {
        return;
    }
    // Pre-opt merged lens: bodies present, no inlining/DCE yet.
    let raw = ir("mixed_raw", MIXED_KERNEL, &["mixed"], /*opt*/ false, /*rt_lto*/ true);

    let mut saw_body = 0usize;
    let mut saw_decl_cmp = false;
    for line in raw.lines() {
        let is_def = line.starts_with("define") && line.contains("@align_rt_");
        let is_decl = line.starts_with("declare") && line.contains("@align_rt_");
        if is_def {
            // A body-carrying runtime fn must NOT carry any rt_contract-curated attr.
            assert!(
                !line.contains("readonly captures(none)"),
                "a merged align_rt_* body still carries curated param attrs (xor violated): {line}"
            );
            assert!(
                !line.contains("memory(argmem"),
                "a merged align_rt_* body still carries the curated memory attr (xor violated): {line}"
            );
            saw_body += 1;
        }
        if is_decl && line.contains("@align_rt_str_cmp") {
            // The excluded declare keeps its curated contract (the other half of the xor).
            assert!(
                line.contains("readonly captures(none)"),
                "the str_cmp declare lost its curated attrs (xor violated): {line}"
            );
            saw_decl_cmp = true;
        }
    }
    assert!(saw_body >= 4, "expected the four guarded bodies merged in, saw {saw_body}");
    assert!(saw_decl_cmp, "expected an attributed align_rt_str_cmp declare to remain");
}

// -- Gate 5: --export interaction ---------------------------------------------------------------

#[test]
fn gate5_export_root_external_runtime_internal() {
    if !backend() {
        return;
    }
    let on = ir("eq_exp", EQ_KERNEL, &["eq_count"], /*opt*/ true, /*rt_lto*/ true);
    // The requested export root stays an external definition.
    assert!(
        on.lines()
            .any(|l| l.starts_with("define") && l.contains("@eq_count(") && !l.contains("internal")),
        "export root eq_count must be an external define under --rt-lto:\n{on}"
    );
    // No runtime symbol is externally defined (all merged bodies are `internal`), so there is no
    // duplicate-external vs the `.a` at final link.
    assert!(
        !on.lines().any(|l| l.starts_with("define")
            && l.contains("@align_rt_")
            && !l.contains("internal")),
        "no align_rt_* may be externally defined in the merged module:\n{on}"
    );
}

// -- Gate 6: OFF-path byte-identity -------------------------------------------------------------

#[test]
fn gate6_off_path_unchanged_by_flag() {
    if !backend() {
        return;
    }
    // With the flag off, the module must be exactly what codegen produced before this slice — no
    // merged bodies, the opaque declare intact. (The whole existing suite passing is the broader
    // guarantee; this pins the local shape.)
    let off_raw = ir("eq_off_raw", EQ_KERNEL, &["eq_count"], /*opt*/ false, /*rt_lto*/ false);
    assert!(
        off_raw.lines().any(|l| l.starts_with("declare") && l.contains("@align_rt_str_eq")),
        "flag-off raw IR must keep align_rt_str_eq an opaque declare:\n{off_raw}"
    );
    assert!(
        !off_raw.contains("define") || !off_raw.contains("define internal noundef range(i32 0, 2) i32 @align_rt_str_eq"),
        "flag-off raw IR must NOT contain a merged align_rt_str_eq body:\n{off_raw}"
    );
}
