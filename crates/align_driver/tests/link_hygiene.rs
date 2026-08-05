//! M13 Slice 1 — symbol internalization + constant hygiene. Pins the LLVM *linkage map* codegen
//! emits, so the whole-program single-object contract stays explicit and any future regression
//! (a new emitted symbol defaulting back to `external`) is caught here.
//!
//! The decision table (see `align_codegen_llvm` `mark_internal`/`mark_private_helper`/
//! `mark_private_unnamed_addr`):
//!
//! | symbol class                                    | linkage                  |
//! |-------------------------------------------------|--------------------------|
//! | C entry `main` (void/`i32` main, or the wrapper)| external (keep by name)  |
//! | encoded Align body for a wrapped `main`        | internal                 |
//! | every other Align program fn (+ lifted lambdas) | internal                 |
//! | fn-value / closure / spawn / par_map thunks     | private                  |
//! | runtime `align_rt_*` + `extern "C"` declares     | external (undefined decl)|
//! | string / JSON-descriptor / PHF constants        | private unnamed_addr     |
//!
//! Align compiles the entry file + every imported user module into ONE module and ONE object; `pub`
//! is a sema-level module visibility fully resolved before codegen, and no Align function body is
//! C-exported by default. So the ONLY definition the linker must resolve by name is `main`; everything
//! else is module-private. A thunk reached only through a function pointer handed to the runtime keeps
//! `private` (its symbol name is never used). An undefined `declare` cannot be made internal.
//!
//! **Exception:** `emit-obj`/`emit-llvm --export <name>` (M13 Codex-audit item 1, see
//! `export_roots.rs`) names additional program functions that keep `external` linkage instead —
//! a no-`main` library/benchmark object's C-ABI surface. This table describes the DEFAULT (no
//! `--export`) linkage map; every row above is unaffected by an empty export set.

mod common;
use common::*;

/// The text LLVM prints between `define ` and the `@` of the definition named `sym` — i.e. the
/// linkage/attribute words plus the return type. Matches both the bare `@sym(` and the quoted
/// `@"sym"(` form LLVM uses when a symbol contains `$`. Panics if there is no such definition.
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
    assert!(
        pfx.contains("internal"),
        "@{sym} should have `internal` linkage, got `define {pfx}@{sym}(...`"
    );
}

fn assert_private_prefix(ir: &str, prefix: &str) {
    let matches = ir
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("define ") && line.contains(&format!("@\"{prefix}"))
        })
        .collect::<Vec<_>>();
    assert!(!matches.is_empty(), "no generated definition with prefix {prefix}:\n{ir}");
    for line in matches {
        let before_symbol = line
            .split_once('@')
            .expect("a define line always names a symbol")
            .0;
        assert!(
            before_symbol.contains("private"),
            "generated symbol with prefix {prefix} must be private: {line}"
        );
    }
}

fn lowercase_hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn align_symbol(value: &str) -> String {
    format!("align_fn${}${}", value.len(), lowercase_hex(value))
}

/// External linkage prints as *no* linkage word — LLVM omits it. So the `define` prefix must carry
/// neither `internal` nor `private`.
fn assert_external(ir: &str, sym: &str) {
    let pfx = define_prefix(ir, sym);
    assert!(
        !pfx.contains("internal") && !pfx.contains("private"),
        "@{sym} must stay external (the linker/runtime resolves it by name), got `define {pfx}@{sym}(...`"
    );
}

/// The `@sym = ...` global-definition line, panicking if absent. (`@sym` matches the first global
/// of that base name; codegen suffixes later ones `@sym.1` etc., all sharing the same linkage.)
fn global_line<'a>(ir: &'a str, sym: &str) -> &'a str {
    let needle = format!("@{sym} =");
    ir.lines()
        .find(|l| l.trim_start().starts_with(&needle))
        .unwrap_or_else(|| panic!("no global @{sym} found in IR:\n{ir}"))
}

/// A representative program touching most emitted symbol classes at once: an `extern "C"` decl, a
/// non-exported helper, a `Result`-returning `main` (encoded body + generated C `main`), lifted
/// pipeline/`par_map`/closure lambdas, a string constant, and generated parallel/closure helpers.
const REPRESENTATIVE: &str = concat!(
    "extern \"C\" fn cabs(x: i32) -> i32\n",
    "\n",
    "fn helper(x: i64) -> i64 = x * 2 + 1\n",
    "\n",
    "fn main() -> Result<(), Error> {\n",
    "  s := \"align link hygiene representative string\"\n",
    "  print(s)\n",
    "  print(helper(20))\n",
    "  print([1, 2, 3].par_map(fn x { x + 100 }).sum())\n",
    "  k: i32 := 7\n",
    "  g := fn x: i32 { x + k }\n",
    "  print(g(5) as i64)\n",
    "  unsafe { print(cabs(-5) as i64) }\n",
    "  return Ok(())\n",
    "}\n",
);

