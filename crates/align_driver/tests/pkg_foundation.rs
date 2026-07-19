//! pkg-foundation import-edge rules (F0 of `docs/impl/15-pkg-web-plan.md`; `open-questions.md`
//! "pkg-foundation" D7/D8). Two pure path rules checked per import edge in `load_units`, adding zero
//! new compiler concepts on top of the settled module system:
//!   D7 — the `internal` path rule: a module whose path contains an `internal` segment is importable
//!        only from within the subtree rooted at that segment's parent.
//!   D8 — layering: a module under `pkg/` may import only `core` / `std` / `pkg` modules (never the
//!        consuming project's own modules).

mod common;
use common::*;

// ── D7: the `internal` path rule ──────────────────────────────────────────────────────────────

#[test]
fn internal_module_importable_from_within_its_package() {
    if !backend_available() {
        return;
    }
    // `pkg.web` may import its own `pkg.web.internal.util`; `main` reaches it transitively via the
    // package's public API. 41 + 1 = 42.
    let web = "module pkg.web\nimport pkg.web.internal.util\npub fn dispatch(x: i64) -> i64 = pkg.web.internal.util.helper(x)\n";
    let util = "module pkg.web.internal.util\npub fn helper(x: i64) -> i64 = x + 1\n";
    let main = "module main\nimport pkg.web\nfn main() -> i32 {\n  return pkg.web.dispatch(41) as i32\n}\n";
    let out = build_and_run_multi(
        "f0-internal-ok",
        &[("pkg/web.align", web), ("pkg/web/internal/util.align", util), ("main.align", main)],
        "main.align",
    );
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn internal_module_not_importable_from_outside_its_package() {
    // `main` (a project module) importing `pkg.web.internal.util` directly is illegal — the module
    // is internal to `pkg.web`.
    let util = "module pkg.web.internal.util\npub fn helper(x: i64) -> i64 = x + 1\n";
    let main = "module main\nimport pkg.web.internal.util\nfn main() -> i32 = pkg.web.internal.util.helper(41) as i32\n";
    let d = check_multi_diagnostics(
        "f0-internal-bad",
        &[("pkg/web/internal/util.align", util), ("main.align", main)],
        "main.align",
    );
    assert!(
        d.contains("cannot import internal module") && d.contains("only from within `pkg.web`"),
        "an internal module imported from outside its package must be rejected:\n{d}"
    );
}

#[test]
fn sibling_package_cannot_import_another_packages_internal() {
    // `pkg.other` cannot reach `pkg.web`'s internals — a different subtree, even though both are `pkg`.
    let util = "module pkg.web.internal.util\npub fn helper(x: i64) -> i64 = x + 1\n";
    let other = "module pkg.other\nimport pkg.web.internal.util\npub fn f(x: i64) -> i64 = pkg.web.internal.util.helper(x)\n";
    let main = "module main\nimport pkg.other\nfn main() -> i32 = pkg.other.f(1) as i32\n";
    let d = check_multi_diagnostics(
        "f0-internal-sibling",
        &[("pkg/web/internal/util.align", util), ("pkg/other.align", other), ("main.align", main)],
        "main.align",
    );
    assert!(
        d.contains("cannot import internal module"),
        "a sibling package importing another package's internal must be rejected:\n{d}"
    );
}

// ── D8: pkg layering ──────────────────────────────────────────────────────────────────────────

#[test]
fn pkg_module_importing_project_module_rejected() {
    // A `pkg.*` module reaching back into the consuming project's `helpers` inverts the dependency
    // arrow and would compile in exactly one tree — rejected.
    let helpers = "module helpers\npub fn h(x: i64) -> i64 = x * 2\n";
    let web = "module pkg.web\nimport helpers\npub fn dispatch(x: i64) -> i64 = helpers.h(x)\n";
    let main = "module main\nimport pkg.web\nfn main() -> i32 = pkg.web.dispatch(21) as i32\n";
    let d = check_multi_diagnostics(
        "f0-layering-bad",
        &[("helpers.align", helpers), ("pkg/web.align", web), ("main.align", main)],
        "main.align",
    );
    assert!(
        d.contains("a module under `pkg/` may import only") && d.contains("project module `helpers`"),
        "a pkg module importing a project module must be rejected:\n{d}"
    );
}

#[test]
fn pkg_module_may_import_another_pkg_module() {
    if !backend_available() {
        return;
    }
    // pkg -> pkg is allowed (the layering permits `pkg`). 20 * 2 + 1 = 41.
    let other = "module pkg.other\npub fn twice(x: i64) -> i64 = x * 2\n";
    let web = "module pkg.web\nimport pkg.other\npub fn dispatch(x: i64) -> i64 = pkg.other.twice(x) + 1\n";
    let main = "module main\nimport pkg.web\nfn main() -> i32 {\n  return pkg.web.dispatch(20) as i32\n}\n";
    let out = build_and_run_multi(
        "f0-layering-ok",
        &[("pkg/other.align", other), ("pkg/web.align", web), ("main.align", main)],
        "main.align",
    );
    assert_eq!(out.status.code(), Some(41));
}

#[test]
fn project_module_may_import_pkg_module() {
    if !backend_available() {
        return;
    }
    // The normal direction (project -> pkg) is unrestricted — the whole point of the pkg layer.
    let web = "module pkg.web\npub fn ping(x: i64) -> i64 = x + 100\n";
    let main = "module main\nimport pkg.web\nfn main() -> i32 {\n  return pkg.web.ping(5) as i32\n}\n";
    let out = build_and_run_multi(
        "f0-project-imports-pkg",
        &[("pkg/web.align", web), ("main.align", main)],
        "main.align",
    );
    assert_eq!(out.status.code(), Some(105));
}
