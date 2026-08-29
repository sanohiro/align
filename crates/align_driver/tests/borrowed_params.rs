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
    assert_eq!(
        differential.whole_errors,
        differential.per_unit_errors,
        "whole: {}\nper-unit: {}",
        differential.whole_diags,
        differential.per_unit_diags
    );
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
fn borrowed_sum_match_accepts_arrays_but_rejects_nested_projection_and_consumption() {
    assert!(!check_errs(
        "borrowed-sum-array",
        "fn inspect(borrow value: Option<array<i64>>) -> i64 = match value { Some(values) => values.len() None => 0 }\nfn main() -> i32 = 0\n",
    ));
    let aggregate_graphs = "\
Record { value: string }\n\
Holder { values: array<Record> }\n\
Choice { Numbers(array<i64>), Wrapped(Holder), Empty }\n\
fn inspect(borrow value: Record) -> i64 = value.value.len()\n\
fn option_strings(borrow value: Option<array<string>>) -> i64 = match value { Some(values) => values.len() None => 0 }\n\
fn option_records(borrow value: Option<array<Record>>) -> i64 = match value { Some(values) => inspect(values[0]) None => 0 }\n\
fn result_records(borrow value: Result<array<Record>, Error>) -> i64 = match value { Ok(values) => inspect(values[0]) Err(_) => 0 }\n\
fn choice_records(borrow value: Choice) -> i64 = match value { Numbers(values) => values.len() Wrapped(holder) => inspect(holder.values[0]) Empty => 0 }\n\
fn main() -> i32 = 0\n\
";
    assert!(
        !check_errs("borrowed-sum-aggregate-graphs", aggregate_graphs),
        "{}",
        check_diagnostics("borrowed-sum-aggregate-graphs", aggregate_graphs)
    );
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
    assert!(!check_errs(
        "borrowed-sum-mixed-nested",
        "Holder { choice: Mixed }\nMixed { Text(string), Bad(array<i64>) }\nfn inspect(borrow value: Holder) -> i64 = match value.choice { Text(text) => text.len() Bad(values) => values.len() }\nfn main() -> i32 = 0\n",
    ));
}

