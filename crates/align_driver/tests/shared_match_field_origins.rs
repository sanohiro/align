//! Request130: shared matched payloads retain their stable field origin.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

use align_driver::{BuildTarget, Profile, check, lower_to_mir};
use align_span::SourceMap;

fn certify(source: &str) -> align_mir::Program {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "shared-match-origin.align", source);
    assert!(
        !checked.diags.has_errors(),
        "{}\n{source}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    let program = lower_to_mir(&checked.hir);
    align_mir::producer::validate_mir_producers(&program).unwrap();
    let defined = program.fns.iter().map(|f| f.name.clone()).collect();
    align_mir::producer::validate_partition_resource_rvalues(&program, &defined).unwrap();
    program
}

#[test]
fn source_matrix() {
    for (declaration, sum, selected, empty) in [
        ("", "Option<Payload>", "Some(payload)", "None"),
        ("", "Result<Payload, i64>", "Ok(payload)", "Err(_)"),
        ("", "Result<i64, Payload>", "Err(payload)", "Ok(_)"),
        (
            "Choice { Full(Payload), Empty }",
            "Choice",
            "Full(payload)",
            "Empty",
        ),
    ] {
        for reverse in [false, true] {
            for nested in [false, true] {
                for indirect in [false, true] {
                    let params = if reverse {
                        "borrow mut scratch: slice<u8>, borrow payload: Payload"
                    } else {
                        "borrow payload: Payload, borrow mut scratch: slice<u8>"
                    };
                    let args = if reverse {
                        "scratch, payload"
                    } else {
                        "payload, scratch"
                    };
                    let (owner, place) = if nested {
                        ("Outer", "state.inner")
                    } else {
                        ("State", "state")
                    };
                    let source = format!(
                        r#"
Payload {{ values: array<i64> }}
{declaration}
State {{ optional: {sum}, scratch: buffer }}
Outer {{ inner: State, sibling: string }}
fn combined({params}) {{ scratch[0] = if payload.values[0] == 1 {{1 as u8}} else {{2 as u8}} }}
fn exercise(borrow mut state: {owner}) {{
  match {place}.optional {{
    {selected} => {{
      mut scratch := {place}.scratch.bytes()
      {call}
    }}
    {empty} => {{}}
  }}
}}
fn main() {{}}
"#,
                        call = if indirect {
                            format!("f := combined; f({args})")
                        } else {
                            format!("combined({args})")
                        }
                    );
                    let program = certify(&source);
                    align_driver::emit_llvm_ir(
                        &program,
                        BuildTarget::Baseline,
                        Profile::Dev,
                        false,
                        &[],
                        false,
                    )
                    .unwrap();
                }
            }
        }
    }
}

