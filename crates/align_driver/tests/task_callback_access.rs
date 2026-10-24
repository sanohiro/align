//! Task-shared callbacks preserve their access contract across independent modules.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_span::SourceMap;
use std::path::Path;

const TYPES: &str = r#"module model
pub Ctx { prefix: str, body: slice<i64> }
pub Route { prefix: str, handler: fn(Ctx) -> i64 }
"#;
const CONSTRUCTORS: &str = r#"module constructors
import model
pub fn route(prefix: str, handler: fn(model.Ctx) -> i64) -> model.Route =
  model.Route { prefix: prefix, handler: handler }
pub fn forward<T>(value: T) -> T = value
"#;
const RUNNER: &str = r#"module runner
import model
fn dispatch(routes: slice<model.Route>, values: slice<i64>) -> i64 {
  row := routes[0]
  handler := row.handler
  return handler(model.Ctx { prefix: row.prefix, body: values })
}
pub fn local(routes: slice<model.Route>) -> i64 {
  task_group {
    task := spawn(fn { values := [40, 2]; dispatch(routes, values[..]) })
    wait()
    return task.get()
  }
}
pub fn shared(routes: slice<model.Route>, values: slice<i64>) -> i64 {
  task_group {
    task := spawn(fn { dispatch(routes, values) })
    wait()
    return task.get()
  }
}
pub fn value(route: model.Route) -> i64 {
  task_group { task := spawn(fn { copied := route; handler := copied.handler; values := [40, 2]; handler(model.Ctx { prefix: copied.prefix, body: values[..] }) }); wait(); return task.get() }
}
"#;
// Value-view writers are refused at their declaration. This writer changes a
// local fixed array, preserving callback transport coverage with an admitted body.
const HELPERS: &str = r#"
fn read(c: model.Ctx) -> i64 = c.body[0] + c.body[1]
fn write(c: model.Ctx) -> i64 { mut local := [c.body[0], c.body[1]]; local[0] = 40; return local[0] + local[1] }
fn change(borrow mut values: slice<i64>) { mut view := values; view[0] = 9 }
fn poison(borrow mut rows: [model.Route; 1], values: slice<i64>) -> slice<i64> {
  rows[0].handler = write
  return values
}
fn choose(handler: fn(model.Ctx) -> i64, flag: bool) -> model.Route =
  constructors.route("/", if flag { read } else { handler })
fn sequential(handler: fn(model.Ctx) -> i64, values: slice<i64>) -> i64 =
  handler(model.Ctx { prefix: "/", body: values })
Choice { Value(model.Route), Empty }
fn propagate(value: Result<model.Route, model.Route>) -> Result<model.Route, model.Route> {
  row := value?
  return Ok(row)
}
fn identity(row: model.Route) -> model.Route = row
fn replaced_view(borrow mut rows: [model.Route; 1]) -> slice<model.Route> {
  rows[0].handler = write
  return rows[..]
}
"#;

fn put(stage: &Path, name: &str, source: &str) {
    if let Err(error) = std::fs::write(stage.join(name), source) {
        panic!("write {name}: {error}");
    }
}

