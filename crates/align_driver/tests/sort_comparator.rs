//! Plan208: stable, sequential comparator calls share the ordinary Copy sort storage.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
mod common;
use common::*;

fn ints(values: &[i64]) -> String {
    format!("[{}]", values.iter().map(i64::to_string).collect::<Vec<_>>().join(","))
}

const ORDERING: &str = r#"module ordering
pub fn before<T: Ord>(a: T, b: T) -> bool = a < b
pub fn integer(a:i64,b:i64)->bool = before(a,b)
pub fn character(a:char,b:char)->bool = before(a,b)
pub fn text(a:str,b:str)->bool = before(a,b)
pub fn floating(a:f64,b:f64)->bool = before(a,b)
pub fn sorted(xs: slice<i64>) -> array<i64> = xs.sort_by(integer)
pub fn stable(xs: slice<i64>) -> array<i64> = xs.sort_by(fn a: i64, b: i64 { a % 7 < b % 7 })
pub fn agrees(xs: slice<i64>, expected: slice<i64>) -> bool {
  result := sorted(xs)
  if result.len() != expected.len() { return false }
  mut i := 0
  loop { if i == result.len() { break }; if result[i] != expected[i] { return false }; i = i + 1 }
  return true
}
pub fn permutation(xs: slice<i64>, cyclic: bool) -> bool {
  result := xs.sort_by(fn a: i64, b: i64 { if cyclic { (a % 3 + 1) % 3 == b % 3 } else { true } })
  normalized := result.sort()
  if normalized.len() != xs.len() { return false }
  mut i := 0
  loop { if i == xs.len() { break }; if normalized[i] != i { return false }; i = i + 1 }
  return true
}
"#;

