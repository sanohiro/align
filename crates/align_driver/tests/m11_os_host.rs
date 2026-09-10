//! Owned host observations and builtin nominal identity.
mod common;
const _: extern "C" fn() -> i64 = align_runtime::align_rt_alloc_count;
const _: extern "C" fn() -> i64 = align_runtime::align_rt_free_count;
use common::*;

#[test]
fn host_observation() {
    let source = "import std.os\nfn main() -> i32 {\n h := os.host() else { return 1 }\n print(h.system)\n print(h.release)\n print(h.machine)\n print(match h.cpu { Some(value) => true, None => false })\n print(h.logical_cpu_count else 0)\n return 0\n}\n";
    let checked = diff_check_multi("host-observe", &[("main.align", source)], "main.align");
    assert!(!checked.whole_errors && !checked.per_unit_errors, "whole: {}\nunit: {}", checked.whole_diags, checked.per_unit_diags);
    if backend_available() {
        let out = build_and_run("host-observe", source);
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        let mut expected = String::new();
        for flag in ["-s", "-r", "-m"] {
            let oracle = std::process::Command::new("uname").arg(flag).output().expect("uname oracle");
            assert!(oracle.status.success());
            expected.push_str(&String::from_utf8(oracle.stdout).expect("uname UTF-8"));
        }
        expected.push_str("false\n");
        let count = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
        expected.push_str(&format!("{}\n", count.max(0)));
        assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
    }
}

#[test]
fn host_formation() {
    for source in [
        "fn main() { h := os.host() }",
        "import std.os\nfn main() { h := os.host(1) }",
        "import std.os\nfn bad(h: host_info) {}\nfn main() {}",
        "fn bad(h: os.host_info) {}\nfn main() {}",
        "import std.os\nfn main() { h := os.host() else { return }; moved := h.system; print(h.system) }",
        "import std.os\nfn main() { xs := [1,2]; ys := xs.par_map(|x| { h := os.host() else { return x }; x }) }",
    ] {
        let checked = diff_check_multi("host-invalid", &[("main.align", source)], "main.align");
        assert!(checked.whole_errors && checked.per_unit_errors, "accepted: {source}");
    }
}

const HOST_HELPER: &str = r#"module helper
import std.os
pub fn observe() -> Result<os.host_info, Error> = arena { os.host() }
fn forward<T>(value: T) -> T = value
fn early() -> Result<(), Error> {
  h := os.host()?
  return Err(Error.Invalid)
}
fn changed(error: Error) -> Error = error
pub fn exercise() -> Result<(), Error> {
  first := observe()?
  text := first.system
  if text.len() == 0 { return Err(Error.Invalid) }
  mut replacement := forward(os.host()?)
  replacement = os.host()?
  optional: Option<os.host_info> := Some(os.host()?)
  held := optional else { return Err(Error.Invalid) }
  selected := if held.release.len() > 0 { os.host()? } else { os.host()? }
  match early().map_err(changed) { Ok(_) => {}, Err(_) => {} }
  loop { dropped := os.host()?; break }
  custom := os.host_info { system: "s".clone(), release: "r".clone(), machine: "m".clone(), cpu: Some("cpu".clone()), logical_cpu_count: Some(1) }
  moved := custom.cpu
  match moved { Some(description) => { if description.len() != 3 { return Err(Error.Invalid) } }, None => { return Err(Error.Invalid) } }
  return Ok(())
}
"#;

