//! The 1:1 value-carrying-control-flow matrix: every form preserves the shortest escape Region
//! and the runtime individual-vs-arena ownership bit of an owned value.

mod common;
use common::*;

fn check_message(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(checked.diags.has_errors(), "{name} must reject an escaping arena value");
    align_driver::format_diagnostics(&sm, &checked.diags)
}

fn mir_text(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    align_mir::print::program_to_string(&lower_to_mir(&checked.hir))
}

fn function_body<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text.find(&format!("fn {name}")).unwrap_or_else(|| panic!("no fn {name} in MIR:\n{text}"));
    let body = &text[start..];
    let end = body.find("\n}").map(|i| i + 2).unwrap_or(body.len());
    &body[..end]
}

fn assert_flag_is_forwarded(text: &str, function: &str, flag: &str) {
    let body = function_body(text, function);
    let prefix = format!("{flag} <- ");
    assert!(
        body.lines().any(|line| line.trim().starts_with(&prefix) && line.contains('%')),
        "{flag} must receive a path-specific runtime ownership bit:\n{body}"
    );
}

// Region row: the result Region is the trailing/selected payload Region. Each program attempts to
// carry an arena-backed `str` across the arena boundary and must fail at that boundary.

#[test]
fn block_value_preserves_region() {
    let msg = check_message(
        "value-region-block",
        "fn main() -> i32 {\n  out := arena {\n    n := 1\n    s := template \"{n}\"\n    { s }\n  }\n  return out.len() as i32\n}\n",
    );
    assert!(msg.contains("cannot escape"), "unexpected diagnostic:\n{msg}");
}

#[test]
fn if_value_joins_regions() {
    let msg = check_message(
        "value-region-if",
        "fn main() -> i32 {\n  out := arena {\n    n := 1\n    s := template \"{n}\"\n    if n > 0 { s } else { \"static\" }\n  }\n  return out.len() as i32\n}\n",
    );
    assert!(msg.contains("cannot escape"), "unexpected diagnostic:\n{msg}");
}

#[test]
fn match_value_joins_regions() {
    let msg = check_message(
        "value-region-match",
        "Tag { A, B }\nfn main() -> i32 {\n  out := arena {\n    n := 1\n    s := template \"{n}\"\n    match Tag.A { A => s, B => \"static\" }\n  }\n  return out.len() as i32\n}\n",
    );
    assert!(msg.contains("cannot escape"), "unexpected diagnostic:\n{msg}");
}

#[test]
fn else_unwrap_value_joins_regions() {
    let msg = check_message(
        "value-region-else",
        "fn main() -> i32 {\n  out := arena {\n    n := 1\n    s := template \"{n}\"\n    opt: Option<str> := Some(s)\n    opt else \"static\"\n  }\n  return out.len() as i32\n}\n",
    );
    assert!(msg.contains("cannot escape"), "unexpected diagnostic:\n{msg}");
}

#[test]
fn try_value_preserves_payload_region() {
    let msg = check_message(
        "value-region-try",
        "fn run() -> Result<i32, Error> {\n  out := arena {\n    n := 1\n    s := template \"{n}\"\n    r: Result<str, Error> := Ok(s)\n    r?\n  }\n  return Ok(out.len() as i32)\n}\nfn main() -> i32 = 0\n",
    );
    assert!(msg.contains("cannot escape"), "unexpected diagnostic:\n{msg}");
}

// Ownership row: an owned result may be arena-owned on one path and individually heap-owned on
// another. The destination flag must receive the selected path's runtime bit, not a conservative
// constant derived from the joined escape Region.

#[test]
fn block_value_forwards_owned_flag() {
    let text = mir_text(
        "value-owned-block",
        "fn make() -> array<i64> = [7, 8].to_array()\nfn run(cond: bool) -> i32 {\n  arena {\n    mut xs := [1, 2].to_array()\n    if cond { xs = make() }\n    ys := { xs }\n    return ys[0] as i32\n  }\n}\nfn main() -> i32 = run(true)\n",
    );
    // run locals: cond=_0, xs=_1, ys=_2; flags follow at _3 and _4.
    assert_flag_is_forwarded(&text, "run", "_4");
}

#[test]
fn if_value_forwards_selected_owned_flag() {
    let text = mir_text(
        "value-owned-if",
        "fn make() -> array<i64> = [7, 8].to_array()\nfn run(cond: bool) -> i32 {\n  arena {\n    xs := if cond { make() } else { [1, 2].to_array() }\n    return xs[0] as i32\n  }\n}\nfn main() -> i32 = run(true)\n",
    );
    // run locals: cond=_0, xs=_1; xs flag=_2.
    assert_flag_is_forwarded(&text, "run", "_2");
}

