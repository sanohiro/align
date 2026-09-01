//! `pkg.auth` owner tests: bounded HS256, canonical Argon2id PHC, session tokens, and ordinary
//! package/interface/capability behavior.

mod common;
use common::*;

fn auth_source() -> &'static str {
    fixture("apps/auth/pkg/auth.align")
}

fn auth_files(main: &str) -> [(&str, &str); 2] {
    [("pkg/auth.align", auth_source()), ("main.align", main)]
}

const KEY_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
const CLAIMS: &str = r#"{"sub":"123","exp":2}"#;
const TOKEN: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjMiLCJleHAiOjJ9.2knxxaZ2bNtC6CBQsecnvGl5Chu6kx6vrwgjYPttIqw";

#[test]
fn jwt_vector_and_whole_per_unit_execution_are_exact() {
    let claims = format!("{CLAIMS:?}");
    let main = format!(
        r#"module main
import pkg.auth
import std.encoding

fn main() -> Result<(), Error> {{
  key := encoding.hex_decode("{KEY_HEX}")?
  token := pkg.auth.encode_hs256({claims}, key.bytes())?
  print(token)
  verified := pkg.auth.verify_hs256(token, key.bytes(), 1000000000)?
  print(verified == {claims})
  return Ok(())
}}
"#
    );
    let files = auth_files(&main);
    let differential = diff_check_multi("pkg-auth-jwt-interface", &files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    if !backend_available() {
        return;
    }
    for (name, output) in [
        (
            "whole",
            build_and_run_multi("pkg-auth-jwt-whole", &files, "main.align"),
        ),
        (
            "per-unit",
            build_per_unit_multi("pkg-auth-jwt-per-unit", &files, "main.align").link_and_run(),
        ),
    ] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("{TOKEN}\ntrue\n"),
            "{name}",
        );
    }
}

