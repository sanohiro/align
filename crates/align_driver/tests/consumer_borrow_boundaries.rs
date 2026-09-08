//! Cross-product owners for consumer-reported borrow and producer boundaries.
mod common;
use common::*;

#[test]
fn borrowed_projection_producer_paths_preserve_readable_elements() {
    for element in ["str", "string"] {
        for (carrier, present, absent) in [("Option", "Some", "None"), ("Result", "Ok", "Err(_)")] {
            for clone in [false, true] {
                let input = if carrier == "Option" {
                    format!("Option<array<{element}>>")
                } else {
                    format!("Result<array<{element}>, Error>")
                };
                let (ret, selected, fallback) = if clone {
                    ("string", "items[0].clone()", "\"\".clone()")
                } else {
                    ("str", "items[0]", "\"\"")
                };
                let helper = format!(
                    "module helper\npub fn selected(borrow input: {input}) -> {ret} = match input {{ {present}(items) => {selected}, {absent} => {fallback} }}\n"
                );
                let checked = diff_check_multi(
                    &format!("consumer-projection-{carrier}-{element}-{clone}"),
                    &[
                        ("helper.align", &helper),
                        ("main.align", "import helper\nfn main() {}\n"),
                    ],
                    "main.align",
                );
                assert!(
                    !checked.whole_errors && !checked.per_unit_errors,
                    "{carrier} {element} clone={clone}: {}\n{}",
                    checked.whole_diags,
                    checked.per_unit_diags
                );
            }
        }
    }
}

#[test]
fn derived_view_mutation_preserves_disjoint_owner_facts() {
    let helper = "module helper\npub Holder { name: str, data: buffer, count: i64 }\npub fn put(borrow mut bytes: slice<u8>) { bytes[0] = 66 }\npub fn read(borrow owner: Holder) -> i64 = owner.count\n";
    let main = "import helper\nfn encode(borrow mut owner: helper.Holder) -> i64 { mut bytes := owner.data.bytes(); helper.put(bytes); return helper.read(owner) }\nfn main() {}\n";
    let checked = diff_check_multi(
        "consumer-derived-owner",
        &[("helper.align", helper), ("main.align", main)],
        "main.align",
    );
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
}

#[test]
fn completed_actions_do_not_retain_operand_headers() {
    for prefix in ["if a[0] == 0", "flag := a[0] == 0; if flag"] {
        let source = format!(
            "fn filled() -> array<i64> {{ mut b: array_builder<i64> := array_builder(); b.push(0); return b.build() }}\nfn main() -> i32 {{ mut a := filled(); {prefix} {{ a = filled() }}; return a[0] as i32 }}\n"
        );
        let checked = diff_check_multi(
            "consumer-completed-guard",
            &[("main.align", &source)],
            "main.align",
        );
        assert!(
            !checked.whole_errors && !checked.per_unit_errors,
            "{}\n{}",
            checked.whole_diags,
            checked.per_unit_diags
        );
    }
}

#[test]
fn repaired_boundaries_execute_in_whole_and_unit_builds() {
    let helper = "module helper\npub Holder { value: i64 }\npub fn set(borrow mut owner: Holder, value: i64) { owner.value = value }\npub fn selected(borrow input: Option<array<string>>) -> str = match input { Some(items) => items[0], None => \"none\" }\n";
    let main = "import helper\nfn main() -> i32 { mut b: array_builder<string> := array_builder(); b.push(\"first\".clone()); input := Some(b.build()); if helper.selected(input) != \"first\" { return 1 }; mut owner := helper.Holder { value: 42 }; value := owner.value; helper.set(owner, value); if owner.value != 42 { return 2 }; return 0 }\n";
    let files = [("helper.align", helper), ("main.align", main)];
    if backend_available() {
        for output in [
            build_and_run_multi("consumer-native-whole", &files, "main.align"),
            build_per_unit_multi("consumer-native-units", &files, "main.align").link_and_run(),
        ] {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
