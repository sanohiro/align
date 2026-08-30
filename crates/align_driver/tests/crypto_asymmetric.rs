//! Post-pkg.db algorithm-specific signature owners.

mod common;
use common::*;

const ED25519_PRIVATE_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEIJ1hsZ3v/VpguoRK9JLsLMREScVpezJpGXA7rAMcrn9g\n-----END PRIVATE KEY-----\n";
const ED25519_PUBLIC_PEM: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=\n-----END PUBLIC KEY-----\n";

fn align_string(value: &str) -> String {
    let mut rendered = String::from("\"");
    for byte in value.bytes() {
        match byte {
            b'\\' => rendered.push_str("\\\\"),
            b'\"' => rendered.push_str("\\\""),
            b'\n' => rendered.push_str("\\n"),
            b'\r' => rendered.push_str("\\r"),
            0x20..=0x7e => rendered.push(char::from(byte)),
            _ => panic!("fixture contains a non-ASCII byte"),
        }
    }
    rendered.push('\"');
    rendered
}

fn ed25519_program() -> String {
    format!(
        "import std.crypto\n\
         pub fn main() -> Result<(), Error> {{\n\
           private := crypto.ed25519_private_key_from_pem({})?\n\
           public := crypto.ed25519_public_key_from_pem({})?\n\
           signature := crypto.ed25519_sign(private, \"\")?\n\
           print(signature.len())\n\
           print(crypto.ed25519_verify(public, \"\", signature.bytes())?)\n\
           print(crypto.ed25519_verify(public, \"wrong\", signature.bytes())?)\n\
           again := crypto.ed25519_sign(private, \"again\")?\n\
           print(again.len())\n\
           return Ok(())\n\
         }}\n",
        align_string(ED25519_PRIVATE_PEM),
        align_string(ED25519_PUBLIC_PEM),
    )
}

#[test]
fn ed25519_compiles_signs_verifies_and_keeps_borrowed_keys_usable() {
    if !backend_available() {
        return;
    }
    let source = ed25519_program();
    let output = build_and_run("crypto-asymmetric-ed25519", &source);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "64\ntrue\nfalse\n64\n"
    );
}

#[test]
fn asymmetric_crypto_ignores_hostile_process_openssl_configuration() {
    if !backend_available() {
        return;
    }
    let output = build_and_run_with_env(
        "crypto-asymmetric-hostile-openssl-env",
        &ed25519_program(),
        &[
            ("OPENSSL_CONF", "/align/does/not/exist/openssl.cnf"),
            ("OPENSSL_MODULES", "/align/does/not/exist/providers"),
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "64\ntrue\nfalse\n64\n"
    );
}

#[test]
fn signature_key_types_are_move_owners_and_qualified_spellings_need_the_import() {
    let bare = "fn pass(key: ed25519_public_key) -> ed25519_public_key = key\n";
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "crypto-asymmetric-bare", bare);
    assert!(
        !checked.diags.has_errors(),
        "bare key types are the no-import builtin fallback: {}",
        align_driver::format_diagnostics(&sources, &checked.diags),
    );

    let qualified = "fn inspect(key: crypto.ed25519_public_key) -> i32 = 0\n";
    let diagnostics = check_diagnostics("crypto-asymmetric-qualified", qualified);
    assert!(
        diagnostics.contains("requires `import std.crypto`"),
        "{diagnostics}"
    );

    let operation = "fn main() -> Result<(), Error> {\n  key := crypto.ed25519_public_key_from_jwk(\"01234567890123456789012345678901\")?\n  return Ok(())\n}\n";
    let diagnostics = check_diagnostics("crypto-asymmetric-operation", operation);
    assert!(diagnostics.contains("import std.crypto"), "{diagnostics}");
}

