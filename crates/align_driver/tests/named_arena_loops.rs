//! Exact lexical binding inventory for named arenas inside loops.
#![cfg(unix)]
mod common;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{UnitReuse, build_package};
use common::*;

const REGIONS: &str = r#"module regions
fn ready(at:i64, fail:bool)->Result<i64,i64> {
  if fail && at==2 { return Err(7) }
  return Ok(at)
}
pub fn repeat(count:i64, early:bool, fail:bool)->Result<i64,i64> {
  mut at:=0
  mut total:=0
  loop {
    if at>=count { break }
    arena region {
      text:="loop".clone_in(region)
      total=total+text.len()
      if early { return Ok(total) }
      step:=ready(at,fail)?
      total=total+step
    }
    at=at+1
  }
  return Ok(total)
}
pub fn identity<T>(value:T)->T {
  loop { arena region {}; break }
  return value
}
"#;

const MAIN: &str = r#"module main
import regions
fn main()->i32 {
  zero:=regions.repeat(0,false,false) else {return 1}
  one:=regions.repeat(1,false,false) else {return 2}
  many:=regions.repeat(4,false,false) else {return 3}
  early:=regions.repeat(4,true,false) else {return 4}
  if zero!=0 || one!=4 || many!=22 || early!=4 {return 5}
  match regions.repeat(4,false,true) {
    Ok(value)=>{return 6},
    Err(error)=>{if error!=7 {return 7}},
  }
  if regions.identity(9)!=9 {return 8}
  return 0
}
"#;

#[test]
fn named_arena_loops_execute_across_units_and_replay_cache() {
    owned_fixture::run(
        "named_arena_loops_execute_across_units_and_replay_cache",
        |stage| {
            let files = [("regions.align", REGIONS), ("main.align", MAIN)];
            let checked = assert_same_verdict("named-loop-check", &files, "main.align");
            assert!(!checked.diags.has_errors());
            for output in [
                build_and_run_multi("named-loop-whole", &files, "main.align"),
                build_per_unit_multi("named-loop-units", &files, "main.align").link_and_run(),
            ] {
                assert!(output.status.success(), "{output:?}");
                assert!(
                    output.stdout.is_empty() && output.stderr.is_empty(),
                    "{output:?}"
                );
            }
            for (name, source) in files {
                std::fs::write(stage.join(name), source).unwrap();
            }
            let cache = CacheContext::at(stage.join("cache"));
            let entry = stage.join("main.align");
            for round in 0..2 {
                let mut sources = SourceMap::new();
                let mut built = build_package(
                    &mut sources,
                    entry.to_str().unwrap(),
                    MAIN,
                    &cache,
                    UnitReuse::Allowed,
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
                for index in 0..built.units.len() {
                    built
                        .materialize(index)
                        .expect("cached named loop must validate and lower");
                }
            }
        },
    );
}

#[test]
fn named_loop_regions_keep_scope_escape_and_break_restrictions() {
    for (name, source, diagnostic) in [
        (
            "scope",
            "fn use_region(out:region) {}\nfn main(){loop{arena region {};use_region(region);break}}",
            "undefined name: 'region'",
        ),
        (
            "escape",
            "fn leak()->str {loop {arena region {return \"bad\".clone_in(region)}}}\nfn main(){}",
            "cannot return a value allocated in an arena",
        ),
        (
            "break",
            "fn main(){loop{arena region {break}}}",
            "a `break` inside an `arena`/`task_group` nested in the loop is not supported yet",
        ),
    ] {
        let errors = check_diagnostics("named-loop-negative", source);
        assert!(errors.contains(diagnostic), "{name}: {errors}");
        assert!(!errors.contains("internal error"), "{name}: {errors}");
    }
}