#[test]
fn match_value_forwards_selected_owned_flag() {
    let text = mir_text(
        "value-owned-match",
        "Choice { Heap, Arena }\nfn make() -> array<i64> = [7, 8].to_array()\nfn run(choice: Choice) -> i32 {\n  arena {\n    xs := match choice { Heap => make(), Arena => [1, 2].to_array() }\n    return xs[0] as i32\n  }\n}\nfn main() -> i32 = run(Choice.Heap)\n",
    );
    // run locals: choice=_0, xs=_1; xs flag=_2.
    assert_flag_is_forwarded(&text, "run", "_2");
}

#[test]
fn else_unwrap_forwards_selected_owned_flag() {
    let text = mir_text(
        "value-owned-else",
        "fn maybe(cond: bool) -> Option<array<i64>> {\n  if cond { return Some([7, 8].to_array()) }\n  return None\n}\nfn run(cond: bool) -> i32 {\n  arena {\n    opt := maybe(cond)\n    xs := opt else [1, 2].to_array()\n    return xs[0] as i32\n  }\n}\nfn main() -> i32 = run(true)\n",
    );
    // run locals: cond=_0, opt=_1, xs=_2; flags follow at _3 and _4.
    assert_flag_is_forwarded(&text, "run", "_4");
}

#[test]
fn try_forwards_unwrapped_owned_flag() {
    let text = mir_text(
        "value-owned-try",
        "fn make() -> array<i64> = [7, 8].to_array()\nfn run(cond: bool) -> Result<i32, Error> {\n  arena {\n    mut r: Result<array<i64>, Error> := Ok([1, 2].to_array())\n    if cond { r = Ok(make()) }\n    xs := r?\n    return Ok(xs[0] as i32)\n  }\n}\nfn main() -> i32 = 0\n",
    );
    // run locals: cond=_0, r=_1, xs=_2; flags follow at _3 and _4.
    assert_flag_is_forwarded(&text, "run", "_4");
}

const RETURN_COMPLETENESS_SOURCE: &str = "import std.process\nChoice { A, B }\nfn tail() -> i64 = 11\nfn explicit() -> i64 { return 12 }\nfn branch(flag: bool) -> i64 { if flag { return 13 } else { return 14 } }\nfn selected(value: Choice) -> i64 { result := match value { A => { return 15 } B => 16 }\nreturn result\n}\nfn loop_value() -> i64 = loop { break 17 }\nfn endless() -> i64 { loop {} }\nfn exits() -> i64 { process.exit(18)\n}\nfn aborts() -> i64 { process.abort()\n}\nfn unwrapped(value: Option<i64>) -> i64 { x := value else { return 20 }\nreturn x\n}\nfn tried(value: Result<i64, Error>) -> Result<i64, Error> { x := value?\nreturn Ok(x)\n}\nfn arena_return() -> i64 { arena { return 21 } }\nfn unsafe_return() -> i64 { unsafe { return 22 } }\nfn group_return() -> i64 { task_group { return 23 } }\nfn moved() -> string = \"owned\".clone()\nfn dead_tail() -> i64 { return 24\nprint(25)\n}\nfn main() -> i32 { print(tail() + explicit() + branch(true) + selected(Choice.A) + loop_value() + dead_tail())\nprint(moved().len())\nprint(unwrapped(Some(26)) + match tried(Ok(27)) { Ok(value) => value Err(error) => 0 } + arena_return() + unsafe_return() + group_return())\nreturn 0\n}\n";

fn assert_non_unit_mir_returns_are_typed(program: &align_driver::MirProgram, path: &str) {
    for function in program.fns.iter().filter(|function| function.ret != align_sema::Ty::Unit) {
        assert!(
            function
                .blocks
                .iter()
                .all(|block| !matches!(block.term, align_mir::Term::Return(None))),
            "{path} {} must not contain a void return under {:?}",
            function.name,
            function.ret
        );
    }
}

fn llvm_function_body<'a>(llvm: &'a str, name: &str) -> &'a str {
    llvm.find(&format!(" @{name}("))
        .map(|start| &llvm[start..])
        .and_then(|tail| tail.split_once("{\n").map(|(_, body)| body))
        .and_then(|body| body.split("\n}").next())
        .unwrap_or_else(|| panic!("missing @{name}:\n{llvm}"))
}

