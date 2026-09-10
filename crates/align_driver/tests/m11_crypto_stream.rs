//! Incremental SHA-256: native values, exclusive borrowing, ownership and interface parity.

mod common;
use common::*;

#[test]
fn stream_vectors_and_partitions() {
    let source = r#"import std.crypto
import std.encoding
fn digest_at(data: str, split: i64) -> string {
  digest := crypto.sha256_stream()
  digest.update(data[0..split])
  digest.update("")
  digest.update(data[split..])
  result := digest.finish()
  return encoding.hex_encode(result[..])
}
fn arena_finish() -> array<u8> = arena {
  d := crypto.sha256_stream()
  d.update("abc")
  d.finish()
}
fn early() -> i64 {
  d := crypto.sha256_stream()
  d.update({ return 7; "" })
  return 9
}
fn main() -> i32 {
  if early() != 7 { return 3 }
  arena_bytes := arena_finish()
  arena_expected := crypto.sha256("abc")
  if !crypto.constant_time_equal(arena_bytes[..], arena_expected[..]) { return 4 }
  empty := crypto.sha256_stream()
  empty_result := empty.finish()
  print(encoding.hex_encode(empty_result[..]))
  print(digest_at("abc", 1))
  data := "0123456789012345678901234567890123456789012345678901234567890123456"
  expected := crypto.sha256(data)
  expected_hex := encoding.hex_encode(expected[..])
  mut split: i64 := 0
  loop {
    if split > data.len() { break }
    if digest_at(data, split) != expected_hex { return 1 }
    split = split + 1
  }
  binary := encoding.hex_decode("00ff0041") else { return 2 }
  binary_digest := crypto.sha256_stream()
  binary_digest.update(binary.bytes())
  binary_result := binary_digest.finish()
  binary_expected := crypto.sha256(binary.bytes())
  print(crypto.constant_time_equal(binary_result[..], binary_expected[..]))
  return 0
}
"#;
    let checked = diff_check_multi("digest-vectors", &[("main.align", source)], "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole: {}\nunit: {}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let out = build_and_run("digest-vectors", source);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\nba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\ntrue\n"
        );
    }
}

#[test]
fn stream_borrowed_inputs_and_interfaces() {
    let files = &[
        (
            "digest_api.align",
            r#"module digest_api
import std.crypto
pub Holder { digest: crypto.digest }
pub fn start() -> Holder = Holder { digest: crypto.sha256_stream() }
pub fn add(borrow mut digest: crypto.digest, input: str) { digest.update(input) }
pub fn finish(digest: crypto.digest) -> array<u8> = digest.finish()
"#,
        ),
        (
            "main.align",
            r#"import std.crypto
import std.encoding
import digest_api
fn main() -> i32 {
  holder := digest_api.start()
  mut digest := holder.digest
  arena {
    text := "abc".clone()
    add: fn(borrow mut crypto.digest, str) -> () := digest_api.add
    add(digest, text)
  }
  bytes := digest_api.finish(digest)
  print(encoding.hex_encode(bytes[..]))
  return 0
}
"#,
        ),
    ];
    let checked = diff_check_multi("digest-interface", files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole: {}\nunit: {}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let out = build_per_unit_multi("digest-interface", files, "main.align").link_and_run();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
        );
    }
}