#[test]
fn host_owned_control_flow() {
    let main = r#"import helper
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn main() -> i32 {
  before_alloc := unsafe { align_rt_alloc_count() }
  before_free := unsafe { align_rt_free_count() }
  helper.exercise() else { return 2 }
  allocated := unsafe { align_rt_alloc_count() } - before_alloc
  freed := unsafe { align_rt_free_count() } - before_free
  print(allocated)
  print(freed)
  if allocated < 25 { return 3 }
  if allocated != freed { return 1 }
  return 0
}
"#;
    let files = &[("helper.align", HOST_HELPER), ("main.align", main)];
    let checked = diff_check_multi("host-owned", files, "main.align");
    assert!(!checked.whole_errors && !checked.per_unit_errors, "whole: {}\nunit: {}", checked.whole_diags, checked.per_unit_diags);
    if backend_available() {
        for per_unit in [false,true] {
            let good = run_host_cleanup_probe(main, per_unit, false);
            assert_eq!(good.status.code(), Some(0), "stdout:{} stderr:{}", String::from_utf8_lossy(&good.stdout), String::from_utf8_lossy(&good.stderr));
            let bad = run_host_cleanup_probe(main, per_unit, true);
            assert_eq!(bad.status.code(), Some(1), "omitted Drop must fail: {}", String::from_utf8_lossy(&bad.stdout));
        }
    }
}

#[test]
fn host_interfaces_and_cache() {
    let main = "import helper\nfn main() -> i32 { h := helper.observe() else { return 1 }; print(h.system); return 0 }\n";
    let files = &[("helper.align", HOST_HELPER), ("main.align", main)];
    let checked = diff_check_multi("host-interfaces", files, "main.align");
    assert!(!checked.whole_errors && !checked.per_unit_errors, "whole: {}\nunit: {}", checked.whole_diags, checked.per_unit_diags);
    if backend_available() {
        let out = build_per_unit_multi("host-interfaces", files, "main.align").link_and_run();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let project = Proj::new("host-cache", files, "main.align");
        let cache = project.cache();
        let cold = thin_build(&project, &cache, 1);
        assert!(cold.all_miss());
        let hit = thin_build(&project, &cache, 1);
        assert!(hit.all_hit());

    }
}

fn run_host_cleanup_probe(main: &str, per_unit: bool, omit_drop: bool) -> std::process::Output {
    let project = Proj::new("host-cleanup", &[("helper.align",HOST_HELPER),("main.align",main)], "main.align");
    let entry = project.dir.join("main.align");
    let mut map = SourceMap::new();
    let mut programs = if per_unit {
        let walk = build_per_unit(&mut map, &entry.display().to_string(), main);
        assert!(!walk.diags.has_errors(), "{}", align_driver::format_diagnostics(&map,&walk.diags));
        walk.units.into_iter().map(|unit| unit.mir).collect::<Vec<_>>()
    } else {
        let checked = check(&mut map,&entry.display().to_string(),main);
        assert!(!checked.diags.has_errors());
        vec![lower_to_mir(&checked.hir)]
    };
    if omit_drop {
        let mut removed = 0;
        for program in &mut programs {
            for function in &mut program.fns {
                if !function.name.as_str().ends_with("$early") { continue; }
                for block in &mut function.blocks {
                    block.stmts.retain(|statement| {
                        let remove = matches!(statement, align_mir::Stmt::Drop(slot) if matches!(function.slots[*slot as usize], align_sema::Ty::Struct(id) if align_sema::host_info_schema_valid(&program.structs[id as usize])));
                        if remove { removed += 1; }
                        !remove
                    });
                    block.stmt_lines.clear();
                }
            }
        }
        assert!(removed > 0, "negative control removed no host Drop");
    }
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index,program) in programs.iter().enumerate() {
        let object = project.dir.join(format!("unit{index}.o"));
        emit_object_file(program,&object,BuildTarget::Baseline,Profile::Release,&[],false).expect("emit host probe");
        objects.push(object);
        for library in &program.link_libs { if !libraries.contains(library) { libraries.push(library.clone()); } }
    }
    let executable = project.dir.join("probe");
    let refs = objects.iter().map(|path|path.as_path()).collect::<Vec<_>>();
    link_objects(&align_driver::CDriver::default(),&refs,&executable,&libraries,Profile::Release).expect("link host probe");
    std::process::Command::new(executable).output().expect("run host probe")
}