#[test]
fn signature_operations_require_the_exact_kind_and_a_stable_borrow_place() {
    let wrong_kind = "import std.crypto\nfn bad(key: ed25519_public_key) -> Result<buffer, Error> {\n  return crypto.ed25519_sign(key, \"message\")\n}\n";
    let diagnostics = check_diagnostics("crypto-asymmetric-kind", wrong_kind);
    assert!(diagnostics.contains("ed25519_private_key"), "{diagnostics}");

    let unstable = format!(
        "import std.crypto\nfn bad() -> Result<buffer, Error> {{\n  return crypto.ed25519_sign(crypto.ed25519_private_key_from_pem({})?, \"message\")\n}}\n",
        align_string(ED25519_PRIVATE_PEM),
    );
    let diagnostics = check_diagnostics("crypto-asymmetric-stable", &unstable);
    assert!(diagnostics.contains("stable"), "{diagnostics}");
}

#[test]
fn every_asymmetric_surface_has_its_exact_static_key_kind() {
    let source = "import std.crypto
fn exercise(
  rs_private: rs256_private_key,
  rs_public: rs256_public_key,
  es_private: es256_private_key,
  es_public: es256_public_key,
  ed_private: ed25519_private_key,
  ed_public: ed25519_public_key,
  pem: str,
  bytes: slice<u8>,
) -> Result<(), Error> {
  made_rs_private := crypto.rs256_private_key_from_pem(pem)?
  made_rs_public_pem := crypto.rs256_public_key_from_pem(pem)?
  made_rs_public_jwk := crypto.rs256_public_key_from_jwk(bytes, bytes)?
  made_es_private := crypto.es256_private_key_from_pem(pem)?
  made_es_public_pem := crypto.es256_public_key_from_pem(pem)?
  made_es_public_jwk := crypto.es256_public_key_from_jwk(bytes, bytes)?
  made_ed_private := crypto.ed25519_private_key_from_pem(pem)?
  made_ed_public_pem := crypto.ed25519_public_key_from_pem(pem)?
  made_ed_public_jwk := crypto.ed25519_public_key_from_jwk(bytes)?
  rs_signature := crypto.rs256_sign(rs_private, bytes)?
  es_signature := crypto.es256_sign(es_private, bytes)?
  ed_signature := crypto.ed25519_sign(ed_private, bytes)?
  rs_ok := crypto.rs256_verify(rs_public, bytes, rs_signature.bytes())?
  es_ok := crypto.es256_verify(es_public, bytes, es_signature.bytes())?
  ed_ok := crypto.ed25519_verify(ed_public, bytes, ed_signature.bytes())?
  return Ok(())
}
fn main() -> i32 = 0
";
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "crypto-asymmetric-surface-matrix", source);
    assert!(
        !checked.diags.has_errors(),
        "all 15 asymmetric operations must retain their algorithm-specific key types: {}",
        align_driver::format_diagnostics(&sources, &checked.diags),
    );
}

