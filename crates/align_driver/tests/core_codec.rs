//! `core.codec` owner tests: canonical bytes, zero-copy access, ownership, checked-HIR, and the
//! alignment-independent LLVM lowering contract.

mod common;
use common::*;

#[cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

#[test]
#[cfg(unix)]
fn codec_column_projection_admission_matrix() {
    if !backend_available() {
        return;
    }
    owned_fixture::run("codec_column_projection_admission_matrix", |_| {
        let api = r#"module codec_column_api
import core.codec
pub fn integers(batch: codec.batch, index: i64) -> i64 {
  column := batch.i64s(index) else { return -1 }
  return column.len()
}
pub fn floats(batch: codec.batch, index: i64) -> i64 {
  column := batch.f64s(index) else { return -1 }
  return column.len()
}
pub fn flags(batch: codec.batch, index: i64) -> i64 {
  column := batch.bools(index) else { return -1 }
  return column.len()
}
pub fn texts(batch: codec.batch, index: i64) -> i64 {
  column := batch.strs(index) else { return -1 }
  return column.len()
}

pub fn verify(batch: codec.batch, rows: i64, columns: i64) -> bool {
  indices := [-1, 0, 1, 2, 3, 4, 9223372036854775807]
  mut step := 0
  loop {
    if step >= indices.len() { break }
    index := indices[step]
    a := if index == 0 && index < columns { rows } else { -1 }
    b := if index == 1 && index < columns { rows } else { -1 }
    c := if index == 2 && index < columns { rows } else { -1 }
    d := if index == 3 && index < columns { rows } else { -1 }
    if integers(batch, index) != a || floats(batch, index) != b
      || flags(batch, index) != c || texts(batch, index) != d { return false }
    step = step + 1
  }
  return true
}
"#;
        let main = r#"module main
import core.codec
import codec_column_api
fn main() -> Result<(), Error> {
  ids := [7, 9]
  scores := [1.5, -2.5]
  flags := [false, true]
  names := ["", "second"]
  mut full := codec.encoder(2)?
  full.put_i64("i", ids)?
  full.put_f64("f", scores)?
  full.put_bool("b", flags)?
  full.put_str("s", names)?
  full_bytes := full.finish()
  full_batch := codec.open(full_bytes.bytes())?
  if !codec_column_api.verify(full_batch, 2, 4) { return Err(Error.Invalid) }
  mut zero := codec.encoder(0)?
  zero.put_i64("i", ids[0..0])?
  zero.put_f64("f", scores[0..0])?
  zero.put_bool("b", flags[0..0])?
  zero.put_str("s", names[0..0])?
  zero_bytes := zero.finish()
  zero_batch := codec.open(zero_bytes.bytes())?
  if !codec_column_api.verify(zero_batch, 0, 4) { return Err(Error.Invalid) }
  empty := codec.encoder(0)?
  empty_bytes := empty.finish()
  empty_batch := codec.open(empty_bytes.bytes())?
  if !codec_column_api.verify(empty_batch, 0, 0) { return Err(Error.Invalid) }
  return Ok(())
}
"#;
        let files = [("codec_column_api.align", api), ("main.align", main)];
        for output in [
            build_and_run_multi("codec-column-admission-whole", &files, "main.align"),
            build_per_unit_multi("codec-column-admission-unit", &files, "main.align")
                .link_and_run(),
        ] {
            assert!(
                output.status.success(),
                "stdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    });
}

#[test]
#[cfg(unix)]
fn codec_batch_result_survives_imported_generic_cache_replay() {
    owned_fixture::run("codec_batch_result_survives_imported_generic_cache_replay", |stage| {
        let api = r#"module codec_api
import core.codec
pub Views { batch: codec.batch, ints: codec.i64_column, floats: codec.f64_column, flags: codec.bool_column, texts: codec.str_column }
pub Wrapped { values: Option<Views>, good: Result<codec.batch, string>, bad: Result<string, codec.batch> }
pub fn same(error: Error) -> Error = error
pub fn identity<T>(value: T) -> T = value
pub fn rows_or_len(borrow result: Result<codec.batch, string>) -> i64 = match result { Ok(batch) => batch.rows(), Err(message) => message.len() }
pub fn len_or_rows(borrow result: Result<string, codec.batch>) -> i64 = match result { Ok(message) => message.len(), Err(batch) => batch.rows() }
pub fn open<T>(marker: T, input: slice<u8>, first: bool) -> Result<codec.batch, Error> {
  batch := codec.open(input).map_err(same)?
  return if first { Ok(batch) } else { loop { break Ok(batch) } }
}
pub fn hold<T>(marker: T, batch: codec.batch) -> Result<Views, Error> {
  return Ok(Views {
    batch: batch,
    ints: batch.i64s(0) else { return Err(Error.Invalid) },
    floats: batch.f64s(1) else { return Err(Error.Invalid) },
    flags: batch.bools(2) else { return Err(Error.Invalid) },
    texts: batch.strs(3) else { return Err(Error.Invalid) }
  })
}
pub fn inspect(borrow holder: Wrapped, populated: bool) -> bool {
  good := match holder.good { Ok(batch) => batch.rows(), Err(message) => message.len() }
  bad := match holder.bad { Ok(message) => message.len(), Err(batch) => batch.rows() }
  if !populated { return good == 3 && bad == 3 }
  views := holder.values else { return false }
  return good == 2 && bad == 2 && views.batch.columns() == 4
    && (views.ints.at(1) else -1) == 9
    && (views.floats.at(1) else -1.0) == 2.5
    && (views.flags.at(1) else false)
    && (views.texts.at(1) else "") == "second"
}
"#;
        let main = r#"module main
import core.codec
import codec_api
fn main() -> Result<(), Error> {
  values := [7, 9]
  floats := [1.5, 2.5]
  flags := [false, true]
  texts := ["first", "second"]
  mut encoder := codec.encoder(2)?
  encoder.put_i64("id", values)?
  encoder.put_f64("float", floats)?
  encoder.put_bool("flag", flags)?
  encoder.put_str("text", texts)?
  owner := encoder.finish()
  batch := codec_api.open(0, owner.bytes(), true)?
  again := codec_api.open(false, owner.bytes(), false)?
  if batch.rows() != 2 || again.columns() != 4 { return Err(Error.Invalid) }
  views := codec_api.hold(false, again)?
  holder := codec_api.identity(codec_api.Wrapped { values: Some(views), good: Ok(batch), bad: Err(again) })
  absent := codec_api.Wrapped { values: None, good: Err("bad".clone()), bad: Ok("bad".clone()) }
  if !codec_api.inspect(holder, true) || !codec_api.inspect(absent, false) { return Err(Error.Invalid) }
  batch_ok: Result<codec.batch, string> := Ok(batch)
  batch_err: Result<string, codec.batch> := Err(again)
  text_err: Result<codec.batch, string> := Err("bad".clone())
  text_ok: Result<string, codec.batch> := Ok("bad".clone())
  if codec_api.rows_or_len(batch_ok) != 2 || codec_api.len_or_rows(batch_err) != 2 { return Err(Error.Invalid) }
  if codec_api.rows_or_len(text_err) != 3 || codec_api.len_or_rows(text_ok) != 3 { return Err(Error.Invalid) }
  ids := again.i64s(0) else { return Err(Error.Invalid) }
  last := ids.at(1) else { return Err(Error.Invalid) }
  if last != 9 { return Err(Error.Invalid) }
  malformed := buffer.filled(8, 0 as u8)
  match codec_api.open(0, malformed.bytes(), false) {
    Ok(_) => { return Err(Error.Invalid) }
    Err(error) => { match error { Invalid => {}, _ => { return Err(Error.Invalid) } } }
  }
  return Ok(())
}
"#;
        std::fs::write(stage.join("codec_api.align"), api).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, main).unwrap();
        let entry = entry.to_str().unwrap();
        let cache = align_driver::CacheContext::at(stage.join("cache"));
        for round in 0..3 {
            let mut sources = SourceMap::new();
            let programs = if round == 0 {
                let checked = check(&mut sources, entry, main);
                assert!(!checked.diags.has_errors(), "{}",
                    align_driver::format_diagnostics(&sources, &checked.diags));
                vec![align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap()]
            } else {
                let mut built = align_driver::build_package(&mut sources, entry, main, &cache,
                    align_driver::UnitReuse::Allowed);
                assert!(!built.diags.has_errors(), "{}",
                    align_driver::format_diagnostics(&sources, &built.diags));
                assert_eq!(built.units.len(), 2);
                assert!(built.units.iter().all(|unit|
                    unit.frontend.as_ref().unwrap().hit == (round == 2)));
                (0..built.units.len()).map(|index| built.materialize(index).unwrap().clone())
                    .collect::<Vec<_>>()
            };
            let mut objects = Vec::new();
            let mut libraries = Vec::new();
            for (index, program) in programs.iter().enumerate() {
                let object = stage.join(format!("codec-{round}-{index}.o"));
                align_driver::emit_object_file(program, &object, BuildTarget::Baseline,
                    Profile::Release, &[], false).unwrap();
                objects.push(object);
                for library in &program.link_libs {
                    if !libraries.contains(library) { libraries.push(library.clone()); }
                }
            }
            let binary = stage.join(format!("codec-{round}"));
            align_driver::link_objects(&align_driver::CDriver::default(),
                &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(), &binary,
                &libraries, Profile::Release).unwrap();
            let output = std::process::Command::new(binary).output().unwrap();
            assert!(output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
                "cache round {round}: {output:?}");
        }
    });
}

#[test]
fn codec_view_field_lifetime_and_placement_refusals() {
    let setup = "mut encoder := codec.encoder(1)?; encoder.put_i64(\"i\", [1])?; encoder.put_f64(\"f\", [1.0])?; encoder.put_bool(\"b\", [true])?; encoder.put_str(\"s\", [\"text\"])?; mut owner := encoder.finish(); mut alias := owner.bytes(); batch := codec.open(alias)?;";
    for (kind, select, observe) in [
        ("batch", "batch", "rows"),
        ("i64_column", "batch.i64s(0) else { return Err(Error.Invalid) }", "len"),
        ("f64_column", "batch.f64s(1) else { return Err(Error.Invalid) }", "len"),
        ("bool_column", "batch.bools(2) else { return Err(Error.Invalid) }", "len"),
        ("str_column", "batch.strs(3) else { return Err(Error.Invalid) }", "len"),
    ] {
        for (carrier, ty, value, access) in [
            ("direct", format!("codec.{kind}"), "view", "holder.value"),
            ("option", format!("Option<codec.{kind}>"), "Some(view)", "holder.value else { return Err(Error.Invalid) }"),
            ("result", format!("Result<codec.{kind}, Error>"), "Ok(view)", "holder.value?"),
        ] {
            for action in ["live", "mutate", "replace", "escape"] {
                let mutation = match action {
                    "mutate" => "alias[16] = 255;",
                    "replace" => "owner = buffer.filled(1, 0 as u8);",
                    _ => "",
                };
                let (ret, finish) = if action == "escape" {
                    ("Holder", "return Ok(holder)".to_string())
                } else {
                    ("i64", "return inspect(holder)".to_string())
                };
                let source = format!(
                    "module main\nimport core.codec\nHolder {{ value: {ty} }}\n\
                     fn inspect(holder: Holder) -> Result<i64, Error> {{ selected := {access}; return Ok(selected.{observe}()) }}\n\
                     fn probe() -> Result<{ret}, Error> {{ {setup} view := {select}; holder := Holder {{ value: {value} }}; {mutation} {finish} }}\nfn main() {{}}\n"
                );
                let label = format!("codec-field-{kind}-{carrier}-{action}");
                let checked = diff_check_multi(&label, &[("main.align", source.as_str())], "main.align");
                for (errors, diagnostics) in [
                    (checked.whole_errors, checked.whole_diags.as_str()),
                    (checked.per_unit_errors, checked.per_unit_diags.as_str()),
                ] {
                    assert_eq!(errors, action != "live", "{label}: {diagnostics}");
                    assert!(!diagnostics.contains("internal error"), "{label}: {diagnostics}");
                    if action == "mutate" {
                        assert!(diagnostics.contains("validated codec bytes were modified"), "{label}: {diagnostics}");
                    }
                }
            }
        }
        for declaration in [
            format!("Bad {{ values: array<codec.{kind}> }}"),
            format!("extern \"C\" fn bad(value: codec.{kind})"),
        ] {
            let source = format!("module main\nimport core.codec\n{declaration}\nfn main() {{}}\n");
            let checked = diff_check_multi("codec-field-forbidden-carriers", &[("main.align", source.as_str())], "main.align");
            assert!(checked.whole_errors && checked.per_unit_errors, "{kind}: {declaration}");
        }
    }
}

#[test]
fn validated_codec_observation_whole_unit_parity() {
    for imported in [false, true] {
        for copied in [false, true] {
            for modified in [false, true] {
                let helper = "pub fn identity(value: codec.batch) -> codec.batch = value\n";
                let imports = if imported { "import views\n" } else { helper };
                let call = if imported { "views.identity" } else { "identity" };
                let input = if copied { "alias.to_array()" } else { "alias" };
                let action = if modified { "alias[16] = 255" } else { "print(alias[16])" };
                let source = format!("module main\nimport core.codec\n{imports}fn probe<T>(marker: T) -> Result<(), Error> {{ encoder := codec.encoder(0)?; owner := encoder.finish(); mut alias := owner.bytes(); input := {input}; batch := codec.open(input)?; selected := {call}(batch); {action}; print(selected.rows()); Ok(()) }}\nfn main() -> Result<(), Error> = probe(0)\n");
                let helper_module = format!("module views\nimport core.codec\n{helper}");
                let files = if imported {
                    vec![("views.align", helper_module.as_str()), ("main.align", source.as_str())]
                } else {
                    vec![("main.align", source.as_str())]
                };
                let name = format!("codec-observation-{imported}-{copied}-{modified}");
                let result = diff_check_multi(&name, &files, "main.align");
                let stale = modified && !copied;
                assert_eq!(result.whole_errors, stale, "{name}: {}", result.whole_diags);
                assert_eq!(result.per_unit_errors, stale, "{name}: {}", result.per_unit_diags);
                if stale {
                    assert!(result.whole_diags.contains("validated codec bytes were modified"), "{}", result.whole_diags);
                    assert!(result.per_unit_diags.contains("validated codec bytes were modified"), "{}", result.per_unit_diags);
                }
            }
        }
    }
}

#[test]
fn validated_codec_observation_safe_execution() {
    let source = r#"import core.codec
fn main() -> Result<(), Error> {
    encoder := codec.encoder(0)?
    owner := encoder.finish()
    mut alias := owner.bytes()
    copied := alias.to_array()
    stable := codec.open(copied)?
    batch := codec.open(alias)?
    before := batch.rows()
    alias[16] = 255
    fresh := codec.open(alias)?
    print(before)
    print(stable.rows())
    print(fresh.rows())
    print(alias[16])
    Ok(())
}
"#;
    let files = [("main.align", source)];
    let checked = diff_check_multi("codec-observation-safe", &files, "main.align");
    assert!(!checked.whole_errors && !checked.per_unit_errors, "{}\n{}", checked.whole_diags, checked.per_unit_diags);
    if backend_available() {
        for output in [
            build_and_run_multi("codec-observation-safe", &files, "main.align"),
            build_per_unit_multi("codec-observation-safe-unit", &files, "main.align").link_and_run(),
        ] {
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout), "0\n0\n255\n255\n");
        }
    }
}

#[test]
fn codec_encoder_and_views_round_trip_all_four_kinds() {
    if !backend_available() {
        return;
    }
    let source = r#"import core.codec
pub fn main() -> Result<(), Error> {
  ids := [-1, 2]
  scores := [1.5, -0.0]
  flags := [true, false]
  names := ["x", ""]
  mut encoder := codec.encoder(2)?
  encoder.put_i64("i", ids)?
  encoder.put_f64("f", scores)?
  encoder.put_bool("b", flags)?
  encoder.put_str("s", names)?
  finished := encoder.finish()
  batch := codec.open(finished.bytes())?
  print(batch.rows())
  print(batch.columns())
  print(batch.name(0) else "missing")
  kind := batch.kind(1) else { return Err(Error.Invalid) }
  print(match kind { F64 => true, _ => false })
  print(batch.find("b") else -1)
  i64s := batch.i64s(0) else { return Err(Error.Invalid) }
  f64s := batch.f64s(1) else { return Err(Error.Invalid) }
  bools := batch.bools(2) else { return Err(Error.Invalid) }
  strs := batch.strs(3) else { return Err(Error.Invalid) }
  print(i64s.len())
  print(i64s.at(0) else 0)
  print(f64s.at(0) else 0.0)
  print(bools.at(0) else false)
  print(strs.at(0) else "missing")
  print(strs.at(1) else "missing")
  return Ok(())
}
"#;
    let output = build_and_run("core-codec-round-trip", source);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "2\n4\ni\ntrue\n2\n2\n-1\n1.5\ntrue\nx\n\n",
    );
}

