//! L3 package-defined native resources: ownership, provenance, interface, and codegen parity.

mod common;
use common::*;

const INTERNAL: &str = "\
module pkg.db.internal.resource
pub fn drop_conn(handle: raw) {
  unsafe { raw.free(handle) }
}
";

const ROOT: &str = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub fn open() -> conn {
  unsafe {
    handle := raw.alloc(8)
    return resource.from_raw(handle)
  }
}
pub fn present(reference: resource_ref<conn>) -> bool {
  unsafe { return !resource.raw(reference).is_null() }
}
";

fn files(entry: &str) -> [(&str, &str); 3] {
    [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", ROOT),
        ("main.align", entry),
    ]
}

fn assert_rejected(name: &str, project: &[(&str, &str)], needle: &str) {
    let differential = diff_check_multi(name, project, "main.align");
    assert!(
        differential.whole_errors && differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags
    );
    assert!(
        differential.whole_diags.contains(needle),
        "missing `{needle}` in whole-program diagnostics:\n{}",
        differential.whole_diags
    );
    assert!(
        differential.per_unit_diags.contains(needle),
        "missing `{needle}` in per-unit diagnostics:\n{}",
        differential.per_unit_diags
    );
}

#[test]
fn resource_construct_borrow_drop_whole_and_per_unit() {
    let project = files(
        "module main\nimport pkg.db\nfn main() -> i32 { owner: pkg.db.conn := pkg.db.open(); reference := resource.borrow(owner); if pkg.db.present(reference) { return 42 }; return 0 }\n",
    );
    let differential = diff_check_multi("resource-basic", &project, "main.align");
    assert_eq!(
        differential.whole_errors,
        differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}\nresources: {:?}",
        differential.whole_diags,
        differential.per_unit_diags,
        differential
            .per_unit
            .summaries
            .iter()
            .map(|summary| (&summary.unit, &summary.resources))
            .collect::<Vec<_>>()
    );
    assert!(
        !differential.whole_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    if backend_available() {
        assert_eq!(
            build_and_run_multi("resource-basic-whole", &project, "main.align")
                .status
                .code(),
            Some(42),
        );
        assert_eq!(
            build_per_unit_multi("resource-basic-units", &project, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42),
        );
    }
}