#[test]
fn signature_key_forbidden_carriers_and_mutable_modes_fail_closed() {
    for (name, source, expected) in [
        (
            "array",
            "fn bad(value: array<ed25519_public_key>) -> i32 = 0\nfn main() -> i32 = 0\n",
            "single owner",
        ),
        (
            "slice",
            "fn bad(value: slice<ed25519_public_key>) -> i32 = 0\nfn main() -> i32 = 0\n",
            "single owner",
        ),
        (
            "box",
            "fn bad(value: box<ed25519_public_key>) -> i32 = 0\nfn main() -> i32 = 0\n",
            "single owner",
        ),
        (
            "tuple",
            "fn bad(value: (ed25519_public_key, i64)) -> i32 = 0\nfn main() -> i32 = 0\n",
            "tuple elements",
        ),
        (
            "out",
            "fn bad(out value: ed25519_public_key) -> i32 = 0\nfn main() -> i32 = 0\n",
            "never `out`",
        ),
        (
            "borrow-mut",
            "fn bad(borrow mut value: ed25519_public_key) -> i32 = 0\nfn main() -> i32 = 0\n",
            "borrow mut",
        ),
        (
            "generic-borrow-mut",
            "fn mutate<T>(borrow mut value: T) {}\nfn bad(key: ed25519_public_key) { mut value := key; mutate(value) }\nfn main() -> i32 = 0\n",
            "generic substitution cannot place a signature-key owner",
        ),
    ] {
        let diagnostics = check_diagnostics(&format!("crypto-asymmetric-carrier-{name}"), source);
        assert!(diagnostics.contains(expected), "{name}: {diagnostics}");
    }

    for (name, source) in [
        (
            "fixed-array",
            "fn bad(key: ed25519_public_key) { values := [key] }\nfn main() -> i32 = 0\n",
        ),
        (
            "array-builder",
            "fn bad(value: array_builder<ed25519_public_key>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "sum-array",
            "KeyChoice { Present(ed25519_public_key), Empty }\nfn bad(value: array<KeyChoice>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "tagged-array",
            "fn bad(value: array<Option<ed25519_public_key>>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "tagged-builder",
            "fn bad(value: array_builder<Option<ed25519_public_key>>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "sum-builder",
            "KeyChoice { Present(ed25519_public_key), Empty }\nfn bad(value: array_builder<KeyChoice>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "tagged-borrow-mut",
            "fn bad(borrow mut value: Option<ed25519_public_key>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "struct-borrow-mut",
            "Holder { key: ed25519_public_key }\nfn bad(borrow mut value: Holder) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "function-value-borrow-mut",
            "fn bad(callback: fn(borrow mut ed25519_public_key) -> i32) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "constant",
            "KEY: ed25519_public_key := 0\nfn main() -> i32 = 0\n",
        ),
        (
            "generic-tuple",
            "fn pair<T>(value: T) -> (T, i64) = (value, 0)\nfn bad(key: ed25519_public_key) { value := pair(key) }\nfn main() -> i32 = 0\n",
        ),
        (
            "layout-c",
            "layout(C) Bad { key: ed25519_public_key }\nfn main() -> i32 = 0\n",
        ),
        (
            "extern",
            "extern \"C\" fn expose(key: ed25519_public_key) -> i32\nfn main() -> i32 = 0\n",
        ),
        (
            "print",
            "fn bad(key: ed25519_public_key) { print(key) }\nfn main() -> i32 = 0\n",
        ),
        (
            "equality",
            "fn bad(a: ed25519_public_key, b: ed25519_public_key) -> bool = a == b\nfn main() -> i32 = 0\n",
        ),
        (
            "capture",
            "import std.crypto\nfn bad(key: ed25519_private_key) { f := fn unused: i64 { crypto.ed25519_sign(key, \"x\") } }\nfn main() -> i32 = 0\n",
        ),
        (
            "task-capture",
            "fn inspect(borrow key: ed25519_private_key) -> i64 = 0\nfn bad(key: ed25519_private_key) { task_group { t := spawn(fn { inspect(key) }); wait() } }\nfn main() -> i32 = 0\n",
        ),
        (
            "parallel-capture",
            "fn inspect(borrow key: ed25519_private_key) -> i64 = 0\nfn bad(key: ed25519_private_key) { values := [1, 2].par_map(fn x { inspect(key) + x }) }\nfn main() -> i32 = 0\n",
        ),
        (
            "order",
            "fn bad(a: ed25519_public_key, b: ed25519_public_key) -> bool = a < b\nfn main() -> i32 = 0\n",
        ),
        (
            "hash",
            "fn bad(key: ed25519_public_key) -> u64 = hash64(key)\nfn main() -> i32 = 0\n",
        ),
    ] {
        assert!(
            check_errs(&format!("crypto-asymmetric-carrier-{name}"), source),
            "{name} must reject a signature-key storage/observation edge",
        );
    }
}

