//! Initialized builder elements copy out without retaining storage authority.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{backend_available, build_per_unit, check, emit_object_file, link_objects,
    lower_to_mir, BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

fn check_diagnostics(label: &str, source: &str) -> String {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, label, source);
    align_driver::format_diagnostics(&sources, &checked.diags)
}
fn check_errs(label: &str, source: &str) -> bool {
    let mut sources = SourceMap::new();
    check(&mut sources, label, source).diags.has_errors()
}

// Called only inside owned_fixture::run, including every transitive native wait.
fn execute(stage: &Path, files: &[(&str, &str)], per_unit: bool) -> Option<std::process::Output> {
    for (name, source) in files { std::fs::write(stage.join(name), source).unwrap(); }
    let entry = stage.join("main.align");
    let source = std::fs::read_to_string(&entry).unwrap();
    let mut sources = SourceMap::new();
    let programs = if per_unit {
        let walk = build_per_unit(&mut sources, entry.to_str().unwrap(), &source);
        assert!(!walk.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &walk.diags));
        assert_eq!(walk.units.len(), files.len());
        walk.units.into_iter().map(|unit| unit.mir).collect::<Vec<_>>()
    } else {
        let checked = check(&mut sources, entry.to_str().unwrap(), &source);
        assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &checked.diags));
        vec![lower_to_mir(&checked.hir)]
    };
    if !backend_available() { return None; }
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{per_unit}-{index}.o"));
        emit_object_file(program, &object, BuildTarget::Baseline, Profile::Release, &[], false).unwrap();
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) { libraries.push(library.clone()); }
        }
    }
    let executable = stage.join(format!("program-{per_unit}"));
    link_objects(&align_driver::CDriver::default(),
        &objects.iter().map(|path| path.as_path()).collect::<Vec<_>>(),
        &executable, &libraries, Profile::Release).unwrap();
    Some(std::process::Command::new(executable).output().unwrap())
}

fn assert_checked(label: &str, source: &str) {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, label, source);
    assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &checked.diags));
    let effects = align_sema::fn_effects(&checked.hir, &std::collections::HashMap::new());
    for function in checked.hir.fns.iter().filter(|function| function.name == "query") {
        assert_eq!(effects.get(&function.name), Some(&align_sema::FnEffect::Pure));
        assert_eq!(function.return_borrow, align_sema::hir::ReturnBorrowSummary::None);
    }
    assert!(align_mir::lower_program_checked(&checked.hir, false, None).is_ok());
}