fn check_modes(stage: &Path, label: &str, body: &str, valid: bool) {
    let source = format!(
        "module main\nimport model\nimport constructors\nimport runner\n{HELPERS}\nfn main() -> i32 {{ {body}\nreturn 0 }}"
    );
    put(stage, "main.align", &source);
    let entry = stage.join("main.align").to_string_lossy().into_owned();
    let mut sources = SourceMap::new();
    let whole = align_driver::check(&mut sources, &entry, &source);
    let diagnostics = align_driver::format_diagnostics(&sources, &whole.diags);
    assert_eq!(
        !whole.diags.has_errors(),
        valid,
        "whole {label}: {diagnostics}"
    );
    if !valid {
        assert!(
            diagnostics.contains("task callback") || diagnostics.contains("callback may modify shared task backing") || diagnostics.contains("spawned task") || diagnostics.contains("read-only view") || diagnostics.contains("invalidated"),
            "whole {label}: unrelated rejection: {diagnostics}"
        );
    }
    if valid {
        assert!(
            align_mir::lower_program_checked(&whole.hir, false, Some(&sources)).is_ok(),
            "whole {label}: checked HIR must lower"
        );
    }
    let mut sources = SourceMap::new();
    let units = align_driver::build_per_unit(&mut sources, &entry, &source);
    let diagnostics = align_driver::format_diagnostics(&sources, &units.diags);
    assert_eq!(
        !units.diags.has_errors(),
        valid,
        "per-unit {label}: {diagnostics}"
    );
    if !valid {
        assert!(
            diagnostics.contains("task callback") || diagnostics.contains("callback may modify shared task backing") || diagnostics.contains("spawned task") || diagnostics.contains("read-only view") || diagnostics.contains("invalidated"),
            "per-unit {label}: unrelated rejection: {diagnostics}"
        );
    }
}

#[test]
fn callback_readers_and_request_local_writes_cross_units() {
    owned_fixture::run(
        "callback_readers_and_request_local_writes_cross_units",
        |stage| {
            put(stage, "model.align", TYPES);
            put(stage, "constructors.align", CONSTRUCTORS);
            put(stage, "runner.align", RUNNER);
            for (label, body) in [
                (
                    "direct-constrained",
                    "_ := runner.value(constructors.route(\"/\", read))",
                ),
                (
                    "reader",
                    "mut rows := [constructors.route(\"/\", read)]; _ := runner.local(rows[..])",
                ),
                (
                    "local-writer",
                    "rows := [constructors.route(\"/\", write)]; _ := runner.local(rows[..])",
                ),
                (
                    "shared-reader",
                    "values := [40, 2]; mut rows := [constructors.route(\"/\", read)]; _ := runner.shared(rows[..], values[..])",
                ),
                (
                    "optional-constructor",
                    "values := [40, 2]; row := Some(constructors.route(\"/\", read)) else { constructors.route(\"/\", read) }; rows := [row]; _ := runner.shared(rows[..], values[..])",
                ),
                (
                    "loop-constructor",
                    "values := [40, 2]; row := loop { break constructors.route(\"/\", read) }; rows := [row]; _ := runner.shared(rows[..], values[..])",
                ),
                (
                    "sequential-writer",
                    "values := [1, 2]; _ := sequential(write, values[..])",
                ),
                (
                    "copied-callback",
                    "values := [40, 2]; mut rows := [constructors.route(\"/\", read)]; row := rows[0]; saved := row.handler; _ := poison(rows, values[..]); view := values[..]; task_group { task := spawn(fn { local := saved; local(model.Ctx { prefix: \"/\", body: view }) }); wait(); _ := task.get() }",
                ),
                (
                    "generic-constructor",
                    "values := [40, 2]; rows := [constructors.forward(constructors.route(\"/\", read))]; _ := runner.shared(rows[..], values[..])",
                ),
            ] {
                check_modes(stage, label, body, true);
            }
        },
    );
}

