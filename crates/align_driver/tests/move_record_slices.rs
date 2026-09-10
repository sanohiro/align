//! Borrowed views retain existing Move elements without copying their owners.
mod common;
use common::*;

const HELPER: &str = r#"module helper
pub Row { text: string, number: i64 }
pub fn field(view: slice<Row>) -> str = view[0].text
pub fn inspect(borrow row: Row) -> str { text: str := row.text; return text }
pub fn via_call(view: slice<Row>) -> str = inspect(view[0])
pub fn indirect(view: slice<Row>) -> str { f := inspect; return f(view[0]) }
pub fn make() -> array<Row> {
  mut builder: array_builder<Row> := array_builder()
  builder.push(Row { text: "owned".clone(), number: 7 })
  return builder.build()
}
"#;
#[test]
fn field_and_shared_calls() {
    let main = r#"import helper
fn main() -> i32 {
  rows := helper.make()
  view: slice<helper.Row> := rows
  print(view[0].text)
  print(view[0].number)
  print(helper.field(view[0..1]))
  print(helper.via_call(view))
  print(helper.indirect(view))
  print(helper.inspect(view[0]))
  print(rows[0].text)
  return 0
}
"#;
    let files = &[("helper.align", HELPER), ("main.align", main)];
    let checked = diff_check_multi("move-slice-fields", files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:{}\nunit:{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let out = build_per_unit_multi("move-slice-fields", files, "main.align").link_and_run();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"owned\n7\nowned\nowned\nowned\nowned\nowned\n");
    }
}

fn local_prefix() -> String {
    HELPER.replace("module helper\n", "").replace("pub ", "")
        + r#"
fn consume(rows: array<Row>) -> i64 = rows.len()
fn inspect_with(borrow row: Row, value: i64) -> i64 = row.number + value
fn shrink(borrow mut view: slice<Row>) -> i64 { view = view[0..0]; return 0 }
Views { view: slice<Row> }
Text { text: str }
fn retain(borrow row: Row, borrow mut target: Text) { text: str := row.text; target.text = text }
fn change(borrow mut row: Row) { row = Row { text: "changed".clone(), number: 0 } }
fn replace(borrow mut rows: array<Row>) -> i64 { rows = make(); return 0 }

"#
}

#[test]
fn source_generation_and_retention() {
    let prefix = local_prefix();
    for (name, body, diagnostic) in [
        (
            "temporary-return",
            "fn bad() -> slice<Row> = make()[0..1] fn main() -> i32 = 0",
            "cannot return",
        ),
        (
            "whole-value",
            "fn main() -> i32 { rows := make(); view: slice<Row> := rows; row := view[0]; return 0 }",
            "Move type",
        ),
        (
            "field-local-return",
            "fn bad() -> str { rows := make(); holder := Views { view: rows }; return inspect(holder.view[0]) } fn main() -> i32 = 0",
            "cannot return",
        ),
        (
            "mutable-retention",
            "fn main() -> i32 { rows := make(); view: slice<Row> := rows; mut target := Text { text: \"\" }; retain(view[0], target); consume(rows); print(target.text); return 0 }",
            "borrow",
        ),
        (
            "exclusive-element",
            "fn main() -> i32 { rows := make(); view: slice<Row> := rows; change(view[0]); return 0 }",
            "Move type",
        ),
        (
            "replace-source",
            "fn main() -> i32 { mut rows := make(); view: slice<Row> := rows; return inspect_with(view[0], replace(rows)) as i32 }",
            "value snapshot was invalidated",
        ),
        (
            "local-return",
            "fn bad() -> str { rows := make(); view: slice<Row> := rows; return inspect(view[0]) } fn main() -> i32 = 0",
            "cannot return",
        ),
        (
            "source-move",
            "fn main() -> i32 { rows := make(); view: slice<Row> := rows; text := inspect(view[0]); consume(rows); print(text); return 0 }",
            "borrow",
        ),
        (
            "index-source",
            "fn main() -> i32 { rows := make(); view: slice<Row> := rows; return inspect_with(view[consume(rows)], 0) as i32 }",
            "value snapshot was invalidated",
        ),
        (
            "later-source",
            "fn main() -> i32 { rows := make(); view: slice<Row> := rows; return inspect_with(view[0], consume(rows)) as i32 }",
            "value snapshot was invalidated",
        ),
        (
            "index-header",
            "fn main() -> i32 { rows := make(); mut view: slice<Row> := rows; return inspect_with(view[shrink(view)], 0) as i32 }",
            "value snapshot was invalidated",
        ),
        (
            "later-header",
            "fn main() -> i32 { rows := make(); mut view: slice<Row> := rows; return inspect_with(view[0], shrink(view)) as i32 }",
            "value snapshot was invalidated",
        ),
        (
            "field-header",
            "fn main() -> i32 { rows := make(); mut holder := Views { view: rows }; return inspect_with(holder.view[0], shrink(holder.view)) as i32 }",
            "value snapshot was invalidated",
        ),
    ] {
        let source = format!("{prefix}\n{body}\n");
        let diagnostics = check_diagnostics(&format!("move-slice-{name}"), &source);
        assert!(diagnostics.contains(diagnostic), "{name}: {diagnostics}");
    }
}

