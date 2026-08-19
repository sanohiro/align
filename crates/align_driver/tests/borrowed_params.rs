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
fn borrowed_sum_match_projects_move_payloads_and_preserves_the_root() {
    let source = "\
Holder { choice: Option<string> }\n\
Inner { choice: Option<string> }\n\
Outer { inner: Inner }\n\
Content { Text(string), Empty }\n\
TextValue { text: string }\n\
NestedContent { Item(TextValue), Empty }\n\
fn option_len(borrow value: Option<string>) -> i64 {\n\
  first := match value {\n\
    Some(text) => text.len()\n\
    None => 0\n\
  }\n\
  second := match value {\n\
    Some(text) => text.len()\n\
    None => 0\n\
  }\n\
  return first + second\n\
}\n\
fn result_len(borrow value: Result<string, string>) -> i64 = match value {\n\
  Ok(text) => text.len()\n\
  Err(text) => text.len()\n\
}\n\
fn holder_len(borrow value: Holder) -> i64 = match value.choice {\n\
  Some(text) => text.len()\n\
  None => 0\n\
}\n\
fn nested_holder_len(borrow value: Outer) -> i64 = match value.inner.choice {\n\
  Some(text) => text.len()\n\
  None => 0\n\
}\n\
fn exclusive_option_len(borrow mut value: Option<string>) -> i64 = match value {\n\
  Some(text) => text.len()\n\
  None => 0\n\
}\n\
fn content_len(borrow value: Content) -> i64 = match value {\n\
  Text(text) => text.len()\n\
  Empty => 0\n\
}\n\
fn nested_content_len(borrow value: NestedContent) -> i64 = match value {\n\
  Item(item) => item.text.len()\n\
  Empty => 0\n\
}\n\
fn option_struct_len(borrow value: Option<TextValue>) -> i64 = match value {\n\
  Some(item) => item.text.len()\n\
  None => 0\n\
}\n\
fn cloned_len(borrow value: Option<string>) -> i64 = match value {\n\
  Some(text) => text.clone().len()\n\
  None => 0\n\
}\n\
fn main() -> i32 {\n\
  option: Option<string> := Some(\"hello\".clone())\n\
  ok: Result<string, string> := Ok(\"ok\".clone())\n\
  err: Result<string, string> := Err(\"bad\".clone())\n\
  holder := Holder { choice: Some(\"field\".clone()) }\n\
  nested := Outer { inner: Inner { choice: Some(\"nested\".clone()) } }\n\
  mut exclusive: Option<string> := Some(\"exclusive\".clone())\n\
  content := Content.Text(\"payload\".clone())\n\
  nested_content := NestedContent.Item(TextValue { text: \"deep\".clone() })\n\
  option_struct: Option<TextValue> := Some(TextValue { text: \"option\".clone() })\n\
  if option_len(option) != 10 { return 1 }\n\
  if option_len(option) != 10 { return 2 }\n\
  if result_len(ok) != 2 { return 3 }\n\
  if result_len(err) != 3 { return 4 }\n\
  if holder_len(holder) != 5 { return 5 }\n\
  if holder_len(holder) != 5 { return 6 }\n\
  if nested_holder_len(nested) != 6 { return 7 }\n\
  if nested_holder_len(nested) != 6 { return 8 }\n\
  if exclusive_option_len(exclusive) != 9 { return 9 }\n\
  if option_len(exclusive) != 18 { return 10 }\n\
  if content_len(content) != 7 { return 11 }\n\
  if content_len(content) != 7 { return 12 }\n\
  if nested_content_len(nested_content) != 4 { return 13 }\n\
  if nested_content_len(nested_content) != 4 { return 14 }\n\
  if option_struct_len(option_struct) != 6 { return 15 }\n\
  if option_struct_len(option_struct) != 6 { return 16 }\n\
  if cloned_len(option) != 5 { return 17 }\n\
  return 42\n\
}\n\
";
    assert!(
        !check_errs("borrowed-sum-match", source),
        "{}",
        check_diagnostics("borrowed-sum-match", source)
    );
    if backend_available() {
        assert_eq!(build_and_run("borrowed-sum-match", source).status.code(), Some(42));
    }
}