#[test]
fn callback_storage_updates_and_captured_view_refusals() {
    owned_fixture::run(
        "callback_storage_updates_and_captured_view_refusals",
        |stage| {
            put(stage, "model.align", TYPES);
            put(stage, "constructors.align", CONSTRUCTORS);
            put(stage, "runner.align", RUNNER);
            for (label, body) in [
                (
                    "argument-writer",
                    "values := [40, 2]; rows := [constructors.route(\"/\", write)]; _ := runner.shared(rows[..], values[..])",
                ),
                (
                    "captured-writer",
                    "values := [40, 2]; view := values[..]; handler := fn c: model.Ctx { mut selected := view; change(selected); 0 }; rows := [constructors.route(\"/\", handler)]; _ := runner.local(rows[..])",
                ),
                (
                    "joined-writer",
                    "values := [40, 2]; handler := if values[0] == 40 { read } else { write }; rows := [constructors.route(\"/\", handler)]; _ := runner.shared(rows[..], values[..])",
                ),
                (
                    "later-operand",
                    "values := [40, 2]; mut rows := [constructors.route(\"/\", read)]; _ := runner.shared(rows[..], poison(rows, values[..]))",
                ),
                (
                    "constrained-function-value",
                    "call := runner.value; _ := call(constructors.route(\"/\", read))",
                ),
                (
                    "constrained-pipeline",
                    "rows := [constructors.route(\"/\", read)]; _ := rows.map(runner.value).sum()",
                ),
                (
                    "reusable-capture",
                    "values := [40, 2]; mut rows := [constructors.route(\"/\", read)]; view := rows[..]; body := values[..]; held := fn { runner.shared(view, body) }; _ := poison(rows, values[..]); _ := held()",
                ),
                (
                    "known-and-formal-join",
                    "values := [40, 2]; rows := [choose(write, values[0] != 40)]; _ := runner.shared(rows[..], values[..])",
                ),
                (
                    "alias-replacement",
                    "values := [40, 2]; mut rows := [constructors.route(\"/\", read)]; alias := rows[..]; rows[0].handler = write; _ := runner.shared(alias, values[..])",
                ),
                (
                    "returned-replaced-view",
                    "values := [40, 2]; mut rows := [constructors.route(\"/\", read)]; _ := runner.shared(replaced_view(rows), values[..])",
                ),
            ] {
                check_modes(stage, label, body, matches!(label, "argument-writer" | "joined-writer" | "known-and-formal-join"));
            }
        },
    );
}

#[test]
fn callback_sources_survive_each_control_carrier() {
    owned_fixture::run("callback_sources_survive_each_control_carrier", |stage| {
        put(stage, "model.align", TYPES);
        put(stage, "constructors.align", CONSTRUCTORS);
        put(stage, "runner.align", RUNNER);
        for (label, carrier) in [
            (
                "if",
                "row := if values[0] == 40 { incoming } else { constructors.route(\"/\", read) }",
            ),
            (
                "option-match",
                "choice := Some(incoming); row := match choice { Some(value) => value, None => constructors.route(\"/\", read) }",
            ),
            (
                "enum-match",
                "choice: Choice := Choice.Value(incoming); row := match choice { Value(value) => value, Empty => constructors.route(\"/\", read) }",
            ),
            (
                "result-else",
                "choice: Result<model.Route, i64> := Ok(incoming); row := choice else { constructors.route(\"/\", read) }",
            ),
            (
                "try-error",
                "choice := propagate(Err(incoming)); row := match choice { Ok(value) => value, Err(error) => error }",
            ),
            (
                "map-error",
                "choice: Result<model.Route, model.Route> := Err(incoming); mapped := choice.map_err(identity); row := match mapped { Ok(value) => value, Err(error) => error }",
            ),
            (
                "loop-branch",
                "row := loop { if values[0] == 40 { break incoming }; break constructors.route(\"/\", read) }",
            ),
        ] {
            for (callback, valid) in [("read", true), ("write", true)] {
                let body = format!(
                    "values := [40, 2]; incoming := constructors.route(\"/\", {callback}); {carrier}; rows := [row]; _ := runner.shared(rows[..], values[..])"
                );
                check_modes(stage, &format!("{label}/{callback}"), &body, valid);
            }
        }
    });
}

fn execute(stage: &Path, programs: &[align_mir::Program], label: &str) {
    use align_driver::{BuildTarget, Profile};
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{index}.o"));
        if let Err(error) = align_driver::emit_object_file(
            program,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
        ) {
            panic!("{label}: {error}");
        }
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
    }
    let binary = stage.join(label);
    if let Err(error) = align_driver::link_objects(
        &align_driver::CDriver::default(),
        &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        &binary,
        &libraries,
        Profile::Release,
    ) {
        panic!("{label}: {error}");
    }
    let output = std::process::Command::new(binary)
        .output()
        .unwrap_or_else(|e| panic!("{label}: {e}"));
    assert!(output.status.success(), "{label}: {output:?}");
}

