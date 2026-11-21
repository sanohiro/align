//! Literal scalar-set search shares string predicate borrowing and Option lowering.
mod common;
use common::*;

#[test]
fn searches_scalar_sets_in_whole_and_imported_generic_code() {
    if !backend_available() {
        return;
    }
    let helpers = r#"module search
pub fn first<T>(unused: T, text: str, chars: str) -> Option<i64> = text.find_any(chars)
"#;
    let main = r#"import search
fn hay() -> string { print(101); return "aあb".clone() }
fn delimiters() -> string { print(102); return "bあ".clone() }
fn early(stop: bool) -> i64 = hay().find_any({ if stop { return 7 }; "b" }) else -1
fn main() {
  print(hay().find_any(delimiters()) else -1)
  print(early(true))
  s := "aé界🚀;".clone()
  set := ";🚀".clone()
  print(search.first(7, s, set) else -1)
  print(s.len())
  print(set.len())
  print("xé".find_any("ê") else -1)
  print("Ê".find_any("Ċ") else -1)
  print("literal".find_any("") else -1)
  print("".find_any("a") else -1)
  print("あ\0b".find_any("x\0") else -1)
  print("xyzz".find_any("zzx") else -1)
  print("foo/bar".find_any(":,.//") else -1)
  mut i := 0
  loop {
    if i == 2 { break }
    print(search.first(true, "a:b", ",:") else -1)
    i = i + 1
  }
}
"#;
    let files = [("search.align", helpers), ("main.align", main)];
    let mir = whole_mir_multi("find-any-mir", &files, "main.align");
    assert!(mir.contains("str_find_any("), "{mir}");
    for out in [
        build_and_run_multi("find-any-whole", &files, "main.align"),
        build_per_unit_multi("find-any-units", &files, "main.align").link_and_run(),
    ] {
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "101\n102\n1\n101\n7\n6\n11\n5\n-1\n-1\n-1\n-1\n3\n0\n3\n1\n1\n"
        );
    }
}

#[test]
fn checks_operand_types_and_observation_lifetime() {
    let prefix = "owner := [(65 as u8)].to_array(); mut alias: slice<u8> := owner; text := alias.as_str() else { return };";
    for (body, error) in [
        ("answer := text.find_any()", true),
        ("answer := text.find_any(1)", true),
        ("answer := text.find_any({ alias[0] = 255; \"A\" })", true),
        (
            "answer := text.find_any(\"A\"); alias[0] = 255; print(answer else -1)",
            false,
        ),
    ] {
        let src = format!("fn main() {{ {prefix} {body} }}\n");
        let result = diff_check_multi("find-any-check", &[("main.align", &src)], "main.align");
        assert_eq!(result.whole_errors, error, "{}", result.whole_diags);
        assert_eq!(result.per_unit_errors, error, "{}", result.per_unit_diags);
    }
}
