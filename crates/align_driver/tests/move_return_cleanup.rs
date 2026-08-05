//! L2c owner: recursively Move returns carry one path-selected cleanup bit through every call ABI.

mod common;
use common::*;

const SOURCE: &str = r#"
fn owned(mode: i32) -> Option<string> {
  if mode == 0 { return None }
  return Some("owned".clone())
}

fn fallible(mode: i32) -> Result<string, string> {
  if mode == 0 { return Ok("ok".clone()) }
  return Err("err".clone())
}

fn invoke(handler: fn(i32) -> Result<string, string>, mode: i32) -> Result<string, string> = handler(mode)

fn relay(mode: i32) -> Result<string, string> {
  value := fallible(mode)?
  return Ok(value)
}

fn keep_error(message: string) -> string = message
fn mapped(mode: i32) -> Result<string, string> = fallible(mode).map_err(keep_error)
fn copy() -> i32 = 7

fn option_len(value: Option<string>) -> i32 = match value {
  Some(text) => text.len() as i32
  None => 0
}

fn result_len(value: Result<string, string>) -> i32 = match value {
  Ok(text) => text.len() as i32
  Err(text) => text.len() as i32
}

fn main() -> i32 = option_len(owned(1))
  + result_len(invoke(fallible, 0))
  + result_len(invoke(fallible, 1))
  + result_len(relay(0))
  + result_len(relay(1))
  + result_len(mapped(1))
  + copy()
"#;

fn mir_text(source: &str) -> String {
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "move-return-cleanup.align", source);
    assert!(
        !checked.diags.has_errors(),
        "fixture must check:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags)
    );
    align_mir::print::program_to_string(&lower_to_mir(&checked.hir))
}

#[test]
fn move_return_cleanup_is_explicit_in_direct_indirect_and_return_mir() {
    let mir = mir_text(SOURCE);
    assert!(
        mir.contains("call_with_cleanup program owned")
            && mir.contains("call_indirect_with_cleanup")
            && mir.contains("return_with_cleanup"),
        "Move-return value and cleanup bit must share every direct, indirect, and return edge:\n{mir}"
    );
    assert!(
        mir.contains("fn copy() -> i32 borrow=None region=None cleanup=None")
            && mir.contains("call program copy()"),
        "Copy returns must retain the value-only ABI:\n{mir}"
    );
}

#[test]
fn move_return_cleanup_executes_none_some_try_and_map_err_paths() {
    if !backend_available() {
        return;
    }
    assert_eq!(build_and_run("move-return-cleanup", SOURCE).status.code(), Some(25));
}

#[test]
fn imported_move_return_cleanup_matches_whole_program_and_per_unit_abi() {
    if !backend_available() {
        return;
    }
    let files = &[
        (
            "values.align",
            r#"
module values
pub fn owned(flag: bool) -> Option<string> =
  if flag { Some("cross".clone()) } else { None }
pub fn copy() -> i32 = 4
"#,
        ),
        (
            "main.align",
            r#"
import values
fn length(value: Option<string>) -> i32 = match value {
  Some(text) => text.len() as i32
  None => 0
}
fn main() -> i32 = length(values.owned(true)) + length(values.owned(false)) + values.copy()
"#,
        ),
    ];
    let whole = build_and_run_multi("move-return-import-whole", files, "main.align");
    let per_unit = build_per_unit_multi("move-return-import-per-unit", files, "main.align");
    assert_eq!(whole.status.code(), Some(9));
    assert_eq!(per_unit.link_and_run().status.code(), Some(9));
    let main_mir = &per_unit.unit("main").mir;
    assert!(
        align_mir::print::program_to_string(main_mir)
            .contains("call_with_cleanup program values$owned"),
        "the importing unit must consume the producer's DynamicBit ABI"
    );
}