#[test]
fn jwt_validation_order_and_error_classes_cover_strict_authenticated_products() {
    if !backend_available() {
        return;
    }
    let main = format!(
        r#"module main
import pkg.auth
import std.crypto
import std.encoding

fn error_class(result: Result<string, Error>) -> i64 = match result {{
  Ok(_) => 0,
  Err(error) => match error {{
    Invalid => 1,
    Denied => 2,
    NotFound => 3,
    Timeout => 4,
    Code(_) => 5,
  }},
}}

fn sign_raw(header: str, payload: str, key: slice<u8>) -> string {{
  h := encoding.base64url_encode(header)
  p := encoding.base64url_encode(payload)
  mut input := builder()
  input.write(h)
  input.write(".")
  input.write(p)
  signed := input.to_string()
  tag := crypto.hmac_sha256(key, signed)
  mut token := builder()
  token.write(signed)
  token.write(".")
  token.write(encoding.base64url_encode(tag[0..tag.len()]))
  return token.to_string()
}}

fn main() -> Result<(), Error> {{
  key := encoding.hex_decode("{KEY_HEX}")?
  short_key := encoding.hex_decode("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e")?
  header := "{{\"alg\":\"HS256\",\"typ\":\"JWT\"}}"

  print(error_class(pkg.auth.encode_hs256("{{}}", short_key.bytes())))
  print(error_class(pkg.auth.encode_hs256("", key.bytes())))
  print(error_class(pkg.auth.encode_hs256("{{\"x\":\"a\0b\"}}", key.bytes())))
  print(error_class(pkg.auth.encode_hs256("{{\"n\":007}}", key.bytes())))
  print(error_class(pkg.auth.encode_hs256("{{\"a\":1,\"\\u0061\":2}}", key.bytes())))

  print(error_class(pkg.auth.verify_hs256(sign_raw("{{", "{{}}", key.bytes()), key.bytes(), 0)))
  print(error_class(pkg.auth.verify_hs256(sign_raw("[]", "{{}}", key.bytes()), key.bytes(), 0)))
  print(error_class(pkg.auth.verify_hs256(sign_raw("{{\"alg\":\"HS256\",\"\\u0061lg\":\"HS256\"}}", "{{}}", key.bytes()), key.bytes(), 0)))
  print(error_class(pkg.auth.verify_hs256(sign_raw("{{\"alg\":\"none\"}}", "{{}}", key.bytes()), key.bytes(), 0)))
  print(error_class(pkg.auth.verify_hs256(sign_raw("{{\"alg\":\"HS256\",\"typ\":1}}", "{{}}", key.bytes()), key.bytes(), 0)))
  print(error_class(pkg.auth.verify_hs256(sign_raw("{{\"alg\":\"HS256\",\"crit\":null}}", "{{}}", key.bytes()), key.bytes(), 0)))

  print(error_class(pkg.auth.verify_hs256(sign_raw(header, "{{\"x\":\"a\0b\"}}", key.bytes()), key.bytes(), 0)))
  print(error_class(pkg.auth.verify_hs256(sign_raw(header, "{{\"n\":007}}", key.bytes()), key.bytes(), 0)))
  print(error_class(pkg.auth.verify_hs256(sign_raw(header, "{{\"a\":1,\"\\u0061\":2}}", key.bytes()), key.bytes(), 0)))
  print(error_class(pkg.auth.verify_hs256(sign_raw(header, "{{\"exp\":1.0}}", key.bytes()), key.bytes(), 0)))
  print(error_class(pkg.auth.verify_hs256(sign_raw(header, "{{\"exp\":1}}", key.bytes()), key.bytes(), 1000000000)))
  print(error_class(pkg.auth.verify_hs256(sign_raw(header, "{{\"nbf\":2}}", key.bytes()), key.bytes(), 1000000000)))
  print(error_class(pkg.auth.verify_hs256(sign_raw(header, "{{\"exp\":2,\"nbf\":1}}", key.bytes()), key.bytes(), 1000000000)))

  zeros := encoding.hex_decode("0000000000000000000000000000000000000000000000000000000000000000")?
  mut unauthenticated := builder()
  unauthenticated.write(encoding.base64url_encode(header))
  unauthenticated.write(".")
  unauthenticated.write(encoding.base64url_encode("{{"))
  unauthenticated.write(".")
  unauthenticated.write(encoding.base64url_encode(zeros.bytes()))
  print(error_class(pkg.auth.verify_hs256(unauthenticated.to_string(), key.bytes(), 0)))

  zero_tag := encoding.base64url_encode(zeros.bytes())
  mut invalid_alphabet := builder()
  invalid_alphabet.write(encoding.base64url_encode(header))
  invalid_alphabet.write(".!.")
  invalid_alphabet.write(zero_tag)
  print(error_class(pkg.auth.verify_hs256(invalid_alphabet.to_string(), key.bytes(), 0)))

  mut noncanonical_header := builder()
  noncanonical_header.write("Zh.")
  noncanonical_header.write(encoding.base64url_encode("{{}}"))
  noncanonical_header.write(".")
  noncanonical_header.write(zero_tag)
  print(error_class(pkg.auth.verify_hs256(noncanonical_header.to_string(), key.bytes(), 0)))

  mut noncanonical_payload := builder()
  noncanonical_payload.write(encoding.base64url_encode(header))
  noncanonical_payload.write(".Zh.")
  noncanonical_payload.write(zero_tag)
  print(error_class(pkg.auth.verify_hs256(noncanonical_payload.to_string(), key.bytes(), 0)))

  mut noncanonical_signature := builder()
  noncanonical_signature.write(encoding.base64url_encode(header))
  noncanonical_signature.write(".")
  noncanonical_signature.write(encoding.base64url_encode("{{}}"))
  noncanonical_signature.write(".")
  noncanonical_signature.write(zero_tag[0..zero_tag.len() - 1])
  noncanonical_signature.write("B")
  print(error_class(pkg.auth.verify_hs256(noncanonical_signature.to_string(), key.bytes(), 0)))

  signed := sign_raw(header, "{{}}", key.bytes())
  mut padded_signature := builder()
  padded_signature.write(signed)
  padded_signature.write("=")
  print(error_class(pkg.auth.verify_hs256(padded_signature.to_string(), key.bytes(), 0)))

  print(error_class(pkg.auth.verify_hs256("a.b.A", key.bytes(), 0)))
  print(error_class(pkg.auth.verify_hs256("a.b", key.bytes(), 0)))
  print(error_class(pkg.auth.verify_hs256(sign_raw(header, "{{}}", key.bytes()), key.bytes(), -1)))
  return Ok(())
}}
"#
    );
    let output = build_and_run_multi("pkg-auth-jwt-validation", &auth_files(&main), "main.align");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "1\n1\n1\n1\n1\n1\n1\n1\n2\n2\n2\n1\n1\n1\n1\n2\n2\n0\n2\n1\n1\n1\n1\n1\n1\n1\n1\n",
    );
}

