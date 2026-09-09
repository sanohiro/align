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

fn bound_if_families() -> String {
    let mut source = String::from(
        "module choices\npub Record { text: string }\npub Choice { Text(string), Empty }\n",
    );
    for (name, constructor) in [
        ("text", "\"owned\".clone()"),
        ("numbers", "[1, 2].to_array()"),
        ("texts", "{ mut b: array_builder<string> := array_builder(); b.push(\"owned\".clone()); b.build() }"),
        ("records", "{ mut b: array_builder<Record> := array_builder(); b.push(Record{text: \"owned\".clone()}); b.build() }"),
        ("chunks", "[1, 2].chunks(1)"),
        ("record", "Record{text: \"owned\".clone()}"),
        ("tuple", "(\"owned\".clone(), [1, 2].to_array())"),
        ("optional", "Some(\"owned\".clone())"),
        ("result", "Ok(\"owned\".clone())"),
        ("sum", "Choice.Text(\"owned\".clone())"),
        ("bytes", "buffer(8)"),
    ] {
        let annotation = if name == "result" { ": Result<string, Error>" } else { "" };
        source.push_str(&format!("pub fn {name}(c: bool) {{\n  left{annotation} := {constructor}\n  right{annotation} := {constructor}\n  chosen := if c {{ left }} else {{ right }}\n}}\n"));
    }
    source
}

const BOUND_IF_HELPERS: &str = r#"
extern "C" {
  fn align_rt_requested_live_bytes() -> i64
}
pub fn replacement(c: bool) {
  baseline := unsafe { align_rt_requested_live_bytes() }
  replace_buffer(c, baseline)
  print(unsafe { align_rt_requested_live_bytes() } == baseline)
}
fn replace_buffer(c: bool, baseline: i64) {
  mut x := buffer(4096)
  single := unsafe { align_rt_requested_live_bytes() } - baseline
  x = if c { x } else { buffer(4096) }
  print(unsafe { align_rt_requested_live_bytes() } - baseline == single)
  wrapped := { x = if c { x } else { buffer(4096) }; 0 }
  print(unsafe { align_rt_requested_live_bytes() } - baseline == single)
  branch: Option<i64> := if c { Some(1) } else { None }
  x = match branch { Some(_) => x, None => buffer(4096) }
  print(unsafe { align_rt_requested_live_bytes() } - baseline == single)
  wrapper := { x = match branch { Some(_) => x, None => buffer(4096) }; 0 }
  print(unsafe { align_rt_requested_live_bytes() } - baseline == single)
  absent: Option<buffer> := None
  x = (if c { Some(x) } else { absent }) else buffer(4096)
  print(unsafe { align_rt_requested_live_bytes() } - baseline == single)
  x = x
  x = { x }
  print(unsafe { align_rt_requested_live_bytes() } - baseline == single)
  moved := if c { x } else { buffer(4096) }
  x = buffer(4096)
  print(unsafe { align_rt_requested_live_bytes() } - baseline == 2 * single)
}
pub fn pick<T>(c: bool, left: T, right: T) -> T = if c { left } else { right }
pub fn consume(value: string) -> i64 = value.len()
pub fn fallible(value: string) -> Result<string, Error> = Ok(value)
pub fn error_code(error: Error) -> Error = error
pub fn exercise(c: bool) -> Result<(), Error> {
  a := "a".clone()
  b := "bb".clone()
  print(consume(if c { a } else { b }))
  x := "xxx".clone()
  y := "yyyy".clone()
  nested := if c { print(10); if c { x } else { y } } else { print(20); y }
  print(nested)
  optional := Some("option".clone())
  fallback := "fallback".clone()
  selected := if c { optional else "fallback".clone() } else { fallback }
  print(selected)
  tried := fallible("try".clone())
  other := "other".clone()
  result := if c { tried.map_err(error_code)? } else { other }
  print(result)
  matched := "match".clone()
  alternate := "alternate".clone()
  branch: Option<i64> := if c { Some(1) } else { None }
  choice := if c { match branch { Some(_) => matched, None => alternate } } else { alternate }
  print(choice)
  looped := "loop".clone()
  fresh := loop { break if c { looped } else { "fresh".clone() } }
  print(fresh)
  shared := "same".clone()
  same := if c { shared } else { shared }
  print(same)
  mut source := "old".clone()
  moved := if c { source } else { "new".clone() }
  source = "reset".clone()
  print(moved)
  print(source)
  mut replaced := "before".clone()
  second := "after".clone()
  replaced = if c { replaced } else { second }
  print(replaced)
  wrapped := { replaced = match branch { Some(_) => replaced, None => "match-new".clone() }; 0 }
  carried := Some(replaced)
  replaced = carried else "unreachable".clone()
  replaced = replaced
  replaced = { replaced }
  print(replaced)
  heap := [7, 8].to_array()
  arena {
    local := [1, 2].to_array()
    selected_array := if c { heap } else { local }
    print(selected_array[0])
  }
  return Ok(())
}
pub fn early(c: bool) -> string {
  value := "kept".clone()
  return if c { return "early".clone() } else { value }
}
pub fn borrow_only(c: bool) {
  left := "left".clone()
  right := "right".clone()
  print((if c { left } else { right }).len())
  print(left)
  print(right)
}
"#;