#[test]
fn public_key_signatures_round_trip_through_per_unit_interfaces() {
    let helper = "module helper
import std.crypto
pub fn qualified_rs_private(key: crypto.rs256_private_key) -> crypto.rs256_private_key = key
pub fn qualified_rs_public(key: crypto.rs256_public_key) -> crypto.rs256_public_key = key
pub fn qualified_es_private(key: crypto.es256_private_key) -> crypto.es256_private_key = key
pub fn qualified_es_public(key: crypto.es256_public_key) -> crypto.es256_public_key = key
pub fn qualified_ed_private(key: crypto.ed25519_private_key) -> crypto.ed25519_private_key = key
pub fn qualified_ed_public(key: crypto.ed25519_public_key) -> crypto.ed25519_public_key = key
pub fn bare_rs_private(key: rs256_private_key) -> rs256_private_key = key
pub fn bare_rs_public(key: rs256_public_key) -> rs256_public_key = key
pub fn bare_es_private(key: es256_private_key) -> es256_private_key = key
pub fn bare_es_public(key: es256_public_key) -> es256_public_key = key
pub fn bare_ed_private(key: ed25519_private_key) -> ed25519_private_key = key
pub fn bare_ed_public(key: ed25519_public_key) -> ed25519_public_key = key
";
    let main = "module main
import helper
import std.crypto
fn qualified_rs_private(key: rs256_private_key) -> rs256_private_key = helper.qualified_rs_private(key)
fn qualified_rs_public(key: rs256_public_key) -> rs256_public_key = helper.qualified_rs_public(key)
fn qualified_es_private(key: es256_private_key) -> es256_private_key = helper.qualified_es_private(key)
fn qualified_es_public(key: es256_public_key) -> es256_public_key = helper.qualified_es_public(key)
fn qualified_ed_private(key: ed25519_private_key) -> ed25519_private_key = helper.qualified_ed_private(key)
fn qualified_ed_public(key: ed25519_public_key) -> ed25519_public_key = helper.qualified_ed_public(key)
fn bare_rs_private(key: rs256_private_key) -> rs256_private_key = helper.bare_rs_private(key)
fn bare_rs_public(key: rs256_public_key) -> rs256_public_key = helper.bare_rs_public(key)
fn bare_es_private(key: es256_private_key) -> es256_private_key = helper.bare_es_private(key)
fn bare_es_public(key: es256_public_key) -> es256_public_key = helper.bare_es_public(key)
fn bare_ed_private(key: ed25519_private_key) -> ed25519_private_key = helper.bare_ed_private(key)
fn bare_ed_public(key: ed25519_public_key) -> ed25519_public_key = helper.bare_ed_public(key)
fn main() -> i32 = 0
";
    let checked = check_per_unit_multi(
        "crypto-asymmetric-interface",
        &[("helper.align", helper), ("main.align", main)],
        "main.align",
    );
    assert!(
        !checked.diags.has_errors(),
        "all bare and qualified signature-key paths must retain their nominal Move identity through interface source reconstruction: {:?}",
        checked
            .diags
            .iter()
            .map(|diag| &diag.message)
            .collect::<Vec<_>>(),
    );
}