#[test]
fn resource_tuple_annotations_preserve_move_and_reference_provenance() {
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub fn open() -> conn { unsafe { return resource.from_raw(raw.alloc(8)) } }
pub fn present(reference: resource_ref<conn>) -> bool {
  unsafe { return !resource.raw(reference).is_null() }
}
pub fn move_through(pair: (conn, i64)) -> conn {
  (owner, _) := pair
  return owner
}
pub fn inspect_through(pair: (resource_ref<conn>, i64)) -> bool {
  (reference, _) := pair
  return present(reference)
}
";
    let project = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 { reference_owner := pkg.db.open(); reference := resource.borrow(reference_owner); if !pkg.db.inspect_through((reference, 7)) { return 1 }; move_owner := pkg.db.open(); moved := pkg.db.move_through((move_owner, 9)); if pkg.db.present(resource.borrow(moved)) { return 42 }; return 2 }\n",
        ),
    ];
    let differential = diff_check_multi("resource-tuple-annotations", &project, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    assert!(
        !differential.whole_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    let per_unit = build_per_unit_multi("resource-tuple-annotations-units", &project, "main.align");
    let producer_mir = align_mir::print::program_to_string(&per_unit.unit("pkg.db").mir);
    assert!(
        producer_mir.contains("fn pkg.db$move_through")
            && producer_mir.contains("fn pkg.db$inspect_through"),
        "resource tuples must survive the checked-HIR boundary:\n{producer_mir}"
    );
    if backend_available() {
        assert_eq!(
            build_and_run_multi("resource-tuple-annotations", &project, "main.align")
                .status
                .code(),
            Some(42),
        );
        assert_eq!(
            per_unit.link_and_run().status.code(),
            Some(42),
        );
    }
}

#[test]
fn representation_operations_require_descendant_privilege_even_in_unsafe() {
    let project = files(
        "module main\nimport pkg.db\nfn main() -> i32 { unsafe { handle := raw.alloc(8); owner: pkg.db.conn := resource.from_raw(handle); print(resource.raw(resource.borrow(owner)).is_null()) }; return 0 }\n",
    );
    assert_rejected(
        "resource-foreign-privilege",
        &project,
        "resource representation is private",
    );
}

#[test]
fn resource_reference_rejects_use_after_owner_move() {
    let project = files(
        "module main\nimport pkg.db\nfn main() -> i32 { owner := pkg.db.open(); reference := resource.borrow(owner); moved := owner; print(pkg.db.present(reference)); print(resource.borrow(moved)); return 0 }\n",
    );
    assert_rejected(
        "resource-stale-reference",
        &project,
        "while dependent resource/reference",
    );
}

#[test]
fn dependent_resource_blocks_parent_move_until_child_drop() {
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub resource stmt = pkg.db.internal.resource.drop_conn
pub fn open() -> conn { unsafe { return resource.from_raw(raw.alloc(8)) } }
pub fn prepare(parent: resource_ref<conn>) -> stmt {
  unsafe { return resource.from_raw_borrowed(raw.alloc(8), parent) }
}
";
    let project = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 { parent := pkg.db.open(); child := pkg.db.prepare(resource.borrow(parent)); moved := parent; print(resource.borrow(child)); print(resource.borrow(moved)); return 0 }\n",
        ),
    ];
    assert_rejected(
        "resource-dependent-parent",
        &project,
        "while dependent resource/reference",
    );
}

#[test]
fn dependent_resource_blocks_mutable_owner_borrow() {
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub resource stmt = pkg.db.internal.resource.drop_conn
pub fn open() -> conn { unsafe { return resource.from_raw(raw.alloc(8)) } }
pub fn prepare(parent: resource_ref<conn>) -> stmt {
  unsafe { return resource.from_raw_borrowed(raw.alloc(8), parent) }
}
pub fn reset(borrow mut owner: conn) {}
";
    let project = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 { mut parent := pkg.db.open(); child := pkg.db.prepare(resource.borrow(parent)); pkg.db.reset(parent); print(resource.borrow(child)); return 0 }\n",
        ),
    ];
    assert_rejected(
        "resource-dependent-parent-mutable-borrow",
        &project,
        "while dependent resource/reference",
    );
}

#[test]
fn into_raw_accepts_roots_and_rejects_projections_and_temporaries() {
    let valid_root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub fn open() -> conn { unsafe { return resource.from_raw(raw.alloc(8)) } }
pub fn transfer() -> bool {
  owner := open()
  unsafe {
    handle := resource.into_raw(owner)
    present := !handle.is_null()
    raw.free(handle)
    return present
  }
}
pub fn transfer_param(owner: conn) -> bool {
  unsafe {
    handle := resource.into_raw(owner)
    present := !handle.is_null()
    raw.free(handle)
    return present
  }
}
";
    let valid = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", valid_root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 { if pkg.db.transfer() && pkg.db.transfer_param(pkg.db.open()) { return 42 }; return 0 }\n",
        ),
    ];
    let differential = diff_check_multi("resource-transfer-root", &valid, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    assert!(!differential.whole_errors, "{}", differential.whole_diags);
    if backend_available() {
        assert_eq!(
            build_and_run_multi("resource-transfer-root", &valid, "main.align")
                .status
                .code(),
            Some(42),
        );
        assert_eq!(
            build_per_unit_multi("resource-transfer-root-units", &valid, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42),
        );
    }

    let invalid_root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub Holder { owner: conn }
pub fn open() -> conn { unsafe { return resource.from_raw(raw.alloc(8)) } }
pub fn bad_projection() { holder := Holder { owner: open() }; unsafe { resource.into_raw(holder.owner) } }
pub fn bad_temporary() { unsafe { resource.into_raw(open()) } }
";
    let invalid = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", invalid_root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 = 0\n",
        ),
    ];
    assert_rejected(
        "resource-transfer-shape",
        &invalid,
        "requires a standalone owned local",
    );
}