#[test]
fn rejects_aliases_and_expiry() {
    let declarations = r#"
Payload { values: array<i64> }
State { optional: Option<Payload>, scratch: buffer }
fn combined(borrow payload: Payload, borrow mut scratch: slice<u8>) { scratch[0] = 1 }
fn containing(borrow payload: Payload, borrow mut state: State) {}
fn consume(payload: Payload) {}
fn exclusive(borrow mut payload: Payload) {}
fn replace(borrow mut state: State) { state.optional = None }
"#;
    for (name, body, diagnostic) in [
        (
            "containing-owner",
            "containing(payload, state)",
            "aliases argument",
        ),
        (
            "replaced-field",
            "state.optional = None; mut scratch := state.scratch.bytes(); combined(payload, scratch)",
            "invalid",
        ),
        (
            "replaced-owner-call",
            "replace(state); mut scratch := state.scratch.bytes(); combined(payload, scratch)",
            "invalid",
        ),
        ("consume", "consume(payload)", "borrowed"),
        ("exclusive", "exclusive(payload)", "borrowed"),
    ] {
        let source = format!(
            "{declarations}\nfn exercise(borrow mut state: State) {{ match state.optional {{ Some(payload) => {{ {body} }} None => {{}} }} }}\nfn main() {{}}"
        );
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, name, &source);
        let rendered = align_driver::format_diagnostics(&sources, &checked.diags);
        assert!(
            checked.diags.has_errors() && rendered.contains(diagnostic),
            "{name}: {rendered}"
        );
    }
    for (name, source) in [
        (
            "same-payload-backing",
            r#"
Payload { values: array<i64> }
State { optional: Option<Payload>, scratch: buffer }
fn combined(borrow payload: Payload, borrow mut values: slice<i64>) { values[0] = 7 }
fn exercise(borrow mut state: State) {
  match state.optional { Some(payload) => {
    mut values := payload.values[..]
    combined(payload, values)
  } None => {} }
}

fn main() {}
"#,
        ),
        (
            "view-bearing-sibling",
            r#"
Payload { values: slice<u8> }
State { optional: Option<Payload>, scratch: buffer }
fn combined(borrow payload: Payload, borrow mut scratch: slice<u8>) { scratch[0] = payload.values[0] }
fn exercise(borrow mut state: State) {
  match state.optional { Some(payload) => {
    mut scratch := state.scratch.bytes()
    combined(payload, scratch)
  } None => {} }
}
fn main() {}
"#,
        ),
        (
            "rebound-view",
            r#"
Payload { values: buffer }
State { optional: Option<Payload>, scratch: buffer }
fn redirect(borrow mut destination: slice<u8>, source: slice<u8>) { destination = source }
fn combined(borrow payload: Payload, borrow mut scratch: slice<u8>) { scratch[0] = payload.values.bytes()[0] }
fn exercise(borrow mut state: State) {
  match state.optional { Some(payload) => {
    mut scratch := state.scratch.bytes()
    redirect(scratch, payload.values.bytes())
    combined(payload, scratch)
  } None => {} }
}
fn main() {}
"#,
        ),
    ] {
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, name, source);
        let rendered = align_driver::format_diagnostics(&sources, &checked.diags);
        assert!(
            checked.diags.has_errors()
                && (rendered.contains("aliases argument")
                    || rendered.contains("borrowed")
                    || rendered.contains("read-only")),
            "{name}: {rendered}"
        );
    }
}

#[test]
fn possibly_rebinding_calls_discard_origins() {
    for (declaration, parameter, call) in [
        (
            "fn redirect(borrow mut destination: slice<u8>, source: slice<u8>, flag: bool) { if flag { destination = source } }",
            "",
            "redirect(scratch, payload.values.bytes(), true)",
        ),
        (
            "fn redirect(borrow mut destination: slice<u8>, source: slice<u8>) { destination = source }",
            "",
            "f := redirect; f(scratch, payload.values.bytes())",
        ),
        (
            "",
            ", f: fn(borrow mut slice<u8>, slice<u8>) -> ()",
            "f(scratch, payload.values.bytes())",
        ),
    ] {
        let source = format!(
            r#"
Payload {{ values: buffer }}
State {{ optional: Option<Payload>, scratch: buffer }}
{declaration}
fn combined(borrow payload: Payload, borrow mut scratch: slice<u8>) {{ scratch[0] = payload.values.bytes()[0] }}
fn exercise(borrow mut state: State{parameter}) {{
  match state.optional {{ Some(payload) => {{
    mut scratch := state.scratch.bytes()
    {call}
    combined(payload, scratch)
  }} None => {{}} }}
}}
fn main() {{}}
"#
        );
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, "rebind-origin", &source);
        let rendered = align_driver::format_diagnostics(&sources, &checked.diags);
        assert!(
            checked.diags.has_errors() && rendered.contains("aliases argument"),
            "{call}: {rendered}"
        );
    }
}

const MODEL: &str = r#"module model
pub Payload { values: array<i64>, label: string }
pub State { optional: Option<Payload>, scratch: buffer }
pub fn combined(borrow payload: Payload, borrow mut scratch: slice<u8>) {
  scratch[0] = if payload.values[0] == 7 && payload.label == "alive" {9 as u8} else {1 as u8}
}
pub fn generic<T>(borrow payload: Payload, borrow mut scratch: slice<u8>, marker: T) {
  combined(payload, scratch)
}
fn guard(fail: bool) -> Result<(), i64> { if fail { return Err(7) }; return Ok(()) }
fn error(value: i64) -> i64 = value + 1
pub fn run(borrow mut state: State, fail: bool) -> Result<i64, i64> {
  match state.optional {
    Some(payload) => {
      mut scratch := state.scratch.bytes()
      generic(payload, scratch, 1)
      mut index := 0
      loop {
        if index == 3 { break }
        combined(payload, scratch)
        index = index + 1
      }
      f := combined
      f(payload, scratch)
      combined(payload, scratch)
      guard(fail).map_err(error)?
      return Ok(payload.values[1])
    }
    None => { return Ok(0) }
  }
}
"#;

