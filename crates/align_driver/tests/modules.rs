//! Multi-file user modules (`draft.md` §17). User modules resolve by filename convention:
//! `import geom` → `geom.align` in the entry file's directory (nested `import a.b` → `a/b.align`),
//! which must declare `module geom`. Cross-module calls are written `geom.fn(...)` and an exported
//! type is named `geom.Point`; both reach only `pub` members. Each module has its own function and
//! type namespace, so two modules may share a name.

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
fn a_local_variable_shadows_a_module_of_the_same_name() {
    if !backend_available() {
        return;
    }
    // A local box named `geom` must shadow the imported module `geom`: `geom.get()` is the box
    // method, not a (nonexistent) cross-module call. Without the shadowing check, module-path
    // resolution would intercept `geom.get()` and reject it.
    let geom = "module geom\npub fn square(x: i64) -> i64 = x * x\n";
    let main = concat!(
        "module main\n",
        "import geom\n",
        "fn main() -> i32 {\n",
        "  arena {\n",
        "    geom := heap.new(7)\n",
        "    return geom.get() as i32\n",
        "  }\n",
        "}\n",
    );
    let out = build_and_run_multi(
        "mod-shadow",
        &[("geom.align", geom), ("main.align", main)],
        "main.align",
    );
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn cross_module_struct_type_is_qualified() {
    if !backend_available() {
        return;
    }
    // A `pub` struct in `geom` is constructed + read in `main` via the qualified `geom.Point`.
    let geom = "module geom\npub Point { x: i64, y: i64 }\n";
    let main = concat!(
        "module main\n",
        "import geom\n",
        "fn main() -> i32 {\n",
        "  p := geom.Point { x: 3, y: 4 }\n",
        "  return (p.x + p.y) as i32\n", // 7
        "}\n",
    );
    let out = build_and_run_multi(
        "mod-struct",
        &[("geom.align", geom), ("main.align", main)],
        "main.align",
    );
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn a_private_struct_is_not_exportable() {
    // A non-`pub` struct is not visible across modules even when qualified.
    let geom = "module geom\nPoint { x: i64, y: i64 }\n";
    let main = "module main\nimport geom\nfn main() -> i32 {\n  p := geom.Point { x: 1, y: 2 }\n  return p.x as i32\n}\n";
    assert!(check_multi_errs("mod-struct-priv", &[("geom.align", geom), ("main.align", main)], "main.align"));
}

#[test]
fn a_qualified_type_requires_importing_its_module() {
    // `geom.Point` needs `import geom`.
    let geom = "module geom\npub Point { x: i64, y: i64 }\n";
    let main = "module main\nfn main() -> i32 {\n  p := geom.Point { x: 1, y: 2 }\n  return p.x as i32\n}\n";
    assert!(check_multi_errs("mod-struct-noimport", &[("geom.align", geom), ("main.align", main)], "main.align"));
}

#[test]
fn an_imported_type_must_be_qualified() {
    // A bare `Point` does not name a type in `main` (it lives in `geom`); it must be `geom.Point`.
    let geom = "module geom\npub Point { x: i64, y: i64 }\n";
    let main = "module main\nimport geom\nfn main() -> i32 {\n  p := Point { x: 1, y: 2 }\n  return p.x as i32\n}\n";
    assert!(check_multi_errs("mod-struct-bare", &[("geom.align", geom), ("main.align", main)], "main.align"));
}

#[test]
fn same_struct_name_in_two_modules_does_not_collide() {
    if !backend_available() {
        return;
    }
    // Each module defines its own `Point`; per-module namespacing keeps them distinct.
    let geom = "module geom\npub Point { x: i64, y: i64 }\n";
    let phys = "module phys\npub Point { mass: i64, vel: i64 }\n";
    let main = concat!(
        "module main\n",
        "import geom\n",
        "import phys\n",
        "fn main() -> i32 {\n",
        "  a := geom.Point { x: 3, y: 4 }\n",
        "  b := phys.Point { mass: 10, vel: 20 }\n",
        "  return (a.x + b.mass) as i32\n", // 3 + 10 = 13
        "}\n",
    );
    let out = build_and_run_multi(
        "mod-struct-dup",
        &[("geom.align", geom), ("phys.align", phys), ("main.align", main)],
        "main.align",
    );
    assert_eq!(out.status.code(), Some(13));
}

#[test]
fn an_enum_in_a_nonentry_module_constructs_and_matches() {
    if !backend_available() {
        return;
    }
    // A sum type declared + constructed + matched entirely inside a non-entry module: per-module
    // type namespacing must resolve the bare `Dir.North` against `nav`'s own canonical key.
    let nav = concat!(
        "module nav\n",
        "Dir { North, South }\n",
        "pub fn step() -> i64 {\n",
        "  d := Dir.North\n",
        "  return match d { North => 1, South => 2 }\n",
        "}\n",
    );
    let main = "module main\nimport nav\nfn main() -> i32 = nav.step() as i32\n";
    let out = build_and_run_multi("mod-enum", &[("nav.align", nav), ("main.align", main)], "main.align");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn an_imported_sum_type_variant_is_constructed_qualified() {
    if !backend_available() {
        return;
    }
    // `mod.Type.Variant` and `mod.Type.Variant(payload)` — constructing an imported `pub` sum type's
    // variant from another module (the receiver `pal.Color` resolves the type qualified).
    let pal = concat!(
        "module pal\n",
        "pub Color { Red, Green, Blue, Code(i32) }\n",
        "pub fn code(c: Color) -> i32 = match c {\n",
        "  Red => 1, Green => 2, Blue => 3, Code(n) => n,\n",
        "}\n",
    );
    let main = concat!(
        "import pal\n",
        "fn main() -> i32 {\n",
        "  g := pal.Color.Green\n",          // tag-only, qualified
        "  c := pal.Color.Code(40)\n",       // payload, qualified
        "  return pal.code(g) + pal.code(c)\n", // 2 + 40 = 42
        "}\n",
    );
    let out = build_and_run_multi("mod-qual-variant", &[("pal.align", pal), ("main.align", main)], "main.align");
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn constructing_a_private_sum_type_variant_across_modules_is_rejected() {
    // A non-`pub` sum type cannot be constructed from an importing module.
    let lib = "module lib\nSecret { A, B }\n";
    let main = "import lib\nfn main() -> i32 {\n  s := lib.Secret.A\n  return 0\n}\n";
    assert!(check_multi_errs("mod-priv-variant", &[("lib.align", lib), ("main.align", main)], "main.align"));
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