#[test]
fn password_phc_vector_policy_and_canonical_mutation_matrix_are_exact() {
    if !backend_available() {
        return;
    }
    // OpenSSL 3.5 `EVP_KDF(ARGON2ID)` oracle: password="password", salt="0123456789abcdef",
    // m=65536 KiB, t=2, lanes=1, threads=1, output=32 bytes.
    let vector = "$argon2id$v=19$m=65536,t=2,p=1$MDEyMzQ1Njc4OWFiY2RlZg$7+/qEheh109gdg4M3e8OHdzFjQSPKlC6NgbVvwpor58";
    let main = format!(
        r#"module main
import pkg.auth

fn bool_result(result: Result<bool, Error>) -> i64 = match result {{
  Ok(value) => if value {{ 10 }} else {{ 11 }},
  Err(error) => match error {{ Invalid => 1, Denied => 2, NotFound => 3, Timeout => 4, Code(_) => 5 }},
}}

fn string_result(result: Result<string, Error>) -> i64 = match result {{
  Ok(_) => 10,
  Err(error) => match error {{ Invalid => 1, Denied => 2, NotFound => 3, Timeout => 4, Code(_) => 5 }},
}}

fn main() -> Result<(), Error> {{
  exact := pkg.auth.Argon2Policy{{m_cost: 65536, t_cost: 2, parallelism: 1}}
  print(bool_result(pkg.auth.password_verify("password".bytes(), "{vector}", exact)))
  print(bool_result(pkg.auth.password_verify("wrong".bytes(), "{vector}", exact)))
  print(bool_result(pkg.auth.password_verify("password".bytes(), "{vector}", pkg.auth.Argon2Policy{{m_cost: 65535, t_cost: 2, parallelism: 1}})))
  print(bool_result(pkg.auth.password_verify("password".bytes(), "{vector}", pkg.auth.Argon2Policy{{m_cost: 0, t_cost: 2, parallelism: 1}})))
  print(bool_result(pkg.auth.password_verify("password".bytes(), "$argon2i$v=19$m=65536,t=2,p=1$MDEyMzQ1Njc4OWFiY2RlZg$7+/qEheh109gdg4M3e8OHdzFjQSPKlC6NgbVvwpor58", exact)))
  print(bool_result(pkg.auth.password_verify("password".bytes(), "$argon2id$v=16$m=65536,t=2,p=1$MDEyMzQ1Njc4OWFiY2RlZg$7+/qEheh109gdg4M3e8OHdzFjQSPKlC6NgbVvwpor58", exact)))
  print(bool_result(pkg.auth.password_verify("password".bytes(), "$argon2id$v=19$t=2,m=65536,p=1$MDEyMzQ1Njc4OWFiY2RlZg$7+/qEheh109gdg4M3e8OHdzFjQSPKlC6NgbVvwpor58", exact)))
  print(bool_result(pkg.auth.password_verify("password".bytes(), "$argon2id$v=19$m=065536,t=2,p=1$MDEyMzQ1Njc4OWFiY2RlZg$7+/qEheh109gdg4M3e8OHdzFjQSPKlC6NgbVvwpor58", exact)))
  print(bool_result(pkg.auth.password_verify("password".bytes(), "$argon2id$v=19$m=7,t=2,p=1$MDEyMzQ1Njc4OWFiY2RlZg$7+/qEheh109gdg4M3e8OHdzFjQSPKlC6NgbVvwpor58", exact)))
  print(bool_result(pkg.auth.password_verify("password".bytes(), "$argon2id$v=19$m=65536,t=2,p=1$MDEyMzQ1Njc4OWFiY2RlZg==$7+/qEheh109gdg4M3e8OHdzFjQSPKlC6NgbVvwpor58", exact)))
  print(bool_result(pkg.auth.password_verify("password".bytes(), "$argon2id$v=19$m=65536,t=2,p=1$MDEyMzQ1Njc4OWFiY2RlZh$7+/qEheh109gdg4M3e8OHdzFjQSPKlC6NgbVvwpor58", exact)))
  print(bool_result(pkg.auth.password_verify("password".bytes(), "$argon2id$v=19$m=65536,t=2,p=1$MDEyMzQ1Njc4OWFiY2RlZg$7+/qEheh109gdg4M3e8OHdzFjQSPKlC6NgbVvwpor59", exact)))
  print(bool_result(pkg.auth.password_verify("password".bytes(), "$argon2id$v=19$m=65536,t=2,p=1$MDEyMzQ1Njc4OWFiY2RlZg$8+/qEheh109gdg4M3e8OHdzFjQSPKlC6NgbVvwpor58", exact)))

  print(string_result(pkg.auth.password_hash("pw".bytes(), pkg.auth.Argon2Policy{{m_cost: 7, t_cost: 1, parallelism: 1}})))
  print(string_result(pkg.auth.password_hash("pw".bytes(), pkg.auth.Argon2Policy{{m_cost: 64, t_cost: 0, parallelism: 1}})))
  print(string_result(pkg.auth.password_hash("pw".bytes(), pkg.auth.Argon2Policy{{m_cost: 64, t_cost: 1, parallelism: 0}})))
  print(string_result(pkg.auth.password_hash("pw".bytes(), pkg.auth.Argon2Policy{{m_cost: 4194305, t_cost: 1, parallelism: 1}})))

  small := pkg.auth.Argon2Policy{{m_cost: 64, t_cost: 1, parallelism: 1}}
  first := pkg.auth.password_hash("a\0b".bytes(), small)?
  second := pkg.auth.password_hash("a\0b".bytes(), small)?
  empty := pkg.auth.password_hash("".bytes(), small)?
  print(first.len())
  print(first == second)
  print(bool_result(pkg.auth.password_verify("a\0b".bytes(), first, small)))
  print(bool_result(pkg.auth.password_verify("wrong".bytes(), second, small)))
  print(bool_result(pkg.auth.password_verify("".bytes(), empty, small)))
  return Ok(())
}}
"#
    );
    let output = build_and_run_multi("pkg-auth-password", &auth_files(&main), "main.align");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "10\n11\n1\n1\n1\n1\n1\n1\n1\n1\n1\n1\n11\n1\n1\n1\n1\n94\nfalse\n10\n11\n10\n",
    );
}

