//! Explicit page preferences survive source checking, interfaces, cache replay and native owners.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;

#[test]
fn buffer_page_policy_whole_per_unit_and_cache() {
    owned_fixture::run("buffer_page_policy_whole_per_unit_and_cache", exercise);
}

fn exercise(stage: &Path) {
    let helper = r#"module helper
pub Holder<T> { bytes: buffer, tag: T }
pub fn policy() -> buffer.page_policy = buffer.page_policy.PreferHuge
pub fn make<T>(tag: T, pages: buffer.page_policy) -> Holder<T> =
  Holder { bytes: buffer.filled(2097152, 65, 16384, pages), tag: tag }
pub fn identity(value: buffer.page_policy) -> buffer.page_policy =
  match value { Default => buffer.page_policy.Default, PreferHuge => buffer.page_policy.PreferHuge }
pub fn failed(pages: buffer.page_policy) -> Result<buffer, Error> {
  owner := buffer.try_filled(2097152, 0, 64, pages)?
  return Err(Error.Invalid)
}
"#;
    let source = r#"import helper
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
fn step(value: i64) -> i64 { print(value); return value }
fn byte() -> u8 { print(2); return 65 }
fn pages() -> buffer.page_policy { print(4); return helper.identity(helper.policy()) }
fn stopped(which: i64) -> i64 {
  if which == 0 { b := buffer({ return 11; 8 }, step(3), pages()) }
  if which == 1 { b := buffer.filled(8, { return 12; 0 as u8 }, step(3), pages()) }
  if which == 2 { b := buffer.try_new(8, { return 13; 64 }, pages()) }
  if which == 4 { b := buffer(8, 64, { return 15; helper.policy() }) }
  if which == 5 { b := buffer.filled(8, 0, 64, { return 16; helper.policy() }) }
  if which == 6 { b := buffer.try_new(8, 64, { return 17; helper.policy() }) }
  b := buffer.try_filled(8, 0, 64, { return 14; helper.policy() })
  return 99
}
fn exercise() -> i32 {
  policy := helper.identity(helper.policy())
  b := buffer.filled(step(1), byte(), step(8), pages())
  zero := buffer.try_filled(step(0), byte(), step(8), pages()) else { return 20 }
  zero_window := buffer(step(0), step(8), pages())
  try_window := buffer.try_new(step(0), step(8), pages()) else { return 21 }
  if b.len() != 1 || b.bytes()[0] != 65 as u8 { return 1 }
  if stopped(0) != 11 || stopped(1) != 12 || stopped(2) != 13 || stopped(3) != 14 { return 2 }
  if stopped(4) != 15 || stopped(5) != 16 || stopped(6) != 17 { return 22 }
  empty := buffer(0, 64, policy)
  window := buffer.try_new(2097152, 4096, policy) else { return 3 }
  if empty.capacity() != 0 || window.capacity() != 2097152 || window.len() != 0 { return 4 }
  mut held := helper.make(7, policy)
  if held.bytes.bytes()[2097151] != 65 as u8 { return 6 }
  held.bytes.append_filled(1, 66 as u8)
  if held.bytes.len() != 2097153 || held.bytes.bytes()[2097152] != 66 as u8 { return 7 }
  moved := held
  if moved.tag != 7 { return 8 }
  mut replace := helper.make(8, policy)
  replace = helper.make(9, buffer.page_policy.Default)
  if replace.tag != 9 { return 11 }
  match helper.failed(policy).map_err(fn e: Error { e }) {
    Ok(_) => { return 12 }, Err(_) => {}
  }
  mut i := 0
  loop {
    if i == 2 { break }
    once := buffer.filled(2097152, 0, 64, if i == 0 { policy } else { buffer.page_policy.Default })
    if once.bytes()[2097151] != 0 as u8 { return 13 }
    i = i + 1
  }
  return 0
}
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  status := exercise()
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 90 }
  return status
}
"#;
    std::fs::write(stage.join("helper.align"), helper).unwrap();
    let entry = stage.join("main.align");
    std::fs::write(&entry, source).unwrap();
    assert!(align_driver::backend_available());
    let context = align_driver::CacheContext::at(stage.join("cache"));
    let mut cold = Vec::new();
    for mode in 0..5 {
        if mode == 4 {
            std::fs::write(
                stage.join("helper.align"),
                helper.replace(
                    "= buffer.page_policy.PreferHuge",
                    "= buffer.page_policy.Default",
                ),
            )
            .unwrap();
        }
        let mut sm = SourceMap::new();
        let programs = if mode >= 2 {
            let mut built = align_driver::build_package(
                &mut sm,
                entry.to_str().unwrap(),
                source,
                &context,
                align_driver::UnitReuse::Allowed,
            );
            assert!(
                !built.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &built.diags)
            );
            if mode == 4 {
                assert!(
                    built
                        .units
                        .iter()
                        .any(|unit| !unit.frontend.as_ref().unwrap().hit)
                );
            } else {
                for unit in &built.units {
                    assert_eq!(unit.frontend.as_ref().unwrap().hit, mode == 3);
                }
            }
            let programs = (0..built.units.len())
                .map(|i| built.materialize(i).unwrap().clone())
                .collect::<Vec<_>>();
            let printed = programs
                .iter()
                .map(align_mir::print::program_to_string)
                .collect::<Vec<_>>();
            if mode == 2 {
                cold = printed;
            } else if mode == 3 {
                assert_eq!(printed, cold);
            } else {
                assert_ne!(printed, cold);
            }
            programs
        } else if mode == 1 {
            let built = align_driver::build_per_unit(&mut sm, entry.to_str().unwrap(), source);
            assert!(
                !built.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &built.diags)
            );
            for unit in &built.units {
                let wire = align_interface::serialize(&unit.summary);
                assert_eq!(
                    wire,
                    align_interface::serialize(&align_interface::deserialize(&wire).unwrap())
                );
            }
            built
                .units
                .into_iter()
                .map(|unit| unit.mir)
                .collect::<Vec<_>>()
        } else {
            let checked = align_driver::check(&mut sm, entry.to_str().unwrap(), source);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &checked.diags)
            );
            vec![align_driver::lower_to_mir(&checked.hir)]
        };
        let mut objects = Vec::new();
        let mut libraries = Vec::new();
        for (index, program) in programs.iter().enumerate() {
            let object = stage.join(format!("{mode}-{index}.o"));
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
        let executable = stage.join(format!("program-{mode}"));
        align_driver::link_objects(
            &align_driver::CDriver::default(),
            &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
            &executable,
            &libraries,
            Profile::Release,
        )
        .unwrap();
        let output = std::process::Command::new(executable).output().unwrap();
        assert!(
            output.status.success(),
            "{:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"1\n2\n8\n4\n0\n2\n8\n4\n0\n8\n4\n0\n8\n4\n");
    }
}

#[test]
fn buffer_page_policy_rejects_wrong_type_arity_and_shadowing() {
    for body in [
        "b := buffer(8, 64, true)",
        "b := buffer.filled(8, 0, 64, 1)",
        "b := buffer.try_new(8, 64, Error.Invalid)",
        "b := buffer.try_filled(8, 0, 64, buffer.page_policy.PreferHuge, 1)",
        "b := buffer(8, buffer.page_policy.PreferHuge)",
        "p: buffer.page_policy<i64> := buffer.page_policy.Default",
        "buffer := 1; p := buffer.page_policy.PreferHuge",
        "b := buffer(1); other := buffer(1, 1, consume(b)); print(b.len())",
    ] {
        let source = format!(
            "fn consume(b: buffer) -> buffer.page_policy = buffer.page_policy.Default\nfn main() {{ {body} }}"
        );
        let mut sm = SourceMap::new();
        let checked = align_driver::check(&mut sm, "main.align", &source);
        assert!(checked.diags.has_errors(), "{body}");
    }
}
