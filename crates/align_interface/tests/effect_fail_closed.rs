//! M15 S1b gate 3 (fail-closed effect bits): a cross-unit `par_map` callee whose effect is ABSENT
//! from `external_effects` (a corrupted / truncated summary that dropped the bit) must be rejected —
//! never optimistically Pure. This exercises the `None` branch of sema's cross-unit effect seeding
//! directly, which the driver's `check_per_unit` (which always builds a complete effect map) cannot
//! reach. The contrast test shows the SAME program accepts when the bit is present and Pure.

use std::collections::HashMap;

use align_diag::Diagnostics;
use align_sema::{check_program_with_effects, FnEffect, Module};

#[test]
fn shipped_web_and_websocket_task_dispatch_checks() {
    // Keep the real package graph in the bounded gate: a trivial import must still check
    // the task consumer bodies, including their opaque handler and middleware dispatch.
    let paths = [
        ("pkg.web.types", "apps/web/pkg/web/types.align"),
        ("pkg.web.internal.router", "apps/web/pkg/web/internal/router.align"),
        ("pkg.web.internal.query", "apps/web/pkg/web/internal/query.align"),
        ("pkg.web", "apps/web/pkg/web.align"),
        ("pkg.ws", "apps/ws/pkg/ws.align"),
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = paths.iter().map(|(module, path)| {
        let source = std::fs::read_to_string(root.join(path)).unwrap_or_else(|error| panic!("{path}: {error}"));
        (*module, source)
    }).collect::<Vec<_>>();
    sources.push(("main", "module main\nimport pkg.web\nimport pkg.ws\nfn main() -> i32 = 0\n".to_owned()));
    let files = sources.iter().map(|(_, source)| parse(source)).collect::<Vec<_>>();
    let modules = sources.iter().zip(&files).map(|((path, _), file)| Module {
        path: (*path).to_owned(), file, is_entry: *path == "main", interface_only: false,
    }).collect::<Vec<_>>();
    let mut diagnostics = Diagnostics::new();
    check_program_with_effects(&modules, &HashMap::new(), &mut diagnostics);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
}

fn parse(src: &str) -> align_ast::File {
    let mut sm = align_span::SourceMap::new();
    let fid = sm.add_file("<test>", src);
    let mut diags = Diagnostics::new();
    let toks = align_lexer::tokenize(fid, src, &mut diags);
    let file = align_parser::parse_file(toks, &mut diags);
    assert!(!diags.has_errors(), "test source failed to parse");
    file
}

/// Build the two modules: an interface-only dependency `geom` exposing `noisy`, and a `main` that
/// `par_map`s over a local wrapper calling `geom.noisy`. Returns whether checking (with the given
/// cross-unit effect seeds) reports an error.
fn check_with_effects(external: &HashMap<String, FnEffect>) -> bool {
    // The dependency body is pure-looking, but it is interface-only: its body is NOT analyzed here —
    // its effect must come from `external`. When `external` omits it, the fail-closed default applies.
    let geom = parse("pub fn noisy(x: i64) -> i64 = x + 1\n");
    let main = parse(concat!(
        "import geom\n",
        "fn w(x: i64) -> i64 = geom.noisy(x)\n",
        "fn main() -> Result<(), Error> {\n",
        "  out := [1, 2, 3].par_map(w)\n",
        "  print(out.sum())\n",
        "  return Ok(())\n",
        "}\n",
    ));
    let modules = [
        Module { path: "geom".to_string(), file: &geom, is_entry: false, interface_only: true },
        Module { path: "main".to_string(), file: &main, is_entry: true, interface_only: false },
    ];
    let mut diags = Diagnostics::new();
    check_program_with_effects(&modules, external, &mut diags);
    diags.has_errors()
}

#[test]
fn absent_effect_bit_is_rejected_at_par_map() {
    // Empty map: `geom$noisy` is absent → fail-closed to impure + unknown → par_map(w) rejected.
    let empty: HashMap<String, FnEffect> = HashMap::new();
    assert!(check_with_effects(&empty), "an absent cross-unit effect bit must fail closed at par_map");
}

#[test]
fn unknown_effect_bit_is_rejected_at_par_map() {
    let mut m = HashMap::new();
    m.insert("geom$noisy".to_string(), FnEffect::Unknown);
    assert!(check_with_effects(&m), "an Unknown cross-unit effect bit must be rejected at par_map");
}

#[test]
fn impure_effect_bit_is_rejected_at_par_map() {
    let mut m = HashMap::new();
    m.insert("geom$noisy".to_string(), FnEffect::Impure);
    assert!(check_with_effects(&m), "an Impure cross-unit effect bit must be rejected at par_map");
}

#[test]
fn pure_effect_bit_is_accepted_at_par_map() {
    // The SAME program accepts when the bit is present and Pure — proving the rejection above is the
    // effect seed, not an unrelated error.
    let mut m = HashMap::new();
    m.insert("geom$noisy".to_string(), FnEffect::Pure);
    assert!(!check_with_effects(&m), "a Pure cross-unit effect bit must be accepted at par_map");
}

#[test]
fn comparator_import_effects_fail_closed_at_enclosing_parallel() {
    let geom = parse("pub fn compare(a:i64,b:i64)->bool = a < b\n");
    let main = parse("import geom\nfn work(n:i64)->i64 { values:=[n,0].sort_by(geom.compare); return values[0] }\nfn main() { print([1,2].par_map(work).sum()) }\n");
    let modules = [
        Module { path: "geom".into(), file: &geom, is_entry: false, interface_only: true },
        Module { path: "main".into(), file: &main, is_entry: true, interface_only: false },
    ];
    for effect in [None, Some(FnEffect::Unknown), Some(FnEffect::Impure), Some(FnEffect::Pure)] {
        let external = effect.map(|e| ("geom$compare".to_string(), e)).into_iter().collect();
        let mut diagnostics = Diagnostics::new();
        check_program_with_effects(&modules, &external, &mut diagnostics);
        assert_eq!(diagnostics.has_errors(), effect != Some(FnEffect::Pure), "{effect:?}: {:?}", diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
    }
}