fn comparator_sort_order_and_stability_body() {
    if !backend_available() { return; }
    let mut source = String::from("module main\nimport ordering\nfn main() {\n");
    let mut expected = String::new();
    for n in [0, 1, 2, 31, 32, 33, 63, 64, 65, 129] {
        let values: Vec<_> = (0..n).map(|i| (i * 37 + 11) % 19 - 9).collect();
        let mut ordered = values.clone();
        ordered.sort();
        let view = |xs: &[i64]| if xs.is_empty() { "[0][0..0]".to_string() } else { format!("{}[..]", ints(xs)) };
        for input in [values, ordered.clone(), ordered.iter().rev().copied().collect()] {
            source.push_str(&format!("print(ordering.agrees({}, {}))\n", view(&input), view(&ordered)));
            expected.push_str("true\n");
        }
        let ids: Vec<_> = (0..n).collect();
        for cyclic in [false, true] {
            source.push_str(&format!("print(ordering.permutation({}, {cyclic}))\n", view(&ids)));
            expected.push_str("true\n");
        }
    }
    let ids: Vec<_> = (0..129).rev().collect();
    let mut stable = ids.clone();
    stable.sort_by_key(|i| i % 7);
    source.push_str(&format!("stable := ordering.stable({}[..])\n", ints(&ids)));
    for (i, value) in stable.iter().enumerate() {
        source.push_str(&format!("print(stable[{i}] == {value})\n"));
        expected.push_str("true\n");
    }
    source.push_str(r#"  text := "zab"
  starts := [0, 1, 2]
  ends := [1, 2, 3]
  // Real Request27 shape: compare borrowed spans using index values and Copy captures.
  spans := [0, 2, 1].sort_by(fn a: i64, b: i64 { text[starts[a]..ends[a]] < text[starts[b]..ends[b]] })
  print(spans[0]); print(spans[1]); print(spans[2])
  labels := ["b", "a", "a", "b"]
  weights := [1, 2, 1, 0]
  multi := [0, 1, 2, 3].sort_by(fn a: i64, b: i64 { if labels[a] == labels[b] { weights[a] < weights[b] } else { labels[a] < labels[b] } })
  print(multi[0]); print(multi[1]); print(multi[2]); print(multi[3])
  flags := [true, false, true].sort_by(fn a: bool, b: bool { !a && b })
  print(flags[0]); print(flags[1]); print(flags[2])
  letters := ['中', 'a', 'é'].sort_by(ordering.character)
  print(letters[0]); print(letters[1]); print(letters[2])
  words := ["z", "a\0b", "a", "é"].sort_by(ordering.text)
  print(words[0]); print(words[1]); print(words[2]); print(words[3])
  numbers := [2.5, -0.5, 1.0].sort_by(ordering.floating)
  print(numbers[0] == -0.5); print(numbers[2] == 2.5)
  nan := 0.0 / 0.0
  floats := [nan, 2.0, -1.0].sort_by(fn a: f64, b: f64 { if a != a { false } else { if b != b { true } else { a < b } } })
  print(floats[0] == -1.0); print(floats[1] == 2.0); print(floats[2] != floats[2])
  arena {
    stages := [3, 0, 2, 1].where(fn n { n > 0 }).map(fn n { n * 2 }).sort_by(fn a, b { a > b })
    print(stages[0]); print(stages[2])
    again := stages.sort_by(ordering.integer)
    print(again[0]); print(again[2])
  }
}
"#);
    expected.push_str("1\n2\n0\n2\n1\n3\n0\nfalse\ntrue\ntrue\na\né\n中\na\na\0b\nz\né\ntrue\ntrue\ntrue\ntrue\ntrue\n6\n2\n2\n6\n");
    let files = [("main.align", source.as_str()), ("ordering.align", ORDERING)];
    for output in [build_and_run_multi("comparator-whole", &files, "main.align"),
        build_per_unit_multi("comparator-units", &files, "main.align").link_and_run()] {
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
    // The cached frontend must retain imported generic comparator instances and lifted captures.
    let project = Proj::new("comparator-cache", &files, "main.align");
    let entry = project.dir.join("main.align");
    let cache = project.cache();
    for round in 0..2 {
        let mut map = SourceMap::new();
        let mut built = align_driver::build_package(&mut map, entry.to_str().unwrap(), &source, &cache, align_driver::UnitReuse::Allowed);
        assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&map, &built.diags));
        assert_eq!(built.units.len(), 2);
        let mut objects = Vec::new();
        let mut libraries = Vec::new();
        for index in 0..built.units.len() {
            assert_eq!(built.units[index].frontend.as_ref().unwrap().hit, round == 1);
            let mir = built.materialize(index).unwrap();
            assert!(!mir.fns.is_empty());
            if round == 1 {
                let object = project.dir.join(format!("cached-{index}.o"));
                emit_object_file(mir, &object, BuildTarget::Baseline, Profile::Release, &[], false).unwrap();
                objects.push(object);
                for library in &mir.link_libs {
                    if !libraries.contains(library) { libraries.push(library.clone()); }
                }
            }
        }
        if round == 1 {
            let binary = project.dir.join("cache-hit");
            link_objects(&align_driver::CDriver::default(), &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(), &binary, &libraries, Profile::Release).unwrap();
            let output = std::process::Command::new(binary).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
        }
    }
}

