//! Named UTC wire-format family: source admission, ownership and compilation parity.
mod common;
use common::*;

const FORMATS: [(&str, &str, i64); 5] = [
    ("rfc3339", "1969-12-31T23:59:59.999999999Z", -1),
    ("rfc3339_ms", "1969-12-31T23:59:59.999Z", -1_000_000),
    ("rfc1123", "Wed, 31 Dec 1969 23:59:59 GMT", -1_000_000_000),
    ("basic_iso", "19691231T235959Z", -1_000_000_000),
    ("basic_date", "19691231", -86_400_000_000_000),
];

#[test]
fn named_formats_round_trip_negative_epoch_whole_and_unit() {
    if !backend_available() {
        return;
    }
    let mut body = String::from("import std.time\npub fn main() -> Result<(), Error> {\n");
    let mut expected = String::new();
    for (i, (name, rendered, ns)) in FORMATS.iter().enumerate() {
        body.push_str(&format!(
            "  text{i} := time.{name}(-1)?\n  print(text{i})\n  print(time.parse_{name}(text{i})?)\n"
        ));
        expected.push_str(&format!("{rendered}\n{ns}\n"));
    }
    body.push_str("  return Ok(())\n}\n");
    let files = [("main.align", body.as_str())];
    let whole = build_and_run_multi("time-negative-whole", &files, "main.align");
    let unit = build_per_unit_multi("time-negative-unit", &files, "main.align").link_and_run();
    for output in [whole, unit] {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
}

#[test]
fn named_formats_reject_wrong_types_and_arities() {
    for (name, _, _) in FORMATS {
        for call in [
            format!("time.{name}()"),
            format!("time.{name}(0, 0)"),
            format!("time.{name}(true)"),
            format!("time.{name}(value)"),
            format!("time.parse_{name}()"),
            format!("time.parse_{name}(0)"),
            format!("time.parse_{name}(\"\", \"\")"),
        ] {
            let source =
                format!("import std.time\npub fn bad(value: i32) {{\n  result := {call}\n}}\n");
            let diagnostics = check_diagnostics("time-invalid-api", &source);
            assert!(!diagnostics.is_empty(), "unexpected acceptance: {call}");
            assert!(
                !diagnostics.contains("undefined name"),
                "{call}: {diagnostics}"
            );
        }
    }
}

#[test]
fn format_family_imports_generic_replay_control_flow_and_purity() {
    if !backend_available() {
        return;
    }
    for (name, _, expected) in FORMATS {
        let helper = format!(
            r#"module helper
import std.time
pub fn render<T>(marker: T, value: i64) -> Result<string, Error> = time.{name}(value)
pub fn parse(value: str) -> Result<i64, Error> = time.parse_{name}(value)
pub fn fixed(value: i64) -> Result<string, Error> = time.{name}(value)
"#
        );
        let source = r#"
import helper
import std.time
Packet { text: string, other: Option<string> }
fn keep(error: Error) -> Error = error
fn observed_ns() -> i64 { print("ns"); return -1 }
fn observed_text() -> string { print("text"); return "1970-01-01T00:00:00Z".clone() }
fn operand(stop: bool) -> Result<string, Error> {
  return time.rfc3339({
    if stop { return Err(Error.Invalid) }
    value: i64 := -1
    value
  })
}
fn scalar(value: i64) -> i64 {
  text := helper.fixed(value) else { return 999 }
  return helper.parse(text) else { return 999 }
}
pub fn main() -> Result<(), Error> {
  owned := arena { helper.render(true, -1)? }
  print(helper.parse(owned)?)
  callback := helper.fixed
  mut replacement := callback(-1)?
  replacement = helper.fixed(-1).map_err(keep)?
  print(helper.parse(replacement)?)
  joined := if true { helper.fixed(-1) } else { helper.fixed(0) }
  selected := match joined { Ok(value) => value, Err(error) => { return Err(error) } }
  loop_value := loop { break helper.parse(selected)? }
  print(loop_value)
  print([-1, -1].par_map(scalar).sum())
  print(match operand(true) { Ok(value) => false, Err(error) => true })
  print(operand(false)?)
  packet := Packet { text: helper.fixed(-1)?, other: Some(helper.fixed(-1)?) }
  print(helper.parse(packet.text)?)
  print(match packet.other { Some(value) => helper.parse(value)?, None => 999 })
  parsed := { temporary := helper.fixed(-1)?; helper.parse(temporary)? }
  print(parsed)
  print(time.rfc3339(observed_ns())?)
  print(time.parse_rfc3339(observed_text())?)
  return Ok(())
}
"#;
        let files = [("helper.align", helper.as_str()), ("main.align", source)];
        let whole = build_and_run_multi(&format!("time-flow-whole-{name}"), &files, "main.align");
        let unit = build_per_unit_multi(&format!("time-flow-unit-{name}"), &files, "main.align")
            .link_and_run();
        for output in [whole, unit] {
            assert!(
                output.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                format!(
                    "{expected}\n{expected}\n{expected}\n{}\ntrue\n1969-12-31T23:59:59.999999999Z\n{expected}\n{expected}\n{expected}\nns\n1969-12-31T23:59:59.999999999Z\ntext\n0\n",
                    expected * 2
                )
            );
        }
    }
}

#[test]
fn moved_formatter_results_are_rejected_whole_and_unit() {
    for (name, _, _) in FORMATS {
        let source = format!(
            "import std.time\nfn bad() -> Result<string, Error> {{\n  text := time.{name}(0)?\n  moved := text\n  return Ok(text)\n}}\n"
        );
        let verdict = assert_same_verdict(
            "time-moved",
            &[("main.align", source.as_str())],
            "main.align",
        );
        assert!(verdict.diags.has_errors());
    }
}

#[test]
fn malformed_timestamps_map_to_the_shared_invalid_error() {
    if !backend_available() { return; }
    for (name, _, _) in FORMATS {
        let source = format!("import std.time\npub fn main() -> Result<(), Error> {{\n  value := time.parse_{name}(\"not a timestamp\")?\n  print(value)\n  return Ok(())\n}}\n");
        let output = build_and_run(&format!("time-invalid-{name}"), &source);
        assert_eq!(output.status.code(), Some(2), "{name}: {}", String::from_utf8_lossy(&output.stderr));
        assert!(output.stdout.is_empty());
    }
}