#[test]
fn borrowed_sum_array_views_build_and_run_whole_and_per_unit() {
    let body = "\
Command { argv: array<str> }\n\
Task { build: Command, targeted: Option<Command>, full: Command }\n\
Row { value: i64 }\n\
Rows { values: array<Row> }\n\
fn argc(argv: slice<str>) -> i64 = argv.len()\n\
fn row_total(rows: slice<Row>) -> i64 = rows.len()\n\
fn inspect(borrow task: Task) -> i64 {\n\
  before := argc(task.build.argv) + argc(task.full.argv)\n\
  targeted := match task.targeted {\n\
    Some(command) => argc(command.argv) + argc(command.argv)\n\
    None => 0\n\
  }\n\
  return before + targeted + argc(task.build.argv) + argc(task.full.argv)\n\
}\n\
fn inspect_rows(borrow rows: Option<Rows>) -> i64 = match rows {\n\
  Some(value) => row_total(value.values) + row_total(value.values)\n\
  None => 0\n\
}\n\
pub fn run() -> i64 {\n\
  arena {\n\
    present := Task {\n\
      build: Command { argv: [\"build\"].to_array() },\n\
      targeted: Some(Command { argv: [\"target\", \"fast\"].to_array() }),\n\
      full: Command { argv: [\"full\", \"suite\", \"all\"].to_array() },\n\
    }\n\
    absent := Task {\n\
      build: Command { argv: [\"build\"].to_array() },\n\
      targeted: None,\n\
      full: Command { argv: [\"full\", \"suite\", \"all\"].to_array() },\n\
    }\n\
    rows: Option<Rows> := Some(Rows { values: [Row { value: 5 }, Row { value: 6 }].to_array() })\n\
    return inspect(present) + inspect(present) + inspect(absent) + inspect_rows(rows)\n\
  }\n\
}\n";
    let source = format!("{body}fn main() -> i32 {{ if run() == 36 {{ return 42 }}; return 0 }}\n");
    assert!(
        !check_errs("borrowed-sum-array-view-retype", &source),
        "{}",
        check_diagnostics("borrowed-sum-array-view-retype", &source)
    );
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "borrowed-sum-array-view-retype-mir", &source);
    let mir = align_mir::lower_program_checked(&checked.hir, false, None);
    assert!(
        mir.is_ok(),
        "borrowed projection view retypes must validate: {mir:?}"
    );
    let rendered = if let Ok(mir) = mir {
        align_mir::print::program_to_string(&mir)
    } else {
        String::new()
    };
    assert!(
        rendered.contains("call program argc(borrow slot")
            && rendered.contains("call program row_total(borrow slot"),
        "array-to-slice views must retain borrowed field paths:\n{rendered}"
    );
    if backend_available() {
        assert_eq!(
            build_and_run("borrowed-sum-array-view-retype", &source)
                .status
                .code(),
            Some(42)
        );
    }

    let module = format!("module views\n{body}");
    let files = &[
        ("views.align", module.as_str()),
        (
            "main.align",
            "import views\nfn main() -> i32 { if views.run() == 36 { return 42 }; return 0 }\n",
        ),
    ];
    let differential = diff_check_multi(
        "borrowed-sum-array-view-retype-per-unit",
        files,
        "main.align",
    );
    assert_eq!(
        differential.whole_errors,
        differential.per_unit_errors,
        "whole: {}\nper-unit: {}",
        differential.whole_diags,
        differential.per_unit_diags
    );
    assert!(
        !differential.whole_errors,
        "whole: {}\nper-unit: {}",
        differential.whole_diags,
        differential.per_unit_diags
    );
    if backend_available() {
        assert_eq!(
            build_per_unit_multi(
                "borrowed-sum-array-view-retype-per-unit",
                files,
                "main.align",
            )
            .link_and_run()
            .status
            .code(),
            Some(42)
        );
    }
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
fn indexed_shared_borrow_addresses_dynamic_record_elements_at_the_call_action() {
    let source = "\
Record { text: string, value: i64 }\n\
Inspector { call: fn(borrow Record) -> i64 }\n\
RecordHolder { values: array<Record> }\n\
fn inspect(borrow record: Record) -> i64 = record.text.len() + record.value\n\
fn inspect_text(borrow text: string) -> i64 = text.len()\n\
fn generic<T>(borrow value: T) -> i64 = 7\n\
fn records() -> array<Record> {\n\
  mut values: array_builder<Record> := array_builder()\n\
  values.push(Record { text: \"first\".clone(), value: 1 })\n\
  values.push(Record { text: \"second\".clone(), value: 36 })\n\
  return values.build()\n\
}\n\
fn strings() -> array<string> {\n\
  mut values: array_builder<string> := array_builder()\n\
  values.push(\"scalar\".clone())\n\
  return values.build()\n\
}\n\
fn field_base(borrow holder: RecordHolder) -> i64 = inspect(holder.values[1])\n\
fn projected(borrow values: Option<array<Record>>) -> i64 = match values {\n\
  Some(records) => inspect(records[1])\n\
  None => 0\n\
}\n\
fn main() -> i32 {\n\
  values := records()\n\
  f := inspect\n\
  holder := Inspector { call: inspect }\n\
  joined := if values.len() == 2 { inspect } else { f }\n\
  direct := inspect(values[0])\n\
  indirect := f(values[1])\n\
  joined_result := joined(values[0])\n\
  field_result := holder.call(values[0])\n\
  generic_result := generic(values[0])\n\
  aggregate_holder := RecordHolder { values: records() }\n\
  nested_base_result := field_base(aggregate_holder)\n\
  text_values := strings()\n\
  scalar_string_result := inspect_text(text_values[0])\n\
  wrapped: Option<array<Record>> := Some(records())\n\
  if direct == 6 && indirect == 42 && joined_result == 6 && field_result == 6 && generic_result == 7 && nested_base_result == 42 && scalar_string_result == 6 && projected(wrapped) == 42 && values.len() == 2 { return 42 }\n\
  return 0\n\
}\n\
";
    assert!(
        !check_errs("borrowed-indexed-record", source),
        "{}",
        check_diagnostics("borrowed-indexed-record", source)
    );
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "borrowed-indexed-record-mir", source);
    assert!(align_sema::checked_hir_body_facts_are_valid(&checked.hir));
    let mir = align_mir::lower_program_checked(&checked.hir, false, None)
        .expect("indexed borrowed HIR must validate");
    let rendered = align_mir::print::program_to_string(&mir);
    assert!(
        rendered.contains("borrow-element"),
        "indexed borrow must remain a MIR place descriptor:\n{rendered}"
    );
    if backend_available() {
        assert_eq!(
            build_and_run("borrowed-indexed-record", source)
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn indexed_shared_borrow_reserves_the_array_across_index_and_later_arguments() {
    let prefix = "\
Record { value: string }\n\
fn inspect(borrow record: Record) -> i64 = record.value.len()\n\
fn inspect_with(borrow record: Record, value: i64) -> i64 = record.value.len() + value\n\
fn inspect_many(borrow record: Record, first: i64, middle: i64, last: i64) -> i64 = record.value.len() + first + middle + last\n\
fn consume(records: array<Record>) -> i64 = records.len()\n\
fn replace(borrow mut values: array<Record>) -> i64 { values = records(\"replacement\"); return 0 }\n\
fn observe_mut(borrow mut values: array<Record>) -> i64 = values.len()\n\
fn records(value: str) -> array<Record> {\n\
  mut builder: array_builder<Record> := array_builder()\n\
  builder.push(Record { value: value.clone() })\n\
  return builder.build()\n\
}\n\
";
    let during_index = format!(
        "{prefix}fn main() -> i32 {{ values := records(\"x\"); return inspect(values[consume(values)]) as i32 }}\n"
    );
    let diagnostics = check_diagnostics("borrowed-index-self-invalidation", &during_index);
    assert!(
        diagnostics.contains("value snapshot was invalidated"),
        "the pre-index reservation must reject self-invalidation:\n{diagnostics}"
    );

    let during_later_argument = format!(
        "{prefix}fn main() -> i32 {{ values := records(\"x\"); return inspect_with(values[0], consume(values)) as i32 }}\n"
    );
    let diagnostics = check_diagnostics(
        "borrowed-index-later-invalidation",
        &during_later_argument,
    );
    assert!(
        diagnostics.contains("value snapshot was invalidated"),
        "the guarded descriptor must retain its reservation through later arguments:\n{diagnostics}"
    );

    for (operation, expression) in [
        ("move", "consume(values)"),
        ("replace", "replace(values)"),
        ("borrow-mut", "observe_mut(values)"),
    ] {
        let during_index = format!(
            "{prefix}fn main() -> i32 {{ mut values := records(\"x\"); return inspect(values[{expression}]) as i32 }}\n"
        );
        let diagnostics = check_diagnostics(
            &format!("borrowed-index-{operation}-during-index"),
            &during_index,
        );
        assert!(
            diagnostics.contains("value snapshot was invalidated"),
            "{operation} during index evaluation must invalidate the reservation:\n{diagnostics}"
        );

        for (position, arguments) in [
            ("first", format!("{expression}, 0, 0")),
            ("middle", format!("0, {expression}, 0")),
            ("last", format!("0, 0, {expression}")),
        ] {
            let during_later = format!(
                "{prefix}fn main() -> i32 {{ mut values := records(\"x\"); return inspect_many(values[0], {arguments}) as i32 }}\n"
            );
            let diagnostics = check_diagnostics(
                &format!("borrowed-index-{operation}-later-{position}"),
                &during_later,
            );
            assert!(
                diagnostics.contains("value snapshot was invalidated"),
                "{operation} in the {position} later argument must invalidate the reservation:\n{diagnostics}"
            );
        }
    }

    let unrelated = format!(
        "{prefix}fn main() -> i32 {{ values := records(\"forty-two\"); other := records(\"other\"); if inspect_with(values[0], consume(other)) == 10 {{ return 42 }}; return 0 }}\n"
    );
    assert!(
        !check_errs("borrowed-index-unrelated", &unrelated),
        "{}",
        check_diagnostics("borrowed-index-unrelated", &unrelated)
    );
    if backend_available() {
        assert_eq!(
            build_and_run("borrowed-index-unrelated", &unrelated)
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn indexed_shared_borrow_substitutes_return_and_mutable_retention_roots() {
    let prefix = "\
Record { value: string }\n\
View { text: str }\n\
fn element_text(borrow record: Record) -> str { text: str := record.value; return text }\n\
fn retain_text(borrow record: Record, borrow mut destination: View) { text: str := record.value; destination.text = text }\n\
fn first_text(borrow records: array<Record>) -> str = element_text(records[0])\n\
fn retain_first(borrow records: array<Record>, borrow mut destination: View) { retain_text(records[0], destination) }\n\
fn retain_first_indirect(borrow records: array<Record>, borrow mut destination: View) { f := retain_text; f(records[0], destination) }\n\
fn records() -> array<Record> {\n\
  mut builder: array_builder<Record> := array_builder()\n\
  builder.push(Record { value: \"forty-two\".clone() })\n\
  return builder.build()\n\
}\n\
";
    let positive = format!(
        "{prefix}fn main() -> i32 {{ values := records(); mut output := View {{ text: \"\" }}; mut indirect_output := View {{ text: \"\" }}; direct := first_text(values); retain_first(values, output); retain_first_indirect(values, indirect_output); if direct.len() == 9 && output.text.len() == 9 && indirect_output.text.len() == 9 && values.len() == 1 {{ return 42 }}; return 0 }}\n"
    );
    assert!(
        !check_errs("borrowed-index-summary-positive", &positive),
        "{}",
        check_diagnostics("borrowed-index-summary-positive", &positive)
    );
    if backend_available() {
        assert_eq!(
            build_and_run("borrowed-index-summary-positive", &positive)
                .status
                .code(),
            Some(42)
        );
    }

    for (name, body) in [
        (
            "return",
            "values := records(); view := first_text(values); consumed := values; if consumed.len() == 1 && view.len() == 9 { return 42 }; return 0",
        ),
        (
            "retention",
            "values := records(); mut output := View { text: \"\" }; retain_first(values, output); consumed := values; if consumed.len() == 1 && output.text.len() == 9 { return 42 }; return 0",
        ),
        (
            "indirect-retention",
            "values := records(); mut output := View { text: \"\" }; retain_first_indirect(values, output); consumed := values; if consumed.len() == 1 && output.text.len() == 9 { return 42 }; return 0",
        ),
    ] {
        let source = format!("{prefix}fn main() -> i32 {{ {body} }}\n");
        let owner = format!("borrowed-index-summary-{name}");
        assert!(
            !check_errs(&owner, &source),
            "indexed {name} roots must follow the stable array generation across owner transfer:\n{}",
            check_diagnostics(&owner, &source)
        );
        if backend_available() {
            assert_eq!(build_and_run(&owner, &source).status.code(), Some(42));
        }
    }
}

#[test]
fn indexed_shared_borrow_rejects_non_shared_or_unstable_element_places() {
    let prefix = "\
Record { value: string }\n\
fn inspect(borrow record: Record) -> i64 = record.value.len()\n\
fn consume(record: Record) -> i64 = record.value.len()\n\
fn mutate(borrow mut record: Record) { record.value = \"changed\".clone() }\n\
fn records() -> array<Record> { mut builder: array_builder<Record> := array_builder(); builder.push(Record { value: \"x\".clone() }); return builder.build() }\n\
";
    for (name, body) in [
        ("by-value", "values := records(); print(consume(values[0]))"),
        ("borrow-mut", "mut values := records(); mutate(values[0])"),
        ("temporary", "print(inspect(records()[0]))"),
    ] {
        let source = format!("{prefix}fn main() -> i32 {{ {body}; return 0 }}\n");
        assert!(
            check_errs(&format!("borrowed-index-reject-{name}"), &source),
            "indexed Move element {name} form must stay rejected"
        );
    }

    let unstable_bad_index =
        format!("{prefix}fn main() -> i32 {{ print(inspect(records()[true])); return 0 }}\n");
    let diagnostics = check_diagnostics(
        "borrowed-index-unstable-precedes-index",
        &unstable_bad_index,
    );
    assert!(
        diagnostics.contains("stable named local"),
        "the unstable base must be rejected first:\n{diagnostics}"
    );
    assert!(
        !diagnostics.contains("array index must be an integer"),
        "index checking must not run after base formation fails:\n{diagnostics}"
    );
}

#[test]
fn indexed_shared_borrow_preserves_termination_and_bounds_order() {
    let prefix = "\
Record { value: string }\n\
fn inspect_with(borrow record: Record, value: i64) -> i64 = record.value.len() + value\n\
fn fail_index() -> Result<i64, Error> = Err(Error.Invalid)\n\
fn later() -> i64 { print(99); return 0 }\n\
fn records() -> array<Record> { mut builder: array_builder<Record> := array_builder(); builder.push(Record { value: \"x\".clone() }); return builder.build() }\n\
";
    let cases = [
        (
            "index-terminates",
            "fn run() -> Result<i64, Error> { values := records(); return Ok(inspect_with(values[fail_index()?], later())) }\nfn main() -> Result<(), Error> { print(run()?); return Ok(()) }\n",
        ),
        (
            "later-terminates",
            "fn run() -> Result<i64, Error> { values := records(); return Ok(inspect_with(values[0], fail_index()?)) }\nfn main() -> Result<(), Error> { print(run()?); return Ok(()) }\n",
        ),
        (
            "bounds-before-later",
            "fn main() -> i32 { values := records(); print(inspect_with(values[2], later())); return 0 }\n",
        ),
    ];
    for (name, body) in cases {
        let source = format!("{prefix}{body}");
        assert!(
            !check_errs(&format!("borrowed-index-{name}"), &source),
            "{}",
            check_diagnostics(&format!("borrowed-index-{name}"), &source)
        );
        if backend_available() {
            let output = build_and_run(&format!("borrowed-index-{name}"), &source);
            assert_ne!(output.status.code(), Some(0), "{name} must terminate before success");
            assert!(
                output.stdout.is_empty(),
                "{name} must not evaluate or call the later printing action: {:?}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
}

#[test]
fn index_preserves_i64_context_for_generic_result_inference() {
    let source = "\
Record { value: string }\n\
fn make_index<T>(seed: i64) -> T { loop {} }\n\
fn inspect(borrow record: Record) -> i64 = record.value.len()\n\
fn ordinary(values: array<i64>) -> i64 = values[make_index(0)]\n\
fn borrowed(values: array<Record>) -> i64 = inspect(values[make_index(0)])\n\
fn main() -> i32 = 0\n\
";
    assert!(
        !check_errs("borrowed-index-generic-result-context", source),
        "{}",
        check_diagnostics("borrowed-index-generic-result-context", source)
    );
}

#[test]
fn indexed_shared_borrow_termination_family_forms_no_element_descriptor() {
    let prefix = "\
import std.process\n\
Record { value: string }\n\
fn inspect_with(borrow record: Record, value: i64) -> i64 = record.value.len() + value\n\
fn later() -> i64 { print(99); return 0 }\n\
fn records() -> array<Record> { mut builder: array_builder<Record> := array_builder(); builder.push(Record { value: \"x\".clone() }); return builder.build() }\n\
";
    let cases = [
        (
            "index-return",
            "fn run() -> i64 { values := records(); return inspect_with(values[{ return 7 }], later()) }\n",
        ),
        (
            "index-break",
            "fn run() -> i64 { values := records(); return loop { inspect_with(values[{ break 7 }], later()) } }\n",
        ),
        (
            "index-exit",
            "fn run() -> i64 { values := records(); return inspect_with(values[process.exit(7)], later()) }\n",
        ),
        (
            "index-abort",
            "fn run() -> i64 { values := records(); return inspect_with(values[process.abort()], later()) }\n",
        ),
        (
            "index-diverge",
            "fn run() -> i64 { values := records(); return inspect_with(values[loop {}], later()) }\n",
        ),
        (
            "later-return",
            "fn run() -> i64 { values := records(); return inspect_with(values[0], { return 7 }) }\n",
        ),
        (
            "later-break",
            "fn run() -> i64 { values := records(); return loop { inspect_with(values[0], { break 7 }) } }\n",
        ),
        (
            "later-exit",
            "fn run() -> i64 { values := records(); return inspect_with(values[0], { process.exit(7) }) }\n",
        ),
        (
            "later-abort",
            "fn run() -> i64 { values := records(); return inspect_with(values[0], { process.abort() }) }\n",
        ),
        (
            "later-diverge",
            "fn run() -> i64 { values := records(); return inspect_with(values[0], loop {}) }\n",
        ),
    ];
    for (name, body) in cases {
        let source = format!("{prefix}{body}fn main() -> i32 = 0\n");
        let mut source_map = SourceMap::new();
        let checked = check(
            &mut source_map,
            &format!("borrowed-index-termination-{name}"),
            &source,
        );
        assert!(
            !checked.diags.has_errors(),
            "{name}: {}",
            align_driver::format_diagnostics(&source_map, &checked.diags)
        );
        assert!(align_sema::checked_hir_body_facts_are_valid(&checked.hir));
        let mir = align_mir::lower_program_checked(&checked.hir, false, None)
            .expect("terminating indexed-borrow HIR must validate");
        let rendered = align_mir::print::program_to_string(&mir);
        assert!(
            !rendered.contains("borrow-element"),
            "{name} must not retain an element descriptor after termination:\n{rendered}"
        );
    }
}

#[test]
fn copy_view_index_termination_family_forms_no_load_or_result_fact() {
    let prefix = "import std.process\n";
    let cases = [
        (
            "return",
            "fn run(borrow values: array<str>) -> str { return values[{ return \"done\" }] }\n",
        ),
        (
            "break",
            "fn run(borrow values: array<str>) -> str { return loop { values[{ break \"done\" }] } }\n",
        ),
        (
            "exit",
            "fn run(borrow values: array<str>) -> str { return values[process.exit(7)] }\n",
        ),
        (
            "abort",
            "fn run(borrow values: array<str>) -> str { return values[process.abort()] }\n",
        ),
        (
            "diverge",
            "fn run(borrow values: array<str>) -> str { return values[loop {}] }\n",
        ),
    ];
    for (name, body) in cases {
        let source = format!("{prefix}{body}fn main() -> i32 = 0\n");
        let mut source_map = SourceMap::new();
        let checked = check(
            &mut source_map,
            &format!("copy-index-termination-{name}"),
            &source,
        );
        assert!(
            !checked.diags.has_errors(),
            "{name}: {}",
            align_driver::format_diagnostics(&source_map, &checked.diags)
        );
        assert!(align_sema::checked_hir_body_facts_are_valid(&checked.hir));
        let mir = align_mir::lower_program_checked(&checked.hir, false, None)
            .expect("terminating Copy index HIR must validate");
        assert!(
            mir.fns.iter().all(|function| function.blocks.iter().all(|block| {
                block.stmts.iter().all(|statement| {
                    !matches!(
                        statement,
                        align_mir::Stmt::Let(
                            _,
                            align_mir::Rvalue::Index(..) | align_mir::Rvalue::SliceIndex(..)
                        )
                    )
                })
            })),
            "{name} must form no element load or Copy result fact"
        );
    }
}

#[test]
fn copy_view_indices_preserve_direct_field_and_projected_array_roots() {
    let source = "\
Holder { values: array<str> }\n\
View { text: str }\n\
fn retain(value: str, borrow mut destination: View) { destination.text = value }\n\
fn direct(borrow values: array<str>) -> str = values[0]\n\
fn field(borrow holder: Holder) -> str = holder.values[0]\n\
fn projected(borrow values: Option<array<str>>) -> str = match values { Some(items) => items[0] None => \"\" }\n\
fn retain_both(borrow values: array<str>) -> i64 { mut destination := View { text: \"\" }; retain(values[0], destination); f := retain; f(values[0], destination); return destination.text.len() }\n\
fn main() -> i32 = 0\n\
";
    assert!(
        !check_errs("borrowed-copy-index-roots", source),
        "{}",
        check_diagnostics("borrowed-copy-index-roots", source)
    );
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "borrowed-copy-index-roots-hir", source);
    assert!(align_sema::checked_hir_body_facts_are_valid(&checked.hir));

    let invalidated = "\
fn consume(values: array<str>) -> i64 = values.len()\n\
fn first(borrow values: array<str>) -> str = values[0]\n\
fn main(args: array<str>) -> Result<(), Error> { view := first(args); print(consume(args)); print(view); return Ok(()) }\n\
";
    let diagnostics = check_diagnostics("borrowed-copy-index-invalidation", invalidated);
    assert!(
        diagnostics.contains("invalidated") || diagnostics.contains("moved"),
        "a copied view must retain its source array generation:\n{diagnostics}"
    );

    for (name, retain) in [
        ("direct", "retain(args[0], output)"),
        ("indirect", "f := retain; f(args[0], output)"),
    ] {
        let invalidated = format!(
            "View {{ text: str }}\nfn consume(values: array<str>) -> i64 = values.len()\nfn retain(value: str, borrow mut destination: View) {{ destination.text = value }}\nfn main(args: array<str>) -> Result<(), Error> {{ mut output := View {{ text: \"\" }}; {retain}; print(consume(args)); print(output.text); return Ok(()) }}\n"
        );
        let diagnostics = check_diagnostics(
            &format!("borrowed-copy-index-retention-{name}"),
            &invalidated,
        );
        assert!(
            diagnostics.contains("invalidated") || diagnostics.contains("moved"),
            "{name} mutable retention must preserve the indexed array root:\n{diagnostics}"
        );
    }
}

#[test]
fn copy_record_indices_preserve_every_direct_and_nested_view_leaf() {
    let source = "\
Inner { text: str, bytes: slice<u8> }\n\
Views { text: str, bytes: slice<u8>, inner: Inner }\n\
Holder { values: array<Views> }\n\
fn whole(borrow values: array<Views>) -> Views = values[0]\n\
fn direct_text(borrow values: array<Views>) -> str = values[0].text\n\
fn direct_bytes(borrow values: array<Views>) -> slice<u8> = values[0].bytes\n\
fn nested_text(borrow values: array<Views>) -> str = values[0].inner.text\n\
fn nested_bytes(borrow values: array<Views>) -> slice<u8> = values[0].inner.bytes\n\
fn field_text(borrow holder: Holder) -> str = holder.values[0].text\n\
fn projected_bytes(borrow values: Option<array<Views>>) -> slice<u8> = match values { Some(items) => items[0].inner.bytes None => \"\".bytes() }\n\
fn main() -> i32 = 0\n\
";
    assert!(
        !check_errs("borrowed-copy-record-index-roots", source),
        "{}",
        check_diagnostics("borrowed-copy-record-index-roots", source)
    );

    let escape = "\
Inner { text: str, bytes: slice<u8> }\n\
Views { text: str, bytes: slice<u8>, inner: Inner }\n\
fn bad(text: str, bytes: slice<u8>) -> Views { arena out { mut builder: array_builder<Views> := array_builder(out); builder.push(Views { text: text, bytes: bytes, inner: Inner { text: text, bytes: bytes } }); values := builder.build(); return values[0] } }\n\
fn main() -> i32 = 0\n\
";
    assert!(
        check_errs("borrowed-copy-record-index-escape", escape),
        "a copied record must not outlive its source array generation"
    );
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
fn indexed_shared_borrow_matches_imported_whole_and_per_unit_modes() {
    let files = &[
        (
            "records.align",
            "module records\npub Record { value: string }\npub View { text: str }\npub fn inspect(borrow record: Record) -> i64 = record.value.len()\npub fn element_text(borrow record: Record) -> str { text: str := record.value; return text }\npub fn retain_text(borrow record: Record, borrow mut destination: View) { text: str := record.value; destination.text = text }\npub fn make() -> array<Record> { mut builder: array_builder<Record> := array_builder(); builder.push(Record { value: \"forty-two\".clone() }); return builder.build() }\n",
        ),
        (
            "main.align",
            "module main\nimport records\nfn main() -> i32 { values := records.make(); f := records.inspect; view := records.element_text(values[0]); mut output := records.View { text: \"\" }; records.retain_text(values[0], output); if records.inspect(values[0]) == 9 && f(values[0]) == 9 && view.len() == 9 && output.text.len() == 9 && values.len() == 1 { return 42 }; return 0 }\n",
        ),
    ];
    let differential = diff_check_multi("borrow-index-import", files, "main.align");
    assert_eq!(
        differential.whole_errors,
        differential.per_unit_errors,
        "whole: {}\nper-unit: {}",
        differential.whole_diags,
        differential.per_unit_diags
    );
    assert!(
        !differential.whole_errors,
        "whole: {}\nper-unit: {}",
        differential.whole_diags,
        differential.per_unit_diags
    );
    if backend_available() {
        assert_eq!(
            build_and_run_multi("borrow-index-import-whole", files, "main.align")
                .status
                .code(),
            Some(42)
        );
        assert_eq!(
            build_per_unit_multi("borrow-index-import-per-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
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
