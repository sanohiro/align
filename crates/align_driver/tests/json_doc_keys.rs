//! Linear JSON object keys preserve ordered borrowed views in explicit arenas.
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

#[test]
fn json_doc_keys_lifecycle_and_replay() {
    owned_fixture::run("json_doc_keys_lifecycle_and_replay", |stage| {
        let helper = r#"module helper
import core.json
pub fn total<T>(d: json.doc, marker: T) -> i64 {
  arena {
    keys := d.keys()
    mut sum: i64 := 0
    mut i: i64 := 0
    loop { if i == keys.len() { break }; sum = sum + keys[i].len(); i = i + 1 }
    return sum
  }
}
pub fn parsed(text: str) -> Result<i64, Error> {
  arena { d := json.doc(text)?; return Ok(total(d, true)) }
}
pub fn choose(d: json.doc, first: bool) -> i64 {
  arena {
    keys := if first { d.keys() } else { d.keys() }
    again := match Some(first) { Some(_) => d.keys(), None => d.keys() }
    last := loop { break keys.len() }
    return keys.len() + again.len() + last
  }
}
"#;
        let main = r#"import core.json
import helper
fn mapped(error: Error) -> Error = error
fn require(value: bool) -> Result<(), Error> { if value { return Ok(()) }; return Err(Error.Invalid) }
fn exercise() -> Result<(), Error> {
  input := "{\"a\":1,\"\\u0061\":2,\"\\ud83d\\ude00\":3,\"\":4,\"nested\":{\"skip\":[1,2]},\"last\":6}".clone()
  arena {
    d := json.doc(input)?
    keys := d.keys()
    values := d.elems()
    require(keys.len() == 6)?
    require(values.len() == 6)?
    require(keys[0] == "a")?
    require(keys[1] == "a")?
    require(keys[2] == "😀")?
    require(keys[3] == "")?
    require(keys[4] == "nested")?
    require(keys[5] == "last")?
    require((values[0].as_i64() else 0) == 1)?
    require((values[1].as_i64() else 0) == 2)?
    require((values[5].as_i64() else 0) == 6)?
    require(helper.total(d, true) == 16)?
    require(helper.choose(d, true) == 18)?
    require(helper.choose(d, false) == 18)?
    arena { nested := d.keys(); require(nested[2] == "😀")? }
    require(d.keys()[1] == "a")?
    require(d.get("absent").keys().len() == 0)?
    require(values[0].keys().len() == 0)?
    require((d.get("a").as_i64() else 0) == 1)?
    require((json.doc("{}").map_err(mapped)?).keys().len() == 0)?
    fallback := json.doc("[") else d
    require(fallback.keys().len() == 6)?
  }
  require(helper.parsed(input)? == 16)?
  require(match helper.parsed("{") { Err(_) => true, Ok(_) => false })?
  return Ok(())
}
fn main() -> i32 { return match exercise() { Ok(_) => 0, Err(_) => 1 } }
"#.to_string();
        std::fs::write(stage.join("helper.align"), helper).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, &main).unwrap();
        let entry = entry.to_str().unwrap();
        let mut sources = align_span::SourceMap::new();
        let checked = align_driver::check(&mut sources, entry, &main);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let whole = align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[whole], "whole");
        let mut sources = align_span::SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry, &main);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &built.diags)
        );
        execute(
            stage,
            &built
                .units
                .into_iter()
                .map(|unit| unit.mir)
                .collect::<Vec<_>>(),
            "units",
        );
        let cache = align_driver::CacheContext::at(stage.join("cache"));
        for round in 0..2 {
            let mut sources = align_span::SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sources,
                entry,
                &main,
                &cache,
                align_driver::UnitReuse::Allowed,
            );
            assert!(
                !built.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &built.diags)
            );
            assert_eq!(built.units.len(), 2);
            assert!(
                built
                    .units
                    .iter()
                    .all(|unit| unit.frontend.as_ref().unwrap().hit == (round == 1))
            );
            let programs = (0..built.units.len())
                .map(|i| built.materialize(i).unwrap().clone())
                .collect::<Vec<_>>();
            if round == 1 {
                execute(stage, &programs, "cache-hit")
            }
        }
    });
}

fn execute(stage: &std::path::Path, programs: &[align_mir::Program], label: &str) {
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (i, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{i}.o"));
        align_driver::emit_object_file(
            program,
            &object,
            align_driver::BuildTarget::Baseline,
            align_driver::Profile::Release,
            &[],
            false,
        )
        .unwrap();
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
    }
    let binary = stage.join(label);
    align_driver::link_objects(
        &align_driver::CDriver::default(),
        &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        &binary,
        &libraries,
        align_driver::Profile::Release,
    )
    .unwrap();
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(
        output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
}

#[test]
fn json_doc_keys_rejects_invalid_source_and_escaping_views() {
    owned_fixture::run(
        "json_doc_keys_rejects_invalid_source_and_escaping_views",
        |stage| {
            let diagnostics = [
                "materializes a document level in an arena",
                "takes no arguments",
                "unknown method '.keys()'",
                "cannot escape",
                "cannot escape",
                "invalidated",
                "source 'input' was moved",
                "immutable 'keys'",
                "undefined name",
            ];
            for (source, diagnostic) in [
            "fn bad(d:json.doc) { d.keys() }",
            "fn bad(d:json.doc) { arena { d.keys(0) } }",
            "fn bad() { arena { 3.keys() } }",
            "fn bad(d:json.doc)->slice<str> = arena { d.keys() }",
            "fn bad(d:json.doc)->str = arena { d.keys()[0] }",
            "fn bad(d:json.doc) { arena { mut escaped:=d.keys(); arena { escaped=d.keys() }; print(escaped.len()) } }",
            "fn take(input:string) {}\nfn bad()->Result<(),Error> { input:=\"{\\\"a\\\":1}\".clone(); arena { d:=json.doc(input)?; keys:=d.keys(); take(input); print(keys[0]) }; return Ok(()) }",
            "fn bad(borrow keys:slice<str>) { keys[0]=\"changed\" }",
            "fn bad() { arena { missing.keys() } }",
        ].into_iter().zip(diagnostics) {
            let source = format!("import core.json\n{source}\nfn main() {{}}\n");
            let entry = stage.join("main.align");
            std::fs::write(&entry, &source).unwrap();
            let entry = entry.to_str().unwrap();
            let mut sources = align_span::SourceMap::new();
            let whole = align_driver::check(&mut sources, entry, &source);
            assert!(whole.diags.has_errors(), "whole-program accepted {source}");
            assert!(align_driver::format_diagnostics(&sources, &whole.diags).contains(diagnostic), "wrong rejection for {source}");
            let mut sources = align_span::SourceMap::new();
            let units = align_driver::check_per_unit(&mut sources, entry, &source);
            assert!(units.diags.has_errors(), "per-unit accepted {source}");
            assert!(align_driver::format_diagnostics(&sources, &units.diags).contains(diagnostic), "wrong per-unit rejection for {source}");
        }
        },
    );
}