const MAIN: &str = r#"import model
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn exercise(fail: bool) -> Result<i64, i64> {
  mut state := model.State {
    optional: Some(model.Payload { values: [7, 11].to_array(), label: "alive".clone() }),
    scratch: buffer.filled(8, 0 as u8),
  }
  if unsafe { align_rt_requested_live_bytes() } <= 0 { return Err(1) }
  allocations := unsafe { align_rt_alloc_count() }
  frees := unsafe { align_rt_free_count() }
  answer := model.run(state, fail)?
  if answer != 11 || state.scratch.bytes()[0] != 9 { return Err(2) }
  second := model.run(state, false) else { return Err(3) }
  if second != 11 { return Err(4) }
  if unsafe { align_rt_alloc_count() } != allocations || unsafe { align_rt_free_count() } != frees { return Err(5) }
  return Ok(answer)
}
fn empty() -> i64 {
  mut state := model.State { optional: None, scratch: buffer.filled(8, 0 as u8) }
  return model.run(state, false) else { -1 }
}
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  baseline := unsafe { align_rt_requested_live_bytes() }
  if (exercise(false) else { return 1 }) != 11 { return 2 }
  if unsafe { align_rt_requested_live_bytes() } != baseline { return 3 }
  match exercise(true) { Ok(_) => { return 4 } Err(code) => { if code != 8 { return 5 } } }
  if unsafe { align_rt_requested_live_bytes() } != baseline { return 6 }
  if empty() != 0 || unsafe { align_rt_requested_live_bytes() } != baseline { return 7 }
  return 0
}
"#;

fn execute(stage: &std::path::Path, programs: &[align_mir::Program], label: &str) {
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{index}.o"));
        align_driver::emit_object_file(
            program,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
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
        &objects
            .iter()
            .map(|path| path.as_path())
            .collect::<Vec<_>>(),
        &binary,
        &libraries,
        Profile::Release,
    )
    .unwrap();
    // The fixture owns this entire compile/link/run process group and its deadline.
    let result = std::process::Command::new(binary).output().unwrap();
    assert!(
        result.status.success() && result.stdout.is_empty() && result.stderr.is_empty(),
        "{label}: {result:?}"
    );
}

#[test]
fn native_lifecycle_and_replay() {
    owned_fixture::run("native_lifecycle_and_replay", |stage| {
        let model = stage.join("model.align");
        let entry = stage.join("main.align");
        std::fs::write(&model, MODEL).unwrap();
        std::fs::write(&entry, MAIN).unwrap();
        let entry = entry.to_str().unwrap();
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, entry, MAIN);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        execute(stage, &[lower_to_mir(&checked.hir)], "whole");
        let context = align_driver::CacheContext::at(stage.join("cache"));
        let mut original = Vec::new();
        for round in 0..4 {
            let body = if round == 2 {
                MODEL.replace(
                    "  combined(payload, scratch)\n}",
                    "  combined(payload, scratch); combined(payload, scratch)\n}",
                )
            } else {
                MODEL.to_owned()
            };
            std::fs::write(&model, body).unwrap();
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sources,
                entry,
                MAIN,
                &context,
                align_driver::UnitReuse::Allowed,
            );
            assert!(
                !built.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &built.diags)
            );
            assert_eq!(built.units.len(), 2);
            if round < 2 {
                assert!(
                    built
                        .units
                        .iter()
                        .all(|unit| unit.frontend.as_ref().unwrap().hit == (round == 1))
                );
            }
            let programs = (0..built.units.len())
                .map(|i| built.materialize(i).unwrap().clone())
                .collect::<Vec<_>>();
            let snapshot = programs
                .iter()
                .map(align_mir::print::program_to_string)
                .collect::<Vec<_>>();
            if round == 0 {
                original = snapshot;
            } else if round == 2 {
                assert_ne!(snapshot, original);
            } else {
                assert_eq!(snapshot, original);
            }
            execute(stage, &programs, &format!("replay-{round}"));
        }
    });
}