#[test]
fn resources_are_excluded_from_fixed_arrays_and_spawn_captures() {
    let fixed = files(
        "module main\nimport pkg.db\nfn main() -> i32 { owners := [pkg.db.open(), pkg.db.open()]; print(owners.len()); return 0 }\n",
    );
    assert_rejected(
        "resource-fixed-array",
        &fixed,
        "cannot be an element of a fixed array",
    );

    let spawned = files(
        "module main\nimport pkg.db\nfn main() -> i32 { owner := pkg.db.open(); reference := resource.borrow(owner); task_group { task := spawn(fn { pkg.db.present(reference) }); wait(); print(task.get()) }; return 0 }\n",
    );
    assert_rejected("resource-spawn", &spawned, "native resources are non-Send");
}

#[test]
fn drop_hook_contract_is_checked_in_the_producer() {
    let private_internal = "\
module pkg.db.internal.resource
fn drop_conn(handle: raw) { unsafe { raw.free(handle) } }
";
    let project = [
        ("pkg/db/internal/resource.align", private_internal),
        ("pkg/db.align", ROOT),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 = 0\n",
        ),
    ];
    assert_rejected("resource-private-hook", &project, "resource Drop hook");

    let unsafe_missing = "\
module pkg.db.internal.resource
pub fn drop_conn(handle: raw) {}
";
    let project = [
        ("pkg/db/internal/resource.align", unsafe_missing),
        ("pkg/db.align", ROOT),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 = 0\n",
        ),
    ];
    assert_rejected("resource-hook-body", &project, "top-level `unsafe");
}

#[test]
fn null_raw_aborts_before_resource_construction() {
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
extern \"C\" fn memchr(ptr: raw, value: i32, count: u64) -> raw
pub fn fail() {
  unsafe {
    storage := raw.alloc(1)
    raw.store(storage, 0, 0 as u8)
    null := memchr(storage, 1, 1)
    owner: conn := resource.from_raw(null)
    print(resource.raw(resource.borrow(owner)).is_null())
  }
}
";
    let project = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 { pkg.db.fail(); return 0 }\n",
        ),
    ];
    let differential = diff_check_multi("resource-null-construction", &project, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    assert!(!differential.whole_errors, "{}", differential.whole_diags);
    if backend_available() {
        assert!(
            !build_and_run_multi("resource-null-construction-whole", &project, "main.align")
                .status
                .success(),
            "a null native handle must abort before it becomes an owned resource"
        );
        assert!(
            !build_per_unit_multi("resource-null-construction-units", &project, "main.align")
                .link_and_run()
                .status
                .success(),
            "separate compilation must preserve the null-construction abort"
        );
    }
}

