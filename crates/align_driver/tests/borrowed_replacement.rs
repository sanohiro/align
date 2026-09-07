//! Request 39: replacement cleanup is distinct from borrowed function-exit cleanup.

mod common;
use common::*;

#[test]
fn borrowed_replacement_reclaims_buffer_storage() {
    let helper = r#"module helper
pub fn replace(borrow mut window: buffer) { window = buffer(4096) }
pub fn forward(borrow mut window: buffer) { replace(window) }
"#;
    let source = r#"module main
import helper
extern "C" {
  fn align_rt_requested_live_reset()
  fn align_rt_requested_live_bytes() -> i64
  fn align_rt_requested_live_peak() -> i64
}
fn exercise() -> i64 {
  mut window := buffer(4096)
  single := unsafe { align_rt_requested_live_bytes() }
  mut i := 0
  loop {
    if i >= 32 { break }
    helper.forward(window)
    if unsafe { align_rt_requested_live_bytes() } != single { return -1 }
    i = i + 1
  }
  window.put_u8(7)
  if window.bytes().u8(0) != 7 { return -2 }
  return single
}
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  single := exercise()
  if single <= 0 { return 1 }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 2 }
  if unsafe { align_rt_requested_live_peak() } != 2 * single { return 3 }
  return 0
}
"#;
    let files = [("main.align", source), ("helper.align", helper)];
    let checked = diff_check_multi("borrowed-buffer-replacement", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if !backend_available() {
        return;
    }
    for output in [
        build_and_run_multi("borrowed-buffer-replacement-whole", &files, "main.align"),
        build_per_unit_multi("borrowed-buffer-replacement-units", &files, "main.align")
            .link_and_run(),
    ] {
        assert!(
            output.status.success(),
            "replacement must reclaim old storage before the next call: {:?}\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn borrowed_replacement_preserves_drop_order() {
    let internal = r#"module pkg.probe.internal.resource
pub fn release(handle: raw) {
  print("drop")
  unsafe { raw.free(handle) }
}
"#;
    let package = r#"module pkg.probe
import pkg.probe.internal.resource
pub resource owner = pkg.probe.internal.resource.release
pub Box<T> { value: T }
pub fn make() -> owner {
  unsafe { return resource.from_raw(raw.alloc(8)) }
}
pub fn keep(borrow mut value: owner) {}
pub fn replace<T>(borrow mut value: T, replacement: T) -> Result<(), Error> {
  value = replacement
  return Err(Error.Invalid)
}
"#;
    let source = r#"module main
import pkg.probe
fn exercise() {
  mut direct := pkg.probe.make()
  print("ready")
  pkg.probe.keep(direct)
  print("unchanged")
  pkg.probe.replace(direct, pkg.probe.make()) else { () }
  print("replaced")
  mut wrapped := pkg.probe.Box { value: pkg.probe.make() }
  pkg.probe.replace(wrapped, pkg.probe.Box { value: pkg.probe.make() }) else { () }
  print("aggregate")
}
fn main() -> i32 {
  exercise()
  print("done")
  return 0
}
"#;
    let files = [
        ("main.align", source),
        ("pkg/probe.align", package),
        ("pkg/probe/internal/resource.align", internal),
    ];
    let checked = diff_check_multi("borrowed-replacement-order", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if !backend_available() {
        return;
    }
    for output in [
        build_and_run_multi("borrowed-replacement-order-whole", &files, "main.align"),
        build_per_unit_multi("borrowed-replacement-order-units", &files, "main.align")
            .link_and_run(),
    ] {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "ready\nunchanged\ndrop\nreplaced\ndrop\naggregate\ndrop\ndrop\ndone\n"
        );
    }
}

#[test]
fn borrowed_replacement_move_class_matrix() {
    for (ty, owned) in [
        ("buffer", true),
        ("reader", true),
        ("writer", true),
        ("string", true),
        ("array<i64>", true),
        ("array_builder<i64>", true),
        ("Option<string>", true),
        ("Row", true),
        ("i64", false),
        ("str", false),
        ("slice<i64>", false),
    ] {
        let source = format!(
            "Row {{ text: string }}\n\
             fn replace(borrow mut dst: {ty}, replacement: {ty}) {{ dst = replacement }}\n\
             fn keep(borrow dst: {ty}) {{}}\nfn main() {{}}\n"
        );
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, "borrowed-replacement-class", &source);
        assert!(
            !checked.diags.has_errors(),
            "{ty}: {}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let mir = lower_to_mir(&checked.hir);
        for (name, should_drop) in [("replace", owned), ("keep", false)] {
            let function = mir
                .fns
                .iter()
                .find(|f| f.name.as_str() == name)
                .unwrap_or_else(|| panic!("missing {name} for {ty}"));
            let destination = function
                .params
                .first()
                .unwrap_or_else(|| panic!("missing destination for {ty}"));
            let drops = function.blocks.iter().flat_map(|block| &block.stmts)
                .filter(|statement| matches!(statement, align_mir::Stmt::Drop(slot) if slot == destination))
                .count();
            assert_eq!(
                drops,
                usize::from(should_drop),
                "{name}({ty}) cleanup must occur only at replacement"
            );
        }
    }
}

#[test]
fn borrowed_replacement_preserves_arena_cleanup() {
    let helper = r#"module helper
pub fn heap(borrow mut dst: array<i64>) { dst = [4, 5].to_array() }
pub fn regional(borrow mut dst: array<i64>, out: region) {
  mut builder: array_builder<i64> := array_builder(out)
  builder.push(7)
  builder.push(8)
  dst = builder.build()
}
"#;
    let source = r#"module main
import helper
fn main() -> i32 {
  arena out {
    mut builder: array_builder<i64> := array_builder(out)
    builder.push(1)
    mut values := builder.build()
    helper.heap(values)
    if values.sum() != 9 { return 1 }
    helper.regional(values, out)
    if values.sum() != 15 { return 2 }
    helper.heap(values)
    if values.sum() != 9 { return 3 }
  }
  return 0
}
"#;
    // Foreign region-argument summaries remain a separate R49 gap. The same-unit control
    // isolates the dynamic arena/heap cleanup bit; imported forwarding is covered above.
    let source = format!(
        "{}\n{}",
        helper.replace("module helper", "module main"),
        source
            .replace("module main\nimport helper", "")
            .replace("helper.", "")
    );
    let files = [("main.align", source.as_str())];
    let checked = diff_check_multi("borrowed-replacement-arena", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if !backend_available() {
        return;
    }
    for output in [
        build_and_run_multi("borrowed-replacement-arena-whole", &files, "main.align"),
        build_per_unit_multi("borrowed-replacement-arena-units", &files, "main.align")
            .link_and_run(),
    ] {
        assert!(
            output.status.success(),
            "{:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
