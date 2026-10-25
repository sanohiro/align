//! Plan210: unfinished text-prefix authority and generation retirement.
mod common;
use common::*;
use align_driver::{backend_available, build_per_unit, check, emit_object_file, link_objects,
    lower_to_mir, BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

fn verdict(label: &str, source: &str, rejected: bool) {
    let files = [("main.align", source)];
    let result = diff_check_multi(label, &files, "main.align");
    assert_eq!(result.whole_errors, result.per_unit_errors, "{source}\nwhole: {}\nper-unit: {}", result.whole_diags, result.per_unit_diags);
    assert_eq!(result.whole_errors, rejected, "{source}\n{}", result.whole_diags);
}

#[test]
fn builder_bytes_formation_and_readonly_authority() {
    for receiver in ["b", "fresh()", "builder()", "if true { b } else { builder() }", "{ b }"] {
        let source = format!("fn fresh() -> builder = builder()\nfn main() {{ b := builder(); bytes := ({receiver}).bytes(); print(bytes.len()) }}");
        verdict("builder-bytes-receiver", &source, receiver != "b");
    }
    for mode in ["", "borrow ", "borrow mut "] {
        let source = format!("fn prefix({mode}b: builder) -> i64 {{ bytes := b.bytes(); bytes.len() }}\nfn main() {{}}");
        verdict("builder-bytes-parameter", &source, false);
    }
    for body in ["b.bytes(1)", "{ mut bytes := b.bytes(); bytes[0] = 0 as u8 }",
        "{ mut bytes := returned(b); bytes[0] = 0 as u8 }",
        "{ mut bytes := b.bytes()[0..1]; bytes.fill(0 as u8) }",
        "{ mut bytes: slice<u32> := b.bytes().view_le() else { return }; bytes[0] = 0 as u32 }",
        "{ mut bytes := b.bytes(); unsafe { bytes[0] = 0 as u8 } }",
        "{ mut bytes := returned(b); mutate(bytes) }"] {
        let source = format!("fn returned(borrow b: builder) -> slice<u8> = b.bytes()\nfn mutate(borrow mut x: slice<u8>) {{ x.fill(0 as u8) }}\nfn main() {{ mut b := builder(); b.write(\"abcdefgh\"); {body} }}");
        verdict("builder-bytes-readonly", &source, true);
    }
    verdict("builder-bytes-reader", "fn prefix(borrow b: builder) -> slice<u8> = b.bytes()\nfn read(x: slice<u8>) -> i64 = x.len()\nfn main() { mut b := builder(); b.write(\"hello\"); f := read; print(f(prefix(b))); print(b.bytes()[0]) }", false);
    for (carry, observe) in [
        ("holder := Holder { view: bytes }", "holder.view"),
    ] {
        for (action, tail, rejected) in [
            ("", "print(alias.len())", false),
            ("", "alias.fill(0)", true),
            ("b.write(\"x\")", "print(alias.len())", true),
        ] {
            let source = format!("Holder {{ view: slice<u8> }}\nfn main() {{ mut b := builder(); b.write(\"hello\"); bytes := b.bytes(); {carry}; mut alias := {observe}; {action}; {tail} }}");
            verdict("builder-bytes-carrier", &source, rejected);
        }
    }
    for (inside, action, rejected) in [
        ("bytes.len()", "", false),
        ("mut alias := bytes; alias.fill(0); bytes.len()", "", true),
        ("bytes.len()", "b.write(\"x\")", true),
    ] {
        let source = format!("fn main() {{ mut b := builder(); b.write(\"hello\"); bytes := b.bytes(); read := fn ignored: i64 {{ {inside} }}; {action}; print(read(0)) }}");
        verdict("builder-bytes-capture", &source, rejected);
    }
}

#[test]
fn builder_bytes_each_write_ends_only_its_own_views() {
    for write in ["write(\"\")", "write(\"x\")", "write_int(1)", "write_bool(true)", "write_char('a')", "write_float(1.5)"] {
        for call in [false, true] {
            for (tail, rejected) in [("print(old.len())", true), ("print(b.bytes().len())", false), ("print(independent.len())", false)] {
                let operation = if call { "append(b)".to_string() } else { format!("b.{write}") };
                let source = format!("fn append(borrow mut target: builder) {{ target.{write} }}\nfn main() {{ mut b := builder(); b.write(\"hello\"); other := builder(); old := b.bytes(); independent := other.bytes(); {operation}; {tail} }}");
                verdict("builder-bytes-writes", &source, rejected);
            }
        }
    }
}

#[test]
fn builder_bytes_imported_calls_keep_authority_and_generations() {
    for writer in ["write(\"\")", "write(\"x\")", "write_int(1)", "write_bool(true)", "write_char('a')", "write_float(1.5)"] {
        for generic in [false, true] {
            let parameter = if generic { "<T: RegionPlain>" } else { "" };
            let argument = if generic { ", ignored: T" } else { "" };
            let actual = if generic { ", true" } else { "" };
            let helper = format!("module helper\npub fn prefix{parameter}(borrow b: builder{argument}) -> slice<u8> = b.bytes()\npub fn append{parameter}(borrow mut b: builder{argument}) {{ b.{writer} }}\n");
            for (tail, rejected) in [
                (format!("helper.append(b{actual}); print(old.len())"), true),
                (format!("helper.append(b{actual}); print(helper.prefix(b{actual}).len())"), false),
                (format!("helper.append(other{actual}); print(old.len())"), false),
                ("old.fill(0)".to_string(), true),
            ] {
                let source = format!("import helper\nfn main() {{ mut b := builder(); b.write(\"hello\"); mut other := builder(); mut old := helper.prefix(b{actual}); {tail} }}\n");
                let files = [("main.align", source.as_str()), ("helper.align", helper.as_str())];
                let result = diff_check_multi("builder-bytes-imported-authority", &files, "main.align");
                assert_eq!(result.whole_errors, rejected, "{source}\n{}", result.whole_diags);
                assert_eq!(result.per_unit_errors, rejected, "{source}\n{}", result.per_unit_diags);
            }
        }
    }
}

#[test]
fn builder_bytes_selected_borrowed_storage_and_eager_order() {
    for receiver in ["a", "{ a }", "if flag { a } else { b }", "match 0 { 0 => a, _ => b }", "unsafe { if flag { a } else { b } }", "if flag { a } else { builder() }"] {
        for (argument, tail, rejected) in [("\"x\"", "print(old.len())", true),
            ("\"x\"", "print(independent.len())", false),
            ("old.as_str() else { return }", "", true),
            ("{ return; \"x\" }", "", false),
            ("{ a = builder(); \"x\" }", "", true)] {
            let source = format!("fn work(borrow mut a: builder, borrow mut b: builder, borrow c: builder, flag: bool) {{ old := a.bytes(); independent := c.bytes(); ({receiver}).write({argument}); {tail} }}\nfn main() {{}}");
            verdict("builder-bytes-selected", &source, rejected);
        }
    }
}

#[test]
fn builder_bytes_move_finish_replacement_and_escape() {
    for action in ["finished := b.to_string()", "moved := b", "b = builder()", "consume(b)"] {
        let source = format!("fn consume(b: builder) {{}}\nfn main() {{ mut b := builder(); b.write(\"x\"); old := b.bytes(); {action}; print(old.len()) }}");
        verdict("builder-bytes-retire", &source, true);
    }
    for expression in ["loop { b := builder(); break b.bytes() }", "arena { b := builder(); b.bytes() }"] {
        verdict("builder-bytes-escape", &format!("fn main() {{ bytes := {expression}; print(bytes.len()) }}"), true);
    }
    verdict("builder-bytes-local-return", "fn bad() -> slice<u8> { b := builder(); b.bytes() }\nfn main() {}", true);
    verdict("builder-bytes-independent-copy", "fn main() { mut b := builder(); b.write(\"hello\"); old := b.bytes(); first := old[0]; text := old.as_str() else { return }; copy := text.clone(); b.write(copy); b.write_int(first); print(copy) }", false);
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


#[test]
fn builder_bytes_control_flow_and_disjoint_validation() {
    for action in ["if flag { b.write(\"x\") }", "match 0 { 0 => b.write(\"x\"), _ => {} }",
        "value: Option<i64> := None; x := value else { b.write(\"x\"); return Ok(()) }",
        "b.write(\"x\"); return Err(Error.Invalid)",
        "b.write(\"x\"); answer: Result<(), Error> := Err(Error.Invalid); answer?",
        "b.write(\"x\"); answer: Result<(), Error> := Err(Error.Invalid); answer.map_err(fn e: Error { e })?",
        "mut i := 0; loop { b.write(\"x\"); i = i + 1; if i == 2 { break } }"] {
        for (source, rejected) in [("a", true), ("other", false)] {
            let program = format!("fn update(borrow mut b: builder, flag: bool) -> Result<(), Error> {{ {action}; result_value: Result<(), Error> := Ok(()); result_value }}\nfn main() {{ mut a := builder(); a.write(\"one\"); other := builder(); old := {source}.bytes().as_str() else {{ return }}; update(a, true) else {{}}; print(old) }}");
            verdict("builder-bytes-control", &program, rejected);
        }
    }
    verdict("builder-bytes-disjoint-loop", "fn update(borrow mut b: builder) { b.write(\"x\") }\nfn main() { mut source := builder(); source.write(\"hello\"); mut target := builder(); mut i := 0; loop { text := source.bytes().as_str() else { return }; update(target); print(text); i = i + 1; if i == 2 { break } } }", false);
}

#[test]
fn builder_bytes_native_imports_and_cache() {
    owned_fixture::run("builder_bytes_native_imports_and_cache", builder_bytes_native_imports_and_cache_in);
}
fn builder_bytes_native_imports_and_cache_in(stage: &Path) {
    let main = r#"import helper
fn main() {
  mut b := builder(1)
  print(helper.prefix(b, 0).len())
  b.write("é\0")
  b.write_int(-42)
  b.write_bool(true)
  b.write_char('界')
  b.write_float(1.5)
  bytes := helper.prefix(b, 0)
  print(bytes.len())
  text := bytes.as_str() else { return }
  print(text)
  copy := text.clone()
  b.write(copy)
  print(b.bytes().len())
  helper.append(b)
  print(helper.prefix(b, true).len())
  arena {
    mut local := builder()
    local.write("local")
    print(helper.prefix(local, 0).len())
  }
}
"#;
    let helper = |view: &str| format!("module helper\npub fn prefix<T: RegionPlain>(borrow b: builder, ignored: T) -> slice<u8> = {view}\npub fn append(borrow mut b: builder) {{ b.write(\"!\") }}\n");
    for per_unit in [false, true] {
        if let Some(output) = execute(stage, &[("main.align", main), ("helper.align", &helper("b.bytes()"))], per_unit) {
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(output.stdout, "0\n16\né\0-42true界1.5\n32\n33\n5\n".as_bytes());
        }
    }
    let cache = align_driver::CacheContext::at(stage.join("cache"));
    let entry = stage.join("main.align");
    let mut first = Vec::new();
    for (round, view) in ["b.bytes()", "b.bytes()", "b.bytes()[0..0]", "b.bytes()"].into_iter().enumerate() {
        std::fs::write(stage.join("helper.align"), helper(view)).unwrap();
        let mut sources = SourceMap::new();
        let mut package = align_driver::build_package(&mut sources, entry.to_str().unwrap(), main,
            &cache, align_driver::UnitReuse::Allowed);
        assert!(!package.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &package.diags));
        let unit = package.units.iter().find(|unit| unit.unit == "helper").unwrap();
        assert_eq!(unit.frontend.as_ref().unwrap().hit, matches!(round, 1 | 3));
        let mut printed = Vec::new();
        for index in 0..package.units.len() {
            printed.push(align_mir::print::program_to_string(&package.materialize(index).unwrap()));
        }
        match round { 0 => first = printed, 1 | 3 => assert_eq!(first, printed), _ => assert_ne!(first, printed) }
    }
}

#[test]
fn builder_bytes_dead_reads_emit_in_both_modes() {
    owned_fixture::run("builder_bytes_dead_reads_emit_in_both_modes", |stage| {
        let source = "fn main() { mut text := builder(); text.write(\"x\"); mut numbers: array_builder<i64> := array_builder(); numbers.push(7); flag := false; if false { print(text.bytes().len()); print(numbers[0]) }; if flag { print(text.bytes().len()); print(numbers[0]) }; loop { if false { print(text.bytes().len()) }; break }; print(0) }";
        for per_unit in [false, true] {
            if let Some(output) = execute(stage, &[("main.align", source)], per_unit) {
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                assert_eq!(output.stdout, b"0\n");
            }
        }
    });
}

#[test]
fn builder_bytes_selected_cleanup_keeps_live_original_owner() {
    verdict("builder-bytes-mixed-disjoint", "fn apply(borrow mut b: builder, borrow c: builder, flag: bool) { old := c.bytes().as_str() else { return }; (if flag { b } else { builder() }).write(\"x\"); print(old); print(b.bytes().len()) }\nfn main() {}", false);
    owned_fixture::run("builder_bytes_selected_cleanup_keeps_live_original_owner", |stage| {
        let source = "fn apply(borrow mut b: builder, flag: bool) { (if flag { b } else { builder() }).write(\"x\"); print(b.bytes().len()) }\nfn main() { mut b := builder(); apply(b, false); apply(b, true); print(b.bytes().len()); (if false { b } else { builder() }).write(\"y\"); print(b.bytes().len()) }";
        for per_unit in [false, true] {
            if let Some(output) = execute(stage, &[("main.align", source)], per_unit) {
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                assert_eq!(output.stdout, b"0\n1\n1\n1\n");
            }
        }
    });
}

#[test]
fn builder_bytes_opaque_calls_preserve_disjoint_validation_in_loops() {
    for (owner, constructor, mutation) in [
        ("builder", "builder()", "target.write(\"x\")"),
        ("array_builder<i64>", "array_builder()", "target.push(0)"),
    ] {
        for source in ["buffer(8)", "builder()"] {
            let initialize = if source.starts_with("buffer") { "source.append(\"hi\".bytes())" } else { "source.write(\"hi\")" };
            let program = format!("fn append(borrow mut target: {owner}) {{ {mutation} }}\nfn main() -> Result<(), Error> {{ mut source := {source}; {initialize}; mut target: {owner} := {constructor}; mut i := 0; loop {{ if i == 2 {{ break }}; text := source.bytes().as_str()?; append(target); print(text); i = i + 1 }}; Ok(()) }}");
            verdict("builder-bytes-opaque-loop", &program, false);
        }
    }
    verdict("builder-bytes-real-loop-write", "fn append(borrow mut target: builder) { target.write(\"x\") }\nfn main() { mut target := builder(); mut i := 0; loop { text := target.bytes().as_str() else { return }; append(target); print(text); i = i + 1; if i == 2 { break } } }", true);
}