#[test]
fn generic_identity_and_hidden_drop_thunk_are_exact_across_units() {
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource stmt<T> = pkg.db.internal.resource.drop_conn
pub fn open() -> stmt<i64> { unsafe { return resource.from_raw(raw.alloc(8)) } }
pub fn expose(reference: resource_ref<stmt<i64>>) -> raw { unsafe { return resource.raw(reference) } }
";
    let project = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 { owner := pkg.db.open(); if pkg.db.expose(resource.borrow(owner)).is_null() { return 1 }; return 0 }\n",
        ),
    ];
    let built = build_per_unit_multi("resource-generic-thunk", &project, "main.align");
    let producer = built.unit("pkg.db");
    let consumer = built.unit("main");
    let producer_mir = align_mir::print::program_to_string(&producer.mir);
    assert!(
        producer_mir.contains("resource_from_raw("),
        "{producer_mir}"
    );
    assert!(producer_mir.contains("resource#0"), "{producer_mir}");

    if backend_available() {
        let producer_ir = emit_llvm_ir(&producer.mir, BuildTarget::Baseline, false, &[], false)
            .expect("producer LLVM");
        let consumer_ir = emit_llvm_ir(&consumer.mir, BuildTarget::Baseline, false, &[], false)
            .expect("consumer LLVM");
        let thunk = "__align_resource_drop$pkg.db$stmt";
        assert!(
            producer_ir.contains(&format!("define hidden void @\"{thunk}\"(ptr")),
            "{producer_ir}"
        );
        assert!(
            consumer_ir.contains(&format!("declare hidden void @\"{thunk}\"(ptr")),
            "{consumer_ir}"
        );

        let objects = built.emit_objects_with(Profile::Dev, false);
        if let Some(producer_symbols) = nm_symbols(
            &objects[built
                .walk
                .units
                .iter()
                .position(|unit| unit.unit == "pkg.db")
                .expect("producer index")],
        ) {
            let consumer_symbols = nm_symbols(
                &objects[built
                    .walk
                    .units
                    .iter()
                    .position(|unit| unit.unit == "main")
                    .expect("consumer index")],
            )
            .expect("consumer symbols");
            assert!(
                producer_symbols
                    .iter()
                    .any(|(kind, name)| *kind != 'U' && name.ends_with(thunk)),
                "producer symbols: {producer_symbols:?}"
            );
            assert!(
                consumer_symbols
                    .iter()
                    .any(|(kind, name)| *kind == 'U' && name.ends_with(thunk)),
                "consumer symbols: {consumer_symbols:?}"
            );
        }
        assert!(built.link_and_run().status.success());
    }
}

#[test]
fn dormant_generic_resource_producer_owns_the_shared_drop_thunk() {
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource stmt<T> = pkg.db.internal.resource.drop_conn
pub fn open<T>() -> stmt<T> { unsafe { return resource.from_raw(raw.alloc(8)) } }
";
    let project = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 { owner: pkg.db.stmt<i64> := pkg.db.open(); return 42 }\n",
        ),
    ];
    let built = build_per_unit_multi("resource-dormant-generic-thunk", &project, "main.align");
    if backend_available() {
        let producer_ir = emit_llvm_ir(
            &built.unit("pkg.db").mir,
            BuildTarget::Baseline,
            false,
            &[],
            false,
        )
        .expect("producer LLVM");
        let consumer_ir = emit_llvm_ir(
            &built.unit("main").mir,
            BuildTarget::Baseline,
            false,
            &[],
            false,
        )
        .expect("consumer LLVM");
        let thunk = "__align_resource_drop$pkg.db$stmt";
        assert!(
            producer_ir.contains(&format!("define hidden void @\"{thunk}\"(ptr")),
            "{producer_ir}"
        );
        assert!(
            consumer_ir.contains(&format!("declare hidden void @\"{thunk}\"(ptr")),
            "{consumer_ir}"
        );
        let output = built.link_and_run();
        assert_eq!(output.status.code(), Some(42));
    }
}

