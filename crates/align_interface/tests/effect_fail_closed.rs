//! M15 S1b gate 3 (fail-closed effect bits): a cross-unit `par_map` callee whose effect is ABSENT
//! from `external_effects` (a corrupted / truncated summary that dropped the bit) must be rejected —
//! never optimistically Pure. This exercises the `None` branch of sema's cross-unit effect seeding
//! directly, which the driver's `check_per_unit` (which always builds a complete effect map) cannot
//! reach. The contrast test shows the SAME program accepts when the bit is present and Pure.

use std::collections::HashMap;

use align_diag::Diagnostics;
use align_sema::{check_program_with_effects, FnEffect, Module};

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