#[test]
fn borrowed_sum_match_imported_body_matches_whole_program() {
    let files = &[
        (
            "views.align",
            "module views\npub fn inspect(borrow value: Option<string>) -> i64 = match value { Some(text) => text.len() None => 0 }\n",
        ),
        (
            "main.align",
            "import views\nfn main() -> i32 { value: Option<string> := Some(\"cross-unit\".clone()); if views.inspect(value) == 10 { return 42 }; return 0 }\n",
        ),
    ];
    let differential = diff_check_multi("borrowed-sum-import", files, "main.align");
    assert_eq!(differential.whole_errors, differential.per_unit_errors);
    assert!(!differential.whole_errors, "whole: {}\nper-unit: {}", differential.whole_diags, differential.per_unit_diags);
    if backend_available() {
        assert_eq!(
            build_and_run_multi("borrowed-sum-import-whole", files, "main.align")
                .status
                .code(),
            Some(42),
        );
        assert_eq!(
            build_per_unit_multi("borrowed-sum-import-per-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42),
        );
    }
}

#[test]
fn borrowed_sum_match_uses_pointer_projections_without_move_cleanup() {
    let source = "\
Content { Text(string), Empty }\n\
fn inspect(borrow value: Content) -> i64 = match value {\n\
  Text(text) => text.len()\n\
  Empty => 0\n\
}\n\
fn main() -> i32 {\n\
  value := Content.Empty\n\
  return inspect(value) as i32\n\
}\n\
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "borrowed-sum-mir", source);
    assert!(!checked.diags.has_errors(), "unexpected diagnostics");
    let mir = lower_to_mir(&checked.hir);
    let rendered = align_mir::print::program_to_string(&mir);
    let inspect = rendered
        .split("fn inspect")
        .nth(1)
        .and_then(|body| body.split("fn main").next())
        .expect("borrowed inspect MIR");
    assert!(inspect.contains("borrow slot"), "projection must retain a borrowed place:\n{rendered}");
    assert!(
        inspect.contains("variant0.payload0"),
        "user-sum payload must be addressed by its typed path:\n{rendered}"
    );
    assert!(!inspect.contains("enum_payload("), "borrowed projection must not extract an aggregate:\n{rendered}");
    assert!(!inspect.contains("null "), "borrowed projection must not null the source:\n{rendered}");
    assert!(!inspect.contains("drop "), "projection locals/root must not gain cleanup:\n{rendered}");
}

#[test]
fn borrowed_sum_match_rejects_unsupported_payloads_and_consumption() {
    assert!(check_errs(
        "borrowed-sum-array",
        "fn inspect(borrow value: Option<array<i64>>) -> i64 = match value { Some(values) => values.len() None => 0 }\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "borrowed-sum-nested-match",
        "fn inspect(borrow value: Option<Option<string>>) -> i64 = match value { Some(inner) => match inner { Some(text) => text.len() None => 0 } None => 0 }\nfn main() -> i32 = 0\n",
    ));
    let consuming = "fn take(value: string) -> i64 = value.len()\nfn inspect(borrow value: Option<string>) -> i64 = match value { Some(text) => take(text) None => 0 }\nfn main() -> i32 = 0\n";
    let diagnostics = check_diagnostics("borrowed-sum-consume", consuming);
    assert!(
        diagnostics.contains("borrowed match payload"),
        "whole payload consumption must remain rejected:\n{diagnostics}"
    );
    assert!(check_errs(
        "borrowed-sum-return",
        "fn inspect(borrow value: Option<string>) -> string = match value { Some(text) => text None => \"empty\".clone() }\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "borrowed-sum-store",
        "Holder { value: string }\nfn inspect(borrow value: Option<string>) -> Holder = match value { Some(text) => Holder { value: text } None => Holder { value: \"empty\".clone() } }\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "borrowed-sum-mixed-nested",
        "Holder { choice: Mixed }\nMixed { Text(string), Bad(array<i64>) }\nfn inspect(borrow value: Holder) -> i64 = match value.choice { Text(text) => text.len() Bad(values) => values.len() }\nfn main() -> i32 = 0\n",
    ));
}

#[test]
fn borrowed_sum_match_reads_copy_scalars_through_nested_move_payloads() {
    let source = "\
Record { text: string, count: i64 }\n\
Wrapped { Item(Record), Empty }\n\
fn inspect(borrow value: Wrapped) -> i64 = match value {\n\
  Item(item) => item.count + item.text.len()\n\
  Empty => 0\n\
}\n\
fn main() -> i32 {\n\
  value := Wrapped.Item(Record { text: \"hello\".clone(), count: 37 })\n\
  if inspect(value) == 42 && inspect(value) == 42 { return 42 }\n\
  return 0\n\
}\n\
";
    assert!(
        !check_errs("borrowed-sum-copy-scalar", source),
        "{}",
        check_diagnostics("borrowed-sum-copy-scalar", source)
    );
    if backend_available() {
        assert_eq!(
            build_and_run("borrowed-sum-copy-scalar", source)
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn borrowed_sum_match_borrow_mut_keeps_the_sum_unchanged() {
    let source = "\
Holder { choice: Option<string> }\n\
Inner { choice: Option<string> }\n\
Outer { inner: Inner }\n\
Content { Text(string), Empty }\n\
fn option_len(borrow value: Option<string>) -> i64 {\n\
  first := match value {\n\
    Some(text) => text.len()\n\
    None => 0\n\
  }\n\
  second := match value {\n\
    Some(text) => text.len()\n\
    None => 0\n\
  }\n\
  return first + second\n\
}\n\
fn exclusive_option_len(borrow mut value: Option<string>) -> i64 = match value {\n\
  Some(text) => text.len()\n\
  None => 0\n\
}\n\
fn main() -> i32 {\n\
  mut value: Option<string> := Some(\"stable\".clone())\n\
  if exclusive_option_len(value) != 6 { return 1 }\n\
  if option_len(value) != 12 { return 2 }\n\
  return 42\n\
  return 0\n\
}\n\
";
    assert_eq!(build_and_run("borrowed-sum-borrow-mut", source).status.code(), Some(42));
}

#[test]
fn borrowed_sum_match_can_return_a_view_of_a_string_leaf() {
    let source = "\
fn identity(value: str) -> str = value\n\
fn view(borrow value: Option<string>) -> str = match value {\n\
  Some(text) => identity(text)\n\
  None => \"\"\n\
}\n\
fn main() -> i32 {\n\
  value: Option<string> := Some(\"hello\".clone())\n\
  result := view(value)\n\
  if result.len() == 5 && match value { Some(text) => text.len() None => 0 } == 5 { return 42 }\n\
  return 0\n\
}\n\
";
    assert!(
        !check_errs("borrowed-sum-view", source),
        "{}",
        check_diagnostics("borrowed-sum-view", source)
    );
    if backend_available() {
        assert_eq!(build_and_run("borrowed-sum-view", source).status.code(), Some(42));
    }
}

#[test]
fn borrowed_sum_match_passes_a_move_projection_through_shared_calls() {
    let source = "\
fn length(borrow value: string) -> i64 = value.len()\n\
fn direct(borrow value: Option<string>) -> i64 = match value {\n\
  Some(text) => length(text)\n\
  None => 0\n\
}\n\
fn apply(f: fn(borrow string) -> i64, borrow value: Option<string>) -> i64 = match value {\n\
  Some(text) => f(text)\n\
  None => 0\n\
}\n\
fn main() -> i32 {\n\
  value: Option<string> := Some(\"hello\".clone())\n\
  f := length\n\
  result := apply(f, value)\n\
  if direct(value) == 5 && result == 5 && match value { Some(text) => text.len() None => 0 } == 5 { return 42 }\n\
  return 0\n\
}\n\
";
    assert!(
        !check_errs("borrowed-sum-shared-calls", source),
        "{}",
        check_diagnostics("borrowed-sum-shared-calls", source)
    );
    if backend_available() {
        assert_eq!(build_and_run("borrowed-sum-shared-calls", source).status.code(), Some(42));
    }
}

#[test]
fn borrowed_sum_match_passes_copy_projection_to_runtime_calls() {
    let source = "\
fn inspect(borrow value: Option<string>) -> i64 = match value {\n\
  Some(text) => { print(text); 1 }\n\
  None => 0\n\
}\n\
fn main() -> i32 {\n\
  value: Option<string> := Some(\"hello\".clone())\n\
  if inspect(value) == 1 && inspect(value) == 1 { return 42 }\n\
  return 0\n\
}\n\
";
    assert!(
        !check_errs("borrowed-sum-runtime-call", source),
        "{}",
        check_diagnostics("borrowed-sum-runtime-call", source)
    );
    if backend_available() {
        assert_eq!(build_and_run("borrowed-sum-runtime-call", source).status.code(), Some(42));
    }
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
fn shared_copy_uses_stable_caller_storage_without_a_copy() {
    let source = "\
Payload { a: i64, b: i64, c: i64, d: i64 }
fn scalar(borrow value: i64) -> i64 = value
fn sum(borrow value: Payload) -> i64 = value.a + value.b + value.c + value.d
fn apply(f: fn(borrow i64) -> i64, borrow value: i64) -> i64 = f(value)
fn main() -> i32 {
  scalar_value := 2
  payload := Payload { a: 10, b: 10, c: 10, d: 10 }
  f := scalar
  if apply(f, scalar_value) + sum(payload) == 42 && scalar_value == 2 && payload.a == 10 { return 42 }
  return 0
}
";
    if backend_available() {
        assert_eq!(
            build_and_run("borrow-shared-copy-place", source)
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn shared_copy_abi_is_one_nonnull_readonly_pointer() {
    if !backend_available() {
        return;
    }
    let source = "\
Payload { a: i64, b: i64, c: i64, d: i64 }
fn inspect(borrow value: Payload) -> i64 = value.a + value.b + value.c + value.d
fn main() -> i32 = 0
";
    let llvm = emit_llvm_with_exports(source, &["inspect"]);
    let signature = llvm
        .lines()
        .find(|line| line.starts_with("define ") && line.contains("@inspect("))
        .expect("exported inspect definition");
    assert!(
        signature.contains("ptr"),
        "Copy borrow must use a pointer ABI: {signature}"
    );
    assert!(
        signature.contains("nonnull"),
        "Copy borrow must remain non-null: {signature}"
    );
    assert!(
        signature.contains("readonly"),
        "shared Copy borrow must be readonly: {signature}"
    );
    assert!(
        signature.contains("captures(none)"),
        "a non-returned Copy borrow must not be captured: {signature}"
    );
}

#[test]
fn shared_rejects_temporary_move_and_aliasing() {
    assert!(check_errs(
        "borrow-shared-copy-temp",
        "fn inspect(borrow value: i64) -> i64 = value\nfn main() -> i32 = inspect(40 + 2) as i32\n",
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

    let copy_aggregate = "\
View { text: str }
fn project(borrow value: View) -> str = value.text
fn main() -> i32 {
  owner := \"align\".clone()
  text := owner.bytes().as_str() else { return 1 }
  holder := View { text: text }
  result := project(holder)
  if result.len() == 5 && owner.len() == 5 { return 42 }
  return 0
}
";
    if backend_available() {
        assert_eq!(
            build_and_run("borrow-shared-copy-view", copy_aggregate)
                .status
                .code(),
            Some(42)
        );
    }
    let stale_copy_aggregate = "\
View { text: str }
fn project(borrow value: View) -> str = value.text
fn main() -> i32 {
  owner := \"align\".clone()
  text := owner.bytes().as_str() else { return 1 }
  holder := View { text: text }
  result := project(holder)
  moved := owner
  print(result)
  print(moved)
  return 0
}
";
    assert!(check_errs(
        "borrow-shared-copy-view-stale",
        stale_copy_aggregate
    ));
}

#[test]
fn shared_imported_call_matches_whole_program() {
    let files = &[
        (
            "views.align",
            "module views\npub fn size(borrow value: string) -> i64 = value.len()\npub fn view(borrow value: string) -> slice<u8> = value.bytes()\npub fn scalar(borrow value: i64) -> i64 = value\n",
        ),
        (
            "main.align",
            "import views\nfn main() -> i32 { value := \"align\".clone(); count := 42; bytes := views.view(value); if views.size(value) == 5 && bytes.len() == 5 && value.len() == 5 && views.scalar(count) == 42 && count == 42 { return 42 }; return 0 }\n",
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