#[test]
fn native_views_validate_every_pointer_length_and_encoding_state() {
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
extern \"C\" fn memchr(ptr: raw, value: i32, count: u64) -> raw

fn owner() -> conn {
  unsafe { return resource.from_raw(raw.alloc(16)) }
}
fn null_pointer(reference: resource_ref<conn>) -> raw {
  unsafe {
    pointer := resource.raw(reference)
    raw.store(pointer, 0, 0 as u8)
    return memchr(pointer, 1, 1)
  }
}
fn valid_text() -> bool {
  owner := owner()
  reference := resource.borrow(owner)
  unsafe {
    pointer := resource.raw(reference)
    raw.store(pointer, 0, 65 as u8)
    text: str := resource.view_from_raw(reference, pointer, 1) else { return false }
    return text.len() == 1
  }
}
fn valid_empty_null() -> bool {
  owner := owner()
  reference := resource.borrow(owner)
  pointer := null_pointer(reference)
  unsafe {
    text: str := resource.view_from_raw(reference, pointer, 0) else { return false }
    return text.len() == 0
  }
}
fn rejects_nonempty_null() -> bool {
  owner := owner()
  reference := resource.borrow(owner)
  pointer := null_pointer(reference)
  unsafe {
    text: str := resource.view_from_raw(reference, pointer, 1) else { return true }
    return text.len() == 99
  }
}
fn rejects_negative_length() -> bool {
  owner := owner()
  reference := resource.borrow(owner)
  unsafe {
    text: str := resource.view_from_raw(reference, resource.raw(reference), -1) else { return true }
    return text.len() == 99
  }
}
fn rejects_unrepresentable_length() -> bool {
  owner := owner()
  reference := resource.borrow(owner)
  unsafe {
    values: slice<i64> := resource.view_from_raw(reference, resource.raw(reference), 9223372036854775807) else { return true }
    return values.len() == 99
  }
}
fn rejects_misalignment() -> bool {
  owner := owner()
  reference := resource.borrow(owner)
  unsafe {
    pointer := raw.offset(resource.raw(reference), 1)
    values: slice<i64> := resource.view_from_raw(reference, pointer, 1) else { return true }
    return values.len() == 99
  }
}
fn rejects_invalid_utf8() -> bool {
  owner := owner()
  reference := resource.borrow(owner)
  unsafe {
    pointer := resource.raw(reference)
    raw.store(pointer, 0, 255 as u8)
    text: str := resource.view_from_raw(reference, pointer, 1) else { return true }
    return text.len() == 99
  }
}
pub fn validate_views() -> bool {
  return valid_text()
    && valid_empty_null()
    && rejects_nonempty_null()
    && rejects_negative_length()
    && rejects_unrepresentable_length()
    && rejects_misalignment()
    && rejects_invalid_utf8()
}
";
    let project = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 { if pkg.db.validate_views() { return 42 }; return 0 }\n",
        ),
    ];
    let differential = diff_check_multi("resource-native-views", &project, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    assert!(!differential.whole_errors, "{}", differential.whole_diags);
    if backend_available() {
        assert_eq!(
            build_and_run_multi("resource-native-views-whole", &project, "main.align")
                .status
                .code(),
            Some(42),
        );
        let per_unit = build_per_unit_multi("resource-native-views-units", &project, "main.align");
        let producer_mir = align_mir::print::program_to_string(&per_unit.unit("pkg.db").mir);
        assert!(
            producer_mir.contains("StrUtf8, empty_null=true, nonnegative=true, align=1, utf8=true"),
            "producer MIR:\n{producer_mir}"
        );
        assert!(
            producer_mir.contains(
                "Slice(Int(IntTy { bits: 64, signed: true })), empty_null=true, nonnegative=true, align=8, utf8=false"
            ),
            "producer MIR:\n{producer_mir}"
        );
        assert_eq!(per_unit.link_and_run().status.code(), Some(42),);
    }
}