#[test]
fn codec_surface_rejects_missing_import_wrong_types_unbound_receivers_and_shared_mutation() {
    assert!(check_errs(
        "core-codec-import",
        "fn main() { _ := codec.encoder(0) }\n",
    ));
    assert!(check_errs(
        "core-codec-open-type",
        "import core.codec\nfn main() { _ := codec.open(7) }\n",
    ));
    assert!(check_errs(
        "core-codec-open-str",
        "import core.codec\nfn main() { _ := codec.open(\"ALNCOL01\") }\n",
    ));
    for (name, method) in [("at", "at(0)"), ("finish", "finish()")] {
        let diagnostics = check_diagnostics(
            &format!("core-codec-batch-cross-method-{name}"),
            &format!(
                "import core.codec\nfn bad(batch: codec.batch) {{ _ := batch.{method} }}\nfn main() -> i32 = 0\n"
            ),
        );
        assert!(diagnostics.contains("is not a method on codec.batch"), "{diagnostics}");
    }
    assert!(check_errs(
        "core-codec-put-type",
        "import core.codec\nfn main() -> Result<(), Error> { encoder := codec.encoder(1)?; encoder.put_i64(\"x\", [true])?; return Ok(()) }\n",
    ));
    assert!(check_errs(
        "core-codec-unbound-put",
        "import core.codec\nfn main() -> Result<(), Error> { codec.encoder(1)?.put_i64(\"x\", [1])?; return Ok(()) }\n",
    ));
    let argument_diagnostics = check_diagnostics(
        "core-codec-unbound-put-argument-first",
        "import core.codec\nfn main() -> Result<(), Error> { codec.encoder(1)?.put_i64(\"x\", [true])?; return Ok(()) }\n",
    );
    assert!(argument_diagnostics.contains("i64"), "{argument_diagnostics}");
    assert!(!argument_diagnostics.contains("bind the codec encoder"), "{argument_diagnostics}");
    assert!(check_errs(
        "core-codec-shared-put",
        "import core.codec\nfn bad(borrow encoder: codec.encoder) -> Result<(), Error> { encoder.put_i64(\"x\", [1])?; return Ok(()) }\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "core-codec-shared-finish",
        "import core.codec\nfn bad(borrow encoder: codec.encoder) -> buffer = encoder.finish()\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "core-codec-borrow-mut-finish",
        "import core.codec\nfn bad(borrow mut encoder: codec.encoder) -> buffer = encoder.finish()\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "core-codec-use-after-finish",
        "import core.codec\nfn main() -> Result<(), Error> { encoder := codec.encoder(0)?; finished := encoder.finish(); second := encoder.finish(); _ := finished.len(); _ := second.len(); return Ok(()) }\n",
    ));
}