#[test]
fn control_and_eager_operands() {
    let source = local_prefix()
        + r#"
fn fail() -> Result<i64, Error> = Err(Error.Invalid)
fn error_index(view: slice<Row>) -> Result<str, Error> = Ok(inspect(view[fail()?]))
fn index_exit(view: slice<Row>) -> i32 { print(view[{ return 11 }].text); return 1 }
fn start_exit(view: slice<Row>) -> i32 { print(view[{ return 12 }..].len()); return 1 }
fn end_exit(view: slice<Row>) -> i32 { print(view[0..{ return 13 }].len()); return 1 }
fn borrowed_exit(view: slice<Row>) -> i32 { print(inspect(view[{ return 14 }])); return 1 }
fn choose(view: slice<Row>, flag: bool) -> str {
  selected := if flag { view } else { view[0..1] }
  optional: Option<Views> := Some(Views { view: selected })
  held := optional else { return "missing" }
  result: Result<Views, Error> := Ok(held)
  recovered := result.map_err(changed) else { return "error" }
  loop { match optional { Some(_) => { break inspect(recovered.view[0]) }, None => { break recovered.view[0].text } } }
}
fn changed(error: Error) -> Error = error
fn propagated(view: slice<Row>) -> Result<str, Error> {
  result: Result<Views, Error> := Ok(Views { view: view })
  selected := result?
  return Ok(inspect(selected.view[0]))
}
fn traced(view: slice<Row>) -> slice<Row> { print(1); return view }
fn index() -> i64 { print(2); return 0 }
fn main() -> i32 {
  rows := make()
  view: slice<Row> := rows
  match error_index(view) { Err(_) => {}, Ok(_) => { return 5 } }
  print(index_exit(view)); print(start_exit(view)); print(end_exit(view)); print(borrowed_exit(view))
  print(traced(view)[index()].text)
  print(choose(view, true)); print(choose(view, false)); print(propagated(view) else { return 4 })
  return 0
}
"#;
    let diagnostics = check_diagnostics("move-slice-control", &source);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    if backend_available() {
        let out = build_and_run("move-slice-control", &source);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(
            out.stdout,
            b"11\n12\n13\n14\n1\n2\nowned\nowned\nowned\nowned\n"
        );
        let per_unit = build_per_unit_multi(
            "move-slice-control-unit",
            &[("main.align", &source)],
            "main.align",
        )
        .link_and_run();
        assert_eq!(per_unit.status.code(), Some(0));
        assert_eq!(per_unit.stdout, out.stdout);
        for bound in [-1, 1] {
            let invalid = format!(
                "{}\nfn index() -> i64 = {bound}\nfn main() -> i32 {{ rows := make(); view: slice<Row> := rows; print(view[index()].text); return 0 }}\n",
                local_prefix()
            );
            let out = build_and_run(&format!("move-slice-bounds-{bound}"), &invalid);
            assert!(
                !out.status.success(),
                "out-of-range slice field read succeeded"
            );
        }
    }
}