#[test]
fn jwt_claim_and_result_bounds_are_exact() {
    if !backend_available() {
        return;
    }
    let exact_claims = format!(r#"{{"x":"{}"}}"#, "a".repeat(8184));
    let over_claims = format!(r#"{{"x":"{}"}}"#, "a".repeat(8185));
    assert_eq!(exact_claims.len(), 8192);
    assert_eq!(over_claims.len(), 8193);
    let exact_claims = format!("{exact_claims:?}");
    let over_claims = format!("{over_claims:?}");
    let main = format!(
        r#"module main
import pkg.auth
import std.encoding

fn invalid(result: Result<string, Error>) -> bool = match result {{
  Err(error) => match error {{ Invalid => true, _ => false }},
  Ok(_) => false,
}}

fn main() -> Result<(), Error> {{
  key := encoding.hex_decode("{KEY_HEX}")?
  token := pkg.auth.encode_hs256({exact_claims}, key.bytes())?
  print(token.len())
  print(invalid(pkg.auth.encode_hs256({over_claims}, key.bytes())))
  verified := pkg.auth.verify_hs256(token, key.bytes(), 0)?
  print(verified.len())
  return Ok(())
}}
"#
    );
    let output = build_and_run_multi("pkg-auth-jwt-bounds", &auth_files(&main), "main.align");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "11004\ntrue\n8192\n"
    );
}

#[test]
fn session_only_use_keeps_module_crypto_and_public_surface_is_closed() {
    for required in [
        "pub Argon2Policy {\n  m_cost: i64,\n  t_cost: i64,\n  parallelism: i64,\n}",
        "pub fn encode_hs256(claims_json: str, key: slice<u8>) -> Result<string, Error>",
        "pub fn verify_hs256(token: str, key: slice<u8>, now_ns: i64) -> Result<string, Error>",
        "pub fn password_hash(password: slice<u8>, policy: Argon2Policy) -> Result<string, Error>",
        "pub fn password_verify(\n  password: slice<u8>,\n  phc: str,\n  maximum: Argon2Policy,\n) -> Result<bool, Error>",
        "pub fn session_token() -> string",
        "mut salt := buffer(16)\n  crypto.random(salt)",
        "mut random := buffer(32)\n  crypto.random(random)",
    ] {
        assert!(
            auth_source().contains(required),
            "missing public surface `{required}`"
        );
    }
    let public: Vec<_> = auth_source()
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub "))
        .map(|line| line.split([' ', '(']).take(3).collect::<Vec<_>>().join(" "))
        .collect();
    assert_eq!(
        public,
        [
            "pub Argon2Policy {",
            "pub fn encode_hs256",
            "pub fn verify_hs256",
            "pub fn password_hash",
            "pub fn password_verify",
            "pub fn session_token",
        ],
    );

    let main = r#"module main
import pkg.auth
import std.encoding

fn main() -> Result<(), Error> {
  first := pkg.auth.session_token()
  second := pkg.auth.session_token()
  decoded := encoding.base64url_decode(first)?
  print(first.len())
  print(decoded.len())
  print(first == second)
  return Ok(())
}
"#;
    let files = auth_files(main);
    let differential = diff_check_multi("pkg-auth-session-interface", &files, "main.align");
    assert!(!differential.whole_errors && !differential.per_unit_errors);
    if !backend_available() {
        return;
    }
    let per_unit = build_per_unit_multi("pkg-auth-session-capability", &files, "main.align");
    assert!(
        per_unit
            .unit("pkg.auth")
            .mir
            .link_libs
            .iter()
            .any(|library| library == "crypto"),
        "session-only pkg.auth unit must retain module-wide libcrypto: {:?}",
        per_unit.unit("pkg.auth").mir.link_libs,
    );
    let output = per_unit.link_and_run();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "43\n32\nfalse\n");

    let verify_source = auth_source()
        .split_once("pub fn password_verify")
        .expect("password_verify source")
        .1;
    let maximum_validation = verify_source
        .find("maximum_policy_valid")
        .expect("maximum validation");
    let phc_read = verify_source.find("phc.len()").expect("PHC read");
    let kdf = verify_source.find("crypto.argon2id").expect("Argon2 call");
    assert!(maximum_validation < phc_read && phc_read < kdf);

    let function_value = r#"module main
import pkg.auth

fn main() -> i32 {
  signer := pkg.auth.encode_hs256
  return 0
}
"#;
    let diagnostics = check_multi_diagnostics(
        "pkg-auth-function-value",
        &[
            ("pkg/auth.align", auth_source()),
            ("main.align", function_value),
        ],
        "main.align",
    );
    assert!(
        diagnostics.contains("cannot be used as a function value yet")
            && diagnostics.contains("only scalar parameters/return"),
        "{diagnostics}",
    );
}

