//! M15 S1a gate tests: the interface summary's determinism, the interface/impl hash split, canonical
//! round-trip (+ fail-closed versioning), `out[i]` markers, and per-unit capability capture.

use std::collections::HashMap;

use align_interface::{
    build_summaries, deserialize, serialize, DecodeError, IType, InterfaceSummary, FORMAT_VERSION,
};

/// One in-memory source module for a test program.
struct Unit {
    path: &'static str,
    is_entry: bool,
    src: String,
}

fn unit(path: &'static str, is_entry: bool, src: impl Into<String>) -> Unit {
    Unit { path, is_entry, src: src.into() }
}

/// Parse + check + lower the given units and build their interface summaries. Asserts the program
/// type-checks (a summary of an ill-typed program is meaningless). Fresh data structures every call,
/// so building twice exercises determinism against any internal HashMap iteration order.
fn summaries(units: &[Unit]) -> Vec<InterfaceSummary> {
    let mut diags = align_diag::Diagnostics::new();
    let asts: Vec<align_ast::File> = units
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let toks = align_lexer::tokenize(i as u32, &u.src, &mut diags);
            align_parser::parse_file(toks, &mut diags)
        })
        .collect();
    let modules: Vec<align_sema::Module> = units
        .iter()
        .zip(&asts)
        .map(|(u, ast)| align_sema::Module { path: u.path.to_string(), file: ast, is_entry: u.is_entry })
        .collect();
    let hir = align_sema::check_program(&modules, &mut diags);
    assert!(!diags.has_errors(), "program should type-check");
    let mir = align_mir::lower_program(&hir);
    let sources: HashMap<String, String> =
        units.iter().map(|u| (u.path.to_string(), u.src.clone())).collect();
    build_summaries(&modules, &hir, &mir, &sources)
}

/// A single-entry-module program.
fn one(src: impl Into<String>) -> Vec<InterfaceSummary> {
    summaries(&[unit("main", true, src)])
}

fn find<'a>(sums: &'a [InterfaceSummary], unit: &str) -> &'a InterfaceSummary {
    sums.iter().find(|s| s.unit == unit).unwrap_or_else(|| panic!("no unit `{unit}`"))
}

// ---- 1. determinism -----------------------------------------------------------------------------

#[test]
fn determinism_same_source_twice_is_byte_identical() {
    let src = "pub fn add(a: i64, b: i64) -> i64 = a + b\n\
               pub fn shout(s: str) { print(s) }\n\
               pub Point { x: i64, y: i64 }\n\
               pub MAX: i64 := 100\n\
               fn main() -> i32 = 0\n";
    let a = one(src);
    let b = one(src);
    assert_eq!(a, b, "summaries must be equal across builds");
    assert_eq!(serialize(&a[0]), serialize(&b[0]), "serialization must be byte-identical");
}

#[test]
fn interface_is_independent_of_pub_fn_declaration_order() {
    // Reordering two unrelated `pub` fns is NOT an interface change (the exported set is order-free).
    let ab = one("pub fn a() -> i64 = 1\npub fn b() -> i64 = 2\nfn main() -> i32 = 0\n");
    let ba = one("pub fn b() -> i64 = 2\npub fn a() -> i64 = 1\nfn main() -> i32 = 0\n");
    assert_eq!(
        find(&ab, "main").interface_hash,
        find(&ba, "main").interface_hash,
        "reordering pub fns must not change the interface hash"
    );
    // The exported fns come out name-sorted (canonicalization pin).
    let names: Vec<&str> = find(&ab, "main").fns.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b"]);
}

// ---- 2. hash-split semantics (the headline) -----------------------------------------------------