#[test]
fn resource_drop_is_exactly_once_across_moves_returns_and_into_raw() {
    let counting_internal = "\
module pkg.db.internal.resource
pub fn drop_conn(handle: raw) {
  unsafe {
    raw.free(handle)
    print(101)
  }
}
";
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub Holder { owner: conn }
pub MaybeConn { Some(conn), None }
pub fn open() -> conn { unsafe { return resource.from_raw(raw.alloc(8)) } }
pub fn consume(owner: conn) {}
pub fn early() { owner := open(); return }
pub fn fallthrough() { owner := open() }
pub fn returned() -> conn { owner := open(); return owner }
pub fn replaced() { mut owner := open(); owner = open() }
pub fn aggregate() { holder := Holder { owner: open() } }
pub fn sum_value() { value := MaybeConn.Some(open()) }
pub fn optional(flag: bool) -> Option<conn> {
  if flag { return Some(open()) }
  return None
}
pub fn option_drop() { value := optional(true) }
pub fn option_else() { owner := optional(true) else { return } }
pub fn fallible(flag: bool) -> Result<conn, Error> {
  if flag { return Ok(open()) }
  return Err(error(1))
}
fn keep_error(value: Error) -> Error = value
pub fn try_drop() -> Result<(), Error> { owner := fallible(true)?; return Ok(()) }
pub fn try_early() -> Result<(), Error> { held := open(); owner := fallible(false)?; consume(owner); return Ok(()) }
pub fn map_drop() { mapped := fallible(true).map_err(keep_error) }
pub fn selected(flag: bool) { owner := if flag { open() } else { open() } }
pub fn looped() {
  mut owner := open()
  mut done := false
  loop {
    if done { break }
    owner = open()
    done = true
  }
}
pub fn transfer() {
  owner := open()
  unsafe {
    handle := resource.into_raw(owner)
    raw.free(handle)
  }
}
";
    let project = [
        ("pkg/db/internal/resource.align", counting_internal),
        ("pkg/db.align", root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 { first := pkg.db.open(); moved := first; pkg.db.consume(moved); pkg.db.early(); pkg.db.fallthrough(); returned := pkg.db.returned(); pkg.db.consume(returned); pkg.db.replaced(); pkg.db.aggregate(); pkg.db.sum_value(); pkg.db.option_drop(); pkg.db.option_else(); tried := pkg.db.try_drop(); early_result := pkg.db.try_early(); pkg.db.map_drop(); pkg.db.selected(true); pkg.db.looped(); pkg.db.transfer(); return 0 }\n",
        ),
    ];
    let differential = diff_check_multi("resource-drop-count", &project, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    assert!(!differential.whole_errors, "{}", differential.whole_diags);
    if backend_available() {
        let whole = build_and_run_multi("resource-drop-count-whole", &project, "main.align");
        assert!(
            whole.status.success(),
            "{}",
            String::from_utf8_lossy(&whole.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&whole.stdout), "101\n".repeat(16));

        let per_unit = build_per_unit_multi("resource-drop-count-units", &project, "main.align")
            .link_and_run();
        assert!(
            per_unit.status.success(),
            "{}",
            String::from_utf8_lossy(&per_unit.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&per_unit.stdout),
            "101\n".repeat(16)
        );
    }
}

#[test]
fn declaration_construction_and_transfer_contracts_fail_closed() {
    let non_public = [
        ("pkg/db/internal/resource.align", INTERNAL),
        (
            "pkg/db.align",
            "module pkg.db\nimport pkg.db.internal.resource\nresource conn = pkg.db.internal.resource.drop_conn\n",
        ),
        ("main.align", "module main\nimport pkg.db\nfn main() -> i32 = 0\n"),
    ];
    assert_rejected(
        "resource-public",
        &non_public,
        "resource declaration must be `pub`",
    );

    let wrong_subtree = [
        ("pkg/db/internal/resource.align", INTERNAL),
        (
            "pkg/db.align",
            "module pkg.db\npub fn drop_conn(handle: raw) { unsafe { raw.free(handle) } }\npub resource conn = pkg.db.drop_conn\n",
        ),
        ("main.align", "module main\nimport pkg.db\nfn main() -> i32 = 0\n"),
    ];
    assert_rejected(
        "resource-hook-subtree",
        &wrong_subtree,
        "must be in the declaring module's",
    );

    let bad_signature_internal = "\
module pkg.db.internal.resource
pub fn drop_conn(handle: i64) { unsafe {} }
";
    let bad_signature = [
        ("pkg/db/internal/resource.align", bad_signature_internal),
        ("pkg/db.align", ROOT),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 = 0\n",
        ),
    ];
    assert_rejected(
        "resource-hook-signature",
        &bad_signature,
        "non-generic `pub fn(raw) -> ()`",
    );

    let missing_expected_root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub fn bad() { unsafe { owner := resource.from_raw(raw.alloc(8)); print(resource.borrow(owner)) } }
";
    let missing_expected = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", missing_expected_root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 = 0\n",
        ),
    ];
    assert_rejected(
        "resource-construction-expected",
        &missing_expected,
        "needs an expected concrete resource type",
    );

    let unsafe_required_root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub fn bad() -> conn { return resource.from_raw(raw.alloc(8)) }
