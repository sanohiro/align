//! The `import` system, slice A (`draft.md` §17, `open-questions.md` module system). `core` and
//! `std` are compiler builtins, but the **prefix-accessed** namespaces — `json` (`core.json`),
//! `fs` (`std.fs`), `io` (`std.io`) — must be `import`ed before use: a file's capability surface
//! is visible in its header ("Nothing hidden"). The language-syntactic core (`Option`/`Result`/`?`,
//! `arena`, the array pipeline `.map`/`.where`/`.sum`, math methods) needs no import.

mod common;
use common::*;

#[test]
fn json_without_import_is_rejected() {
    let src = concat!(
        "User { id: i64 }\n",
        "fn main() -> Result<(), Error> {\n",
        "  u: User := json.decode(\"{}\")?\n",
        "  return Ok(())\n",
        "}\n",
    );
    assert!(check_errs("imp-json-missing", src));
}

#[test]
fn json_with_import_checks_and_runs() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "import core.json\n",
        "User { id: i64 }\n",
        "fn main() -> Result<(), Error> {\n",
        "  u: User := json.decode(\"{\\\"id\\\": 7}\")?\n",
        "  print(u.id)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("imp-json-ok", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn fs_and_io_require_their_imports() {
    // `fs.read_file` needs `import std.fs`; `io.stdout.write` needs `import std.io`.
    let fs_src = "fn main() -> Result<(), Error> {\n  data := fs.read_file(\"x\")?\n  return Ok(())\n}\n";
    assert!(check_errs("imp-fs-missing", fs_src));
    let io_src = "fn main() -> Result<(), Error> {\n  io.stdout.write(\"hi\")?\n  return Ok(())\n}\n";
    assert!(check_errs("imp-io-missing", io_src));
}

#[test]
fn unknown_module_is_rejected() {
    let src = "import core.frobnicate\nfn main() -> i32 = 0\n";
    assert!(check_errs("imp-unknown", src));
}

#[test]
fn duplicate_import_is_rejected() {
    let src = "import core.json\nimport core.json\nfn main() -> i32 = 0\n";
    assert!(check_errs("imp-dup", src));
}

#[test]
fn syntactic_core_needs_no_import() {
    if !backend_available() {
        return;
    }
    // The array pipeline / `Option` / `Result` / `arena` are language syntax, not imported modules.
    let src = concat!(
        "fn dbl(x: i64) -> i64 = x * 2\n",
        "fn main() -> i32 {\n",
        "  xs := [1, 2, 3]\n",
        "  return xs.map(dbl).sum() as i32\n",
        "}\n",
    );
    let out = build_and_run("imp-syntactic", src);
    assert_eq!(out.status.code(), Some(12));
}

#[test]
fn importing_a_capability_used_elsewhere_is_still_required_per_file() {
    // A `module` declaration is allowed (single-file today); the import rule still applies.
    let src = concat!(
        "module main\n",
        "import std.io\n",
        "fn main() -> Result<(), Error> {\n",
        "  io.stdout.write(\"hi\")?\n",
        "  return Ok(())\n",
        "}\n",
    );
    if !backend_available() {
        return;
    }
    let out = build_and_run("imp-module-decl", src);
    assert_eq!(out.status.code(), Some(0));
}