#[test]
fn formation_and_type_domain() {
    let source = local_prefix()
        + r#"
fn forward<T>(value: T) -> T = value
fn main() -> i32 {
  rows := [Row { text: "fixed".clone(), number: 9 }]
  view: slice<Row> := rows
  print(forward(view)[0].text)
  print(field(rows))
  holder := Views { view: rows }
  print(inspect(holder.view[0]))
  mut builder: array_builder<string> := array_builder()
  builder.push("string".clone())
  strings := builder.build()
  texts: slice<string> := strings
  print(texts[0]); print(texts[0..1][0]); print(strings[0])
  return 0
}
"#;
    let diagnostics = check_diagnostics("move-slice-formation", &source);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    if backend_available() {
        let out = build_and_run("move-slice-formation", &source);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(out.stdout, b"fixed\nfixed\nfixed\nstring\nstring\nstring\n");
    }
    for (name, declaration) in [
        (
            "move-sum",
            "E { A(string), B }\nfn reject(xs: slice<E>) -> i64 = xs[0..1].len()",
        ),
        (
            "buffer",
            "fn reject(xs: slice<buffer>) -> i64 = xs[0..1].len()",
        ),
    ] {
        let diagnostics = check_diagnostics(
            &format!("move-slice-{name}"),
            &format!("{declaration}\nfn main() -> i32 = 0\n"),
        );
        assert!(
            (diagnostics.contains("Move type") || diagnostics.contains("slice element cannot"))
                && !diagnostics.contains("internal error"),
            "{diagnostics}"
        );
    }
    for element in ["Row", "string"] {
        for (name, body) in [
            (
                "shuffle",
                "fn reject(out xs: slice<E>) { mut r := rand.seed_with(1); r.shuffle(xs) }",
            ),
            (
                "sample",
                "fn reject(xs: slice<E>) -> i64 { mut r := rand.seed_with(1); ys := r.sample(xs, 1); return ys.len() }",
            ),
            (
                "map-into",
                "fn reject(xs: slice<E>, out ys: slice<E>) { xs.map_into(ys) }",
            ),
            (
                "collect",
                "fn reject(xs: slice<E>) -> i64 = xs.to_array().len()",
            ),
            (
                "chunks",
                "fn reject(xs: slice<E>) -> i64 = xs.chunks(1).len()",
            ),
            (
                "map",
                "fn identity(x: E) -> E = x\nfn reject(xs: slice<E>) -> i64 = xs.map(identity).count()",
            ),
            (
                "write",
                "fn reject(out xs: slice<E>, value: E) { xs[0] = value }",
            ),
        ] {
            let source = format!(
                "import std.rand\n{}\n{}\nfn main() -> i32 = 0\n",
                local_prefix(),
                body.replace('E', element)
            );
            let diagnostics = check_diagnostics(&format!("move-slice-{name}-{element}"), &source);
            assert!(
                !diagnostics.is_empty() && !diagnostics.contains("internal error"),
                "{name}/{element}: {diagnostics}"
            );
        }
    }
}

const CLEANUP_HELPER: &str = r#"module helper
Row { text: string }
extern "C" fn align_rt_alloc_count() -> i64
fn inspect(borrow row: Row) -> str { value: str := row.text; return value }
fn early() -> Result<(), Error> {
  mut builder: array_builder<Row> := array_builder()
  builder.push(Row { text: "owned".clone() })
  rows := builder.build()
  before := unsafe { align_rt_alloc_count() }
  view: slice<Row> := rows
  nested := view[0..1]
  if inspect(nested[0]) != "owned" || rows[0].text != "owned" { return Err(Error.Invalid) }
  if unsafe { align_rt_alloc_count() } != before { return Err(Error.Invalid) }
  cloned := inspect(nested[0]).clone()
  if cloned != "owned" { return Err(Error.Invalid) }
  return Err(Error.NotFound)
}
pub fn exercise() -> i32 {
  match early() { Err(NotFound) => { return 0 }, _ => { return 2 } }
}
"#;

