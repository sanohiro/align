//! Copy string sorting owns the result spine and retains the borrowed byte owners.
mod common;
use common::*;

fn literal(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"").replace('\0', "\\0"))
}

fn array(values: &[&str]) -> String {
    format!("[{}]", values.iter().map(|value| literal(value)).collect::<Vec<_>>().join(", "))
}

#[test]
fn string_sort_order_and_stability() {
    if !backend_available() { return; }
    let helpers = r#"module ordering
pub fn ordered<T: Ord>(xs: slice<T>) -> array<T> = xs.sort()
pub fn by_length(xs: slice<str>) -> array<str> = xs.sort_by_key(fn s: str { s.len() })
pub fn agrees(xs: slice<str>, expected: slice<str>) -> bool {
  sorted := ordered(xs)
  repeated := sorted.sort()
  by_text := xs.sort_by_key(fn text: str { text })
  if repeated.len() != expected.len() { return false }
  mut i := 0
  loop {
    if i >= expected.len() { break }
    if repeated[i] != expected[i] { return false }
    if by_text[i] != expected[i] { return false }
    i = i + 1
  }
  return true
}
"#;
    let catalog = ["", "a", "a\0", "a\0b", "aa", "ab", "z", "é", "中", "😀"];
    let mut source = String::from("module main\nimport ordering\nfn main() {\n");
    let mut expected = String::new();
    // One imported sort body exercises every algorithm exit rather than duplicating lowering.
    for n in [0, 1, 2, 31, 32, 33, 63, 64, 65, 129] {
        let mut values: Vec<_> = (0..n).map(|i| catalog[(i * 7 + 3) % catalog.len()]).collect();
        values.sort();
        for state in ["sorted", "reverse", "mixed"] {
            let input = match state {
                "sorted" => values.clone(),
                "reverse" => values.iter().rev().copied().collect(),
                _ => (0..n).map(|i| catalog[(i * 7 + 3) % catalog.len()]).collect(),
            };
            let input = if n == 0 { "[\"seed\"][0..0]".to_string() } else { format!("{}[..]", array(&input)) };
            let output = if n == 0 { "[\"seed\"][0..0]".to_string() } else { format!("{}[..]", array(&values)) };
            source.push_str(&format!("  print(ordering.agrees({input}, {output}))\n"));
            expected.push_str("true\n");
        }
    }
    // Distinct strings of equal length reveal the original order after keyed reordering.
    let tied = ["dd", "a", "bb", "c", "ee", "f", "gg", "h"];
    let keyed: Vec<_> = (0..65).map(|i| tied[i % tied.len()]).collect();
    let mut stable = keyed.clone();
    stable.sort_by_key(|text| text.len());
    source.push_str(&format!("  keyed := ordering.by_length({}[..])\n", array(&keyed)));
    for (i, value) in stable.iter().enumerate() {
        source.push_str(&format!("  print(keyed[{i}] == {})\n", literal(value)));
        expected.push_str("true\n");
    }
    source.push_str(r#"  empty := ["seed"][0..0].sort_by_key(fn text: str { print("unexpected-empty-key"); text.len() })
  singleton := ["only"].sort_by_key(fn text: str { print(text); text.len() })
  print(empty.len()); print(singleton[0])
  effects := ["bb", "a", "cc", "d"].sort_by_key(fn text: str { print(text); text.len() })
  print(effects[0]); print(effects[1]); print(effects[2]); print(effects[3])
  characters := ordering.ordered(['中', 'a', 'é'][..])
  print(characters[0]); print(characters[1]); print(characters[2])
  temporary := ordering.ordered(["z", "a"][..]).sort()
  print(temporary[0]); print(temporary[1])
  arena {
    owned := "borrowed".clone()
    views := [owned.trim(), "alpha"].where(fn s: str { s.len() > 0 }).sort()
    print(views[0]); print(views[1])
  }
}
"#);
    expected.push_str("only\n0\nonly\nbb\na\ncc\nd\na\nd\nbb\ncc\na\né\n中\na\nz\nalpha\nborrowed\n");
    let files = [("main.align", source.as_str()), ("ordering.align", helpers)];
    for output in [build_and_run_multi("string-sort-whole", &files, "main.align"),
        build_per_unit_multi("string-sort-unit", &files, "main.align").link_and_run()] {
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
}

#[test]
fn copy_sort_domain_and_diagnostics() {
    for (name, source, diagnostic) in [
        ("bool", "fn main() { values := [true, false].sort() }", "Copy Ord"),
        ("owned", "fn bad(values: array<string>) { sorted := values.sort() }\nfn main() {}", "Copy Ord"),
        ("generic-owned", "fn ordered<T: Ord>(xs: slice<T>) -> array<T> = xs.sort()\nfn bad(values: slice<string>) { sorted := ordered(values) }\nfn main() {}", "Copy Ord"),
        ("arity", "fn main() { values := [\"x\"].sort(1) }", "takes no arguments"),
        ("result", "fn main() { values: array<i64> := [\"x\"].sort() }", "type mismatch"),
        ("key", "fn main() { values := [\"x\"].sort_by_key(fn s: str { true }) }", "orderable"),
        ("owned-key", "fn main() { values := [\"x\"].sort_by_key(fn s: str { s.clone() }) }", "Move key"),
        ("unresolved", "fn main() { values := missing.sort() }", "undefined"),
    ] {
        let checked = diff_check_multi(&format!("string-sort-{name}"), &[("main.align", source)], "main.align");
        assert!(checked.whole_errors && checked.per_unit_errors, "{name}: {}\n{}", checked.whole_diags, checked.per_unit_diags);
        assert!(checked.whole_diags.contains(diagnostic) && checked.per_unit_diags.contains(diagnostic), "{name}: {}\n{}", checked.whole_diags, checked.per_unit_diags);
        if name == "unresolved" {
            assert_eq!(checked.whole_diags.matches("error:").count(), 1);
            assert_eq!(checked.per_unit_diags.matches("error:").count(), 1);
        }
    }
}

#[test]
fn string_sort_lifetime_and_control() {
    let terminals = ["sort()", "sort_by_key(fn s: str { s.len() })"];
    for terminal in terminals {
        let cases = [
            ("local-return", format!("fn bad() -> array<str> {{ owner := \"text\".clone(); return [owner.trim()].{terminal} }}\nfn main() {{}}")),
            ("mapped-return", format!("fn bad() -> array<str> {{ owner := \"text\".clone(); view: str := owner; return [1, 2].map(fn n: i64 {{ view }}).{terminal} }}\nfn main() {{}}")),
            ("replace", format!("fn main() {{ mut owner := \"text\".clone(); sorted := [owner.trim()].{terminal}; owner = \"new\".clone(); print(sorted[0]) }}")),
            ("move", format!("fn take(s: string) {{}}\nfn main() {{ owner := \"text\".clone(); sorted := [owner.trim()].{terminal}; take(owner); print(sorted[0]) }}")),
            ("source-spine", format!("fn main() {{ mut source := [\"z\", \"a\"].to_array(); sorted := source.{terminal}; source = [\"replacement\"].to_array(); print(sorted[0]) }}")),
            ("truncate-byte-owner", format!("fn main() {{ mut builder: array_builder<string> := array_builder(); builder.push(\"z\".clone()); builder.push(\"a\".clone()); mut owners := builder.build(); sorted := [owners[0], owners[1]].{terminal}; owners.truncate(0); print(sorted[0]) }}")),
            ("loop-drop", format!("fn main() {{ mut keep := [\"static\"].{terminal}; loop {{ owner := \"text\".clone(); keep = [owner.trim()].{terminal}; break }}; print(keep[0]) }}")),
            ("arena-return", format!("fn bad() -> array<str> {{ return arena {{ [\"x\"].{terminal} }} }}\nfn main() {{}}")),
        ];
        for (name, source) in cases {
            let checked = diff_check_multi(&format!("string-sort-lifetime-{name}-{terminal}"), &[("main.align", source.as_str())], "main.align");
            assert!(checked.whole_errors && checked.per_unit_errors, "{name}: {}\n{}", checked.whole_diags, checked.per_unit_diags);
            assert!(checked.whole_diags.contains("borrow") || checked.whole_diags.contains("arena"), "{name}: {}", checked.whole_diags);
        }
    }
    let legal = r#"module main
fn identity(s: str) -> str = s
fn returned(xs: slice<str>) -> array<str> = xs.sort()
fn result(xs: slice<str>) -> Result<array<str>, Error> = Ok(returned(xs))
fn joined(flag: bool, xs: slice<str>) -> array<str> = if flag { xs.sort() } else { xs.sort_by_key(identity) }
fn main() -> Result<(), Error> {
  owner := "text".clone()
  source := [owner.trim(), "alpha"]
  selected := joined(true, source[..])
  through_try := result(source[..]).map_err(fn e: Error { e })?
  through_else := result(source[..]) else { return Err(Error.Invalid) }
  optional: Option<array<str>> := Some(returned(source[..]))
  through_match := match optional { Some(value) => value, None => returned(source[..]) }
  mut replacement := ["before"].sort()
  replacement = source.sort()
  repeat := replacement.sort_by_key(identity)
  print(selected[0]); print(through_try[0]); print(through_else[0]); print(through_match[0]); print(repeat[0])
  return Ok(())
}
"#;
    let files = [("main.align", legal)];
    if backend_available() {
        for output in [build_and_run_multi("string-sort-control-whole", &files, "main.align"),
            build_per_unit_multi("string-sort-control-unit", &files, "main.align").link_and_run()] {
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout), "alpha\nalpha\nalpha\nalpha\nalpha\n");
        }
    }
}

#[test]
fn string_sort_mir_keeps_shallow_storage() {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "string-sort-mir", "fn sorted(xs: slice<str>) -> array<str> = xs.sort()\n");
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    assert!(!mir.fns.is_empty());
    let statements: Vec<_> = mir.fns.iter().flat_map(|function| function.blocks.iter().flat_map(|block| block.stmts.iter())).collect();
    assert!(!statements.iter().any(|statement| matches!(statement, align_mir::Stmt::Let(_, align_mir::Rvalue::StrClone(_)))));
    assert!(statements.iter().any(|statement| matches!(statement, align_mir::Stmt::DropValue(_))));
    assert!(statements.iter().any(|statement| matches!(statement, align_mir::Stmt::Let(_, align_mir::Rvalue::HeapAllocBuf { elem: align_sema::Ty::Str, .. }))));
}