fn sort_comparator_effects_body() {
    let helpers = r#"module ordering
pub fn noisy(a: i64, b: i64) -> bool { print(a); print(b); return a < b }
pub fn compare(a: i64, b: i64) -> bool = noisy(a, b)
"#;
    let mut source = String::from("module main\nimport ordering\nfn main() {\n");
    let values: Vec<_> = (1..=32).chain([0]).collect();
    source.push_str(&format!("sorted := {}.map(fn n {{ print(n + 100); n }}).sort_by(ordering.compare)\nprint(sorted[0]); print(sorted[32])\n", ints(&values)));
    source.push_str("empty := [0][0..0].sort_by(ordering.compare)\none := [5].sort_by(ordering.compare)\nprint(empty.len()); print(one[0])\n}\n");
    let mut expected = values.iter().map(|v| format!("{}\n", v + 100)).collect::<String>();
    // Independent fixed trace: one increasing 32-run, one singleton, then one right-head choice.
    for i in 2..=32 { expected.push_str(&format!("{i}\n{}\n", i - 1)); }
    expected.push_str("0\n1\n0\n32\n0\n5\n");
    if backend_available() {
        let files = [("main.align", source.as_str()), ("ordering.align", helpers)];
        for output in [build_and_run_multi("comparator-trace", &files, "main.align"), build_per_unit_multi("comparator-trace-unit", &files, "main.align").link_and_run()] {
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
        }
    }
    for body in [
        "[n, 0].sort_by(ordering.compare)",
        "[n, 0].sort_by(fn a, b { print(a); a < b })",
        "[n, 0].sort_by(fn a, b { f := callback; f(a) < f(b) })",
    ] {
        let source = format!("module main\nimport ordering\nfn noisy(n:i64)->i64 {{ print(n); return n }}\nfn work(n:i64)->i64 {{ callback := noisy; sorted := {body}; return sorted[0] }}\nfn main() {{ print([1,2].par_map(work).sum()) }}");
        let checked = diff_check_multi("comparator-parallel-effect", &[("main.align", &source), ("ordering.align", helpers)], "main.align");
        assert!(checked.whole_errors && checked.per_unit_errors, "{}\n{}", checked.whole_diags, checked.per_unit_diags);
        assert!(checked.whole_diags.contains("Pure") && checked.per_unit_diags.contains("Pure"), "{}\n{}", checked.whole_diags, checked.per_unit_diags);
    }
}

fn sort_comparator_domain_body() {
    for source in [
        "fn main() { x := [1,2].sort_by() }",
        "fn main() { x := [1,2].sort_by(fn a { true }) }",
        "fn main() { x := [1,2].sort_by(fn a,b { a + b }) }",
        "fn wrong(a:i64,b:str)->bool = true\nfn main() { x := [1,2].sort_by(wrong) }",
        "fn wrong(borrow a:i64,b:i64)->bool = a < b\nfn main() { x := [1,2].sort_by(wrong) }",
        "fn wrong(a:i64,borrow b:i64)->bool = a < b\nfn main() { x := [1,2].sort_by(wrong) }",
        "import core.json\nRow { value:i64 }\nfn main() { rows:json.scanner<Row>:=json.scan(\"[]\"); x:=rows.value.sort_by(fn a,b { a < b }) }",
        "fn bad(xs:array<string>) { x := xs.sort_by(fn a,b { true }) }\nfn main() {}",
        "Row { value:i64 }\nfn main() { x := [Row { value:1 }].sort_by(fn a,b { true }) }",
        "fn main() { x := missing.sort_by(fn a,b { true }) }",
    ] {
        let checked = diff_check_multi("comparator-invalid", &[("main.align", source)], "main.align");
        assert!(checked.whole_errors && checked.per_unit_errors, "{source}\n{}\n{}", checked.whole_diags, checked.per_unit_diags);
    }
}