fn bound_if_main() -> String {
    let mut main = String::from("import choices\nfn main() -> Result<(), Error> {\n");
    for flag in ["true", "false"] {
        for family in [
            "text", "numbers", "texts", "records", "chunks", "record", "tuple", "optional",
            "result", "sum", "bytes",
        ] {
            main.push_str(&format!("  choices.{family}({flag})\n"));
        }
        main.push_str(&format!("  choices.replacement({flag})\n  choices.exercise({flag})?\n  print(choices.early({flag}))\n  choices.borrow_only({flag})\n"));
    }
    main.push_str("  print(choices.pick(true, \"generic\".clone(), \"unused\".clone()))\n  return Ok(())\n}\n");
    main
}

struct IfProject(std::path::PathBuf);
impl IfProject {
    fn new(main: &str, helpers: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let project = loop {
            let path = std::env::temp_dir().join(format!(
                "align-bound-if-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => break Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("acquire project: {error}"),
            }
        };
        std::fs::write(project.0.join("main.align"), main).unwrap();
        std::fs::write(project.0.join("choices.align"), helpers).unwrap();
        project
    }
    fn entry(&self) -> String {
        self.0.join("main.align").display().to_string()
    }
    fn programs(&self, main: &str, unit: bool) -> Vec<align_driver::MirProgram> {
        let mut sm = SourceMap::new();
        if unit {
            let checked = align_driver::build_per_unit(&mut sm, &self.entry(), main);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &checked.diags)
            );
            checked.units.into_iter().map(|unit| unit.mir).collect()
        } else {
            let checked = check(&mut sm, &self.entry(), main);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &checked.diags)
            );
            vec![lower_to_mir(&checked.hir)]
        }
    }
    fn run(&self, programs: &[align_driver::MirProgram], profile: Profile) -> String {
        let mut objects = Vec::new();
        let mut libraries = Vec::new();
        for (index, program) in programs.iter().enumerate() {
            let object = self.0.join(format!("unit{index}.o"));
            emit_object_file(program, &object, BuildTarget::Baseline, profile, &[], false)
                .expect("codegen");
            objects.push(object);
            for library in &program.link_libs {
                if !libraries.contains(library) {
                    libraries.push(library.clone());
                }
            }
        }
        let exe = self.0.join(format!("run{}", std::env::consts::EXE_SUFFIX));
        let refs = objects
            .iter()
            .map(std::path::PathBuf::as_path)
            .collect::<Vec<_>>();
        link_objects(
            &align_driver::CDriver::default(),
            &refs,
            &exe,
            &libraries,
            profile,
        )
        .expect("link");
        let stdout = self.0.join("stdout");
        let stderr = self.0.join("stderr");
        let mut command = std::process::Command::new(exe);
        command.stdout(std::fs::File::create(&stdout).unwrap());
        command.stderr(std::fs::File::create(&stderr).unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = IfChild(Some(command.spawn().expect("spawn")));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "child deadline: {}",
                std::fs::read_to_string(&stderr).unwrap()
            );
            match child.0.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    child.0.take();
                    assert!(
                        status.success(),
                        "{status}: {}",
                        std::fs::read_to_string(stderr).unwrap()
                    );
                    return std::fs::read_to_string(stdout).unwrap();
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => panic!("wait: {error}"),
            }
        }
    }
}
impl Drop for IfProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct IfChild(Option<std::process::Child>);
impl Drop for IfChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            #[cfg(unix)]
            if let Ok(pid) = i32::try_from(child.id()) {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn bound_if_result_formation_matrix() {
    let helpers = bound_if_families() + BOUND_IF_HELPERS;
    let main = bound_if_main();
    let project = IfProject::new(&main, &helpers);
    for unit in [false, true] {
        assert!(project
            .programs(&main, unit)
            .iter()
            .all(|program| !program.fns.is_empty()));
    }
}

#[test]
fn bound_if_result_execution_matrix() {
    if !backend_available() {
        return;
    }
    let helpers = bound_if_families() + BOUND_IF_HELPERS;
    let main = bound_if_main();
    let project = IfProject::new(&main, &helpers);
    let expected = "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n1\n10\nxxx\noption\ntry\nmatch\nloop\nsame\nold\nreset\nbefore\nbefore\n7\nearly\n4\nleft\nright\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n2\n20\nyyyy\nfallback\nother\nalternate\nfresh\nsame\nnew\nreset\nafter\nmatch-new\n1\nkept\n5\nleft\nright\ngeneric\n";
    for unit in [false, true] {
        let programs = project.programs(&main, unit);
        for profile in [Profile::Dev, Profile::Release] {
            assert_eq!(project.run(&programs, profile), expected);
        }
    }
}

#[test]
fn bound_if_result_borrow_and_rejection_matrix() {
    for (source, message) in [
        ("pub fn bad(c: bool) { a := \"a\".clone(); b := if c { a } else { \"b\".clone() }; print(a) }", "moved"),
        ("pub fn bad(c: bool, borrow a: string) -> string = if c { a } else { \"b\".clone() }", "borrow"),
        ("pub fn bad(c: bool, borrow a: Option<string>) -> string = match a { Some(s) => if c { s } else { \"b\".clone() }, None => \"n\".clone() }", "borrowed match payload"),
        ("pub fn bad(c: bool) -> array<i64> { return arena { a := [1].to_array(); if c { a } else { [2].to_array() } } }", "escape"),
        ("pub fn bad(c: bool) { a := buffer(8); view := a.bytes(); b := if c { a } else { buffer(8) }; print(view.len()) }", "borrow"),
    ] {
        let helpers = format!("module choices\n{source}\n");
        let main = "import choices\nfn main() {}\n";
        let project = IfProject::new(main, &helpers);
        for unit in [false, true] {
            let mut sm = SourceMap::new();
            let diagnostics = if unit {
                let result = align_driver::build_per_unit(&mut sm, &project.entry(), main);
                assert!(result.diags.has_errors(), "accepted {source}");
                align_driver::format_diagnostics(&sm, &result.diags)
            } else {
                let result = check(&mut sm, &project.entry(), main);
                assert!(result.diags.has_errors(), "accepted {source}");
                align_driver::format_diagnostics(&sm, &result.diags)
            };
            assert!(diagnostics.contains(message), "{source}: {diagnostics}");
        }
    }
}

// Execute the small ownership-only MIR graph symbolically. Payload identity makes leaks,
// premature Drop and duplicate Drop observable without allocator reuse or native UB.
#[test]
fn bound_if_result_flag_transfer() {
    use align_mir::{Const, Operand, Rvalue, Stmt, Term};
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Value {
        Empty,
        Bool(bool),
        Payload(usize),
    }
    fn operand(op: &Operand, args: &[Value], values: &[Value]) -> Value {
        match op {
            Operand::Arg(i) => args[*i as usize],
            Operand::Value(i) => values[*i as usize],
            Operand::Const(Const::Bool(b)) => Value::Bool(*b),
            _ => panic!("unexpected operand {op:?}"),
        }
    }
    for assignment in [
        "x = if c { x } else { y }",
        "wrapped := { x = if c { x } else { y }; false }",
    ] {
        let source = format!("fn replace(c: bool, incoming: string, y: string) -> string {{ mut x := incoming; {assignment}; x = x; x = {{ x }}; return x }}\nfn main() {{}}\n");
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, "flag-transfer", &source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &checked.diags)
        );
        let program = lower_to_mir(&checked.hir);
        let function = program.fns.iter().find(|f| f.params.len() == 3).unwrap();
        for condition in [false, true] {
            let args = [Value::Bool(condition), Value::Payload(0), Value::Payload(1)];
            let mut slots = vec![Value::Empty; function.slots.len()];
            let mut values = vec![Value::Empty; function.value_tys.len()];
            let mut drops = [0; 2];
            let mut block = function.entry;
            let mut returned = None;
            for _ in 0..100 {
                let current = &function.blocks[block as usize];
                for stmt in &current.stmts {
                    match stmt {
                        Stmt::Store(slot, op) => {
                            slots[*slot as usize] = operand(op, &args, &values)
                        }
                        Stmt::Let(id, Rvalue::Load(slot)) => {
                            values[*id as usize] = slots[*slot as usize]
                        }
                        Stmt::DropFlagInit(slot) => slots[*slot as usize] = Value::Empty,
                        Stmt::Drop(slot) => {
                            if let Value::Payload(id) = slots[*slot as usize] {
                                drops[id] += 1
                            } else {
                                panic!("dropped cleared source")
                            }
                        }
                        _ => panic!("unexpected statement {stmt:?}"),
                    }
                }
                match &current.term {
                    Term::Goto(next) => block = *next,
                    Term::Branch(op, yes, no) => match operand(op, &args, &values) {
                        Value::Bool(b) => block = if b { *yes } else { *no },
                        other => panic!("invalid branch {other:?}"),
                    },
                    Term::ReturnWithCleanup(pair) => {
                        returned = Some((
                            operand(&pair.0, &args, &values),
                            operand(&pair.1, &args, &values),
                        ));
                        break;
                    }
                    term => panic!("unexpected terminator {term:?}"),
                }
            }
            let selected = if condition { 0 } else { 1 };
            assert_eq!(
                returned,
                Some((Value::Payload(selected), Value::Bool(true)))
            );
            assert_eq!(drops[selected], 0, "returned value dropped");
            assert_eq!(
                drops[1 - selected],
                1,
                "unselected value leaked or dropped twice"
            );
        }
    }
}
