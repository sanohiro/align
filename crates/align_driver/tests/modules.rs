//! Multi-file user modules (`draft.md` §17, module-system slice B1). User modules resolve by
//! filename convention: `import geom` → `geom.align` in the entry file's directory, which must
//! declare `module geom`. Cross-module calls are written `geom.fn(...)` and reach only `pub`
//! functions; each module has its own function namespace (two modules may share a name).

mod common;
use common::*;

#[test]
fn cross_module_pub_call_runs() {
    if !backend_available() {
        return;
    }
    let geom = "module geom\npub fn square(x: i64) -> i64 = x * x\n";
    let main = "module main\nimport geom\nfn main() -> i32 {\n  return geom.square(5) as i32\n}\n";
    let out = build_and_run_multi("mod-cross", &[("geom.align", geom), ("main.align", main)], "main.align");
    assert_eq!(out.status.code(), Some(25));
}

#[test]
fn calling_a_private_function_is_rejected() {
    // A non-`pub` function is not visible across modules.
    let geom = "module geom\nfn helper(x: i64) -> i64 = x + 1\n";
    let main = "module main\nimport geom\nfn main() -> i32 = geom.helper(5) as i32\n";
    assert!(check_multi_errs("mod-private", &[("geom.align", geom), ("main.align", main)], "main.align"));
}

#[test]
fn calling_without_importing_is_rejected() {
    // `geom.square` requires `import geom`.
    let geom = "module geom\npub fn square(x: i64) -> i64 = x * x\n";
    let main = "module main\nfn main() -> i32 = geom.square(5) as i32\n";
    assert!(check_multi_errs("mod-noimport", &[("geom.align", geom), ("main.align", main)], "main.align"));
}

#[test]
fn missing_module_file_is_rejected() {
    let main = "module main\nimport geom\nfn main() -> i32 = 0\n";
    assert!(check_multi_errs("mod-missing", &[("main.align", main)], "main.align"));
}

#[test]
fn module_name_must_match_filename() {
    // `geom.align` declares the wrong module name.
    let geom = "module shapes\npub fn square(x: i64) -> i64 = x * x\n";
    let main = "module main\nimport geom\nfn main() -> i32 = geom.square(5) as i32\n";
    assert!(check_multi_errs("mod-mismatch", &[("geom.align", geom), ("main.align", main)], "main.align"));
}

#[test]
fn same_function_name_in_two_modules_does_not_collide() {
    if !backend_available() {
        return;
    }
    // Each module has its own `helper`; per-module mangling keeps them distinct. `main` also has a
    // private `helper`, exercising the bare-call-resolves-in-own-module rule.
    let geom = "module geom\npub fn helper() -> i64 = 10\n";
    let util = "module util\npub fn helper() -> i64 = 20\n";
    let main = concat!(
        "module main\n",
        "import geom\n",
        "import util\n",
        "fn helper() -> i64 = 3\n",
        "fn main() -> i32 {\n",
        "  return (geom.helper() + util.helper() + helper()) as i32\n", // 10 + 20 + 3 = 33
        "}\n",
    );
    let out = build_and_run_multi(
        "mod-namespace",
        &[("geom.align", geom), ("util.align", util), ("main.align", main)],
        "main.align",
    );
    assert_eq!(out.status.code(), Some(33));
}

#[test]
fn transitive_imports_load() {
    if !backend_available() {
        return;
    }
    // main → mid → leaf. `mid` calls `leaf.base()`; `main` calls `mid.bump()`.
    let leaf = "module leaf\npub fn base() -> i64 = 40\n";
    let mid = "module mid\nimport leaf\npub fn bump() -> i64 = leaf.base() + 2\n";
    let main = "module main\nimport mid\nfn main() -> i32 = mid.bump() as i32\n";
    let out = build_and_run_multi(
        "mod-transitive",
        &[("leaf.align", leaf), ("mid.align", mid), ("main.align", main)],
        "main.align",
    );
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn nested_module_path_resolves_to_subdirectory() {
    if !backend_available() {
        return;
    }
    // `import util.math` → `util/math.align` (declaring `module util.math`); called `util.math.fn`.
    let math = "module util.math\npub fn cube(x: i64) -> i64 = x * x * x\n";
    let main = "module main\nimport util.math\nfn main() -> i32 = util.math.cube(3) as i32\n";
    let out = build_and_run_multi(
        "mod-nested",
        &[("util/math.align", math), ("main.align", main)],
        "main.align",
    );
    assert_eq!(out.status.code(), Some(27));
}

#[test]
fn nested_module_wrong_declaration_is_rejected() {
    // `util/math.align` must declare the full `module util.math`, not just `module math`.
    let math = "module math\npub fn cube(x: i64) -> i64 = x * x * x\n";
    let main = "module main\nimport util.math\nfn main() -> i32 = util.math.cube(3) as i32\n";
    assert!(check_multi_errs("mod-nested-bad", &[("util/math.align", math), ("main.align", main)], "main.align"));
}

#[test]
fn a_module_using_a_builtin_must_import_it() {
    // The capability rule applies per file: `geom` uses `json` but does not import `core.json`.
    let geom = concat!(
        "module geom\n",
        "User { id: i64 }\n",
        "pub fn parse(s: str) -> Result<i64, Error> {\n",
        "  u: User := json.decode(s)?\n",
        "  return Ok(u.id)\n",
        "}\n",
    );
    let main = "module main\nimport geom\nfn main() -> i32 = 0\n";
    assert!(check_multi_errs("mod-cap", &[("geom.align", geom), ("main.align", main)], "main.align"));
}