fn comparator_sort_lifetimes_and_control_body() {
    let terminal = "sort_by(fn a:str,b:str { a < b })";
    for source in [
        format!("fn bad()->array<str> {{ owner:=\"text\".clone(); return [owner.trim()].{terminal} }}\nfn main() {{}}"),
        format!("fn bad()->array<str> {{ owner:=\"text\".clone(); view:str:=owner; return [1,2].map(fn n:i64 {{ view }}).{terminal} }}\nfn main() {{}}"),
        format!("fn main() {{ mut owner:=\"text\".clone(); sorted:=[owner.trim()].{terminal}; owner=\"new\".clone(); print(sorted[0]) }}"),
        format!("fn take(s:string) {{}}\nfn main() {{ owner:=\"text\".clone(); sorted:=[owner.trim()].{terminal}; take(owner); print(sorted[0]) }}"),
        format!("fn main() {{ mut source:=[\"z\",\"a\"].to_array(); sorted:=source.{terminal}; source=[\"x\"].to_array(); print(sorted[0]) }}"),
        format!("fn main() {{ mut builder:array_builder<string>:=array_builder(); builder.push(\"z\".clone()); mut owners:=builder.build(); sorted:=[owners[0]].{terminal}; owners.truncate(0); print(sorted[0]) }}"),
        format!("fn main() {{ mut keep:=[\"static\"].{terminal}; loop {{ owner:=\"text\".clone(); keep=[owner.trim()].{terminal}; break }}; print(keep[0]) }}"),
        format!("fn bad()->array<str> = arena {{ [\"x\"].{terminal} }}\nfn main() {{}}"),
        "fn main() { mut saved:=[\"static\"]; { owner:=\"local\".clone(); view:str:=owner; target:=saved[..]; sorted:=[view,\"a\"].sort_by(fn a:str,b:str { mut destination:=target; destination[0]=a; a < b }); }; print(saved[0]) }".to_string(),
    ] {
        let checked = diff_check_multi("comparator-borrow", &[("main.align", &source)], "main.align");
        assert!(checked.whole_errors && checked.per_unit_errors, "{source}\n{}\n{}", checked.whole_diags, checked.per_unit_diags);
        assert!(checked.whole_diags.contains("borrow") || checked.whole_diags.contains("arena"), "{}", checked.whole_diags);
    }
    let source = r#"module main
fn before(a:str,b:str)->bool = a < b
fn sorted(xs:slice<str>)->array<str> = xs.sort_by(before)
fn result(xs:slice<str>)->Result<array<str>,Error> = Ok(sorted(xs))
fn independent()->array<i64> {
  owner := "local".clone()
  view:str := owner
  return [3,1,2].sort_by(fn a,b { if view.len() > 0 { a < b } else { a > b } })
}
fn early(flag:bool)->i64 {
  source := [3,2,1]
  result := (if flag { return 7 } else { source[..] }).sort_by(fn a,b { print("unexpected"); a < b })
  return result[0]
}
fn main()->Result<(),Error> {
  print(early(true))
  owner := "text".clone()
  source := [owner.trim(),"alpha"]
  selected := if true { sorted(source[..]) } else { sorted(source[..]) }
  tried := result(source[..]).map_err(fn e:Error { e })?
  fallback := result(source[..]) else { return Err(Error.Invalid) }
  optional:Option<array<str>> := Some(sorted(source[..]))
  matched := match optional { Some(value)=>value, None=>sorted(source[..]) }
  mut replacement := ["before"].sort_by(before)
  replacement = source.sort_by(before)
  loop { repeated := replacement.sort_by(before); print(repeated[0]); break }
  print(selected[0]); print(tried[0]); print(fallback[0]); print(matched[0])
  print(independent()[0])
  return Ok(())
}
"#;
    if backend_available() {
        let files = [("main.align", source)];
        for output in [build_and_run_multi("comparator-control", &files, "main.align"), build_per_unit_multi("comparator-control-unit", &files, "main.align").link_and_run()] {
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout), "7\nalpha\nalpha\nalpha\nalpha\nalpha\n1\n");
        }
    }
}

// Require the counter-enabled runtime archive used by the existing sort allocation owners.
const _: extern "C" fn() -> i64 = align_runtime::align_rt_alloc_count;
const _: extern "C" fn() -> i64 = align_runtime::align_rt_free_count;