#[test]
fn formation_and_domains() {
    owned_fixture::run("formation_and_domains", formation_and_domains_in);
}
fn formation_and_domains_in(stage: &Path) {
    for mode in ["", "borrow", "borrow mut"] {
        assert_checked("read-mode", &format!("fn query({mode} b: array_builder<i64>) -> i64 = b[0]\nfn main() {{}}"));
    }
    for (source, diagnostic) in [
        ("fn create() -> array_builder<i64> = array_builder()\nfn main() { print(create()[true]) }", "bound variable"),
        ("fn main() { b: array_builder<i64> := array_builder(); print(b[true]) }", "bool"),
        ("fn main() { b: array_builder<string> := array_builder(); print(b[0]) }", "independent Copy"),
        ("Row { name: str }\nfn query(borrow b: array_builder<Row>) { v := b[0] }\nfn main() {}", "independent Copy"),
        ("Row { name: string }\nfn query(borrow b: array_builder<Row>) { v := b[0] }\nfn main() {}", "independent Copy"),
        ("Row { hidden: [str; 0] }\nfn query(borrow b: array_builder<Row>) { v := b[0] }\nfn main() {}", "independent Copy"),
        ("Choice { Data(str), Empty }\nfn query(borrow b: array_builder<Choice>) { v := b[0] }\nfn main() {}", "independent Copy"),
        ("fn query(borrow b: array_builder<()>) { v := b[0] }\nfn main() {}", "got ()"),
        ("fn query(borrow b: array_builder<raw>) { v := b[0] }\nfn main() {}", "got raw"),
        ("fn query(action: fn(borrow mut array_builder<i64>) -> ()) {}\nfn main() {}", "function-type parameter must be a scalar"),
    ] {
        let actual = check_diagnostics("read-domain", source);
        assert!(check_errs("read-domain", source) && actual.contains(diagnostic), "{source}\n{actual}");
    }
    let source = r#"Row { flag: bool, small: u8, value: i64 }
Zero { value: [i64; 0] }
Choice { One(Row), None }
fn main() {
  mut b: array_builder<Row> := array_builder(1)
  b.push(Row { flag: true, small: 251, value: -11 })
  saved := b[0]
  print(saved.flag); print(saved.small); print(saved.value)
  arena out {
    v: vec4<i32> := [1, 2, 3, 4]
    w: vec4<i32> := [0, 3, 2, 5]
    mut vectors: array_builder<vec4<i32>> := array_builder(out)
    vectors.push(v)
    vector := vectors[0]
    print(vector[2])
    mut masks: array_builder<mask4<i32>> := array_builder(out)
    masks.push(v > w)
    mask := masks[0]
    print(select(mask, v, w)[1])
    mut fixed: array_builder<[i64; 2]> := array_builder(out)
    input: [i64; 2] := [4, 5]
    fixed.push(input)
    copied := fixed[0]
    print(copied[1])
    mut records: array_builder<[Row; 1]> := array_builder(out)
    row_input: [Row; 1] := [saved]
    records.push(row_input)
    copied_records := records[0]
    print(copied_records[0].value)
    mut zeros: array_builder<Zero> := array_builder(out)
    empty: [i64; 0] := []
    zeros.push(Zero { value: empty })
    zero := zeros[0]
    print(zero.value.len())
    mut options: array_builder<Option<i64>> := array_builder(out)
    options.push(Some(12))
    option := options[0]
    print(option else 0)
    mut sums: array_builder<Choice> := array_builder(out)
    sums.push(Choice.One(saved))
    choice := sums[0]
    print(match choice { One(row) => row.value, None => 0 })
  }
}
"#;
    if let Some(output) = execute(stage, &[("main.align", source)], false) {
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n251\n-11\n3\n3\n5\n-11\n0\n12\n-11\n");
    }
    for (ty, value, expected) in [("i8", "-7", "-7"), ("u8", "251", "251"), ("i16", "-300", "-300"),
        ("u16", "60000", "60000"), ("i32", "-70000", "-70000"), ("u32", "4000000000", "4000000000"),
        ("i64", "-9000000000", "-9000000000"), ("u64", "18000000000000000000", "18000000000000000000"),
        ("f32", "1.5", "1.5"), ("f64", "-2.5", "-2.5"), ("bool", "true", "true"), ("char", "'界'", "界")] {
        for region in [false, true] {
            let source = format!("fn main() {{ {} mut b: array_builder<{ty}> := array_builder({}); b.push({value}); print(b[0]); {} }}",
                if region { "arena out {" } else { "" }, if region { "out" } else { "" }, if region { "}" } else { "" });
            if let Some(output) = execute(stage, &[("main.align", &source)], false) {
                assert!(output.status.success(), "{ty}: {}", String::from_utf8_lossy(&output.stderr));
                assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected, "{ty}/{region}");
            }
        }
    }
}

#[test]
fn evaluation_and_control() {
    owned_fixture::run("evaluation_and_control", evaluation_and_control_in);
}
fn evaluation_and_control_in(stage: &Path) {
    for operation in ["b.build(); 0", "b = array_builder(); 0", "b.push(2); 0", "take(b); 0", "change(b); 0"] {
        let source = format!("fn take(b: array_builder<i64>) {{}}\nfn change(borrow mut b: array_builder<i64>) {{ b.push(3) }}\nfn main() {{ mut b: array_builder<i64> := array_builder(); b.push(7); print(b[{{ {operation} }}]) }}");
        assert!(check_errs("read-reservation", &source), "accepted invalidated receiver: {source}");
    }
    let source = r#"fn query(borrow b: array_builder<i64>, early: bool) -> i64 {
  x := b[{ if early { return 17 }; 0 }]
  return x
}
fn result(borrow b: array_builder<i64>) -> Result<i64, i32> {
  some: Option<i64> := Some(0)
  good: Result<i64, i32> := Ok(some else 1)
  index := good.map_err(fn error: i32 { error })?
  return Ok(b[match index { 0 => if b.len() == 2 { 0 } else { 1 }, _ => 1 }])
}
fn main() {
  mut b: array_builder<i64> := array_builder()
  b.push(0); b.push(9)
  mut count := 0
  print(b[{ count = count + 1; b[b[0]] + b.len() - 1 }])
  print(count)
  print(query(b, true)); print(query(b, false))
  print(result(b) else -1)
  value := loop { b[{ break 23 }] }
  print(value)
}
"#;
    if let Some(output) = execute(stage, &[("main.align", source)], false) {
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "9\n1\n17\n0\n0\n23\n");
    }
    for (push, index, expected) in [("", "0", "len is 0"), ("b.push(7)", "-1", "index is -1"), ("b.push(7)", "1", "index is 1")] {
        let source = format!("fn main() {{ mut b: array_builder<i64> := array_builder(32); {push}; unused := b[{{ print(42); {index} }}]; return }}");
        if let Some(output) = execute(stage, &[("main.align", &source)], false) {
            assert!(!output.status.success(), "discarded read must check bounds");
            assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
            assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
        }
    }
}