/// A two-module program: entry `main` imports `lib` and calls only its STABLE fn, so edits to `lib`'s
/// other items never break `main`'s check. `{TARGET}` / `{GEN}` / `{SECRET}` are the edit points.
fn two_module(target: &str, generic: &str, secret: &str) -> Vec<InterfaceSummary> {
    let main = "import lib\nfn main() -> i32 {\n  x := lib.stable()\n  return x as i32\n}\n";
    let lib = format!(
        "module lib\n\
         pub fn stable() -> i64 = 7\n\
         pub fn target(a: i64, b: i64) -> i64 = {target}\n\
         pub fn pick<T>(a: T, b: T) -> T = {generic}\n\
         fn secret(x: i64) -> i64 = {secret}\n"
    );
    summaries(&[unit("main", true, main), unit("lib", false, lib)])
}

#[test]
fn split_a_private_body_edit_keeps_interface_changes_impl() {
    let v1 = two_module("a + b", "a", "x * 2");
    let v2 = two_module("a + b + 0", "a", "x * 2"); // target body differs, same sig + effect
    let l1 = find(&v1, "lib");
    let l2 = find(&v2, "lib");
    assert_eq!(l1.interface_hash, l2.interface_hash, "body edit must NOT change interface hash");
    assert_ne!(l1.impl_hash, l2.impl_hash, "body edit MUST change impl hash");
    // Headline win: the dependent unit's interface is entirely untouched.
    assert_eq!(find(&v1, "main").interface_hash, find(&v2, "main").interface_hash);
}

#[test]
fn split_b_signature_edit_changes_interface() {
    let v1 = two_module("a + b", "a", "x * 2");
    // Change `target`'s signature (a param type).
    let main = "import lib\nfn main() -> i32 {\n  x := lib.stable()\n  return x as i32\n}\n";
    let lib = "module lib\n\
               pub fn stable() -> i64 = 7\n\
               pub fn target(a: i32, b: i64) -> i64 = a as i64 + b\n\
               pub fn pick<T>(a: T, b: T) -> T = a\n\
               fn secret(x: i64) -> i64 = x * 2\n";
    let v2 = summaries(&[unit("main", true, main), unit("lib", false, lib)]);
    assert_ne!(
        find(&v1, "lib").interface_hash,
        find(&v2, "lib").interface_hash,
        "a signature change must change the interface hash"
    );
}

#[test]
fn split_c_effect_flip_pure_to_impure_changes_interface() {
    // The most-likely-to-get-wrong case: adding a side effect to a pub fn's body flips its effect
    // bit, which lives IN the interface.
    let v1 = two_module("a + b", "a", "x * 2");
    let main = "import lib\nfn main() -> i32 {\n  x := lib.stable()\n  return x as i32\n}\n";
    let lib = "module lib\n\
               pub fn stable() -> i64 = 7\n\
               pub fn target(a: i64, b: i64) -> i64 {\n  print(a)\n  return a + b\n}\n\
               pub fn pick<T>(a: T, b: T) -> T = a\n\
               fn secret(x: i64) -> i64 = x * 2\n";
    let v2 = summaries(&[unit("main", true, main), unit("lib", false, lib)]);
    // Sanity: the effect bit really flipped.
    let e1 = find(&v1, "lib").fns.iter().find(|f| f.name == "target").unwrap().effect;
    let e2 = find(&v2, "lib").fns.iter().find(|f| f.name == "target").unwrap().effect;
    assert_eq!(e1, align_interface::Effect::Pure);
    assert_eq!(e2, align_interface::Effect::Impure);
    assert_ne!(
        find(&v1, "lib").interface_hash,
        find(&v2, "lib").interface_hash,
        "a Pure->Impure effect flip must change the interface hash"
    );
}

#[test]
fn split_d_generic_template_body_edit_changes_interface() {
    // A generic pub fn's body is part of its interface (C++-template-like).
    let v1 = two_module("a + b", "a", "x * 2"); // pick<T> body = `a`
    let v2 = two_module("a + b", "b", "x * 2"); // pick<T> body = `b` (still type-correct)
    assert_ne!(
        find(&v1, "lib").interface_hash,
        find(&v2, "lib").interface_hash,
        "editing a generic template body must change the interface hash"
    );
}

