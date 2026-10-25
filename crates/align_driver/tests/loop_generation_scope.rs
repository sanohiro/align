//! Plan245: break-edge generation advances belong to the innermost loop entry.
mod common;
use common::*;

#[path = "helpers/indexed_array_field.rs"]
mod native;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

fn verdict(label: &str, source: &str, rejected: bool) {
    let checked = diff_check_multi(label, &[("main.align", source)], "main.align");
    assert_eq!(checked.whole_errors, rejected, "{source}\n{}", checked.whole_diags);
    assert_eq!(checked.per_unit_errors, rejected, "{source}\n{}", checked.per_unit_diags);
    if rejected {
        for diagnostics in [&checked.whole_diags, &checked.per_unit_diags] {
            assert!(diagnostics.contains("invalidated") || diagnostics.contains("outlive"), "{diagnostics}");
        }
    }
}

fn nested(depth: usize, owner_depth: usize, init: &str, read: &str) -> String {
    let mut body = format!("print({read})\n");
    for level in (1..=depth).rev() {
        if owner_depth == level {
            body = format!("expected := {init}\n{body}print({read})\n");
        }
        body = format!("mut i{level} := 0\nloop {{\nif i{level} >= 2 {{ break }}\n{body}i{level} = i{level} + 1\n}}\n");
    }
    if owner_depth == 0 { body = format!("expected := {init}\n{body}"); }
    format!("fn read(borrow value: array<i64>) -> i64 = value[0]\nfn main() {{\n{body}}}\n")
}

#[test]
fn depth_and_owner_matrix() {
    for depth in 1..=4 {
        for owner_depth in 0..=depth {
            verdict("loop-generation-depth", &nested(depth, owner_depth, "[7].to_array()", "read(expected)"), false);
        }
    }
    for (init, read) in [("\"abc\".clone()", "expected.len()"), ("7", "expected")] {
        verdict("loop-generation-scalar-control", &nested(4, 1, init, read), false);
    }
}

#[test]
fn control_edges() {
    // The conditional frames inside the innermost loop must still contribute their advances;
    // only frames predating that loop are excluded. A value break uses the iterative walker.
    for (label, action, value_break) in [
        ("if", "if k == 1 { break }", false),
        ("match", "match k { 1 => { break }, _ => {} }", false),
        ("option-else", "value := optional(k) else { break }; print(value)", false),
        ("result-else", "value := fallible(k) else { break }; print(value)", false),
        ("try", "value := fallible(k)?; print(value)", false),
        ("map-err", "value := fallible(k).map_err(fn error: Error { error })?; print(value)", false),
        ("return", "if k == 1 { return Ok(expected[0]) }", false),
        ("value", "if k == 1 { break expected[0] }", true),
    ] {
        let binding = if value_break { "selected := " } else { "" };
        let end = if value_break { "break expected[0]" } else { "break" };
        let after = if value_break { "print(selected)" } else { "" };
        let source = format!(r#"
fn optional(n: i64) -> Option<i64> = if n == 0 {{ Some(n) }} else {{ None }}
fn fallible(n: i64) -> Result<i64, Error> = Ok(n)
fn work() -> Result<i64, Error> {{
 mut i := 0
 loop {{
  if i >= 2 {{ break }}
  expected := [7, 9].to_array()
  mut j := 0
  loop {{
   if j >= 2 {{ break }}
   mut k := 0
   {binding}loop {{
    if k >= 2 {{ {end} }}
    {action}
    print(expected[0])
    k = k + 1
   }}
   {after}
   print(expected[1])
   j = j + 1
  }}
  print(expected.len())
  i = i + 1
 }}
 Ok(0)
}}
fn main() {{ result := work() else {{ return }}; print(result) }}
"#);
        verdict(&format!("loop-generation-{label}"), &source, false);
    }
}

#[test]
fn actual_expiration() {
    for replacement in [
        "owner = [8].to_array()",
        "if n == 0 { owner = [8].to_array() }",
        "match n { 0 => { owner = [8].to_array() }, _ => {} }",
    ] {
        let source = format!("fn main() {{ mut owner := [7].to_array(); stale := owner[..]; mut n := 0; loop {{ if n >= 2 {{ break }}; loop {{ {replacement}; break }}; print(stale[0]); n = n + 1 }} }}");
        verdict("loop-generation-replacement", &source, true);
    }
    for init in ["owner := [7].to_array(); escaped = owner[..]", "escaped = [7].to_array()[..]"] {
        let source = format!("fn main() {{ seed := [0]; mut escaped := seed[..]; loop {{ loop {{ {init}; break }}; break }}; print(escaped[0]) }}");
        verdict("loop-generation-expiration", &source, true);
    }
}

#[test]
fn imported_native() {
    owned_fixture::run("imported_native", |stage| {
        let helper = "module values\npub fn make() -> array<i64> = [7, 9].to_array()\npub fn read<T>(borrow value: array<i64>, marker: T) -> i64 = value[0] + value[1]\n";
        let source = r#"import values
fn run_case(borrow expected: array<i64>) -> i64 {
 mut total := 0
 mut j := 0
 loop {
  if j >= 2 { break }
  mut k := 0
  loop { if k >= 2 { break }; total = total + values.read(expected, true); k = k + 1 }
  j = j + 1
 }
 total
}
fn main() -> i32 {
 mut total := 0
 mut control := 0
 mut i := 0
 loop {
  if i >= 2 { break }
  expected := values.make()
  control = control + run_case(expected)
  mut j := 0
  loop {
   if j >= 2 { break }
   mut k := 0
   loop { if k >= 2 { break }; total = total + values.read(expected, 1); k = k + 1 }
   j = j + 1
  }
  i = i + 1
 }
 if total == 128 && control == 128 { 0 } else { 1 }
}
"#;
        std::fs::write(stage.join("values.align"), helper).unwrap();
        native::execute(stage, source, "nested-native").unwrap();
        let cache = align_driver::CacheContext::at(stage.join("cache"));
        let entry = stage.join("main.align");
        let mut first = Vec::new();
        for round in 0..2 {
            let mut sources = align_span::SourceMap::new();
            let mut package = align_driver::build_package(&mut sources, entry.to_str().unwrap(), source,
                &cache, align_driver::UnitReuse::Allowed);
            assert!(!package.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &package.diags));
            let helper = package.units.iter().find(|unit| unit.unit == "values").unwrap();
            assert_eq!(helper.frontend.as_ref().unwrap().hit, round == 1);
            let programs = (0..package.units.len()).map(|index| package.materialize(index).unwrap().clone()).collect::<Vec<_>>();
            let printed = programs.iter().map(align_mir::print::program_to_string).collect::<Vec<_>>();
            if round == 0 { first = printed; } else { assert_eq!(first, printed); }
            let output = native::run_programs(stage, &programs, &format!("cached-{round}"), false).unwrap();
            assert!(output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(), "{output:?}");
        }
    });
}