fn comparator_sort_scratch_and_drop_body() {
    if !backend_available() { return; }
    let values: Vec<_> = (0..65).rev().collect();
    let source = format!(r#"extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
fn exercise(xs:slice<i64>)->i64 {{
  sorted := xs.sort_by(fn a:i64,b:i64 {{ a < b }})
  return sorted[0]
}}
fn measure(xs:slice<i64>,count:i64) {{
  before_alloc := unsafe {{ align_rt_alloc_count() }}
  before_free := unsafe {{ align_rt_free_count() }}
  first := exercise(xs)
  allocated := unsafe {{ align_rt_alloc_count() }} - before_alloc
  freed := unsafe {{ align_rt_free_count() }} - before_free
  print(first == xs[xs.len()-1]); print(allocated == count); print(freed == count)
}}
fn main() {{
  xs := {}
  measure(xs[0..1],1); measure(xs[0..32],1); measure(xs[0..33],2); measure(xs[..],2)
}}
"#, ints(&values));
    let output = build_and_run("comparator-scratch", &source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n".repeat(12));
}

fn comparator_capture_write_authority_body() {
    let legal = r#"fn write(out values:slice<i64>,n:i64) { values[0] = n }
fn main() {
  mut output := [0]
  target := output[..]
  sorted := [3,1,2].sort_by(fn a,b { mut captured:=target; write(captured,a); a < b })
  print(sorted[0]); print(output[0])
}
"#;
    let checked = diff_check_multi("comparator-independent-write", &[("main.align", legal)], "main.align");
    assert!(!checked.whole_errors && !checked.per_unit_errors, "{}\n{}", checked.whole_diags, checked.per_unit_diags);
    let illegal = r#"fn write(out values:slice<u8>) { values[0] = 122 }
fn main()->Result<(),Error> {
  mut bytes:[u8;1] := [97]
  view := bytes[..]
  text := view.as_str()?
  sorted := [text,"a"].sort_by(fn a:str,b:str { mut captured:=view; write(captured); a < b })
  print(sorted[0])
  return Ok(())
}
"#;
    for source in [
        illegal.to_string(),
        illegal.replace("sort_by(fn a:str,b:str { mut captured:=view; write(captured); a < b })", "sort_by_key(fn a:str { mut captured:=view; write(captured); a })"),
        illegal.replace("sort_by(fn a:str,b:str { mut captured:=view; write(captured); a < b })", "map(fn a:str { mut captured:=view; write(captured); a }).to_array()"),
        illegal.replace("[text,\"a\"].sort_by(fn a:str,b:str { mut captured:=view; write(captured); a < b })", "[1,2].sort_by(fn a,b { mut captured:=view; write(captured); text.len() > a - b })"),
    ] {
        let checked = diff_check_multi("comparator-source-write", &[("main.align", &source)], "main.align");
        assert!(checked.whole_errors && checked.per_unit_errors, "{source}\n{}\n{}", checked.whole_diags, checked.per_unit_diags);
        assert!(checked.whole_diags.contains("invalidated"), "{}", checked.whole_diags);
    }
    if backend_available() {
        let output = build_and_run("comparator-independent-write-run", legal);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n2\n");
    }
}

#[test]
fn comparator_sort_order_and_stability() {
    owned_fixture::run("comparator_sort_order_and_stability", |_| comparator_sort_order_and_stability_body());
}

#[test]
fn sort_comparator_effects() {
    owned_fixture::run("sort_comparator_effects", |_| sort_comparator_effects_body());
}

#[test]
fn sort_comparator_domain() {
    owned_fixture::run("sort_comparator_domain", |_| sort_comparator_domain_body());
}

#[test]
fn comparator_sort_lifetimes_and_control() {
    owned_fixture::run("comparator_sort_lifetimes_and_control", |_| comparator_sort_lifetimes_and_control_body());
}

#[test]
fn comparator_sort_scratch_and_drop() {
    owned_fixture::run("comparator_sort_scratch_and_drop", |_| comparator_sort_scratch_and_drop_body());
}

#[test]
fn comparator_capture_write_authority() {
    owned_fixture::run("comparator_capture_write_authority", |_| comparator_capture_write_authority_body());
}