#[test]
fn split_e_private_fn_edit_does_not_change_interface() {
    let v1 = two_module("a + b", "a", "x * 2");
    let v2 = two_module("a + b", "a", "x * 3"); // only `secret` (private) differs
    assert_eq!(
        find(&v1, "lib").interface_hash,
        find(&v2, "lib").interface_hash,
        "editing a private fn must NOT change the interface hash"
    );
    assert_ne!(find(&v1, "lib").impl_hash, find(&v2, "lib").impl_hash, "impl hash still changes");
}

// ---- 3. round-trip + fail-closed version --------------------------------------------------------

#[test]
fn round_trip_equality() {
    let sums = one("pub fn add(a: i64, b: i64) -> i64 = a + b\n\
                    pub fn shout(s: str) { print(s) }\n\
                    pub fn pick<T>(a: T, b: T) -> T = a\n\
                    pub Point { x: i64, y: i64 }\n\
                    pub layout(C) align(16) Wide { a: i64, b: i64 }\n\
                    pub Color { Red, Green, Blue }\n\
                    pub MAX: i64 := 100\n\
                    fn main() -> i32 = 0\n");
    let s = &sums[0];
    let bytes = serialize(s);
    let back = deserialize(&bytes).expect("round-trip should succeed");
    assert_eq!(*s, back);
}

#[test]
fn deserialize_unknown_version_fails_closed() {
    let sums = one("pub fn f() -> i64 = 1\nfn main() -> i32 = 0\n");
    let mut bytes = serialize(&sums[0]);
    // Corrupt the leading format-version u32.
    bytes[0] = bytes[0].wrapping_add(7);
    match deserialize(&bytes) {
        Err(DecodeError::UnknownVersion(v)) => assert_ne!(v, FORMAT_VERSION),
        other => panic!("expected UnknownVersion, got {other:?}"),
    }
}

#[test]
fn deserialize_truncated_and_trailing_fail_closed() {
    let sums = one("pub fn f() -> i64 = 1\nfn main() -> i32 = 0\n");
    let bytes = serialize(&sums[0]);
    // Truncated.
    assert_eq!(deserialize(&bytes[..bytes.len() - 3]), Err(DecodeError::Truncated));
    // Trailing bytes.
    let mut extra = bytes.clone();
    extra.push(0);
    assert_eq!(deserialize(&extra), Err(DecodeError::TrailingBytes));
}

// ---- 4. out[i] markers --------------------------------------------------------------------------

#[test]
fn out_param_marker_is_recorded() {
    let sums = one("pub fn put(out dst: slice<i64>, k: i64) {\n  dst[k] = 42\n}\nfn main() -> i32 = 0\n");
    let put = find(&sums, "main").fns.iter().find(|f| f.name == "put").unwrap();
    assert_eq!(put.params.len(), 2);
    assert!(put.params[0].is_out, "first param is `out`");
    assert!(!put.params[1].is_out, "second param is not `out`");
    // And the out-param's type survived.
    assert!(matches!(&put.params[0].ty, IType::Named { path, .. } if path == "slice"));
}

// ---- 5. capability set ---------------------------------------------------------------------------

#[test]
fn capabilities_captured_per_unit() {
    let main = "import zip\nfn main() -> Result<(), Error> {\n  n := zip.csize(\"hello\")?\n  print(n)\n  return Ok(())\n}\n";
    let zip = "module zip\nimport std.compress\n\
               pub fn csize(s: str) -> Result<i64, Error> {\n\
               \x20 c := compress.gzip_compress(s, 6)?\n\
               \x20 return Ok(c.len())\n}\n";
    let sums = summaries(&[unit("main", true, main), unit("zip", false, zip)]);
    assert_eq!(find(&sums, "zip").capabilities, vec!["Zlib".to_string()], "compress unit shows Zlib");
    assert!(find(&sums, "main").capabilities.is_empty(), "pure-numeric entry unit has no capabilities");
}