#[test]
fn representative_program_linkage_map() {
    if !backend_available() {
        return;
    }
    let ir = emit_llvm(REPRESENTATIVE);

    // The C entry is the ONLY definition that stays external.
    assert_external(&ir, "main");

    // A `Result`-main's body and every other Align program function (incl. lifted lambdas) → internal.
    assert_internal(&ir, &align_symbol("main"));
    assert_internal(&ir, &align_symbol("helper"));
    assert_internal(&ir, &align_symbol("main$lambda0"));
    assert_internal(&ir, &align_symbol("main$lambda1"));
    assert_internal(&ir, &align_symbol("main$lambda2"));

    // Compiler-generated helper thunks, reached only via a function pointer → private.
    assert_private_prefix(&ir, "align_gen$par$");
    assert_private_prefix(
        &ir,
        &format!("align_gen$clos${}$", lowercase_hex("main$lambda2")),
    );

    // The `extern "C"` symbol is an undefined declaration resolved by the linker → external, and it
    // must be a `declare` (never `define internal`, which would drop the reference to libc's `abs`).
    assert!(
        ir.contains("declare i32 @cabs("),
        "the extern \"C\" `cabs` must be an external declaration:\n{ir}"
    );

    // String literal bytes → `private unnamed_addr constant`: private hides the symbol, unnamed_addr
    // lets `constmerge` fold equal literals, constant marks the immutable bytes.
    let str_line = global_line(&ir, "str");
    assert!(
        str_line.contains("private unnamed_addr constant"),
        "string constant must be `private unnamed_addr constant`, got:\n{str_line}"
    );
}

#[test]
fn plain_main_stays_external_no_wrapper() {
    if !backend_available() {
        return;
    }
    // An `-> i32` `main` IS the C entry directly (no wrapper or encoded body) — its LLVM return
    // type already matches the C ABI's `i32` — so it keeps the symbol name `main` and external
    // linkage; a sibling helper is still internalized.
    let ir = emit_llvm("fn helper(x: i64) -> i64 = x + 1\nfn main() -> i32 {\n  print(helper(41))\n  return 0\n}\n");
    assert_external(&ir, "main");
    assert_internal(&ir, &align_symbol("helper"));
    assert!(
        !ir.contains(&align_symbol("main")),
        "an `-> i32` `main` needs no separate Align body:\n{ir}"
    );
}

#[test]
fn unit_main_gets_the_c_entry_wrapper() {
    if !backend_available() {
        return;
    }
    // A `Unit`-returning `main` is NOT the C entry directly (it lowers to `void`, and the C ABI's
    // `main` must return `i32` — leaving `main` void would leave the return register undefined,
    // `docs/open-questions.md` "Unit-returning `fn main()` yields a nondeterministic exit code").
    // Its encoded program identity stays internal and gets a generated external `main` wrapper that
    // always returns a defined `i32`, same shape as the `Result`-returning case.
    let ir = emit_llvm("fn helper(x: i64) -> i64 = x + 1\nfn main() {\n  print(helper(41))\n}\n");
    assert_external(&ir, "main");
    let body = align_symbol("main");
    assert_internal(&ir, &body);
    assert_internal(&ir, &align_symbol("helper"));
    assert!(
        ir.contains(&format!("call void @\"{body}\"()")),
        "wrapper must call the encoded Align body:\n{ir}"
    );
    assert!(ir.contains("ret i32 0"), "wrapper must return a defined 0:\n{ir}");
}

#[test]
fn fn_value_thunk_is_private() {
    if !backend_available() {
        return;
    }
    // Using a function as a first-class value emits a canonical adapter thunk (called through the
    // fn-value pointer) → private; the underlying function → internal.
    let ir = emit_llvm(
        "fn double(x: i32) -> i32 = x * 2\n\nfn main() -> Result<(), Error> {\n  f := double\n  print(f(5))\n  return Ok(())\n}\n",
    );
    assert_internal(&ir, &align_symbol("double"));
    assert_private_prefix(
        &ir,
        &format!("align_gen$fnval${}$", lowercase_hex("double")),
    );
    assert_external(&ir, "main");
}

#[test]
fn spawn_trampoline_is_private() {
    if !backend_available() {
        return;
    }
    // A `spawn`ed closure emits a canonical task trampoline (invoked by the task runtime through a
    // pointer) → private, plus its canonical capturing-closure thunk → private.
    let ir = emit_llvm(
        "fn main() -> Result<(), Error> {\n  k: i64 := 100\n  task_group {\n    a := spawn(fn { k + 5 })\n    wait()\n    print(a.get())\n  }\n  return Ok(())\n}\n",
    );
    assert_private_prefix(&ir, "align_gen$tramp$");
    assert_private_prefix(
        &ir,
        &format!("align_gen$clos${}$", lowercase_hex("main$lambda0")),
    );
    assert_external(&ir, "main");
}

#[test]
fn json_descriptor_globals_are_private_unnamed_addr() {
    if !backend_available() {
        return;
    }
    // `json.decode` emits a field-descriptor table (`@jfields`) and a perfect-hash table (`@jphf`),
    // both immutable → `private unnamed_addr constant`.
    let ir = emit_llvm(
        "import core.json\nUser { id: i64, score: i32 }\nfn main() -> Result<(), Error> {\n  u: User := json.decode(\"{\\\"id\\\": 7, \\\"score\\\": 3}\")?\n  print(u.id)\n  return Ok(())\n}\n",
    );
    for g in ["jfields", "jphf"] {
        let line = global_line(&ir, g);
        assert!(
            line.contains("private unnamed_addr constant"),
            "@{g} must be `private unnamed_addr constant`, got:\n{line}"
        );
    }
}

#[test]
fn runtime_declares_stay_external() {
    if !backend_available() {
        return;
    }
    // Runtime builtins are undefined `declare`s the linker resolves against `libalign_runtime.a`;
    // they must NOT be internalized (that would leave the reference unresolved).
    let ir = emit_llvm("fn main() {\n  print(7)\n}\n");
    let decl = ir
        .lines()
        .find(|l| l.trim_start().starts_with("declare ") && l.contains("@align_rt_print_i64("))
        .expect("the print builtin must be declared");
    assert!(
        !decl.contains("internal") && !decl.contains("private"),
        "a runtime declaration cannot be internal/private:\n{decl}"
    );
}