#[test]
fn codec_types_cross_whole_program_and_per_unit_interfaces() {
    let files = &[
        (
            "codec_api.align",
            r#"module codec_api
import core.codec
pub fn open(input: slice<u8>) -> Result<codec.batch, Error> = codec.open(input)
pub fn names(batch: codec.batch) -> Option<codec.str_column> = batch.strs(3)
pub fn start(rows: i64) -> Result<codec.encoder, Error> = codec.encoder(rows)
pub fn add_ids(encoder: codec.encoder, values: slice<i64>) -> Result<codec.encoder, Error> {
  encoder.put_i64("i", values)?
  return Ok(encoder)
}
pub fn finish(encoder: codec.encoder) -> buffer = encoder.finish()
pub fn kind(batch: codec.batch) -> Option<codec.kind> = batch.kind(0)
"#,
        ),
        (
            "main.align",
            r#"import core.codec
import codec_api
pub fn main() -> Result<(), Error> {
  values := [7, 9]
  encoder := codec_api.start(2)?
  encoder2 := codec_api.add_ids(encoder, values)?
  finished := codec_api.finish(encoder2)
  batch := codec_api.open(finished.bytes())?
  print(batch.rows())
  ids := batch.i64s(0) else { return Err(Error.Invalid) }
  print(ids.at(1) else -1)
  return Ok(())
}
"#,
        ),
    ];
    let differential = diff_check_multi("core-codec-interface", files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    if backend_available() {
        let output =
            build_per_unit_multi("core-codec-interface-run", files, "main.align").link_and_run();
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "2\n9\n");
    }
}