";
    let unsafe_required = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", unsafe_required_root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 = 0\n",
        ),
    ];
    assert_rejected(
        "resource-construction-unsafe",
        &unsafe_required,
        "is an unsafe operation",
    );

    let borrowed_transfer_root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub fn bad(borrow owner: conn) -> raw { unsafe { return resource.into_raw(owner) } }
";
    let borrowed_transfer = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", borrowed_transfer_root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 = 0\n",
        ),
    ];
    assert_rejected(
        "resource-transfer-borrowed-param",
        &borrowed_transfer,
        "standalone owned local or by-value resource parameter",
    );
}

#[test]
fn mutable_call_aliasing_is_recursive_and_argument_order_independent() {
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub resource stmt = pkg.db.internal.resource.drop_conn
pub RefHolder { reference: resource_ref<conn> }
pub fn open() -> conn { unsafe { return resource.from_raw(raw.alloc(8)) } }
pub fn prepare(parent: resource_ref<conn>) -> stmt { unsafe { return resource.from_raw_borrowed(raw.alloc(8), parent) } }
pub fn first(borrow mut owner: conn, peer: RefHolder) {}
pub fn second(peer: RefHolder, borrow mut owner: conn) {}
pub fn child_peer(borrow mut owner: conn, child: stmt) {}
";
    for (name, body) in [
        (
            "resource-alias-first",
            "mut owner := pkg.db.open(); holder := pkg.db.RefHolder { reference: resource.borrow(owner) }; pkg.db.first(owner, holder)",
        ),
        (
            "resource-alias-second",
            "mut owner := pkg.db.open(); holder := pkg.db.RefHolder { reference: resource.borrow(owner) }; pkg.db.second(holder, owner)",
        ),
        (
            "resource-alias-child",
            "mut owner := pkg.db.open(); child := pkg.db.prepare(resource.borrow(owner)); pkg.db.child_peer(owner, child)",
        ),
    ] {
        let entry = format!("module main\nimport pkg.db\nfn main() -> i32 {{ {body}; return 0 }}\n");
        let project = [
            ("pkg/db/internal/resource.align", INTERNAL),
            ("pkg/db.align", root),
            ("main.align", entry.as_str()),
        ];
        assert_rejected(name, &project, "aliases argument");
    }
}

#[test]
fn imported_and_indirect_results_preserve_resource_generations() {
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub resource stmt = pkg.db.internal.resource.drop_conn
pub fn open() -> conn { unsafe { return resource.from_raw(raw.alloc(8)) } }
pub fn prepare(parent: resource_ref<conn>) -> stmt { unsafe { return resource.from_raw_borrowed(raw.alloc(8), parent) } }
pub fn echo_ref(reference: resource_ref<conn>) -> resource_ref<conn> = reference
pub fn echo_stmt(child: stmt) -> stmt = child
";
    for (name, body) in [
        (
            "resource-return-ref-direct",
            "owner := pkg.db.open(); reference := pkg.db.echo_ref(resource.borrow(owner)); moved := owner; print(pkg.db.echo_ref(reference)); print(resource.borrow(moved))",
        ),
        (
            "resource-return-ref-indirect",
            "owner := pkg.db.open(); identity := pkg.db.echo_ref; reference := identity(resource.borrow(owner)); moved := owner; print(identity(reference)); print(resource.borrow(moved))",
        ),
        (
            "resource-return-child-indirect",
            "parent := pkg.db.open(); child := pkg.db.prepare(resource.borrow(parent)); identity := pkg.db.echo_stmt; retained := identity(child); moved := parent; print(resource.borrow(retained)); print(resource.borrow(moved))",
        ),
    ] {
        let entry = format!("module main\nimport pkg.db\nfn main() -> i32 {{ {body}; return 0 }}\n");
        let project = [
            ("pkg/db/internal/resource.align", INTERNAL),
            ("pkg/db.align", root),
            ("main.align", entry.as_str()),
        ];
        assert_rejected(name, &project, "while dependent resource/reference");
    }
}