#[test]
fn function_return_completeness_matrix() {
    let mut source_map = SourceMap::new();
    let checked = check(
        &mut source_map,
        "return-completeness-whole",
        RETURN_COMPLETENESS_SOURCE,
    );
    assert!(
        !checked.diags.has_errors(),
        "whole-program fixture must check:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags)
    );
    let whole = lower_to_mir(&checked.hir);
    let per = build_per_unit_multi(
        "return-completeness-per-unit",
        &[("main.align", RETURN_COMPLETENESS_SOURCE)],
        "main.align",
    );
    assert!(
        !per.walk.diags.has_errors(),
        "per-unit fixture must check"
    );
    let per_entry = per
        .walk
        .units
        .iter()
        .find(|unit| unit.is_entry)
        .expect("per-unit entry");
    assert_non_unit_mir_returns_are_typed(&whole, "whole-program");
    assert_non_unit_mir_returns_are_typed(&per_entry.mir, "per-unit");

    for (case, source, message) in [
        (
            "bare",
            "fn bad() -> i64 { return }\nfn main() -> i32 = 0\n",
            "return without a value is only valid in a function returning (); this function returns i64",
        ),
        (
            "fallthrough",
            "fn bad() -> i64 { value := 1 }\nfn main() -> i32 = 0\n",
            "function returning i64 has a reachable path without a return value",
        ),
    ] {
        let differential = diff_check_multi(
            &format!("return-completeness-rejected-{case}"),
            &[("main.align", source)],
            "main.align",
        );
        assert!(
            differential.whole_errors && differential.per_unit_errors,
            "{case} must reject in both modes:\nwhole:\n{}\nper-unit:\n{}",
            differential.whole_diags,
            differential.per_unit_diags
        );
        assert!(differential.whole_diags.contains(message), "{case} whole diagnostic");
        assert!(differential.per_unit_diags.contains(message), "{case} per-unit diagnostic");
        assert!(
            differential.per_unit.summaries.is_empty(),
            "{case} must publish no interface/cacheable unit"
        );
    }

    if !backend_available() {
        return;
    }
    let exports = [
        "tail",
        "explicit",
        "branch",
        "selected",
        "loop_value",
        "endless",
        "exits",
        "aborts",
        "unwrapped",
        "tried",
        "arena_return",
        "unsafe_return",
        "group_return",
        "moved",
        "dead_tail",
    ]
    .map(str::to_string);
    for (path, program) in [("whole-program", &whole), ("per-unit", &per_entry.mir)] {
        for optimized in [false, true] {
            let llvm = emit_llvm_ir(program, BuildTarget::Baseline, optimized, &exports, false)
                .unwrap_or_else(|error| panic!("{path} optimized={optimized}: {error}"));
            for name in &exports {
                let definition = llvm
                    .lines()
                    .find(|line| {
                        line.starts_with("define ") && line.contains(&format!("@{name}("))
                    })
                    .unwrap_or_else(|| panic!("{path} optimized={optimized}: missing @{name}"));
                assert!(
                    !definition.split_whitespace().any(|word| word == "void"),
                    "{path} optimized={optimized}: {definition}"
                );
                assert!(
                    !llvm_function_body(&llvm, name).contains("ret void"),
                    "{path} optimized={optimized}: @{name} emitted ret void"
                );
            }
        }
    }

    let whole_run = build_and_run("return-completeness-whole-run", RETURN_COMPLETENESS_SOURCE);
    let per_run = per.link_and_run();
    assert_eq!(whole_run.status.code(), Some(0));
    assert_eq!(per_run.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&whole_run.stdout), "92\n5\n119\n");
    assert_eq!(whole_run.stdout, per_run.stdout);

    let project = Proj::new(
        "return-completeness-cache",
        &[("main.align", RETURN_COMPLETENESS_SOURCE)],
        "main.align",
    );
    let entry = project.dir.join("main.align");
    let source = std::fs::read_to_string(&entry).expect("read cache fixture");
    let mut source_map = SourceMap::new();
    let walk = align_driver::build_per_unit(
        &mut source_map,
        &entry.display().to_string(),
        &source,
    );
    assert!(!walk.diags.has_errors());
    let unit = walk.units.iter().find(|unit| unit.is_entry).expect("cache entry");
    let cache = project.cache();
    let cold = align_driver::emit_object_cached(
        &cache,
        &unit.unit,
        unit.summary.impl_hash,
        &unit.dep_interface_hashes,
        &unit.mir,
        &project.dir.join("cold.o"),
        BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
    )
    .expect("cold object");
    let hot = align_driver::emit_object_cached(
        &cache,
        &unit.unit,
        unit.summary.impl_hash,
        &unit.dep_interface_hashes,
        &unit.mir,
        &project.dir.join("hot.o"),
        BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
    )
    .expect("hot object");
    assert!(!cold.hit, "first accepted build must miss");
    assert!(hot.hit, "unchanged accepted build must hit");
}