#[test]
fn owned_source_cleanup() {
    let main = r#"import helper
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  before_alloc := unsafe { align_rt_alloc_count() }
  before_free := unsafe { align_rt_free_count() }
  if helper.exercise() != 0 { return 2 }
  allocated := unsafe { align_rt_alloc_count() } - before_alloc
  freed := unsafe { align_rt_free_count() } - before_free
  if allocated != 2 { return 3 }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 1 }
  if freed != 3 { return 4 }
  return 0
}
"#;
    if backend_available() {
        for per_unit in [false, true] {
            let good = run_slice_cleanup_probe(main, per_unit, false);
            assert_eq!(
                good.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&good.stdout)
            );
            assert_eq!(
                run_slice_cleanup_probe(main, per_unit, true).status.code(),
                Some(1)
            );
        }
    }
}

fn run_slice_cleanup_probe(main: &str, per_unit: bool, omit_drop: bool) -> std::process::Output {
    let project = Proj::new(
        "move-slice-cleanup",
        &[("helper.align", CLEANUP_HELPER), ("main.align", main)],
        "main.align",
    );
    let entry = project.dir.join("main.align");
    let mut map = SourceMap::new();
    let mut programs = if per_unit {
        let walk = build_per_unit(&mut map, &entry.display().to_string(), main);
        assert!(
            !walk.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&map, &walk.diags)
        );
        walk.units
            .into_iter()
            .map(|unit| unit.mir)
            .collect::<Vec<_>>()
    } else {
        let checked = check(&mut map, &entry.display().to_string(), main);
        assert!(!checked.diags.has_errors());
        vec![lower_to_mir(&checked.hir)]
    };
    if omit_drop {
        let mut removed = 0;
        for program in &mut programs {
            for function in &mut program.fns {
                if !function.name.as_str().ends_with("$early") {
                    continue;
                }
                for block in &mut function.blocks {
                    block.stmts.retain(|statement| {
                        let remove = matches!(statement, align_mir::Stmt::Drop(slot) if matches!(function.slots[*slot as usize], align_sema::Ty::DynStructArray(..)));
                        if remove { removed += 1; }
                        !remove
                    });
                    block.stmt_lines.clear();
                }
            }
        }
        assert!(removed > 0, "negative control removed no source Drop");
    }
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = project.dir.join(format!("unit{index}.o"));
        emit_object_file(
            program,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
        )
        .expect("emit slice probe");
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
    }
    let executable = project.dir.join("probe");
    let refs = objects
        .iter()
        .map(|path| path.as_path())
        .collect::<Vec<_>>();
    link_objects(
        &align_driver::CDriver::default(),
        &refs,
        &executable,
        &libraries,
        Profile::Release,
    )
    .expect("link slice probe");
    std::process::Command::new(executable)
        .output()
        .expect("run slice probe")
}

#[test]
fn interfaces_and_cache() {
    let main = "import helper\nfn main() -> i32 { rows := helper.make(); view: slice<helper.Row> := rows; print(helper.via_call(view))
  print(helper.indirect(view)); return 0 }\n";
    let files = &[("helper.align", HELPER), ("main.align", main)];
    let checked = diff_check_multi("move-slice-cache", files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let project = Proj::new("move-slice-cache", files, "main.align");
        let cache = project.cache();
        assert!(thin_build(&project, &cache, 1).all_miss());
        assert!(thin_build(&project, &cache, 1).all_hit());
        project.write("helper.align", &HELPER.replace("owned", "edited"));
        assert!(!thin_build(&project, &cache, 1).all_hit());
        project.write("helper.align", HELPER);
        assert!(thin_build(&project, &cache, 1).all_hit());
    }
}
