//! Unused-import lint — a `warning` (not an error) for an `import` whose module is never referenced
//! in the file. A builtin (`core.json`) is matched by its `json.*` namespace; a user module by its
//! dotted path. Detection is a syntactic AST walk, so signatures / bodies / consts are covered
//! uniformly. A used import (anywhere) emits nothing.

mod common;
use common::*;

/// Whether checking `src` emits an "unused import" diagnostic.
fn warns_unused(name: &str, src: &str) -> bool {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    align_driver::format_diagnostics(&sm, &checked.diags).contains("unused import")
}

/// Same, for a multi-file program (entry + other modules), checked from the entry.
fn warns_unused_multi(name: &str, files: &[(&str, &str)], entry: &str) -> bool {
    use std::path::PathBuf;
    let dir = std::env::temp_dir().join(format!("align-uimp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (f, s) in files {
        std::fs::write(dir.join(f), s).unwrap();
    }
    let entry_path: PathBuf = dir.join(entry);
    let entry_src = std::fs::read_to_string(&entry_path).unwrap();
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, &entry_path.display().to_string(), &entry_src);
    let warned = align_driver::format_diagnostics(&sm, &checked.diags).contains("unused import");
    let _ = std::fs::remove_dir_all(&dir);
    warned
}

#[test]
fn an_unused_builtin_import_warns() {
    assert!(warns_unused("uimp-builtin", "import core.json\nfn main() -> i32 { return 0 }\n"));
}

#[test]
fn a_used_builtin_import_does_not_warn() {
    // `json.decode` references the `json` namespace, so `core.json` is used.
    let src = concat!(
        "import core.json\n",
        "User { id: i64 }\n",
        "fn main() -> i32 {\n",
        "  u: User := json.decode(\"{}\") else User { id: 0 }\n",
        "  return u.id as i32\n",
        "}\n",
    );
    assert!(!warns_unused("uimp-builtin-used", src));
}

#[test]
fn an_unused_user_import_warns() {
    let geom = "module geom\npub fn area(w: i64, h: i64) -> i64 = w * h\n";
    let main = "import geom\nfn main() -> i32 { return 0 }\n";
    assert!(warns_unused_multi("uimp-user", &[("geom.align", geom), ("main.align", main)], "main.align"));
}

#[test]
fn a_user_import_used_in_a_call_does_not_warn() {
    let geom = "module geom\npub fn area(w: i64, h: i64) -> i64 = w * h\n";
    let main = "import geom\nfn main() -> i32 { return geom.area(6, 7) as i32 }\n";
    assert!(!warns_unused_multi("uimp-user-call", &[("geom.align", geom), ("main.align", main)], "main.align"));
}

#[test]
fn a_user_import_used_only_in_a_signature_does_not_warn() {
    // The walk covers signatures, so a qualified parameter type counts as a use.
    let g = "module g\npub Point { x: i64, y: i64 }\n";
    let main = concat!(
        "import g\n",
        "fn px(p: g.Point) -> i64 = p.x\n",
        "fn main() -> i32 { return 0 }\n",
    );
    assert!(!warns_unused_multi("uimp-sig", &[("g.align", g), ("main.align", main)], "main.align"));
}

#[test]
fn a_user_import_used_only_in_a_constant_does_not_warn() {
    // A qualified constant reference in a top-level constant initializer counts as a use.
    let cfg = "module cfg\npub BASE: i32 := 7\n";
    let main = concat!(
        "import cfg\n",
        "DOUBLED: i32 := cfg.BASE + cfg.BASE\n",
        "fn main() -> i32 { return DOUBLED }\n",
    );
    assert!(!warns_unused_multi("uimp-const", &[("cfg.align", cfg), ("main.align", main)], "main.align"));
}

#[test]
fn an_unused_import_is_not_a_hard_error() {
    // The lint is a warning: the program still type-checks (no error).
    assert!(!check_errs("uimp-not-error", "import core.json\nfn main() -> i32 { return 0 }\n"));
}
