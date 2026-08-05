//! L2d/L2e borrowed-parameter formation, ownership, ABI, aliasing, and per-unit parity.

mod common;
use common::*;

#[test]
fn shared_direct_call_keeps_move_owner_live() {
    if !backend_available() {
        return;
    }
    let source = "\
fn size(borrow value: string) -> i64 = value.len()
fn main() -> i32 {
  value := \"forty-two\".clone()
  first := size(value)
  second := size(value)
  if first == 9 && second == 9 && value.len() == 9 { return 42 }
  return 0
}
";
    let output = build_and_run("borrow-shared-direct", source);
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn shared_function_value_preserves_mode_and_owner() {
    if !backend_available() {
        return;
    }
    let source = "\
fn size(borrow value: string) -> i64 = value.len()
fn apply(f: fn(borrow string) -> i64, borrow value: string) -> i64 = f(value)
fn main() -> i32 {
  value := \"align\".clone()
  f := size
  if apply(f, value) == 5 && value.len() == 5 { return 42 }
  return 0
}
";
    let output = build_and_run("borrow-shared-indirect", source);
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn shared_rejects_copy_temporary_move_and_aliasing() {
    assert!(check_errs(
        "borrow-shared-copy",
        "fn inspect(borrow value: i64) -> i64 = value\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "borrow-shared-temp",
        "fn inspect(borrow value: string) -> i64 = value.len()\nfn main() -> i32 = inspect(\"x\".clone()) as i32\n",
    ));
    assert!(check_errs(
        "borrow-shared-move",
        "fn take(borrow value: string) -> string = value\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "borrow-shared-alias",
        "fn clash(borrow left: string, right: string) -> i64 = left.len() + right.len()\nfn main() -> i32 { value := \"x\".clone(); return clash(value, value) as i32 }\n",
    ));
}

#[test]
fn shared_returned_view_tracks_exact_owner_generation() {
    let valid = "\
fn view(borrow value: string) -> slice<u8> = value.bytes()
fn main() -> i32 {
  value := \"align\".clone()
  result := view(value)
  if result.len() == 5 && value.len() == 5 { return 42 }
  return 0
}
";
    if backend_available() {
        assert_eq!(build_and_run("borrow-shared-view", valid).status.code(), Some(42));
    }
    let stale = "\
fn view(borrow value: string) -> slice<u8> = value.bytes()
fn main() -> i32 {
  value := \"align\".clone()
  result := view(value)
  moved := value
  print(result)
  print(moved)
  return 0
}
";
    assert!(check_errs("borrow-shared-view-stale", stale));
}

#[test]
fn shared_imported_call_matches_whole_program() {
    let files = &[
        (
            "views.align",
            "module views\npub fn size(borrow value: string) -> i64 = value.len()\npub fn view(borrow value: string) -> slice<u8> = value.bytes()\n",
        ),
        (
            "main.align",
            "import views\nfn main() -> i32 { value := \"align\".clone(); bytes := views.view(value); if views.size(value) == 5 && bytes.len() == 5 && value.len() == 5 { return 42 }; return 0 }\n",
        ),
    ];
    let differential = diff_check_multi("borrow-shared-import", files, "main.align");
    assert_eq!(differential.whole_errors, differential.per_unit_errors);
    assert!(!differential.whole_errors, "whole: {}\nper-unit: {}", differential.whole_diags, differential.per_unit_diags);
    if backend_available() {
        assert_eq!(
            build_and_run_multi("borrow-shared-import-whole", files, "main.align")
                .status
                .code(),
            Some(42),
        );
        assert_eq!(
            build_per_unit_multi("borrow-shared-import-per-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42),
        );
    }
}

#[test]
fn exclusive_copy_and_field_updates_are_visible() {
    if !backend_available() {
        return;
    }
    let source = "\
Counter { value: i64 }
fn increment(borrow mut value: i64) { value = value + 1 }
fn main() -> i32 {
  mut scalar := 40
  mut counter := Counter { value: 1 }
  increment(scalar)
  increment(counter.value)
  if scalar == 41 && counter.value == 2 { return 42 }
  return 0
}
";
    assert_eq!(build_and_run("borrow-exclusive-copy", source).status.code(), Some(42));
}

#[test]
fn exclusive_move_replacement_updates_caller_cleanup() {
    if !backend_available() {
        return;
    }
    let source = "\
fn replace(borrow mut value: string) { value = \"replacement\".clone() }
fn leave(borrow mut value: string) { print(value.len()) }
fn main() -> i32 {
  mut value := \"old\".clone()
  leave(value)
  replace(value)
  if value.len() == 11 { return 42 }
  return 0
}
";
    assert_eq!(build_and_run("borrow-exclusive-move", source).status.code(), Some(42));
}

#[test]
fn exclusive_rejects_immutable_temporary_partial_move_and_stale_view() {
    assert!(check_errs(
        "borrow-exclusive-immutable",
        "fn inc(borrow mut value: i64) { value = value + 1 }\nfn main() -> i32 { value := 1; inc(value); return 0 }\n",
    ));
    assert!(check_errs(
        "borrow-exclusive-temp",
        "fn inc(borrow mut value: i64) { value = value + 1 }\nfn main() -> i32 { inc(1 + 2); return 0 }\n",
    ));
    assert!(check_errs(
        "borrow-exclusive-partial-move",
        "Holder { value: string }\nfn replace(borrow mut value: string) { value = \"new\".clone() }\nfn main() -> i32 { mut holder := Holder { value: \"old\".clone() }; replace(holder.value); return 0 }\n",
    ));
    assert!(check_errs(
        "borrow-exclusive-stale",
        "fn replace(borrow mut value: string) { value = \"new\".clone() }\nfn main() -> i32 { mut value := \"old\".clone(); view := value.as_str(); replace(value); print(view); return 0 }\n",
    ));
}

#[test]
fn exclusive_all_peer_aliases_are_rejected() {
    let cases = [
        "fn f(borrow mut a: string, b: string) {}",
        "fn f(borrow mut a: string, borrow b: string) {}",
        "fn f(borrow mut a: string, borrow mut b: string) {}",
    ];
    for (index, declaration) in cases.iter().enumerate() {
        let source = format!(
            "{declaration}\nfn main() -> i32 {{ mut value := \"x\".clone(); f(value, value); return 0 }}\n"
        );
        assert!(check_errs(&format!("borrow-exclusive-alias-{index}"), &source));
    }

    assert!(check_errs(
        "borrow-exclusive-alias-copy-view",
        "fn f(borrow mut owner: string, peer: str) {}\nfn main() -> i32 { mut owner := \"x\".clone(); peer := owner.as_str(); f(owner, peer); return 0 }\n",
    ));
    assert!(check_errs(
        "borrow-exclusive-alias-copy-aggregate",
        "View { text: str }\nfn f(borrow mut owner: string, peer: View) {}\nfn main() -> i32 { mut owner := \"x\".clone(); peer := View { text: owner.as_str() }; f(owner, peer); return 0 }\n",
    ));
}

#[test]
fn exclusive_function_value_and_imported_call_preserve_mode() {
    let files = &[
        (
            "ops.align",
            "module ops\npub fn increment(borrow mut value: i64) { value = value + 1 }\npub fn replace(borrow mut value: string) { value = \"replacement\".clone() }\n",
        ),
        (
            "main.align",
            "import ops\nfn apply(f: fn(borrow mut i64) -> (), borrow mut value: i64) { f(value) }\nfn main() -> i32 { mut count := 40; mut value := \"old\".clone(); f := ops.increment; apply(f, count); ops.increment(count); ops.replace(value); if count == 42 && value.len() == 11 { return 42 }; return 0 }\n",
        ),
    ];
    let differential = diff_check_multi("borrow-exclusive-import", files, "main.align");
    assert_eq!(differential.whole_errors, differential.per_unit_errors);
    assert!(!differential.whole_errors, "whole: {}\nper-unit: {}", differential.whole_diags, differential.per_unit_diags);
    if backend_available() {
        assert_eq!(
            build_and_run_multi("borrow-exclusive-import-whole", files, "main.align")
                .status
                .code(),
            Some(42),
        );
        assert_eq!(
            build_per_unit_multi("borrow-exclusive-import-per-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42),
        );
    }
}
