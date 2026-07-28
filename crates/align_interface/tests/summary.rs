//! M15 S1a gate tests: the interface summary's determinism, the interface/impl hash split, canonical
//! round-trip (+ fail-closed versioning), `out[i]` markers, and per-unit capability capture.

use std::collections::HashMap;

use align_interface::{
    build_summaries, deserialize, encode_interface_surface, serialize, validate_for_import,
    DecodeError, Effect, Hash128, IType, ImportCompatibilityError, InterfaceSummary, ParamMode,
    ReturnBorrowSummary, ReturnRegionSummary, FORMAT_VERSION,
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
        .map(|(u, ast)| align_sema::Module { path: u.path.to_string(), file: ast, is_entry: u.is_entry, interface_only: false })
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

fn rehash(summary: &mut InterfaceSummary) {
    summary.interface_hash = Hash128::of(&encode_interface_surface(summary));
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

// ---- 2b. aggregate constants (S1) ---------------------------------------------------------------

#[test]
fn an_aggregate_constant_exports_its_literal_source() {
    // A `pub` aggregate constant is part of the exported surface; its value is carried as the array
    // literal's source text (consumers rematerialize the slice against their own per-unit rodata).
    let sums = one("pub TABLE: slice<i64> := [1, 2, 3]\nfn main() -> i32 = 0\n");
    let c = &find(&sums, "main").consts[0];
    assert_eq!(c.name, "TABLE");
    assert_eq!(c.value_src, "[1, 2, 3]");
}

#[test]
fn editing_an_aggregate_constant_value_changes_the_interface_hash() {
    // The initializer source is folded into the interface hash (`IConst.value_src`), so changing an
    // element invalidates every dependent unit — the M15 cross-unit cache gate, no FORMAT_VERSION bump.
    let v1 = one("pub TABLE := [1, 2, 3]\nfn main() -> i32 = 0\n");
    let v2 = one("pub TABLE := [1, 2, 4]\nfn main() -> i32 = 0\n");
    assert_ne!(
        find(&v1, "main").interface_hash,
        find(&v2, "main").interface_hash,
        "editing an aggregate constant's value must change the interface hash"
    );
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

#[test]
fn deserialize_stale_effect_bit_fails_closed() {
    let sums = one("pub fn noisy(x: i64) -> i64 {\n  print(x)\n  return x\n}\nfn main() -> i32 = 0\n");
    let mut stale = sums[0].clone();
    let noisy = stale.fns.iter_mut().find(|f| f.name == "noisy").expect("pub fn present");
    assert_eq!(noisy.effect, Effect::Impure);

    // Simulate a stale/tampered artifact whose effect byte says Pure while its stored surface hash
    // still describes the original Impure interface. A consumer must reject it before effect seeding.
    noisy.effect = Effect::Pure;
    let bytes = serialize(&stale);
    assert_eq!(deserialize(&bytes), Err(DecodeError::InterfaceHashMismatch));
}

// ---- 4. out[i] markers --------------------------------------------------------------------------

#[test]
fn out_param_marker_is_recorded() {
    let sums = one("pub fn put(out dst: slice<i64>, k: i64) {\n  dst[k] = 42\n}\nfn main() -> i32 = 0\n");
    let put = find(&sums, "main").fns.iter().find(|f| f.name == "put").unwrap();
    assert_eq!(put.params.len(), 2);
    assert_eq!(put.params[0].mode, ParamMode::Out);
    assert_eq!(put.params[1].mode, ParamMode::ByValue);
    assert_eq!(put.return_borrow, ReturnBorrowSummary::None);
    assert_eq!(put.return_region, ReturnRegionSummary::None);
    // And the out-param's type survived.
    assert!(matches!(&put.params[0].ty, IType::Named { path, .. } if path == "slice"));
}

#[test]
fn parameter_mode_and_return_summaries_have_canonical_codec_identity() {
    let base = one("pub fn inspect(value: slice<i64>) -> i64 = value.len()\nfn main() -> i32 = 0\n");
    let mut mode = base[0].clone();
    mode.fns[0].params[0].mode = ParamMode::Out;
    rehash(&mut mode);
    assert_ne!(base[0].interface_hash, mode.interface_hash, "parameter mode is hash identity");

    let mut roots = base[0].clone();
    roots.fns[0].return_borrow =
        ReturnBorrowSummary::Roots { params: vec![0], captures: vec![] };
    rehash(&mut roots);
    assert_ne!(base[0].interface_hash, roots.interface_hash, "return-borrow roots are hash identity");
    assert_ne!(mode.interface_hash, roots.interface_hash, "mode and summary encode independently");
    let decoded = deserialize(&serialize(&roots)).expect("known canonical summary tag round-trips");
    assert_eq!(decoded, roots);
    assert_eq!(
        validate_for_import(&decoded),
        Err(ImportCompatibilityError::UnsupportedReturnBorrow),
        "codec recognition must not enable L2b semantics"
    );
}

#[test]
fn known_future_parameter_mode_round_trips_but_semantic_import_rejects() {
    for mode in [ParamMode::Borrow, ParamMode::BorrowMut] {
        let mut summary =
            one("pub fn inspect(value: slice<i64>) -> i64 = value.len()\nfn main() -> i32 = 0\n").remove(0);
        summary.fns[0].params[0].mode = mode;
        rehash(&mut summary);
        let decoded = deserialize(&serialize(&summary)).expect("known future mode tag round-trips");
        assert_eq!(decoded, summary);
        assert_eq!(
            validate_for_import(&decoded),
            Err(ImportCompatibilityError::UnsupportedParamMode(mode))
        );
    }
}

#[test]
fn malformed_return_roots_fail_closed_during_decode() {
    let mut duplicate =
        one("pub fn inspect(value: slice<i64>) -> i64 = value.len()\nfn main() -> i32 = 0\n").remove(0);
    duplicate.fns[0].return_region =
        ReturnRegionSummary::Roots { params: vec![0, 0], captures: vec![] };
    rehash(&mut duplicate);
    assert_eq!(
        deserialize(&serialize(&duplicate)),
        Err(DecodeError::InvalidSummary("parameter roots must be strictly increasing"))
    );

    let mut out_of_range = duplicate.clone();
    out_of_range.fns[0].return_region =
        ReturnRegionSummary::Roots { params: vec![1], captures: vec![] };
    rehash(&mut out_of_range);
    assert_eq!(
        deserialize(&serialize(&out_of_range)),
        Err(DecodeError::InvalidSummary("parameter root is outside the signature"))
    );

    let mut capture = duplicate;
    capture.fns[0].return_region =
        ReturnRegionSummary::Roots { params: vec![], captures: vec![0] };
    rehash(&mut capture);
    assert_eq!(
        deserialize(&serialize(&capture)),
        Err(DecodeError::InvalidSummary(
            "capture roots are forbidden in exported interfaces"
        ))
    );

    let mut empty = capture;
    empty.fns[0].return_region =
        ReturnRegionSummary::Roots { params: vec![], captures: vec![] };
    rehash(&mut empty);
    assert_eq!(
        deserialize(&serialize(&empty)),
        Err(DecodeError::InvalidSummary(
            "an empty root set must use the canonical None tag"
        ))
    );
}

#[test]
fn parameter_mode_codec_has_a_byte_golden_and_rejects_unknown_tags() {
    let summary =
        one("pub fn inspect(out value: slice<i64>) -> i64 = value.len()\nfn main() -> i32 = 0\n").remove(0);
    let surface = encode_interface_surface(&summary);
    let hex = surface.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    assert_eq!(
        hex,
        "02000000040000006d61696e0100000007000000696e73706563740000000001000000010005000000736c6963650100000000030000006936340000000000030000006936340000000000000000000000000000000000000000"
    );

    let mut artifact = serialize(&summary);
    let mode_offset = 4 // format
        + 4 + summary.unit.len()
        + 4 // fn sequence
        + 4 + summary.fns[0].name.len()
        + 4 // type-parameter sequence
        + 4; // parameter sequence
    artifact[mode_offset] = 0xff;
    assert_eq!(
        deserialize(&artifact),
        Err(DecodeError::BadTag { what: "parameter mode", tag: 0xff })
    );

    // This one-function surface ends with the function's borrow tag, region tag, effect, generic
    // body option, then the three empty top-level type/const sequences (16 bytes total).
    let mut bad_borrow = serialize(&summary);
    bad_borrow[surface.len() - 16] = 0xff;
    assert_eq!(
        deserialize(&bad_borrow),
        Err(DecodeError::BadTag { what: "return-borrow summary", tag: 0xff })
    );
    let mut bad_region = serialize(&summary);
    bad_region[surface.len() - 15] = 0xff;
    assert_eq!(
        deserialize(&bad_region),
        Err(DecodeError::BadTag { what: "return-region summary", tag: 0xff })
    );
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