const OWNERSHIP_HELPER: &str = "module helper
import std.crypto
pub KeyHolder { key: ed25519_private_key, tag: i64 }
pub KeyChoice { Present(ed25519_private_key), Empty }
fn make(pem: str) -> Result<ed25519_private_key, Error> = crypto.ed25519_private_key_from_pem(pem)
fn consume(key: ed25519_private_key) -> Result<(), Error> {
  signature := crypto.ed25519_sign(key, \"ownership\")?
  if signature.len() != 64 { return Err(error(91)) }
  return Ok(())
}
fn identity<T>(value: T) -> T = value
fn pass(key: ed25519_private_key) -> ed25519_private_key = key
fn through_function(key: ed25519_private_key) -> ed25519_private_key {
  function: fn(ed25519_private_key) -> ed25519_private_key := pass
  return function(key)
}
fn optional(pem: str, present: bool) -> Result<Option<ed25519_private_key>, Error> {
  if present { return Ok(Some(make(pem)?)) }
  return Ok(None)
}
fn rows(pem: str) -> Result<array<KeyHolder>, Error> {
  mut values: array_builder<KeyHolder> := array_builder()
  values.push(KeyHolder { key: make(pem)?, tag: 3 })
  return Ok(values.build())
}
fn fail() -> Result<(), Error> = Err(error(7))
fn try_early(pem: str) -> Result<(), Error> {
  held := make(pem)?
  fail()?
  consume(held)?
  return Ok(())
}
fn keep_error(value: Error) -> Error = value
pub fn exercise(pem: str) -> Result<(), Error> {
  first := make(pem)?
  moved := first
  consume(moved)?

  returned := identity(make(pem)?)
  consume(returned)?
  indirect := through_function(make(pem)?)
  consume(indirect)?

  holder := KeyHolder { key: make(pem)?, tag: 7 }
  field := holder.key
  if holder.tag != 7 { return Err(error(92)) }
  consume(field)?
  fixed := [KeyHolder { key: make(pem)?, tag: 1 }, KeyHolder { key: make(pem)?, tag: 2 }]
  dynamic := rows(pem)?

  choice := KeyChoice.Present(make(pem)?)
  match choice {
    Present(key) => { consume(key)? }
    Empty => {}
  }
  maybe := optional(pem, true)?
  option_key := maybe else { return Err(error(93)) }
  consume(option_key)?

  result: Result<ed25519_private_key, Error> := Ok(make(pem)?)
  mapped := result.map_err(keep_error)
  consume(mapped?)?
  selected := if true { make(pem)? } else { make(pem)? }
  consume(selected)?

  mut owner := make(pem)?
  owner = make(pem)?
  mut done := false
  loop {
    if done { break }
    owner = make(pem)?
    done = true
  }
  consume(owner)?

  early := try_early(pem)
  match early {
    Ok(_) => { return Err(error(94)) }
    Err(_) => {}
  }
  return Ok(())
}
";

#[test]
fn signature_key_carrier_and_ownership_matrix_runs_whole_and_per_unit() {
    let main = format!(
        "module main\nimport helper\npub fn main() -> Result<(), Error> {{\n  helper.exercise({})?\n  return Ok(())\n}}\n",
        align_string(ED25519_PRIVATE_PEM),
    );
    let files = [
        ("helper.align", OWNERSHIP_HELPER),
        ("main.align", main.as_str()),
    ];
    let differential = diff_check_multi("crypto-asymmetric-ownership", &files, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags,
    );
    assert!(
        !differential.whole_errors,
        "ownership matrix must type-check:\n{}",
        differential.whole_diags,
    );
    if !backend_available() {
        return;
    }
    let whole = build_and_run_multi("crypto-asymmetric-ownership-whole", &files, "main.align");
    assert_eq!(
        whole.status.code(),
        Some(0),
        "whole stderr: {}",
        String::from_utf8_lossy(&whole.stderr),
    );
    let per_unit = build_per_unit_multi("crypto-asymmetric-ownership-units", &files, "main.align")
        .link_and_run();
    assert_eq!(
        per_unit.status.code(),
        Some(0),
        "per-unit stderr: {}",
        String::from_utf8_lossy(&per_unit.stderr),
    );
}

#[test]
fn signature_key_compilation_paths_keep_runtime_calls_in_raw_and_optimized_llvm() {
    if !backend_available() {
        return;
    }
    let source = ed25519_program();
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "crypto-asymmetric-llvm", &source);
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags),
    );
    let mir = lower_to_mir(&checked.hir);
    for (label, optimized) in [("raw", false), ("optimized", true)] {
        let ir = emit_llvm_ir(&mir, BuildTarget::Baseline, optimized, &[], false)
            .unwrap_or_else(|error| panic!("{label} LLVM emission failed: {error}"));
        for symbol in [
            "@align_rt_crypto_private_key_from_pem",
            "@align_rt_crypto_public_key_from_pem",
            "@align_rt_crypto_sign",
            "@align_rt_crypto_verify",
            "@align_rt_crypto_key_free",
        ] {
            assert!(ir.contains(symbol), "{label} LLVM omitted {symbol}:\n{ir}");
        }
    }
}