#[test]
fn callback_contracts_execute_in_both_modes() {
    owned_fixture::run("callback_contracts_execute_in_both_modes", |stage| {
        put(stage, "model.align", TYPES);
        put(stage, "constructors.align", CONSTRUCTORS);
        put(stage, "runner.align", RUNNER);
        let source = format!(
            "module main\nimport model\nimport constructors\nimport runner\n{HELPERS}\nfn main() -> i32 {{ values := [1, 2]; mut rows := [constructors.route(\"/\", read)]; if runner.shared(rows[..], values[..]) != 3 {{ return 1 }}; writers := [constructors.route(\"/\", write)]; if runner.local(writers[..]) != 42 {{ return 2 }}; return 0 }}"
        );
        put(stage, "main.align", &source);
        let entry = stage.join("main.align").to_string_lossy().into_owned();
        let mut sources = SourceMap::new();
        let whole = align_driver::check(&mut sources, &entry, &source);
        assert!(
            !whole.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &whole.diags)
        );
        let mir = align_mir::lower_program_checked(&whole.hir, false, Some(&sources))
            .unwrap_or_else(|e| panic!("checked whole: {e:?}"));
        execute(stage, &[mir], "whole");
        let mut sources = SourceMap::new();
        let units = align_driver::build_per_unit(&mut sources, &entry, &source);
        assert!(
            !units.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &units.diags)
        );
        execute(
            stage,
            &units.units.into_iter().map(|u| u.mir).collect::<Vec<_>>(),
            "units",
        );
    });
}

#[test]
fn cached_constructor_effects_follow_private_callback_edits() {
    owned_fixture::run(
        "cached_constructor_effects_follow_private_callback_edits",
        |stage| {
            put(stage, "model.align", TYPES);
            put(stage, "runner.align", RUNNER);
            let provider = format!(
                "{CONSTRUCTORS}\nfn internal(c: model.Ctx) -> i64 = c.body[0] + c.body[1]\npub fn provided() -> model.Route = route(\"/\", internal)\n"
            );
            let source = "module main\nimport constructors\nimport runner\nfn main() -> i32 { values := [40, 2]; rows := [constructors.provided()]; _ := runner.shared(rows[..], values[..]); return 0 }\n";
            put(stage, "main.align", source);
            let entry = stage.join("main.align").to_string_lossy().into_owned();
            let cache = align_driver::CacheContext::at(stage.join("cache"));
            for (label, text, valid, warm) in [
                ("cold", provider.clone(), true, false),
                ("hit", provider.clone(), true, true),
                (
                    "writer",
                    provider.replace(
                        "= c.body[0] + c.body[1]",
                        "{ mut values := c.body; values[0] = 7; return values[0] }",
                    ),
                    false,
                    false,
                ),
                ("restored", provider, true, true),
            ] {
                put(stage, "constructors.align", &text);
                let mut sources = SourceMap::new();
                let mut build = align_driver::build_package(
                    &mut sources,
                    &entry,
                    source,
                    &cache,
                    align_driver::UnitReuse::Allowed,
                );
                let diagnostics = align_driver::format_diagnostics(&sources, &build.diags);
                assert_eq!(!build.diags.has_errors(), valid, "{label}: {diagnostics}");
                if !valid {
                    assert!(diagnostics.contains("read-only view"), "{label}: {diagnostics}");
                    continue;
                }
                assert_eq!(build.units.len(), 4, "{label}: complete module graph");
                if warm {
                    assert!(
                        build
                            .units
                            .iter()
                            .all(|u| u.frontend.as_ref().is_some_and(|outcome| outcome.hit)),
                        "{label}: every frontend must hit"
                    );
                }
                for index in 0..build.units.len() {
                    assert!(
                        build.materialize(index).is_ok(),
                        "{label}: authenticated rehydration {index}"
                    );
                }
            }
        },
    );
}