#[test]
fn codec_views_keep_the_input_generation_live_and_stay_out_of_invalid_carriers() {
    let carriers = r#"import core.codec
Holder { batch: codec.batch, names: Option<codec.str_column> }
Choice { Batch(codec.batch), Empty }
Generic<T> { Value(T), Empty }
fn hold(batch: codec.batch) -> Holder = Holder { batch: batch, names: batch.strs(0) }
fn rows(holder: Holder) -> i64 = holder.batch.rows()
fn choose(batch: codec.batch) -> Choice = Choice.Batch(batch)
fn inspect(choice: Choice) -> i64 = match choice { Batch(batch) => batch.rows(), Empty => 0 }
fn generic_view(value: Generic<codec.batch>) -> i64 = match value { Value(batch) => batch.rows(), Empty => 0 }
fn generic_owner(value: Generic<codec.encoder>) -> i32 = 0
fn main() -> i32 = 0
"#;
    assert!(
        !check_errs("core-codec-view-carriers", carriers),
        "{}",
        check_diagnostics("core-codec-view-carriers", carriers),
    );
    assert!(check_errs(
        "core-codec-owner-mutation",
        "import core.codec\nfn main() -> Result<(), Error> { mut encoder := codec.encoder(0)?; finished := encoder.finish(); batch := codec.open(finished.bytes())?; finished.append(\"x\"); _ := batch.rows(); return Ok(()) }\n",
    ));
    assert!(check_errs(
        "core-codec-local-owner-escape",
        "import core.codec\nfn bad() -> Result<codec.batch, Error> { encoder := codec.encoder(0)?; finished := encoder.finish(); return codec.open(finished.bytes()) }\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "core-codec-column-array",
        "import core.codec\nfn bad(values: array<codec.i64_column>) {}\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "core-codec-encoder-array",
        "import core.codec\nfn bad(values: array<codec.encoder>) {}\nfn main() -> i32 = 0\n",
    ));
}