#[test]
fn lifetime_and_growth() {
    owned_fixture::run("lifetime_and_growth", lifetime_and_growth_in);
}
fn lifetime_and_growth_in(stage: &Path) {
    let source = r#"Row { values: [i64; 2] }
fn query() -> Row {
  return arena out {
    mut b: array_builder<Row> := array_builder(out, 1)
    input: [i64; 2] := [7, 8]
    b.push(Row { values: input })
    saved := b[0]
    mut i := 0
    loop { b.push(Row { values: input }); i = i + 1; if i == 70 { break } }
    frozen := b.build()
    print(frozen.len())
    saved
  }
}
fn consume(b: array_builder<i64>) -> i64 = b[0]
fn replace(borrow mut b: array_builder<i64>) { b = array_builder(); b.push(19) }
fn main() {
  row := query()
  print(row.values[1])
  mut b: array_builder<i64> := array_builder(1)
  b.push(9)
  old := b[0]
  b = array_builder()
  b.push(10)
  print(old); print(consume(b)); print(old)
  value := loop {
    mut local: array_builder<i64> := array_builder()
    local.push(11)
    break local[0]
  }
  print(value)
  mut first: array_builder<i64> := array_builder()
  first.push(12)
  mut moved := first
  print(moved[0])
  replace(moved)
  print(moved[0])
  mut round := 0
  loop {
    moved = array_builder()
    moved.push(20 + round)
    print(moved[0])
    replace(moved)
    print(moved[0])
    round = round + 1
    if round == 3 { break }
  }
}
"#;
    if let Some(output) = execute(stage, &[("main.align", source)], false) {
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "71\n8\n9\n10\n9\n11\n12\n19\n20\n19\n21\n19\n22\n19\n");
    }
}

#[test]
fn imports_and_cache() {
    owned_fixture::run("imports_and_cache", imports_and_cache_in);
}
fn imports_and_cache_in(stage: &Path) {
    let entry = stage.join("main.align");
    let helper = stage.join("helper.align");
    let main = "import helper\nfn main() { mut a: array_builder<i64> := array_builder(); a.push(7); print(helper.read(a)); print(helper.generic(a)); mut rows: array_builder<helper.Row> := array_builder(); rows.push(helper.Row { value: Some(17) }); row := helper.record(rows); finished := rows.build(); print(row.value else 0) }\n";
    let helper_source = |ty: &str| format!("module helper\npub Row {{ value: Option<i64> }}\npub fn record(borrow rows: array_builder<Row>) -> Row = rows[0]\npub fn read(borrow a: array_builder<i64>) -> {ty} {{ value: {ty} := a[0] as {ty}; return value }}\npub fn generic<T: RegionPlain>(borrow a: array_builder<T>) -> T = a[0]\n");
    for per_unit in [false, true] {
        if let Some(output) = execute(stage, &[("main.align", main), ("helper.align", &helper_source("i64"))], per_unit) {
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n7\n17\n");
        }
    }
    let context = align_driver::CacheContext::at(stage.join("cache"));
    let mut snapshots = Vec::new();
    for (round, ty) in ["i64", "i64", "i32", "i64"].into_iter().enumerate() {
        std::fs::write(&helper, helper_source(ty)).unwrap();
        let mut sources = SourceMap::new();
        let mut built = align_driver::build_package(&mut sources, entry.to_str().unwrap(), main, &context, align_driver::UnitReuse::Allowed);
        assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &built.diags));
        for unit in ["helper", "main"] {
            let item = built.units.iter().find(|item| item.unit == unit).unwrap();
            assert_eq!(item.frontend.as_ref().unwrap().hit, matches!(round, 1 | 3), "{round}/{unit}");
        }
        let mut snapshot = Vec::new();
        let mut objects = Vec::new();
        let mut libraries = Vec::new();
        for index in 0..built.units.len() {
            let program = built.materialize(index).unwrap();
            snapshot.push(align_mir::print::program_to_string(&program));
            if backend_available() {
                let object = stage.join(format!("cache-{round}-{index}.o"));
                emit_object_file(&program, &object, BuildTarget::Baseline, Profile::Release, &[], false).unwrap();
                objects.push(object);
                for library in &program.link_libs {
                    if !libraries.contains(library) { libraries.push(library.clone()); }
                }
            }
        }
        match round {
            1 | 3 => assert_eq!(snapshot, snapshots[0]),
            2 => assert_ne!(snapshot, snapshots[0]),
            _ => {}
        }
        snapshots.push(snapshot);
        if backend_available() {
            let executable = stage.join(format!("cache-run-{round}"));
            link_objects(&align_driver::CDriver::default(), &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
                &executable, &libraries, Profile::Release).unwrap();
            let output = std::process::Command::new(executable).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n7\n17\n");
        }
    }
    let bad = "import helper\nfn main() { arena out { mut a: array_builder<str> := array_builder(out); a.push(\"view\"); print(helper.generic(a)) } }\n";
    let mut sources = SourceMap::new();
    assert!(check(&mut sources, entry.to_str().unwrap(), bad).diags.has_errors());
}