#[test]
fn captured_resource_references_preserve_joined_owner_generations() {
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub fn open() -> conn { unsafe { return resource.from_raw(raw.alloc(8)) } }
pub fn present(reference: resource_ref<conn>) -> bool { unsafe { return !resource.raw(reference).is_null() } }
";
    for (name, body) in [
        (
            "resource-capture-direct",
            "owner := pkg.db.open(); reference := resource.borrow(owner); callback := fn { reference }; moved := owner; print(pkg.db.present(callback())); print(resource.borrow(moved))",
        ),
        (
            "resource-capture-moved-function",
            "owner := pkg.db.open(); reference := resource.borrow(owner); callback := fn { reference }; moved_callback := callback; moved := owner; print(pkg.db.present(moved_callback())); print(resource.borrow(moved))",
        ),
        (
            "resource-capture-target-join",
            "left := pkg.db.open(); right := pkg.db.open(); left_ref := resource.borrow(left); right_ref := resource.borrow(right); mut callback := fn { left_ref }; if true { callback = fn { right_ref } }; moved := left; print(pkg.db.present(callback())); print(resource.borrow(moved))",
        ),
    ] {
        let entry =
            format!("module main\nimport pkg.db\nfn main() -> i32 {{ {body}; return 0 }}\n");
        let project = [
            ("pkg/db/internal/resource.align", INTERNAL),
            ("pkg/db.align", root),
            ("main.align", entry.as_str()),
        ];
        assert_rejected(name, &project, "while dependent resource/reference");
    }
}

#[test]
fn raw_views_cannot_escape_their_resource_generation() {
    let root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
pub fn bad() -> str {
  owner: conn := unsafe { resource.from_raw(raw.alloc(8)) }
  reference := resource.borrow(owner)
  unsafe {
    text: str := resource.view_from_raw(reference, resource.raw(reference), 0) else { return \"\" }
    return text
  }
}
";
    let project = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 = 0\n",
        ),
    ];
    assert_rejected("resource-view-escape", &project, "borrows local storage");
}

#[test]
fn resources_are_rejected_at_ffi_display_equality_and_dynamic_collection_boundaries() {
    let ffi_root = "\
module pkg.db
import pkg.db.internal.resource
pub resource conn = pkg.db.internal.resource.drop_conn
extern \"C\" fn foreign(owner: conn)
";
    let ffi = [
        ("pkg/db/internal/resource.align", INTERNAL),
        ("pkg/db.align", ffi_root),
        (
            "main.align",
            "module main\nimport pkg.db\nfn main() -> i32 = 0\n",
        ),
    ];
    assert_rejected("resource-ffi", &ffi, "not an FFI-safe");

    for (name, statement, needle) in [
        ("resource-print", "print(owner)", "'print' expects"),
        (
            "resource-equality",
            "print(owner == owner)",
            "has no equality",
        ),
        (
            "resource-dynamic-array",
            "values := [owner].to_array(); print(values.len())",
            "cannot be an element of a fixed array",
        ),
    ] {
        let entry = format!(
            "module main\nimport pkg.db\nfn main() -> i32 {{ owner := pkg.db.open(); {statement}; return 0 }}\n"
        );
        let project = files(&entry);
        assert_rejected(name, &project, needle);
    }
}