#[test]
fn codec_llvm_uses_keyed_runtime_rows_and_alignment_one_inline_loads() {
    if !backend_available() {
        return;
    }
    let mut sources = SourceMap::new();
    let checked = check(
        &mut sources,
        "core-codec-llvm",
        r#"import core.codec
pub fn inspect(input: slice<u8>, index: i64) -> Result<i64, Error> {
  batch := codec.open(input)?
  ids := batch.i64s(0) else { return Err(Error.Invalid) }
  return Ok(ids.at(index) else -1)
}
pub fn encode(values: slice<i64>) -> Result<buffer, Error> {
  encoder := codec.encoder(values.len())?
  encoder.put_i64("i", values)?
  return Ok(encoder.finish())
}
fn main() -> i32 = 0
"#,
    );
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags),
    );
    let mir = align_driver::try_lower_to_mir(&checked.hir).expect("checked codec HIR");
    let llvm = emit_llvm_ir(&mir, BuildTarget::Baseline, align_driver::Profile::Release, false, &[], false).expect("LLVM IR");
    for call in [
        "call i32 @align_rt_codec_open_v1(ptr",
        "call i32 @align_rt_codec_encoder_new_v1(i64",
        "call i32 @align_rt_codec_encoder_put_i64_v1(ptr",
        "call ptr @align_rt_codec_encoder_finish_v1(ptr",
        "call void @align_rt_codec_encoder_free_v1(ptr",
    ] {
        assert!(llvm.contains(call), "missing `{call}`:\n{llvm}");
    }
    assert!(
        llvm.matches("align 1").count() >= 5,
        "header, descriptor, and element loads must be explicitly unaligned:\n{llvm}",
    );
    assert!(!llvm.contains("align_rt_codec_batch_"));
    assert!(!llvm.contains("align_rt_codec_column_"));
}