#[test]
fn stream_formation_and_carriers() {
    for (name, body) in [
        (
            "array",
            "fn bad(value: array<crypto.digest>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "slice",
            "fn bad(value: slice<crypto.digest>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "box",
            "fn bad(value: box<crypto.digest>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "tuple",
            "fn bad(value: (crypto.digest, i64)) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "out",
            "fn bad(out value: crypto.digest) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "fixed-array",
            "fn bad(key: crypto.digest) { values := [key] }\nfn main() -> i32 = 0\n",
        ),
        (
            "array-builder",
            "fn bad(value: array_builder<crypto.digest>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "sum-array",
            "KeyChoice { Present(crypto.digest), Empty }\nfn bad(value: array<KeyChoice>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "tagged-array",
            "fn bad(value: array<Option<crypto.digest>>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "tagged-builder",
            "fn bad(value: array_builder<Option<crypto.digest>>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "sum-builder",
            "KeyChoice { Present(crypto.digest), Empty }\nfn bad(value: array_builder<KeyChoice>) -> i32 = 0\nfn main() -> i32 = 0\n",
        ),
        (
            "constant",
            "KEY: crypto.digest := 0\nfn main() -> i32 = 0\n",
        ),
        (
            "generic-tuple",
            "fn pair<T>(value: T) -> (T, i64) = (value, 0)\nfn bad(key: crypto.digest) { value := pair(key) }\nfn main() -> i32 = 0\n",
        ),
        (
            "layout-c",
            "layout(C) Bad { key: crypto.digest }\nfn main() -> i32 = 0\n",
        ),
        (
            "extern",
            "extern \"C\" fn expose(key: crypto.digest) -> i32\nfn main() -> i32 = 0\n",
        ),
        (
            "print",
            "fn bad(key: crypto.digest) { print(key) }\nfn main() -> i32 = 0\n",
        ),
        (
            "equality",
            "fn bad(a: crypto.digest, b: crypto.digest) -> bool = a == b\nfn main() -> i32 = 0\n",
        ),
        (
            "eager-replace",
            "fn main() { mut d := crypto.sha256_stream(); d.update({ d = crypto.sha256_stream(); \"a\" }) }",
        ),
        (
            "eager-consume",
            "fn main() { d := crypto.sha256_stream(); d.update({ _ := d.finish(); \"a\" }) }",
        ),
        ("new-arity", "fn main() { d := crypto.sha256_stream(1) }"),
        (
            "update-arity",
            "fn main() { d := crypto.sha256_stream(); d.update() }",
        ),
        (
            "direct-array",
            "fn main() { d := crypto.sha256_stream(); a := crypto.sha256(\"a\"); d.update(a) }",
        ),
        (
            "update-data",
            "fn main() { d := crypto.sha256_stream(); d.update(7) }",
        ),
        (
            "finish-arity",
            "fn main() { d := crypto.sha256_stream(); _ := d.finish(7) }",
        ),
        (
            "shared-update",
            "fn bad(borrow d: crypto.digest) { d.update(\"a\") }\nfn main() {}",
        ),
        (
            "shared-finish",
            "fn bad(borrow d: crypto.digest) -> array<u8> = d.finish()\nfn main() {}",
        ),
        (
            "exclusive-finish",
            "fn bad(borrow mut d: crypto.digest) -> array<u8> = d.finish()\nfn main() {}",
        ),
        (
            "double-finish",
            "fn main() { d := crypto.sha256_stream(); a := d.finish(); b := d.finish() }",
        ),
        (
            "update-finished",
            "fn main() { d := crypto.sha256_stream(); a := d.finish(); d.update(\"a\") }",
        ),
        (
            "unbound-update",
            "fn main() { crypto.sha256_stream().update(\"a\") }",
        ),
        (
            "unbound-finish",
            "fn main() { _ := crypto.sha256_stream().finish() }",
        ),
    ] {
        let source = format!("import std.crypto\n{body}\n");
        let checked = diff_check_multi(
            &format!("digest-{name}"),
            &[("main.align", &source)],
            "main.align",
        );
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{name}: whole={} {}\nunit={} {}",
            checked.whole_errors,
            checked.whole_diags,
            checked.per_unit_errors,
            checked.per_unit_diags
        );
    }
}

const DIGEST_OWNERSHIP_HELPER: &str = "module helper
import std.crypto
pub KeyHolder { key: crypto.digest, tag: i64 }
pub KeyChoice { Present(crypto.digest), Empty }
fn make(pem: str) -> Result<crypto.digest, Error> = Ok(crypto.sha256_stream())
fn pem_for_test() -> str = \"abc\"
fn consume(key: crypto.digest) -> Result<(), Error> {
  key.update(pem_for_test())
  digest := key.finish()
  if digest.len() != 32 { return Err(error(91)) }
  return Ok(())
}
fn identity<T>(value: T) -> T = value
fn pass(key: crypto.digest) -> crypto.digest = key
fn through_function(key: crypto.digest) -> crypto.digest {
  function: fn(crypto.digest) -> crypto.digest := pass
  return function(key)
}
fn optional(pem: str, present: bool) -> Result<Option<crypto.digest>, Error> {
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

  result: Result<crypto.digest, Error> := Ok(make(pem)?)
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
fn stream_owned_control_flow() {
    let main = "import helper\nfn main() -> Result<(), Error> = helper.exercise(\"unused\")\n";
    let files = &[
        ("helper.align", DIGEST_OWNERSHIP_HELPER),
        ("main.align", main),
    ];
    let checked = diff_check_multi("digest-owned", files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole: {}\nunit: {}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let whole = build_and_run_multi("digest-owned-whole", files, "main.align");
        assert!(
            whole.status.success(),
            "{}",
            String::from_utf8_lossy(&whole.stderr)
        );
        let out = build_per_unit_multi("digest-owned", files, "main.align").link_and_run();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