#[test]
fn owned_results_escape_inputs_and_auth_operations_remain_impure() {
    let claims = format!("{CLAIMS:?}");
    let main = format!(
        r#"module main
import pkg.auth
import std.encoding

fn detached() -> Result<string, Error> {{
  key := encoding.hex_decode("{KEY_HEX}")?
  token := pkg.auth.encode_hs256({claims}, key.bytes())?
  arena {{ return pkg.auth.verify_hs256(token, key.bytes(), 1000000000) }}
}}

fn main() -> Result<(), Error> {{
  claims := detached()?
  print(claims == {claims})
  return Ok(())
}}
"#
    );
    let files = auth_files(&main);
    let differential = diff_check_multi("pkg-auth-owned-result", &files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    if backend_available() {
        let output = build_and_run_multi("pkg-auth-owned-result-run", &files, "main.align");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n");
    }

    let impure = r#"module main
import pkg.auth

fn generate(value: i64) -> i64 {
  token := pkg.auth.session_token()
  return value + token.len()
}

fn main() -> i32 {
  arena { print([1, 2].par_map(generate).sum()) }
  return 0
}
"#;
    let diagnostics = check_multi_diagnostics(
        "pkg-auth-impure",
        &[("pkg/auth.align", auth_source()), ("main.align", impure)],
        "main.align",
    );
    assert!(
        diagnostics.contains("Pure") || diagnostics.contains("pure"),
        "{diagnostics}",
    );
}