#[test]
fn codec_all_four_golden_decodes_at_every_base_alignment() {
    if !backend_available() {
        return;
    }
    let source = r#"import core.codec
import std.encoding
fn verify(input: slice<u8>) -> Result<i64, Error> {
  batch := codec.open(input)?
  if batch.rows() != 2 || batch.columns() != 4 { return Err(Error.Invalid) }
  if (batch.name(-1) else "none") != "none" { return Err(Error.Invalid) }
  if (batch.name(4) else "none") != "none" { return Err(Error.Invalid) }
  if (batch.find("B") else -1) != -1 { return Err(Error.Invalid) }
  kind := batch.kind(3) else { return Err(Error.Invalid) }
  if !match kind { Str => true, _ => false } { return Err(Error.Invalid) }
  ids := batch.i64s(0) else { return Err(Error.Invalid) }
  scores := batch.f64s(1) else { return Err(Error.Invalid) }
  flags := batch.bools(2) else { return Err(Error.Invalid) }
  names := batch.strs(3) else { return Err(Error.Invalid) }
  if match batch.f64s(0) { Some(_) => true, None => false } { return Err(Error.Invalid) }
  if (ids.at(-1) else 17) != 17 || (ids.at(2) else 19) != 19 { return Err(Error.Invalid) }
  if (ids.at(0) else 0) != -1 || (ids.at(1) else 0) != 2 { return Err(Error.Invalid) }
  if (scores.at(0) else 0.0) != 1.5 || (scores.at(1) else 1.0) != -0.0 { return Err(Error.Invalid) }
  if !(flags.at(0) else false) || (flags.at(1) else true) { return Err(Error.Invalid) }
  if (names.at(0) else "none") != "x" || (names.at(1) else "none") != "" { return Err(Error.Invalid) }
  return Ok(ids.at(1) else -1)
}
pub fn main() -> Result<(), Error> {
  raw := encoding.hex_decode("414c4e434f4c3031280100000000000002000000000000000400000000000000e0000000000000000100000000000000e800000000000000100000000000000000000000000000000000000000000000e1000000000000000100000001000000f800000000000000100000000000000000000000000000000000000000000000e20000000000000001000000020000000801000000000000010000000000000000000000000000000000000000000000e300000000000000010000000300000010010000000000000c00000000000000200100000000000001000000000000006966627300000000ffffffffffffffff0200000000000000000000000000f83f00000000000000800100000000000000000000000100000001000000000000007800000000000000")?
  mut residue := 0
  loop {
    mut shifted := buffer(0)
    mut padding := 0
    loop {
      if padding >= residue { break }
      shifted.put_u8(0)
      padding = padding + 1
    }
    shifted.append(raw.bytes())
    bytes := shifted.bytes()
    print(verify(bytes[residue..])?)
    residue = residue + 1
    if residue == 8 { break }
  }
  return Ok(())
}
"#;
    let output = build_and_run("core-codec-alignment", source);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "2\n2\n2\n2\n2\n2\n2\n2\n");
}
